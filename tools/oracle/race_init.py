"""Reference states for the race start (docs/engine/race-init.md).

    .venv/Scripts/python.exe tools/oracle/cases.py race-init            # every capture in work/<sha8>/race-init/
    .venv/Scripts/python.exe tools/oracle/cases.py race-init sprint

For each NAME_pre capture (`record.py race-init`) it runs the game's own race_start_from_table_a in the
function oracle (no IRQs) and saves the result as NAME_oracle.<domain>.bin; crates/nfsgba-game/tests/race_init.rs
compares the port with it. It also finds the number of VBlank IRQs that ran before setup_race_cars read the tick
counter as the rand seed: the one count for which the oracle equals mGBA's NAME_post outside the IRQs' own writes
(IRQ_WRITES), saved as NAME_seed.txt; the Rust test then compares the port with mGBA directly.
"""
import sys

from common import data_dir
from oracle import REGIONS, Gba

WORK = data_dir() / "work" / "e5298b24" / "race-init"
# What the VBlank and VCount IRQs write while the race start runs (vblank_irq, the sound mix, the counters), the
# IRQ stack, and the hardware registers that change with time. The rand index follows the tick counter the
# IRQs advance (setup_race_cars seeds it with 0x03000044).
IRQ_WRITES = [
    (0x0200_0000, 0x0204_0000, "sound engine work area (checked below to lie inside the engine's block)"),
    (0x0300_0044, 0x0300_0048, "tick counter"),
    (0x0300_53B4, 0x0300_53B8, "VBlank counter"),
    (0x0300_56E8, 0x0300_56EC, "sky gradient read pointer"),
    (0x0300_5724, 0x0300_572C, "IRQ counter and flag"),
    (0x0300_5DEC, 0x0300_5F4C, "sound mix buffers"),
    (0x0300_6378, 0x0300_6380, "mixer counters"),
    (0x0300_64C8, 0x0300_64CC, "rand index (seeded from the tick counter)"),
    (0x0300_7B00, 0x0300_8000, "IRQ stack and BIOS IRQ area"),
    (0x0400_0004, 0x0400_0008, "DISPSTAT, VCOUNT"),
    (0x0400_00A0, 0x0400_00A8, "sound FIFOs"),
    (0x0400_00BC, 0x0400_00D4, "sound DMA 1/2 restarted by the VBlank IRQ"),
]
TICK = 0x0300_0044


def unexplained(r, name, engine):
    """Bytes where the oracle result `r` and mGBA's NAME_post differ outside IRQ_WRITES."""
    out = []
    for dom, base, size in REGIONS:
        if dom == "bios":
            continue
        ours, post = r.read(base, size), (WORK / f"{name}_post.{dom}.bin").read_bytes()[:size]
        for i in (i for i in range(size) if post[i] != ours[i]):
            a = base + i
            why = next((w for lo, hi, w in IRQ_WRITES if lo <= a < hi), None)
            if why and why.startswith("sound engine") and not engine <= a < engine + 0x26AC:
                why = None
            if not why:
                out.append(a)
    return out


def run(name):
    gba = Gba(f"race-init/{name}_pre")
    call = lambda k: gba.call(0x08139E34, mode="thumb", regs={"r0": 0x030000C0}, max_insns=200_000_000,
                              mem=[(TICK, (tick + k).to_bytes(4, "little"))])
    iwram = (WORK / f"{name}_pre.iwram.bin").read_bytes()
    engine, tick = (int.from_bytes(iwram[a:a + 4], "little") for a in (0x6370, 0x44))
    r = call(0)
    if r.stop != "return":
        sys.exit(f"{name}: the oracle stopped: {r.stop} {r.notes}")
    for dom, base, size in REGIONS:
        if dom != "bios":
            (WORK / f"{name}_oracle.{dom}.bin").write_bytes(r.read(base, size))
    # The IRQs before setup_race_cars advance the tick counter it seeds the RNG with: find how many.
    post_rand = (WORK / f"{name}_post.iwram.bin").read_bytes()[0x64C8]
    fits = [k for k in range(0, 40) if (rk := call(k)).read(0x030064C8, 1)[0] == post_rand
            and not unexplained(rk, name, engine)]
    print(f"{name}: oracle state saved; with the tick advanced by {fits} VBlanks the oracle equals mGBA outside "
          f"the IRQ writes (rand index included)")
    if len(fits) == 1:
        (WORK / f"{name}_seed.txt").write_text(f"{fits[0]}\n")
    return len(fits) == 1


def main(argv: list[str]) -> None:
    names = argv or sorted(p.name[:-len("_pre.wram.bin")] for p in WORK.glob("*_pre.wram.bin"))
    ok = all([run(n) for n in names])
    sys.exit(0 if ok else 1)
