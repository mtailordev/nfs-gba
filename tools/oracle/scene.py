"""Oracle cases for the menu scenes and sprites (crates/nfsgba-game/src/menu/scene.rs): the game's own
`load_menu_descriptor`, `oam_reset`, `sprite_screen_select`/`_update`, `menu_scene_setup_a`/`_b`, `unpack_to_buffer`,
`intro_page_setup`, `health_screen_image`, `menu_scene_free`, `copy_palette_to_ram` and `effect_list_init` run for
real in unicorn (heap included) on menu snapshots (menus3/scene-*.jsonl). Each case keeps what a screen can see:
the changed bytes of the pages, OBJ tiles, palettes, OAM, the shadow OAM, the OBJ tile base and the display
registers, plus the heap blocks a typed object replaces (the sprite objects, the unpack buffer).

    .venv/Scripts/python.exe tools/oracle/cases.py scene [oam descriptor select update setup ...]
"""
import json
import random
import struct

from menus import OUT, Skip

from oracle import Gba

SNAPS = ["ui-2d/lang", "ui-2d/n7", "ui-2d/n9", "race-rules/a4"]
WORLD, SS, DESC = 0x0300_00C0, 0x0300_00C0 + 0xA4, 0x087F_2FE8
UNPACK_BUF, SHADOW, TILE_BASE = 0x0300_57F0, 0x0300_64F0, 0x0300_64E0
MENU_MATERIALS, MENU_TEXELS, MATERIALS = 0x0834_5114, 0x0816_C244, 273
SCRATCH = 0x0203_0000  # EWRAM the snapshots leave free
FN = dict(oam=0x0812B10C, descriptor=0x0813_9C8C, select=0x0816_1EEC, update=0x0816_1C24, setup_a=0x0813_70D4,
          setup_b=0x0813_71A4, unpack=0x0816_3D30, page=0x0813_64C4, health=0x0813_644C, free=0x0813_9B7C,
          palette=0x0815_DFD8, effects=0x0816_200C)
VBLANK_WAIT = 0x0815_1454  # the only stub: it spins on the frame counter


def seen(a):
    """Addresses a typed screen has: BG VRAM pages and OBJ tiles, palettes, OAM, display registers, the shadow OAM."""
    return (0x0400_0000 <= a < 0x0400_0060 or 0x0500_0000 <= a < 0x0601_8000 or SHADOW <= a < SHADOW + 0x400
            or TILE_BASE <= a < TILE_BASE + 4 or 0x0400_0100 <= a < 0x0400_0110)


def now(gba, writes, addr, n, mem=()):
    """`n` bytes at `addr` after the call: the snapshot with the call's writes applied."""
    out = bytearray(gba.read_base(addr, n))
    for a, b in [*mem, *writes]:
        lo, hi = max(a, addr), min(a + len(b), addr + n)
        if lo < hi:
            out[lo - addr:hi - addr] = b[lo - a:hi - a]
    return bytes(out)


def word(gba, addr):
    return int.from_bytes(gba.read_base(addr, 4), "little")


def case(gba, snap, name, regs=(), mem=(), stack=(), dst=None, objects=False, palettes=False, **extra):
    """Runs one function. `dst` = (address, length) of a buffer whose bytes are results; `objects`: the sprite
    objects block (`SS + 0x14`) after the call."""
    gba.poke(0, bytes(0x4000))  # the BIOS reads as zeros (as menus.run)
    fn = FN[name.split(":")[0]]
    regs = {f"r{i}": v & 0xFFFF_FFFF for i, v in enumerate(regs)}
    r = gba.call(fn, mem=list(mem), regs=regs, stack=stack, stubs={VBLANK_WAIT: lambda uc: None})
    if r.stop != "return":
        raise Skip(hex(fn), r.stop)
    w = [(a, bytes(b)) for a, b in r.writes if seen(a)]
    c = dict(snap=snap, fn=hex(fn), regs=list(regs.values()), mem=[[a, bytes(b).hex()] for a, b in mem], r0=r.regs["r0"],
             writes=[[a, b.hex()] for a, b in w], **extra)
    if dst:
        c["dst"] = [dst[0], now(gba, r.writes, *dst, mem).hex()]
    if objects:
        p = int.from_bytes(now(gba, r.writes, SS + 0x14, 4, mem), "little")
        c["objects"] = now(gba, r.writes, p, 0x370, mem).hex() if p else ""
    if palettes:  # the two base palette buffers (heap) and the dirty flag
        c["palettes"] = [now(gba, r.writes, p, 0x200, mem).hex() if p else ""
                         for p in (int.from_bytes(now(gba, r.writes, a, 4, mem), "little") for a in (0x0300_577C, 0x0300_55F0))]
        c["dirty"] = int.from_bytes(now(gba, r.writes, 0x0300_563C, 4, mem), "little")
    return c


