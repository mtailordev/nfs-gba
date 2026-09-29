"""Smoke test for tools/retro.py. Run: .venv/Scripts/python.exe -m unittest tools/test_retro.py
Needs ext/libretro/mgba_libretro.dll and the reference race dump; skipped otherwise (fails if NFSGBA_REQUIRE_DATA=1)."""
import os
import unittest

from common import data_dir
from retro import DLL, ROM

REF = data_dir() / "work" / "e5298b24" / "mgba"
HAVE = DLL.exists() and ROM.exists() and (REF / "race.ss").exists()


@unittest.skipUnless(HAVE or os.environ.get("NFSGBA_REQUIRE_DATA") == "1", "no libretro core / reference dump")
class RetroTest(unittest.TestCase):
    def test_race_ss_one_frame(self):
        from retro import Retro
        r = Retro()
        r.load_ss(REF / "race.ss")
        r.run(1)
        for name, addr in (("iwram", 0x03000000), ("wram", 0x02000000), ("palette", 0x05000000), ("oam", 0x07000000)):
            ref = (REF / f"race.{name}.bin").read_bytes()
            self.assertEqual(r.read(addr, len(ref)), ref, name)


if __name__ == "__main__":
    unittest.main()
