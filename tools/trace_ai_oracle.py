"""Replay the opponents' and traffic cars' steps on the game's own code in unicorn (docs/engine/ai.md).

    python tools/trace_ai_oracle.py start accel ...   # scenarios recorded by tools/trace_race.py

The traces hold the full RAM at the entry of the player's car handler, once per game frame. That call is the first
of `update_entities` (FUN_0813765c), which then calls the handler of every later entity whose flags +0x08 & 3 == 3
(table 0x087F38B8 by entity +0x4E: 0x29 opponents, 0x36 traffic). For each step this runs that loop in unicorn on
the step's RAM and
  1. checks every non-player car against the next traced step (the oracle's own check; fields other game code
     maintains are masked as in trace_oracle.py), and
  2. writes <out>/<name>.ai-oracle.txt: per step, every handler call of the loop (the player's too) with every RAM
     byte it changed and its stubbed calls, as `step;entity;handler;writes;calls`. The Rust test replays the
     other handlers' writes and checks its own opponents and traffic against theirs.
It also prints which game functions the non-player handlers ran (`--coverage`).

Traces are read from $NFSGBA_TRACE_SESSION (default vehicle-physics); output goes to $NFSGBA_MGBA_SESSION
(default ai-traffic).
"""
import os
import struct
import sys
from collections import Counter
from pathlib import Path

from unicorn import UC_HOOK_BLOCK, UC_HOOK_CODE, UC_HOOK_INTR
from unicorn.arm_const import UC_ARM_REG_LR, UC_ARM_REG_PC, UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_SP

sys.path.insert(0, str(Path(__file__).resolve().parent))
import trace_oracle as base  # noqa: E402
from mgba_ctl import canonical, data_dir  # noqa: E402

HANDLERS = 0x087F38B8
ENTITIES = 36
# The handlers this oracle records: opponents and traffic. Others (the player, effect entities 0x34) just run.
OWN = {0x29, 0x36}
# opponent_effects (FUN_0814e628) places a car's rear light/exhaust sprites in the 2D layer (the sprite pool at
# *0x03000058): rendering, so stubbed and recorded with its arguments (entity, angle a, angle b, size).
EFFECTS = 0x0814E628


def work_dir(var: str, default: str) -> Path:
    return data_dir() / "work" / canonical()[1] / (os.environ.get(var) or default)


def functions() -> dict[int, str]:
    """Function entry points and names from the Ghidra export, for coverage."""
    path = data_dir() / "work" / canonical()[1] / "ghidra" / "carbon_decomp.c"
    out = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.startswith("// ==== "):
            addr, name = line[8:].split()[:2]
            out[int(addr, 16)] = name
    return out


def call(u, handler: int, entity: int, covered: Counter | None, entries: dict[int, str]) -> list[str]:
    """Run one entity handler; returns its stubbed (sound) calls."""
    calls = []

    def code(uc, addr, size, _):
        if addr == EFFECTS:
            e, a, b = (uc.reg_read(r) for r in (UC_ARM_REG_R1, base.UC_ARM_REG_R2, base.UC_ARM_REG_R3))
            size = struct.unpack("<i", bytes(uc.mem_read(uc.reg_read(UC_ARM_REG_SP), 4)))[0]
            calls.append(f"effects({(e - rd(uc, base.WORLD + 0x3C, 4)) // 0xA4},{a},{b},{size})")
            uc.reg_write(UC_ARM_REG_PC, uc.reg_read(UC_ARM_REG_LR))
        elif addr in base.STUBS:
            name, n = base.STUBS[addr]
            regs = [uc.reg_read(r) for r in (UC_ARM_REG_R0, UC_ARM_REG_R1, base.UC_ARM_REG_R2, base.UC_ARM_REG_R3)]
            args = [struct.unpack("<i", struct.pack("<I", v))[0] for v in regs[:n]]
            if name != "decal":
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
        uc.reg_write(base.UC_ARM_REG_R3, abs(q) & 0xFFFFFFFF)

    def block(uc, addr, size, _):
        if addr & ~1 in entries:
            covered[addr & ~1] += 1

    u.reg_write(UC_ARM_REG_R0, base.WORLD)
    u.reg_write(UC_ARM_REG_R1, entity)
    u.reg_write(UC_ARM_REG_SP, base.SP)
    u.reg_write(UC_ARM_REG_LR, base.RET | 1)
    hooks = [u.hook_add(UC_HOOK_CODE, code, begin=0x08135000, end=0x08153000), u.hook_add(UC_HOOK_INTR, swi)]
    if covered is not None:
        hooks.append(u.hook_add(UC_HOOK_BLOCK, block))
    try:
        u.emu_start(handler | 1, base.RET, count=50_000_000)
    finally:
        for h in hooks:
            u.hook_del(h)
    return calls


