"""rk.py NAME FRAME...: the trace rows (without the big hex fields) of the frames."""
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import data_dir

t = json.loads((data_dir() / "work" / "e5298b24" / "career" / f"{sys.argv[1]}.json").read_text())
for f in map(int, sys.argv[2:]):
    r = t["frames"][f]
    print(f, {k: v for k, v in r.items() if k not in ("ranked", "results", "profile")})
