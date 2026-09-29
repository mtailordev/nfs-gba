"""Reference states for the race start's countdown (docs/engine/game-loop.md, crates/nfsgba-game/tests/edges.rs).

    .venv/Scripts/python.exe tools/oracle/cases.py game-countdown

Runs race_start_from_table_b (0x0813af7c) in the function oracle on the reference race (mgba/race) with the state
it reads set per case (phase 0x03000048, fade 0x03005630, start state 0x03005714, the countdown accumulator
0x030000AC, the frame time 0x03005640, DISPCNT) and saves the result as game-loop/countdown_NAME.<domain>.bin.
The cases are listed in game-loop/countdown_cases.txt (NAME phase fade start acc dt dispcnt), which the Rust test reads.
"""
import struct
import sys

from common import data_dir
from oracle import REGIONS, Gba

WORK = data_dir() / "work" / "e5298b24" / "game-loop"
FN = 0x0813AF7C
# name, phase, fade, start state, accumulator, frame time, DISPCNT
CASES = [
    ("intro", 9, 0, 0, 0x0100, 25, 0x1040),
    ("intro_fast", 9, 0, 0, 0x0100, 10, 0x1040),
    ("digit1", 9, 0, 0, 0x3F00, 25, 0x1040),
    ("digit2", 1, 0, 1, 0x3F00, 25, 0x1040),
    ("go", 9, 0, 2, 0x3F00, 25, 0x1040),
    ("go_phase1", 1, 0, 2, 0x3F00, 100, 0x1040),
    ("tiles", 2, 0, 3, 0, 25, 0x1040),
    ("tiles_2d_off", 2, 0, 3, 0, 25, 0x1000),
    ("fading", 9, 8, 0, 0x3F00, 25, 0x1040),
    ("racing", 2, 0, 4, 0, 25, 0x1040),
]


def main(argv: list[str]) -> None:
    lines = []
    for name, phase, fade, start, acc, dt, dispcnt in CASES:
        gba = Gba("mgba/race")
        for addr, v in ((0x03000048, phase), (0x03005630, fade), (0x03005714, start), (0x030000AC, acc),
                        (0x03005640, dt)):
            gba.poke(addr, struct.pack("<i", v))
        gba.poke(0x04000000, struct.pack("<H", dispcnt))
        gba.poke(0x06013000, bytes(0x5000))  # the OBJ tiles the uploads fill, so that a missing upload shows
        r = gba.call(FN)
        if r.stop != "return":
            sys.exit(f"{name}: the oracle stopped: {r.stop} {r.notes}")
        for dom, base, size in REGIONS:
            if dom != "bios":
                (WORK / f"countdown_{name}.{dom}.bin").write_bytes(r.read(base, size))
        lines.append(f"{name} {phase} {fade} {start} {acc} {dt} {dispcnt}")
        print(f"{name}: {len(r.writes)} writes")
    (WORK / "countdown_cases.txt").write_text("\n".join(lines) + "\n")
