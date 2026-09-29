"""The recorder: mGBA with tools/mgba_remote.lua plus probe modules (the .lua files here), driven by tools/record.py.

Probes: `audio` (sound engine per update), `hud` (HUD inputs and OAM per frame), `game` (whole machine per game
frame), `race_init` (states around the race start), `frame` (renderer inputs and the finished page), `coverage`
(function entries), `rules` (race-rule tracer with an autopilot), `autopilot` (deterministic driving), `calls` (call
counts). The remote itself records the car handler (`trace NAME`). Each Python module here drives one recorder;
its session (fixture folder) is `SESSION`.
"""
from contextlib import contextmanager
from pathlib import Path

import mgba_ctl

HERE = Path(__file__).resolve().parent


def probe(name: str) -> Path:
    """A probe module's path (`name` without `.lua`), or an explicit .lua path."""
    p = Path(name) if name.endswith(".lua") else HERE / f"{name}.lua"
    if not p.is_file():
        raise FileNotFoundError(f"no probe {name!r} (tools/recorders/*.lua)")
    return p


@contextmanager
def running(session: str, probes=(), env: dict | None = None):
    """mGBA running in `session` with the remote and `probes`; yields (work folder, process) and stops it after."""
    p = mgba_ctl.start(session, [probe(n) for n in probes], env)
    try:
        yield mgba_ctl.session_dir(session), p
    finally:
        mgba_ctl.stop(session)
