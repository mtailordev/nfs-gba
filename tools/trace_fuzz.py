"""Oracle cases for the car step on perturbed real states (docs/engine/physics.md): paths no recording reaches.

    .venv/Scripts/python.exe tools/trace_fuzz.py [COUNT]    # default 2000 cases

Takes steps of the recorded car traces (vehicle-physics/<name>.ramdelta) and runs the game's car handler
FUN_0814bd4c (or, outside hunter races, an opponent's handler FUN_0814a2a0) on them in the function oracle (as
tools/trace_oracle.py), after one of two perturbations:
  - speeds: a random body velocity (+0x11C, log-uniform magnitude, any direction in x/z, sometimes y), sometimes a
    random angular velocity (+0x158). They carry the car two portals in a step (`find_sector_far`, FUN_0814dbbc)
    or out of every sector (the push-back loops in FUN_0813d1f0 and FUN_0813c5a8), which no recording does;
  - controls (player only): automatic or manual gearbox with a random shift state, random keys (A, B, L, R, left,
    right and combinations: the manual shifts, nitro on A+L, the wingman command on R+L), gear, nitro tank, state,
    state, the full-tank flag. The recordings never use the manual gearbox or nitro. Writes vehicle-physics/fuzz.jsonl for
crates/nfsgba-sim/tests/fuzz.rs: per case the trace, step, patch, every RAM byte the step changed, its sound calls,
and which of those two paths ran. Seeded: reruns give the same file.
"""
import json
import math
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import trace_ai_oracle as ai  # noqa: E402
import trace_oracle as base  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402

TRACES = ["wall", "drive", "long", "reverse", "hunter", "tipped", "start"]
# Opponents (handler 0x29, FUN_0814a2a0) only outside hunter races, whose AI part is not ported.
AI_TRACES = {"wall", "drive", "long", "reverse", "start"}
OPPONENT = 0x0814A2A0
# The player's push-back loop, and the opponent's (in FUN_0813c5a8).
PATHS = {0x0814DBBC: "far", 0x0813DF98: "pushback", 0x0813CFBC: "ai-pushback"}


KEYS = [0x001, 0x002, 0x201, 0x301, 0x100, 0x200, 0x300, 0x011, 0x021, 0x111, 0x221, 0x003, 0x101, 0x000]


def controls(rng: random.Random, e: int, index: int, p: int) -> list[tuple[int, bytes]]:
    """The player's inputs and the gearbox and nitro state the step reads: automatic or manual transmission
    (0x03005798) with its shift state (0x03006074), the control word (0x030057D8 + 2·index, 0xFC00 | keys) and the
    previous one (+0x4AE), gear (+0x40), nitro tank (+0x4C8), nitro on (+0x4D1), full-tank flag (0x03006150)."""
    s32 = lambda v: struct.pack("<i", v)  # noqa: E731
    return [(0x03005798, s32(rng.choice([0, 1]))), (0x03006074, struct.pack("<h", rng.randrange(4))),
            (0x030057D8 + 2 * index, struct.pack("<H", 0xFC00 | rng.choice(KEYS))),
            (p + 0x4AE, struct.pack("<H", 0xFC00 | rng.choice(KEYS))), (p + 0x40, s32(rng.randrange(8))),
            (p + 0x4C8, s32(rng.choice([0, 1, 2, rng.randrange(0x40000)]))), (p + 0x4D1, bytes([rng.randrange(2)])),
            (p + 0x4CC, struct.pack("<HH", rng.choice([0, rng.randrange(0x10000)]), rng.randrange(0x1000, 0x2000))),
            (0x03006150, s32(rng.choice([0, 0, 1])))]


def perturb(rng: random.Random, p: int) -> list[tuple[int, bytes]]:
    mag = int(2 ** rng.uniform(8, 21))
    a = rng.uniform(0, 2 * math.pi)
    v = [int(mag * math.sin(a)), rng.choice([0, 0, rng.randint(-mag, mag)]), int(mag * math.cos(a))]
    patch = [(p + 0x11C, struct.pack("<3i", *v))]
    if rng.random() < 0.3:
        patch.append((p + 0x158, struct.pack("<3i", *[rng.randint(-0x4000, 0x4000) for _ in range(3)])))
    return patch


INIT_TRACES = ["start", "hunter", "wingman", "sprint", "circuit", "shortcut"]


def init_inputs(rng: random.Random, e: int) -> list[tuple[int, bytes]]:
    """The race globals the car init reads: career flag (0x030000A0: 0 quick race, 1/2 career), event AI skill
    (0x030000BC), difficulty (0x03005608), race mode (0x030056E0), route index (0x03005720) and number (0x03005388),
    environment (0x0300006C), reverse (0x03005610), wingman (0x03006104), opponents (0x03005784); the car's state
    is set to 0 so the handler runs the init."""
    s32 = lambda v: struct.pack("<i", v)  # noqa: E731
    return [(0x030000A0, s32(rng.choice([0, 1, 2]))), (0x030000BC, s32(rng.randint(0x28, 0x60))),
            (0x03005608, s32(rng.randint(1, 3))), (0x030056E0, s32(rng.randrange(4))),
            (0x03005720, s32(rng.choice([0x13, rng.randrange(44)]))), (0x03005388, s32(rng.randint(1, 43))),
            (0x0300006C, s32(rng.randrange(12))), (0x03005610, s32(rng.randrange(2))),
            (0x03006104, s32(rng.randrange(13))), (0x03005784, s32(rng.choice([3, 3, 2, 1, 0]))),
            (e + 0x4A, struct.pack("<H", 0))]


