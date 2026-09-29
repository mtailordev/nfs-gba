"""Reference states for the race end (docs/engine/game-loop.md, crates/nfsgba-game/tests/edges.rs).

    .venv/Scripts/python.exe tools/oracle/cases.py game-end

Runs fill_results (0x0812eaac, with results_tiebreak 0x0812e9e8 and finish_time_estimate 0x0814f050 below it) and
race_menu_palette_setup (0x081372e4) in the function oracle on the reference race (mgba/race) with the state they
read set per case, and saves the results as game-loop/end_NAME.<domain>.bin. The fill_results cases are listed in
game-loop/end_cases.txt: `NAME phase opponents circuit laps time` then per racer `finish best_lap progress life
laps_left ranked` (the results' finish time, the driver's best lap `+0xB4`, distance `+0xAC`, hunter life `+0x4E8`,
laps left `+0xC5`, and the ranked block's finish time `0x03005750 + 4 * racer`), which the Rust test reads.
"""
import random
import struct
import sys

from common import data_dir
from oracle import REGIONS, Gba

WORK = data_dir() / "work" / "e5298b24" / "game-loop"
WORLD = 0x030000C0
FILL, PALETTE_SETUP = 0x0812EAAC, 0x081372E4


def u32(gba, addr):
    return struct.unpack("<I", gba.read_base(addr, 4))[0]


def poke(gba, addr, fmt, v):
    gba.poke(addr, struct.pack(fmt, v))


def save(gba, name, r):
    for dom, base, size in REGIONS:
        if dom != "bios":
            (WORK / f"end_{name}.{dom}.bin").write_bytes(r.read(base, size))


def cases():
    rng = random.Random(7)
    out = []
    for k, (phase, opponents, circuit, laps) in enumerate(
            [(3, 3, 1, 3), (3, 3, 0, 1), (8, 2, 1, 2), (3, 1, 1, 4), (2, 3, 1, 3), (3, 3, 1, 3), (3, 2, 0, 1)]):
        racers = []
        for i in range(4):
            racers.append((rng.choice([0, 0, rng.randrange(1, 60000)]), rng.choice([0, rng.randrange(1, 9000)]),
                           rng.randrange(1, 3000), rng.randrange(0, 0x80001), rng.randrange(1, laps + 1),
                           rng.choice([5000, 5000, rng.randrange(1, 60000)]) if k >= 4 else rng.randrange(0, 9)))
        out.append((f"c{k}", phase, opponents, circuit, laps, rng.randrange(600, 20000), racers))
    return out


def main(argv: list[str]) -> None:
    lines = []
    for name, phase, opponents, circuit, laps, time, racers in cases():
        gba = Gba("mgba/race")
        for addr, v in ((0x03000048, phase), (0x03005784, opponents), (0x0300608C, circuit), (0x030056E4, laps),
                        (0x03005800, time)):
            poke(gba, addr, "<I", v)
        entities = u32(gba, WORLD + 0x3C)
        for i, (finish, best, progress, life, left, ranked) in enumerate(racers):
            driver = u32(gba, entities + 0xA4 * i + 0x8C)
            poke(gba, 0x03005650 + 0x20 + 4 * i, "<I", finish)
            poke(gba, 0x03005750 + 4 * i, "<I", ranked)
            poke(gba, driver + 0xB4, "<I", best)
            poke(gba, driver + 0xAC, "<i", progress)
            poke(gba, driver + 0x4E8, "<i", life)
            poke(gba, driver + 0xC5, "<b", left)
        r = gba.call(FILL, r0=WORLD)
        if r.stop != "return":
            sys.exit(f"{name}: the oracle stopped: {r.stop} {r.notes}")
        save(gba, name, r)
        flat = " ".join(str(v) for row in racers for v in row)
        lines.append(f"{name} {phase} {opponents} {circuit} {laps} {time} {flat}")
        print(f"{name}: {len(r.writes)} writes")
    (WORK / "end_cases.txt").write_text("\n".join(lines) + "\n")
    # The resume from the pause menu: DISPSTAT's VCount IRQ off and the gradient start moved beforehand.
    for name, phase in (("palette_setup", 1), ("palette_setup_preview", 5)):
        gba = Gba("mgba/race")
        poke(gba, 0x03000048, "<I", phase)
        poke(gba, 0x04000004, "<H", 0x0008)
        poke(gba, 0x030056E8, "<I", u32(gba, 0x030053B8) + 40)
        gba.poke(0x06000000, bytes([7]) * 0x9600)
        gba.poke(0x0600A000, bytes([7]) * 0x9600)
        r = gba.call(PALETTE_SETUP, r0=WORLD)
        if r.stop != "return":
            sys.exit(f"{name}: the oracle stopped: {r.stop} {r.notes}")
        save(gba, name, r)
        print(f"{name}: {len(r.writes)} writes")
