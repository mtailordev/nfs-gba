"""Headless capture of the garage screens (tools/retro.py): power-on with a career save, into the main menu, then
key presses through GARAGE -> car select -> the garage list -> the part shop (0x13) and the upgrade pages (0x14), and
the career profile (0x12). Oracle snapshots at each settled Kind18 frame (menus3/gar-<screen>-<n>.*) and
menus3/gar-index.json; the whole-scene check `menu::typed::tests::garage_screens_match_the_game` redraws each from
its state and compares page, palette and OAM.

    .venv/Scripts/python.exe tools/oracle/garage_capture.py [log] [SAVE.sav]

Default save: race-rules/rr-career.sav (a career with unlocks). `log` only prints (frame, screen) changes.
"""
import ctypes as C
import json
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import retro  # noqa: E402
from common import data_dir  # noqa: E402

SCREEN = 0x03005944
BOOT = [(30, "A"), (60, "A"), (90, "A"), (120, "START"), (150, "A"), (1500, "START")]
# From the main menu (career): (frame, key). GARAGE is the second item; the garage list's first item is the part shop
# (screen 29 -> 0x13 pages), the second the upgrade pages (30 -> 0x14); PROFILE is the fourth main item (0x12).
MENU = [(k, 1) for k in (
    "RIGHT A RIGHT A A A "  # main -> career -> GARAGE (9) -> car (owned: 4) -> performance parts (29)
    "RIGHT A DOWN RIGHT RIGHT B "  # the part shop (0x13) page 0: row, picks; back
    "RIGHT A UP RIGHT B B "  # page 1; back to the garage list (4)
    "RIGHT A RIGHT RIGHT RIGHT A "  # visual upgrades (30): item 3 -> the upgrade page (0x14)
    "RIGHT LEFT B RIGHT A LEFT B B B B "  # its level; the next upgrade page; back to the career menu
    "RIGHT RIGHT A").split()]  # PROFILE (0x12): the career menu cursor rests on GARAGE
GAP = 150  # frames between presses: enter is slow (a fade in, the scene setup)


def main(argv):
    save = next((a for a in argv if a.endswith(".sav")), None)
    save = Path(save) if save else data_dir() / "work" / "e5298b24" / "race-rules" / "rr-career.sav"
    session = data_dir() / "work" / "e5298b24" / "menus3" / "session-garage"
    session.mkdir(parents=True, exist_ok=True)
    retro.SESSION = session
    r = retro.Retro(video=False)
    core = r.core
    core.retro_get_memory_data.restype = C.c_void_p
    core.retro_get_memory_size.restype = C.c_size_t
    start = save.read_bytes()
    C.memmove(core.retro_get_memory_data(0), start, len(start))
    held = {}
    for first, key in BOOT:
        held.update({f: (key,) for f in range(first, first + 12)})
    t, looks = 1700, set()
    for key, _ in MENU:
        held.update({f: (key,) for f in range(t, t + 12)})
        looks.add(t + GAP - 20)  # settled: 78 frames after the press, before the next one
        t += GAP
    seen, stable, log, snaps = None, 0, [], []
    for f in range(t + 200):
        screen = struct.unpack("<i", r.read(SCREEN, 4))[0]
        if screen != seen:
            log.append((f, screen))
            seen, stable = screen, 0
        stable += 1
        if f in looks and screen in (0x12, 0x13, 0x14) and "log" not in argv:
            r.dump(f"menus3/gar-{screen}-{len(snaps)}")
            snaps.append([f, screen])
        r.run(1, keys=held.get(f, ()))
    print("screens:", log)
    print("snapshots:", snaps)
    if "log" not in argv:
        (data_dir() / "work" / "e5298b24" / "menus3" / "gar-index.json").write_text(json.dumps(snaps), encoding="utf-8")


if __name__ == "__main__":
    main(sys.argv[1:])
