"""Record the player car's per-step trace for scripted driving scenarios (docs/engine/physics.md).

    python tools/trace_race.py                 # record every scenario (mGBA must be running: mgba_ctl.py start)
    python tools/trace_race.py accel brake     # record some
    python tools/trace_race.py --summary NAME  # print a recorded trace's key fields

Scenarios run in the session directory: copy race.ss and mainmenu.ss there from data/work/<sha8>/mgba/. A scenario
without its own `trace` command loads race.ss and traces all of its commands; one with `trace` (the race start)
runs as written. Each writes <session>/<name>.csv, the memory dump of its first traced step and <name>.ramdelta
(the full RAM at every step, as differences from that dump). Rerunning gives identical files: the emulator is
deterministic from the savestate.
"""
import csv
import os
import struct
import subprocess
import sys
from pathlib import Path

# The Quick Play menu route from mainmenu.ss (docs/TOOLS.md): Quick Play, Random, confirm, then the race info
# screen's A starts the race (a 3-lap circuit, Mazda RX-7, heavy traffic).
MENU_TO_RACE = ["load mainmenu", "wait 30", "hold A 10", "wait 60", "hold A 10", "wait 60", "hold A 10", "wait 60"]
# The autopilot (tools/trace_autopilot.lua) drives the longer scenarios; each starts from its defaults.
AUTOPILOT = [f"lua {Path(__file__).resolve().with_name('trace_autopilot.lua').as_posix()}", "luax AUTOPILOT.reset()"]
# Race-info screens saved from a fresh profile (docs/engine/physics.md, "Scenarios"): hunter-info (Quick Play
# Random: hunter, Southside, Mazda RX-7, easy, 3 opponents, heavy traffic).

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
    # From the race info screen: the car's init step, the intro, the launch among the opponents, traffic.
    "start": [*MENU_TO_RACE, "trace", "hold A 10", "wait 100", "hold A 700"],
    # Hunter race: the autopilot rams opponent 3 (car-to-car responses, hunter hits and wall hits; traffic hits
    # the player twice between steps).
    "hunter": ["load hunter-info", *AUTOPILOT, "wait 30", "trace", "hold A 10", "wait 60", "luax AUTOPILOT.target=3",
               'luax AUTOPILOT.mode="ram"', "wait 3000", 'luax AUTOPILOT.mode="off"'],
}

# Entity 0 fields for --summary: name, offset, size (see docs/engine/physics.md)
FIELDS = [("x", 0x0C, 4), ("y", 0x10, 4), ("z", 0x14, 4), ("heading", 0x2C, 4), ("state", 0x4A, 2),
          ("sector", 0x78, 2)]


def commands(name: str) -> list[str]:
    """The scenario's mgba_remote commands; `trace [SKIP]` becomes `trace NAME [SKIP]`."""
    steps = SCENARIOS[name]
    if not any(c.split()[0] == "trace" for c in steps):
        steps = ["load race", "wait 2", "trace", *steps]
    return [" ".join(["trace", name, *c.split()[1:]]) if c.split()[0] == "trace" else c for c in steps] + ["untrace"]


RAM = 0x48000  # EWRAM (0x40000) then IWRAM (0x8000), as mgba_remote.lua appends them per step
MAGIC = b"RAMD"


def ram_deltas(work: Path, name: str) -> None:
    """<name>.ram.bin (full RAM per step) -> <name>.ramdelta: per step, the byte runs that differ from the first
    step's dump. Format: "RAMD", u32 steps; per step u32 runs, then per run u32 offset, u32 length, bytes
    (offsets into EWRAM followed by IWRAM)."""
    import numpy as np

    raw = work / f"{name}.ram.bin"
    data = np.fromfile(raw, dtype=np.uint8).reshape(-1, RAM)
    first = np.concatenate([np.fromfile(work / f"{name}.wram.bin", dtype=np.uint8),
                            np.fromfile(work / f"{name}.iwram.bin", dtype=np.uint8)])
    assert (data[0] == first).all(), "the first step's RAM should equal the dump"
    out = [MAGIC, struct.pack("<I", len(data))]
    for step in data:
        idx = np.nonzero(step != first)[0]
        runs = []
        if len(idx):
            # Merge differences closer than 8 bytes into one run.
            breaks = np.nonzero(np.diff(idx) > 8)[0]
            starts = np.concatenate([[idx[0]], idx[breaks + 1]])
            ends = np.concatenate([idx[breaks], [idx[-1]]]) + 1
            runs = list(zip(starts.tolist(), ends.tolist()))
        out.append(struct.pack("<I", len(runs)))
        for s, e in runs:
            out.append(struct.pack("<II", s, e - s) + step[s:e].tobytes())
    (work / f"{name}.ramdelta").write_bytes(b"".join(out))
    raw.unlink()


def run(names: list[str]) -> None:
    from mgba_ctl import canonical, data_dir

    work = data_dir() / "work" / canonical()[1] / (os.environ.get("NFSGBA_MGBA_SESSION") or "mgba")
    ctl = [sys.executable, str(Path(__file__).with_name("mgba_ctl.py"))]
    for name in names:
        subprocess.run([*ctl, *commands(name)], check=True)
        ram_deltas(work, name)
        print("recorded", name)


def summary(path: Path) -> None:
    with path.open(encoding="utf-8") as f:
        for row in csv.DictReader(f):
            e = bytes.fromhex(row["entity"])
            vals = [f"{n}={int.from_bytes(e[o:o + s], 'little', signed=True)}" for n, o, s in FIELDS]
            print(row["frame"], f"phase={row['phase']}", f"input={int(row['input']):#06x}", f"dt={row['dt']}", *vals)


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
