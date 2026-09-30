"""A career run from a chosen save in headless mGBA (tools/retro.py), one row per video frame, for
crates/nfsgba-game/tests/career.rs. A test oracle, not a runtime.

    .venv/Scripts/python.exe tools/career_trace.py NAME     # reads tools/career/NAME.script.json (the save it names is in $NFSGBA_DATA/work/e5298b24/career/)

The script file: {"save": "x.sav", "frames": N, "boot": [[first, n, "A"], ...], "boot_frames": F, "rules": [[screen, wait,
"A"], ...], "tail": 300, "finish_at": 150, "win": true}. The boot keys are by video frame (power-on to the main menu, before
frame F); the rules are the presses after it, in order: rule i fires (a 12 frame press) when the menu screen has been the
current one and settled (state 1, no fade, no exit) for `wait` frames since it was entered or the last press, so a run
whose screens take other times (ours: none) makes the same choices. A rule with a fourth element is optional: it is
skipped when the screen is one a later rule presses on. The run ends `tail` frames after the last press.
Writes NAME.json next to it: per frame the session_trace rows plus the career profile (index into `blobs`, hex,
deduplicated) and sha1 (16 hex) of the shown page, the 512 palette entries and the OAM. Once the race has run
finish_at game frames the player's entity is marked finished (as session_trace does).
"""
import ctypes as C
import hashlib
import json
import shutil
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import retro
from common import data_dir
from session_trace import rows as base_rows

PROFILE = 0x4F0


def sha(b):
    return hashlib.sha1(b).hexdigest()[:16]


def rows(s, blobs):
    row = base_rows(s)
    if row["profile"] >> 24 == 2:
        ew = s[0x21000:0x21000 + 0x40000]
        blob = ew[row["profile"] & 0x3FFFF:][:PROFILE].hex()
        row["blob"] = blobs.setdefault(blob, len(blobs))
        row["hint"] = ew[(row["profile"] & 0x3FFFF) + 0x1FA]
    dispcnt = struct.unpack_from("<H", s, 0x400)[0]
    row["dispcnt"] = dispcnt
    page = 0x1000 + (0xA000 if dispcnt & 0x10 else 0)
    return row, (s[page:page + 0x9600], s[0x800:0xC00], s[0xC00:0x1000])


def main(argv):
    name = argv[0]
    out = data_dir() / "work" / "e5298b24" / "career"
    cfg = json.loads((Path(__file__).resolve().parent / "career" / f"{name}.script.json").read_text())
    session = out / f"session-{name}"
    shutil.rmtree(session, ignore_errors=True)
    session.mkdir(parents=True)
    retro.SESSION = session
    r = retro.Retro(video=False)
    core = r.core
    core.retro_get_memory_size.restype = C.c_size_t
    start = (out / cfg["save"]).read_bytes()
    C.memmove(core.retro_get_memory_data(0), start, len(start))
    held = {}
    for first, n, key in cfg["boot"]:
        for f in range(first, first + n):
            held[f] = (key,)
    rules, rule, entered, fired, end = cfg["rules"], 0, 0, -99, None  # see the module docs
    prev_key, tail_frames = None, cfg["tail"] + 13
    finish_at = cfg.get("finish_at", 150)
    trace, blobs, poked, started, settled, last_key = [], {}, False, False, None, None
    for f in range(cfg["frames"]):
        if end is not None and f >= end:
            break
        r.run(1, keys=held.get(f, ()))
        s = r.serialize()
        row, shot = rows(s, blobs)
        key = (row["screen"], row["state"], row.get("hint"))  # a hint page counts as a visit
        if key != last_key:  # the last settled frame of the visit that ended carries its screen
            if settled is not None:
                trace[settled[0]]["shot"] = [x.hex() for x in settled[1]]
            settled, last_key = None, key
        if row["state"] != 5 and row["fade"] == 0 and row["menu_exit"] == 0:
            settled = (len(trace), shot)
        if row["screen"] != 0x81 and row["state"] != 5:
            started = poked = False
        started = started or (row["state"] == 5 and row["count"] < finish_at and row["fade"] == 0)
        if started and not poked and row["state"] == 5 and row["count"] >= finish_at:
            iw = s[0x19000:0x21000]
            entity = struct.unpack_from("<I", iw, 0xFC)[0] + struct.unpack_from("<I", iw, 0x60)[0] * 0xA4
            r.write(entity + 0x4A, struct.pack("<H", 2))
            if cfg.get("win"):  # every lap counted: the finish estimate puts the player first
                driver = struct.unpack_from("<I", s, 0x21000 + (entity & 0x3FFFF) + 0x8C)[0]
                r.write(driver + 0xC5, b"\0")
                r.write(driver + 0xAC, struct.pack("<i", 1000000))  # sprints count the distance alone
            poked = True
            row["poke"] = True
        trace.append(row)
        if row["screen"] != prev_key:
            prev_key, entered = row["screen"], f
        if f > cfg["boot_frames"] and rule < len(rules) and row["state"] == 1 and row["fade"] == 0 and row["menu_exit"] == 0:
            screen, wait, key, *opt = rules[rule]
            if opt and row["screen"] != screen and any(x[0] == row["screen"] for x in rules[rule + 1:]):
                rule += 1  # an optional press whose screen was left for a later rule's
                if rule == len(rules):
                    end = f + tail_frames
            elif row["screen"] == screen and f - max(entered, fired + 12) >= wait:
                held.update({g: (key,) for g in range(f + 1, f + 13)})
                rule, fired = rule + 1, f
                if rule == len(rules):
                    end = f + 13 + cfg["tail"]
    if settled is not None:
        trace[settled[0]]["shot"] = [x.hex() for x in settled[1]]
    (out / f"{name}.json").write_text(json.dumps(dict(cfg=cfg, blobs=list(blobs), frames=trace)), encoding="utf-8")
    seq = [(f, t["screen"], t["state"]) for f, t in enumerate(trace)
           if f == 0 or (t["screen"], t["state"]) != (trace[f - 1]["screen"], trace[f - 1]["state"])]
    print(name, "rules fired:", rule, "of", len(rules), "screens:", seq)


if __name__ == "__main__":
    main(sys.argv[1:])
