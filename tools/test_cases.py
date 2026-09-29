"""Smoke test for tools/oracle/cases.py (no data needed): every set module imports and has `main`."""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent / "oracle"))
import cases  # noqa: E402


class CasesTest(unittest.TestCase):
    def test_every_set_dispatches_to_a_main(self):
        for name in cases.SETS:
            self.assertTrue(callable(cases.module(name).main), name)
        with self.assertRaises(SystemExit):
            cases.module("nope")


if __name__ == "__main__":
    unittest.main()
