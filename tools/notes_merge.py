"""Merge agents' machine-readable notes into the shared symbol list, reporting what needs a human.

Agents write, in docs/engine/notes/, `symbols.<agent>.csv` (the docs/engine/symbols.csv format:
address,name,kind,comment; the comment may hold commas) and `addresses.<agent>.csv` (ordinary CSV with a header:
region,address,size,what,doc; region is rom, ram, or a struct name such as world, entity, driver, profile, level;
the address spelled as the map spells it: ROM file offsets like 0x7F2B08, RAM like 0x03005614). Then:

    python tools/notes_merge.py [NOTES ...]            # default: docs/engine/notes/*.csv; dry run
    python tools/notes_merge.py --write [NOTES ...]    # also append the clean symbol rows to symbols.csv

Symbols: a row already present (same address and name) is a duplicate; the same address under another name, or
the same name at another address (also between two agents' notes), is a conflict and is never written. Exit
code 1 when there are conflicts. Addresses: rows whose address already appears in docs/engine/address-map.md
are listed for a manual edit; the rest are printed as markdown rows to paste (the map is edited by hand).
"""
import argparse
import csv
import io
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
    """addresses.<agent>.csv: ordinary CSV (quote fields that hold commas)."""
    return list(csv.DictReader(io.StringIO(text)))


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

    known = ADDRESS_MAP.read_text(encoding="utf-8").lower()
    for source, rows in addr_notes.items():
        for r in rows:
            a = r["address"].lower()
            state = "EDIT (already in the map)" if f"`{a}" in known else "new"  # the map's own spelling
            print(f"address {state:26} {source}: | `{r['address']}` | {r['size']} | {r['what']} | {r['doc']} |  "
                  f"({r['region']})")
    return 1 if conflicts else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
