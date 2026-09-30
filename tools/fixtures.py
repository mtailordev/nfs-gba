"""The fixture manifest (hard rule 3: provenance): every fixture file the Rust tests read, with its SHA-1 and size;
the ROM it was recorded from is the header line, and the tool, command and source state are per top folder
(`PROVENANCE` below). Paths are relative to $NFSGBA_DATA/work/<sha1-8 of the ROM>/.

    .venv/Scripts/python.exe tools/fixtures.py build [--log FILE] # run the tests (or read a test run's log), write
                                                                  # docs/engine/fixtures.csv
    .venv/Scripts/python.exe tools/fixtures.py check [--log FILE] # every listed file present with its SHA-1; with
                                                                  # --log (NFSGBA_FIXTURE_LOG of a test run), every
                                                                  # fixture the tests read is listed

`build` runs `cargo test --release --workspace` with NFSGBA_REQUIRE_DATA=1 and NFSGBA_FIXTURE_LOG, so the list is
exactly what the tests resolve through `nfsgba_testkit` (a folder fixture lists every file under it).
"""
import argparse
import csv
import hashlib
import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from common import ROOT, data_dir, write_if_changed

MANIFEST = ROOT / "docs" / "engine" / "fixtures.csv"
COLUMNS = ["path", "sha1", "size"]

