"""Task 1, step 6: shallow first look at the vaulted ROMs.

Canonical Carbon ROM: block map, entropy map, strings, pointer tables, BIOS-compression
candidates with test decompression, audio engine. Siblings (one ROM per other game): block
map, audio engine and code/data similarity only.

Writes $NFSGBA_DATA/out/first-look/ (report.json, per-ROM listings) and
docs/recon/FIRST-LOOK-DATA.md. Rerunning changes nothing.

    python tools/first_look.py
"""
import collections
import json
import math
import re
import zlib
from pathlib import Path

from common import ROOT, data_dir, provenance, write_if_changed

BLOCK = 16384
ROM_BASE = 0x08000000
RETURNS = {0x4770, 0x4700, 0x4708, 0x4710, 0x4718}  # Thumb bx lr, bx r0..r3
# MP2K/M4A SelectSong prologue (signature used by saptapper-style rippers). A hit is conclusive, a miss is not.
MP2K_SELECTSONG = bytes.fromhex("00b5 0004 074a 0849 400b 4018 8388 5900 c918 8900 8918 0a68 0168 101c 00f0")
SOUND_AREA = (0x03007FF0).to_bytes(4, "little")  # BIOS sound-area pointer every MP2K build references
STRING = re.compile(rb"[\x20-\x7e]{6,}")
IDENT = re.compile(r"^[A-Z][A-Z0-9]*(_[A-Z0-9]+)+$")
CREDIT = re.compile(r"pocketeers|logik|electronic arts|licen[cs]e|trademark|copyright|\(c\)", re.I)
LEGEND = ("`.` constant fill, `A` ARM code (>35% of words have the AL condition), `T` Thumb code "
          "(>=1.5 returns and >=1.5 `push {..lr}` per KiB), `t` weak Thumb signs (>=0.5 returns per KiB), "
          "`L` at least half covered by LZ77 blobs, `S` smooth bytes centred on 0 (signed PCM audio), "
          "`i` many equal neighbouring bytes (pixel data), `Z` entropy >=7.5 bits/byte, `d` other data")
CODE, THUMB, DATA = set("TtA"), set("Tt"), set("LSiZd")


def entropy(counts: collections.Counter) -> float:
    n = sum(counts.values())
    return -sum(c / n * math.log2(c / n) for c in counts.values())


def signal_class(b: bytes, ent: float) -> str:
    """Data block: S if smooth and centred on 0 (neighbour differences more predictable than the bytes),
    i if many neighbours are equal, else Z/d by entropy. Samples every third byte pair."""
    diffs = collections.Counter((y - x) & 0xFF for x, y in zip(b[::3], b[1::3]))
    near0 = sum(1 for x in b[::3] if x < 32 or x >= 224) / len(b[::3])
    if near0 > 0.55 and entropy(diffs) < ent - 0.3:
        return "S"
    if diffs[0] / sum(diffs.values()) > 0.25:
        return "i"
    return "Z" if ent >= 7.5 else "d"


def blocks(rom: bytes) -> list[tuple[float, str]]:
    """Entropy and rough class (see LEGEND, minus `L`) of every 16 KiB block."""
    out = []
    for off in range(0, len(rom), BLOCK):
        b = rom[off:off + BLOCK]
        counts = collections.Counter(b)
        ent = entropy(counts)
        hw, w, kib = memoryview(b).cast("H"), memoryview(b).cast("I"), len(b) / 1024
        rets = sum(1 for x in hw if x in RETURNS) / kib
        push = sum(1 for x in hw if x >> 8 == 0xB5) / kib
        arm = sum(1 for x in w if x >> 28 == 0xE) / len(w)
        cls = ("." if len(counts) == 1 else "A" if arm > 0.35 else "T" if rets >= 1.5 and push >= 1.5
               else "t" if rets >= 0.5 else signal_class(b, ent))
        out.append((ent, cls))
    return out


