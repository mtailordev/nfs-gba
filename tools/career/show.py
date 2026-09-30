"""show.py NAME: the screen visits of a career_trace run, with the career profile whenever it changes."""
import json
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import data_dir

t = json.loads((data_dir() / "work" / "e5298b24" / "career" / f"{sys.argv[1]}.json").read_text())
blobs = [bytes.fromhex(b) for b in t["blobs"]]
prev = None
for f, r in enumerate(t["frames"]):
    if "blob" not in r:
        continue
    b = blobs[r["blob"]]
    ev = "".join("%d" % (b[0x205 + (i >> 2)] >> (2 * (i & 3)) & 3) for i in range(66))
    key = (r["screen"], r["state"], b[0xC:0x10], b[0x1FB:0x1FD], b[0x1F8:0x1FB], b[0x205:0x217], b[0x256], b[0x388:0x38E], b[0x42D:0x455])
    if key == prev:
        continue
    prev = key
    print(f, "scr", hex(r["screen"]), "st", r["state"], "cash", struct.unpack_from("<i", b, 0xC)[0], "zone", b[0x1FB], "slot", b[0x1FC],
          "hints", b[0x1F8], b[0x1F9], b[0x1FA], b[0x256], "car", b[0x10], "ev", ev, "|", " ".join("%d" % b[0x388 + z] for z in range(6)), "res", r["results"][8:24])