# Top folder -> (recorder, command, source state). Recorded per session; details in the named doc.
PROVENANCE = {
    "career": ("tools/career_trace.py (tools/retro.py, headless mGBA); saves: cargo run -p nfsgba-game --example career_save NAME", "career_trace.py NAME (script tools/career/NAME.script.json); base.sav = race-rules/rr-career.sav", "career runs from a chosen save: boot presses, then a press per settled screen; the player marked finished (a win with all laps counted) in the race; per frame the profile, the shown page, palettes and OAM (docs/engine/game-loop.md, The whole game)"),
    "session": ("tools/session_trace.py, tools/menu_audio_trace.py (tools/retro.py, headless mGBA)", "session_trace.py, menu_audio_trace.py", "power-on to a Quick Play race and its results with the key script in the file, the mode poked to a circuit and the player marked finished (docs/engine/game-loop.md, Session)"),
    "boot": ("tools/boot_trace.py (tools/retro.py, headless mGBA)", "boot_trace.py fresh; boot_trace.py existing race-rules/rr-career.sav", "power-on runs of the canonical ROM with the key script in each file (docs/formats/ui.md, Menus)"),
    "mgba": ("tools/mgba_ctl.py + tools/mgba_remote.lua", "mgba_ctl.py dump/save", "race.ss: Quick Play route 23 (docs/TOOLS.md route)"),
    "audio": ("tools/record.py audio (recorders/audio.py + audio.lua)", "record.py audio <state> <frames> [<name> <keys>]", "race.ss, mainmenu.ss (docs/formats/audio.md)"),
    "hud-logic": ("tools/record.py hud (recorders/hud.py + hud.lua)", "record.py hud <state> <frames> [<name> <keys> <pokes>]", "Quick Play races per mode (docs/formats/ui.md, HUD logic)"),
    "menus": ("tools/oracle/cases.py menus (function oracle)", "cases.py menus <set>; unlock.jsonl: cases.py unlock", "menu snapshots named in each case (docs/formats/ui.md, Menus)"),
    "menus3": ("tools/oracle/cases.py menus, draw, scene, save, garage (function oracle); power-on, garage_capture (headless mGBA, tools/retro.py)", "cases.py menus all; cases.py draw; cases.py scene; cases.py power-on; save.jsonl: cases.py save; garage.jsonl, mapdraw.jsonl: cases.py garage; gar-*: oracle/garage_capture.py", "menu snapshots named in each case (docs/formats/ui.md, Menus, continued); po-*: a power-on run to the main menu; gar-*: rr-career.sav into the garage screens"),
    "reach": ("tools/oracle/cases.py upgrades (function oracle)", "cases.py upgrades", "upgrades.jsonl: the game's upgrades_changed on generated garage states over the menus3 snapshots (ui-2d, race-rules)"),
    "menus4": ("tools/oracle/scene_frames.py (headless mGBA, tools/retro.py)", "cases.py power-frames", "pf-*: the power-on run to the name screen, 24 consecutive frames of screen 48 (health, colour 4 blinking) and 60 of the name keyboard (scripted cursor moves, two letters, a delete) (docs/FIDELITY.md U7)"),
    "garage2": ("tools/oracle/cases.py turntable (function oracle); tools/oracle/garage_turntable.py (headless mGBA, tools/retro.py)", "cases.py turntable; garage_turntable.py", "turntable.jsonl: the game's garage_load_car_atlas, _palette and garage_draw_car on the menus3/gar-* snapshots with generated cars; tt-*: rr-career.sav into the career profile screen, 30 consecutive frames"),
    "ui-2d": ("tools/mgba_ctl.py + tools/mgba_remote.lua", "mgba_ctl.py dump <name>", "fresh save, intro screens (docs/formats/ui.md)"),
    "car-paint": ("tools/mgba_ctl.py + tools/mgba_remote.lua", "mgba_ctl.py dump <name>", "race.ss, driving left (docs/formats/car-paint.md)"),
    "car-atlas": ("mGBA session car-atlas, trace scripts in the folder", "see docs/formats/car-paint.md (car-atlas)", "race starts from race.ss and mainmenu.ss, RAM-poked records"),
    "sky": ("probe scripts in the folder's scripts/", "see docs/engine/sky.md, Verification", "race.ss, both camera views"),
    "entity-draw": ("tools/recorders/frame.lua (probe)", "probe <name> [ADDR=VALUE ...] (probe.txt)", "race.ss, g0.ss (docs/engine/renderer.md, entity-draw)"),
    "race-rules": ("tools/recorders/rules.lua (probe) + tools/oracle/cases.py rules", "see docs/formats/career.md, Race-rule checks", "Quick Play and career races (savestates in the folder)"),
    "vehicle-physics": ("tools/record.py car + tools/oracle/cases.py car (fuzz, calls, suspension: cases.py fuzz / calls / suspension)", "record.py car <name>; cases.py car <name>", "race.ss and the scenario states in docs/engine/physics.md"),
    "ai-traffic": ("tools/record.py ai + tools/oracle/cases.py ai", "record.py ai <name>; cases.py ai <name>", "Quick Play races (docs/engine/ai.md)"),
    "race-init": ("tools/record.py race-init (recorders/race_init.py + race_init.lua), tools/oracle/cases.py race-init", "record.py race-init <name>; cases.py race-init <name>", "menu states before each race start (docs/engine/race-init.md)"),
    "game-loop": ("tools/record.py game (recorders/game.py + game.lua), tools/oracle/cases.py game-edges / game-countdown / game-end", "record.py game record <name>; record.py game pack <name>; cases.py game-edges; cases.py game-countdown; cases.py game-end", "race.ss (docs/engine/game-loop.md)"),
    "live-race": ("tools/record.py game (recorders/game.py + game.lua)", "record.py game record <name>; record.py game pack <name>", "race starts and race.ss (docs/engine/game-loop.md, live-race)"),
    "coverage2": ("tools/oracle/cases.py rules career_payout (the game's own career_race_payout on generated profiles)", "cases.py rules career_payout --n 1500", "the career payout cases of menu/results.rs"),
    "live-race2": ("tools/record.py game (recorders/game.py + game.lua), NFSGBA_MGBA_SESSION=live-race2", "record.py game record <name>; record.py game pack <name> (drafter, overhunter, overcareer)", "wingmaninfo.ss, hunter-race.ss, career-race.ss (copies of the ai-traffic and live-race states)"),
    "harness": ("tools/oracle/cases.py prove (function oracle)", "cases.py prove", "mgba/race dumps"),
}


def work_dir():
    manifest = json.loads((data_dir() / "vault" / "manifest.json").read_text(encoding="utf-8"))
    sha1 = manifest["canonical_target"]
    return data_dir() / "work" / sha1[:8], sha1


def sha1_of(path: Path) -> str:
    h = hashlib.sha1()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


# A recording session's own bookkeeping (mgba_remote.lua, the recorders): rewritten by every recording, never test data.
SESSION_FILES = {"done.txt", "pid.txt", "log.txt", "gtrace_log.txt", "cmd.txt", "probe.txt", "probe.tmp"}


