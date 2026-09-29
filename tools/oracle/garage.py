"""Oracle cases for the garage and the map drawing (crates/nfsgba-game/src/menu/garage.rs, map.rs): the game's own
Kind18 handlers (0x08133708 enter, 0x081338E0 update, 0x08133F2C draw), the unlock rules (0x0812C81C state, 0x0812C5C4
owned, 0x0812D960 price, 0x0812C8A8 buy, 0x0812C984 and 0x081300E0 the new marks), the stat bars (0x08133D30) and
`map_draw` (0x081435C4, and Kind7's draw handler around it) on generated menu states.

    .venv/Scripts/python.exe tools/oracle/cases.py garage [COUNT]     # default 1500 handler + 300 per helper

Writes menus3/garage.jsonl and menus3/mapdraw.jsonl (same case format as `menus`: memory poked, every byte written, the
stub calls, the result). Only the car models and atlases (garage_load_car_atlas/_palette, garage_draw_car), sounds and
the save write stay stubs.
"""
import json
import random
import struct

import menus as M
from draw import CELL, PAGE, WORLD_PAGE_CELL, base
from menus import KIND_HANDLERS, OUT, PROFILE_AT, Skip, menu_state, run, word

from oracle import Gba

KIND18 = KIND_HANDLERS[5]
HELPERS = dict(state=0x0812C81C, owned=0x0812C5C4, price=0x0812D960, buy=0x0812C8A8, newpart=0x0812C984,
               listnew=0x081300E0, stats=0x08133D30, copy=0x0812D564)
MAP_DRAW, MAP_PALETTES = 0x081435C4, 0x08143284
SNAPS = ["ui-2d/lang", "ui-2d/n7", "ui-2d/n9", "race-rules/a4", "race-rules/v2"]


def stubs_without(*addrs):
    return {a: n for a, n in M.STUBS.items() if a not in addrs}


GARAGE_STUBS = stubs_without(*KIND18[:3], *HELPERS.values())
MAP_STUBS = stubs_without(MAP_DRAW, MAP_PALETTES)


def with_stubs(table, *args, **kw):
    saved = M.STUBS
    M.STUBS = table
    try:
        return run(*args, **kw)
    finally:
        M.STUBS = saved


def unlock_ids(gba):
    """Ids of the unlock table (id, price pairs, sorted) plus the ranges the rules test."""
    raw = gba.read_base(0x087E4DE4, 4 * 400)
    ids = [struct.unpack_from("<H", raw, 4 * i)[0] for i in range(400)]
    ids = ids[:next(i for i in range(1, 400) if ids[i] <= ids[i - 1])]
    return [i for i in ids if i < 0x200] + list(range(0x136)) + list(range(0x108, 0x117))


def garage_state(rng, gba, screen, ids):
    pick = rng.choice
    lev = lambda: pick([0, 0, 0x55, 0xAA, 0xFF, 0x1B, rng.randrange(256)])
    mem = menu_state(rng, [screen])
    ext = [
        (PROFILE_AT + 0xC, word(pick([0, 100, 5000, 30000, 250000, rng.randrange(1 << 20), -5]))),
        (PROFILE_AT + 0x10, bytes([rng.randrange(15), rng.randrange(15)])),
        (PROFILE_AT + 0x12, struct.pack("<H", pick([0, 0xFFFF, 1, 0x7FFF, rng.randrange(1 << 16)]))),
        (PROFILE_AT + 0x14, bytes(pick([0, 0xFF, rng.randrange(256)]) for _ in range(225))),
        (PROFILE_AT + 0xF5, bytes(pick([0, 0xFF, rng.randrange(256)]) for _ in range(4))),
        (PROFILE_AT + 0xF9, bytes(lev() if k % 17 < 16 else pick([0, 1, 0xF, 0xFF]) for k in range(255))),
        (PROFILE_AT + 0x205, bytes(pick([0, 0x55, 0xAA, 0xFF, rng.randrange(256)]) for _ in range(19))),
        (PROFILE_AT + 0x364, bytes([rng.randrange(8), rng.randrange(7), 0])),
        (PROFILE_AT + 0x338, word(pick([0, 0, 1, 2, 3, 5, 9, 14]))),
        (PROFILE_AT + 0x33C, bytes(pick([0, 1, 3, 0xFF]) for _ in range(8))),
        (PROFILE_AT + 0x404, bytes([pick([0, 2, 3])])),
        (PROFILE_AT + 0x42D, bytes(pick([0, 0xFF, 0x7F, rng.randrange(256)]) for _ in range(48))),
        (0x03005718, word(rng.randrange(15))),
        (0x03005700, bytes(lev() for _ in range(17))),
        (0x03005F9C, word(rng.randrange(1 << 16))),
        (0x030059A0, b"".join(struct.pack("<HH", pick(ids), rng.randrange(977)) for _ in range(16))),
        (0x030059E0, bytes(rng.randrange(4) for _ in range(4))),
        (0x030059E4, bytes([rng.randrange(4)])),
        (0x030064C4, struct.pack("<H", pick([0, 0x40, 0x80, 0xC0]))),
        (0x030059F4, word(pick([0, 0, 1, -1]))),
        (0x030059F0, word(pick([-1, -1, -1, 1, 2]))),
        (0x030064C0, struct.pack("<H", pick([0, 1, 1, 1, 2, 0x10, 0x20, 0x40, 0x80, 0x30, 0xC0, 0x11, 3]))),
    ]
    if screen == 0x12:
        ext[7] = (PROFILE_AT + 0x364, bytes([rng.randrange(8), rng.randrange(7), 0]))
    return mem + ext


