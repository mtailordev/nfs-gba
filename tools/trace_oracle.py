"""Replay traced car steps on the game's own code in unicorn, and record what each step does to memory.

    python tools/trace_oracle.py accel drive ...   # needs the trace_race.py outputs of each scenario

For every step of a trace (docs/engine/physics.md) this loads the full RAM the reference build had at the step's
entry (<name>.ramdelta) into unicorn, runs the car handler FUN_0814bd4c from the ROM and
  1. checks the result against the next traced state (the oracle's own check: must be exact), and
  2. writes <session>/<name>.oracle.txt: per step, every RAM byte the step changed and the sound calls it made.
The Rust test (crates/nfsgba-sim/tests/trace.rs) compares its own writes and sound commands with this file.

Stubbed (recorded, not run): the sound entry points, which drive the audio engine, and the decal blit onto the car
atlas FUN_0813bd90 (rendering). The IWRAM stack (0x03007A00-0x03007E80) is ignored. Unicorn runs as an ARMv5 core
(unaligned loads rotate like the ARM7TDMI); SWI 6 (Div) is emulated.
"""
import csv
import os
import struct
import sys
from pathlib import Path

import numpy as np
from unicorn import UC_ARCH_ARM, UC_HOOK_CODE, UC_HOOK_INTR, UC_MODE_THUMB, Uc
from unicorn.arm_const import (UC_ARM_REG_LR, UC_ARM_REG_PC, UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2,
                               UC_ARM_REG_R3, UC_ARM_REG_SP, UC_CPU_ARM_926)

sys.path.insert(0, str(Path(__file__).resolve().parent))
from mgba_ctl import canonical, data_dir  # noqa: E402

WORLD = 0x030000C0
HANDLER = 0x0814BD4C
RET = 0x08000000
SP = 0x03007E80
STACK = range(0x03007A00, SP)
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
REGIONS = ((0x02000000, 0x40000, "wram"), (0x03000000, 0x8000, "iwram"))


def session() -> Path:
    return data_dir() / "work" / canonical()[1] / (os.environ.get("NFSGBA_MGBA_SESSION") or "vehicle-physics")


def machine(work: Path, name: str, rom: bytes) -> Uc:
    u = Uc(UC_ARCH_ARM, UC_MODE_THUMB)
    u.ctl_set_cpu_model(UC_CPU_ARM_926)
    for base, size, dom in ((0x00000000, 0x4000, "bios"), *REGIONS, (0x04000000, 0x1000, "io"),
                            (0x05000000, 0x1000, "palette"), (0x06000000, 0x18000, "vram"), (0x07000000, 0x1000, "oam")):
        u.mem_map(base, max(size, 0x1000))
        u.mem_write(base, (work / f"{name}.{dom}.bin").read_bytes()[:size])
    u.mem_map(0x08000000, 0x800000)
    u.mem_write(0x08000000, rom)
    return u


def run_step(u: Uc, entity: int) -> list[str]:
    """Run the car handler once; returns the stubbed calls made."""
    calls = []

    def code(uc, addr, size, _):
        if addr in STUBS:
            name, n = STUBS[addr]
            regs = [uc.reg_read(r) for r in (UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3)]
            args = [struct.unpack("<i", struct.pack("<I", v))[0] for v in regs[:n]]
            if name != "decal":  # rendering: not part of the simulation
                calls.append(f"{name}({','.join(map(str, args))})")
            uc.reg_write(UC_ARM_REG_PC, uc.reg_read(UC_ARM_REG_LR))

    def swi(uc, intno, _):
        pc = uc.reg_read(UC_ARM_REG_PC)
        n = uc.mem_read(pc - 2, 1)[0]
        if n != 6:
            raise RuntimeError(f"swi {n:#x} at {pc:#x}")
        s32 = lambda v: struct.unpack("<i", struct.pack("<I", v))[0]  # noqa: E731
        a, b = s32(uc.reg_read(UC_ARM_REG_R0)), s32(uc.reg_read(UC_ARM_REG_R1))
        q = int(a / b) if b else 0
        uc.reg_write(UC_ARM_REG_R0, q & 0xFFFFFFFF)
        uc.reg_write(UC_ARM_REG_R1, (a - q * b) & 0xFFFFFFFF)
        uc.reg_write(UC_ARM_REG_R3, abs(q) & 0xFFFFFFFF)

    u.reg_write(UC_ARM_REG_R0, WORLD)
    u.reg_write(UC_ARM_REG_R1, entity)
    u.reg_write(UC_ARM_REG_SP, SP)
    u.reg_write(UC_ARM_REG_LR, RET | 1)
    hooks = [u.hook_add(UC_HOOK_CODE, code, begin=0x08135000, end=0x08153000), u.hook_add(UC_HOOK_INTR, swi)]
    try:
        u.emu_start(HANDLER | 1, RET, count=50_000_000)
    finally:
        for h in hooks:
            u.hook_del(h)
    return calls


def snapshot(u: Uc) -> list[np.ndarray]:
    return [np.frombuffer(bytes(u.mem_read(base, size)), dtype=np.uint8) for base, size, _ in REGIONS]


def changes(before: list[np.ndarray], after: list[np.ndarray]) -> list[tuple[int, int]]:
    out = []
    for (base, _, _), a, b in zip(REGIONS, before, after):
        for o in np.nonzero(a != b)[0]:
            if base + int(o) not in STACK:
                out.append((base + int(o), int(b[o])))
    return out


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
    rom = canonical()[0].read_bytes()
    u = machine(work, name, rom)
    with (work / f"{name}.csv").open(encoding="utf-8") as f:
        rows = list(csv.DictReader(f))
    entity = struct.unpack("<I", u.mem_read(WORLD + 0x3C, 4))[0]
    lines, bad = [], 0
    for i, state in zip(range(len(rows) - 1), ram_states(work, name)):
        nxt = rows[i + 1]
        # The whole game state at the step's entry, as the reference build had it.
        u.mem_write(0x02000000, state[:0x40000].tobytes())
        u.mem_write(0x03000000, state[0x40000:].tobytes())
        before = snapshot(u)
        calls = run_step(u, entity)
        writes = changes(before, snapshot(u))
        physics = struct.unpack("<I", u.mem_read(entity + 0x8C, 4))[0]
        got_e, got_p = bytes(u.mem_read(entity, 0xA4)), bytes(u.mem_read(physics, 0x4FC))
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
