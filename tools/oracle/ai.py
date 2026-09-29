"""Replay the opponents' and traffic cars' steps on the game's own code (docs/engine/ai.md), with tools/oracle.

    .venv/Scripts/python.exe tools/oracle/cases.py ai start accel ...   # scenarios recorded by `record.py car` / `record.py ai`

The traces hold the full RAM at the entry of the player's car handler, once per game frame. That call is the first
of `update_entities` (FUN_0813765c), which then calls the handler of every later entity whose flags +0x08 & 3 == 3
(table 0x087F38B8 by entity +0x4E: 0x29 opponents, 0x36 traffic, 0x34 effects). For each step this runs that loop
on the step's RAM, each call starting where the previous one left off, and
  1. checks every opponent and traffic car against the next traced step (the oracle's own check; fields other game
     code maintains are masked as in trace_oracle.py), and
  2. writes <out>/<name>.ai-oracle.txt: per step, every handler call of the loop (the player's too) with every RAM
     byte it changed and its stubbed calls, as `step;entity;handler;writes;calls`. The Rust test replays the
     other handlers' writes and checks its own opponents and traffic against theirs.

Stubbed (recorded, not run): the sound entry points, the atlas decal blit (as in trace_oracle.py) and
`opponent_effects` (FUN_0814e628, a car's rear light/exhaust sprites in the 2D layer), recorded with its arguments.
Traces are read from $NFSGBA_TRACE_SESSION (default vehicle-physics); output goes to $NFSGBA_MGBA_SESSION (default
ai-traffic).
"""
import os
import struct
import sys
from pathlib import Path

import car as base
from mgba_ctl import canonical, data_dir
from oracle import REGS, Gba

HANDLERS = 0x087F38B8
ENTITIES = 36
OWN = {0x29, 0x36}
EFFECTS = 0x0814E628


def work_dir(var: str, default: str) -> Path:
    return data_dir() / "work" / canonical()[1] / (os.environ.get(var) or default)


def s32(v: int) -> int:
    return struct.unpack("<i", struct.pack("<I", v & 0xFFFFFFFF))[0]


def call(gba: Gba, handler: int, entity: int, array: int):
    """Run one entity handler on the current state and keep its result; returns (writes, stubbed calls)."""
    calls = []

    def stub(name, n):
        def record(uc):
            if name != "decal":  # rendering, not part of the simulation
                calls.append(f"{name}({','.join(str(s32(uc.reg_read(REGS[f'r{k}']))) for k in range(n))})")
        return record

    def effects(uc):
        e, a, b = (uc.reg_read(REGS[r]) for r in ("r1", "r2", "r3"))
        size = s32(int.from_bytes(bytes(uc.mem_read(uc.reg_read(REGS["sp"]), 4)), "little"))
        calls.append(f"effects({(e - array) // 0xA4},{a},{b},{size})")

    stubs = {a: stub(name, n) for a, (name, n) in base.STUBS.items()}
    stubs[EFFECTS] = effects
    r = gba.call(handler, mode="thumb", r0=base.WORLD, r1=entity, sp=base.SP, max_insns=50_000_000, stubs=stubs,
                 keep=True)
    if r.stop != "return":
        raise RuntimeError(f"handler {handler:#010x} on entity {entity:#010x} stopped: {r.stop}")
    writes = [(a + k, v) for a, b in r.writes if base.RAM[0] <= a < base.RAM[1] for k, v in enumerate(b)]
    return writes, calls


def rd(gba: Gba, addr: int, n: int) -> int:
    return int.from_bytes(gba.read_base(addr, n), "little")


def replay(name: str) -> int:
    src, out = work_dir("NFSGBA_TRACE_SESSION", "vehicle-physics"), work_dir("NFSGBA_MGBA_SESSION", "ai-traffic")
    out.mkdir(parents=True, exist_ok=True)
    states = list(base.ram_states(src, name))
    gba = None
    lines, bad, checked = [], 0, 0
    for i in range(len(states) - 1):
        if i % 100 == 0:
            # A fresh machine now and then: unicorn 2.1.4 has crashed natively after thousands of hooked runs.
            gba = Gba(f"{src.name}/{name}")
        gba.poke(0x02000000, states[i][:0x40000].tobytes())
        gba.poke(0x03000000, states[i][0x40000:].tobytes())
        array = rd(gba, base.WORLD + 0x3C, 4)
        for k in range(ENTITIES):
            e = array + 0xA4 * k
            if rd(gba, e + 8, 2) & 3 != 3:
                continue
            index = rd(gba, e + 0x4E, 2)
            handler = rd(gba, HANDLERS + 4 * index, 4) & ~1
            try:
                writes, calls = call(gba, handler, e, array)
            except (OSError, RuntimeError) as err:
                raise RuntimeError(f"{name} step {i} entity {k} handler {index:#x}: {err}") from err
            lines.append(f"{i};{k};{index:#x};{' '.join(f'{a:08x}={v:02x}' for a, v in writes)};{' '.join(calls)}")
        # The next traced step must show every opponent and traffic car as the loop left it.
        nxt = states[i + 1]
        for k in range(1, ENTITIES):
            e = array + 0xA4 * k
            if rd(gba, e + 8, 2) & 3 != 3 or rd(gba, e + 0x4E, 2) not in OWN or rd(gba, e + 0x8C, 4) < 0x02000000:
                continue
            p = rd(gba, e + 0x8C, 4)
            size = 0x4FC if rd(gba, e + 0x4E, 2) != 0x36 else 0x28
            off_e, off_p = e - 0x02000000, p - 0x02000000
            checked += 1
            diffs = [f"{label}+{o:#x}" for label, got, want, ext in (
                ("entity", gba.read_base(e, 0xA4), nxt[off_e:off_e + 0xA4].tobytes(), base.EXTERNAL),
                ("physics", gba.read_base(p, size), nxt[off_p:off_p + size].tobytes(),
                 base.PHYSICS_EXTERNAL if size == 0x4FC else {}))
                for o, (g, w) in enumerate(zip(got, want)) if (g ^ w) & ~ext.get(o, 0) & 0xFF]
            if diffs:
                bad += 1
                print(f"{name} step {i} entity {k}: the loop does not reproduce the next step: {' '.join(diffs[:12])}")
    (out / f"{name}.ai-oracle.txt").write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"{name}: {checked - bad} of {checked} car states reproduced; wrote {name}.ai-oracle.txt")
    return bad


def main(argv: list[str]) -> None:
    if not argv:
        sys.exit(__doc__)
    sys.exit(1 if sum(replay(n) for n in argv) else 0)
