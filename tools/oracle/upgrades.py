"""Oracle cases for `upgrades_changed` (0x081302C4, the upgrade pages' automatic purchase;
crates/nfsgba-game/src/menu/garage.rs) on generated garage states, the unlock rules and rand_table running for real.

    .venv/Scripts/python.exe tools/oracle/cases.py upgrades [COUNT]   # default 400

Writes reach/upgrades.jsonl (the `garage` case format; replayed by `menu::tests::upgrades_match_the_game`).
"""
import json
import random
import struct

import garage as G
from menus import PROFILE_AT, Skip

from common import data_dir
from oracle import Gba

FN = 0x081302C4
OUT = data_dir() / "work" / "e5298b24" / "reach"
STUBS = G.stubs_without(*G.KIND18[:3], *G.HELPERS.values(), FN)


def main(argv: list[str]) -> None:
    n = int(argv[0]) if argv else 400
    rng = random.Random(0x0813_02C4)
    gbas = {s: Gba(s) for s in G.SNAPS}
    ids = G.unlock_ids(gbas[G.SNAPS[0]])
    cases = []
    for _ in range(n):
        snap = rng.choice(G.SNAPS)
        mem = G.garage_state(rng, gbas[snap], rng.choice([0x1D, 0x1E]), ids)
        # Mostly with +0x12 set (the lists are built only then) and cash to buy with.
        mem.append((PROFILE_AT + 0x12, struct.pack("<H", rng.choice([1, 1, 1, 0x7FFF, 0]))))
        mem.append((PROFILE_AT + 0xC, struct.pack("<i", rng.choice([0, 5000, 50000, 500000, 1 << 24]))))
        args = [rng.choice([0, 1])]
        try:
            r0, writes, calls = G.with_stubs(STUBS, gbas[snap], FN, mem, {}, regs={"r0": args[0]})
        except Skip:
            continue
        cases.append(dict(snap=snap, fn=hex(FN), args=args, mem=[[a, b.hex()] for a, b in mem], ret={}, r0=r0,
                          writes=[[a, b.hex()] for a, b in writes], calls=calls))
    OUT.mkdir(parents=True, exist_ok=True)
    path = OUT / "upgrades.jsonl"
    path.write_text("".join(json.dumps(c) + "\n" for c in cases), encoding="utf-8")
    print(f"upgrades: {len(cases)} cases ({sum(c['r0'] != 0 for c in cases)} buying) -> {path}")
