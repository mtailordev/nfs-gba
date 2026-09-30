"""Race frames facing the city's 64-row walls (FIDELITY R20): the game's own `build_visible_sectors` and
`draw_visible_sectors` on the reference race snapshot with the camera moved.

    .venv/Scripts/python.exe tools/oracle/cases.py render2

For each view: the camera stands at its sector's corner average, 40 units above the floor, level, looking at the
middle of a solid wall whose material is 64 texel rows high; the camera matrix (world `+0x54`) and list entry 0
(world `+0x60`: the sector over the whole screen) are poked, then the two IWRAM functions run. Writes
`render2/<view>.{iwram,wram,vram}.bin` in the `entity-draw` layout (inputs before the draw, the page after it);
`render::entities::tests::probe_frames_match` draws them with `render::draw_world`.
"""
import math
import struct

from unicorn import UC_HOOK_MEM_WRITE

from oracle import Gba, canonical, data_dir

WORLD = 0x0300_00C0
OUT = data_dir() / "work" / canonical()[1] / "render2"
# (name, camera sector, wall): span 8,192 = one texture height (sectors 448, 344), and 22,937 (2.8 heights, 528).
VIEWS = [("w64-448", 448, 1813), ("w64-528", 528, 2133), ("w64-344", 344, 1404)]


def camera(rom, sector, wall):
    u16 = lambda o: struct.unpack_from("<H", rom, o)[0]
    i16 = lambda o: struct.unpack_from("<h", rom, o)[0]
    i32 = lambda o: struct.unpack_from("<i", rom, o)[0]
    level = 0x7F_2B08
    walls, sectors = i32(level + 0x14) - 0x0800_0000, i32(level + 0x18) - 0x0800_0000
    first, n = u16(sectors + 0x30 * sector), u16(sectors + 0x30 * sector + 2)
    assert first <= wall < first + n
    at = lambda k: walls + 0x44 * k
    cx = sum(i32(at(k)) for k in range(first, first + n)) // n
    cz = sum(i32(at(k) + 4) for k in range(first, first + n)) // n
    floor = sum(i16(at(k) + 0x38) for k in range(first, first + n)) // n
    nxt = first + (wall - first + 1) % n
    mx, mz = (i32(at(wall)) + i32(at(nxt))) / 2, (i32(at(wall) + 4) + i32(at(nxt) + 4)) / 2
    d = math.hypot(mx - cx, mz - cz)
    dx, dz = round(16384 * (mx - cx) / d), round(16384 * (mz - cz) / d)
    # Rows: right (x'), up, ahead (depth); `-y` is up. Then the translation: minus the camera position.
    return [dz, 0, dx, 0, 16384, 0, -dx, 0, dz, -cx, -(floor - 40), -cz]


def main(argv):
    assert not argv, "usage: cases.py render2"
    OUT.mkdir(parents=True, exist_ok=True)
    rom = canonical()[0].read_bytes()
    for name, sector, wall in VIEWS:
        gba = Gba("mgba/race")
        # A byte store to mode-4 BG VRAM writes both pixels of its halfword (the floor spans store one byte per
        # pair); unicorn stores one byte. The hook runs before the store, so it writes the partner byte itself.
        vram = gba.mem["vram"]

        def dup(uc, access, addr, size, value, _):
            if size == 1:
                vram[(addr - 0x0600_0000) ^ 1] = value & 0xFF

        gba.uc.hook_add(UC_HOOK_MEM_WRITE, dup, begin=0x0600_0000, end=0x0601_3FFF)
        r32 = lambda a: struct.unpack("<I", gba.read_base(a, 4))[0]
        gba.poke(r32(WORLD + 0x54), struct.pack("<12i", *camera(rom, sector, wall)))
        gba.poke(r32(WORLD + 0x60), struct.pack("<8H", sector, 0, 240, 0, 159, 0, 0, 0))
        gba.poke(WORLD + 0xEA, struct.pack("<H", sector))
        r = gba.call(0x0300_4828, mode="arm", r0=WORLD, keep=True)
        assert r.stop == "return", r.stop
        inputs = {d: gba.read_base(b, s) for d, b, s in (("iwram", 0x0300_0000, 0x8000), ("wram", 0x0200_0000, 0x4_0000))}
        r = gba.call(0x0300_48C8, mode="arm", r0=WORLD, keep=True)
        assert r.stop == "return", r.stop
        for d, data in inputs.items():
            (OUT / f"{name}.{d}.bin").write_bytes(data)
        (OUT / f"{name}.vram.bin").write_bytes(gba.read_base(0x0600_0000, 0x1_8000))
        count = struct.unpack("<H", gba.read_base(WORLD + 0xEE, 2))[0]
        print(f"{name}: sector {sector} wall {wall}, {count} visible sectors -> render2/{name}.*")
