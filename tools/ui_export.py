"""Export Carbon's 2D assets (docs/formats/ui.md) as PNGs for inspection.

    python tools/ui_export.py           # -> $NFSGBA_DATA/out/ui/<sha1-8>/
    python tools/ui_export.py --check   # self-check against the ROM (and the reference dumps, if present)

Writes menu/ (273 menu materials, 8bpp), hud/ (280 HUD sprite materials, 4bpp, in their OBJ palette bank),
fonts/ (glyph sheets and widths), hud_screen_N.png (the four HUD layouts, every element at frame 0) and
index.json (palette provenance per image, tool provenance). Colour 0 is transparent. Rerunning changes nothing.
"""
import json
import struct
import sys

import numpy as np
from PIL import Image

from common import data_dir, provenance, write_if_changed

ROM_BASE = 0x08000000
LEVEL_TABLE, MENU_DESCRIPTOR = 0x7F2B08, 0x7F2FE8
MENU_PALETTES = 0x33EF14
FONTS, CHAR_MAP = 0x7EE974, 0x7F5BC8
FONT_MATERIALS = (12, 13, 14, 14)
OBJ_SIZES = [(8, 8), (16, 16), (32, 32), (64, 64), (16, 8), (32, 8), (32, 16), (64, 32), (8, 16), (8, 32), (16, 32),
             (32, 64)]


def u32(rom, o):
    return struct.unpack_from("<I", rom, o)[0]


def ptr(rom, o):
    return u32(rom, o) - ROM_BASE


def materials(rom, table):
    out = []
    while True:
        i = len(out)
        idx, kind, aux, off, w, h = struct.unpack_from("<HHIIHH", rom, table + 0x24 * i)
        if idx != i:
            return out
        out.append({"index": i, "kind": kind, "offset": off, "w": w, "h": h,
                    "palette": struct.unpack_from("<H", rom, table + 0x24 * i + 0x20)[0]})


def unpack(rom, at):
    """The game's decompressor (ROM 0x169208): BIOS LZ77 stream through a 0xFF-filled 4 KiB ring from 0xFEE."""
    size = u32(rom, at) >> 8
    ring, out = bytearray(b"\xff" * 0x1000), bytearray()
    src, pos, stored, left, flags, bit = at + 4, 0xFEE, 0, size, 7, 7
    while True:
        flags, bit = flags << 1, bit + 1
        if bit == 8:
            bit, flags, src = 0, rom[src], src + 1
        if not flags & 0x80:
            b, src = rom[src], src + 1
            if stored < size:
                left -= 1
                out.append(b)
                if left <= 0:
                    return bytes(out)
            ring[pos], stored, pos = b, stored + 1, (pos + 1) & 0xFFF
            continue
        hi, lo, src = rom[src], rom[src + 1], src + 2
        for _ in range((hi >> 4) + 3):
            b = ring[(pos - ((hi & 15) << 8 | lo) - 1) & 0xFFF]
            if stored < size:
                left -= 1
                out.append(b)
                if left <= 0:
                    break
            ring[pos], stored, pos = b, stored + 1, (pos + 1) & 0xFFF


def pixels_8bpp(rom, texels, m):
    at, n = texels + m["offset"], m["w"] * m["h"]
    data = unpack(rom, at)[:n] if m["kind"] & 0x40 else rom[at:at + n]
    return np.frombuffer(data, np.uint8).reshape(m["h"], m["w"])


