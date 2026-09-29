"""Oracle cases for the menu port (crates/nfsgba-game/src/menu/): runs the game's own functions in unicorn
(tools/oracle) on generated inputs and saves the cases as JSONL under $NFSGBA_DATA/work/<sha8>/menus/.

    .venv/Scripts/python.exe tools/oracle/cases.py menus [fades] [...]

Each case holds the inputs and every byte the game's code changed, so the Rust tests replay them without unicorn.
"""
import json
import random
import struct

from oracle import REGS, Gba, _r, _w

from common import data_dir

OUT = data_dir() / "work" / "e5298b24" / "menus3"  # menus-3 cases: the drawing primitives run for real (menus2/ stubbed them)
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
KIND_SCREENS = {  # kind name -> (index in KIND_HANDLERS, screens)
    "list": (0, [0, 1, 2, 3, 4, 5, 6, 9, 27, 28, 29, 30, 35, 36, 45, 46]), "kind7": (1, [7, 8, 14, 17]),
    "career": (2, [11, 12]), "event": (3, [13]), "setup": (4, [10, 15, 16]), "kind18": (5, [18, 19, 20]),
    "intro": (6, [21, 22, 23, 24, 25, 26, 37, 47, 48]), "kind38": (7, [38, 39, 40, 41, 42, 43]),
}
# Ported handlers: every exit (each is only `FUN_081372D8(world)`), the intro kind and those listed per kind.
PORTED_KINDS = ["intro", "kind7", "event", "career", "setup", "kind38", "list"]
PORTED = {k[3] for k in KIND_HANDLERS} | {a for n in PORTED_KINDS for a in KIND_HANDLERS[KIND_SCREENS[n][0]]}
STUBS = {a: (1 if i == 2 else 0) for k in KIND_HANDLERS for i, a in enumerate(k) if a not in PORTED}
STUBS.update({
    0x08135FDC: 2,  # sound
    0x08160E74: 0, 0x0815E9E8: 2, 0x08139E34: 1, 0x0813A514: 1, 0x0813A954: 1,  # game state step
    0x08135F38: 0, 0x0812EAAC: 1, 0x081396C4: 1, 0x08136054: 1,
    0x08162228: 1, 0x0816223C: 1, 0x081621F0: 4, 0x0812B084: 0, 0x08161F38: 1, 0x0816102C: 0,  # main_frame
    0x0815DFD8: 1, 0x0812B040: 0, 0x08142090: 0,
    0x08151454: 0, 0x081372E4: 1, 0x08143010: 1, 0x08139E10: 1,  # vblank wait; goto_screen(0x82)
    0x08149FD8: 1, 0x08149D84: 1,  # intro: save write, save load
    0x0813644C: 2, 0x081356DC: 0,  # intro enter: health image, profile_reset
    0x08139C8C: 2, 0x08163D30: 2, 0x08161EEC: 2, 0x08161C24: 2,  # menu scene: descriptor, unpack, sprite screen
    0x081364C4: 1,  # intro page setup (clears the page)
    0x081372D8: 1,  # scene teardown (every exit handler)
    0x08143284: 0, 0x081435C4: 0,  # map screens: zone colours, map draw
    0x0812EFE8: 0,  # career_race_payout
    0x08136028: 1, 0x0815240C: 0,  # stop sound, stop music
    0x0812BEEC: 0, 0x0812BF48: 0, 0x0812BFA4: 3, 0x0812FFB0: 0,  # garage car atlas, palette, draw; quick race
    0x0812C81C: 1, 0x0812C8A8: 1, 0x081302C4: 1, 0x0812C5C4: 1,  # unlock state, buy, upgrades changed, owned
    0x081300E0: 2, 0x08133D30: 4, 0x0812D960: 1,  # list item new mark, car stats, unlock price
})
STACK_TEXT = (0x0300_7000, 0x0300_7C00)  # stub pointer arguments in here are the game's stack strings
STRUCT_ARGS = {}  # stub: (argument, bytes) passed by pointer to a stack struct
UNPACK_TO_BUFFER = 0x08163D30  # stays a logged stub, but leaves the material's pixels where the blit reads them
MENU_FRAME, GAME_STATE_STEP, MAIN_FRAME, GOTO_SCREEN = 0x0812B5F0, 0x0812ACEC, 0x0812AE64, 0x0812BB5C
DRAW_SCREEN = 0x0812D334
PROFILE_AT = 0x0200_0808


