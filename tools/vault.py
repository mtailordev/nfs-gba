"""Task 1: inventory dumps/*.zip, hash and check every .gba, build the read-only vault.

Reads ROMs straight out of the zips (nothing is extracted into dumps/). Writes
  $NFSGBA_DATA/vault/roms/<GAMECODE>_v<ver>_<sha1-8>.gba   (never overwritten, set read-only)
  $NFSGBA_DATA/vault/manifest.json
  docs/recon/ROM-INVENTORY.md
Rerunning changes nothing.

    python tools/vault.py
"""
import hashlib
import io
import json
import os
import re
import stat
import sys
import zipfile
import zlib
from pathlib import Path

from common import ROOT, data_dir, hashes, provenance, write_if_changed

# Last character of the game code (GBATEK, "Cartridge Header").
REGIONS = {"J": "Japan", "E": "USA/English", "P": "Europe", "D": "German", "F": "French", "I": "Italian", "S": "Spanish"}


def parse_header(rom: bytes) -> dict:
    """GBA cartridge header fields per GBATEK."""
    chk = -(sum(rom[0xA0:0xBD]) + 0x19) & 0xFF
    code = rom[0xAC:0xB0].decode("ascii", "replace")
    return {
        "title": rom[0xA0:0xAC].rstrip(b"\0").decode("ascii", "replace"),
        "game_code": code,
        "region": REGIONS.get(code[3:], "unknown"),
        "maker_code": rom[0xB0:0xB2].decode("ascii", "replace"),
        "fixed_96h_ok": rom[0xB2] == 0x96,
        "version": rom[0xBC],
        "header_checksum": f"{rom[0xBD]:02x}",
        "header_checksum_ok": rom[0xBD] == chk,
        "logo_crc32": f"{zlib.crc32(rom[0x04:0xA0]):08x}",
        "entry_word": f"{int.from_bytes(rom[0:4], 'little'):08x}",
    }


def trailing_padding(rom: bytes) -> dict:
    """Trailing run of the ROM's last byte value."""
    return {"byte": f"{rom[-1]:02x}", "length": len(rom) - len(rom.rstrip(rom[-1:]))}


def diff(a: bytes, b: bytes, gap: int = 16) -> tuple[int, list]:
    """Differing byte count and [start, end) ranges, merged across gaps <= gap. Equal lengths only."""
    count, ranges = 0, []
    for base in range(0, len(a), 4096):
        ca, cb = a[base:base + 4096], b[base:base + 4096]
        if ca == cb:
            continue
        for i, (x, y) in enumerate(zip(ca, cb)):
            if x != y:
                count += 1
                off = base + i
                if ranges and off - ranges[-1][1] <= gap:
                    ranges[-1][1] = off + 1
                else:
                    ranges.append([off, off + 1])
    return count, ranges


def game_name(zip_name: str) -> str:
    return re.sub(r"\s*\([^)]*\)", "", zip_name.removesuffix(".zip")).strip()


def run(dumps: Path, data: Path, docs: Path) -> dict:
    roms_dir = data / "vault" / "roms"
    roms_dir.mkdir(parents=True, exist_ok=True)
    zips, roms, blobs = [], {}, {}
    for zp in sorted(dumps.glob("*.zip")):
        raw = zp.read_bytes()
        entry = {"zip": zp.name, "zip_sha1": hashlib.sha1(raw).hexdigest(), "members": []}
        with zipfile.ZipFile(io.BytesIO(raw)) as zf:
            for info in zf.infolist():
                m = {"name": info.filename, "size": info.file_size, "zip_crc32": f"{info.CRC:08x}"}
                entry["members"].append(m)
                if not info.filename.lower().endswith(".gba"):
                    continue
                rom = zf.read(info)
                h = hashes(rom)
                m.update(sha1=h["sha1"], crc32_matches_zip=h["crc32"] == m["zip_crc32"])
                blobs[h["sha1"]] = rom
                r = roms.setdefault(h["sha1"], {**h, "size": len(rom), "sources": []})
                r["sources"].append({"zip": zp.name, "member": info.filename, "crc32_matches_zip": m["crc32_matches_zip"]})
        zips.append(entry)

    for sha1, r in roms.items():
        rom = blobs[sha1]
        hdr = parse_header(rom)
        r.update(game=game_name(r["sources"][0]["zip"]), header=hdr,
                 size_power_of_two=len(rom) & (len(rom) - 1) == 0, trailing_padding=trailing_padding(rom))
        r["valid"] = all(s["crc32_matches_zip"] for s in r["sources"]) and hdr["header_checksum_ok"] and hdr["fixed_96h_ok"]
        if not r["valid"]:
            r["vault_file"] = None  # reported, not vaulted
            continue
        code = "".join(c for c in hdr["game_code"] if c.isalnum()) or "XXXX"
        path = roms_dir / f"{code}_v{hdr['version']}_{sha1[:8]}.gba"
        if not path.exists():
            path.write_bytes(rom)
            path.chmod(stat.S_IREAD)
        if hashlib.sha1(path.read_bytes()).hexdigest() != sha1:
            sys.exit(f"vault file {path} does not match sha1 {sha1}; not touching it, investigate")
        r["vault_file"] = path.relative_to(data).as_posix()
        r["read_only"] = not os.access(path, os.W_OK)

    # Same game, different dump: game codes share the first three characters.
    comparisons = []
    by_game = {}
    for sha1, r in roms.items():
        by_game.setdefault(r["header"]["game_code"][:3], []).append(sha1)
    for group in by_game.values():
        for i, a in enumerate(group):
            for b in group[i + 1:]:
                ha, hb = roms[a]["header"], roms[b]["header"]
                c = {"a": a, "b": b, "header_diffs": {k: [ha[k], hb[k]] for k in ha if ha[k] != hb[k]}}
                if len(blobs[a]) == len(blobs[b]):
                    n, ranges = diff(blobs[a], blobs[b])
                    c.update(differing_bytes=n, ranges=len(ranges),
                             first_ranges=[f"{s:06x}-{e:06x}" for s, e in ranges[:40]])
                comparisons.append(c)

    carbon = sorted({s for s, r in roms.items() if any("carbon" in x["zip"].lower() for x in r["sources"])})
    manifest = {
        "provenance": provenance(__file__),
        "canonical_target": carbon[0] if len(carbon) == 1 else None,  # None = Carbon dumps differ: ask the user
        "zips": zips,
        "roms": sorted(roms.values(), key=lambda r: (r["game"], r["header"]["game_code"])),
        "comparisons": comparisons,
    }
    write_if_changed(data / "vault" / "manifest.json", json.dumps(manifest, indent=2) + "\n")
    write_if_changed(docs / "recon" / "ROM-INVENTORY.md", inventory_md(manifest))
    return manifest


