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
from oracle import REGS, Gba, _r, _w  # noqa: E402

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


# Game functions the port does not implement yet, with their argument counts. The oracle runs everything else for
# real; these are stubbed on both sides (Rust: `Gba::unported`) and must be called in the same order with the same
# arguments.
KIND_HANDLERS = [
    (0x0812FE38, 0x081303E8, 0x08130D8C, 0x081314A4), (0x0812E80C, 0x0812E3D4, 0x0812E5AC, 0x0812E850),
    (0x0812F204, 0x0812F364, 0x0812F450, 0x0812FC6C), (0x0812E308, 0x0812DAFC, 0x0812DD80, 0x0812E380),
    (0x081328F4, 0x08132AB8, 0x08133074, 0x081336BC), (0x08133708, 0x081338E0, 0x08133F2C, 0x081348D8),
    (0x081315A0, 0x081318E4, 0x08131FE0, 0x08132780), (0x08134DF0, 0x08134EB8, 0x08135340, 0x081354FC),
]
PORTED = {0x081318E4}  # intro_update
STUBS = {a: (1 if i == 2 else 0) for k in KIND_HANDLERS for i, a in enumerate(k) if a not in PORTED}
STUBS.update({
    0x08135FDC: 2, 0x0813550C: 1, 0x081439C0: 1, 0x0812B320: 0,  # sound, message box draw, screen 0x11, screen 15
    0x08160E74: 0, 0x0815E9E8: 2, 0x08139E34: 1, 0x08160D18: 4, 0x0813A514: 1, 0x0813A954: 1,  # game state step
    0x08135F38: 0, 0x0812EAAC: 1, 0x081396C4: 1, 0x08136054: 1,
    0x08162228: 1, 0x0816223C: 1, 0x081621F0: 4, 0x0812B084: 0, 0x08161F38: 1, 0x0816102C: 0,  # main_frame
    0x0815DFD8: 1, 0x0812B040: 0, 0x08142090: 0,
    0x08151454: 0, 0x0815E04C: 3, 0x081372E4: 1, 0x08143010: 1, 0x08139E10: 1,  # vblank wait; goto_screen(0x82)
    0x081419C0: 7, 0x08149FD8: 1, 0x08149D84: 1,  # intro: title text, save write, save load
})
MENU_FRAME, GAME_STATE_STEP, MAIN_FRAME = 0x0812B5F0, 0x0812ACEC, 0x0812AE64
PROFILE_AT = 0x0200_0808


def run(gba, fn, mem, ret):
    """One oracle call with the stubs; returns the result, the writes and the stub calls."""
    calls = []

    def stub(addr, n):
        def f(uc):
            sp = uc.reg_read(REGS["sp"])
            stack = [int.from_bytes(uc.mem_read(sp + 4 * i, 4), "little") for i in range(max(0, n - 4))]
            calls.append([addr, [_r(uc, i) for i in range(min(n, 4))] + stack])
            _w(uc, 0, ret.get(addr, 0))
        return f

    r = gba.call(fn, mem=mem, stubs={a: stub(a, n) for a, n in STUBS.items()})
    assert r.stop == "return", (hex(fn), r.stop)
    return r.regs["r0"], r.writes, calls


def word(v):
    return struct.pack("<I", v & 0xFFFF_FFFF)


def toplevel(_gba, rng, n=2400):
    """menu_frame, game_state_step and main_frame on random menu states over several menu and race snapshots."""
    snaps = ["ui-2d/lang", "ui-2d/n7", "ui-2d/n9", "race-rules/a4", "race-rules/v2", "mgba/race"]
    gbas = {s: Gba(s) for s in snaps}
    cases = []
    for i in range(n):
        snap = rng.choice(snaps)
        fn = [MENU_FRAME, GAME_STATE_STEP, MAIN_FRAME][i % 3]
        pick = rng.choice
        mem = [
            (0x030056EC, word(PROFILE_AT)),
            (0x03005944, word(pick(list(range(0x31)) + [0x31, 0x40, 0x7F, 0x80, 0x81, 0x81, 0x90]))),
            (0x03005780, word(pick([0] * 6 + [7, 7, 3]))),
            (0x03005630, word(pick([0] * 6 + [-16, 16, 2, -2, 5]))),
            (0x0300594C, word(pick([-1, rng.randrange(0x31), rng.randrange(0x31), 0x31, 0x40]))),
            (0x030059F0, word(pick([-1] * 5 + [0, 1, 2]))),
            (0x030064C0, struct.pack("<H", pick([0, 1, 2, 2, 2, 0x20, 0x10, 0x40, 0x80, 0x100, 0x200, 3,
                                                 rng.randrange(0x400)]))),
            (0x0300593C, bytes([pick([0xFF, 0, 1, 2, 3, 7])])),
            (0x03005948, word(pick([0, 0, 1]))),
            (0x030000A0, word(pick([0, 1]))),
            (0x03005388, word(rng.randrange(43))),
            (0x03005610, word(pick([0, 1]))),
            (0x030064C8, word(rng.randrange(256))),
            (0x03005808, word(pick([0, 1, 1, 1, 4, 5, 5, 2]))),
            (0x03000048, word(pick([5, 3]))),
            (0x03005624, word(pick([0, 0, 0, 2]))),
            (0x0300563C, word(pick([0, 1]))),
            (PROFILE_AT + 0x33C, bytes(pick([0, 1, 2, 3, 0xFE, 0xFF]) for _ in range(8))),
            (PROFILE_AT + 0x344, bytes(rng.randrange(0x31) for _ in range(8))),
            (PROFILE_AT + 0x404, bytes([pick([0, 3, 2])])),
            (PROFILE_AT + 0x10, bytes([rng.randrange(15), rng.randrange(15), pick([0, 1]), 0])),
            (PROFILE_AT + 0x42D + 0x21, bytes([rng.randrange(256), rng.randrange(256)])),
            (0x05000000, bytes(rng.randrange(256) for _ in range(0x400))),
            # The credits screen (0x15) reads a list its enter handler sets up; give it one.
            (0x03005964, word(0x0201_0000)), (0x0201_0000, bytes([1, 0, 0, 0, 0, 0, 0, 0])),
        ]
        if rng.random() < 0.3:
            mem.append((0x03005620, word(0)))  # no level descriptor: no gradient fade
        upd = KIND_HANDLERS
        ret = {h[1]: pick([0, 1, 1, 2]) for h in upd}
        ret.update({0x0813A954: pick([0, 1]), 0x08160E74: rng.randrange(1 << 32),
                    0x0816223C: pick([0, 1, 9, 0x100, 0x200, 0x639C, 0x1000, 3000, rng.randrange(0x10000)])})
        r0, writes, calls = run(gbas[snap], fn, mem, ret)
        cases.append(dict(snap=snap, fn=hex(fn), mem=[[a, b.hex()] for a, b in mem],
                          ret={hex(a): v for a, v in ret.items()}, r0=r0,
                          writes=[[a, b.hex()] for a, b in writes], calls=calls))
    return cases


