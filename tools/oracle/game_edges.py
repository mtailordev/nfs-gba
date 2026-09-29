"""Reference states for the game loop's sound stops (docs/engine/game-loop.md, crates/nfsgba-game/tests/edges.rs).

    .venv/Scripts/python.exe tools/oracle/cases.py game-edges

Runs snd_stop_all (0x08135f38) and music_stop (0x0813609c) in the function oracle on the reference race
(mgba/race: music and the engine loop playing) and saves the results as game-loop/edges_NAME.<domain>.bin.
"""
import sys

from common import data_dir
from oracle import REGIONS, Gba

WORK = data_dir() / "work" / "e5298b24" / "game-loop"
CALLS = {"stop_all": 0x08135F38, "music_stop": 0x0813609C}


def main(argv: list[str]) -> None:
    gba = Gba("mgba/race")
    for name, fn in CALLS.items():
        r = gba.call(fn)
        if r.stop != "return":
            sys.exit(f"{name}: the oracle stopped: {r.stop} {r.notes}")
        for dom, base, size in REGIONS:
            if dom != "bios":
                (WORK / f"edges_{name}.{dom}.bin").write_bytes(r.read(base, size))
        print(f"{name}: {len(r.writes)} writes")
