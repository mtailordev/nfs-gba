"""The one oracle case CLI: runs the game's own code in the function oracle (oracle.py) and writes the cases or
reference states the Rust tests replay. Each set is a module here; see its docstring for arguments and outputs.

    .venv/Scripts/python.exe tools/oracle/cases.py list
    .venv/Scripts/python.exe tools/oracle/cases.py SET [ARGS...]

Sets (output folder under $NFSGBA_DATA/work/<sha8>/): unlock (menus), save (menus3), render2 (render2), prove (harness/oracle), car, fuzz, calls, suspension
(vehicle-physics), ai (ai-traffic), rules (race-rules), menus (menus, menus3), draw (menus3), race-init, race-init-inputs, game-edges
(race-init). Regenerating a set gives byte-identical files (seeded inputs, deterministic oracle).
"""
import importlib
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path[:0] = [str(HERE), str(HERE.parent)]  # the set modules import `oracle` and the tools' `common`/`mgba_ctl`

SETS = {"prove": "prove", "car": "car", "fuzz": "fuzz", "calls": "calls", "suspension": "suspension", "ai": "ai",
        "rules": "rules", "unlock": "unlock", "synth": "synth", "menus": "menus", "draw": "draw", "fill-rect-hw": "fill_rect_hw",
        "race-init": "race_init", "race-init-inputs": "race_init_inputs", "game-edges": "game_edges", "game-countdown": "game_countdown",
        "game-end": "game_end", "garage": "garage", "upgrades": "upgrades", "turntable": "turntable", "scene": "scene", "power-on": "scene_power_on", "power-frames": "scene_frames", "save": "save",
        "render2": "walls64"}


def module(name: str):
    if name not in SETS:
        sys.exit(f"unknown set {name!r}; known: {', '.join(SETS)}")
    return importlib.import_module(SETS[name])


def main(argv: list[str]) -> None:
    if not argv or argv[0] == "list":
        print("\n".join(f"{k}: {(module(k).__doc__ or '').strip().splitlines()[0]}" for k in SETS))
    else:
        module(argv[0]).main(argv[1:])


if __name__ == "__main__":
    main(sys.argv[1:])
