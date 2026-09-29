"""Smoke tests for tools/record.py (no emulator): every recorder imports and has `main`, scenarios parse."""
import unittest

import record


class RecordTest(unittest.TestCase):
    def test_every_recorder_imports_with_a_main_and_a_session(self):
        for name in record.RECORDERS:
            m = record.recorder(name)
            self.assertTrue(callable(m.main), name)
            self.assertIsInstance(m.SESSION, str, name)

    def test_scenario(self):
        sc = record.load_scenario('{"session": "tools", "state": "mgba/race", "probes": ["frame", "autopilot"],'
                                  ' "commands": ["hold A 60", "dump x"]}')
        self.assertEqual([p.name for p in sc["probes"]], ["frame.lua", "autopilot.lua"])
        self.assertEqual((sc["state"], sc["commands"]), ("mgba/race", ["hold A 60", "dump x"]))
        with self.assertRaises(FileNotFoundError):
            record.load_scenario('{"session": "tools", "probes": ["nope"], "commands": []}')
        with self.assertRaises(ValueError):
            record.load_scenario('{"session": "tools", "keys": []}')


if __name__ == "__main__":
    unittest.main()
