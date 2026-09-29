"""Smoke tests for the car-trace tools (no emulator needed): python tools/trace_tests.py"""
import re
import sys
import tempfile
import unittest
from contextlib import redirect_stdout
from io import StringIO
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import trace_race  # noqa: E402

LUA = (Path(__file__).resolve().parent / "mgba_remote.lua").read_text(encoding="utf-8")


class TraceTools(unittest.TestCase):
    def test_scenarios_only_use_remote_commands_and_keys(self):
        keys = set(re.search(r"local KEYS = \{(.*?)\}", LUA).group(1).replace(" ", "").split(","))
        keys = {k.split("=")[0] for k in keys}
        for name, commands in trace_race.SCENARIOS.items():
            for c in commands:
                op, *args = c.split()
                self.assertIn(op, ("hold", "wait"), name)
                if op == "hold":
                    self.assertTrue(set(args[0].split(",")) <= keys, c)
                self.assertTrue(args[-1].isdigit(), c)

    def test_summary_decodes_entity_fields(self):
        entity = bytearray(0xA4)
        entity[0x0C:0x10] = (0x1234).to_bytes(4, "little")
        entity[0x14:0x18] = (-5 & 0xFFFFFFFF).to_bytes(4, "little")
        entity[0x78:0x7A] = (760).to_bytes(2, "little")
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / "t.csv"
            path.write_text("frame,keys,dt,phase,input,flag610c,sector,entity,physics\n"
                            f"10,1,23,2,64513,1,760,{entity.hex()},\n")
            out = StringIO()
            with redirect_stdout(out):
                trace_race.summary(path)
        self.assertIn("x=4660", out.getvalue())
        self.assertIn("z=-5", out.getvalue())
        self.assertIn("sector=760", out.getvalue())
        self.assertIn("input=0xfc01", out.getvalue())


if __name__ == "__main__":
    unittest.main()