def intro(_gba, rng, n=2400):
    """menu_frame on the intro screens (intro_update ported): keys, deadlines, the name keyboard, languages, credits,
    the health screen blink and the title's profile paths."""
    snaps = ["ui-2d/lang", "ui-2d/n1", "ui-2d/n7", "ui-2d/n9", "race-rules/a4"]
    gbas = {s: Gba(s) for s in snaps}
    cases = []
    credits = 0x0201_0000
    for i in range(n):
        snap = rng.choice(snaps)
        pick = rng.choice
        screen = pick([0x15, 0x16, 0x16, 0x16, 0x17, 0x18, 0x19, 0x19, 0x1A, 0x25, 0x2F, 0x30, 0x30])
        ticks = rng.randrange(-5, 5000)
        name = bytes(pick([0, 0x20, 0x41, 0x31, 0x2E]) for _ in range(9))
        mem = [
            (0x030056EC, word(PROFILE_AT)),
            (0x03005944, word(screen)),
            (0x03005780, word(0)), (0x03005630, word(0)), (0x030059F0, word(-1)),
            (0x030064C0, struct.pack("<H", pick([0, 1, 2, 8, 8, 0x10, 0x20, 0x40, 0x80, 0x100, 0x200, 3, 0xB, 0x50,
                                                 0xA0, 0x30, 9, rng.randrange(0x400)]))),
            (0x0300593C, bytes([pick([0xFF, 0, 1, 2])])),
            (0x03000044, word(ticks)),
            (PROFILE_AT + 0x3B0, word(ticks + pick([-1, 0, 1, -100, 100]))),
            (0x0300597C, word(pick([0, 1, 2, 3, 4, 4]))),
            (0x03005990, word(rng.randrange(10))),
            (0x03005970, name),
            (0x0300598C, word(pick([0, 1, 5, 7, 8, rng.randrange(9)]))),
            (0x03005960, word(rng.randrange(5))),
            (0x03005600, word(rng.randrange(5))),
            (0x03005964, word(credits + 2 * rng.randrange(4))),
            (credits, bytes(pick([0, 0, 1, 2]) if k % 2 == 0 else 0 for k in range(64))),
            (PROFILE_AT + 0x490, struct.pack("<HH", pick([0, 1]), pick([0, 2, 2, 1]))),
            (PROFILE_AT + 0x4E8, struct.pack("<H", rng.randrange(5))),
            (0x03000000, word(pick([0, 1]))),
            (PROFILE_AT + 0x344, bytes(rng.randrange(0x31) for _ in range(8))),
        ]
        second = struct.unpack("<I", gbas[snap].read_base(0x0300577C, 4))[0]
        if 0x0200_0000 <= second < 0x0204_0000:
            mem.append((second + 8, struct.pack("<H", pick([0, 0x421, 0x7FFF, 0x7BDE, rng.randrange(0x8000)]))))
        ret = {h[1]: pick([0, 1]) for h in KIND_HANDLERS}
        ret.update({0x08149FD8: pick([0, 0, 1])})
        r0, writes, calls = run(gbas[snap], MENU_FRAME, mem, ret)
        cases.append(dict(snap=snap, fn=hex(MENU_FRAME), mem=[[a, b.hex()] for a, b in mem],
                          ret={hex(a): v for a, v in ret.items()}, r0=r0,
                          writes=[[a, b.hex()] for a, b in writes], calls=calls))
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
