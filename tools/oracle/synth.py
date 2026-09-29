"""Synthesized race states (bake-off B1): a race start for a setup no capture has.

    cases.py synth make NAME ENV ROUTE MODE CAR   # NAME_pre.* = race-init/circuit_pre with the setup poked in
    cargo run -p nfsgba-game --example synth_race -- NAME   # our race_start -> synth/NAME.<domain>.bin
    cases.py synth check NAME                     # the game's race_start in the oracle == ours, byte for byte;
                                                  # then the car handler on the player of synth/NAME, both racers
Files in $NFSGBA_DATA/work/<sha8>/synth/. Setup inputs poked: environment 0x0300006C, route number and route
index 0x03005388/0x03005720, mode 0x030056E0, player car cars[0] 0x0300611C.
"""
import struct
import sys

from car import EXTERNAL, HANDLER, WORLD, run_step
from oracle import REGIONS, Gba, canonical, data_dir

SYNTH = data_dir() / "work" / canonical()[1] / "synth"
SRC = data_dir() / "work" / canonical()[1] / "race-init" / "circuit_pre"
PATCH = lambda env, route, mode, car: [(0x0300006C, env), (0x03005388, route), (0x03005720, route),  # noqa: E731
                                       (0x030056E0, mode)]


def make(name, env, route, mode, car):
    SYNTH.mkdir(parents=True, exist_ok=True)
    for dom, base, size in REGIONS:
        f = SRC.with_name(SRC.name + f".{dom}.bin")
        d = bytearray(f.read_bytes()) if f.exists() else bytearray(size)
        if dom == "iwram":
            for a, v in PATCH(env, route, mode, car):
                d[a - base:a - base + 4] = struct.pack("<I", v)
            d[0x0300611C - base] = car
        (SYNTH / f"{name}_pre.{dom}.bin").write_bytes(d)
    print("wrote", SYNTH / f"{name}_pre.*")


def check(name):
    pre = Gba(f"synth/{name}_pre")
    r = pre.call(0x08139E34, mode="thumb", regs={"r0": 0x030000C0}, max_insns=200_000_000)
    assert r.stop == "return", r.stop
    bad = 0
    for dom, base, size in REGIONS:
        if dom == "bios":
            continue
        ours = (SYNTH / f"{name}.{dom}.bin").read_bytes()[:size]
        want = r.read(base, size)
        diff = [i for i in range(size) if ours[i] != want[i]]
        print(f"race_start {dom}: {len(diff)} differing bytes", [hex(base + i) for i in diff[:6]])
        bad += len(diff)
    # One more original function on the synthesized state: the car handler, player entity then an opponent.
    gba = Gba(f"synth/{name}")
    ents = struct.unpack("<I", gba.read_base(WORLD + 0x3C, 4))[0]
    for k in range(2):
        e = ents + 0xA4 * k
        res, calls = run_step(gba, e)
        before = gba.read_base(e, 0xA4)
        after = res.read(e, 0xA4)
        print(f"car handler entity {k}: stop={res.stop} bytes changed={sum(a != b for a, b in zip(before, after))}"
              f" writes={len(res.writes)} stub calls={calls[:3]}")
    print("MISMATCH" if bad else "race_start: oracle == Rust, byte for byte")
    sys.exit(1 if bad else 0)


def main(argv):
    cmd, name, *a = argv
    make(name, *map(int, a)) if cmd == "make" else check(name)