class Skip(Exception):
    pass


def game_unpack(read, src):
    """The game's ring LZ77 decoder (`ui.rs` `unpack`/`ring_decode`, ported as is): the bytes it writes."""
    data = read(src, 0x10000)
    size = int.from_bytes(data[:4], "little") >> 8
    at = 4
    ring, pos, out, stored, left = bytearray([0xFF]) * 0x1000, 0xFEE, bytearray(), 0, size
    flags = bit = 7

    def nxt():
        nonlocal at
        at += 1
        return data[at - 1]

    while True:
        flags = flags << 1 & 0xFFFF_FFFF
        bit += 1
        if bit == 8:
            bit, flags = 0, nxt()
        if not flags & 0x80:
            b = nxt()
            if stored < size:
                left -= 1
                out.append(b)
                if left <= 0:
                    return bytes(out)
            ring[pos] = b
            stored += 1
            pos = pos + 1 & 0xFFF
            continue
        hi, lo = nxt(), nxt()
        for _ in range((hi >> 4) + 3):
            b = ring[pos - ((hi & 0xF) << 8 | lo) - 1 & 0xFFF]
            if stored < size:
                left -= 1
                out.append(b)
                if left <= 0:
                    break
            ring[pos] = b
            stored += 1
            pos = pos + 1 & 0xFFF


def run(gba, fn, mem, ret, regs=None, stack=()):
    """One oracle call with the stubs; returns the result, the writes and the stub calls."""
    calls = []

    def cstring(uc, a):
        s = bytes(uc.mem_read(a, 64))
        return ["s", s[:s.index(0)].hex() if 0 in s else s.hex()]

    def stub(addr, n):
        def f(uc):
            sp = uc.reg_read(REGS["sp"])
            stack = [int.from_bytes(uc.mem_read(sp + 4 * i, 4), "little") for i in range(max(0, n - 4))]
            args = [_r(uc, i) for i in range(min(n, 4))] + stack
            args = [cstring(uc, a) if STACK_TEXT[0] <= a < STACK_TEXT[1] else a for a in args]
            if addr in STRUCT_ARGS:
                k, size = STRUCT_ARGS[addr]
                args[k] = ["s", bytes(uc.mem_read(_r(uc, k), size)).hex()]
            calls.append([addr, args])
            if addr == UNPACK_TO_BUFFER:  # into the snapshot itself: the buffer is scratch, not a result
                gba.poke(_r(uc, 1), game_unpack(gba.read_base, _r(uc, 0)))
            _w(uc, 0, ret.get(addr, 0))
        return f

    r = gba.call(fn, mem=mem, regs=regs or {}, stack=stack, stubs={a: stub(a, n) for a, n in STUBS.items()})
    if r.stop != "return":  # the game draws off the mapped memory, or the oracle meets an unaligned store: no case
        raise Skip(hex(fn), r.stop)
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
        try:
            r0, writes, calls = run(gbas[snap], fn, mem, ret)
        except Skip:
            continue
        cases.append(dict(snap=snap, fn=hex(fn), mem=[[a, b.hex()] for a, b in mem],
                          ret={hex(a): v for a, v in ret.items()}, r0=r0,
                          writes=[[a, b.hex()] for a, b in writes], calls=calls))
    return cases


