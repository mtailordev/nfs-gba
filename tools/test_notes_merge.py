"""Test for tools/notes_merge.py. Run: python -m unittest test_notes_merge (from tools/)."""
import unittest

from notes_merge import merge_symbols, read_rows

BASE = read_rows("address,name,kind,comment\n0x0816a708,__divsi3,function,div\n0x0815f948,sin_q14,function,sine\n")


class MergeTest(unittest.TestCase):
    def test_classifies_rows(self):
        notes = {
            "symbols.a.csv": read_rows(
                "address,name,kind,comment\n"
                "0x0816A708,__divsi3,function,same row, other case\n"  # duplicate
                "0x0815f948,sin14,function,renamed\n"  # same address, other name
                "0x08000100,sin_q14,function,moved\n"  # same name, other address
                "0x08000200,new_fn,function,new\n"
            ),
            "symbols.b.csv": read_rows(
                "address,name,kind,comment\n"
                "0x08000200,other_name,function,clashes with agent a\n"
                "0x08000300,new_fn,function,same name as agent a\n"
                "0x08000400,fine,label,new\n"
            ),
        }
        new, dups, conflicts = merge_symbols(BASE, notes)
        self.assertEqual([r["name"] for r in new], ["new_fn", "fine"])
        self.assertEqual([(s, r["name"]) for s, r, _ in dups], [("symbols.a.csv", "__divsi3")])
        self.assertEqual(
            [(s, r["name"], where) for s, r, where, _ in conflicts],
            [("symbols.a.csv", "sin14", "symbols.csv"), ("symbols.a.csv", "sin_q14", "symbols.csv"),
             ("symbols.b.csv", "other_name", "symbols.a.csv"), ("symbols.b.csv", "new_fn", "symbols.a.csv")],
        )
        self.assertEqual(new[0]["address"], "0x08000200")  # addresses normalised


if __name__ == "__main__":
    unittest.main()
