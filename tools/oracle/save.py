"""Oracle cases for the profile and its save (crates/nfsgba-game/src/menu/save.rs): the game's save_decode 0x08149820,
save_encode 0x081492C0 and profile_reset 0x081356DC on generated buffers and profiles.

    .venv/Scripts/python.exe tools/oracle/cases.py save [COUNT]     # default 600 per function

Writes menus3/save.jsonl. Each case: snapshot, function, the bytes poked before the call (`mem`) and every byte the game's
code changed (`writes`); the Rust test replays them on the typed state.
"""
import json
import random

from oracle import Gba

from common import data_dir

PROFILE_AT, PROFILE_PTR, BUF = 0x0200_0808, 0x0300_56EC, 0x0201_0000
DECODE, ENCODE, RESET = 0x0814_9820, 0x0814_92C0, 0x0813_56DC
MAP_PALETTES = 0x0814_3284  # map_zone_palettes, in the same file: it is the map screens' colours (menu/map.rs)
SNAPS = ["ui-2d/lang", "ui-2d/n7", "ui-2d/n9", "race-rules/a4"]
GLOBALS = [0x0300_53E4, 0x0300_0040, 0x0300_5698, 0x0300_5798, 0x0300_578C, 0x0300_53A4, 0x0300_5600, 0x0300_0050,
           0x0300_0070]


def word(v):
    return (v & 0xFFFF_FFFF).to_bytes(4, "little")


def main(argv: list[str]) -> None:
    n = int(argv[0]) if argv else 600
    rng = random.Random(0x5A7E)
    gbas = {s: Gba(s) for s in SNAPS}
    cases = []
    for i in range(3 * n):
        snap = rng.choice(SNAPS)
        fn = (DECODE, ENCODE, RESET)[i % 3]
        buf = bytes(rng.randrange(256) for _ in range(512))
        if rng.random() < 0.3:  # sparse buffers: mostly zero or mostly set bits
            buf = bytes(b & m for b, m in zip(buf, iter(lambda: rng.choice([0, 0xFF, 0x0F, 0x80]), None)))
        prof = bytearray(rng.randrange(256) for _ in range(0x4F0))
        prof[0x494:0x496] = rng.choice([0, 0, 1, 2]).to_bytes(2, "little")
        for k in range(0x478, 0x490, 4):  # the six flags are 0/1 words (as the game stores them)
            prof[k:k + 4] = word(rng.choice([0, 1]))
        if rng.random() < 0.5:  # a realistic events field
            prof[0x205:0x217] = bytes(rng.choice([1, 2, 3, 0xFF]) for _ in range(18))
        mem = [(PROFILE_PTR, word(PROFILE_AT)), (PROFILE_AT, bytes(prof)), (BUF, buf),
               (0x0300_64C8, word(rng.randrange(256)))]
        for a in GLOBALS:
            mem.append((a, word(rng.choice([0, 1, 2, 3, 0x10, 0x18, rng.randrange(1 << 32)]))))
        mem.append((0x0300_5600, word(rng.randrange(5))))
        mem.append((0x0300_0070, word(rng.randrange(16))))
        r = gbas[snap].call(fn, r0=BUF if fn != RESET else 0, mem=mem, align="ignore")
        assert r.stop == "return", (hex(fn), r.stop)
        cases.append(dict(snap=snap, fn=hex(fn), mem=[[a, b.hex()] for a, b in mem],
                          writes=[[a, b.hex()] for a, b in r.writes]))
    for i in range(n):  # map_zone_palettes: screens, cursor, grid, unlock bits, flash, map mode
        snap = rng.choice(SNAPS)
        screen = rng.choice([7, 8, 0xE, 0x11, 0x11, 8, 7, 0xE, 3])
        mem = [(PROFILE_PTR, word(PROFILE_AT)), (0x0300_5944, word(screen)),
               (0x0300_6238, bytes([rng.choice([0, 1, 2, 3, 5, 8, 11, 12, 17, 0xFF, rng.randrange(18)])])),
               (0x0300_53B4, word(rng.choice([0, 0x20, 0x30, rng.randrange(1 << 16)]))),
               (PROFILE_AT + 0x254, rng.choice([0, 0, 1, 2, 3]).to_bytes(2, "little")),
               (PROFILE_AT + 0x404, bytes([rng.randrange(4)])),
               (PROFILE_AT + 0x42D, bytes(rng.choice([0, 0xFF, rng.randrange(256)]) for _ in range(48)))]
        r = gbas[snap].call(MAP_PALETTES, mem=mem, align="ignore")
        assert r.stop == "return", r.stop
        cases.append(dict(snap=snap, fn=hex(MAP_PALETTES), mem=[[a, b.hex()] for a, b in mem],
                          writes=[[a, b.hex()] for a, b in r.writes]))
    out = data_dir() / "work" / "e5298b24" / "menus3"
    out.mkdir(parents=True, exist_ok=True)
    (out / "save.jsonl").write_text("".join(json.dumps(c) + "\n" for c in cases), encoding="utf-8")
    print(f"{len(cases)} cases -> {out / 'save.jsonl'}")