def rd(u, addr: int, n: int) -> int:
    return int.from_bytes(bytes(u.mem_read(addr, n)), "little")


def replay(name: str, coverage: bool) -> int:
    src, out = work_dir("NFSGBA_TRACE_SESSION", "vehicle-physics"), work_dir("NFSGBA_MGBA_SESSION", "ai-traffic")
    out.mkdir(parents=True, exist_ok=True)
    rom = canonical()[0].read_bytes()
    u = base.machine(src, name, rom)
    entries = functions() if coverage else {}
    covered = Counter() if coverage else None
    states = list(base.ram_states(src, name))
    lines, bad, checked = [], 0, 0
    for i in range(len(states) - 1):
        if i and i % 100 == 0:
            # A fresh machine now and then: unicorn 2.1.4 crashes natively after a few thousand runs with hooks.
            u = base.machine(src, name, rom)
        u.mem_write(0x02000000, states[i][:0x40000].tobytes())
        u.mem_write(0x03000000, states[i][0x40000:].tobytes())
        array = rd(u, base.WORLD + 0x3C, 4)
        for k in range(ENTITIES):
            e = array + 0xA4 * k
            if rd(u, e + 8, 2) & 3 != 3:
                continue
            index = rd(u, e + 0x4E, 2)
            handler = rd(u, HANDLERS + 4 * index, 4) & ~1
            own = index in OWN
            before = base.snapshot(u)
            try:
                calls = call(u, handler, e, covered if own else None, entries)
            except OSError as err:
                raise RuntimeError(f"{name} step {i} entity {k} handler {index:#x}: unicorn failed ({err})") from err
            writes = base.changes(before, base.snapshot(u))
            lines.append(f"{i};{k};{index:#x};{' '.join(f'{a:08x}={v:02x}' for a, v in writes)};{' '.join(calls)}")
        # The next traced step must show every non-player car as the loop left it.
        nxt = states[i + 1]
        for k in range(1, ENTITIES):
            e = array + 0xA4 * k
            if rd(u, e + 8, 2) & 3 != 3 or rd(u, e + 0x4E, 2) not in OWN or rd(u, e + 0x8C, 4) < 0x02000000:
                continue
            p = rd(u, e + 0x8C, 4)
            size = 0x4FC if rd(u, e + 0x4E, 2) != 0x36 else 0x28
            off_e, off_p = e - 0x02000000, p - 0x02000000
            checked += 1
            diffs = [f"{label}+{o:#x}" for label, got, want, ext in (
                ("entity", bytes(u.mem_read(e, 0xA4)), nxt[off_e:off_e + 0xA4].tobytes(), base.EXTERNAL),
                ("physics", bytes(u.mem_read(p, size)), nxt[off_p:off_p + size].tobytes(),
                 base.PHYSICS_EXTERNAL if size == 0x4FC else {}))
                for o, (g, w) in enumerate(zip(got, want)) if (g ^ w) & ~ext.get(o, 0) & 0xFF]
            if diffs:
                bad += 1
                print(f"{name} step {i} entity {k}: the loop does not reproduce the next step: {' '.join(diffs[:12])}")
    (out / f"{name}.ai-oracle.txt").write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"{name}: {checked - bad} of {checked} car states reproduced; wrote {name}.ai-oracle.txt")
    if coverage:
        for addr, n in sorted(covered.items()):
            print(f"  {addr:08x} {entries[addr]} x{n}")
    return bad


if __name__ == "__main__":
    args = [a for a in sys.argv[1:] if a != "--coverage"]
    if not args:
        sys.exit(__doc__)
    sys.exit(1 if sum(replay(n, "--coverage" in sys.argv) for n in args) else 0)