def files_of(work: Path, rels) -> list[str]:
    """Logged fixture paths, folders expanded to the files under them (without a session's bookkeeping files)."""
    out = set()
    for rel in rels:
        p = work / rel
        if p.is_dir():
            out.update(f.relative_to(work).as_posix() for f in p.rglob("*") if f.is_file() and f.name not in SESSION_FILES)
        elif p.is_file():
            out.add(Path(rel).as_posix())
    return sorted(out)


def rows_for(work: Path, files: list[str]) -> list[dict]:
    rows = []
    for rel in files:
        top = rel.split("/")[0]
        if top not in PROVENANCE:
            sys.exit(f"no provenance for fixture folder {top!r}: add it to PROVENANCE in tools/fixtures.py")
        p = work / rel
        rows.append({"path": rel, "sha1": sha1_of(p), "size": p.stat().st_size})
    return rows


HEADER = "# rom_sha1="


def render(rom_sha1: str, rows: list[dict]) -> str:
    out = io.StringIO()
    out.write(f"{HEADER}{rom_sha1}; provenance per top folder: PROVENANCE in tools/fixtures.py" + chr(10))
    w = csv.DictWriter(out, COLUMNS, lineterminator=chr(10))
    w.writeheader()
    w.writerows(rows)
    return out.getvalue()


def read() -> tuple[str, list[dict]]:
    """(the ROM the fixtures were recorded from, the rows)."""
    lines = MANIFEST.read_text(encoding="utf-8").splitlines()
    return lines[0][len(HEADER):].split(";")[0], list(csv.DictReader(lines[1:]))


def cargo() -> str:
    return shutil.which("cargo") or str(Path.home() / ".cargo" / "bin" / "cargo")


def build(log: Path | None = None) -> int:
    """With `log` (NFSGBA_FIXTURE_LOG of a finished test run, e.g. the gate's), no second test run."""
    work, rom_sha1 = work_dir()
    if log:
        rels = set(log.read_text(encoding="utf-8").split())
    else:
        with tempfile.TemporaryDirectory() as tmp:
            run_log = Path(tmp) / "fixtures.log"
            env = dict(os.environ, NFSGBA_REQUIRE_DATA="1", NFSGBA_FIXTURE_LOG=str(run_log))
            subprocess.run([cargo(), "test", "--release", "--workspace", "-q"], cwd=ROOT, env=env, check=True)
            rels = set(run_log.read_text(encoding="utf-8").split())
    rows = rows_for(work, files_of(work, rels))
    changed = write_if_changed(MANIFEST, render(rom_sha1, rows))
    print(f"{len(rows)} fixtures, {sum(r['size'] for r in rows) / 1e6:.0f} MB; "
          f"{'wrote' if changed else 'unchanged'} {MANIFEST.relative_to(ROOT)}")
    return 0


def check(log: Path | None) -> int:
    work, rom_sha1 = work_dir()
    recorded, rows = read()
    bad = [] if recorded == rom_sha1 else [f"manifest: recorded from ROM {recorded[:8]}, the vault's is {rom_sha1[:8]}"]
    for r in rows:
        p = work / r["path"]
        if r["path"].split("/")[0] not in PROVENANCE:
            bad.append(f"{r['path']}: no provenance for its folder (PROVENANCE in tools/fixtures.py)")
        elif not p.is_file():
            bad.append(f"{r['path']}: missing")
        elif sha1_of(p) != r["sha1"]:
            bad.append(f"{r['path']}: changed (SHA-1 differs)")
    if log:
        listed = {r["path"] for r in rows}
        extra = [f for f in files_of(work, set(log.read_text(encoding="utf-8").split())) if f not in listed]
        bad += [f"{f}: read by the tests but not in the manifest (run tools/fixtures.py build)" for f in extra]
    for b in bad:
        print(f"FIXTURE  {b}")
    print(f"fixtures: {len(rows)} listed, {len(bad)} problems")
    return 1 if bad else 0


def main(argv) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    b = sub.add_parser("build")
    b.add_argument("--log", type=Path)
    c = sub.add_parser("check")
    c.add_argument("--log", type=Path)
    args = ap.parse_args(argv)
    return build(args.log) if args.cmd == "build" else check(args.log)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