def kind18(gba, rng, n):
    ids = unlock_ids(gba)
    gbas = {s: Gba(s) for s in SNAPS}
    cases = []
    for i in range(n):
        snap = rng.choice(SNAPS)
        screen = rng.choice([0x13, 0x13, 0x14, 0x14, 0x12])
        mem = garage_state(rng, gbas[snap], screen, ids)
        fn = KIND18[i % 3]
        ret = {0x08149FD8: rng.choice([0, 1])}
        try:
            r0, writes, calls = with_stubs(GARAGE_STUBS, gbas[snap], fn, mem, ret, regs={"r0": rng.choice([0, 1])})
        except Skip:
            continue
        cases.append(dict(snap=snap, fn=hex(fn), arg=0, mem=[[a, b.hex()] for a, b in mem],
                          ret={hex(a): v for a, v in ret.items()}, r0=r0,
                          writes=[[a, b.hex()] for a, b in writes], calls=calls))
    return cases


def helpers(gba, rng, n):
    ids = unlock_ids(gba)
    gbas = {s: Gba(s) for s in SNAPS}
    cases = []
    for i in range(n * len(HELPERS)):
        snap = rng.choice(SNAPS)
        name = list(HELPERS)[i % len(HELPERS)]
        fn = HELPERS[name]
        mem = garage_state(rng, gbas[snap], rng.choice([0x13, 0x14, 0x12, 4, 0x1D, 0x1E]), ids)
        ident = rng.choice(ids) if rng.random() < 0.8 else rng.randrange(0x140)
        args = {"listnew": [rng.choice([4, 0x1D, 0x1E, 0x23, 0x24, 5]), rng.randrange(7)],
                "stats": [rng.randrange(15), rng.randrange(8, 160), rng.randrange(8, 100), rng.choice([0, 1])],
                "copy": []}.get(name, [ident])
        regs = {f"r{k}": v for k, v in enumerate(args)}
        try:
            r0, writes, calls = with_stubs(GARAGE_STUBS, gbas[snap], fn, mem, {}, regs=regs)
        except Skip:
            continue
        cases.append(dict(snap=snap, fn=hex(fn), args=args, mem=[[a, b.hex()] for a, b in mem], ret={}, r0=r0,
                          writes=[[a, b.hex()] for a, b in writes], calls=calls))
    return cases


def mapdraw(gba, rng, n):
    gbas = {s: Gba(s) for s in SNAPS}
    cases = []
    for i in range(n):
        snap = rng.choice(SNAPS)
        pick = rng.choice
        screen = pick([7, 8, 0xE, 0x11, 0x11])
        mode = pick([2, 3]) if screen == 0x11 else pick([0, 1, 2])
        cursor = rng.randrange(18 if screen == 8 or (screen == 0x11 and mode == 2) else 12)
        mem = base(rng) + [
            (WORLD_PAGE_CELL, word(CELL)), (CELL, word(PAGE)), (0x03000080, word(PAGE)),
            (0x030056EC, word(PROFILE_AT)),
            (0x03005944, word(screen)),
            (0x03006230, struct.pack("<ii", rng.randrange(0, 0x1F000), rng.randrange(0, 0x1B000))),
            (0x03006238, bytes([cursor, pick([0, 1, 1])])),
            (0x030053B4, word(pick([0, 0x20, 0x30, rng.randrange(1 << 16)]))),
            (PROFILE_AT + 0x254, pick([0, 1, 2, 3]).to_bytes(2, "little")),
            (PROFILE_AT + 0x404, bytes([mode])),
            (PROFILE_AT + 0x205, bytes(pick([0, 0x55, 0xAA, 0xFF, rng.randrange(256)]) for _ in range(19))),
            (PROFILE_AT + 0x218, bytes(rng.randrange(256) for _ in range(60))),
            (PROFILE_AT + 0x42D, bytes(pick([0, 0xFF, rng.randrange(256)]) for _ in range(48))),
        ]
        fn = MAP_DRAW if i % 2 == 0 else KIND_HANDLERS[1][2]
        try:
            r0, writes, calls = with_stubs(MAP_STUBS, gbas[snap], fn, mem, {}, regs={"r0": 1})
        except Skip:
            continue
        cases.append(dict(snap=snap, fn=hex(fn), arg=1, args=[], mem=[[a, b.hex()] for a, b in mem], ret={}, r0=r0,
                          writes=[[a, b.hex()] for a, b in writes], calls=calls))
    return cases


def main(argv: list[str]) -> None:
    n = int(argv[0]) if argv else 1500
    gba, rng = Gba("ui-2d/n7"), random.Random(0x6A2A)
    OUT.mkdir(parents=True, exist_ok=True)
    sets = {"garage": helpers(gba, rng, n // 5) + kind18(gba, rng, n), "mapdraw": mapdraw(gba, rng, n // 3)}
    for name, cases in sets.items():
        path = OUT / f"{name}.jsonl"
        path.write_text("".join(json.dumps(c) + "\n" for c in cases), encoding="utf-8")
        print(f"{name}: {len(cases)} cases -> {path}")