def init_cases(work, count: int, seen: dict) -> list[dict]:
    """The car init (entity state 0) on the first step of the traces that start at a race-info screen, with random
    race globals, for the player or (as the car handler) another racer slot: the career and wingman branches."""
    out = []
    rng = random.Random(0x0814B98C)
    for i in range(count):
        name = rng.choice(INIT_TRACES)
        gba = base.Gba(f"{work.name}/{name}")
        state = next(base.ram_states(work, name))
        gba.poke(0x02000000, state[:0x40000].tobytes())
        gba.poke(0x03000000, state[0x40000:].tobytes())
        entities = struct.unpack("<I", gba.read_base(base.WORLD + 0x3C, 4))[0]
        index = rng.choice([0, 0, rng.randrange(1, 4)])
        e = entities + 0xA4 * index
        patch = init_inputs(rng, e)
        for addr, data in patch:
            gba.poke(addr, data)
        r, calls = base.run_step(gba, e)
        writes = [f"{a + j:08x}={v:02x}" for a, b in r.writes if base.RAM[0] <= a < base.RAM[1] for j, v in enumerate(b)]
        seen["init"] = seen.get("init", 0) + 1
        out.append({"trace": name, "step": 0, "entity": index, "car": True, "patch": [(a, d.hex()) for a, d in patch],
                    "writes": writes, "calls": calls, "paths": ["init"]})
    return out


def main(count: int) -> None:
    work = base.session()
    rng = random.Random(0x0814DBBC)
    lengths = {name: sum(1 for _ in base.ram_states(work, name)) for name in TRACES}
    picks = sorted((name, rng.randrange(1, lengths[name] - 1), i) for i in range(count)
                   for name in [rng.choice(TRACES)])
    cases = [None] * count
    seen = {}
    for name in TRACES:
        wanted = {}
        for n, k, i in picks:
            if n == name:
                wanted.setdefault(k, []).append(i)
        if not wanted:
            continue
        gba = base.Gba(f"{work.name}/{name}")
        entity = struct.unpack("<I", gba.read_base(base.WORLD + 0x3C, 4))[0]
        for k, state in enumerate(base.ram_states(work, name)):
            for i in wanted.get(k, []):
                gba.poke(0x02000000, state[:0x40000].tobytes())
                gba.poke(0x03000000, state[0x40000:].tobytes())
                rng_case = random.Random(i * 7919 + k)
                # The player's car, or (outside hunter races) an opponent with its physics struct allocated.
                index = rng_case.randrange(1, 4) if name in AI_TRACES and rng_case.random() < 0.5 else 0
                e = entity + 0xA4 * index
                p = struct.unpack("<I", gba.read_base(e + 0x8C, 4))[0]
                if not 0x02000000 <= p < 0x02040000:
                    index, e = 0, entity
                    p = struct.unpack("<I", gba.read_base(e + 0x8C, 4))[0]
                # Player cases: half get extreme speeds, half random controls, gearbox and nitro state.
                patch = perturb(rng_case, p) if index or rng_case.random() < 0.5 else controls(rng_case, e, 0, p)
                for addr, data in patch:
                    gba.poke(addr, data)
                hit = set()
                hooks = [gba.uc.hook_add(UC_HOOK_CODE, lambda uc, a, s, tag: hit.add(tag), user_data=tag,
                                         begin=addr, end=addr) for addr, tag in PATHS.items()]
                try:
                    if index == 0:
                        r, calls = base.run_step(gba, entity)
                        changed = [(a + j, v) for a, b in r.writes if base.RAM[0] <= a < base.RAM[1]
                                   for j, v in enumerate(b)]
                    else:
                        changed, calls = ai.call(gba, OPPONENT, e, entity)
                finally:
                    for h in hooks:
                        gba.uc.hook_del(h)
                for tag in hit:
                    seen[tag] = seen.get(tag, 0) + 1
                writes = [f"{a:08x}={v:02x}" for a, v in changed]
                cases[i] = {"trace": name, "step": k, "entity": index, "patch": [(a, d.hex()) for a, d in patch],
                            "writes": writes, "calls": calls, "paths": sorted(hit)}
    cases += init_cases(work, count // 8, seen)
    out = work / "fuzz.jsonl"
    with out.open("w", encoding="utf-8") as f:
        for c in cases:
            f.write(json.dumps(c) + "\n")
    print(f"wrote {len(cases)} cases to {out}; paths reached {seen}")


if __name__ == "__main__":
    main(int(sys.argv[1]) if len(sys.argv) > 1 else 2000)
