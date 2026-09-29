"""Record game-frame traces for the game loop (docs/engine/game-loop.md) and store them compactly.

    .venv/Scripts/python.exe tools/game_trace.py record drive        # mGBA running with mgba_game_trace.lua
    .venv/Scripts/python.exe tools/game_trace.py pack drive          # NAME.frames.bin -> NAME.base.bin + NAME.delta

`record` loads race.ss (copy it into the session directory first), arms tools/mgba_game_trace.lua for the
scenario's game frames and plays its keys. `pack` keeps the first machine state as NAME.base.bin and every later
state as the byte runs that differ from the one before (NAME.delta: per state a u32 run count, then per run u32
offset, u32 length and the bytes), then deletes NAME.frames.bin. A state is EWRAM, IWRAM, palette, VRAM and OAM
(0x40000 + 0x8000 + 0x400 + 0x18000 + 0x400 bytes). Rerunning gives identical files (the emulator is
deterministic from the savestate).
"""
import os
import struct
import sys
import time

import numpy as np

import mgba_ctl
from common import data_dir

STATE = 0x40000 + 0x8000 + 0x400 + 0x18000 + 0x400
SCENARIOS = {
    # From the reference race (race.ss): accelerate, steer both ways, brake, accelerate again.
    "drive": (150, ["wait 2", "hold A 200", "hold A,LEFT 40", "hold A 100", "hold A,RIGHT 40", "hold B 40",
                    "hold A 400"]),
}


def session():
    return data_dir() / "work" / "e5298b24" / (os.environ.get("NFSGBA_MGBA_SESSION") or "mgba")


def record(name):
    frames, keys = SCENARIOS[name]
    work = session()
    mgba_ctl.main(["load race"])
    (work / "gtrace.tmp").write_text(f"{name} {frames}\n")
    (work / "gtrace.tmp").replace(work / "gtrace.txt")
    mgba_ctl.main(keys)
    log = work / "gtrace_log.txt"
    for _ in range(600):
        if log.exists() and f"recorded {name}" in log.read_text():
            return
        time.sleep(0.5)
    sys.exit(f"{name}: not finished (see {log})")


def pack(name):
    work = session()
    raw = np.fromfile(work / f"{name}.frames.bin", dtype=np.uint8)
    assert raw.size % STATE == 0, "partial state"
    states = raw.reshape(-1, STATE)
    states[0].tofile(work / f"{name}.base.bin")
    with open(work / f"{name}.delta", "wb") as out:
        for prev, cur in zip(states, states[1:]):
            diff = np.flatnonzero(prev != cur)
            runs = []
            if diff.size:
                breaks = np.flatnonzero(np.diff(diff) > 8) + 1
                for part in np.split(diff, breaks):
                    runs.append((int(part[0]), int(part[-1]) + 1))
            out.write(struct.pack("<I", len(runs)))
            for a, b in runs:
                out.write(struct.pack("<II", a, b - a))
                out.write(cur[a:b].tobytes())
    (work / f"{name}.frames.bin").unlink()
    print(f"{name}: {len(states)} states packed")


if __name__ == "__main__":
    {"record": record, "pack": pack}[sys.argv[1]](sys.argv[2])
