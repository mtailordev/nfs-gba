"""Peek at the ROM: rom_peek.py list PAGE  -> the items of list page PAGE (0x7E544C records; items: text, 2 pictures, action)."""
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import data_dir

rom = (data_dir() / "vault" / "roms" / "BN7E_v0_e5298b24.gba").read_bytes()
u16 = lambda o: struct.unpack_from("<H", rom, o)[0]
u32 = lambda o: struct.unpack_from("<I", rom, o)[0]
if sys.argv[1] == "events":
    for e in range(66):
        b = rom[0x7E4744 + 8 * e:][:8]
        print(e, "skill", b[0], "track", b[1], "mode", b[2], "rev", b[3], "laps", b[4], "traffic", b[5], "reward", struct.unpack_from("<h", b, 6)[0])
if sys.argv[1] == "list":
    rec = 0x7E544C + 0x14 * int(sys.argv[2], 0)
    print("record", [hex(u16(rec + 2 * i)) for i in range(8)], "items", hex(u32(rec + 0x10)))
    n = u16(rec + 0xA)
    p = u32(rec + 0x10) & 0x1FFFFFF
    for i in range(n):
        print(i, [hex(u16(p + 8 * i + 2 * k)) for k in range(4)])
