"""Which of the decompiled functions run in real play: mGBA with a breakpoint on every function entry
(probe coverage.lua), one emulator run per scenario, counts per scenario.

    .venv/Scripts/python.exe tools/record.py coverage [SCENARIO ...]     # default: all scenarios below
    -> $NFSGBA_DATA/out/coverage/<sha8>/coverage.csv and summary.txt

Functions come from the Ghidra export (`// ==== <addr> <name>` headers in carbon_decomp.c). Scenarios are a
savestate (or power-on) plus a key plan; screenshots of each step's end land in the session dir to check the
plan did what it says. Stops only its own mGBA (by PID).
"""
import csv
import json
import os
import re
import shutil
import sys
import time
from collections import defaultdict

import mgba_ctl
from common import data_dir, provenance, write_if_changed
from mgba_ctl import canonical
from recorders import running

SESSION = "harness"

# Label, plan (coverage.lua syntax). Menus need presses of at least 10 frames (docs/TOOLS.md).
PRESS = lambda k, wait=60: f"{k}:10,none:{wait}"  # noqa: E731
SCENARIOS = {
    # Power-on with a fresh save: language, intro and warning screens, the title (START only), then a new profile
    # named "A" (DOWN, A, START), to the main menu.
    "boot": "mark:boot,none:400," + ",".join([PRESS(k, 150) for k in ("A", "A", "A", "START", "A", "A", "START", "A")]
                                             + [PRESS("START", 150), "shot:cov-boot-title", PRESS("DOWN", 30),
                                                PRESS("A", 30), PRESS("START", 300)]) + ",shot:cov-boot",
    # Main menu -> Quick Play -> Random -> race start and countdown.
    "menu_to_race": "load:mainmenu,mark:menu_to_race," + ",".join([PRESS("A"), PRESS("A"), PRESS("A"),
                                                                    PRESS("A", 900)]) + ",shot:cov-race-start",
    # Driving from the reference race state: accelerate, steer both ways, brake.
    "drive": "load:race,mark:drive,A:240,A+LEFT:90,A+RIGHT:90,A:120,B:60,shot:cov-drive",
    # Pause in the race (CONTINUE | X), pick X to quit, confirm, back to the menus.
    "pause_quit": "load:race,A:60,mark:pause_quit," + ",".join([PRESS("START", 60), "shot:cov-pause",
                                                                PRESS("RIGHT", 30), "shot:cov-pause-x", PRESS("A", 90),
                                                                "shot:cov-pause-confirm", PRESS("A", 300)])
                  + ",shot:cov-pause-quit",
}


def career_plan(name):
    """A plan file of tools/oracle/coverage_plan.py (`plan-NAME.json` in the session folder): (plan, save) or None."""
    f = mgba_ctl.session_dir(SESSION) / f"plan-{name}.json"
    if not f.is_file():
        return None
    d = json.loads(f.read_text(encoding="utf-8"))
    # `mark` takes one frame: the plan's first wait (it starts with one) is a frame shorter.
    plan = f"mark:{name}," + re.sub(r"^none:(\d+)", lambda m: f"none:{int(m[1]) - 1}", d["plan"])
    return plan, data_dir() / "work" / canonical()[1] / "career" / d["save"]


def functions():
    text = (data_dir() / "work" / canonical()[1] / "ghidra" / "carbon_decomp.c").read_text(encoding="utf-8")
    return [(int(a, 16), n) for a, n in re.findall(r"^// ==== ([0-9a-f]{8}) (\S+)", text, re.M)]


def kind(addr):
    """Where a Ghidra 'function' sits: code, or data it mistook for code."""
    if 0x0300_0000 <= addr < 0x0300_8000:
        return "iwram"
    if 0x0812_A84C <= addr < 0x0816_C244 or 0x0800_00C0 <= addr < 0x0800_0200:
        return "rom"
    return "data?"


def run(label, plan, work, funcs_file, timeout=900):
    out = work / f"cov-{label}.csv"
    for f in (out, work / "done.txt"):
        f.unlink(missing_ok=True)
    env = {"COV_PLAN": plan, "COV_FUNCS": str(funcs_file), "COV_OUT": str(out)}
    t = time.time()
    with running(SESSION, ["coverage"], env) as (_, p):
        while not (work / "done.txt").exists():
            if time.time() - t > timeout or p.poll() is not None:
                raise RuntimeError(f"{label}: no result after {time.time() - t:.0f} s")
            time.sleep(0.5)
    print(f"{label}: {time.time() - t:.0f} s")
    hits = defaultdict(int)
    for row in csv.reader(out.open(encoding="utf-8")):
        if row[0] == label:
            hits[int(row[1], 16)] += int(row[2])
    return hits


def main(names):
    rom, sha8 = canonical()
    work = mgba_ctl.session_dir(SESSION)
    for ss in ("mainmenu.ss", "race.ss"):  # the reference savestates, copied so the session dir is self-contained
        shutil.copy2(data_dir() / "work" / sha8 / "mgba" / ss, work / ss)
    (work / f"{rom.stem}.sav").unlink(missing_ok=True)  # "boot" starts from a fresh save
    funcs = functions()
    funcs_file = work / "cov-functions.txt"
    funcs_file.write_text("".join(f"{a:08x}\n" for a, _ in funcs))
    results = {}
    for name in names or SCENARIOS:
        plan, save = SCENARIOS.get(name), None
        if plan is None:
            plan, save = career_plan(name) or sys.exit(f"unknown scenario {name!r}: not in SCENARIOS, no plan-{name}.json")
        for _ in range(20):  # the previous mGBA may still hold the save for a moment
            try:
                (work / f"{rom.stem}.sav").unlink(missing_ok=True)
                break
            except PermissionError:
                time.sleep(1)
        if save:  # a career save: power-on with it (mGBA writes it back: the copy in the session folder)
            shutil.copy2(save, work / f"{rom.stem}.sav")
        results[name] = run(name, plan, work, funcs_file)

    # NFSGBA_MGBA_SESSION (a new fixture folder) keeps the results there; else the shared data/out/coverage.
    out = work if os.environ.get("NFSGBA_MGBA_SESSION") else data_dir() / "out" / "coverage" / sha8
    rows = [["address", "name", "kind", *results]]
    rows += [[f"{a:#010x}", n, kind(a), *(results[s].get(a, 0) for s in results)] for a, n in funcs]
    lines = [",".join(map(str, r)) for r in rows]
    write_if_changed(out / "coverage.csv", "\n".join(lines) + "\n")

    ran = {a for r in results.values() for a in r}
    named = {a for a, n in funcs if not n.startswith("FUN_")}
    summary = [f"provenance: {provenance(__file__)}", f"functions: {len(funcs)} ({len(named)} named)",
               f"ran in any scenario: {len(ran)} ({len(ran & named)} named, {len(ran - named)} unnamed)",
               f"never ran: {len(funcs) - len(ran)}",
               "data regions mistaken for code: " + ", ".join(f"{a:#010x} {n}" for a, n in funcs if kind(a) == "data?")]
    for s, hits in results.items():
        unnamed = sorted(((c, a) for a, c in hits.items() if a not in named), reverse=True)[:15]
        summary.append(f"{s}: {len(hits)} functions ran ({len(set(hits) & named)} named); top unnamed: "
                       + ", ".join(f"{a:#010x}×{c}" for c, a in unnamed))
    write_if_changed(out / "summary.txt", "\n".join(summary) + "\n")
    print("\n".join(summary))


if __name__ == "__main__":
    main(sys.argv[1:])
