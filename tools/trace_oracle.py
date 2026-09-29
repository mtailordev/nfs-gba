"""Replay traced car steps on the game's own code in unicorn, and record what each step does to memory.

    python tools/trace_oracle.py accel drive ...   # needs <session>/<name>.csv and its memory dump (trace_race.py)

For every step of a trace (docs/engine/physics.md) this loads the traced entry state (player entity, physics
struct, logged globals) into unicorn, runs the car handler FUN_0814bd4c from the ROM and
  1. checks the result against the next traced state (the oracle's own check: must be exact), and
  2. writes <session>/<name>.oracle.txt: per step, every RAM byte the step changed and the sound calls it made.
The Rust test (crates/nfsgba-sim/tests/trace.rs) compares its own writes and sound commands with this file.

Stubbed (recorded, not run): the sound entry points, which drive the audio engine, and the wheel-sprite blit
FUN_0813bd90 (rendering). The IWRAM stack (0x03007A00-0x03007E80) is ignored. Unicorn runs as an ARMv5 core
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
# Globals logged per step by mgba_remote.lua's trace: column, address, size
GLOBALS = (("dt", 0x03005640, 4), ("phase", 0x03000048, 4), ("input", 0x030057D8, 2), ("flag610c", 0x0300610C, 4),
           ("sector", 0x03005614, 4))
# Entity bytes other game code maintains between car steps (sector-list link, view depth, byte +0x88)
EXTERNAL = {0x02, 0x03, 0x28, 0x29, 0x2A, 0x2B, 0x88}
# Stubbed calls: address -> (name, argument count)
STUBS = {0x08135FDC: ("play", 2), 0x08136028: ("stop", 1), 0x081360B4: ("pitch", 2), 0x08152E40: ("start", 4),
         0x0813BD90: ("wheel_sprite", 3)}
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
            if name != "wheel_sprite":  # rendering: not part of the simulation
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


def replay(name: str) -> int:
    work = session()
    rom = canonical()[0].read_bytes()
    u = machine(work, name, rom)
    rows = list(csv.DictReader((work / f"{name}.csv").open()))
    entity = struct.unpack("<I", u.mem_read(WORLD + 0x3C, 4))[0]
    physics = struct.unpack("<I", u.mem_read(entity + 0x8C, 4))[0]
    lines, bad = [], 0
    for i in range(len(rows) - 1):
        row, nxt = rows[i], rows[i + 1]
        u.mem_write(entity, bytes.fromhex(row["entity"]))
        u.mem_write(physics, bytes.fromhex(row["physics"]))
        for key, addr, size in GLOBALS:
            u.mem_write(addr, (int(row[key]) & 0xFFFFFFFF).to_bytes(4, "little")[:size])
        before = snapshot(u)
        calls = run_step(u, entity)
        writes = changes(before, snapshot(u))
        got_e, got_p = bytes(u.mem_read(entity, 0xA4)), bytes(u.mem_read(physics, 0x4FC))
        want_e, want_p = bytes.fromhex(nxt["entity"]), bytes.fromhex(nxt["physics"])
        if any(got_e[k] != want_e[k] for k in range(0xA4) if k not in EXTERNAL) or got_p != want_p:
            bad += 1
            print(f"{name} step {i}: the oracle does not reproduce the trace")
        lines.append(f"{i};{' '.join(f'{a:08x}={v:02x}' for a, v in writes)};{' '.join(calls)}")
    (work / f"{name}.oracle.txt").write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"{name}: {len(rows) - 1 - bad} of {len(rows) - 1} steps reproduced; wrote {name}.oracle.txt")
    return bad


if __name__ == "__main__":
    if not sys.argv[1:]:
        sys.exit(__doc__)
    sys.exit(1 if sum(replay(n) for n in sys.argv[1:]) else 0)
