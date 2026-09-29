"""The race start's inputs from the previous scene (docs/engine/race-init.md): every RAM, I/O and VRAM byte that
race_start_from_table_a reads before writing it, found by running the game's code in the function oracle with a
read hook on a capture (tools/race_init_capture.py).

    .venv/Scripts/python.exe tools/race_init_inputs.py [NAME]      # default: career

Prints the read-before-write ranges grouped by region, and saves them as NAME_inputs.txt next to the capture.
"""
import sys
from pathlib import Path

import unicorn

sys.path.insert(0, str(Path(__file__).resolve().parent / "oracle"))
sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import data_dir  # noqa: E402
from oracle import Gba  # noqa: E402

WORK = data_dir() / "work" / "e5298b24" / "race-init"


def inputs(name):
    gba = Gba(f"race-init/{name}_pre")
    written, read_first = set(), set()

    def hook(uc, access, addr, size, value, _):
        if not 0x0200_0000 <= addr < 0x0800_0000:
            return
        for a in range(addr, addr + size):
            if access == unicorn.UC_MEM_WRITE:
                written.add(a)
            elif a not in written:
                read_first.add(a)

    h = gba.uc.hook_add(unicorn.UC_HOOK_MEM_READ | unicorn.UC_HOOK_MEM_WRITE, hook)
    try:
        r = gba.call(0x08139E34, mode="thumb", regs={"r0": 0x030000C0}, max_insns=200_000_000)
    finally:
        gba.uc.hook_del(h)
    assert r.stop == "return", r.stop
    runs = []
    for a in sorted(read_first):
        if runs and a == runs[-1][1] + 1:
            runs[-1][1] = a
        else:
            runs.append([a, a])
    return runs


if __name__ == "__main__":
    name = (sys.argv[1:] or ["career"])[0]
    runs = inputs(name)
    lines = [f"{a:#010x}..={b:#010x} ({b - a + 1} bytes)" for a, b in runs]
    (WORK / f"{name}_inputs.txt").write_text("\n".join(lines) + "\n")
    by = {}
    for a, b in runs:
        by.setdefault(a >> 24, [0, 0])
        by[a >> 24][0] += 1
        by[a >> 24][1] += b - a + 1
    for reg, (n, total) in sorted(by.items()):
        print(f"region {reg:#04x}: {total} bytes in {n} ranges")
    print("\n".join(lines))
