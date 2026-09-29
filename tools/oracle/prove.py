"""Prove the oracle against functions that are already ported and verified: run each on many inputs, check the
oracle's own invariants, and write the cases to $NFSGBA_DATA/work/<sha8>/harness/oracle/*.jsonl, which the
workspace test crates/nfsgba-formats/tests/oracle_cases.rs compares with the Rust ports. Prints calls per second.

    .venv/Scripts/python.exe tools/oracle/prove.py [--n 20000]
    cargo test --release -p nfsgba-formats --test oracle_cases
"""
import argparse
import json
import random
import struct
import time

from oracle import Gba, canonical, data_dir

DIVSI3, SIN_Q14, LIGHT, RECIP_DIV = 0x0816A708, 0x0815F948, 0x0813A514, 0x03004CA4
WORLD, PLAYER_INDEX, SECTOR = 0x030000C0, 0x03000060, 0x03005614


def s32(v):
    return v - (1 << 32) if v & 0x8000_0000 else v


def timed(label, cases, run):
    t = time.perf_counter()
    out = [run(c) for c in cases]
    dt = time.perf_counter() - t
    print(f"{label}: {len(cases)} calls, {len(cases) / dt:,.0f} calls/s")
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--n", type=int, default=20000)
    args = ap.parse_args()
    rng = random.Random(1)
    gba = Gba()
    _, sha8 = canonical()
    out = data_dir() / "work" / sha8 / "harness" / "oracle"
    out.mkdir(parents=True, exist_ok=True)
    rom = gba.rom.tobytes()

    # __divsi3: edge values plus random pairs (b = 0 included).
    edges = [0, 1, -1, 2, -2, 3, 7, 0x7FFF_FFFF, -0x8000_0000, 0x10000, -0x10000]
    pairs = [(a, b) for a in edges for b in edges]
    pairs += [(s32(rng.getrandbits(32)), s32(rng.getrandbits(rng.choice((4, 12, 32))))) for _ in range(args.n)]
    res = timed("__divsi3", pairs, lambda p: gba.call(DIVSI3, r0=p[0], r1=p[1], align="ignore"))
    assert all(r.stop == "return" and not r.writes for r in res), {r.stop for r in res}
    rows = [{"a": a, "b": b, "q": s32(r.regs["r0"])} for (a, b), r in zip(pairs, res)]
    (out / "divsi3.jsonl").write_text("".join(json.dumps(r) + "\n" for r in rows))
    # A cross-check independent of Rust: truncating division.
    for r in rows:
        if r["b"] and not (r["a"] == -0x8000_0000 and r["b"] == -1):
            q = abs(r["a"]) // abs(r["b"]) * (1 if (r["a"] < 0) == (r["b"] < 0) else -1)
            assert r["q"] == q, r

    # sin_q14: every angle of the turn and beyond, plus random words.
    angles = list(range(-0x100, 0x10100)) + [s32(rng.getrandbits(32)) for _ in range(args.n // 4)]
    res = timed("sin_q14", angles, lambda a: gba.call(SIN_Q14, r0=a, align="ignore"))
    assert all(r.stop == "return" and not r.writes for r in res)
    (out / "sin_q14.jsonl").write_text("".join(json.dumps({"angle": a, "sin": s32(r.regs["r0"])}) + "\n"
                                                for a, r in zip(angles, res)))

    # recip_div (IWRAM, ARM): r0 = (a * recip[b]) >> 24 with recip at 0x7C45F0 (docs/engine/renderer.md).
    cases = [(s32(rng.getrandbits(32)), rng.randrange(0x7FFF)) for _ in range(args.n // 4)]
    res = timed("recip_div (IWRAM, ARM)", cases, lambda c: gba.call(RECIP_DIV, r0=c[0], r1=c[1], align="ignore"))
    for (a, b), r in zip(cases, res):
        recip = struct.unpack_from("<i", rom, 0x7C45F0 + 4 * b)[0]
        assert r.stop == "return" and r.regs["r0"] == (a * recip >> 24) & 0xFFFF_FFFF, (a, b, r.stop)

    # apply_sector_light_to_palette at the reference state: palette RAM must equal the dump's.
    ref = gba.call(LIGHT, r0=WORLD)
    assert ref.stop == "return", ref.stop
    got, dump = ref.read(0x05000000, 512), gba.read_base(0x05000000, 512)
    diff = [i for i in range(256) if got[2 * i:2 * i + 2] != dump[2 * i:2 * i + 2]]
    print(f"apply_sector_light_to_palette at the reference state: {256 - len(diff)}/256 palette entries equal "
          f"the dump; differing: {diff}")

    # ... and at random positions in random sectors, for the Rust checker (sector_light + tint_palette).
    iw, wr = gba.read_base(0x03000000, 0x8000), gba.read_base(0x02000000, 0x40000)
    word = lambda mem, o: struct.unpack_from("<I", mem, o)[0]
    player = word(iw, 0xC0 + 0x3C) + 0xA4 * word(iw, 0x60)
    level = 0x7F2B08
    walls_at, sectors_at = word(rom, level + 0x14) - 0x0800_0000, word(rom, level + 0x18) - 0x0800_0000
    nsec = (walls_at - sectors_at) // 0x30
    cases = []
    for _ in range(args.n // 10):
        s = rng.randrange(nsec)
        first, count = struct.unpack_from("<HH", rom, sectors_at + 0x30 * s)
        xs = [s32(word(rom, walls_at + 0x44 * (first + k))) for k in range(count)]
        zs = [s32(word(rom, walls_at + 0x44 * (first + k) + 4)) for k in range(count)]
        x = rng.randint(min(xs) - 64, max(xs) + 64) * 256 + rng.randrange(256)
        z = rng.randint(min(zs) - 64, max(zs) + 64) * 256 + rng.randrange(256)
        cases.append((s, x, z))

    def light(c):
        s, x, z = c
        mem = [(SECTOR, struct.pack("<I", s)), (player + 0x0C, struct.pack("<i", x)),
               (player + 0x14, struct.pack("<i", z))]
        return gba.call(LIGHT, r0=WORLD, mem=mem)

    res = timed("apply_sector_light_to_palette", cases, light)
    assert all(r.stop == "return" for r in res), {r.stop for r in res}
    (out / "sector_light.jsonl").write_text("".join(
        json.dumps({"sector": s, "x": x, "z": z, "palette": r.read(0x05000000, 512).hex()}) + "\n"
        for (s, x, z), r in zip(cases, res)))
    written = sum(any(0x05000000 <= a < 0x05000400 for a, _ in r.writes) for r in res)
    print(f"  {written}/{len(res)} calls changed palette RAM; cases in {out}")


if __name__ == "__main__":
    main()
