"""Oracle cases for the menu drawing primitives (crates/nfsgba-game/src/menu/draw.rs): the game's own `fill_rect8`,
`menu_blit_material(_alt)`, `text_menu`, `text_menu_7`, `text_menu_wrapped_colour`, `text_box`, `menu_button_prompts`
and `message_box_draw` run in unicorn on a page in EWRAM (menus3/draw-*.jsonl). The Rust replay
(`menu::tests::drawing_matches_the_game`) compares every changed byte, the stub calls (`unpack_to_buffer`) and text
widths.

    .venv/Scripts/python.exe tools/oracle/cases.py draw [fill blit text ...]

`fill_rect8` is only generated for the starts whose word stores are aligned (x0 % 4 in {0, 3}) and `blit_alt` for
even columns: unicorn does not rotate unaligned stores as the ARM7TDMI does (`oracle.py`). The other starts, and the
byte stores' duplication in BG VRAM, are checked on a real mGBA frame by `tools/oracle/fill_rect_hw.py`.
"""
import json
import random
import struct

from menus import OUT, PROFILE_AT, Skip, run, word

from oracle import Gba

WORLD_PAGE_CELL = 0x0300_0110  # world + 0x50: a pointer to the cell that holds the drawing page's address
CELL, PAGE, PAGE2 = 0x0203_A000, 0x0202_0000, 0x0202_A000
STRING, RECT = 0x0203_8000, 0x0203_8200
MENU_MATERIALS = 0x0834_5114
SNAPS = ["ui-2d/lang", "ui-2d/n7", "race-rules/a4"]
CHARS = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789 .,:!?'-/{|\xc9\xe4\xfc\xdf~"
FN = dict(fill=0x08164BEC, blit=0x08136D74, alt=0x08136E60, text=0x08141578, text7=0x081419C0, wrapped=0x08141B40,
          box=0x08141C88, prompts=0x0812BD60, message=0x0813550C)


def base(rng):
    """The page, its cell, the screen size and the language every primitive reads."""
    return [
        (WORLD_PAGE_CELL, word(CELL)), (CELL, word(PAGE)),
        (0x0300_641C, word(PAGE)), (0x0300_6420, word(PAGE2)),
        (0x0300_6410, struct.pack("<hh", 240, 160)),
        (0x0300_5600, word(rng.choice([0, 1, 2, 3, 4, 0, 1, 5, 6]))),
        (0x0300_57F0, word(0x0200_A000)),  # the unpack buffer (base + 0x9608, clear of the pages): its stub logs the call and decompresses
        (0x0300_59F0, word(rng.choice([-1, -1, 0, 1, 2]))),
        (PROFILE_AT + 0x340, bytes(rng.choice([0, 1, 0x7F, 0x80, 0xFF]) for _ in range(3))),
        (0x0300_56EC, word(PROFILE_AT)),
        (0x0300_593C, bytes([rng.choice([0xFF, 0, 1, 2])])),
    ]


def text_arg(rng, mem):
    """A text key, or a pointer to a random string that the case puts in memory."""
    if rng.random() < 0.6:
        return rng.choice([rng.randrange(977), rng.randrange(977), 0x131, 0x1F5, 0x159, 0x174, 0x3C2])
    s = bytes(rng.choice(CHARS) for _ in range(rng.randrange(0, 60)))
    if rng.random() < 0.3:
        s = s.replace(b" ", b"\n", 2)
    mem.append((STRING, s + b"\0"))
    return STRING


SKIPPED = []


def case(gba, snap, name, mem, vals=()):
    """`vals`: the call's arguments, the first four in r0..r3, the rest on the stack."""
    fn, vals = FN[name], [v & 0xFFFF_FFFF for v in vals]
    regs = {f"r{k}": v for k, v in enumerate(vals[:4])}
    try:
        r0, writes, calls = run(gba, fn, mem, {}, regs=regs, stack=vals[4:])
    except Skip as e:  # the game hangs or the oracle cannot follow (an unaligned store): not a case
        SKIPPED.append((name, vals, [(hex(a), b.hex()) for a, b in mem if a == STRING], str(e)))
        return None
    return dict(snap=snap, fn=hex(fn), args=vals, mem=[[a, b.hex()] for a, b in mem], ret={}, r0=r0,
                writes=[[a, b.hex()] for a, b in writes], calls=calls)