def credits_lists(rng):
    """Credits lists back to back (u16 count, then count (flags, key) pairs), ended by a count of 0."""
    out = []
    for _ in range(3):
        c = rng.choice([0, 1, 2, 3, 5])
        out.append(c)
        for _ in range(c):
            f = rng.choice([0, 1, 2, 3]) | rng.choice([0, 0x2000, 0x1000, 0x800, 0x3800, 0x8000, 0x4000, 0xC000])
            out += [f, rng.choice([0x1B, 0x1D, 0x1F, 0x19A, rng.randrange(900)])]
    out.append(0)
    return struct.pack(f"<{len(out)}H", *out)


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
            (0x03005964, word(credits)),
            (credits, credits_lists(rng)),
            (0x030053B4, word(rng.randrange(0x40))),
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
        fn, arg = (GOTO_SCREEN, pick([0x15, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x25, 0x2F, 0x81, 0x80, 0x82, 0x90]))             if i % 3 == 0 else (DRAW_SCREEN, pick([0, 1])) if i % 3 == 1 else (MENU_FRAME, 0)
        mem.append((0x03005938, word(pick([0, 1]))))
        mem.append((0x03005698, word(pick([0, 1]))))
        mem.append((PROFILE_AT, name[::-1]))
        mem.append((PROFILE_AT + 0x478, bytes(rng.randrange(256) for _ in range(0x18))))  # cleared by the name screen
        try:
            r0, writes, calls = run(gbas[snap], fn, mem, ret, regs={"r0": arg})
        except Skip:
            continue
        cases.append(dict(snap=snap, fn=hex(fn), arg=arg, mem=[[a, b.hex()] for a, b in mem],
                          ret={hex(a): v for a, v in ret.items()}, r0=r0,
                          writes=[[a, b.hex()] for a, b in writes], calls=calls))
    return cases


MENU_SNAPS = ["ui-2d/lang", "ui-2d/n7", "ui-2d/n9", "race-rules/a4", "race-rules/v2"]
SETUP_COUNTS_GBA = Gba("ui-2d/n7")  # reads the setup screens' item counts from the ROM


def menu_state(rng, screens):
    """The menu globals every kind reads, randomised."""
    pick = rng.choice
    return [
        (0x030056EC, word(PROFILE_AT)),
        (0x03005944, word(pick(screens))),
        (0x03005780, word(pick([0] * 4 + [7]))),
        (0x03005630, word(pick([0] * 4 + [16, -16]))),
        (0x030059F0, word(pick([-1] * 4 + [0, 1, 2]))),
        (0x030064C0, struct.pack("<H", pick([0, 1, 1, 2, 8, 0x10, 0x20, 0x30, 0x40, 0x80, 0x100, 0x200, 4,
                                             rng.randrange(0x400)]))),
        (0x0300593C, bytes([pick([0xFF, 0, 1, 2, 3])])),
        (PROFILE_AT + 0x344, bytes(rng.randrange(0x31) for _ in range(8))),
        (PROFILE_AT + 0x33C, bytes(pick([0, 1, 3, 0xFF]) for _ in range(8))),
        (0x03005938, word(pick([0, 1]))),
        (0x03005948, word(pick([0, 1]))),
        (0x030000A0, word(pick([0, 1, 2]))),
        (0x03005600, word(rng.randrange(5))),
        (0x03005388, word(rng.randrange(1, 43))),  # route numbers 1..42 (0x7E49C4 has 43 entries)
        (0x03005610, word(pick([0, 1]))),
        (0x030064C8, word(rng.randrange(256))),
        (PROFILE_AT + 0x42D, bytes(pick([0, 0xFF, rng.randrange(256)]) for _ in range(40))),  # unlock bits
        (PROFILE_AT + 0x10, bytes([rng.randrange(15), rng.randrange(15), pick([0, 1]), 0])),
    ]


def record_ties(rng):
    """Track records (profile +0x218) and best laps (results +0x10, both copies) often equal, above or below."""
    if rng.random() < 0.5:
        return [(PROFILE_AT + 0x218, bytes(rng.randrange(256) for _ in range(48)))]
    v = rng.randrange(1, 0xFFFF)
    lap = v + rng.choice([0, 0, -1, 1])
    return [(PROFILE_AT + 0x218, struct.pack("<24H", *[v] * 24)),
            (0x03005660, struct.pack("<4i", *[lap] * 4)), (0x03005740, struct.pack("<4i", *[lap] * 4))]


