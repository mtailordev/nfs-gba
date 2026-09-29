"""Test for tools/gate.py's marker check. Run: python -m unittest test_gate (from tools/)."""
import unittest

from gate import marker_problems, open_ids

FIDELITY = """# Fidelity ledger
## Rendering
| # | Now | Game | Exact source |
|---|---|---|---|
| R11 | open | x | y |
| T1 | open | x | y |
## Closed
- **R12, the entity draw:** exact.
"""


class MarkerTest(unittest.TestCase):
    def test_open_ids_are_the_table_rows(self):
        self.assertEqual(open_ids(FIDELITY), {"R11", "T1"})

    def test_markers(self):
        files = {
            "a.rs": "// NOT 1:1 (R11): the camera\n// NOT 1:1 (live play, T1): timing\n",
            "b.rs": "// NOT 1:1 (R12, grid): closed\n// NOT 1:1: no id\n// NOT 1:1 (unreachable)\n// NOT 1:1 (Z9)\n",
        }
        problems = marker_problems(files, FIDELITY)
        self.assertEqual([p.split(": ")[0] + ": " + p.split(": ")[1][:6] for p in problems],
                         ["b.rs:1: cites ", "b.rs:2: no ID", "b.rs:3: no ID", "b.rs:4: cites "])  # Z9: an ID that is not open
        self.assertIn("R12", problems[0])


if __name__ == "__main__":
    unittest.main()