def inventory_md(m: dict) -> str:
    short = {r["sha1"]: f"{r['header']['game_code']} `{r['sha1'][:8]}`" for r in m["roms"]}
    p = m["provenance"]
    out = [
        "# ROM inventory", "",
        f"Generated by `{p['tool']}` (sha1 `{p['tool_sha1'][:12]}`, Python {p['python']}) from the zips in `dumps/`. "
        "Rerun the tool instead of editing this file. Vault paths are relative to `$NFSGBA_DATA`.", "",
        "## ROMs", "",
        "| Game | Code | Region | Title | Maker | Ver | Size | Header chk | Padding | Vault file | Verdict |",
        "|---|---|---|---|---|---|---|---|---|---|---|",
    ]
    for r in m["roms"]:
        h, pad = r["header"], r["trailing_padding"]
        verdict = ["valid" if r["valid"] else "**INVALID**"]
        if len(r["sources"]) > 1:
            verdict.append(f"identical in {len(r['sources'])} zips")
        if r["sha1"] == m["canonical_target"]:
            verdict.append("**canonical target**")
        size = f"{r['size'] // 2**20} MiB" + ("" if r["size_power_of_two"] else " (not 2^n)")
        out.append(f"| {r['game']} | `{h['game_code']}` | {h['region']} | `{h['title']}` | `{h['maker_code']}` | {h['version']} "
                   f"| {size} | {h['header_checksum']} {'ok' if h['header_checksum_ok'] else 'BAD'} "
                   f"| {pad['length']:,} × `{pad['byte']}` | `{r['vault_file']}` | {', '.join(verdict)} |")
    out += ["", "## Hashes", "", "| Code | CRC32 | MD5 | SHA-1 |", "|---|---|---|---|"]
    out += [f"| `{r['header']['game_code']}` | `{r['crc32']}` | `{r['md5']}` | `{r['sha1']}` |" for r in m["roms"]]
    out += ["", "## Source zips", "", "| Zip | Member | Size | Zip CRC32 | CRC matches | ROM |", "|---|---|---|---|---|---|"]
    for z in m["zips"]:
        for mem in z["members"]:
            ok = {True: "yes", False: "**NO**", None: "n/a"}[mem.get("crc32_matches_zip")]
            out.append(f"| {z['zip']} | {mem['name']} | {mem['size']:,} | `{mem['zip_crc32']}` | {ok} | {short.get(mem.get('sha1'), 'not a ROM')} |")
    out += ["", "## Same-game comparisons", ""]
    if not m["comparisons"]:
        out.append("None: every game code has a single distinct ROM.")
    for c in m["comparisons"]:
        out.append(f"- {short[c['a']]} vs {short[c['b']]}: header differs in "
                   f"{', '.join(f'{k} ({a} → {b})' for k, (a, b) in c['header_diffs'].items()) or 'nothing'}; "
                   + (f"{c['differing_bytes']:,} bytes differ in {c['ranges']} ranges "
                      f"(first: {', '.join(c['first_ranges'][:8])})." if "ranges" in c else "sizes differ, no byte diff."))
    out += ["", "## Canonical target", ""]
    t = m["canonical_target"]
    out.append(f"{short[t]}: the only distinct Carbon ROM, so it is the target (AGENTS.md scope decision)." if t
               else "**Undecided**: the Carbon zips hold different ROMs. Ask the user which one is Carbon Europe.")
    return "\n".join(out) + "\n"


if __name__ == "__main__":
    man = run(ROOT / "dumps", data_dir(), ROOT / "docs")
    for r in man["roms"]:
        print(f"{r['header']['game_code']}  {r['sha1']}  valid={r['valid']}  {r['vault_file']}")
    print("canonical:", man["canonical_target"])
