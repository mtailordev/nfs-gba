"""Power-on to the main menu in headless mGBA (tools/retro.py), one row of menu state per frame.

    .venv/Scripts/python.exe tools/boot_trace.py NAME [SAVE.sav]      # NAME: fresh (no save) or existing (SAVE given)

Writes $NFSGBA_DATA/work/e5298b24/boot/NAME.json for crates/nfsgba-game/src/menu/boot.rs (test `boot_matches_the_game`):
the key script, per frame the main_frame counter (0x03005628, which the vblank IRQ also bumps while 0x03005398 is 0 and 0x03000048 is 2; a frame that waits inside a screen change spans several
video frames, so rows are sampled where it changes), keys (0x030064C0), tick counter (0x03000044) and the menu state, the
first frame's globals, and the EEPROM at the end. The given save goes into the core's SAVE_RAM before the first frame.
A test oracle, not a runtime.
"""
import ctypes as C
import json
import shutil
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import retro
from common import data_dir

FRAMES = 2400
# (first frame, frames held, key): menus need presses of at least 10 frames; the timed logo screens run on their own.
SCRIPT = [(30, 12, "A"), (60, 12, "A"), (90, 12, "A"), (120, 12, "START"), (150, 12, "A"),
          (1500, 12, "START"), (1560, 12, "A"), (1600, 12, "START"), (1700, 12, "DOWN"), (1740, 12, "A")]
IW = {"count": (0x03005628, "I"), "irq_a": (0x03005398, "I"), "irq_b": (0x03000048, "I"), "keys": (0x030064C0, "H"), "ticks": (0x03000044, "i"), "screen": (0x03005944, "I"), "state": (0x03005808, "I"),
      "back_top": (0x0300593C, "b"), "language": (0x03005600, "I"), "fade": (0x03005630, "i"),
      "menu_exit": (0x03005780, "I"), "message_box": (0x030059F0, "i"), "entered": (0x03005938, "I"),
      "units": (0x03000040, "I"), "save_slot": (0x030057F4, "I"), "profile": (0x030056EC, "I")}


def rows(s):
    """The named IWRAM words and the profile fields of one serialized state."""
    iw = s[0x19000:0x19000 + 0x8000]
    ew = s[0x21000:0x21000 + 0x40000]
    row = {k: struct.unpack_from("<" + f, iw, a & 0x7FFF)[0] for k, (a, f) in IW.items()}
    p = row["profile"] & 0x3FFFF
    if row["profile"] >> 24 == 2:
        row["exists"] = struct.unpack_from("<H", ew, p + 0x490)[0]
        row["saved_language"] = struct.unpack_from("<H", ew, p + 0x4E8)[0]
        row["name"] = ew[p:p + 9].hex()
        row["loaded"] = ew[p + 0x328]
        row["slot_name"] = ew[p + 0x496:p + 0x49F].hex()
    return row


def main(argv):
    name = argv[0]
    out = data_dir() / "work" / "e5298b24" / "boot"
    session = out / f"session-{name}"
    if session.exists():
        shutil.rmtree(session)
    session.mkdir(parents=True)
    retro.SESSION = session
    r = retro.Retro(video=False)
    core = r.core
    core.retro_get_memory_data.restype = C.c_void_p
    core.retro_get_memory_size.restype = C.c_size_t
    start = b""
    if len(argv) > 1:  # the frontend loads the save into the core's SAVE_RAM (the core does not read .sav files)
        start = Path(argv[1]).read_bytes()
        assert core.retro_get_memory_size(0) >= len(start)  # the type is unknown until the game's first EEPROM access
        C.memmove(core.retro_get_memory_data(0), start, len(start))
    held = {}
    for first, n, key in SCRIPT:
        for f in range(first, first + n):
            held[f] = (key,)
    trace = []
    for f in range(FRAMES):
        r.run(1, keys=held.get(f, ()))
        trace.append(rows(r.serialize()))
    eeprom = C.string_at(core.retro_get_memory_data(0), core.retro_get_memory_size(0))
    out.mkdir(parents=True, exist_ok=True)
    (out / f"{name}.json").write_text(json.dumps(dict(
        script=SCRIPT, rom_sha1_8="e5298b24", save=argv[1] if len(argv) > 1 else None,
        start_eeprom=start.hex(), frames=trace, end_eeprom=eeprom.hex())), encoding="utf-8")
    seq = [(f, t["screen"]) for f, t in enumerate(trace) if f == 0 or t["screen"] != trace[f - 1]["screen"]]
    print(name, "screens:", seq)


if __name__ == "__main__":
    main(sys.argv[1:])
