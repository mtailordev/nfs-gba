"""Test for tools/rom_attribution.py helpers. Run: python -m unittest test_rom_attribution (from tools/)."""
import unittest

from rom_attribution import Rom, lz77_consumed, size_of


class AttributionTest(unittest.TestCase):
    def test_lz77_consumed_counts_source_bytes(self):
        rom = Rom(bytes.fromhex("100800002041423001") + b"\xEE" * 8)  # "AB", then 6 bytes from 2 back
        self.assertEqual(lz77_consumed(rom, 0, 8), 9)
        self.assertEqual(lz77_consumed(rom, 0, 2), 7)  # two literals: header, flags, 2 bytes

    def test_size_of_map_cells(self):
        self.assertEqual(size_of("227 × 0x24"), 227 * 0x24)
        self.assertEqual(size_of("44 × 0xC"), 44 * 12)
        self.assertEqual(size_of("4 + 40 × 24"), 4 + 40 * 24)
        self.assertEqual(size_of("0x2000 × i16"), 0x4000)
        self.assertEqual(size_of("1,113 × 0x30"), 1113 * 0x30)
        self.assertEqual(size_of("32 B"), 32)
        for unknown in ("0x20 each", "7 per car", "6 × 2 u16", "bytes"):
            self.assertIsNone(size_of(unknown), unknown)

    def test_claims_keep_the_first_owner(self):
        rom = Rom(bytes(16))
        self.assertEqual(rom.claim(0, 8, "a", "first"), 8)
        self.assertEqual(rom.claim(4, 12, "b", "second"), 4)
        self.assertEqual([rom.labels[o][1] for o in rom.owner[:12]], ["first"] * 8 + ["second"] * 4)


if __name__ == "__main__":
    unittest.main()