def make(kind, gbas, rng, n):
    cases, mats = [], {}
    for i in range(n):
        snap = rng.choice(SNAPS)
        gba = gbas[snap]
        mem = base(rng)
        if kind == "fill":
            x0 = rng.choice([0, 3, 4, 7, 8, 11, 12, 15, 16, 100, 103, 104, 200, 235, 236])
            x1 = x0 + rng.choice([-2, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 15, 16, 17, 33, rng.randrange(0, 240 - x0)])
            y0 = rng.randrange(0, 158)
            y1 = y0 + rng.choice([-1, 0, 1, 2, 3, 20, rng.randrange(0, 160 - y0)])
            mem.append((RECT, struct.pack("<4i", x0, y0, x1, y1)))
            pitch = rng.choice([240, 240, 240, 256])
            colour = rng.choice([rng.randrange(1 << 32), 0x01010101 * rng.randrange(256)])
            cases.append(case(gba, snap, "fill", mem, [RECT, PAGE, pitch, colour]))
        elif kind in ("blit", "alt"):
            m = rng.randrange(273)
            if m not in mats:
                mats[m] = struct.unpack("<HH", gba.read_base(MENU_MATERIALS + 0x24 * m + 0xC, 4))
            w, h = mats[m]
            if w > 240 or h > 160 or not w or not h:
                continue
            x, y = rng.randrange(0, 240 - w + 1), rng.randrange(0, 160 - h + 1)
            if kind == "alt":
                x &= ~1
                if w & 1:
                    continue
            cases.append(case(gba, snap, kind, mem, [0x0300_00C0, m, x, y]))
        elif kind == "text":
            t = text_arg(rng, mem)
            y = rng.choice([rng.randrange(8, 0xA1), rng.randrange(8, 0xA1), 0xA0, 0xA1, 0xFF, 8])  # 8+: raised glyphs stay on the page
            cases.append(case(gba, snap, "text", mem,
                              [rng.choice([0xC, 0xD, 0xE, 0xF, 0xB, 0x10]), t, rng.randrange(-10, 260), y,
                               rng.choice([0, 1, -1, 0, 2]), rng.choice([0, 0, 1, 2, -3, 0x10])]))
        elif kind in ("text7", "wrapped", "box"):
            t = text_arg(rng, mem)
            font = rng.choice([0xC, 0xD, 0xE, 0xF, 0xB])
            x, y = rng.randrange(0, 240), rng.choice([rng.randrange(8, 150), rng.randrange(8, 150), -3])
            if kind != "wrapped":
                y = max(y, 8)
            width, lines = rng.choice([40, 60, 100, 0xDC, 0xE0, 0xEE, 0xF0, rng.randrange(20, 240)]), rng.choice([1, 2, 3, 4])
            colour = rng.choice([0, 0, 8, 0xF, 1])
            cases.append(case(gba, snap, kind, mem, [font, t, x, y, width, lines, colour]))
        elif kind == "prompts":
            keys = [0xFFFF_FFFF, 0x8D, 0x92, 0x1F5, 0xB5, 0x100, rng.randrange(977)]
            cases.append(case(gba, snap, "prompts", mem, [rng.choice(keys), rng.choice(keys), rng.choice(keys)]))
        elif kind == "message":
            mem.append((0x0300_59F0, word(rng.choice([1, 2, 1, 2, 0, -1]))))
            mem.append((0x0300_59F8, word(text_arg(rng, mem))))
            mem.append((0x0300_59EC, word(rng.choice([-1, -1, 0, 5, 123, 9999, 1234567, -7, 99999999]))))
            cases.append(case(gba, snap, "message", mem))
    return [c for c in cases if c]


KINDS = {"fill": 400, "blit": 300, "alt": 200, "text": 500, "text7": 200, "wrapped": 300, "box": 400, "prompts": 300,
         "message": 300}


def main(which):
    OUT.mkdir(parents=True, exist_ok=True)
    gbas = {s: Gba(s) for s in SNAPS}
    for kind in which or list(KINDS):
        rng = random.Random(f"draw-{kind}")
        cases = make(kind, gbas, rng, KINDS[kind])
        path = OUT / f"draw-{kind}.jsonl"
        path.write_text("".join(json.dumps(c) + "\n" for c in cases), encoding="utf-8")
        print(f"{kind}: {len(cases)} cases -> {path}")
    for s in SKIPPED[:6]:
        print("skipped", s)
    print(len(SKIPPED), "skipped")