def pixels_4bpp(rom, texels, m):
    w, h, at = m["w"], m["h"], texels + m["offset"]
    raw = np.frombuffer(rom[at:at + w * h // 2], np.uint8)
    nib = np.stack([raw & 15, raw >> 4], -1).reshape(-1)
    if not m["kind"] & 4:
        return nib.reshape(h, w)
    tiles = nib.reshape(h // 8, w // 8, 8, 8)  # OBJ tile order: tile row, tile column, y, x
    return tiles.transpose(0, 2, 1, 3).reshape(h, w)


def palette(rom, at, n=256):
    return np.frombuffer(rom[at:at + 2 * n], "<u2")


def rgba(indices, pal, transparent=True):
    c = pal[indices].astype(np.uint32)
    ch = [((c >> s) & 31) for s in (0, 5, 10)]
    out = np.stack([(v << 3 | v >> 2) for v in ch] + [np.full_like(c, 255)], -1).astype(np.uint8)
    if transparent:
        out[indices == 0, 3] = 0
    return out


def menu_palette_of(m):
    """Palette of a menu material and where that comes from (see docs/formats/ui.md)."""
    if 1 <= m <= 5:
        return m, "menu page table (material = palette; 1, 2, 4 checked against dumps)"
    if 7 <= m <= 11:
        return 1, "health and safety (FUN_081315a0 replaces colours 0..4; 7 checked against a dump)"
    if m == 218:
        return 7, "call site FUN_0812e7xx: menu_scene_setup(0xDA, 7)"
    if 226 <= m <= 266:
        return m - 218, "story table 0x7E78B8 (material - 218)"
    return 0, "unknown: palette 0 assumed"


def save(img, path):
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(".tmp.png")
    Image.fromarray(img).save(tmp)
    if path.exists() and path.read_bytes() == tmp.read_bytes():
        tmp.unlink()
    else:
        tmp.replace(path)


def fonts(rom):
    menu = materials(rom, ptr(rom, MENU_DESCRIPTOR + 0x24))
    texels = ptr(rom, MENU_DESCRIPTOR + 0x10)
    out = []
    for f in range(4):
        d = FONTS + 0x18 * f
        _, flags, key, spacing, w, h = struct.unpack_from("<BBBbhh", rom, d)
        wt, yt = ptr(rom, d + 0x10), ptr(rom, d + 0x14)
        out.append({"flags": flags, "key": key, "spacing": spacing, "w": w, "h": h,
                    "glyphs": texels + menu[FONT_MATERIALS[f]]["offset"], "material": FONT_MATERIALS[f],
                    "widths": list(rom[wt:wt + 224]), "y_offsets": list(struct.unpack_from("<224b", rom, yt))})
    return out


def export(rom, out):
    index = {"provenance": provenance(__file__), "menu": [], "hud": [], "fonts": []}
    menu_tex, hud_tex = ptr(rom, MENU_DESCRIPTOR + 0x10), ptr(rom, LEVEL_TABLE + 0x10)
    for m in materials(rom, ptr(rom, MENU_DESCRIPTOR + 0x24)):
        p, why = menu_palette_of(m["index"])
        img = rgba(pixels_8bpp(rom, menu_tex, m), palette(rom, MENU_PALETTES + 0x200 * p))
        name = f"menu/{m['index']:03d}_{m['w']}x{m['h']}_p{p:02d}.png"
        save(img, out / name)
        index["menu"].append({**m, "file": name, "menu_palette": p, "palette_source": why})
    obj = palette(rom, ptr(rom, LEVEL_TABLE + 4))  # level +0x58 is 0 for all 12 race levels
    hud = materials(rom, ptr(rom, LEVEL_TABLE + 0x24))
    for m in hud:
        bank = obj[16 * (m["palette"] & 15):16 * (m["palette"] & 15) + 16]
        name = f"hud/{m['index']:03d}_{m['w']}x{m['h']}_bank{m['palette'] & 15:02d}.png"
        save(rgba(pixels_4bpp(rom, hud_tex, m), bank), out / name)
        index["hud"].append({**m, "file": name})
    # HUD screens: every element at frame 0, drawn back to front (element 0 = OAM 55 is behind element 1).
    screens_at, elements_at = ptr(rom, LEVEL_TABLE + 0x2C), ptr(rom, LEVEL_TABLE + 0x30)
    for s in range((elements_at - screens_at) // 8):
        sx, sy, first, count = struct.unpack_from("<hhHH", rom, screens_at + 8 * s)
        canvas = np.zeros((160, 240, 4), np.uint8)
        for k in range(first, first + count):
            e = elements_at + 0x14 * k
            x, y, mat, _ = struct.unpack_from("<hhHH", rom, e)
            m = hud[mat]
            pal = rom[e + 0x11]
            bank = obj[16 * (pal & 15):16 * (pal & 15) + 16]
            img = rgba(pixels_4bpp(rom, hud_tex, m), bank)
            x, y = x + sx, y + sy
            for yy in range(m["h"]):
                for xx in range(m["w"]):
                    if 0 <= x + xx < 240 and 0 <= y + yy < 160 and img[yy, xx, 3]:
                        canvas[y + yy, x + xx] = img[yy, xx]
        save(canvas, out / f"hud_screen_{s}.png")
    lang_pal = palette(rom, MENU_PALETTES + 4 * 0x200)
    for f, font in enumerate(fonts(rom)):
        w, h = font["w"], font["h"]
        sheet = np.zeros((14 * (h + 4), 16 * (w + 2)), np.uint8)
        for g in range(224):
            r, c = divmod(g, 16)
            cell = np.frombuffer(rom[font["glyphs"] + g * w * h:font["glyphs"] + (g + 1) * w * h], np.uint8)
            cell = cell.reshape(h, w).copy()
            cell[:, font["widths"][g]:] = 0
            y0 = r * (h + 4) + 2 + font["y_offsets"][g]
            sheet[max(y0, 0):y0 + h, c * (w + 2):c * (w + 2) + w] = cell[max(0, -y0):]
        save(rgba(sheet, lang_pal), out / f"fonts/font{f}.png")
        index["fonts"].append({k: v for k, v in font.items()} | {"file": f"fonts/font{f}.png", "first_char": 0x20})
    write_if_changed(out / "index.json", json.dumps(index, indent=1) + "\n")


def check(rom):
    """Self-check: table sizes, the unpacker on every packed menu image, and the language screen if dumped."""
    menu = materials(rom, ptr(rom, MENU_DESCRIPTOR + 0x24))
    hud = materials(rom, ptr(rom, LEVEL_TABLE + 0x24))
    assert (len(menu), len(hud)) == (273, 280), (len(menu), len(hud))
    last = hud[-1]
    assert ptr(rom, LEVEL_TABLE + 0x10) + last["offset"] + last["w"] * last["h"] // 2 == ptr(rom, LEVEL_TABLE + 4)
    for m in menu:
        if m["kind"] & 0x40:
            assert len(unpack(rom, 0x16C244 + m["offset"])) >= m["w"] * m["h"]
    dump = data_dir() / "work" / rom_sha8(rom) / "ui-2d" / "lang.vram.bin"
    if dump.exists():
        img = pixels_8bpp(rom, 0x16C244, menu[4])
        vram = np.frombuffer(dump.read_bytes()[:38400], np.uint8).reshape(160, 240)
        flag = pixels_8bpp(rom, 0x16C244, menu[181])
        assert ((img != vram).sum() > 0) and (vram[19:81, 9:79][flag != 0] == flag[flag != 0]).all()
    print("ok")


def rom_sha8(rom):
    import hashlib
    return hashlib.sha1(rom).hexdigest()[:8]


def main():
    m = json.loads((data_dir() / "vault" / "manifest.json").read_text(encoding="utf-8"))
    r = next(r for r in m["roms"] if r["sha1"] == m["canonical_target"])
    rom = (data_dir() / r["vault_file"]).read_bytes()
    if "--check" in sys.argv:
        check(rom)
        return
    out = data_dir() / "out" / "ui" / rom_sha8(rom)
    export(rom, out)
    print(f"wrote {out}")


if __name__ == "__main__":
    main()
