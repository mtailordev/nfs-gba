"""Record game-frame traces for the game loop (docs/engine/game-loop.md) and store them compactly.

    .venv/Scripts/python.exe tools/record.py game record drive       # starts mGBA with the probe game.lua
    .venv/Scripts/python.exe tools/record.py game pack drive         # NAME.frames.bin -> NAME.base.bin + NAME.delta

`record` loads the scenario's savestate (copy it into the session directory first), arms
the probe game.lua for the scenario's game frames (`racing`: from the first race frame the game loop runs
whole, i.e. after the countdown) and plays its keys. `pack` keeps the first machine state as NAME.base.bin and every later
state as the byte runs that differ from the one before (NAME.delta: per state a u32 run count, then per run u32
offset, u32 length and the bytes), then deletes NAME.frames.bin. A state is EWRAM, IWRAM, palette, VRAM and OAM
(0x40000 + 0x8000 + 0x400 + 0x18000 + 0x400 bytes). Rerunning gives identical files (the emulator is
deterministic from the savestate).
"""
import struct
from pathlib import Path
import time

import numpy as np

import mgba_ctl
from recorders import running

SESSION = "game-loop"

STATE = 0x40000 + 0x8000 + 0x400 + 0x18000 + 0x400
AUTOPILOT = [f"lua {Path(__file__).with_name('autopilot.lua').as_posix()}", "luax AUTOPILOT.reset()"]
SCENARIOS = {
    # name: (savestate, arming, game frames, keys[, commands before recording, e.g. RAM pokes]).
    # From the reference race (race.ss): accelerate, steer both ways, brake, accelerate again.
    "drive": ("race", "", 150, ["wait 2", "hold A 200", "hold A,LEFT 40", "hold A 100", "hold A,RIGHT 40",
                                "hold B 40", "hold A 400"]),
    # From the race-info screen of LONGPOINT (circuitinfo.ss of the ai-traffic session: hard, 3 opponents, heavy
    # traffic): start, then from the first racing frame bump the neighbours on the grid, brake twice (brake smoke),
    # swerve, and keep going with the opponents and traffic around.
    "live": ("circuitinfo", "racing", 700, ["hold A 10", "wait 100", "hold A 200", "hold A,LEFT 25",
                                            "hold A,RIGHT 50", "hold A,LEFT 25", "hold A 300", "hold B 60",
                                            "hold A 300", "hold A,RIGHT 40", "hold A 300", "hold B,LEFT 40",
                                            "hold A 600", "hold A,LEFT 30", "hold A 900"]),
    # From the race-info screen of JUNKPOINT (sprintinfo.ss of the ai-traffic session: normal, 3 opponents, heavy
    # traffic): brake at the start so the opponents pull away (no car-to-car contact), then follow them through the
    # traffic with swerves, braking and handbrake turns.
    "trail": ("sprintinfo", "racing", 700, ["hold A 10", "wait 100", "hold B 250", "hold A 400", "hold A,LEFT 30",
                                             "hold A 300", "hold B 60", "hold A,RIGHT 30", "hold A 400",
                                             "hold B,LEFT 40", "hold A 1500"]),
    # From the reference race: the camera's other branches. SELECT to the bumper view, DOWN alone looks back,
    # SELECT back to the chase view (the reset behind the car), DOWN in the chase view, then L and R held with A
    # (nitro, if the car has it: the focal length's speed effect and the flames).
    "views": ("race", "", 600, ["wait 2", "hold A 200", "hold A,SELECT 6", "hold A 200", "hold DOWN 60",
                                "hold A 150", "hold A,SELECT 6", "hold A 150", "hold DOWN 60", "hold A 100",
                                "hold A,L 120", "hold A,R 120", "hold A,LEFT 60", "hold A 1200"]),
    # From the reference race with nitro in the tank (the Quick Play car has none: before recording, one byte of
    # the player's tank, driver +0x4C8 = 0x0202CAEC, is poked to make it 0x10000; +0x4CC is 0, so it never drains):
    # A+L (nitro in binding set 0) for the camera's speed effect on the focal length and the nitro flames.
    "nitro": ("race", "", 300, ["wait 2", "hold A 200", "hold A,L 150", "hold A 100", "hold A,L 60",
                                "hold A,L,LEFT 40", "hold A,L,RIGHT 40", "hold A 600"], ["poke 0x0202CAEE 1"]),
    # From the race-info screen of JUNKPOINT (sprintinfo.ss): A starts the race; recorded from the main_frame entry
    # of game state 4 (the race start) through the intro, the countdown and GO, with no keys held after the start.
    "start": ("sprintinfo", "start", 90, ["hold A 10", "wait 500"]),
    # From the reference race with the player's entity marked finished (`+0x4A` = 2, what the lap logic does at the
    # last lap): the car handler starts the race end, the palette fades out over 8 frames, phase 3, the state-5 exit
    # (`fill_results`, `race_cleanup`) and its screen change; recorded past it (menu frames, not replayed).
    "over": ("race", "", 16, ["wait 2", "hold A 400"],
             [f"lua {Path(__file__).with_name('finish.lua')}"]),
    # From the reference race: START (the pause block up to `goto_screen(5)`), then the menus.
    "pause": ("race", "", 14, ["wait 2", "hold A 40", "hold START 6", "hold A 200"]),
    # From the reference race with `main_frame`'s palette fade poked (FADE 0x03005630): out (-30: 15 frames, the
    # palettes and the sky gradient lose 4 per channel each), and in (+20: 10 frames, fadein.lua) from black.
    "fadeout": ("race", "", 20, ["hold A 400"], ["luax emu:write32(0x03005630,-30)"]),
    # The other race modes, from in-race savestates (copied into the session folder as *-race.ss) with the autopilot
    # (autopilot.lua) driving: a hunter race (mode 2, HUD life bars) ramming opponent 3; an elimination race
    # (mode 1) on the racing line; a circuit with the wingman (attacker_start: 2 opponents plus the wingman),
    # the wingman command (R+L) given twice; a career event (story3_race).
    "hunter": ("hunter-race", "racing", 500, ["wait 1100"], [*AUTOPILOT, "luax AUTOPILOT.target=3",
                                                            'luax AUTOPILOT.mode="ram"']),
    "elimination": ("elimination-race", "racing", 900, ["wait 2000"], [*AUTOPILOT, 'luax AUTOPILOT.mode="race"']),
    # The same race for 1,745 game frames: past the first lap crossing (a knock-out), before the race ends.
    "elimlap": ("elimination-race", "racing", 1745, ["wait 10"], [*AUTOPILOT, 'luax AUTOPILOT.mode="race"']),
    "attacker": ("attacker-race", "racing", 700, ["wait 400", "luax AUTOPILOT.extra=0x300", "wait 12",
                                                  "luax AUTOPILOT.extra=0", "wait 1200"],
                 [*AUTOPILOT, 'luax AUTOPILOT.mode="race"']),
    "career": ("career-race", "racing", 600, ["wait 1400"], [*AUTOPILOT, 'luax AUTOPILOT.mode="race"']),
    # Round two. A circuit with the DRAFTER wingman (profile +0x200 = 2 poked on the race-info screen wingmaninfo.ss,
    # copied from ai-traffic; the poke is a state, never the ROM), the command (R+L) given after 400 frames.
    "drafter": ("wingmaninfo", "racing", 700, ["hold A 10", "wait 500", "luax AUTOPILOT.extra=0x300", "wait 12",
                                               "luax AUTOPILOT.extra=0", "wait 1200"],
                [*AUTOPILOT, 'luax AUTOPILOT.mode="race"', "luax emu:write32(emu:read32(0x030056EC)+0x200,2)"]),
    # A hunter race and a career event to their finish: the player's entity marked finished (finish.lua, as `over`
    # does), then the race end, the state-5 exit and the hand-over to the results screen.
    "overhunter": ("hunter-race", "", 16, ["wait 2", "hold A 400"], [f"lua {Path(__file__).with_name('finish.lua')}"]),
    "overcareer": ("career-race", "", 16, ["wait 2", "hold A 400"], [f"lua {Path(__file__).with_name('finish.lua')}"]),
    "fadein": ("race", "", 14,["hold A 400"], [f"lua {Path(__file__).with_name('fadein.lua')}"]),
}


