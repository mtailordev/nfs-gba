"""The one mGBA launcher: runs the canonical ROM in mGBA (nightly) with tools/mgba_remote.lua.

    python tools/mgba_ctl.py start                     # launch mGBA with the remote script, sound muted
    python tools/mgba_ctl.py "hold START 5" "wait 120" "shot title"
    python tools/mgba_ctl.py stop

From Python: `start(session, probes, env)`, `send(*commands, session=...)`, `stop(session)`. Commands are listed in
mgba_remote.lua. More Lua (the probe modules in tools/recorders/) loads into the remote's environment through
NFSGBA_MGBA_EXTRA (';'-separated paths), the only way to load extra scripts; `start(probes=...)` sets it, and the
remote's `lua FILE` command loads one at run time. Screenshots, dumps, savestates, save games and log.txt all go to
$NFSGBA_DATA/work/<sha1-8>/<session>/, never next to the vault ROM. Session names: NFSGBA_MGBA_SESSION, else the
caller's default: "mgba" here (manual use); each recorder in tools/record.py defaults to its fixture folder.
Several emulators can run at once with different sessions. NFSGBA_MGBA overrides the mGBA executable (default
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


def session_dir(default: str = "mgba") -> Path:
    """The session's work folder: NFSGBA_MGBA_SESSION, else `default`."""
    work = data_dir() / "work" / canonical()[1] / (os.environ.get("NFSGBA_MGBA_SESSION") or default)
    work.mkdir(parents=True, exist_ok=True)
    return work


def start(session: str = "mgba", probes=(), env: dict | None = None) -> subprocess.Popen:
    """Launches mGBA on the ROM with the remote and `probes` (Lua paths) in `session`; returns the process."""
    rom, _ = canonical()
    work = session_dir(session)
    extra = [p for p in os.environ.get("NFSGBA_MGBA_EXTRA", "").split(";") if p] + [str(p) for p in probes]
    cmd = [str(MGBA), "--script", str(ROOT / "tools" / "mgba_remote.lua")]
    for key in ("savegamePath", "savestatePath", "screenshotPath", "patchPath", "cheatsPath"):
        cmd += ["-C", f"{key}={work}"]
    cmd += ["-C", "mute=1", str(rom)]
    p = subprocess.Popen(cmd, cwd=work, env={**os.environ, **(env or {}), "NFSGBA_MGBA_DIR": work.as_posix(),
                                             "NFSGBA_MGBA_EXTRA": ";".join(extra)})
    (work / "pid.txt").write_text(str(p.pid))
    return p


def stop(session: str = "mgba") -> None:
    """Stops the session's own mGBA by its PID (never by image name: other sessions may be running)."""
    pid = session_dir(session) / "pid.txt"
    if pid.exists():
        subprocess.run(["taskkill", "/PID", pid.read_text().strip(), "/T", "/F"], check=False, capture_output=True)


def send(*commands: str, session: str = "mgba", timeout: float = 600) -> None:
    """Runs one batch of remote commands and waits until the remote has finished it."""
    work = session_dir(session)
    batch = str(time.time_ns())
    (work / "cmd.tmp").write_text("\n".join(commands) + f"\nid {batch}\n", encoding="utf-8")
    (work / "cmd.tmp").replace(work / "cmd.txt")
    deadline = time.time() + timeout
    while time.time() < deadline:
        done = work / "done.txt"
        if done.exists() and done.read_text().strip() == batch:
            return
        time.sleep(0.2)
    sys.exit("timeout: is mGBA running? (python tools/mgba_ctl.py start)")


def main(args: list[str]) -> None:
    if args == ["start"]:
        p = start()
        print(f"started pid {p.pid}; output in {session_dir()}")
    elif args == ["stop"]:
        stop()
    else:
        send(*args)


if __name__ == "__main__":
    main(sys.argv[1:])
