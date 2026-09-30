"""Consecutive-frame snapshots of the boot screens whose frames change by themselves (menus4/pf-<screen>-<n>.*), the
whole-scene check of `menu::typed` over time (test `power_on_frames_match_the_game`): screen 48 (0x30, health and safety
with colour 4 blinking) for 24 frames, and the name keyboard (22, 0x16) for 60 frames with scripted cursor moves, a
typed letter and a delete. Same headless power-on run as scene_power_on.py (no presses after the language screen).

    .venv/Scripts/python.exe tools/oracle/scene_frames.py [log]
"""
import json
import pathlib
import struct
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent.parent))
from common import data_dir  # noqa: E402
from retro import Retro  # noqa: E402
from scene_power_on import PRESSES, SCREEN, keys_for  # noqa: E402

FOLDER = "menus4"
HEALTH_FRAMES, NAME_FRAMES = 24, 60
# Name screen, frames since the capture started: (first frame, key); each is held 3 frames.
NAME_KEYS = [(6, "RIGHT"), (14, "DOWN"), (22, "A"), (30, "RIGHT"), (38, "A"), (46, "B")]


def main(argv):
    r = Retro(video=False)
    seen, stable, snaps, cap = None, 0, [], {}
    for f in range(2600):
        screen = struct.unpack("<i", r.read(SCREEN, 4))[0]
        if screen != seen:
            seen, stable = screen, 0
        stable += 1
        keys = keys_for(f, screen)
        if not keys and f > 600 and f % 60 < 12:  # as scene_power_on.py, up to the name screen
            if screen in (23, 24):
                keys = ("START",)
            elif screen in (25, 26, 47, 48):
                keys = ("A",) if (f // 60) % 2 else ("START",)
        if screen in (48, 22) and stable >= 30 and "log" not in argv:
            left = cap.setdefault(screen, [f, 0])
            n, limit = left[1], HEALTH_FRAMES if screen == 48 else NAME_FRAMES
            if n < limit:
                if screen == 22:
                    t = f - left[0]
                    keys = tuple(k for s, k in NAME_KEYS if s <= t < s + 3)
                r.dump(f"{FOLDER}/pf-{screen}-{len(snaps)}")
                snaps.append([f, screen])
                left[1] += 1
        r.run(1, keys=keys)
        if cap.get(48, [0, 0])[1] == HEALTH_FRAMES and cap.get(22, [0, 0])[1] == NAME_FRAMES:
            break
    print("snapshots:", len(snaps), {s: c[1] for s, c in cap.items()})
    if "log" not in argv:
        (data_dir() / "work" / "e5298b24" / FOLDER / "pf-index.json").write_text(json.dumps(snaps), encoding="utf-8")


if __name__ == "__main__":
    main(sys.argv[1:])
