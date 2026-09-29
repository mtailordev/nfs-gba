"""Merge agents' machine-readable notes into the shared symbol list, reporting what needs a human.

Agents write, in docs/engine/notes/, `symbols.<agent>.csv` (the docs/engine/symbols.csv format:
address,name,kind,comment; the comment may hold commas) and `addresses.<agent>.csv` (ordinary CSV with a header:
region,address,size,what,doc; region is rom, ram, or a struct name such as world, entity, driver, profile, level;
the address spelled as the map spells it: ROM file offsets like 0x7F2B08, RAM like 0x03005614). Then:

    python tools/notes_merge.py [NOTES ...]            # default: docs/engine/notes/*.csv; dry run
    python tools/notes_merge.py --write [NOTES ...]    # also write the clean symbol and address rows

Symbols: a row already present (same address and name) is a duplicate; the same address under another name, or
the same name at another address (also between two agents' notes), is a conflict and is never written. Exit
code 1 when there are conflicts. Addresses: rows whose address already appears in docs/engine/address-map.md
are listed for a manual edit; with --write the new ones are inserted into their region's table, sorted by address.
"""
import argparse
import csv
import io
import re
import sys
from pathlib import Path

from common import ROOT

SYMBOLS = ROOT / "docs" / "engine" / "symbols.csv"
ADDRESS_MAP = ROOT / "docs" / "engine" / "address-map.md"
NOTES = ROOT / "docs" / "engine" / "notes"


def norm(addr: str) -> str:
    return f"{int(addr, 16):#010x}"


def read_rows(text: str) -> list[dict]:
    """symbols.csv rows: the last column (comment) may hold unquoted commas, as ApplySymbols.java reads it."""
    lines = [l for l in text.splitlines() if l.strip()]
    keys = lines[0].split(",")
    return [dict(zip(keys, l.split(",", len(keys) - 1))) for l in lines[1:]]


def read_addresses(text: str) -> list[dict]:
    """addresses.<agent>.csv: region,address,size,what,doc. A quoted line is read as CSV; otherwise `what` takes
    every comma between the first three fields and the last one, as agents rarely quote it."""
    keys = ["region", "address", "size", "what", "doc"]
    rows = []
    for line in [l for l in text.splitlines() if l.strip()][1:]:
        if '"' in line:
            rows.append(dict(zip(keys, next(csv.reader(io.StringIO(line))))))
        else:
            region, address, size, rest = line.split(",", 3)
            what, _, doc = rest.rpartition(",")
            rows.append(dict(zip(keys, [region, address, size, what, doc])))
    for r in rows:
        r["address"] = r["address"].strip().strip("`")  # some agents write the map's backticks
    return rows


def merge_symbols(base: list[dict], notes: dict[str, list[dict]]):
    """-> (new rows, duplicates, conflicts). `notes` maps a source name to its rows."""
    by_addr = {norm(r["address"]): ("symbols.csv", r) for r in base}
    by_name = {r["name"]: ("symbols.csv", r) for r in base}
    new, dups, conflicts = [], [], []
    for source, rows in notes.items():
        for r in rows:
            a = norm(r["address"])
            r = {**r, "address": a}
            seen_a, seen_n = by_addr.get(a), by_name.get(r["name"])
            if seen_a and seen_a[1]["name"] == r["name"]:
                dups.append((source, r, seen_a[0]))
            elif seen_a or (seen_n and norm(seen_n[1]["address"]) != a):
                other = seen_a or seen_n
                conflicts.append((source, r, other[0], other[1]))
            else:
                new.append(r)
                by_addr[a] = by_name[r["name"]] = (source, r)
    return new, dups, conflicts


# Region -> the address map heading its table sits under.
SECTIONS = {"rom": "## ROM", "level": "### Level descriptor", "ram": "## RAM", "io": "### I/O registers",
            "world": "### World struct", "entity": "### Entity", "driver": "### Driver", "profile": "### Profile"}
KEY = re.compile(r"^\| `\+?(0x[0-9a-fA-F]+)")


def table_lines(lines: list[str], region: str) -> range:
    """Line numbers of the region's table (header and separator included)."""
    start = next(i for i, l in enumerate(lines) if l.startswith(SECTIONS[region.lower()]))
    first = next(i for i in range(start + 1, len(lines)) if lines[i].startswith("|"))
    end = first
    while end + 1 < len(lines) and lines[end + 1].startswith("|"):
        end += 1
    return range(first, end + 1)


