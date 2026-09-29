"""Which known structure owns each byte of the canonical ROM, and what is still unexplained.

    .venv/Scripts/python.exe tools/rom_attribution.py
    -> $NFSGBA_DATA/out/coverage/<sha8>/rom-attribution.csv (start,end,size,category,owner: every byte, in ranges)
       and rom-attribution.txt (percentages, unattributed ranges largest first)

Claims are made most specific first; a byte keeps its first owner. Computed from the data itself: code (each Ghidra
function from its start to the next one, so literal pools count with their function), the audio regions, the four
material tables and every texel stream they address (packed streams by the bytes the decoder consumes for the
image; the game's decoder then reads 8 bytes past it, docs/formats/ui.md), palettes, the vehicle model bank arrays
(extents from the model records), city and menu-scene sectors and walls, routes (template entities, section
tables, waypoints), the text table and every string it points to, the level descriptors. Then every sized ROM row
of docs/engine/address-map.md. Rows the map marks unknown, and container ranges, claim nothing, so gaps stay visible.
"""
import re
import struct
from collections import Counter

import numpy as np

from common import ROOT, data_dir, provenance, write_if_changed
from mgba_ctl import canonical

LEVEL = 0x7F2B08  # level descriptors (0x68 bytes); the menu descriptor at 0x7F2FE8 has the same layout
MENU_LEVEL = 0x7F2FE8
CODE = (0x12A84C, 0x16C244)


class Rom:
    def __init__(self, data: bytes):
        self.b = data
        self.owner = np.full(len(data), -1, dtype=np.int32)
        self.labels: list[tuple[str, str]] = []  # (category, owner)

    def u8(self, a): return self.b[a]
    def u16(self, a): return struct.unpack_from("<H", self.b, a)[0]
    def u32(self, a): return struct.unpack_from("<I", self.b, a)[0]

    def ptr(self, a):
        v = self.u32(a)
        return v - 0x0800_0000 if 0x0800_0000 <= v < 0x0800_0000 + len(self.b) else None

    def claim(self, start, end, category, owner):
        """Give [start, end) to `owner` where no earlier claim holds it; returns the bytes newly attributed."""
        start, end = max(0, start), min(len(self.b), end)
        if start >= end:
            return 0
        self.labels.append((category, owner))
        seg = self.owner[start:end]
        free = seg == -1
        seg[free] = len(self.labels) - 1
        return int(free.sum())


def lz77_consumed(rom: Rom, at: int, out_len: int) -> int:
    """Source bytes a BIOS-format LZ77 stream at `at` uses to produce `out_len` bytes."""
    s, n = at + 4, 0
    while n < out_len:
        flags = rom.u8(s)
        s += 1
        for bit in range(8):
            if n >= out_len:
                break
            if flags & (0x80 >> bit):
                n += (rom.u8(s) >> 4) + 3
                s += 2
            else:
                n += 1
                s += 1
    return s - at


def materials(rom: Rom, table: int):
    """Self-indexed 0x24-byte material records: (index, kind, aux, offset, width, height)."""
    i = 0
    while rom.u16(table + 0x24 * i) == i:
        m = table + 0x24 * i
        yield i, rom.u16(m + 2), rom.u32(m + 4), rom.u32(m + 8), rom.u16(m + 0x0C), rom.u16(m + 0x0E)
        i += 1


