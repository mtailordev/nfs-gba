"""Test for tools/notes_merge.py. Run: python -m unittest test_notes_merge (from tools/)."""
import unittest

from notes_merge import insert_addresses, merge_symbols, read_addresses, read_rows

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


MAP = """# Map
## ROM
| Offset | Size | What | Doc |
|---|---|---|---|
| `0x000100` | 4 | a | x |
| `0x000300` | 4 | c | x |
## RAM (race)
| Address | What |
|---|---|
| `0x03000010` | ram a |
| palette RAM | not an address |
| `0x03000030` | ram c |
### Driver (`*x`)
| Offset | What |
|---|---|
| `+0x10` | d a |
"""


class ReadAddressesTest(unittest.TestCase):
    def test_unquoted_commas_stay_in_what(self):
        rows = read_addresses("region,address,size,what,doc\n"
                              "io,0x04000050,4,BLDCNT 0x3F3F, BLDALPHA 0x0D0F (EVA 15/16, EVB 13/16),engine/x\n"
                              'ram,0x03000010,4,"quoted, with comma",engine/y\n')
        self.assertEqual(rows[0]["what"], "BLDCNT 0x3F3F, BLDALPHA 0x0D0F (EVA 15/16, EVB 13/16)")
        self.assertEqual((rows[0]["doc"], rows[1]["what"], rows[1]["doc"]), ("engine/x", "quoted, with comma", "engine/y"))


class InsertTest(unittest.TestCase):
    def test_sorted_into_the_region_table(self):
        rows = [
            {"region": "rom", "address": "0x000200", "size": "8", "what": "b", "doc": "y"},
            {"region": "ram", "address": "0x03000020", "size": "4", "what": "ram b", "doc": "y"},
            {"region": "ram", "address": "0x03000040", "size": "", "what": "ram d", "doc": "y"},
            {"region": "driver", "address": "+0x08", "size": "2", "what": "d first", "doc": "y"},
        ]
        out = insert_addresses(MAP, rows).split("\n")
        self.assertEqual(out[out.index("| `0x000100` | 4 | a | x |") + 1], "| `0x000200` | 8 | b | y |")
        self.assertEqual(out[out.index("| `0x03000010` | ram a |") + 1], "| `0x03000020` | ram b (4) |")
        self.assertEqual(out[out.index("| `0x03000030` | ram c |") + 1], "| `0x03000040` | ram d |")
        self.assertEqual(out[out.index("| `+0x10` | d a |") - 1], "| `+0x08` | d first (2) |")


if __name__ == "__main__":
    unittest.main()