def kind_extra(rng, kind):
    """The state a kind's handlers read beyond `menu_state`."""
    pick = rng.choice
    if kind == "kind7":
        return [
            (0x03006230, word(rng.randrange(1 << 16)) + word(rng.randrange(1 << 16))
             + bytes([pick(list(range(-2, 20))) & 0xFF, pick([0, 1])])),
            (PROFILE_AT + 0x404, bytes([pick([0, 0, 1, 2, 3, 4])])),
            (PROFILE_AT + 0x1FB, bytes([rng.randrange(8)])),
        ]
    if kind == "setup":
        counts = [struct.unpack("<H", SETUP_COUNTS_GBA.read_base(0x087E6260 + 0x10 * i + 10, 2))[0] for i in range(6)]
        return [
            (0x030056E0, word(rng.randrange(4))),
            (PROFILE_AT + 0x368, bytes(rng.randrange(c) for c in counts)),
            (PROFILE_AT + 0x374, bytes(pick([0, 1, 3, 0xFF]) for _ in range(16))),
            (PROFILE_AT + 0x3BC, b"".join(word(pick([0, 0, 1, 2, 3, 4, -1, 7])) for _ in range(16))),
            (PROFILE_AT + 0x200, word(pick([0, 0, 1, 2, 7, 12]))),
            (PROFILE_AT + 0x1F8, bytes([pick([0, 0, 1, 2]), pick([0, 0, 1]), 0, rng.randrange(6),
                                        rng.randrange(12)])),  # hints, zone, event slot
            (PROFILE_AT + 0x10, bytes([rng.randrange(15), rng.randrange(15)])),
            (0x03005784, word(pick([0, 1, 2, 3]))),
            (0x03005998, word(pick([0, 1]))),
            (0x030059F4, word(pick([0, 0, 1, -1]))),
            (0x030053B4, word(rng.randrange(0x100))),
            (0x030053E4, word(pick([0, 1, 2]))), (0x03000040, word(pick([0, 1]))), (0x03005698, word(pick([0, 1]))),
            (0x03005798, word(pick([0, 1]))), (0x0300578C, word(pick([0, 8, 0x10, 0x3F]))),
            (0x030053A4, word(pick([0, 8, 0x10, 0x3F]))), (0x03000050, word(pick([0, 1]))),
            (0x030064C0, struct.pack("<H", pick([0, 1, 1, 1, 0x10, 0x20, 0x40, 0x80, 0x200, 0x30]))),
        ]
    if kind == "list":
        counts = [struct.unpack("<h", SETUP_COUNTS_GBA.read_base(0x087E544C + 0x14 * i + 10, 2))[0] for i in range(17)]
        stack = bytes(pick([0xD, 0x26, 7, 8, 9, 0xF, 3, 0, rng.randrange(0x31)]) for _ in range(8))
        return [
            (PROFILE_AT + 0x350, bytes(rng.randrange(max(c, 1)) for c in counts)),
            (PROFILE_AT + 0x10, bytes([rng.randrange(15), rng.randrange(15), pick([0, 1]), 0])),
            (PROFILE_AT + 0x200, word(pick([0, 0, 1, 2, 5, 10, 11, 12])) + bytes([pick([0, 1])])),
            # Cash above 999,999 sends thousands_separator down its stale-register path (NOT 1:1, noted).
            (PROFILE_AT + 0xC, word(pick([0, 999, 1000, 12345, 250000, 999999, rng.randrange(1000000)]))),
            (PROFILE_AT + 0x1F8, bytes([pick([0, 1, 2]), pick([0, 0, 1]), 0, rng.randrange(6)])),
            (PROFILE_AT + 0x256, struct.pack("<HH", pick([0, 1]), pick([0, 1]))),
            (PROFILE_AT + 0x344, stack),
            (0x0300593C, bytes([pick([0xFF, 0, 1, 2, 3])])),
            (0x03005718, word(rng.randrange(15))),
            (0x030056E0, word(rng.randrange(4))),
            (0x03005784, word(pick([1, 2, 3]))),
            (0x030059F4, word(pick([0, 1, 1, -1]))),
            (0x03005954, word(pick([0, 1]))),
            (0x030059F0, word(pick([-1, -1, -1, 0]))),  # message box closed, so the 0x8B question opens
            (0x030064C4, struct.pack("<H", pick([0, 0x40, 0x80, 0xC0]))),
            (0x03005F9C, word(rng.randrange(1 << 16))),
            (0x03000060, word(rng.randrange(4))),
            (0x030064C0, struct.pack("<H", pick([0, 1, 1, 1, 0x10, 0x20]))),
        ]
    if kind == "kind38":
        hint = pick([0, 1, 2, 3, 2])
        page = pick([0, 1, 2, 3])
        return [
            (PROFILE_AT + 0x1F8, bytes([hint, pick([0, 0, 1, 2]), page, rng.randrange(6)])),
            (PROFILE_AT + 0x254, struct.pack("<hH", pick(list(range(12)) + [-1, 0x10]), pick([0, 1]))),
            (PROFILE_AT + 0x200, word(pick([0, 1, 2, 5, 12]))),
            (0x030056E0, word(rng.randrange(4))),
            (0x03000070, word(rng.randrange(1 << 32))),
            (0x030053B4, word(1000)), (0x030059E8, word(pick([0, 900, 1000, 1001, 1200]))),
            (0x03005630, word(pick([0, 0, 0, 16]))),
            (0x030064C0, struct.pack("<H", pick([0, 1, 1, 2, 2, 0x10, 0x20, 0x40, 0x80]))),
        ]
    if kind == "career":
        keys = [pick([0x31A, 0x194, 0x39A, 0xC6, 0x3CF, 0xA3, 0x3C1, 0x100, 0x2AB]) for _ in range(pick([0, 1, 2, 4]))]
        stack = bytes(pick([0xC, 0xC, 6, 5, 0xB, rng.randrange(0x31)]) for _ in range(8))
        return [
            (0x03005650, bytes(rng.randrange(256) for _ in range(0x40))),
            (0x03005730, bytes(rng.randrange(256) for _ in range(0x40))),
            (0x03005658, bytes(pick([0, 8, 8, 1]) for _ in range(4))),
            (0x03005738, bytes(pick([0, 8, 8, 1]) for _ in range(4))),  # knocked out, in the ranked copy too
            (0x03005784, word(pick([0, 1, 2, 3, 3, 3]))),
            (0x030056E0, word(pick([0, 1, 2, 3, 3, 5, -1]))),
            (0x03000048, word(pick([2, 3, 5, 6, 7, 8, 9]))),
            (0x030000A0, word(pick([0, 1, 2]))),
            (0x03005388, word(rng.randrange(1, 43))),  # route numbers 1..42 (0x7E49C4 has 43 entries)
            (PROFILE_AT + 0x3B4, word(pick([0, 1])) + word(pick([0, 0, rng.randrange(100000), -5]))),
            (PROFILE_AT + 0x4A8, struct.pack(f"<H{len(keys)}HH", pick([0, 1]), *keys, 0)),
            *record_ties(rng),
            (PROFILE_AT + 0x344, stack),
            (0x0300593C, bytes([pick([0xFF, 0, 1, 2, 3, 7])])),
            (0x030064C0, struct.pack("<H", pick([0, 1, 1, 1, 2, 0x10]))),
        ]
    if kind == "event":
        zone = rng.randrange(6)
        cursors = bytes(rng.randrange(6 if z == 5 else 12) for z in range(6))
        return [
            (PROFILE_AT + 0x1FB, bytes([zone])),
            (PROFILE_AT + 0x388, cursors),
            (PROFILE_AT + 0x205, bytes(pick([0, 0x55, 0xAA, 0xFF, rng.randrange(256)]) for _ in range(18))),
            (PROFILE_AT + 0x1F8, bytes([pick([0, 0, 1, 2, 3]), rng.randrange(0x19), rng.randrange(256)])),
            (PROFILE_AT + 0x1FC, bytes([rng.randrange(256)])),
            (PROFILE_AT + 0x450, bytes(rng.randrange(256) for _ in range(4))),
            (PROFILE_AT + 0x12, struct.pack("<H", pick([0, 1]))),
            (PROFILE_AT + 0x218, bytes(rng.randrange(256) for _ in range(48))),
            (0x03000070, word(rng.randrange(1 << 32))),
            (0x030056E0, word(pick([0, 1, 2, 3, 40]))),
            (0x030053BC, word(rng.randrange(1 << 32))),
            (0x03000060, word(rng.randrange(4))),
            (0x030064C0, struct.pack("<H", pick([0, 1, 1, 1, 0x200, 0x10, 0x20, 0x40, 0x80, 0x30, 0xC0, 3]))),
        ]
    return []