def in_table(text: str, r: dict) -> bool:
    """Whether the region's table already has a row for this address (in its first cell, where a row may list
    several addresses)."""
    lines = text.split("\n")
    want = f"`{r['address'].lower()}`"
    return any(want in lines[i].lower().split("|")[1] for i in table_lines(lines, r["region"]) if "|" in lines[i][1:])


def insert_addresses(text: str, rows: list[dict]) -> str:
    """Inserts address rows into their region's table in the map `text`, after the last row with a smaller or
    equal address (rows without a leading hex address are skipped when comparing). The row takes the table's shape:
    `| address | size | what | doc |` in the ROM table, `| address | | what |` in the level descriptor's,
    `| address | what (size) |` in the others."""
    lines = text.split("\n")
    for r in rows:
        table = list(table_lines(lines, r["region"]))
        cols = lines[table[0]].count("|") - 1
        addr = f"`{r['address']}`"
        if cols >= 4:
            row = f"| {addr} | {r['size']} | {r['what']} | {r['doc']} |"
        elif cols == 3:
            row = f"| {addr} | | {r['what']} |"
        else:
            size = r["size"].strip()
            row = f"| {addr} | {r['what']}{f' ({size})' if size else ''} |"
        key = int(re.search(r"0x[0-9a-fA-F]+", r["address"])[0], 16)  # the first address of a multi-address cell
        body = table[2:]  # after the header and the |---| line
        before = [i for i in body if (m := KEY.match(lines[i])) and int(m[1], 16) <= key]
        lines.insert(before[-1] + 1 if before else body[0], row)
    return "\n".join(lines)


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("notes", nargs="*", type=Path)
    ap.add_argument("--write", action="store_true")
    args = ap.parse_args(argv)
    files = args.notes or sorted(NOTES.glob("*.csv"))
    base_text = SYMBOLS.read_text(encoding="utf-8")
    sym_notes = {f.name: read_rows(f.read_text(encoding="utf-8")) for f in files if f.name.startswith("symbols.")}
    addr_notes = {f.name: read_addresses(f.read_text(encoding="utf-8")) for f in files
                  if f.name.startswith("addresses.")}

    new, dups, conflicts = merge_symbols(read_rows(base_text), sym_notes)
    for source, r, where in dups:
        print(f"duplicate  {source}: {r['address']} {r['name']} (already in {where})")
    for source, r, where, other in conflicts:
        print(f"CONFLICT   {source}: {r['address']} {r['name']}  vs  {where}: {norm(other['address'])} {other['name']}")
    print(f"symbols: {len(new)} new, {len(dups)} duplicate, {len(conflicts)} conflicting")
    lines = [",".join([r["address"], r["name"], r["kind"], r["comment"]]) for r in new]
    for line in lines:
        print(f"  + {line}")
    if args.write and lines:
        SYMBOLS.write_text(base_text.rstrip("\n") + "\n" + "\n".join(lines) + "\n", encoding="utf-8", newline="\n")
        print(f"appended {len(lines)} rows to {SYMBOLS.relative_to(ROOT)}")

    map_text = ADDRESS_MAP.read_text(encoding="utf-8").replace("\r\n", "\n")
    fresh = []
    for source, rows in addr_notes.items():
        for r in rows:
            if r["region"].lower() not in SECTIONS:
                print(f"UNKNOWN REGION {r['region']!r} in {source}: {r['address']} (regions: {', '.join(SECTIONS)})")
                continue
            state = "EDIT (already in the map)" if in_table(map_text, r) else "new"  # the map's own spelling
            print(f"address {state:26} {source}: | `{r['address']}` | {r['size']} | {r['what']} | {r['doc']} |  "
                  f"({r['region']})")
            if state == "new":
                fresh.append(r)
    if args.write and fresh:
        ADDRESS_MAP.write_text(insert_addresses(map_text, fresh), encoding="utf-8", newline="\n")
        print(f"inserted {len(fresh)} rows into {ADDRESS_MAP.relative_to(ROOT)}; edit the EDIT rows by hand")
    return 1 if conflicts else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
