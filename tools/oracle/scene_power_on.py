"""Headless power-on run to the main menu (tools/retro.py) with oracle snapshots at the menu screens' settled frames
(menus3/po-<screen>-<n>.*): the whole-scene check of the typed menus (`menu::typed`, test
`power_on_screens_match_the_game`) redraws each screen from its state and compares page, palette and OAM.

    .venv/Scripts/python.exe tools/oracle/scene_power_on.py [log]

Keys as docs/TOOLS.md: A A A START A (language, intro), about 900 frames of timed logos, START at PRESS START, then
A START on the name screen. `log` only prints (frame, screen) changes.
"""
import json
import struct
import sys

import pathlib

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent.parent))
from common import data_dir  # noqa: E402
from retro import Retro  # noqa: E402

SCREEN, ENTERED, FADE, MENU_EXIT = 0x03005944, 0x03005938, 0x03005630, 0x03005780
PRESSES = [(10, "A"), (40, "A"), (70, "A"), (100, "START"), (130, "A")]  # then START/A/START by screen below


def keys_for(frame, screen_now):
    for start, name in PRESSES:
        if start <= frame < start + 12:
            return (name,)
    return ()


def main(argv):
    r = Retro(video=False)
    seen, log, stable, snaps = None, [], 0, []
    extra = []  # presses decided from the screen: START at the title, A then START at the name screen
    for f in range(4000):
        screen = struct.unpack("<i", r.read(SCREEN, 4))[0]
        if screen != seen:
            log.append((f, screen))
            seen, stable = screen, 0
        stable += 1
        keys = keys_for(f, screen)
        if not keys and f > 600:
            phase = f % 60
            if screen in (23, 24) and phase < 12:  # PRESS START / logos: START
                keys = ("START",)
            elif screen in (22, 25, 26, 47, 48) and phase < 12:
                keys = ("A",) if (f // 60) % 2 else ("START",)
        if stable == 30 and "log" not in argv:
            r.dump(f"menus3/po-{screen}-{len(snaps)}")
            snaps.append([f, screen])
        r.run(1, keys=keys)
    print("screens:", log)
    print("snapshots:", snaps)
    if "log" not in argv:
        (data_dir() / "work" / "e5298b24" / "menus3" / "po-index.json").write_text(json.dumps(snaps), encoding="utf-8")


if __name__ == "__main__":
    main(sys.argv[1:])
