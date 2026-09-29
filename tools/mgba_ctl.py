"""Drive the canonical ROM in mGBA (nightly) through tools/mgba_remote.lua.

    python tools/mgba_ctl.py start                     # launch mGBA with the remote script, sound muted
    python tools/mgba_ctl.py "hold START 5" "wait 120" "shot title"
    python tools/mgba_ctl.py stop

Commands are listed in mgba_remote.lua. Screenshots, dumps, savestates, save games and log.txt all go to
$NFSGBA_DATA/work/<sha1-8>/<session>/, never next to the vault ROM. Several emulators can run at once with
different sessions: NFSGBA_MGBA_SESSION (default "mgba"). NFSGBA_MGBA overrides the mGBA executable (default
ext/mgba-dev/mGBA.exe in this checkout; git worktrees have no ext/, so point it at the main checkout's).
"""
import json
import os
import subprocess
import sys
import time
from pathlib import Path

from common import ROOT, data_dir

MGBA = Path(os.environ.get("NFSGBA_MGBA") or ROOT / "ext" / "mgba-dev" / "mGBA.exe")


def canonical():
    m = json.loads((data_dir() / "vault" / "manifest.json").read_text(encoding="utf-8"))
    r = next(r for r in m["roms"] if r["sha1"] == m["canonical_target"])
    return data_dir() / r["vault_file"], r["sha1"][:8]


def main(args: list[str]) -> None:
    rom, sha8 = canonical()
    work = data_dir() / "work" / sha8 / (os.environ.get("NFSGBA_MGBA_SESSION") or "mgba")
    work.mkdir(parents=True, exist_ok=True)
    if args == ["start"]:
        cmd = [str(MGBA), "--script", str(ROOT / "tools" / "mgba_remote.lua")]
        for key in ("savegamePath", "savestatePath", "screenshotPath", "patchPath", "cheatsPath"):
            cmd += ["-C", f"{key}={work}"]
        cmd += ["-C", "mute=1", str(rom)]
        p = subprocess.Popen(cmd, cwd=work, env=dict(os.environ, NFSGBA_MGBA_DIR=work.as_posix()))
        (work / "pid.txt").write_text(str(p.pid))
        print(f"started pid {p.pid}; output in {work}")
    elif args == ["stop"]:
        subprocess.run(["taskkill", "/PID", (work / "pid.txt").read_text().strip(), "/T", "/F"], check=False)
    else:
        batch = str(time.time_ns())
        (work / "cmd.tmp").write_text("\n".join(args) + f"\nid {batch}\n", encoding="utf-8")
        (work / "cmd.tmp").replace(work / "cmd.txt")
        deadline = time.time() + 600
        while time.time() < deadline:
            done = work / "done.txt"
            if done.exists() and done.read_text().strip() == batch:
                return
            time.sleep(0.2)
        sys.exit("timeout: is mGBA running? (python tools/mgba_ctl.py start)")


if __name__ == "__main__":
    main(sys.argv[1:])
