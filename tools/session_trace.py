"""Power-on (fresh save) to a Quick Play race and its results in headless mGBA (tools/retro.py), one row per video frame,
for crates/nfsgba-game/src/session.rs (test `session_matches_the_game`). A test oracle, not a runtime.

    .venv/Scripts/python.exe tools/session_trace.py [NAME]     # writes $NFSGBA_DATA/work/e5298b24/session/NAME.json
    .venv/Scripts/python.exe tools/session_trace.py NAME --drive A,LEFT --finish-at 3000 --out session3     # the player drives badly: keys held from the race's frame DRIVE_AT, the finish poked late (3000 frames), until the results settle
    .venv/Scripts/python.exe tools/session_trace.py NAME --language 2 --out session2    # German (cursor RIGHT twice on the language screen), into a new folder

The key script is the boot script (boot_trace.py) to the main menu, then presses into Quick Play. Once the race has run
FINISH_AT game frames the player's entity is marked finished (`+0x4A` = 2, as tools/recorders/finish.lua does) and the
race ends by itself. Per frame: the boot rows, the race choice (mode, route, laps, ...) and the results blocks.
"""
import json
import shutil
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import boot_trace
import retro
from common import data_dir

FINISH_AT = 150
SYNC_AT = 2520  # on the race setup screen: the random choice is synced and the mode poked to a circuit (hunter races are not ported)
FRAMES = 4600
# (first frame, frames held, key): the boot script to the main menu (frame ~2400), then Quick Play.
SCRIPT = boot_trace.SCRIPT + [(2450, 12, "A"), (2560, 12, "A"), (2670, 12, "A"), (2780, 12, "A")]
DRIVE_AT = 150  # with --drive: the race frame from which the keys are held
DRIVE_MAX = 30000  # video frames at most
LANGUAGE_AT = 18  # the language screen (25) is up from frame 17; each cursor step is a 12-frame press, 22 frames apart
LANGUAGE_SHIFT = 50  # the rest of the script moves later by this many frames per two cursor steps


def script(language=0):
    """SCRIPT with the language screen cursor moved `language` steps right first (0 En, 1 Fr, 2 De, 3 It, 4 Es) (everything after it shifted later)."""
    if not language:
        return SCRIPT
    shift = LANGUAGE_SHIFT * ((language + 1) // 2)
    cursor = [(LANGUAGE_AT + 22 * i, 12, "RIGHT") for i in range(language)]
    return cursor + [(f + shift, n, k) for f, n, k in SCRIPT]


RACE = {"mode": (0x030056E0, "I"), "route": (0x03005388, "I"), "laps": (0x030056E4, "I"), "opponents": (0x03005784, "I"),
        "difficulty": (0x03005608, "I"), "traffic": (0x03005604, "I"), "env": (0x0300006C, "I"), "career": (0x030000A0, "I"),
        "player_car": (0x03005718, "I"), "race_car": (0x0300611C, "B"), "route_flag": (0x03005720, "I"),
        "player": (0x03000060, "I"), "phase": (0x03000048, "I"), "reverse": (0x03005610, "I")}


def rows(s):
    row = boot_trace.rows(s)
    iw = s[0x19000:0x19000 + 0x8000]
    for k, (a, f) in RACE.items():
        row[k] = struct.unpack_from("<" + f, iw, a & 0x7FFF)[0]
    if row["profile"] >> 24 == 2:
        row["car"] = s[0x21000 + (row["profile"] & 0x3FFFF) + 0x11]
    row["results"] = iw[0x5650:0x5690].hex()
    row["ranked"] = iw[0x5730:0x5770].hex()
    return row


def main(argv):
    argv = list(argv)
    def opt(k, d):
        if k not in argv:
            return d
        i = argv.index(k)
        v = argv[i + 1]
        del argv[i:i + 2]
        return v

    language, folder = int(opt("--language", 0)), opt("--out", "session")
    finish_at = int(opt("--finish-at", FINISH_AT))
    drive = [k for k in opt("--drive", "").split(",") if k]
    name = argv[0] if argv else "quickplay"
    out = data_dir() / "work" / "e5298b24" / folder
    keys = script(language)
    shift = keys[-1][0] - SCRIPT[-1][0]
    sync_at, frames = SYNC_AT + shift, (DRIVE_MAX if drive else FRAMES + shift)
    session = out / f"session-{name}"
    shutil.rmtree(session, ignore_errors=True)
    session.mkdir(parents=True)
    retro.SESSION = session
    r = retro.Retro(video=False)
    held = {}
    for first, n, key in keys:
        for f in range(first, first + n):
            held[f] = (key,)
    trace, poked, started = [], False, False
    for f in range(frames):
        if f == sync_at:
            r.write(0x030056E0, struct.pack("<I", 0))
        keys_now = held.get(f, ())
        if drive and trace and trace[-1]["state"] == 5 and trace[-1]["screen"] == 0x81 and trace[-1]["count"] >= DRIVE_AT:
            keys_now = tuple(drive)
        r.run(1, keys=keys_now)
        s = r.serialize()
        row = rows(s)
        if drive:
            row["drive"] = list(keys_now)
        started = started or (row["state"] == 5 and row["count"] < finish_at and row["fade"] == 0)
        if started and not poked and row["state"] == 5 and row["count"] >= finish_at:
            iw = s[0x19000:0x21000]
            entity = struct.unpack_from("<I", iw, 0xFC)[0] + struct.unpack_from("<I", iw, 0x60)[0] * 0xA4
            r.write(entity + 0x4A, struct.pack("<H", 2))
            poked = True
            row["poke"] = True
        trace.append(row)
        if drive and row["screen"] == 12 and row["state"] == 1 and len(trace) > 100 and all(t["screen"] == 12 for t in trace[-100:]):
            break
    out.mkdir(parents=True, exist_ok=True)
    (out / f"{name}.json").write_text(json.dumps(dict(script=keys, finish_at=finish_at, drive=drive, drive_at=DRIVE_AT, sync_at=sync_at, language=language, frames=trace)), encoding="utf-8")
    seq = [(f, t["screen"], t["state"]) for f, t in enumerate(trace)
           if f == 0 or (t["screen"], t["state"]) != (trace[f - 1]["screen"], trace[f - 1]["state"])]
    print(name, "screens:", seq)


if __name__ == "__main__":
    main(sys.argv[1:])
