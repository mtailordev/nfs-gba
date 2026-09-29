"""Smoke tests for the function oracle (tools/oracle). Run: .venv/Scripts/python.exe -m unittest tools/test_oracle.py
Needs the vault and the reference race dump; skipped otherwise."""
import json
import struct
import subprocess
import sys
import unittest

from common import ROOT, data_dir

sys.path.insert(0, str(ROOT / "tools" / "oracle"))
HAVE_DATA = (data_dir() / "work" / "e5298b24" / "mgba" / "race.wram.bin").exists()
CODE = 0x0203_FF00  # a stub in EWRAM, poked in for one call


def thumb(*halfwords):
    return [(CODE, struct.pack(f"<{len(halfwords)}H", *halfwords))]


@unittest.skipUnless(HAVE_DATA, "no reference race dump")
class OracleTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        from oracle import Gba
        cls.gba = Gba()

    def test_divsi3(self):
        for a, b, q in [(7, 2, 3), (-7, 2, -3), (7, 0, 0), (-0x8000_0000, -1, -0x8000_0000)]:
            r = self.gba.call(0x0816A708, r0=a, r1=b)
            self.assertEqual((r.stop, r.regs["r0"]), ("return", q & 0xFFFF_FFFF))

    def test_bios_div_and_writes(self):
        r = self.gba.call(CODE, mode="thumb", mem=thumb(0xDF06, 0x4770), r0=-7, r1=2)  # swi 6; bx lr
        self.assertEqual([r.regs[k] for k in ("r0", "r1", "r3")], [(-3) & 0xFFFF_FFFF, (-1) & 0xFFFF_FFFF, 3])
        r = self.gba.call(CODE, mode="thumb", mem=thumb(0x6001, 0x4770), r0=0x0203_FE00, r1=0x1122_3344)  # str
        self.assertIn((0x0203_FE00, bytes.fromhex("44332211")), r.writes)
        self.assertEqual(r.read(0x0203_FDFF, 6)[1:5], bytes.fromhex("44332211"))
        self.assertEqual(self.gba.read_base(0x0203_FE00, 4), self.gba.call(CODE, mode="thumb",
                         mem=thumb(0x4770)).read(0x0203_FE00, 4))  # the snapshot is restored after each call

    def test_bios_lz77_and_cpuset(self):
        stream = bytes.fromhex("100800002041423001")  # "AB", then 6 bytes from 2 back
        mem = thumb(0xDF11, 0x4770) + [(0x0203_FD00, stream)]
        r = self.gba.call(CODE, mode="thumb", mem=mem, r0=0x0203_FD00, r1=0x0203_FE00)
        self.assertEqual(r.read(0x0203_FE00, 8), b"ABABABAB")
        mem = thumb(0xDF0B, 0x4770) + [(0x0203_FD00, b"\xAA\xBB\xCC\xDD")]
        r = self.gba.call(CODE, mode="thumb", mem=mem, r0=0x0203_FD00, r1=0x0203_FE00, r2=3 | 1 << 24 | 1 << 26)
        self.assertEqual(r.read(0x0203_FE00, 16), b"\xAA\xBB\xCC\xDD" * 3 + self.gba.read_base(0x0203_FE0C, 4))

    def test_stack_frames_are_left_out_and_stubs_return(self):
        # push {r4, lr}; str r1, [r0]; bl <stub>; pop {r4, pc}: the push is stack, the IWRAM global is a write
        seen = []
        code = thumb(0xB510, 0x6001, 0xF000, 0xF802, 0xBD10, 0x0000, 0x4770)  # bl +4 -> the bx lr at CODE+0xC
        r = self.gba.call(CODE, mode="thumb", mem=code, r0=0x0300_6104, r1=0x0BAD_F00D,
                          stubs={CODE + 0xC: lambda uc: seen.append("stub")})
        self.assertEqual(r.stop, "return")
        self.assertEqual(seen, ["stub"])
        self.assertEqual(r.writes, [(0x0300_6104, bytes.fromhex("0df0ad0b"))])

    def test_unaligned_access_stops(self):
        r = self.gba.call(CODE, mode="thumb", mem=thumb(0x6808, 0x4770), r1=0x0203_FE01)  # ldr r0, [r1]
        self.assertTrue(r.stop.startswith("unaligned read of 4"), r.stop)

    def test_cli(self):
        q = {"fn": "0x0816a708", "regs": {"r0": 100, "r1": -7}}
        out = subprocess.run([sys.executable, str(ROOT / "tools" / "oracle" / "oracle.py")], input=json.dumps(q) + "\n",
                             capture_output=True, text=True, check=True).stdout
        r = json.loads(out)
        self.assertEqual((r["stop"], r["regs"]["r0"]), ("return", (-14) & 0xFFFF_FFFF))


if __name__ == "__main__":
    unittest.main()
