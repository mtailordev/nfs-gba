"""Record the player car's per-step trace for scripted driving scenarios (docs/engine/physics.md).

    python tools/trace_race.py                 # record every scenario (mGBA must be running: mgba_ctl.py start)
    python tools/trace_race.py accel brake     # record some
    python tools/trace_race.py --summary NAME  # print a recorded trace's key fields

Each scenario loads race.ss from the session directory (copy it from data/work/<sha8>/mgba/), traces the car
handler with mgba_remote.lua's `trace` and writes <session>/<name>.csv plus the memory dump of its first step.
Rerunning gives identical files: the emulator is deterministic from the savestate.
"""
import csv
import os
import subprocess
import sys
from pathlib import Path

SCENARIOS = {
    "accel": ["hold A 240"],
    "brake": ["hold A 150", "hold B 60", "wait 30", "hold B 90"],
    "steer": ["hold A 90", "hold A,LEFT 60", "hold A,RIGHT 60", "hold A 45"],
    "wall": ["hold A 90", "hold A,RIGHT 150", "hold A 60"],
    "drive": ["hold A 200", "hold A,LEFT 40", "hold A 120", "hold A,R 60", "hold A,L 60", "hold R,L 30",
              "hold A,RIGHT 40", "hold B 60", "hold A 200"],
    "reverse": ["hold B 240", "hold B,LEFT 60", "wait 20", "hold A 90"],
    "handbrake": ["hold A 150", "hold A,R,LEFT 50", "hold A,RIGHT 40", "hold A,R,RIGHT 40", "hold A 60"],
    "long": ["hold A 180", "hold A,LEFT 30", "hold A 90", "hold A,RIGHT 45", "hold A 120", "hold A,LEFT 60",
             "hold A 150", "hold A,RIGHT 30", "hold A 200", "hold A,LEFT 40", "hold A 150"],
}

# Entity 0 fields for --summary: name, offset, size (see docs/engine/physics.md)
FIELDS = [("x", 0x0C, 4), ("y", 0x10, 4), ("z", 0x14, 4), ("heading", 0x2C, 4), ("sector", 0x78, 2)]


def run(names: list[str]) -> None:
    ctl = [sys.executable, str(Path(__file__).with_name("mgba_ctl.py"))]
    for name in names:
        subprocess.run([*ctl, "load race", "wait 2", f"trace {name}", *SCENARIOS[name], "untrace"], check=True)
        print("recorded", name)


def summary(path: Path) -> None:
    with path.open(encoding="utf-8") as f:
        for row in csv.DictReader(f):
            e = bytes.fromhex(row["entity"])
            vals = [f"{n}={int.from_bytes(e[o:o + s], 'little', signed=True)}" for n, o, s in FIELDS]
            print(row["frame"], f"input={int(row['input']):#06x}", f"dt={row['dt']}", *vals)


if __name__ == "__main__":
    args = sys.argv[1:]
    if args[:1] == ["--summary"]:
        from mgba_ctl import canonical, data_dir

        work = data_dir() / "work" / canonical()[1] / (os.environ.get("NFSGBA_MGBA_SESSION") or "mgba")
        summary(work / f"{args[1]}.csv")
    else:
        unknown = [a for a in args if a not in SCENARIOS]
        if unknown:
            sys.exit(f"unknown scenarios {unknown}; known: {list(SCENARIOS)}")
        run(args or list(SCENARIOS))
