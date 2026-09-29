"""Oracle cases for the menu port (crates/nfsgba-formats/src/menu.rs): runs the game's own functions in unicorn
(tools/oracle) on generated inputs and saves the cases as JSONL under $NFSGBA_DATA/work/<sha8>/menus/.

    .venv/Scripts/python.exe tools/ui_menu_oracle.py [fades] [...]

Each case holds the inputs and every byte the game's code changed, so the Rust tests replay them without unicorn.
"""
import json
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent / "oracle"))
from oracle import Gba  # noqa: E402

from common import data_dir  # noqa: E402

OUT = data_dir() / "work" / "e5298b24" / "menus"
BUF, TARGET = 0x0201_0000, 0x0201_0400  # scratch EWRAM for palette buffers (restored after every call)
FADE_IN, FADE_OUT = 0x0815_E530, 0x0815_E4D4  # buffer variants: (buf, target, first, n, step) / (buf, first, n, step)
FADE_IN_BG, FADE_OUT_BG = 0x0815_E2F0, 0x0815_E290  # palette RAM 0x05000000: (target, first, n, step) / (first, n, step)
FADE_IN_OBJ, FADE_OUT_OBJ = 0x0816_1838, 0x0816_17D4  # palette RAM 0x05000200, same arguments


def colour(rng):
    """Random BGR555 with channel edge values over-represented (0, 1, step-ish, 30, 31) and bit 15 at times."""
    ch = lambda: rng.choice([0, 1, 2, 3, 4, 15, 16, 27, 28, 29, 30, 31, rng.randrange(32)])
    return ch() | ch() << 5 | ch() << 10 | (rng.random() < 0.1) << 15


def hexs(words):
    return struct.pack(f"<{len(words)}H", *words).hex()


def after(r, addr, before: bytes) -> bytes:
    """`before` (our own `mem=` input at `addr`) with the call's writes applied. `Result.read` rebuilds from the
    snapshot, not from the call's `mem` inputs, so bytes the call left unchanged would read back as the snapshot's."""
    out = bytearray(before)
    for a, b in r.writes:
        lo, hi = max(a, addr), min(a + len(b), addr + len(out))
        if lo < hi:
            out[lo - addr:hi - addr] = b[lo - a:hi - a]
    return bytes(out)


def fades(gba, rng, n=600):
    cases = []
    for i in range(n):
        kind = ["in", "out", "in_bg", "out_bg", "in_obj", "out_obj"][i % 6]
        first, count = rng.choice([(0, 256), (0, 0x100), (0, 0x200 // 2), (rng.randrange(256), rng.randrange(1, 64))])
        count = min(count, 256 - first)
        step = rng.choice([1, 2, 4, 4, 8, 16, 31, 32])
        before = [colour(rng) for _ in range(256)]
        target = [colour(rng) for _ in range(256)]
        if kind in ("in", "out"):
            mem = [(BUF, bytes.fromhex(hexs(before))), (TARGET, bytes.fromhex(hexs(target)))]
            fn = FADE_IN if kind == "in" else FADE_OUT
            regs = dict(r0=BUF, r1=TARGET, r2=first, r3=count) if kind == "in" else dict(r0=BUF, r1=first, r2=count, r3=step)
            stack = [step] if kind == "in" else []
            at = BUF
        else:
            at = 0x0500_0000 if kind.endswith("bg") else 0x0500_0200
            mem = [(at, bytes.fromhex(hexs(before))), (TARGET, bytes.fromhex(hexs(target)))]
            fn = {"in_bg": FADE_IN_BG, "out_bg": FADE_OUT_BG, "in_obj": FADE_IN_OBJ, "out_obj": FADE_OUT_OBJ}[kind]
            regs = dict(r0=TARGET, r1=first, r2=count, r3=step) if kind.startswith("in") else dict(r0=first, r1=count, r2=step)
            stack = []
        r = gba.call(fn, regs=regs, stack=stack, mem=mem)
        assert r.stop == "return", r.stop
        cases.append(dict(kind=kind, first=first, count=count, step=step, before=hexs(before), target=hexs(target),
                          after=after(r, at, bytes.fromhex(hexs(before))).hex()))
    return cases


def main(which):
    OUT.mkdir(parents=True, exist_ok=True)
    gba, rng = Gba("ui-2d/n7"), random.Random(0x6E66)
    for name in which or ["fades"]:
        cases = globals()[name](gba, rng)
        path = OUT / f"{name}.jsonl"
        path.write_text("".join(json.dumps(c) + "\n" for c in cases), encoding="utf-8")
        print(f"{name}: {len(cases)} cases -> {path}")


if __name__ == "__main__":
    main(sys.argv[1:])