def rand_objects(rng, count):
    out = bytearray()
    for k in range(0x37):
        flags = rng.choice([0, 1, 1, 3, 2])
        scale = rng.choice([(0, 0), (0, 0), (0x100, 0x100), (0x180, 0x80), (0, 0x100)])
        frame = rng.choice([0, 0, 1, 2, 3])
        loaded = rng.choice([frame, frame, 0, 5])
        out += struct.pack("<HhhhhhhH", flags, scale[0], scale[1], frame, loaded, rng.randrange(-8, 8),
                           rng.randrange(-8, 8), rng.choice([0, 0, rng.randrange(0x4000)]))
    return bytes(out)


def make(kind, gbas, rng, n):
    cases = []
    for _ in range(n):
        snap = rng.choice(SNAPS)
        gba = gbas[snap]
        try:
            if kind == "oam":
                cases.append(case(gba, snap, "oam"))
            elif kind == "descriptor":
                cases.append(case(gba, snap, "descriptor", [WORLD, DESC], objects=True, palettes=True))
            elif kind == "select":
                cases.append(case(gba, snap, "select", [SS, rng.randrange(10)], objects=True))
            elif kind == "update":
                p = word(gba, SS + 0x14)
                idx, init = rng.randrange(10), rng.randrange(2)
                mem = [(SS + 0x18, struct.pack("<H", idx)), (p, rand_objects(rng, 0))]
                cases.append(case(gba, snap, "update", [SS, init], mem, objects=True))
            elif kind == "setup":
                first = rng.random() < 0.4
                m = rng.randrange(MATERIALS)
                mem = [(0x0300_5780, struct.pack("<I", rng.choice([0, 7])))]
                cases.append(case(gba, snap, "setup_a" if first else "setup_b",
                                  [WORLD, m, rng.randrange(49), rng.choice([0xFFFF, 0xFFFF, rng.randrange(10)])], mem,
                                  dst=(word(gba, UNPACK_BUF), 0x9600), objects=True, palettes=True))
            elif kind == "unpack":
                m = rng.randrange(MATERIALS)
                info = struct.unpack("<HHIHH", gba.read_base(MENU_MATERIALS + 0x24 * m + 2, 2 + 2 + 4 + 4)[:12]) if 0 else None
                flags = int.from_bytes(gba.read_base(MENU_MATERIALS + 0x24 * m + 2, 2), "little")
                if not flags & 0x40:
                    continue
                off = word(gba, MENU_MATERIALS + 0x24 * m + 8)
                cases.append(case(gba, snap, "unpack", [MENU_TEXELS + off, SCRATCH], dst=(SCRATCH, 0x9600), material=m))
            elif kind == "page":
                buf = word(gba, UNPACK_BUF)
                data = bytes(rng.randrange(256) for _ in range(240 * 160))
                cases.append(case(gba, snap, "page", [buf], [(buf, data)]))
            elif kind == "health":
                m = rng.choice([7, 8, 9, 10, 11, rng.randrange(MATERIALS)])
                cases.append(case(gba, snap, "health", [WORLD, m], dst=(word(gba, UNPACK_BUF), 0x9600), material=m))
            elif kind == "free":
                cases.append(case(gba, snap, "free", [WORLD]))
            elif kind == "palette":
                data = bytes(rng.randrange(256) for _ in range(0x200))
                src = rng.choice([0, SCRATCH, 0x0833_EF14, 0x0833_F114])
                cases.append(case(gba, snap, "palette", [src], [(SCRATCH, data)]))
            elif kind == "effects":
                cases.append(case(gba, snap, "effects", [0x0300_0058, 0x20, 0x7F], dst=(0x0300_0058, 8)))
        except Skip:
            continue
    return [c for c in cases if c]


KINDS = {"oam": 4, "descriptor": 8, "select": 40, "update": 300, "setup": 200, "unpack": 60, "page": 6, "health": 30,
         "free": 8, "palette": 12, "effects": 2}


def main(which):
    OUT.mkdir(parents=True, exist_ok=True)
    gbas = {s: Gba(s) for s in SNAPS}
    for kind in which or list(KINDS):
        cases = make(kind, gbas, random.Random(f"scene-{kind}"), KINDS[kind])
        path = OUT / f"scene-{kind}.jsonl"
        path.write_text("".join(json.dumps(c) + "\n" for c in cases), encoding="utf-8")
        print(f"{kind}: {len(cases)} cases -> {path}")
