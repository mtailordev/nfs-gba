"""Record the race HUD from a savestate (probe hud.lua; format in docs/formats/ui.md, "HUD logic").

    .venv/Scripts/python.exe tools/record.py hud STATE FRAMES [NAME [KEYS [POKES]]]

KEYS holds buttons as START:LEN:KEY[+KEY],... and POKES temporary RAM writes, both in frames from the savestate
(hud.lua's header has the formats). The savestate is looked up in the session folder, else copied there from the
mgba one.
"""
import shutil
import sys
import time

import mgba_ctl
from recorders import running

SESSION = "hud-logic"


def main(argv: list[str]) -> None:
    state_name, frames = argv[0], int(argv[1])
    name, keys, pokes = (argv[2:] + ["", "", ""])[:3]
    work = mgba_ctl.session_dir(SESSION)
    state = work / f"{state_name}.ss"
    if not state.exists():
        shutil.copyfile(work.parent / "mgba" / f"{state_name}.ss", state)
    name = name or state_name
    (work / "done.txt").unlink(missing_ok=True)
    env = {"NFSGBA_TRACE_STATE": state.as_posix(), "NFSGBA_TRACE_FRAMES": str(frames), "NFSGBA_TRACE_NAME": name,
           "NFSGBA_TRACE_KEYS": keys, "NFSGBA_TRACE_POKES": pokes}
    with running(SESSION, ["hud"], env) as (work, p):
        deadline = time.time() + 120 + frames / 20
        while not (work / "done.txt").exists():
            if time.time() > deadline or p.poll() is not None:
                sys.exit("trace did not finish")
            time.sleep(0.5)
    print(work / f"{name}.trace")
