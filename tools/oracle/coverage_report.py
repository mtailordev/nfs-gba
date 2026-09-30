"""Which functions the original reaches, and which of those the Rust has no counterpart for.

    .venv/Scripts/python.exe tools/oracle/coverage_report.py [FOLDER]     # default coverage3 (record.py coverage output)

Reads FOLDER/coverage.csv (address, name, kind, one hits column per scenario), looks every function up in crates/
(its address as any 0x literal or bare 08xxxxxx / 03xxxxxx hex (`FUN_0813d1f0`), `_` separators allowed, or its symbol name from docs/engine/symbols.csv as a word),
prints a table (all scenarios, the four original ones, the career ones) and writes FOLDER/unported.csv: the reached
functions with no counterpart.
"""
import csv
import pathlib
import re
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent.parent))
from common import ROOT, data_dir  # noqa: E402

OLD = ["boot", "menu_to_race", "drive", "pause_quit"]


def rust_words():
    text = "\n".join(p.read_text(encoding="utf-8", errors="replace") for p in (ROOT / "crates").rglob("*.rs"))
    # any 0x literal, or a bare 6-8 digit hex address in a comment (`0812B5F0`), `_` separators allowed
    addrs = {int(m.replace("_", ""), 16) for m in re.findall(r"0x([0-9a-fA-F][0-9a-fA-F_]*)", text)}
    addrs |= {int(m, 16) for m in re.findall(r"(?<![0-9a-fA-F])(0[38][0-9a-fA-F]{6})(?![0-9a-fA-F])", text)}
    return addrs, set(re.findall(r"[A-Za-z_][A-Za-z0-9_]*", text))


def main(argv):
    folder = data_dir() / "work" / "e5298b24" / (argv[0] if argv else "coverage3")
    rows = list(csv.DictReader((folder / "coverage.csv").open(encoding="utf-8")))
    scenarios = [c for c in rows[0] if c not in ("address", "name", "kind")]
    addrs, words = rust_words()
    sym = {}
    for line in (ROOT / "docs" / "engine" / "symbols.csv").read_text(encoding="utf-8").splitlines()[1:]:
        f = line.split(",")
        sym[int(f[0], 16)] = f[1]

    def in_rust(r):
        a = int(r["address"], 16)
        return a in addrs or (not r["name"].startswith("FUN_") and r["name"] in words) or (sym.get(a) in words)

    def hit(r, group):
        return any(int(r[s]) for s in group)

    careers = [s for s in scenarios if s not in OLD]
    groups = {"all": scenarios, "original four": [s for s in OLD if s in scenarios], "career and ending runs": careers}
    print(f"functions: {len(rows)}; scenarios: {', '.join(scenarios)}")
    print("| set | reached | in the Rust | no counterpart | not reached | of those in the Rust |")
    print("|---|---|---|---|---|---|")
    for label, g in groups.items():
        reached = [r for r in rows if hit(r, g)]
        out = [r for r in rows if not hit(r, g)]
        print(f"| {label} | {len(reached)} | {sum(map(in_rust, reached))} | {sum(not in_rust(r) for r in reached)} "
              f"| {len(out)} | {sum(map(in_rust, out))} |")
    with (folder / "unported.csv").open("w", encoding="utf-8", newline="") as f:
        w = csv.writer(f, lineterminator="\n")
        w.writerow(["address", "name", "kind", "scenarios"])
        for r in rows:
            if hit(r, scenarios) and not in_rust(r):
                w.writerow([r["address"], r["name"], r["kind"], "+".join(s for s in scenarios if int(r[s]))])


if __name__ == "__main__":
    main(sys.argv[1:])
