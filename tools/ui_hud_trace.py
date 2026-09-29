"""Record the race HUD from a savestate (tools/ui_hud_trace.lua; format in docs/formats/ui.md, "HUD logic").

    python tools/ui_hud_trace.py STATE FRAMES [NAME [KEYS [POKES]]]
    python tools/ui_hud_trace.py race 600 circuit "0:600:A,200:60:LEFT"   # -> <work>/hud-logic/circuit.trace
    python tools/ui_hud_trace.py race 300 units "0:300:A" "0:300:0x03000040=1,100:50:D0+0xA8=1"

KEYS holds buttons as START:LEN:KEY[+KEY],... in frames from the savestate. POKES sets HUD inputs just before
hud_update, as START:LEN:TARGET[/SIZE]=VALUE,... (TARGET an address, E<i>+off for entity i, D<i>+off for its
driver; SIZE 1, 2 or 4), so every input the port can see is exercised. The savestate is looked up in the
hud-logic work folder, else copied there from the mgba one. NFSGBA_MGBA overrides the mGBA executable, as in
mgba_ctl.py. Only the mGBA process started here is stopped.
"""
import os
import shutil
import subprocess
import sys
import time

from common import ROOT
from mgba_ctl import MGBA, canonical


def main(state_name: str, frames: int, name: str = "", keys: str = "", pokes: str = "") -> None:
    rom, sha8 = canonical()
    work = rom.parent.parent.parent / "work" / sha8 / "hud-logic"
    work.mkdir(parents=True, exist_ok=True)
    state = work / f"{state_name}.ss"
    if not state.exists():
        shutil.copyfile(work.parent / "mgba" / f"{state_name}.ss", state)
    name = name or state_name
    (work / "done.txt").unlink(missing_ok=True)
    env = dict(os.environ, NFSGBA_MGBA_DIR=work.as_posix(), NFSGBA_TRACE_STATE=state.as_posix(),
               NFSGBA_TRACE_FRAMES=str(frames), NFSGBA_TRACE_NAME=name, NFSGBA_TRACE_KEYS=keys,
               NFSGBA_TRACE_POKES=pokes)
    cmd = [str(MGBA), "--script", str(ROOT / "tools" / "ui_hud_trace.lua")]
    for key in ("savegamePath", "savestatePath", "screenshotPath", "patchPath", "cheatsPath"):
        cmd += ["-C", f"{key}={work}"]
    p = subprocess.Popen(cmd + ["-C", "mute=1", str(rom)], cwd=work, env=env)
    try:
        deadline = time.time() + 120 + frames / 20
        while not (work / "done.txt").exists():
            if time.time() > deadline or p.poll() is not None:
                sys.exit("trace did not finish")
            time.sleep(0.5)
    finally:
        subprocess.run(["taskkill", "/PID", str(p.pid), "/T", "/F"], check=False, capture_output=True)
    print(work / f"{name}.trace")


if __name__ == "__main__":
    main(sys.argv[1], int(sys.argv[2]), *sys.argv[3:6])