KIND_ENTRIES = {"setup": [(0x0812BB5C, 15)]}  # goto_screen(15)


def kind_cases(rng, kind, n):
    """Each handler of `kind` (enter, update, draw with `full`, exit) called directly on random menu states."""
    gbas = {s: Gba(s) for s in MENU_SNAPS}
    # The kind's four handlers, plus screen entries that run code outside them (enter_screen: screen 15 picks
    # the career opponents first).
    entries = [(h, None) for h in KIND_HANDLERS[KIND_SCREENS[kind][0]]] + KIND_ENTRIES.get(kind, [])
    cases = []
    for i in range(n):
        snap = rng.choice(MENU_SNAPS)
        mem = menu_state(rng, KIND_SCREENS[kind][1]) + kind_extra(rng, kind)
        fn, arg = entries[i % len(entries)]
        arg = rng.choice([0, 1]) if arg is None else arg
        ret = {0x08149FD8: rng.choice([0, 0, 1])}
        if kind == "list":
            pick = rng.choice
            ret.update({0x0812C81C: pick([0, 1, 2, 3, 5]), 0x0812C5C4: pick([0, 1]), 0x081300E0: pick([0, 1]),
                        0x081302C4: pick([0, 1]), 0x0812D960: pick([0, 500, 12000, 150000]),
                        })
        try:
            r0, writes, calls = run(gbas[snap], fn, mem, ret, regs={"r0": arg})
        except Skip:
            continue
        cases.append(dict(snap=snap, fn=hex(fn), arg=arg, mem=[[a, b.hex()] for a, b in mem],
                          ret={hex(a): v for a, v in ret.items()}, r0=r0,
                          writes=[[a, b.hex()] for a, b in writes], calls=calls))
    return cases


