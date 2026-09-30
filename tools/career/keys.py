"""keys.py NAME [FROM]: the frames whose key register is non-zero (first of each run), and the number of frames."""
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import data_dir

t = json.loads((data_dir() / "work" / "e5298b24" / "career" / f"{sys.argv[1]}.json").read_text())
fr = t["frames"]
print(len(fr), "frames;", [(f, hex(r["keys"]), hex(r["screen"])) for f, r in enumerate(fr) if r["keys"] and (f == 0 or not fr[f - 1]["keys"]) and f >= int(sys.argv[2] if len(sys.argv) > 2 else 0)])
