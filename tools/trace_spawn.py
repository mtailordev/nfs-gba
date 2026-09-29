"""Oracle cases for traffic_spawn FUN_08143d48, all three kinds (docs/engine/physics.md).

    .venv/Scripts/python.exe tools/trace_spawn.py [COUNT]    # default 1500 cases

Kinds 0 and 2 come only from the spawner handlers (table entries 0x2A, 0x2D, 0x2E), which no Carbon race has, so
the function is called directly in the function oracle on real states of the recorded car traces, with traffic on
(0x03006298), a random kind, a random racer as the car to spawn near, and sometimes a random racing-line section on
it (+0x72) and a random rand_table index (0x030064C8). Writes vehicle-physics/spawn.jsonl for
crates/nfsgba-sim/tests/trace.rs: per case the trace, step, inputs, the return value and every RAM byte changed.
Seeded: reruns give the same file.
"""
import json
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import trace_oracle as base  # noqa: E402

FN = 0x08143D48
TRACES = ["drive", "long", "start", "hunter"]


def main(count: int) -> None:
    work = base.session()
    rng = random.Random(0x08143D48)
    lengths = {name: sum(1 for _ in base.ram_states(work, name)) for name in TRACES}
    picks = sorted((name, rng.randrange(1, lengths[name]), i) for i in range(count) for name in [rng.choice(TRACES)])
    cases = [None] * count
    kinds = {}
    for name in TRACES:
        wanted = {}
        for n, k, i in picks:
            if n == name:
                wanted.setdefault(k, []).append(i)
        gba = base.Gba(f"{work.name}/{name}")
        entities = struct.unpack("<I", gba.read_base(base.WORLD + 0x3C, 4))[0]
        sections = struct.unpack("<I", gba.read_base(base.WORLD + 0x40, 4))[0]
        for k, state in enumerate(base.ram_states(work, name)):
            for i in wanted.get(k, []):
                gba.poke(0x02000000, state[:0x40000].tobytes())
                gba.poke(0x03000000, state[0x40000:].tobytes())
                r_ = random.Random(i)
                kind = r_.choice([0, 1, 2])
                near = entities + 0xA4 * r_.randrange(4)
                patch = [(0x03006298, bytes([1]))]
                if r_.random() < 0.4:
                    # A section of this race's line: records with 2..255 waypoints starting inside the 0x1800 line.
                    valid = [s for s in range(8)
                             if 2 <= struct.unpack("<H", gba.read_base(sections + 8 * s, 2))[0] < 256
                             and struct.unpack("<I", gba.read_base(sections + 8 * s + 4, 4))[0] < 0x100]
                    patch.append((near + 0x72, struct.pack("<H", r_.choice(valid))))
                if r_.random() < 0.5:
                    patch.append((0x030064C8, struct.pack("<I", r_.randrange(256))))
                for addr, data in patch:
                    gba.poke(addr, data)
                r = gba.call(FN, mode="thumb", r0=base.WORLD, r1=near, r2=kind, sp=base.SP)
                assert r.stop == "return", r.stop
                writes = [f"{a + j:08x}={v:02x}" for a, b in r.writes if base.RAM[0] <= a < base.RAM[1]
                          for j, v in enumerate(b)]
                ret = r.regs["r0"] & 0xFFFFFFFF
                kinds[(kind, ret != 0xFFFF)] = kinds.get((kind, ret != 0xFFFF), 0) + 1
                cases[i] = {"trace": name, "step": k, "near": (near - entities) // 0xA4, "kind": kind,
                            "patch": [(a, d.hex()) for a, d in patch], "ret": ret, "writes": writes}
    out = work / "spawn.jsonl"
    with out.open("w", encoding="utf-8") as f:
        for c in cases:
            f.write(json.dumps(c) + "\n")
    print(f"wrote {count} cases to {out}; (kind, spawned): {sorted(kinds.items())}")


if __name__ == "__main__":
    main(int(sys.argv[1]) if len(sys.argv) > 1 else 1500)