def kind7(_gba, rng, n=1600):
    return kind_cases(rng, "kind7", n)


def event(_gba, rng, n=1600):
    return kind_cases(rng, "event", n)


def career(_gba, rng, n=1600):
    return kind_cases(rng, "career", n)


def setup(_gba, rng, n=1600):
    return kind_cases(rng, "setup", n)


def kind38(_gba, rng, n=1600):
    return kind_cases(rng, "kind38", n)


def lists(_gba, rng, n=2400):
    return kind_cases(rng, "list", n)


def main(which):
    """`all` regenerates every set: needed after porting any kind, since handlers enter and draw arbitrary screens
    (menu_back, goto_screen) whose handlers were stubs when the older sets were made."""
    OUT.mkdir(parents=True, exist_ok=True)
    gba, rng = Gba("ui-2d/n7"), random.Random(0x6E66)
    if which == ["all"]:
        which = ["fades", "toplevel", "intro"] + [{"list": "lists"}.get(k, k) for k in PORTED_KINDS if k != "intro"]
    for name in which or ["fades"]:
        cases = globals()[name](gba, rng)
        path = OUT / f"{name}.jsonl"
        path.write_text("".join(json.dumps(c) + "\n" for c in cases), encoding="utf-8")
        print(f"{name}: {len(cases)} cases -> {path}")


