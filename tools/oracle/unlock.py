"""Oracle cases for three small unlock-id helpers of the garage (docs/engine/symbols.csv):
unlock_table_index 0x0812D7D4, unlock_id_adjust 0x0812D7F4, unlock_group 0x0812D730.

    .venv/Scripts/python.exe tools/oracle/cases.py unlock [COUNT]    # default 3000 cases per function

Runs the game's functions on the reference race snapshot (mgba/race) and writes menus/unlock.jsonl for
crates/nfsgba-formats/tests/unlock.rs. Each case: fn name, argument id, the car index at 0x03005718 (only
unlock_id_adjust reads it) and the return value. Seeded: reruns give the same file.
"""
import json
import random
import struct

from oracle import Gba, canonical, data_dir

FNS = {"table_index": 0x0812D7D4, "id_adjust": 0x0812D7F4, "group": 0x0812D730}
CAR = 0x0300_5718


def main(argv: list[str]) -> None:
    count = int(argv[0]) if argv else 3000
    gba, rng = Gba("mgba/race"), random.Random(7)
    edge = list(range(0, 0x200)) + [0x7FFF, 0xFFFF, 0x10000, 0xFFFFFFFF, 0x80000000]

    out = data_dir() / "work" / canonical()[1] / "menus"
    out.mkdir(parents=True, exist_ok=True)
    rows = []
    for i in range(count * len(FNS)):
        name = list(FNS)[i % 3]
        arg = edge[i // 3] if i // 3 < len(edge) else rng.choice([rng.randrange(0x200), rng.randrange(1 << 32)])
        if name == "table_index" and 0x8000 <= arg < 0x8000_0000:
            arg %= 0x8000  # the game scans on until an entry exceeds the id: larger ids run off the table
        car = rng.choice([0, 1, 2, 3, 5, 9, 17, 20, rng.randrange(0x30)])
        r = gba.call(FNS[name], r0=arg, mem=[(CAR, struct.pack("<I", car))], align="ignore")
        assert r.stop == "return", (name, hex(arg), r.stop)
        rows.append({"fn": name, "id": arg, "car": car, "ret": r.regs["r0"]})
    with open(out / "unlock.jsonl", "w") as f:
        f.writelines(json.dumps(r) + "\n" for r in rows)
    print(f"{len(rows)} cases -> {out / 'unlock.jsonl'}")
