"""Hardware check of `fill_rect8` (0x08164BEC) on a real mGBA frame, for what the function oracle cannot model:
byte stores into BG VRAM (the ARM7TDMI duplicates the byte across the halfword) and unaligned word stores (forced
aligned). A patched COPY of the canonical ROM (never the vault file; written under data/work) boots into a small
ARM stub that sets mode 4 and calls the game's own `fill_rect8` for a table of rectangles, one per 4-row band of
page 0; headless mGBA (tools/retro.py) runs it and the VRAM page is saved with the table as
menus3/fill-rect-hw.json. `menu::tests::fill_rect8_matches_mgba` replays the table with `ui::fill_rect8` (BG VRAM
rules) and compares the page.

    .venv/Scripts/python.exe tools/oracle/cases.py fill-rect-hw

Needs ext/libretro/mgba_libretro.dll (docs/TOOLS.md) and the arm-none-eabi binutils from scoop.
"""
import json
import struct
import subprocess
import sys
from pathlib import Path

from common import data_dir

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from retro import ROM, Retro  # noqa: E402

BIN = Path.home() / "scoop" / "apps" / "gcc-arm-none-eabi" / "current" / "bin"
FN = 0x08164BED  # fill_rect8 (Thumb)


def rects():
    """One rectangle per band: every start column mod 8, widths 1..18, distinct colours."""
    out = []
    for i in range(40):
        x0, w, y0 = 16 + i % 8, 1 + i * 7 % 18, 4 * i
        out.append([x0, y0, x0 + w, y0 + 2, 0x0101_0101 * (i + 1)])
    return out


def free_space(rom, n):
    """4-aligned start inside a run of n zero bytes of the copy (unused: the game never runs)."""
    return (rom.index(b"\x00" * n) + 0x13) & ~3


def build(rom, table, work):
    at = 0x0800_0000 + free_space(rom, 0x1000)
    words = "\n".join(f"    .word {', '.join(str(v) for v in row)}" for row in table)
    (work / "fillhw.s").write_text(f""".arm
.global _start
_start:
    mov r0, #0x04000000
    mov r1, #4
    orr r1, r1, #0x400
    strh r1, [r0]
    ldr r0, =tbl
    mov r1, #0x03000000
    str r0, [r1]
next:
    mov r1, #0x03000000
    ldr r0, [r1]
    ldr r2, =tblend
    cmp r0, r2
    bhs done
    add r3, r0, #20
    str r3, [r1]
    ldr r3, [r0, #16]
    mov r1, #0x06000000
    mov r2, #240
    ldr r7, ={FN:#x}
    ldr lr, =next
    bx r7
done:
    b done
.ltorg
tbl:
{words}
tblend:
""", encoding="utf-8")
    run = lambda tool, *a: subprocess.run([str(BIN / f"arm-none-eabi-{tool}.exe"), *a], check=True, cwd=work)
    run("as", "-mcpu=arm7tdmi", "-o", "fillhw.o", "fillhw.s")
    run("ld", f"-Ttext={at:#x}", "-o", "fillhw.elf", "fillhw.o")
    run("objcopy", "-O", "binary", "fillhw.elf", "fillhw.bin")
    code = (work / "fillhw.bin").read_bytes()
    patched = bytearray(rom)
    patched[at - 0x0800_0000:at - 0x0800_0000 + len(code)] = code
    patched[0:4] = struct.pack("<I", 0xEA00_0000 | ((at - 0x0800_0008) >> 2) & 0xFF_FFFF)  # b stub
    path = work / "fillhw.gba"
    path.write_bytes(patched)
    return path


def main(_args):
    rom = ROM.read_bytes()
    work = data_dir() / "work" / "e5298b24" / "retro"
    work.mkdir(parents=True, exist_ok=True)
    table = rects()
    r = Retro(rom=build(rom, table, work), video=False)
    r.run(4)
    page = r.read(0x0600_0000, 240 * 160)
    out = data_dir() / "work" / "e5298b24" / "menus3" / "fill-rect-hw.json"
    out.write_text(json.dumps(dict(rects=table, page=page.hex(), core=r.version)), encoding="utf-8")
    assert any(page), "the stub did not draw"
    print(f"{len(table)} rectangles, {sum(1 for b in page if b)} non-zero pixels -> {out}")