def mark_lz77(cls: str, blobs: list) -> str:
    """Relabel data blocks at least half covered by LZ77 blobs as `L`."""
    cover = collections.Counter()
    for o, _, packed in blobs:
        s, e = o, o + packed
        while s < e:
            nxt = min(e, (s // BLOCK + 1) * BLOCK)
            cover[s // BLOCK] += nxt - s
            s = nxt
    return "".join("L" if c in DATA and cover[i] >= BLOCK // 2 else c for i, c in enumerate(cls))


def map_rows(chars: str, per: int = 64) -> list[str]:
    return [f"{i * BLOCK:07x} {chars[i:i + per]}" for i in range(0, len(chars), per)]


def pointers(rom: bytes, min_run: int = 8):
    """Aligned words that point into the ROM: runs of >= min_run of them, and the set of targets."""
    w, hi = memoryview(rom).cast("I"), ROM_BASE + len(rom)
    runs, start, targets = [], None, set()
    for i, v in enumerate(w):
        if ROM_BASE <= v < hi:
            targets.add(v - ROM_BASE)
            if start is None:
                start = i
        else:
            if start is not None and i - start >= min_run:
                runs.append((start * 4, i - start, [x - ROM_BASE for x in w[start:i]]))
            start = None
    if start is not None and len(w) - start >= min_run:
        runs.append((start * 4, len(w) - start, [x - ROM_BASE for x in w[start:]]))
    return runs, targets


# GBA BIOS decompressors (GBATEK "BIOS Decompression Functions"). Each returns bytes consumed; raises on bad data.
def lz77(rom: bytes, off: int, size: int) -> int:
    out, i = bytearray(), off + 4
    while len(out) < size:
        flags = rom[i]
        i += 1
        for bit in range(8):
            if len(out) >= size:
                break
            if flags & (0x80 >> bit):
                n, disp = (rom[i] >> 4) + 3, ((rom[i] & 0xF) << 8 | rom[i + 1]) + 1
                i += 2
                if disp > len(out):
                    raise ValueError("back-reference before start")
                for _ in range(n):
                    out.append(out[-disp])
            else:
                out.append(rom[i])
                i += 1
    return i - off


def rle(rom: bytes, off: int, size: int) -> int:
    n_out, i = 0, off + 4
    while n_out < size:
        flag = rom[i]
        n = (flag & 0x7F) + (3 if flag & 0x80 else 1)
        i += 2 if flag & 0x80 else 1 + n
        n_out += n
    if i > len(rom):
        raise ValueError("ran past end")
    return i - off


def huffman(rom: bytes, off: int, size: int) -> int:
    bits = rom[off] & 0xF
    tree, units, need = off + 4, 0, size * 8 // bits
    tree_end = tree + (rom[tree] + 1) * 2
    root = pos = tree + 1
    i = tree_end
    while units < need:
        word = int.from_bytes(rom[i:i + 4], "little")
        if i + 4 > len(rom):
            raise ValueError("ran past end")
        i += 4
        for b in range(31, -1, -1):
            bit = word >> b & 1
            child = (pos & ~1) + (rom[pos] & 0x3F) * 2 + 2 + bit
            if not tree < child < tree_end:
                raise ValueError("tree node out of range")
            if rom[pos] & (0x80 >> bit):
                units += 1
                pos = root
                if units >= need:
                    break
            else:
                pos = child
    return i - off


DECODERS = {0x10: ("lz77", lz77), 0x24: ("huff4", huffman), 0x28: ("huff8", huffman), 0x30: ("rle", rle)}


def bios_candidates(rom: bytes, targets: set) -> list[dict]:
    """Pointer targets that start with a BIOS compression header of plausible size, test-decompressed."""
    found = []
    for t in sorted(targets):
        if t % 4 or t + 8 > len(rom) or rom[t] not in DECODERS:
            continue
        kind, fn = DECODERS[rom[t]]
        size = int.from_bytes(rom[t + 1:t + 4], "little")
        if not 16 <= size <= 0x40000:  # larger than EWRAM is not plausible
            continue
        try:
            packed = fn(rom, t, size)
        except (IndexError, ValueError):
            packed = None
        found.append({"offset": t, "kind": kind, "size": size, "packed": packed})
    return found


def lz77_blobs(rom: bytes) -> list[tuple[int, int, int]]:
    """Every 4-aligned LZ77 header (16 B-256 KiB) that decodes cleanly: (offset, unpacked, packed).
    Random bytes almost never survive a full decode, so hits are real. Carbon's size fields claim 8 bytes
    more than the stream encodes, so a decode overreads into the next blob: overlaps up to 8 bytes are allowed."""
    found, end = [], -1
    for o in range(0, len(rom) - 8, 4):
        if rom[o] != 0x10 or o < end - 8:
            continue
        size = int.from_bytes(rom[o + 1:o + 4], "little")
        if not 16 <= size <= 0x40000:
            continue
        try:
            packed = lz77(rom, o, size)
        except (IndexError, ValueError):
            continue
        found.append((o, size, packed))
        end = o + packed
    return found


def lz77_summary(blobs: list) -> dict:
    return {"blobs": len(blobs), "unpacked": sum(s for _, s, _ in blobs), "packed": sum(p for _, _, p in blobs),
            "top_sizes": collections.Counter(s for _, s, _ in blobs).most_common(8)}


def audio(rom: bytes) -> dict:
    return {
        "logik_state": sorted({m.group().decode() for m in re.finditer(rb"\w+ \(C\) Logik State \d{4}", rom)}),
        "gbamod_modules": rom.count(b"GBAMOD"),
        "mp2k_selectsong": rom.count(MP2K_SELECTSONG),
        "sound_area_literals": rom.count(SOUND_AREA),
    }


def normalize(rom: bytes, cls: str) -> bytes:
    """Zero the relocation-dependent bits in code blocks so the same code linked elsewhere still matches:
    literal-pool addresses (EWRAM..ROM keep only their region byte), ARM B/BL offsets, Thumb BL offsets."""
    b = bytearray(rom)
    hw, w = memoryview(b).cast("H"), memoryview(b).cast("I")
    for bi, c in enumerate(cls):
        if c in CODE:
            for i in range(bi * BLOCK // 4, (bi + 1) * BLOCK // 4):
                if 0x02000000 <= w[i] < 0x0A000000 or w[i] >> 25 == 0x75:  # 0xEA/0xEB = ARM B/BL
                    w[i] &= 0xFF000000
            for i in range(bi * BLOCK // 2, (bi + 1) * BLOCK // 2 - 1):
                if 0xF000 <= hw[i] < 0xF800 and hw[i + 1] >= 0xF800:
                    hw[i], hw[i + 1] = 0xF000, 0xF800
    hw.release()
    w.release()
    return bytes(b)


def fingerprints(rom: bytes, cls: str, which: set, step: int) -> set:
    """Content-defined sample (crc32 % 8 == 0) of varied 32-byte windows inside blocks of the given classes."""
    fp = set()
    for bi, c in enumerate(cls):
        if c in which:
            for i in range(bi * BLOCK, (bi + 1) * BLOCK, step):
                win = rom[i:i + 32]
                if zlib.crc32(win) & 7 == 0 and len(set(win)) >= 12:
                    fp.add(win)
    return fp


def analyse(rom: bytes, full: bool) -> dict:
    blk = blocks(rom)
    blobs = lz77_blobs(rom)
    cls = mark_lz77("".join(c for _, c in blk), blobs)
    r = {"class_map": cls, "audio": audio(rom), "lz77": lz77_summary(blobs), "lz77_blobs": blobs,
         "kib": {k: cls.count(k) * BLOCK // 1024 for k in "TtALSiZd."}}
    if full:
        r["entropy_map"] = "".join(str(min(int(e), 7)) if c != "." else "." for e, c in blk)
        strings = [(m.start(), m.group().decode()) for m in STRING.finditer(rom)]
        runs, targets = pointers(rom)
        r.update(strings=strings, runs=runs, n_targets=len(targets),
                 compression=bios_candidates(rom, targets))
    return r


def pick_roms(manifest: dict) -> list[dict]:
    """Canonical ROM first, then one ROM per other game (Europe build preferred)."""
    canon = next(x for x in manifest["roms"] if x["sha1"] == manifest["canonical_target"])
    by_game = {}
    for x in sorted(manifest["roms"], key=lambda x: x["header"]["game_code"][3] != "P"):
        if x["valid"] and x["sha1"] != canon["sha1"]:
            by_game.setdefault(x["header"]["game_code"][:3], x)
    return [canon] + sorted(by_game.values(), key=lambda x: x["game"])


def run(data: Path, docs: Path) -> dict:
    manifest = json.loads((data / "vault" / "manifest.json").read_text(encoding="utf-8"))
    out_dir = data / "out" / "first-look"
    games = []
    for i, m in enumerate(pick_roms(manifest)):
        rom = (data / m["vault_file"]).read_bytes()
        a = analyse(rom, full=i == 0)
        norm = normalize(rom, a["class_map"])
        a.update(code=m["header"]["game_code"], sha1=m["sha1"], game=m["game"],
                 fp_thumb=fingerprints(norm, a["class_map"], THUMB, 2),
                 fp_arm=fingerprints(norm, a["class_map"], {"A"}, 4),
                 fp_data=fingerprints(rom, a["class_map"], DATA, 4))
        games.append(a)

    def contain(key):  # [a][b]: share of a's sampled windows also found in b
        return {a["code"]: {b["code"]: round(100 * len(a[key] & b[key]) / max(1, len(a[key])), 1) for b in games}
                for a in games}

    canon = games[0]
    sub = out_dir / canon["sha1"][:8]
    write_if_changed(sub / "strings.txt", "".join(f"{o:07x} {s}\n" for o, s in canon["strings"]))
    write_if_changed(sub / "pointer-runs.txt", "".join(
        f"{o:07x} n={n} targets={' '.join(f'{t:07x}' for t in ts)}\n" for o, n, ts in canon["runs"]))
    write_if_changed(sub / "compression.txt", "".join(
        f"{c['offset']:07x} {c['kind']} size={c['size']} packed={c['packed']}\n" for c in canon["compression"]))
    write_if_changed(sub / "lz77.txt", "".join(f"{o:07x} unpacked={s} packed={p}\n" for o, s, p in canon["lz77_blobs"]))
    report = {
        "provenance": provenance(__file__),
        "roms": [{k: g[k] for k in ("code", "sha1", "game", "class_map", "kib", "audio", "lz77")} for g in games],
        "similarity_thumb": contain("fp_thumb"), "similarity_arm": contain("fp_arm"),
        "similarity_data": contain("fp_data"),
        "canonical": {"entropy_map": canon["entropy_map"], "strings": len(canon["strings"]),
                      "pointer_runs": len(canon["runs"]), "pointer_targets": canon["n_targets"],
                      "compression": canon["compression"]},
    }
    write_if_changed(out_dir / "report.json", json.dumps(report, indent=1) + "\n")
    write_if_changed(docs / "recon" / "FIRST-LOOK-DATA.md", data_md(report, canon))
    return report


def data_md(rep: dict, canon: dict) -> str:
    p = rep["provenance"]
    c = rep["canonical"]
    L = ["# First look: generated data", "",
         f"Generated by `{p['tool']}` (sha1 `{p['tool_sha1'][:12]}`, Python {p['python']}). Rerun the tool instead of "
         "editing this file. Interpretation lives in [FIRST-LOOK.md](FIRST-LOOK.md). Offsets are ROM offsets "
         "(address minus 0x08000000). Maps have one character per 16 KiB block, one row per MiB.", "",
         f"## Canonical ROM `{canon['code']}` `{canon['sha1'][:8]}`", "",
         "### Block classes", "", "Legend: " + LEGEND + ".", "", "```", *map_rows(canon["class_map"]), "```", "",
         "### Entropy (bits per byte, floored; `.` constant fill)", "", "```", *map_rows(c["entropy_map"]), "```", ""]
    k = rep["roms"][0]["kib"]
    total = sum(k.values())
    L += ["### Rough split by class", "", "| Class | KiB | Share |", "|---|---|---|"]
    names = {"T": "Thumb code", "t": "weak Thumb signs", "A": "ARM code", "L": "LZ77 blobs", "S": "PCM-like audio",
             "i": "pixel-like", "Z": "high entropy", "d": "other data", ".": "fill"}
    L += [f"| {names[x]} (`{x}`) | {k[x]:,} | {100 * k[x] / total:.1f}% |" for x in "TtALSiZd."]
    strings = canon["strings"]
    idents = [s for _, s in strings if IDENT.match(s)]
    credits = [(o, s) for o, s in strings if CREDIT.search(s)]
    L += ["", "### Strings", "",
          f"{len(strings):,} printable ASCII runs of 6+ bytes (full list in `$NFSGBA_DATA/out/first-look/{canon['sha1'][:8]}/strings.txt`). "
          f"{len(idents):,} look like `UPPER_SNAKE` identifiers (resource or text keys).", "",
          "Credit and licence lines:", ""]
    L += [f"- `{o:07x}` {s[:120]}" for o, s in credits[:30]]
    L += ["", "First 30 identifier-like strings:", "", "```", *[s[:80] for s in idents[:30]], "```", ""]
    L += ["### Pointer tables", "",
          f"{c['pointer_targets']:,} distinct ROM addresses are referenced by aligned words; "
          f"{c['pointer_runs']:,} runs of 8+ consecutive ROM pointers. Longest 15:", "",
          "| Offset | Entries | Targets span |", "|---|---|---|"]
    for o, n, ts in sorted(canon["runs"], key=lambda r: -r[1])[:15]:
        L.append(f"| `{o:07x}` | {n} | `{min(ts):07x}`–`{max(ts):07x}` |")
    lz = rep["roms"][0]["lz77"]
    blobs = canon["lz77_blobs"]
    runs, cur = [], [blobs[0]] if blobs else []
    for b in blobs[1:]:
        if b[0] - (cur[-1][0] + cur[-1][2]) <= 8:
            cur.append(b)
        else:
            runs.append(cur)
            cur = [b]
    runs += [cur] if cur else []
    L += ["", "### LZ77 blobs (brute-force scan)", "",
          f"Every 4-aligned LZ77 header that decodes cleanly, 8-byte overlap allowed (see `lz77_blobs()`): "
          f"**{lz['blobs']} blobs, {lz['unpacked'] / 1024:,.0f} KiB unpacked from {lz['packed'] / 1024:,.0f} KiB**. "
          f"Full list in `$NFSGBA_DATA/out/first-look/{canon['sha1'][:8]}/lz77.txt`.", "",
          "| Unpacked size | Count |", "|---|---|"]
    L += [f"| {s:,} | {n} |" for s, n in lz["top_sizes"]]
    L += ["", "Contiguous banks (4+ blobs):", "", "| Range | Blobs | Sizes |", "|---|---|---|"]
    L += [f"| `{r[0][0]:07x}`–`{r[-1][0] + r[-1][2]:07x}` | {len(r)} | {', '.join(f'{s:,}' for s in sorted({x[1] for x in r})[:6])} |"
          for r in runs if len(r) >= 4]
    comp = c["compression"]
    L += ["", "### BIOS-compression headers at pointer targets", "",
          "Pointer targets that are 4-aligned, start with a BIOS header byte and declare 16 B–256 KiB, then test-decompressed. "
          "Most data is addressed by offsets, not absolute pointers, so this misses most blobs (see above).", "",
          "| Kind | Candidates | Decoded OK | Unpacked KiB (OK) | Packed KiB (OK) |", "|---|---|---|---|---|"]
    for kind in ("lz77", "huff4", "huff8", "rle"):
        cs = [x for x in comp if x["kind"] == kind]
        ok = [x for x in cs if x["packed"]]
        L.append(f"| {kind} | {len(cs)} | {len(ok)} | {sum(x['size'] for x in ok) / 1024:,.0f} | {sum(x['packed'] for x in ok) / 1024:,.0f} |")
    ok = [x for x in comp if x["packed"]]
    if ok:
        L += ["", "Largest decoded:", "", "| Offset | Kind | Unpacked | Packed |", "|---|---|---|---|"]
        L += [f"| `{x['offset']:07x}` | {x['kind']} | {x['size']:,} | {x['packed']:,} |"
              for x in sorted(ok, key=lambda x: -x["size"])[:10]]
    L += ["", "## All five games", "", "### Block classes (same legend)", ""]
    for g in rep["roms"]:
        L += [f"`{g['code']}` {g['game']}", "", "```", *map_rows(g["class_map"]), "```", ""]
    L += ["### Audio engine signatures", "",
          "| ROM | Logik State tag | `GBAMOD` modules | MP2K SelectSong | 0x03007FF0 literals |", "|---|---|---|---|---|"]
    for g in rep["roms"]:
        a = g["audio"]
        L.append(f"| `{g['code']}` | {', '.join(a['logik_state']) or 'none'} | {a['gbamod_modules']} | {a['mp2k_selectsong']} | {a['sound_area_literals']} |")
    L += ["", "### LZ77 use", "", "| ROM | Blobs | Unpacked KiB | Packed KiB | Most common unpacked sizes |", "|---|---|---|---|---|"]
    for g in rep["roms"]:
        z = g["lz77"]
        L.append(f"| `{g['code']}` | {z['blobs']} | {z['unpacked'] / 1024:,.0f} | {z['packed'] / 1024:,.0f} | "
                 + ", ".join(f"{s:,}×{n}" for s, n in z["top_sizes"][:5]) + " |")
    for key, title in (("similarity_thumb", "Thumb code"), ("similarity_arm", "ARM code"), ("similarity_data", "Data")):
        m = rep[key]
        codes = list(m)
        L += ["", f"### Similarity: {title}", "",
              "Share (%) of the row ROM's sampled 32-byte windows (content-defined, 1 in 8) that also occur in the column ROM."
              + (" Code is compared after `normalize()` masks addresses and branch offsets." if key != "similarity_data" else ""), "",
              "| | " + " | ".join(f"`{x}`" for x in codes) + " |", "|---" * (len(codes) + 1) + "|"]
        L += [f"| `{a}` | " + " | ".join(f"{m[a][b]}" for b in codes) + " |" for a in codes]
    return "\n".join(L) + "\n"


if __name__ == "__main__":
    rep = run(data_dir(), ROOT / "docs")
    print(json.dumps({k: rep[k] for k in ("similarity_thumb", "similarity_arm", "similarity_data")}, indent=1))
    for g in rep["roms"]:
        print(g["code"], g["kib"], g["audio"], {k: v for k, v in g["lz77"].items() if k != "top_sizes"})
