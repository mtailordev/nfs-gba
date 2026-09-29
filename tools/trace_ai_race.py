"""Record race traces for the opponents and traffic (docs/engine/ai.md), in the ai-traffic session.

    python tools/trace_ai_race.py                  # record every scenario (mGBA must run: mgba_ctl.py start)
    python tools/trace_ai_race.py sprint

Same trace format as tools/trace_race.py (the full RAM at every call of the player's car handler, i.e. at the
start of every game frame's entity loop); tools/trace_ai_oracle.py then replays each frame's loop. The scenarios
start from race-info savestates in the session directory, made through Quick Play > CUSTOM:
  sprintinfo.ss   JUNKPOINT sprint, 1 lap, normal, 3 opponents, heavy traffic, catch-up on, no wingman
  circuitinfo.ss  LONGPOINT circuit forward, 2 laps, hard, 3 opponents, heavy traffic, catch-up on, no wingman
The player holds A with a few swerves and a brake, so the opponents pass it and traffic appears around it.
Reruns give identical files (the emulator is deterministic from the savestate).
"""
import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import trace_race  # noqa: E402

DRIVE = ["hold A 400", "hold A,LEFT 30", "hold A 300", "hold A,RIGHT 30", "hold A 300", "hold B 60", "hold A 400"]
SCENARIOS = {
    "sprint": ["load sprintinfo", "trace", "hold A 10", "wait 100", *DRIVE],
    "circuit": ["load circuitinfo", "trace", "hold A 10", "wait 100", *DRIVE, *DRIVE],
}

if __name__ == "__main__":
    os.environ.setdefault("NFSGBA_MGBA_SESSION", "ai-traffic")
    names = sys.argv[1:] or list(SCENARIOS)
    unknown = [n for n in names if n not in SCENARIOS]
    if unknown:
        sys.exit(f"unknown scenarios {unknown}; known: {list(SCENARIOS)}")
    trace_race.SCENARIOS.update(SCENARIOS)
    trace_race.run(names)
