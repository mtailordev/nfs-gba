"""Oracle cases for car-step functions that no recording reaches in all their branches (docs/engine/physics.md).

    .venv/Scripts/python.exe tools/oracle/cases.py calls [COUNT]    # default 1500 cases per function

Each function is called directly in the function oracle on real states of the recorded car traces, with random
inputs, and vehicle-physics/calls.jsonl gets per case the function, trace, step, inputs, return value and every RAM
byte it changed, for crates/nfsgba-sim/tests/trace.rs. Seeded: reruns give the same file.
  spawn    traffic_spawn FUN_08143d48 in all three kinds: kinds 0 and 2 only come from spawner handlers (table
           entries 0x2A, 0x2D, 0x2E) no Carbon race has. Traffic on (0x03006298), a random racer to spawn near,
           sometimes a random racing-line section on it (+0x72) and a random rand_table index (0x030064C8).
  wingman  wingman_command FUN_0814078c with random wingman state: wingman (0x03006104), commands left
           (0x030061DC), running (0x030061E8), cooldown (0x030061D8), role (0x030061F8), the wingman's car
           (0x0300619C) and the racers' places (driver +0xA8).
  lap      lap_crossing FUN_0813f098 for a racer put on or near the line (section, segment), armed or not
           (driver +0x4D8 bit 1), with random laps left, places, best lap and lap start, race mode (elimination),
           laps, lapped flag, race time, the camera car and the someone-finished flag: lap, knock-out and finish.
"""
import json
import random
import struct

import car as base

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


def lap(gba, r, entities, sections):
    opponents = struct.unpack("<I", gba.read_base(0x03005784, 4))[0]
    who = r.randrange(opponents + 1)
    e = entities + 0xA4 * who
    count = struct.unpack("<H", gba.read_base(sections, 2))[0]
    lapped = r.choice([0, 1])
    patch = [(0x0300608C, u32(lapped)), (0x030056E0, u32(r.choice([0, 1, 1, 2, 3]))),
             (0x030056E4, u32(r.randint(1, 4))), (0x03005800, u32(r.randrange(1 << 16))),
             (0x030061A4, u32(r.choice([0, 0, 1]))), (0x030057F8, u32(r.randrange(opponents + 1))),
             (e + 0x72, struct.pack("<H", r.choice([0, 0, 0, 1]))),
             (e + 0x90, struct.pack("<h", r.choice([count - 1, count - 2, 0, r.randrange(count)])))]
    places = list(range(1, opponents + 2))
    r.shuffle(places)
    for k in range(opponents + 1):
        p = struct.unpack("<I", gba.read_base(entities + 0xA4 * k + 0x8C, 4))[0]
        if not 0x02000000 <= p < 0x02040000:
            continue
        flags = struct.unpack("<H", gba.read_base(p + 0x4D8, 2))[0]
        patch += [(p + 0xA8, u32(places[k])), (p + 0xC5, bytes([r.choice([1, 1, 2, 3, 0])])),
                  (p + 0x4D8, struct.pack("<H", flags | 2 if r.random() < 0.8 else flags & ~2)),
                  (p + 0xB4, u32(r.choice([0, r.randrange(1 << 14)]))), (p + 0xB8, u32(r.randrange(1 << 15)))]
    return {"who": who}, patch, dict(r1=e)


FUNCTIONS = {"spawn": (0x08143D48, spawn), "wingman": (0x0814078C, wingman), "lap": (0x0813F098, lap)}


def main(argv: list[str]) -> None:
    count = int(argv[0]) if argv else 1500
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
                if fn == "spawn":
                    key = f"spawn kind {inputs['kind']} {'spawned' if ret != 0xFFFF else 'none'}"
                elif fn == "wingman":
                    key = f"wingman {'given' if ret else 'not given'}"
                else:
                    key = f"lap {'crossed' if writes else 'not crossed'}"
                seen[key] = seen.get(key, 0) + 1
                cases.append({"fn": fn, "trace": name, "step": k, **inputs,
                              "patch": [(a, d.hex()) for a, d in patch], "ret": ret, "writes": writes})
    out = work / "calls.jsonl"
    with out.open("w", encoding="utf-8") as f:
        for c in cases:
            f.write(json.dumps(c) + "\n")
    print(f"wrote {len(cases)} cases to {out}: {sorted(seen.items())}")


