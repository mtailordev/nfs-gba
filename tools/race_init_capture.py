"""Capture machine states around the race start (docs/engine/race-init.md), in the mGBA session `race-init`.

    .venv/Scripts/python.exe tools/race_init_capture.py            # every scenario
    .venv/Scripts/python.exe tools/race_init_capture.py sprint

Each scenario loads a savestate, arms tools/race_init_capture.lua with its name and presses keys until the race
starts; the Lua saves NAME_pre.<domain>.bin at the entry of race_start_from_table_a and NAME_post.<domain>.bin at
its return. The savestates are copied into the session from the sessions that made them (listed below).
"""
import os
import shutil
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import ROOT, data_dir  # noqa: E402

SESSION = "race-init"
# savestate (session/name), keys; "b" variants start the same race after a different menu history.
SCENARIOS = {
    "ref": ("mgba/mainmenu", ["hold A 10", "wait 60", "hold A 10", "wait 60", "hold A 10", "wait 60", "hold A 10"]),
    "refb": ("mgba/mainmenu", ["wait 200", "hold A 10", "wait 97", "hold A 10", "wait 60", "hold A 10", "wait 131",
                               "hold A 10"]),
    "sprint": ("ai-traffic/sprintinfo", ["hold A 10"]),
    "sprintb": ("ai-traffic/sprintinfo", ["wait 77", "hold A 10"]),
    "circuit": ("ai-traffic/circuitinfo", ["hold A 10"]),
    "circuitb": ("ai-traffic/circuitinfo", ["wait 131", "hold A 10"]),
    "wingman": ("ai-traffic/wingmaninfo", ["hold A 10"]),
    "wingmanb": ("ai-traffic/wingmaninfo", ["wait 53", "hold A 10"]),
    "hunter": ("physics-paths/hunter-info", ["hold A 10"]),
    "hunterb": ("physics-paths/hunter-info", ["wait 91", "hold A 10"]),
    # Quick Play race info: LONGPOINT, Mazda RX-7, light traffic / VW Golf GTI, easy, no traffic.
    "rx7": ("hud-logic/prerace", ["hold A 10"]),
    "golf": ("physics-paths/easy-info", ["hold A 10"]),
    # The RX-7 race as an elimination: the mode byte (0x030056E0) poked on the info screen.
    "elimination": ("hud-logic/prerace", ["poke 0x030056E0 1", "hold A 10"]),
    # Career: the event grid (Lucky 7's, LONGPOINT circuit), A until the race starts.
    "career": ("race-rules/event_grid", ["hold A 10", "wait 60", "hold A 10", "wait 60", "hold A 10", "wait 60",
                                         "hold A 10"]),
}


def ctl(*cmds):
    import mgba_ctl
    mgba_ctl.main(list(cmds))


def run(names):
    work = data_dir() / "work" / "e5298b24" / SESSION
    work.mkdir(parents=True, exist_ok=True)
    for name in names:
        src, keys = SCENARIOS[name]
        state = src.split("/")[1]
        shutil.copyfile(data_dir() / "work" / "e5298b24" / f"{src}.ss", work / f"{state}.ss")
        ctl(f"load {state}", "wait 2")
        (work / "raceinit.tmp").write_text(name + "\n")
        (work / "raceinit.tmp").replace(work / "raceinit.txt")
        ctl("wait 2", *keys, "wait 400")
        post = work / f"{name}_post.wram.bin"
        deadline = time.time() + 60
        while not post.exists() and time.time() < deadline:
            time.sleep(0.5)
        print(name, "captured" if post.exists() else "NOT captured (the race did not start?)")


if __name__ == "__main__":
    os.environ["NFSGBA_MGBA_SESSION"] = SESSION
    os.environ.setdefault("NFSGBA_MGBA", str(Path("E:/Games/rewrites/nfs_gba/ext/mgba-dev/mGBA.exe")))
    os.environ.setdefault("NFSGBA_MGBA_EXTRA", str(ROOT / "tools" / "race_init_capture.lua"))
    names = sys.argv[1:] or list(SCENARIOS)
    run(names)