def claim_materials(rom: Rom, name: str, table: int, base: int, bpp: int, stats: Counter):
    n = 0
    for i, kind, aux, off, w, h in materials(rom, table):
        n += 1
        at = base + off
        if kind & 0x40:  # packed: the game's ring decoder
            size = rom.u32(at) >> 8
            used = lz77_consumed(rom, at, size - 8)
            rom.claim(at, at + used, "images", f"{name} material {i} texels (packed)")
            stats[f"{name} packed"] += 1
        elif name == "city" and kind == 2:  # column-mapped facade: map byte per u, unique columns
            m = base + aux
            cols = max(rom.b[m:m + w]) + 1
            rom.claim(m, m + w, "images", f"city material {i} column map")
            rom.claim(at, at + cols * h, "images", f"city material {i} columns")
        elif name == "city" and kind == 1:  # sky gradient: w × h BGR555 (1 × 128; the gradient is the first 64)
            rom.claim(at, at + 2 * w * h, "images", f"city material {i} sky gradient")
        else:
            rom.claim(at, at + w * h * bpp // 8, "images", f"{name} material {i} texels")
    rom.claim(table, table + 0x24 * n, "tables", f"{name} materials ({n} × 0x24)")
    stats[f"{name} materials"] = n


def claim_code(rom: Rom, decomp: str):
    funcs = sorted((int(a, 16) - 0x0800_0000, n) for a, n in re.findall(r"^// ==== (08[0-9a-f]{6}) (\S+)", decomp, re.M))
    inside = [(a, n) for a, n in funcs if CODE[0] <= a < CODE[1]]
    for (a, n), (b, _) in zip(inside, inside[1:] + [(CODE[1], None)]):
        rom.claim(a, b, "code", f"code {n}")
    rom.claim(0xC0, 0x210, "code", "code crt0 (FUN_080000c0)")
    return [(a, n) for a, n in funcs if not (CODE[0] <= a < CODE[1]) and a != 0xC0]


def claim_level(rom: Rom, rec: int, tag: str):
    """Sectors, walls and the descriptor itself for a level descriptor (city or menu scene)."""
    rom.claim(rec, rec + 0x68, "tables", f"{tag} descriptor")
    walls, sectors = rom.ptr(rec + 0x14), rom.ptr(rec + 0x18)
    nsec = (walls - sectors) // 0x30
    nwall = max(rom.u16(sectors + 0x30 * s) + rom.u16(sectors + 0x30 * s + 2) for s in range(nsec))
    rom.claim(sectors, sectors + 0x30 * nsec, "city", f"{tag} sectors ({nsec} × 0x30)")
    rom.claim(walls, walls + 0x44 * nwall, "city", f"{tag} walls ({nwall} × 0x44)")


def claim_models(rom: Rom):
    t = LEVEL
    models, verts, idx, uvidx, uvs, sizes = (rom.ptr(t + k) for k in (0x34, 0x38, 0x3C, 0x40, 0x48, 0x54))
    n = (verts - models) // 40
    ends = Counter()
    for i in range(n):
        o = models + 40 * i
        vstart, istart, uvstart, uvistart, sstart = (rom.u32(o + 4 * k) for k in (0, 1, 2, 6, 7))
        npoly, nvert, nuv = rom.u16(o + 0x22), rom.u16(o + 0x24), rom.u16(o + 0x26)
        corners = sum(rom.b[sizes + sstart:sizes + sstart + npoly])
        for key, end in (("verts", vstart + nvert), ("idx", istart + corners), ("uvidx", uvistart + corners),
                         ("uvs", uvstart + nuv), ("sizes", sstart + npoly)):
            ends[key] = max(ends[key], end)
    rom.claim(models, models + 40 * n, "models", f"vehicle models ({n} × 40)")
    rom.claim(verts, verts + 6 * ends["verts"], "models", "model vertices")
    rom.claim(idx, idx + 2 * ends["idx"], "models", "model vertex indices")
    rom.claim(uvidx, uvidx + 2 * ends["uvidx"], "models", "model UV indices")
    rom.claim(uvs, uvs + 4 * ends["uvs"], "models", "model UVs")
    rom.claim(sizes, sizes + ends["sizes"], "models", "model polygon sizes")


def claim_routes(rom: Rom):
    table = 0x7F2798
    n = 0
    while rom.u32(table + 0x14 * n + 0x10) == 0:
        r = table + 0x14 * n
        ents, secs, line = rom.ptr(r), rom.ptr(r + 4), rom.ptr(r + 8)
        if ents is not None:
            rom.claim(ents, ents + 4 * 0xA4, "routes", f"route {n} template entities")
        if secs is not None and line is not None:
            nsec = (line - secs) // 8
            total = sum(rom.u16(secs + 8 * k) for k in range(nsec))
            rom.claim(secs, line, "routes", f"route {n} section table")
            rom.claim(line, line + 24 * total, "routes", f"route {n} racing line ({total} waypoints)")
        n += 1
    rom.claim(table, table + 0x14 * n, "tables", f"route table ({n} × 0x14)")


def claim_text(rom: Rom):
    table, keys = 0x7E86A0, 977
    lo, hi = len(rom.b), 0
    for k in range(keys * 6):  # key names, then 5 languages
        p = rom.ptr(table + 4 * k)
        end = rom.b.index(b"\0", p) + 1
        rom.claim(p, end, "text", "text strings")
        lo, hi = min(lo, p), max(hi, end)
    rom.claim(table, table + 4 * (keys * 6 + 5), "text", "text table (5,867 × 4)")
    return lo, hi


def claim_audio(rom: Rom):
    rom.claim(0x000210, 0x0005D4, "audio", "sound-effect table")
    rom.claim(0x0005D4, 0x04EA12, "audio", "sound-effect sample data")
    mods = [0x04EA14, 0x04F134, 0x04F8DC, 0x050034, 0x0507FC, 0x050FD4]
    for i, (a, b) in enumerate(zip(mods, mods[1:])):
        rom.claim(a, b, "audio", f"GBAMOD30 module {i}")
    rom.claim(0x050FD4, 0x051FD4, "audio", "sample bank headers")
    rom.claim(0x051FD4, 0x12A84C, "audio", "sample bank data")
    for at in (0x15CF2C, 0x15CFD4):  # LZ77-packed ARM mixers (BIOS-decoded: the header size is exact)
        rom.claim(at, at + lz77_consumed(rom, at, rom.u32(at) >> 8), "audio", f"packed ARM mixer {at:#08x}")
    rom.claim(0x151E34, 0x151E3C, "audio", "\"GBAMOD30\" literal")


def size_of(expr: str):
    """A map Size cell part as bytes, or None: '227 × 0x24', '4 + 40 × 24', '0x2000 × i16', '32 B', '5'."""
    e = expr.replace(",", "").replace("×", "*").replace("i16", "2").replace("u16", "2").replace("i32", "4")
    e = e.replace("u32", "4").replace(" B", "").strip().lower()
    if not re.fullmatch(r"[0-9a-fx+* ()]+", e):
        return None
    try:
        return eval(e)  # noqa: S307 (digits, hex and operators only)
    except (SyntaxError, NameError):  # e.g. "6 × 2 u16" -> "6 * 2 2": not a size we can read
        return None


def claim_map(rom: Rom):
    text = (ROOT / "docs" / "engine" / "address-map.md").read_text(encoding="utf-8")
    section = text.split("## ROM", 1)[1].split("\n### ", 1)[0]
    skipped = []
    for line in section.splitlines():
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if len(cells) < 4 or not cells[0].startswith("`"):
            continue
        where, size, what = cells[0], cells[1], cells[2]
        offs = [int(x, 16) for x in re.findall(r"0x[0-9A-Fa-f]+", where)]
        if "–" in where or "…" in where or re.search(r"unknown|unexplained", what, re.I) or not size:
            skipped.append(where)
            continue
        parts = [p.strip() for p in size.split(" / ")]
        mult = re.search(r"×\s*(\S+)$", parts[-1])
        if len(parts) == len(offs) and mult:  # "12 / 12 / 18 × 4": the last factor applies to all
            parts = [p if "×" in p else f"{p} × {mult.group(1)}" for p in parts]
        sizes = [size_of(p) for p in parts] if len(parts) == len(offs) else [size_of(parts[0])] * len(offs)
        if None in sizes:
            skipped.append(where)
            continue
        for o, n in zip(offs, sizes):
            rom.claim(o, o + n, "tables", f"map: {re.sub(r'[`*]', '', what)[:70]}")
    return skipped


def main():
    rom_path, sha8 = canonical()
    rom = Rom(rom_path.read_bytes())
    decomp = (data_dir() / "work" / sha8 / "ghidra" / "carbon_decomp.c").read_text(encoding="utf-8")
    stats = Counter()

    rom.claim(0, 0xC0, "header", "cartridge header")
    claim_audio(rom)
    stray = claim_code(rom, decomp)
    claim_materials(rom, "menu", 0x345114, 0x16C244, 8, stats)
    claim_materials(rom, "hud", 0x36CF5C, 0x347B74, 4, stats)
    claim_materials(rom, "vehicle", 0x45F5C0, 0x370550, 8, stats)
    claim_materials(rom, "city", 0x720DE8, 0x47BC6C, 8, stats)
    rom.claim(0x33EF14, 0x33EF14 + 49 * 0x200, "palettes", "menu palettes (49 × 0x200)")
    rom.claim(0x36C75C, 0x36C75C + 4 * 0x200, "palettes", "OBJ palettes and car ramps (4 × 0x200)")
    rom.claim(0x71F1E8, 0x71F1E8 + 14 * 0x200, "palettes", "city palettes (14 × 0x200)")
    claim_models(rom)
    for i in range(12):
        claim_level(rom, LEVEL + 0x68 * i, f"level {i}")
    claim_level(rom, MENU_LEVEL, "menu scene")
    claim_routes(rom)
    strings = claim_text(rom)
    skipped = claim_map(rom)
    last = len(rom.b.rstrip(b"\0"))  # the ROM is zero-filled after its last data byte
    rom.claim(last, len(rom.b), "padding", "zero fill to the end of the ROM")

    # Ranges, and alignment padding (gaps of 1..3 bytes ending on a word boundary).
    own = rom.owner
    change = np.flatnonzero(np.diff(own)) + 1
    starts, ends = np.concatenate(([0], change)), np.concatenate((change, [len(own)]))
    rows, gaps, padding = [], [], 0
    for s, e in zip(starts.tolist(), ends.tolist()):
        if own[s] >= 0:
            cat, name = rom.labels[own[s]]
        elif e - s < 4 and e % 4 == 0:
            cat, name = "padding", "alignment"
            padding += e - s
        else:
            cat, name = "unattributed", ""
            gaps.append((e - s, s, e))
        rows.append(f"{s:#08x},{e:#08x},{e - s},{cat},{name}")
    out = data_dir() / "out" / "coverage" / sha8
    write_if_changed(out / "rom-attribution.csv", "start,end,size,category,owner\n" + "\n".join(rows) + "\n")

    total = len(own)
    by_cat = Counter()
    for s, e in zip(starts.tolist(), ends.tolist()):
        by_cat[rom.labels[own[s]][0] if own[s] >= 0 else ("padding" if e - s < 4 and e % 4 == 0 else "unattributed")] += e - s
    gaps.sort(reverse=True)

    def show(s, e):
        chunk = rom.b[s:e]
        common = Counter(chunk).most_common(1)[0]
        near = [rom.labels[own[k]][1] if 0 <= k < total and own[k] >= 0 else "-" for k in (s - 1, e)]
        return (f"{s:#08x}-{e:#08x} {e - s:>8} B  first {chunk[:12].hex(' ')}  most common byte {common[0]:#04x} "
                f"×{common[1]}  after: {near[0]}  before: {near[1]}")

    lines = [f"provenance: {provenance(__file__)}",
             f"attributed: {100 * (total - by_cat['unattributed']) / total:.3f}% of {total:,} bytes "
             f"({by_cat['unattributed']:,} bytes unattributed in {len(gaps)} ranges)",
             "by category: " + ", ".join(f"{c} {n:,}" for c, n in by_cat.most_common()),
             "materials: " + ", ".join(f"{k} {v}" for k, v in sorted(stats.items())),
             f"text strings span {strings[0]:#08x}-{strings[1]:#08x}",
             "Ghidra functions outside the code range: " + ", ".join(f"{a + 0x0800_0000:#010x} {n}" for a, n in stray),
             "map rows claiming nothing (ranges, unknown, or no parsable size): " + "; ".join(skipped),
             "known gaps:"]
    for s, e in ((0x794000, 0x799B88), (0x7F5CC8, 0x800000)):
        free = int((own[s:e] == -1).sum())
        lines.append(f"  {s:#08x}-{e:#08x}: {free:,} of {e - s:,} bytes unattributed")
    lines.append("largest unattributed ranges:")
    lines += ["  " + show(s, e) for _, s, e in gaps[:40]]
    write_if_changed(out / "rom-attribution.txt", "\n".join(lines) + "\n")
    print("\n".join(lines))


if __name__ == "__main__":
    main()
