"""Oracle cases for the garage's 3D car (crates/nfsgba-game/src/menu/car.rs): the game's own `garage_load_car_atlas`
(0x0812BEEC), `garage_load_car_palette` (0x0812BF48) and `garage_draw_car` (0x0812BFA4) run in a row in unicorn on the
garage snapshots (menus3/gar-*) with a random car, its record (parts, rim, decals, glass, paint), turntable state,
centre and depth, drawing page and palette fade. A case keeps the changed bytes of the three calls and the atlas
(length and CRC-32); the Rust replay (`menu::car::tests::turntable_matches_the_game`) compares the page, palettes,
turntable variables and atlas.

    .venv/Scripts/python.exe tools/oracle/cases.py turntable [COUNT]     # default 400 -> garage2/turntable.jsonl
"""
import json
import random
import struct
import zlib

from common import data_dir
from menus import PROFILE_AT, Skip, word

from oracle import Gba

OUT = data_dir() / "work" / "e5298b24" / "garage2"
SNAPS = ["menus3/gar-20-7", "menus3/gar-18-12", "menus3/gar-19-0"]
ATLAS, PALETTE, DRAW = 0x0812BEEC, 0x0812BF48, 0x0812BFA4
CAR_TABLE, MATERIALS = 0x087F0BD8, 0x0845F5C0


def call(gba, fn, **kw):
    r = gba.call(fn, keep=True, **kw)
    if r.stop != "return":
        raise Skip(hex(fn), r.stop)
    return r


def make_case(rng, snap):
    gba = Gba(snap)
    pick = rng.choice
    car = rng.randrange(15)
    record = bytes([rng.randrange(4), rng.randrange(7), rng.randrange(15), pick([0, 0, 0, 1]), pick([0, 0, 1, 2, 3]),
                    rng.randrange(8), pick([rng.randrange(20), rng.randrange(20), 0x14, 0x1F])]
                   + [rng.randrange(256) for _ in range(10)])
    drawing = pick([0x0600_0000, 0x0600_A000])
    dispcnt = struct.unpack("<H", gba.read_base(0x0400_0000, 2))[0] & ~0x10 | (0x10 if drawing == 0x0600_0000 else 0)
    x, y, z = pick([0x78, 0xB2, rng.randrange(0x30, 0xC0)]), pick([0x3C, rng.randrange(0x20, 0x70)]), \
        pick([0xFA, 0x172, rng.randrange(0x90, 0x300)])
    mem = [
        (0x03005718, word(car)), (0x03005700, record), (0x03005F9C, word(rng.randrange(1 << 16))),
        (PROFILE_AT + 0x2F6, struct.pack("<H", pick([0, 0, 0x80, 5, 1]))),
        (PROFILE_AT + 0x2F8, word(rng.randrange(1 << 16))),
        (0x03005630, word(pick([0, 0, 0, 16, -16]))), (0x03005780, word(pick([0, 0, 0, 0, 1]))),
        (0x03000080, word(drawing)), (0x0400_0000, struct.pack("<H", dispcnt)),
    ]
    atlas = call(gba, ATLAS, mem=mem)
    pal = call(gba, PALETTE)
    draw = gba.call(DRAW, regs=dict(r0=x, r1=y, r2=z))
    if draw.stop != "return":
        raise Skip(hex(DRAW), draw.stop)
    ptr = struct.unpack("<I", atlas.read(0x03006164, 4))[0]
    material = struct.unpack("<h", gba.read_base(CAR_TABLE + 0x58 * car + 0xC, 2))[0] + record[3]
    m = gba.read_base(MATERIALS + 0x24 * material, 0x24)
    size = struct.unpack_from("<H", m, 0xC)[0] * struct.unpack_from("<H", m, 0xE)[0]
    data = atlas.read(ptr, size)
    hexed = lambda ws: [[a, b.hex()] for a, b in ws]
    small = [w for w in atlas.writes if len(w[1]) < 4096]  # the atlas itself is checked by its CRC
    return dict(snap=snap, mem=hexed(mem), args=[x, y, z], atlas=[len(data), zlib.crc32(data)],
                atlas_writes=hexed(small), pal_writes=hexed(pal.writes), draw_writes=hexed(draw.writes))


def main(argv: list[str]) -> None:
    n = int(argv[0]) if argv else 400
    rng = random.Random(0x7A57)
    OUT.mkdir(parents=True, exist_ok=True)
    cases, skipped = [], 0
    for i in range(n):
        try:
            cases.append(make_case(rng, SNAPS[i % len(SNAPS)]))
        except Skip:
            skipped += 1
    path = OUT / "turntable.jsonl"
    path.write_text("".join(json.dumps(c) + "\n" for c in cases), encoding="utf-8")
    print(f"turntable: {len(cases)} cases ({skipped} skipped) -> {path}")
