"""The one recorder CLI: mGBA with tools/mgba_remote.lua and the probe modules in tools/recorders/.

    .venv/Scripts/python.exe tools/record.py list
    .venv/Scripts/python.exe tools/record.py RECORDER [ARGS...]     # see the recorder's docstring
    .venv/Scripts/python.exe tools/record.py run SCENARIO.json      # an ad-hoc scenario

Recorders (module in tools/recorders/, session = fixture folder): audio (audio), hud (hud-logic), car
(vehicle-physics), ai (ai-traffic), race-init (race-init), game (game-loop), coverage (harness). Each starts its own
mGBA, loads its probes and stops that process when done; NFSGBA_MGBA_SESSION overrides the session.

A scenario is a JSON object: {"session": NAME, "state": SAVESTATE, "probes": [PROBE, ...], "commands": [CMD, ...]}.
`state` is a savestate name in the session folder, or "session/name" to copy one from another session first;
`probes` are tools/recorders/*.lua names (or .lua paths); `commands` are remote commands (tools/mgba_remote.lua),
sent after loading the state. Only `session` and `commands` are required.
"""
import importlib
import json
import shutil
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import mgba_ctl  # noqa: E402
from recorders import probe, running  # noqa: E402

RECORDERS = {"audio": "audio", "hud": "hud", "car": "car", "ai": "ai", "race-init": "race_init", "game": "game",
             "coverage": "coverage"}


def recorder(name: str):
    if name not in RECORDERS:
        sys.exit(f"unknown recorder {name!r}; known: {', '.join(RECORDERS)} (or `run SCENARIO.json`)")
    return importlib.import_module(f"recorders.{RECORDERS[name]}")


def load_scenario(text: str) -> dict:
    """Parses and checks a scenario; probe names resolve to paths (FileNotFoundError for an unknown one)."""
    sc = json.loads(text)
    unknown = set(sc) - {"session", "state", "probes", "commands"}
    if unknown or not isinstance(sc.get("session"), str) or not isinstance(sc.get("commands"), list):
        raise ValueError(f"a scenario needs a session and a commands list (unknown keys: {sorted(unknown)})")
    return {"session": sc["session"], "state": sc.get("state", ""), "commands": [str(c) for c in sc["commands"]],
            "probes": [probe(p) for p in sc.get("probes", [])]}


def run_scenario(sc: dict) -> None:
    with running(sc["session"], sc["probes"]) as (work, _):
        state = sc["state"]
        if "/" in state:
            src, state = state.split("/")
            shutil.copyfile(work.parent / src / f"{state}.ss", work / f"{state}.ss")
        mgba_ctl.send(*([f"load {state}"] if state else []), *sc["commands"], session=sc["session"])
        print(f"scenario done; output in {work}")


def main(argv: list[str]) -> None:
    if not argv or argv[0] == "list":
        print("\n".join(f"{k}: {(recorder(k).__doc__ or '').strip().splitlines()[0]}" for k in RECORDERS))
    elif argv[0] == "run":
        run_scenario(load_scenario(Path(argv[1]).read_text(encoding="utf-8")))
    else:
        recorder(argv[0]).main(argv[1:])


if __name__ == "__main__":
    main(sys.argv[1:])
