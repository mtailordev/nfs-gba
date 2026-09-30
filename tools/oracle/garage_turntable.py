"""Headless capture of the garage turntable over consecutive frames (tools/retro.py): the same career run as
`garage_capture.py` up to the career profile (0x12), then a snapshot on each of the next 30 video frames
(garage2/tt-<screen>-<n>.* and garage2/tt-index.json) while the car turns. The test `garage_screens_match_the_capture`
redraws each from its state and compares page, palette and OAM.

    .venv/Scripts/python.exe tools/oracle/garage_turntable.py [SAVE.sav]
"""
import ctypes as C
import json
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import retro  # noqa: E402
from common import data_dir  # noqa: E402
from garage_capture import BOOT, GAP, MENU, SCREEN  # noqa: E402

FRAMES = 30


def main(argv):
    save = next((a for a in argv if a.endswith(".sav")), None)
    save = Path(save) if save else data_dir() / "work" / "e5298b24" / "race-rules" / "rr-career.sav"
    session = data_dir() / "work" / "e5298b24" / "garage2" / "session-turntable"
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
    t = 1700
    for key, _ in MENU:
        held.update({f: (key,) for f in range(t, t + 12)})
        t += GAP
    settled = t - 20  # the profile screen, 130 frames after its last press
    snaps = []
    for f in range(settled + FRAMES):
        screen = struct.unpack("<i", r.read(SCREEN, 4))[0]
        if f >= settled and screen == 0x12:
            r.dump(f"garage2/tt-{screen}-{len(snaps)}")
            snaps.append([f, screen])
        r.run(1, keys=held.get(f, ()))
    print("snapshots:", len(snaps))
    (data_dir() / "work" / "e5298b24" / "garage2" / "tt-index.json").write_text(json.dumps(snaps), encoding="utf-8")


if __name__ == "__main__":
    main(sys.argv[1:])
