"""Synthesized race states (bake-off B1): a race start for a setup no capture has.

    cargo run -p nfsgba-game --example synth_race -- NAME ENV ROUTE MODE CAR
                                                  # the setup poked into race-init/circuit_pre -> synth/NAME_pre.*
    cases.py synth check NAME                     # the game's race_start on it -> synth/NAME_oracle.*; then the car
                                                  # handler on the player, both racers
    cargo run -p nfsgba-game --example synth_race -- NAME ENV ROUTE MODE CAR check
                                                  # our typed race_start's World == World::load of the oracle's
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
    for dom, base, size in REGIONS:
        if dom != "bios":
            (SYNTH / f"{name}_oracle.{dom}.bin").write_bytes(r.read(base, size))
    print(f"the game's race_start written as synth/{name}_oracle.*; "
          f"compare with: cargo run -p nfsgba-game --example synth_race -- {name} ENV ROUTE MODE CAR check")
    # One more original function on the synthesized state: the car handler, player entity then an opponent.
    gba = Gba(f"synth/{name}_oracle")
    ents = struct.unpack("<I", gba.read_base(WORLD + 0x3C, 4))[0]
    for k in range(2):
        e = ents + 0xA4 * k
        res, calls = run_step(gba, e)
        before = gba.read_base(e, 0xA4)
        after = res.read(e, 0xA4)
        print(f"car handler entity {k}: stop={res.stop} bytes changed={sum(a != b for a, b in zip(before, after))}"
              f" writes={len(res.writes)} stub calls={calls[:3]}")


def main(argv):
    cmd, name = argv
    assert cmd == "check", "usage: synth.py check NAME (make: cargo run -p nfsgba-game --example synth_race)"
    check(name)
