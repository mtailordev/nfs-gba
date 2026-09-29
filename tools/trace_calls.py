"""Oracle cases for car-step functions that no recording reaches in all their branches (docs/engine/physics.md).

    .venv/Scripts/python.exe tools/trace_calls.py [COUNT]    # default 1500 cases per function

Each function is called directly in the function oracle on real states of the recorded car traces, with random
inputs, and vehicle-physics/calls.jsonl gets per case the function, trace, step, inputs, return value and every RAM
byte it changed, for crates/nfsgba-sim/tests/trace.rs. Seeded: reruns give the same file.
  spawn    traffic_spawn FUN_08143d48 in all three kinds: kinds 0 and 2 only come from spawner handlers (table
           entries 0x2A, 0x2D, 0x2E) no Carbon race has. Traffic on (0x03006298), a random racer to spawn near,
           sometimes a random racing-line section on it (+0x72) and a random rand_table index (0x030064C8).
  wingman  wingman_command FUN_0814078c with random wingman state: wingman (0x03006104), commands left
           (0x030061DC), running (0x030061E8), cooldown (0x030061D8), role (0x030061F8), the wingman's car
           (0x0300619C) and the racers' places (driver +0xA8).
"""
import json
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import trace_oracle as base  # noqa: E402

TRACES = ["drive", "long", "start", "hunter", "wingman"]


def u32(v: int) -> bytes:
    return struct.pack("<I", v & 0xFFFFFFFF)


def spawn(gba, r, entities, sections):
    near = r.randrange(4)
    patch = [(0x03006298, bytes([1]))]
    if r.random() < 0.4:
        # A section of this race's line: records with 2..255 waypoints starting inside the 0x1800 line.
        valid = [s for s in range(8) if 2 <= struct.unpack("<H", gba.read_base(sections + 8 * s, 2))[0] < 256
                 and struct.unpack("<I", gba.read_base(sections + 8 * s + 4, 4))[0] < 0x100]
        patch.append((entities + 0xA4 * near + 0x72, struct.pack("<H", r.choice(valid))))
    if r.random() < 0.5:
        patch.append((0x030064C8, u32(r.randrange(256))))
    return {"near": near, "kind": r.choice([0, 1, 2])}, patch, dict(r1=entities + 0xA4 * near)


def wingman(gba, r, entities, sections):
    pick = lambda common, *rare: r.choice([common] * 3 + list(rare))  # noqa: E731
    patch = [(0x03006104, u32(pick(r.randint(1, 12), 0, 13, -1))), (0x030061DC, u32(pick(r.randint(1, 3), 0))),
             (0x030061E8, u32(pick(0, 1))), (0x030061D8, u32(pick(0, 0x100))), (0x030061F8, u32(r.choice([0, 1]))),
             (0x03006188, u32(r.randrange(1 << 20))),
             (0x0300619C, u32(r.choice([0, entities + 0xA4 * r.randrange(4)])))]
    for k in range(4):
        p = struct.unpack("<I", gba.read_base(entities + 0xA4 * k + 0x8C, 4))[0]
        if 0x02000000 <= p < 0x02040000:
            patch.append((p + 0xA8, u32(r.choice([r.randint(1, 4), r.randint(1, 4), 100, 99, -1]))))
    if r.random() < 0.2:
        patch.append((0x03005784, u32(r.choice([0, 1, 2, 3, -1]))))
    return {}, patch, {}


FUNCTIONS = {"spawn": (0x08143D48, spawn), "wingman": (0x0814078C, wingman)}


def main(count: int) -> None:
    work = base.session()
    rng = random.Random(0x0814078C)
    lengths = {name: sum(1 for _ in base.ram_states(work, name)) for name in TRACES}
    picks = sorted((name, rng.randrange(1, lengths[name]), fn, i) for fn in FUNCTIONS for i in range(count)
                   for name in [rng.choice(TRACES)])
    cases, seen = [], {}
    for name in TRACES:
        wanted = {}
        for n, k, fn, i in picks:
            if n == name:
                wanted.setdefault(k, []).append((fn, i))
        gba = base.Gba(f"{work.name}/{name}")
        entities = struct.unpack("<I", gba.read_base(base.WORLD + 0x3C, 4))[0]
        sections = struct.unpack("<I", gba.read_base(base.WORLD + 0x40, 4))[0]
        for k, state in enumerate(base.ram_states(work, name)):
            for fn, i in wanted.get(k, []):
                gba.poke(0x02000000, state[:0x40000].tobytes())
                gba.poke(0x03000000, state[0x40000:].tobytes())
                addr, make = FUNCTIONS[fn]
                inputs, patch, regs = make(gba, random.Random(f"{fn}{i}"), entities, sections)
                for a, data in patch:
                    gba.poke(a, data)
                regs = {"r0": base.WORLD, "r2": inputs.get("kind", 0), **regs}
                r = gba.call(addr, mode="thumb", sp=base.SP, **regs)
                assert r.stop == "return", (fn, r.stop)
                writes = [f"{a + j:08x}={v:02x}" for a, b in r.writes if base.RAM[0] <= a < base.RAM[1]
                          for j, v in enumerate(b)]
                ret = r.regs["r0"] & 0xFFFFFFFF
                key = f"{fn} kind {inputs['kind']} {'spawned' if ret != 0xFFFF else 'none'}" if fn == "spawn" \
                    else f"{fn} {'given' if ret else 'not given'}"
                seen[key] = seen.get(key, 0) + 1
                cases.append({"fn": fn, "trace": name, "step": k, **inputs,
                              "patch": [(a, d.hex()) for a, d in patch], "ret": ret, "writes": writes})
    out = work / "calls.jsonl"
    with out.open("w", encoding="utf-8") as f:
        for c in cases:
            f.write(json.dumps(c) + "\n")
    print(f"wrote {len(cases)} cases to {out}: {sorted(seen.items())}")


if __name__ == "__main__":
    main(int(sys.argv[1]) if len(sys.argv) > 1 else 1500)