def session():
    return mgba_ctl.session_dir(SESSION)


def record(name):
    state, arming, frames, keys, *pre = SCENARIOS[name]
    with running(SESSION, ["game"]) as (work, _):
        mgba_ctl.send(f"load {state}", *(pre[0] if pre else []), session=SESSION)
        (work / "gtrace.tmp").write_text(f"{name} {frames} {arming}\n")
        (work / "gtrace.tmp").replace(work / "gtrace.txt")
        mgba_ctl.send(*keys, session=SESSION)
        log = work / "gtrace_log.txt"
        for _ in range(1200):
            if log.exists() and f"recorded {name}" in log.read_text():
                return
            time.sleep(0.5)
        raise SystemExit(f"{name}: not finished (see {log})")


def pack(name):
    work = session()
    raw = np.fromfile(work / f"{name}.frames.bin", dtype=np.uint8)
    assert raw.size % STATE == 0, "partial state"
    states = raw.reshape(-1, STATE)
    states[0].tofile(work / f"{name}.base.bin")
    with open(work / f"{name}.delta", "wb") as out:
        for prev, cur in zip(states, states[1:]):
            diff = np.flatnonzero(prev != cur)
            runs = []
            if diff.size:
                breaks = np.flatnonzero(np.diff(diff) > 8) + 1
                for part in np.split(diff, breaks):
                    runs.append((int(part[0]), int(part[-1]) + 1))
            out.write(struct.pack("<I", len(runs)))
            for a, b in runs:
                out.write(struct.pack("<II", a, b - a))
                out.write(cur[a:b].tobytes())
    (work / f"{name}.frames.bin").unlink()
    print(f"{name}: {len(states)} states packed")


def main(argv: list[str]) -> None:
    {"record": record, "pack": pack}[argv[0]](argv[1])
