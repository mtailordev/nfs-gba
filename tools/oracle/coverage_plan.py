"""Key plans for `record.py coverage` from the career runs (tools/career/NAME.script.json), so the coverage run plays
the same whole-game routes as the headless traces: a headless mGBA run (tools/retro.py) of the script, recording the
keys held per frame and the race-finish pokes, written as a plan in coverage.lua's syntax (KEY:N, none:N, poke:ADDR=HEX;...).

    .venv/Scripts/python.exe tools/oracle/coverage_plan.py NAME [NAME ...]   # -> work/e5298b24/coverage3/plan-NAME.json

Each mGBA callback after frame n can set the keys of frame n+1, and a step of N frames takes N+1 callbacks, so the
plan is laid out frame by frame (`pos` is the next free callback). Same loop as tools/career_trace.py, without the trace.
"""
import ctypes as C
import json
import pathlib
import shutil
import struct
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent.parent))
import retro  # noqa: E402
from common import data_dir  # noqa: E402
from session_trace import rows  # noqa: E402

ROOT = pathlib.Path(__file__).resolve().parent.parent
FOLDER = "coverage3"


def schedule(name):
    """(frames, held: {frame: key}, pokes: {frame: [(addr, bytes)]}) of the career script, played headless."""
    work = data_dir() / "work" / "e5298b24"
    cfg = json.loads((ROOT / "career" / f"{name}.script.json").read_text())
    session = work / FOLDER / f"session-{name}"
    shutil.rmtree(session, ignore_errors=True)
    session.mkdir(parents=True)
    retro.SESSION = session
    r = retro.Retro(video=False)
    core = r.core
    core.retro_get_memory_size.restype = C.c_size_t
    save = (work / "career" / cfg["save"]).read_bytes()
    C.memmove(core.retro_get_memory_data(0), save, len(save))
    held = {f: key for first, n, key in cfg["boot"] for f in range(first, first + n)}
    pokes, rules, rule, entered, fired, end, prev = {}, cfg["rules"], 0, 0, -99, None, None
    started = poked = False
    frames = 0
    for f in range(cfg["frames"]):
        if end is not None and f >= end:
            break
        r.run(1, keys=(held[f],) if f in held else ())
        frames = f + 1
        s = r.serialize()
        row = rows(s)
        if row["screen"] != 0x81 and row["state"] != 5:
            started = poked = False
        started = started or (row["state"] == 5 and row["count"] < cfg.get("finish_at", 150) and row["fade"] == 0)
        if started and not poked and row["state"] == 5 and row["count"] >= cfg.get("finish_at", 150):
            iw = s[0x19000:0x21000]
            entity = struct.unpack_from("<I", iw, 0xFC)[0] + struct.unpack_from("<I", iw, 0x60)[0] * 0xA4
            writes = [(entity + 0x4A, struct.pack("<H", 2))]
            if cfg.get("win"):
                driver = struct.unpack_from("<I", s, 0x21000 + (entity & 0x3FFFF) + 0x8C)[0]
                writes += [(driver + 0xC5, b"\0"), (driver + 0xAC, struct.pack("<i", 1000000))]
            for addr, data in writes:
                r.write(addr, data)
            pokes[f + 1] = writes  # after frame f+1: mGBA callback f+1
            poked = True
        if row["screen"] != prev:
            prev, entered = row["screen"], f
        if f > cfg["boot_frames"] and rule < len(rules) and row["state"] == 1 and row["fade"] == 0 and row["menu_exit"] == 0:
            screen, wait, key, *opt = rules[rule]
            if opt and row["screen"] != screen and any(x[0] == row["screen"] for x in rules[rule + 1:]):
                rule += 1
                if rule == len(rules):
                    end = f + cfg["tail"] + 13
            elif row["screen"] == screen and f - max(entered, fired + 12) >= wait:
                held.update({g: key for g in range(f + 1, f + 13)})
                rule, fired = rule + 1, f
                if rule == len(rules):
                    end = f + 13 + cfg["tail"]
    return cfg, frames, held, pokes


def plan_text(frames, held, pokes):
    """Runs of one held key (frame, length, key) and pokes laid out on the callbacks; `pos` = next free callback."""
    events = sorted([(f, "key", k) for f, k in held.items() if f < frames] + [(f, "poke", w) for f, w in pokes.items()],
                    key=lambda e: (e[0], e[1] == "key"))
    steps, pos, i = [], 1, 0
    while i < len(events):
        f, kind, val = events[i]
        if f > pos:
            steps.append(f"none:{f - pos - 1}")
            pos = f
        if kind == "poke":
            steps.append("poke:" + ";".join(f"{a:08x}={d.hex()}" for a, d in val))
            pos += 1
            i += 1
        else:
            j = i
            while j + 1 < len(events) and events[j + 1][1:] == (kind, val) and events[j + 1][0] == events[j][0] + 1:
                j += 1
            n = j - i + 1
            steps.append(f"{val}:{n}")
            pos += n + 1
            i = j + 1
    steps.append(f"none:{max(frames - pos, 0) + 60}")  # the tail
    return ",".join(steps)


def main(names):
    out = data_dir() / "work" / "e5298b24" / FOLDER
    out.mkdir(parents=True, exist_ok=True)
    for name in names:
        cfg, frames, held, pokes = schedule(name)
        text = plan_text(frames, held, pokes)
        (out / f"plan-{name}.json").write_text(json.dumps({"save": cfg["save"], "frames": frames, "plan": text}), encoding="utf-8")
        print(name, frames, "frames,", text.count(",") + 1, "steps,", len(pokes), "pokes")


if __name__ == "__main__":
    main(sys.argv[1:])
