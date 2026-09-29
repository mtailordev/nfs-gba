"""A save written by our code (crates/nfsgba-game/src/menu/boot.rs writes boot/ours.sav) boots in headless mGBA and the
main menu shows its profile. Run: .venv/Scripts/python.exe -m unittest tools/test_boot_trace.py
Needs ext/libretro/mgba_libretro.dll and boot/ours.sav; skipped otherwise (fails if NFSGBA_REQUIRE_DATA=1)."""
import json
import os
import unittest

import boot_trace
from common import data_dir
from retro import DLL, ROM

BOOT = data_dir() / "work" / "e5298b24" / "boot"
HAVE = DLL.exists() and ROM.exists() and (BOOT / "ours.sav").exists()


@unittest.skipUnless(HAVE or os.environ.get("NFSGBA_REQUIRE_DATA") == "1", "no libretro core / ours.sav")
class BootTraceTest(unittest.TestCase):
    def test_our_save_loads_in_the_game(self):
        boot_trace.main(["ours", str(BOOT / "ours.sav")])
        last = json.loads((BOOT / "ours.json").read_text())["frames"][-1]
        self.assertEqual(bytes.fromhex(last["name"])[:3], b"ZED")
        self.assertEqual((last["exists"], last["loaded"], last["units"]), (1, 1, 1))


if __name__ == "__main__":
    unittest.main()
