"""Synthesized race states (bake-off B1): a race start for a setup no capture has.

    cargo run -p nfsgba-game --example synth_race -- NAME ENV ROUTE MODE CAR
                                                  # pokes the setup into race-init/circuit_pre (race_init::apply_setup),
                                                  # writes synth/NAME_pre.* and our race_start as synth/NAME.*
    cases.py synth check NAME                     # the game's race_start in the oracle == ours, byte for byte;
                                                  # then the car handler on the player of synth/NAME, both racers
Files in $NFSGBA_DATA/work/<sha8>/synth/.
"""
import struct
import sys

from car import EXTERNAL, HANDLER, WORLD, run_step
from oracle import REGIONS, Gba, canonical, data_dir

SYNTH = data_dir() / "work" / canonical()[1] / "synth"


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
    cmd, name = argv
    assert cmd == "check", "usage: synth.py check NAME (make: cargo run -p nfsgba-game --example synth_race)"
    check(name)
