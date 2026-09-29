"""Smoke tests for tools/. Run: python -m unittest discover tools"""
import hashlib
import json
import os
import subprocess
import tempfile
import unittest
import zipfile
from pathlib import Path

import city
import first_look
import models
import vault
from common import ROOT, data_dir


def fake_rom(code: str = "BN7E", size: int = 32768, patch: int | None = None) -> bytes:
    rom = bytearray(os.urandom(size))  # random bytes: no accidental constant blocks
    rom[0xA0:0xAC] = b"TEST TITLE\0\0"
    rom[0xAC:0xB0] = code.encode()
    rom[0xB0:0xB2] = b"69"
    rom[0xB2] = 0x96
    rom[0xBC] = 0
    if patch is not None:
        rom[patch] ^= 0xFF
    rom[0xBD] = -(sum(rom[0xA0:0xBD]) + 0x19) & 0xFF
    return bytes(rom)


class HeaderTest(unittest.TestCase):
    def test_parse(self):
        h = vault.parse_header(fake_rom())
        self.assertEqual((h["game_code"], h["region"], h["maker_code"], h["title"]), ("BN7E", "USA/English", "69", "TEST TITLE"))
        self.assertTrue(h["header_checksum_ok"] and h["fixed_96h_ok"])
        bad = bytearray(fake_rom())
        bad[0xBD] ^= 1
        self.assertFalse(vault.parse_header(bytes(bad))["header_checksum_ok"])


class VaultTest(unittest.TestCase):
    def test_rerun_changes_nothing(self):
        with tempfile.TemporaryDirectory() as t:
            t = Path(t)
            dumps, data, docs = t / "dumps", t / "data", t / "docs"
            dumps.mkdir()
            a, b = fake_rom("AZFP"), None
            b = bytearray(a)
            b[0x1000] ^= 0xFF  # regional variant: one byte plus the header differ
            b[0xAF] = ord("E")
            b[0xBD] = -(sum(b[0xA0:0xBD]) + 0x19) & 0xFF
            for zname, member, rom in (("Carbon (A).zip", "c.gba", fake_rom()), ("Carbon (B).zip", "c.gba", fake_rom()),
                                       ("Porsche (Europe).zip", "p.gba", a), ("Porsche (USA).zip", "p.gba", bytes(b))):
                with zipfile.ZipFile(dumps / zname, "w") as z:
                    z.writestr(member, rom)
            m1 = vault.run(dumps, data, docs)
            snap = {p: p.read_bytes() for p in t.rglob("*") if p.is_file()}
            m2 = vault.run(dumps, data, docs)
            self.assertEqual(snap, {p: p.read_bytes() for p in t.rglob("*") if p.is_file()})
            self.assertEqual(m1, m2)
            # two random "Carbon" ROMs differ, so no canonical target may be picked
            self.assertIsNone(m1["canonical_target"])
            for r in m1["roms"]:
                self.assertTrue(r["valid"] and r["read_only"])
            cmp = m1["comparisons"]
            self.assertTrue(any(c.get("differing_bytes") == 3 for c in cmp))  # 0x1000, game code, checksum


class DecoderTest(unittest.TestCase):
    def test_lz77(self):
        blob = bytes.fromhex("10090000 10 414243 3002")  # "ABC" then copy 6 from 3 back
        self.assertEqual(first_look.lz77(blob, 0, 9), len(blob))
        with self.assertRaises(ValueError):
            first_look.lz77(bytes.fromhex("10090000 80 3002"), 0, 9)  # back-reference before start

    def test_lz77_blobs_allow_8_byte_overread(self):
        lits = bytes(range(0x41, 0x49))  # 8 literals
        a = bytes.fromhex("10130000") + (b"\0" + lits) * 2 + b"\0x"  # claims 19 bytes: decode reads 2 bytes of b
        b = bytes.fromhex("10100000") + (b"\0" + lits) * 2
        self.assertEqual(first_look.lz77_blobs(a + b + bytes(8)), [(0, 19, 26), (24, 16, 22)])

    def test_rle(self):
        blob = bytes.fromhex("30070000 82 41 01 4243")  # 5 x "A", then "BC"
        self.assertEqual(first_look.rle(blob, 0, 7), len(blob))

    def test_huffman(self):
        # 8-bit, tree [size, root(both children data), 'A', 'B'], bits 0110 -> "ABBA"
        blob = bytes.fromhex("28040000 01 c0 41 42") + (0x60000000).to_bytes(4, "little")
        self.assertEqual(first_look.huffman(blob, 0, 4), len(blob))


