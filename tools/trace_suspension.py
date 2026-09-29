"""Oracle cases for the suspension step FUN_0814de40 (docs/engine/physics.md), which no race reaches.

    .venv/Scripts/python.exe tools/trace_suspension.py [COUNT]    # default 3000 cases

Runs the game's function in the function oracle (tools/oracle) on the reference race (snapshot mgba/race: mid-race,
every car allocated) with random inputs, and writes
vehicle-physics/suspension.jsonl for crates/nfsgba-sim/tests/suspension.rs. Each case sets, for one of the four
racers: its car id, position and heading, and the physics fields the step reads (+0x08..+0x78); four points (x, z
offsets and a y the game overwrites); the frame time. It records the return value, the points and sector outputs
after the call, and every other RAM byte the call changed. Random, but seeded: reruns give the same file.
"""
import json
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent / "oracle"))
from oracle import Gba, canonical, data_dir  # noqa: E402

FN = 0x0814DE40
WORLD = 0x030000C0
PTS, OUT = 0x0203F000, 0x0203F040  # scratch at the end of EWRAM (zero and unused in the snapshot)


def case(gba: Gba, rng: random.Random, entities: int, player_physics: list[int]) -> dict:
    k = rng.randrange(4)
    e = entities + 0xA4 * k
    p = player_physics[k]
    rd = lambda a: struct.unpack("<i", gba.read_base(a, 4))[0]  # noqa: E731
    near = rng.random() < 0.9
    x = rd(e + 0xC) + (rng.randint(-0x40000, 0x40000) if near else rng.randint(-0x800000, 0x800000))
    z = rd(e + 0x14) + (rng.randint(-0x40000, 0x40000) if near else rng.randint(-0x800000, 0x800000))
    y = rd(e + 0x10) + rng.randint(-0x4000, 0x4000)
    patch = []
    if rng.random() < 0.35:
        # Anywhere in the city: a random sector, the car at the mean of its corners.
        s = rng.randrange(struct.unpack("<H", gba.read_base(WORLD + 0xDA, 2))[0])
        sec = struct.unpack("<I", gba.read_base(WORLD + 0x14, 4))[0] + 0x30 * s
        first, n = struct.unpack("<HH", gba.read_base(sec, 4))
        walls = struct.unpack("<I", gba.read_base(WORLD + 0x10, 4))[0]
        corners = [struct.unpack("<ii", gba.read_base(walls + 0x44 * (first + i), 8)) for i in range(n)]
        x = sum(c[0] for c in corners) // n * 256 + rng.randint(-0x8000, 0x8000)
        z = sum(c[1] for c in corners) // n * 256 + rng.randint(-0x8000, 0x8000)
        patch.append((e + 0x78, struct.pack("<H", s)))
    patch += [(e + 0xC, struct.pack("<iii", x, y, z)), (e + 0x2C, struct.pack("<i", rng.randrange(0x400000))),
              (e + 0x89, bytes([rng.randrange(15)]))]
    # Speeds +0x08..+0x14, springs +0x4C.., rates +0x5C.., heights +0x6C..: around the floor, sometimes wild.
    wild = rng.random() < 0.1
    for base, span in ((0x08, 0x40000), (0x4C, 0x8000), (0x5C, 0x40000), (0x6C, 0x8000)):
        vals = [rng.randint(-span, span) * (16 if wild else 1) + (y if base in (0x4C, 0x6C) else 0) for _ in range(4)]
        patch.append((p + base, struct.pack("<4i", *vals)))
    pts = [[rng.randint(-0x80, 0x80), rng.randint(-0x10000, 0x10000), rng.randint(-0x80, 0x80)] for _ in range(4)]
    patch.append((PTS, struct.pack("<12i", *[c for pt in pts for c in pt])))
    dt = rng.choice([10, 15, 20, 21, 22, 23, 24, 30, 50, 100, rng.randint(1, 200)])
    r = gba.call(FN, r0=WORLD, r1=e, r2=PTS, r3=OUT, stack=[4, dt], mem=patch)
    assert r.stop == "return", r.stop
    # Outputs in the scratch area: the patched points plus the call's writes (`writes` is relative to the inputs).
    scratch = bytearray(struct.pack("<12i", *[c for pt in pts for c in pt]) + bytes(OUT + 8 - PTS - 48))
    for a, b in r.writes:
        for i, v in enumerate(b):
            if PTS <= a + i < OUT + 8:
                scratch[a + i - PTS] = v
    writes = [(a, bytes(b).hex()) for a, b in r.writes if not PTS <= a < OUT + 8]
    return {"entity": e, "dt": dt, "patch": [(a, b.hex()) for a, b in patch], "ret": r.regs["r0"],
            "points": list(struct.unpack("<12i", scratch[:48])), "sectors": list(struct.unpack("<4H", scratch[64:])),
            "writes": writes}


def main(count: int) -> None:
    gba = Gba("mgba/race")
    entities = struct.unpack("<I", gba.read_base(WORLD + 0x3C, 4))[0]
    physics = [struct.unpack("<I", gba.read_base(entities + 0xA4 * k + 0x8C, 4))[0] for k in range(4)]
    assert all(0x02000000 <= p < 0x02040000 for p in physics), physics
    assert not any(gba.read_base(PTS, 0x48)), "scratch area is not free"
    rng = random.Random(0x814DE40)
    out = data_dir() / "work" / canonical()[1] / "vehicle-physics" / "suspension.jsonl"
    with out.open("w", encoding="utf-8") as f:
        for _ in range(count):
            f.write(json.dumps(case(gba, rng, entities, physics)) + "\n")
    print(f"wrote {count} cases to {out}")


if __name__ == "__main__":
    main(int(sys.argv[1]) if len(sys.argv) > 1 else 3000)
