"""Record the LS_Play sound engine from a savestate (probe audio.lua; format in docs/formats/audio.md).

    .venv/Scripts/python.exe tools/record.py audio STATE FRAMES [NAME [KEYS]]
    .venv/Scripts/python.exe tools/record.py audio race 5400                    # -> <work>/audio/race.trace
    .venv/Scripts/python.exe tools/record.py audio race 2400 race-drive "0:2400:A,600:300:LEFT"

KEYS holds buttons as START:LEN:KEY[+KEY],... in frames from the savestate. The savestate is looked up in the
session folder, else copied there from the mgba one.
"""
import shutil
import sys
import time

import mgba_ctl
from recorders import running

SESSION = "audio"


def main(argv: list[str]) -> None:
    state_name, frames = argv[0], int(argv[1])
    name, keys = (argv[2:] + ["", ""])[:2]
    work = mgba_ctl.session_dir(SESSION)
    state = work / f"{state_name}.ss"
    if not state.exists():
        shutil.copyfile(work.parent / "mgba" / f"{state_name}.ss", state)
    name = name or state_name
    (work / "done.txt").unlink(missing_ok=True)
    env = {"NFSGBA_TRACE_STATE": state.as_posix(), "NFSGBA_TRACE_FRAMES": str(frames), "NFSGBA_TRACE_NAME": name,
           "NFSGBA_TRACE_KEYS": keys}
    with running(SESSION, ["audio"], env) as (work, p):
        deadline = time.time() + 120 + frames / 30
        while not (work / "done.txt").exists():
            if time.time() > deadline or p.poll() is not None:
                sys.exit("trace did not finish")
            time.sleep(0.5)
    print(work / f"{name}.trace")
