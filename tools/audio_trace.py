"""Record the LS_Play sound engine from a savestate (tools/mgba_audio_trace.lua; format in docs/formats/audio.md).

    python tools/audio_trace.py STATE FRAMES [NAME [KEYS]]
    python tools/audio_trace.py race 5400                              # -> <work>/audio/race.trace
    python tools/audio_trace.py race 2400 race-drive "0:2400:A,600:300:LEFT"

KEYS holds buttons as START:LEN:KEY[+KEY],... in frames from the savestate. The savestate is looked up in the
audio work folder, else copied there from the mgba one. NFSGBA_MGBA overrides the mGBA executable, as in
mgba_ctl.py. Only the mGBA process started here is stopped.
"""
import os
import shutil
import subprocess
import sys
import time

from common import ROOT
from mgba_ctl import MGBA, canonical


def main(state_name: str, frames: int, name: str = "", keys: str = "") -> None:
    rom, sha8 = canonical()
    work = rom.parent.parent.parent / "work" / sha8 / "audio"
    work.mkdir(parents=True, exist_ok=True)
    state = work / f"{state_name}.ss"
    if not state.exists():
        shutil.copyfile(work.parent / "mgba" / f"{state_name}.ss", state)
    name = name or state_name
    (work / "done.txt").unlink(missing_ok=True)
    env = dict(os.environ, NFSGBA_MGBA_DIR=work.as_posix(), NFSGBA_TRACE_STATE=state.as_posix(),
               NFSGBA_TRACE_FRAMES=str(frames), NFSGBA_TRACE_NAME=name, NFSGBA_TRACE_KEYS=keys)
    cmd = [str(MGBA), "--script", str(ROOT / "tools" / "mgba_audio_trace.lua")]
    for key in ("savegamePath", "savestatePath", "screenshotPath", "patchPath", "cheatsPath"):
        cmd += ["-C", f"{key}={work}"]
    p = subprocess.Popen(cmd + ["-C", "mute=1", str(rom)], cwd=work, env=env)
    try:
        deadline = time.time() + 120 + frames / 30
        while not (work / "done.txt").exists():
            if time.time() > deadline or p.poll() is not None:
                sys.exit("trace did not finish")
            time.sleep(0.5)
    finally:
        subprocess.run(["taskkill", "/PID", str(p.pid), "/T", "/F"], check=False, capture_output=True)
    print(work / f"{name}.trace")


if __name__ == "__main__":
    main(sys.argv[1], int(sys.argv[2]), *sys.argv[3:5])
