"""Replay traced car steps on the game's own code in unicorn, and record what each step does to memory.

    python tools/trace_oracle.py accel drive ...   # needs the trace_race.py outputs of each scenario

For every step of a trace (docs/engine/physics.md) this loads the full RAM the reference build had at the step's
entry (<name>.ramdelta) into the function oracle (tools/oracle/oracle.py: memory map, BIOS calls, return trap,
write diff), runs the car handler FUN_0814bd4c from the ROM and
  1. checks the result against the next traced state (the oracle's own check: must be exact), and
  2. writes <session>/<name>.oracle.txt: per step, every RAM byte the step changed and the sound calls it made.
The Rust test (crates/nfsgba-sim/tests/trace.rs) compares its own writes and sound commands with this file.

Stubbed (recorded, not run): the sound entry points, which drive the audio engine, and the decal blit onto the car
atlas FUN_0813bd90 (rendering). The handler's own stack frames are left out. Unaligned accesses would stop the
step: unicorn does not rotate them like the ARM7TDMI.
"""
import csv
import os
import struct
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent / "oracle"))
from oracle import REGS, Gba, canonical, data_dir  # noqa: E402

WORLD = 0x030000C0
HANDLER = 0x0814BD4C
SP = 0x03007E80
# Bits other game code maintains between car steps, as {offset: mask}: entity sector-list link, draw-list link,
# flags +0x0A bit 2, view depth, byte +0x88; physics race position (the ranking)
EXTERNAL = {0x02: 0xFF, 0x03: 0xFF, 0x04: 0xFF, 0x05: 0xFF, 0x0A: 0x04, 0x28: 0xFF, 0x29: 0xFF, 0x2A: 0xFF,
            0x2B: 0xFF, 0x88: 0xFF}
PHYSICS_EXTERNAL = {0xA8: 0xFF, 0xA9: 0xFF, 0xAA: 0xFF, 0xAB: 0xFF}


def same(got: bytes, want: bytes, external: dict) -> bool:
    return all((g ^ w) & ~external.get(k, 0) & 0xFF == 0 for k, (g, w) in enumerate(zip(got, want)))


# Stubbed calls: address -> (name, argument count)
STUBS = {0x08135FDC: ("play", 2), 0x08136028: ("stop", 1), 0x081360B4: ("pitch", 2), 0x08152E40: ("start", 4),
         0x0813BD90: ("decal", 3)}
RAM = (0x02000000, 0x03008000)  # EWRAM and IWRAM: the writes this file records


def session() -> Path:
    return data_dir() / "work" / canonical()[1] / (os.environ.get("NFSGBA_MGBA_SESSION") or "vehicle-physics")


def run_step(gba: Gba, entity: int):
    """Run the car handler once on the oracle's current state; returns the result and the stubbed calls made."""
    calls = []

    def stub(name, n):
        def record(uc):
            args = [struct.unpack("<i", struct.pack("<I", uc.reg_read(REGS[f"r{k}"])))[0] for k in range(n)]
            if name != "decal":  # rendering: not part of the simulation
                calls.append(f"{name}({','.join(map(str, args))})")
        return record

    r = gba.call(HANDLER, mode="thumb", r0=WORLD, r1=entity, sp=SP, max_insns=50_000_000,
                 stubs={a: stub(name, n) for a, (name, n) in STUBS.items()})
    if r.stop != "return":
        raise RuntimeError(f"the car handler stopped: {r.stop}")
    return r, calls


def ram_states(work: Path, name: str):
    """The full EWRAM + IWRAM at each traced step, from <name>.ramdelta (tools/trace_race.py)."""
    first = np.concatenate([np.fromfile(work / f"{name}.wram.bin", dtype=np.uint8),
                            np.fromfile(work / f"{name}.iwram.bin", dtype=np.uint8)])
    data = (work / f"{name}.ramdelta").read_bytes()
    assert data[:4] == b"RAMD"
    steps, at = struct.unpack_from("<I", data, 4)[0], 8
    for _ in range(steps):
        state = first.copy()
        runs, at = struct.unpack_from("<I", data, at)[0], at + 4
        for _ in range(runs):
            off, n = struct.unpack_from("<II", data, at)
            state[off:off + n] = np.frombuffer(data, dtype=np.uint8, count=n, offset=at + 8)
            at += 8 + n
        yield state


def replay(name: str) -> int:
    work = session()
    gba = Gba(f"{work.name}/{name}")
    with (work / f"{name}.csv").open(encoding="utf-8") as f:
        rows = list(csv.DictReader(f))
    entity = struct.unpack("<I", gba.read_base(WORLD + 0x3C, 4))[0]
    lines, bad = [], 0
    for i, state in zip(range(len(rows) - 1), ram_states(work, name)):
        nxt = rows[i + 1]
        # The whole game state at the step's entry, as the reference build had it.
        gba.poke(0x02000000, state[:0x40000].tobytes())
        gba.poke(0x03000000, state[0x40000:].tobytes())
        r, calls = run_step(gba, entity)
        writes = [(a + k, v) for a, b in r.writes if RAM[0] <= a < RAM[1] for k, v in enumerate(b)]
        physics = struct.unpack("<I", r.read(entity + 0x8C, 4))[0]
        got_e, got_p = r.read(entity, 0xA4), r.read(physics, 0x4FC)
        want_e, want_p = bytes.fromhex(nxt["entity"]), bytes.fromhex(nxt["physics"])
        line = f"{i};{' '.join(f'{a:08x}={v:02x}' for a, v in writes)};{' '.join(calls)}"
        if not same(got_e, want_e, EXTERNAL) or not same(got_p, want_p, PHYSICS_EXTERNAL):
            # Other code changed the car between this step and the next (a traffic car's collision response,
            # FUN_08146094 -> FUN_08145dac, in the hunter trace). Checked with a breakpoint for each such step;
            # the Rust test then compares only this step's own writes and sounds.
            bad += 1
            line += ";external"
            print(f"{name} step {i}: the car changed between steps (marked external)")
        lines.append(line)
    (work / f"{name}.oracle.txt").write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"{name}: {len(rows) - 1 - bad} of {len(rows) - 1} steps reproduced, {bad} external; wrote {name}.oracle.txt")
    return 0


if __name__ == "__main__":
    if not sys.argv[1:]:
        sys.exit(__doc__)
    sys.exit(1 if sum(replay(n) for n in sys.argv[1:]) else 0)