class FirstLookTest(unittest.TestCase):
    def test_block_classes(self):
        arm = b"".join((0xE1A00000 + i).to_bytes(4, "little") for i in range(first_look.BLOCK // 4))
        cls = "".join(c for _, c in first_look.blocks(bytes(first_look.BLOCK) + arm))
        self.assertEqual(cls, ".A")

    def test_normalize_masks_relocation(self):
        def thumb(bl_off, ptr):
            body = bytes.fromhex("f0b5") + (0xF000 | bl_off).to_bytes(2, "little") + (0xF800 | bl_off).to_bytes(2, "little")
            body += bytes.fromhex("c046f0bd0047") + ptr.to_bytes(4, "little")  # nop, pop, bx r0, literal (16 B total)
            return body * (first_look.BLOCK // len(body)) + bytes(first_look.BLOCK % len(body))
        a = first_look.normalize(thumb(0x12, 0x08001234), "T")
        b = first_look.normalize(thumb(0x345, 0x08765430), "T")
        self.assertEqual(a, b)


class HookTest(unittest.TestCase):
    def test_pre_commit_rejects_rom(self):
        with tempfile.TemporaryDirectory() as t:
            git = ["git", "-C", t, "-c", "user.name=t", "-c", "user.email=t@t", "-c",
                   f"core.hooksPath={(ROOT / 'tools' / 'git-hooks').as_posix()}"]
            subprocess.run(["git", "init", "-q", t], check=True)
            Path(t, "x.gba").write_bytes(b"rom")
            Path(t, "ok.txt").write_text("fine")
            subprocess.run(git + ["add", "x.gba"], check=True)
            self.assertNotEqual(subprocess.run(git + ["commit", "-qm", "rom"], capture_output=True).returncode, 0)
            subprocess.run(git + ["rm", "-q", "--cached", "x.gba"], check=True)
            subprocess.run(git + ["add", "ok.txt"], check=True)
            self.assertEqual(subprocess.run(git + ["commit", "-qm", "ok"], capture_output=True).returncode, 0)


@unittest.skipUnless((data_dir() / "vault" / "manifest.json").exists(), "no vault yet: run tools/vault.py")
class RealVaultTest(unittest.TestCase):
    def test_vault_matches_manifest(self):
        m = json.loads((data_dir() / "vault" / "manifest.json").read_text(encoding="utf-8"))
        for r in m["roms"]:
            rom = (data_dir() / r["vault_file"]).read_bytes()
            self.assertEqual(hashlib.sha1(rom).hexdigest(), r["sha1"])
            self.assertTrue(vault.parse_header(rom)["header_checksum_ok"])
        canon = next(r for r in m["roms"] if r["sha1"] == m["canonical_target"])
        self.assertEqual(canon["header"]["game_code"], "BN7E")

    def test_city_sectors_are_contiguous_and_portals_match(self):
        m = json.loads((data_dir() / "vault" / "manifest.json").read_text(encoding="utf-8"))
        canon = next(r for r in m["roms"] if r["sha1"] == m["canonical_target"])
        sectors = city.parse((data_dir() / canon["vault_file"]).read_bytes())
        self.assertEqual(len(sectors), 1113)
        for a, b in zip(sectors, sectors[1:]):
            self.assertEqual(a["first"] + len(a["walls"]), b["first"])  # wall ranges tile the wall array
        portals = matched = 0
        for s in sectors:
            for w, p, q in city.segments(s):
                if w["link"] >= 0:
                    portals += 1
                    matched += (q, p) in [(pp, qq) for _, pp, qq in city.segments(sectors[w["link"]])]
        self.assertGreater(matched / portals, 0.99)  # a portal's edge exists reversed in the sector it links to

    def test_vehicle_models_fill_their_arrays_exactly(self):
        m = json.loads((data_dir() / "vault" / "manifest.json").read_text(encoding="utf-8"))
        canon = next(r for r in m["roms"] if r["sha1"] == m["canonical_target"])
        rom = (data_dir() / canon["vault_file"]).read_bytes()
        a, ms = models.bank_arrays(rom), models.parse(rom)
        self.assertEqual(len(ms), 102)
        for mo in ms:
            for p in mo["polys"]:
                self.assertIn(len(p["v"]), (3, 4))
                self.assertTrue(all(k < len(mo["verts"]) for k in p["v"]), mo["index"])
                if mo["flags"] & 1:
                    self.assertTrue(all(k < len(mo["uvs"]) for k in p["uv"]), mo["index"])
        last = ms[-1]
        vstart, istart, uvstart, pstart, _, _ = last["first"]
        self.assertEqual(vstart + len(last["verts"]), (a["indices"] - a["verts"]) // 6)  # vertex array ends here (2 bytes padding)
        self.assertEqual(uvstart + len(last["uvs"]), (a["sizes"] - a["uvs"]) // 4)


if __name__ == "__main__":
    unittest.main()
