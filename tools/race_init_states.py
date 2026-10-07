"""Career event info states for the race-start captures (`record.py race-init`, scenarios in race_init.py). A test oracle.

    .venv/Scripts/python.exe tools/race_init_states.py NAME [SKILL]    # NAME: a tools/career/NAME.script.json (late, boss, boss2, ...)

Runs the career script's boot presses and screen rules in the libretro core (tools/retro.py, the career save the script
names) up to the event info screen (screen 10, settled 30 frames) and writes work/e5298b24/race-init2/NAME-info.ss (an
mGBA state, traces2.write_ss) for the nightly recorder. SKILL, when given, is poked into the event AI skill word
(0x030000BC) only if the career did not set it. Prints the career globals found.
"""
import json
import shutil
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import retro
from common import data_dir
from traces2 import write_ss

WORK = data_dir() / "work" / "e5298b24" / "race-init2"
CAREER = data_dir() / "work" / "e5298b24" / "career"
SKILL, MODE, ROUTE, CAREER_FLAG = 0x030000BC, 0x030056E0, 0x03005388, 0x030000A0


def word(r, addr):
    return struct.unpack("<I", r.read(addr, 4))[0]


def main(argv):
    import ctypes as C
    name = argv[0]
    cfg = json.loads((Path(__file__).resolve().parent / "career" / f"{name}.script.json").read_text())
    WORK.mkdir(parents=True, exist_ok=True)
    session = WORK / f"session-{name}"
    shutil.rmtree(session, ignore_errors=True)
    session.mkdir(parents=True)
    retro.SESSION = session
    r = retro.Retro(video=False)
    r.core.retro_get_memory_size.restype = C.c_size_t
    start = (CAREER / cfg["save"]).read_bytes()
    C.memmove(r.core.retro_get_memory_data(0), start, len(start))
    held = {f: (key,) for first, n, key in cfg["boot"] for f in range(first, first + n)}
    rules, rule, entered, fired, prev_screen, settled_at = cfg["rules"], 0, 0, -99, None, None
    for f in range(cfg["frames"]):
        r.run(1, keys=held.get(f, ()))
        s = r.serialize()
        iw = s[0x19000:0x21000]
        screen, state, fade, exit_ = (struct.unpack_from("<I", iw, a & 0x7FFF)[0] for a in (0x03005944, 0x03005808, 0x03005630, 0x03005780))
        if screen != prev_screen:
            prev_screen, entered = screen, f
        if screen == 10 and state == 1 and fade == 0 and exit_ == 0 and f - entered >= 30:
            if len(argv) > 1 and word(r, SKILL) == 0:
                r.write(SKILL, struct.pack("<I", int(argv[1])))
            skill = word(r, SKILL)
            write_ss(r.serialize(), WORK / f"{name}-info.ss")
            print(name, "frame", f, "skill", skill, "mode", word(r, MODE), "route", word(r, ROUTE), "career", word(r, CAREER_FLAG))
            shutil.rmtree(session, ignore_errors=True)
            return
        if f > cfg["boot_frames"] and rule < len(rules) and state == 1 and fade == 0 and exit_ == 0:
            sc, wait, key, *opt = rules[rule]
            if opt and screen != sc and any(x[0] == screen for x in rules[rule + 1:]):
                rule += 1
            elif screen == sc and f - max(entered, fired + 12) >= wait:
                held.update({g: (key,) for g in range(f + 1, f + 13)})
                rule, fired = rule + 1, f
    sys.exit(f"{name}: the event info screen was not reached")


if __name__ == "__main__":
    main(sys.argv[1:])
