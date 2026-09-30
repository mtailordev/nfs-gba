"""Headless recordings for the ledger rows D14, D8, G1 and U8 (fixture folder `traces2`). A test oracle, not a runtime.

    .venv/Scripts/python.exe tools/traces2.py states     # retro: power-on to the Quick Play setup screen, per race case
    .venv/Scripts/python.exe tools/traces2.py pause      # retro: a race, START, a wait, A (resume): per video frame

`states` runs the key script of session_trace.py (power-on, a fresh save, the menus to Quick Play), sets the race choice on
the setup screen (screen 10) of each CASE and writes, into $NFSGBA_DATA/work/e5298b24/traces2/:
  NAMEinfo.ss       the machine on that screen (an mGBA state: `record.py game record start-NAME` loads it)
  session-NAME.json the script, the choice, the tick and the opponents the race got, for the Session tests
The mGBA-Lua recordings that need breakpoints (`tools/recorders/traces2.lua`: rand_table draws, the rim redraw) and the
game-frame traces start from the .ss files: see `tools/recorders/game.py` (scenarios start-circuit, start-wingman) and
`tools/record.py run` (the circle scenario, `circle.json` in this folder's provenance row).
"""
import json
import shutil
import struct
import sys
import time
import zlib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import retro
import session_trace
from common import data_dir

SETUP_AT = 2620  # on the Quick Play setup screen (screen 10), entered at ~2563; the last A (2670) starts the race
SAVE_AT = 2640
END = 2800
# name: race choice poked on the setup screen (profile +0x200 = the wingman; opponents in the settings copy +0x3C8)
CASES = {"circuit": dict(mode=0, opponents=3, wingman=0), "wingman": dict(mode=0, opponents=2, wingman=1)}
WORK = data_dir() / "work" / "e5298b24" / "traces2"
STATE_SIZE = 397312
CARS, PAINTS = 0x0300611C, 0x03005FEC


def write_ss(raw, path):
    """An mGBA savestate PNG: a nightly state's PNG (`mgba/race.ss`: the screenshot and the extra-data chunks) with its
    gbAs chunk replaced by `raw` (the libretro core appends 576 bytes to mGBA's 397,312-byte state)."""
    src = (data_dir() / "work" / "e5298b24" / "mgba" / "race.ss").read_bytes()
    out, i = [src[:8]], 8
    while i < len(src):
        n, tag = struct.unpack(">I4s", src[i:i + 8])
        if tag == b"gbAs":
            data = zlib.compress(raw[:STATE_SIZE])
            out.append(struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data)))
        else:
            out.append(src[i:i + 12 + n])
        i += 12 + n
    Path(path).write_bytes(b"".join(out))


def poke_choice(r, row, c):
    p = row["profile"] & 0x3FFFF
    r.write(0x02000000 + p + 0x200, struct.pack("<I", c["wingman"]))
    r.write(0x02000000 + p + 0x3C8, struct.pack("<I", c["opponents"]))  # the settings copy of the opponents
    r.write(0x030056E0, struct.pack("<I", c["mode"]))
    r.write(0x03005784, struct.pack("<I", c["opponents"]))


def run_case(name, c):
    session = WORK / f"session-{name}"
    shutil.rmtree(session, ignore_errors=True)
    session.mkdir(parents=True)
    retro.SESSION = session
    r = retro.Retro(video=False)
    held = {}
    for first, n, key in session_trace.SCRIPT:
        for f in range(first, first + n):
            held[f] = (key,)
    out = dict(script=session_trace.SCRIPT, sync_at=session_trace.SYNC_AT, case=c)
    for f in range(END):
        r.run(1, keys=held.get(f, ()))
        if f == SETUP_AT:
            row = session_trace.rows(r.serialize())
            assert (row["screen"], row["state"]) == (10, 1), row
            poke_choice(r, row, c)
        if f == SAVE_AT:
            s = r.serialize()
            row = session_trace.rows(s)
            assert (row["screen"], row["state"]) == (10, 1) and row["mode"] == c["mode"], row
            write_ss(s, WORK / f"{name}info.ss")
            out["choice"] = {k: row[k] for k in ("mode", "route", "reverse", "laps", "opponents", "difficulty", "traffic", "car")}
            out["press_at"] = 2670
        if f > SAVE_AT:
            s = r.serialize()
            row = session_trace.rows(s)
            if "tick_at_state4" not in out and row["state"] == 4:
                out["tick_at_state4"] = row["ticks"]
            if row["state"] == 5 and "cars" not in out:
                iw = s[0x19000:0x21000]
                out["cars"] = list(struct.unpack_from("<4b", iw, CARS & 0x7FFF))
                out["paints"] = list(struct.unpack_from("<4b", iw, PAINTS & 0x7FFF))
                out["race_start_frame"] = f
                out["tick_at_race"] = row["ticks"]
    assert "cars" in out, "the race did not start"
    (WORK / f"session-{name}.json").write_text(json.dumps(out), encoding="utf-8")
    print(name, {k: v for k, v in out.items() if k not in ("script",)})


def lua_run(name, state, commands):
    """mGBA (nightly, scripted: tools/mgba_ctl.py) with traces2.lua; `state` is a .ss in the folder (empty: power-on)."""
    import mgba_ctl
    from recorders import probe, running
    (WORK / f"{name}.log").unlink(missing_ok=True)
    if not state:  # a power-on run needs a fresh save: the core writes the profile to the session's .sav
        for sav in WORK.glob("*.sav"):
            for _ in range(20):  # the last emulator may still be closing it
                try:
                    sav.unlink()
                    break
                except PermissionError:
                    time.sleep(0.5)
    with running("traces2", ["traces2"]) as (work, _):
        mgba_ctl.send(*([f"load {state}"] if state else []), f'luax TR2NAME="{name}"', *commands, session="traces2")
    print(name, sum(1 for _ in open(WORK / f"{name}.log")), "log lines")


def menus(name):
    """Power-on to a race through the menus (the screen-conditioned key driver of traces2.lua, the choice poked on the setup
    screen): every rand_table draw from power-on to the grid deal and every main_frame's screen, state, counters and keys
    (`menus-NAME.log`)."""
    c = CASES[name]
    choice = f"luax TR2CHOICE={{mode={c['mode']},opponents={c['opponents']},wingman={c['wingman']}}}"
    lua_run(f"menus-{name}", "", ["luax emu:reset()", choice, "luax TR2DRIVE()", "wait 3300"])


def circle():
    """A circuit from the setup screen (circuitinfo.ss), the player holding A and LEFT for 30 s: the rim redraw decisions
    (`circle.log`: E, V and D lines of traces2.lua) over a full turn."""
    lua_run("circle", "circuitinfo", ["hold A 10", "wait 700", "hold A,LEFT 1800", "wait 30"])


def main(argv):
    WORK.mkdir(parents=True, exist_ok=True)
    cmd = argv[0] if argv else "states"
    if cmd == "states":
        for name, c in CASES.items():
            run_case(name, c)
    elif cmd == "menus":
        for name in argv[1:] or CASES:
            menus(name)
    elif cmd == "circle":
        circle()
    else:
        sys.exit(f"unknown command {cmd!r}")


if __name__ == "__main__":
    main(sys.argv[1:])
