"""Race-rule oracle cases (docs/formats/career.md, "Race-rule checks"): runs the game's race-rule functions in the
function oracle (tools/oracle) on generated inputs over RAM snapshots, and writes
$NFSGBA_DATA/work/<sha8>/race-rules/oracle-<name>.jsonl. Each line has the keys of a tools/trace_race_rules.lua
line (string values), so the career tests replay traces and oracle cases alike.

    .venv/Scripts/python.exe tools/oracle_race_rules.py [NAME ...] [--n 2000] [--seed 1]
"""
import argparse
import json
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent / "oracle"))
from oracle import Gba, canonical, data_dir  # noqa: E402

WORLD, ENTITIES, PROFILE_PTR, RECORDS_PTR = 0x030000C0, 0x030000FC, 0x030056EC, 0x0300539C
# (key, entity or driver, offset, format) in the order of the tracer's racer string.
RACER = [
    ("e", 0x00, "H"), ("e", 0x72, "H"), ("e", 0x90, "h"), ("e", 0x4A, "H"), ("e", 0x08, "H"), ("e", 0x0C, "i"),
    ("e", 0x14, "i"), ("d", 0xA8, "i"), ("d", 0xAC, "i"), ("d", 0xB4, "I"), ("d", 0xB8, "I"), ("d", 0xBC, "I"),
    ("d", 0xC5, "b"), ("d", 0x4D8, "H"), ("d", 0x4E8, "i"), ("d", 0x4EC, "h"), ("d", 0x4EE, "h"), ("d", 0x4F0, "h"),
    ("d", 0xF8, "I"), ("d", 0xFC, "I"), ("d", 0x100, "I"), ("d", 0x444, "i"), ("d", 0x4D6, "H"),
]
GLOBALS = [  # mode lapped laps opponents time finished view player difficulty state48 rand ww_flag route
    (0x030056E0, "I"), (0x0300608C, "I"), (0x030056E4, "i"), (0x03005784, "I"), (0x03005800, "I"),
    (0x030061A4, "I"), (0x030057F8, "I"), (0x03000060, "I"), (0x03005608, "I"), (0x03000048, "I"),
    (0x030064C8, "I"), (0x03005384, "I"), (0x03005720, "I"),
]
TUNING = [0x030061B0, 0x030061B4, 0x030061B8, 0x030061BC, 0x030061C0, 0x030061D0, 0x030061E0, 0x030061F4,
          0x030061A8, 0x0300617C, 0x03006184, 0x030061A0]
FN = {
    "style_rating": 0x0812C30C, "career_race_payout": 0x0812EFE8, "rebuild_unlocks": 0x08135958,
    "race_progress": 0x081400EC, "lap_crossing": 0x0813F098, "update_places": 0x0813EA04,
    "track_player": 0x0813EDD8, "hunter_life_tick": 0x08140F78, "hunter_hit": 0x0814101C,
    "hunter_drain_a": 0x0814136C, "hunter_drain_b": 0x081413B0, "finish_estimate": 0x0814F050,
    "rank_results": 0x0812E8E4, "save_encode": 0x081492C0, "hunter_tuning_init": 0x081412EC,
}


class Mem:
    """A read function over some memory: the snapshot plus patches, or live unicorn memory in a stub."""

    def __init__(self, read):
        self.read = read

    def get(self, addr, fmt):
        return struct.unpack("<" + fmt, self.read(addr, struct.calcsize(fmt)))[0]

    def hex(self, addr, n):
        return self.read(addr, n).hex()

    def racer(self, e):
        d = self.get(e + 0x8C, "I")
        return ",".join(str(self.get((e if w == "e" else d) + o, f)) for w, o, f in RACER)

    def globals(self):
        return ",".join(str(self.get(a, f)) for a, f in GLOBALS)


def view(gba, patches):
    def read(addr, n):
        out = bytearray(gba.read_base(addr, n))
        for a, b in patches:
            lo, hi = max(a, addr), min(a + len(b), addr + n)
            if lo < hi:
                out[lo - addr:hi - addr] = b[lo - a:hi - a]
        return bytes(out)
    return Mem(read)


def pack(fmt, v):
    return struct.pack("<" + fmt, v)


class Case:
    """Inputs of one call: memory patches, built field by field."""

    def __init__(self, gba):
        self.gba, self.mem = gba, []
        self.base = view(gba, [])

    def put(self, addr, fmt, v):
        self.mem.append((addr, pack(fmt, v)))

    def raw(self, addr, data):
        self.mem.append((addr, bytes(data)))

    def entity(self, i):
        return self.base.get(ENTITIES, "I") + 0xA4 * i

    def field(self, i, key_offset, fmt, v, driver=True):
        e = self.entity(i)
        self.put((self.base.get(e + 0x8C, "I") if driver else e) + key_offset, fmt, v)

    def run(self, fn, **kw):
        r = self.gba.call(fn, mem=self.mem, **kw)
        assert r.stop == "return", (fn, r.stop)
        return view(self.gba, self.mem), view(self.gba, self.mem + list(r.writes)), r


def racers_with_drivers(base, n_max=8):
    out = []
    for i in range(n_max):
        e = base.get(ENTITIES, "I") + 0xA4 * i
        d = base.get(e + 0x8C, "I")
        if 0x02000000 <= d < 0x02040000:
            out.append(i)
    return out


def race_block(m, tag, upto):
    parts = [f"{tag}.g={m.globals()}", f"{tag}.res={m.hex(0x03005650, 0x40)}"]
    n = max(m.get(0x03005784, "I"), upto) + 1
    for i in range(n):
        parts.append(f"{tag}.c{i}={m.racer(m.get(ENTITIES, 'I') + 0xA4 * i)}")
    return dict(p.split("=", 1) for p in parts)


# --- generators: each returns a list of case dicts ------------------------------------------------------------------
def gen_style_rating(gba, rng, n):
    p, recs = gba_u32(gba, PROFILE_PTR), gba_u32(gba, RECORDS_PTR)
    out = []
    for k in range(n):
        c = Case(gba)
        car = rng.randrange(15)
        rec = bytes(rng.randrange(256) for _ in range(17)) if k % 4 == 0 else bytes(
            [rng.randrange(21), rng.randrange(9), rng.randrange(17), rng.randrange(12), rng.randrange(64),
             rng.randrange(4)] + [rng.randrange(128) for _ in range(11)])
        c.put(p + 0x10, "b", car)
        c.raw(recs + 17 * car, rec)
        _, _, r = c.run(FN["style_rating"])
        out.append({"car": car, "record": rec.hex(), "ret": s32(r.regs["r0"])})
    return out


def gba_u32(gba, addr):
    return struct.unpack("<I", gba.read_base(addr, 4))[0]


def s32(v):
    return v - (1 << 32) if v & 0x8000_0000 else v


def random_statuses(rng, done_bias):
    """18 bytes of 2-bit event statuses (1 won, 2 second, 3 not done; 0 never written by the game)."""
    return bytes(sum((rng.choice([1, 2] * done_bias + [3, 3, 0]) << (2 * j)) for j in range(4)) for _ in range(18))


def gen_rebuild_unlocks(gba, rng, n):
    p = gba_u32(gba, PROFILE_PTR)
    out = []
    for k in range(n):
        c = Case(gba)
        ev = random_statuses(rng, rng.randrange(1, 6))
        f1f8 = rng.randrange(4)
        flags = [rng.randrange(2) if k % 3 else rng.getrandbits(32) for _ in range(6)]
        c.raw(p + 0x205, ev)
        c.put(p + 0x1F8, "B", f1f8)
        for o, v in zip([0x47C, 0x480, 0x484, 0x48C, 0x488, 0x478], flags):
            c.put(p + o, "I", v)
        _, post, _ = c.run(FN["rebuild_unlocks"])
        out.append({"events": ev.hex(), "f1f8": f1f8, "flags": ",".join(map(str, flags)),
                    "unlocks": post.hex(p + 0x42D, 40)})
    return out


def set_racer(c, i, rng, count, laps, sections=1):
    c.field(i, 0x72, "H", 0 if rng.random() < 0.85 else rng.randrange(sections), driver=False)
    c.field(i, 0x90, "h", rng.choice([0, count - 1, count - 2, rng.randrange(-1, count + 1)]), driver=False)
    c.field(i, 0x4A, "H", rng.choice([1, 1, 1, 2, 256]), driver=False)
    c.field(i, 0x08, "H", rng.getrandbits(16), driver=False)
    c.field(i, 0xC5, "b", rng.randrange(-1, laps + 1))
    c.field(i, 0x4D8, "H", rng.getrandbits(4) | (rng.getrandbits(16) if rng.random() < 0.1 else 0))
    c.field(i, 0xAC, "i", rng.randrange(-5000, 200000))
    t0 = rng.randrange(0, 40000)
    c.field(i, 0xB8, "I", t0)
    c.field(i, 0xB4, "I", rng.choice([0, rng.randrange(1, 20000), 0xFFFFFFFF]))
    c.field(i, 0xBC, "I", rng.choice([0, rng.randrange(1, 60000)]))


def set_globals(c, rng, opponents, **fix):
    vals = {
        "mode": rng.randrange(4), "lapped": rng.randrange(2), "laps": rng.randrange(1, 6), "opponents": opponents,
        "time": rng.randrange(0, 60000), "finished": rng.randrange(2), "view": rng.randrange(opponents + 1),
        "difficulty": rng.randrange(3), "state48": rng.choice([0, 1, 2, 9]),
    }
    vals.update(fix)
    addr = {"mode": 0x030056E0, "lapped": 0x0300608C, "laps": 0x030056E4, "opponents": 0x03005784,
            "time": 0x03005800, "finished": 0x030061A4, "view": 0x030057F8, "difficulty": 0x03005608,
            "state48": 0x03000048}
    for k, v in vals.items():
        c.put(addr[k], "I" if k != "laps" else "i", v)
    return vals


def world_count(gba):
    return struct.unpack("<H", gba.read_base(gba_u32(gba, WORLD + 0x40), 2))[0]


def gen_lap_crossing(gba, rng, n):
    count, have = world_count(gba), racers_with_drivers(view(gba, []))
    out = []
    for _ in range(n):
        c = Case(gba)
        opponents = rng.randrange(1, min(4, len(have)))
        g = set_globals(c, rng, opponents)
        places = list(range(1, opponents + 2))
        rng.shuffle(places)
        who = rng.choice([i for i in have if i <= opponents + 1])
        for i in range(max(opponents, who) + 1):
            set_racer(c, i, rng, count, g["laps"])
            if i <= opponents:
                c.field(i, 0xA8, "i", places[i])
            if i == who and rng.random() < 0.6:  # make most calls cross
                c.field(i, 0x72, "H", 0, driver=False)
                c.field(i, 0x90, "h", rng.choice([0, count - 1 if g["lapped"] else count - 2]), driver=False)
                c.field(i, 0x4D8, "H", 2 | rng.getrandbits(4))
        c.raw(0x03005650, bytes(rng.randrange(256) for _ in range(0x40)))
        pre, post, _ = c.run(FN["lap_crossing"], r0=WORLD, r1=c.entity(who))
        case = {"who": who, "sprint": 0}
        case.update(race_block(pre, "pre", who))
        case.update(race_block(post, "post", who))
        out.append(case)
    return out


def gen_update_places(gba, rng, n):
    count = world_count(gba)
    out = []
    for _ in range(n):
        c = Case(gba)
        opponents = rng.randrange(1, 4)
        g = set_globals(c, rng, opponents, player=0)
        for i in range(opponents + 1):
            set_racer(c, i, rng, count, g["laps"])
            c.field(i, 0xA8, "i", rng.randrange(0, 5))
            if rng.random() < 0.3:  # ties
                c.field(i, 0xAC, "i", 1000)
                c.field(i, 0xC5, "b", 1)
        pre, post, _ = c.run(FN["update_places"], r0=WORLD)
        case = {"sprint": 0}
        case.update(race_block(pre, "pre", 0))
        case.update(race_block(post, "post", 0))
        out.append(case)
    return out


def gen_race_progress(gba, rng, n):
    count = world_count(gba)
    out = []
    for _ in range(n):
        c = Case(gba)
        g = set_globals(c, rng, 3)
        i = rng.randrange(4)
        set_racer(c, i, rng, count, g["laps"])
        pre, _, r = c.run(FN["race_progress"], r0=WORLD, r1=c.entity(i))
        out.append({"sprint": 0, "g": pre.globals(), "c": pre.racer(c.entity(i)), "ret": s32(r.regs["r0"])})
    return out


def gen_track_player(gba, rng, n):
    """The player's tracker with lap_crossing stubbed: the stub records the racer at the call (`mid`)."""
    base = view(gba, [])
    head = gba_u32(gba, WORLD + 0x40)
    count = base.get(head, "H")
    line = gba_u32(gba, WORLD + 0x44)
    sections = [(base.get(head + 8 * s, "H"), base.get(head + 8 * s + 4, "I")) for s in range(2)]
    out = []
    for _ in range(n):
        c = Case(gba)
        set_globals(c, rng, 3)
        sec = 0 if rng.random() < 0.8 else 1
        cnt, first = sections[sec]
        seg = rng.randrange(cnt)
        w = line + 0x18 * (first + min(seg + rng.choice([0, 0, 1, 1, -1]), cnt - 1))
        x, z = base.get(w, "i"), base.get(w + 4, "i")
        c.field(0, 0x72, "H", sec, driver=False)
        c.field(0, 0x90, "h", seg, driver=False)
        c.field(0, 0x0C, "i", (x + rng.randrange(-2500, 2500)) << 8 | rng.getrandbits(8), driver=False)
        c.field(0, 0x14, "i", (z + rng.randrange(-2500, 2500)) << 8 | rng.getrandbits(8), driver=False)
        c.field(0, 0x4D8, "H", rng.getrandbits(4))
        c.field(0, 0x4EC, "h", rng.choice([0, rng.randrange(0, 40), 27, 28, -1]))
        vec = [rng.randrange(-4096, 4097) for _ in range(6)]
        for k, v in enumerate(vec):
            c.field(0, (0x11C if k < 3 else 0x140 - 12) + 4 * k, "i", v)
        mid = []
        stub = {FN["lap_crossing"]: lambda uc: mid.append(
            Mem(lambda a, k: bytes(uc.mem_read(a, k))).racer(c.entity(0)))}
        pre, post, _ = c.run(FN["track_player"], r0=WORLD, r1=c.entity(0), stubs=stub)
        case = {"sprint": 0, "built": 0, "g": pre.globals(), "vec": ",".join(map(str, vec)),
                "pre": pre.racer(c.entity(0)), "post": post.racer(c.entity(0)),
                "ww_flag": post.get(0x03005384, "I")}
        if mid:
            case["mid"] = mid[0]
        out.append(case)
    return out


def tuned(gba):
    """hunter_tuning_init once, kept in the snapshot (the reference race is a circuit)."""
    r = gba.call(FN["hunter_tuning_init"], keep=True)
    assert r.stop == "return"


def tune_str(m):
    return ",".join(str(m.get(a, "i")) for a in TUNING)


def gen_hunter_life_tick(gba, rng, n):
    tuned(gba)
    out = []
    for _ in range(n):
        c = Case(gba)
        set_globals(c, rng, 3)
        i = rng.randrange(4)
        c.field(i, 0x4E8, "i", rng.choice([0, 0x80000, 0x80000 - 100, rng.randrange(0, 0x80001), 500, 50]))
        c.field(i, 0x4EC, "h", rng.choice([0, 27, 28, rng.randrange(-5, 60)]))
        c.field(i, 0x4EE, "h", rng.choice([0, 50, 51, rng.randrange(-5, 80)]))
        c.field(i, 0xA8, "i", rng.randrange(0, 5))
        pre, post, _ = c.run(FN["hunter_life_tick"], r0=WORLD, r1=c.entity(i))
        out.append({"g": pre.globals(), "tune": tune_str(pre), "pre": pre.racer(c.entity(i)),
                    "post": post.racer(c.entity(i))})
    return out


def gen_hunter_hit(gba, rng, n):
    tuned(gba)
    out = []
    for _ in range(n):
        c = Case(gba)
        set_globals(c, rng, rng.randrange(1, 4))
        a, v = rng.sample(range(4), 2)
        for i in (a, v):
            c.field(i, 0x4A, "H", rng.choice([1, 1, 2, 256]), driver=False)
            c.field(i, 0x4E8, "i", rng.choice([0, 0x80000, rng.randrange(0, 0x80001)]))
            c.field(i, 0x4F0, "h", rng.randrange(-3, 50))
        imp = rng.choice([0, 1, rng.randrange(0, 0x3000), rng.randrange(-0x100, 0)])
        pre, post, _ = c.run(FN["hunter_hit"], r0=c.entity(a), r1=c.entity(v), r2=imp)
        out.append({"g": pre.globals(), "tune": tune_str(pre), "impulse": imp,
                    "pre.a": pre.racer(c.entity(a)), "pre.v": pre.racer(c.entity(v)),
                    "post.a": post.racer(c.entity(a)), "post.v": post.racer(c.entity(v))})
    return out


def gen_drain(name):
    def gen(gba, rng, n):
        tuned(gba)
        out = []
        for _ in range(n):
            c = Case(gba)
            set_globals(c, rng, 3)
            i = rng.randrange(4)
            c.field(i, 0x4A, "H", rng.choice([1, 2, 256]), driver=False)
            c.field(i, 0x4E8, "i", rng.choice([0, rng.randrange(0, 0x80001)]))
            c.field(i, 0x4F0, "h", rng.randrange(-3, 50))
            amt = rng.choice([0, rng.randrange(0, 0x4000), rng.randrange(-0x100, 0)])
            pre, post, _ = c.run(FN[name], r0=c.entity(i), r1=amt)
            out.append({"g": pre.globals(), "tune": tune_str(pre), "amount": amt,
                        "pre": pre.racer(c.entity(i)), "post": post.racer(c.entity(i))})
        return out
    return gen


def gen_finish_estimate(gba, rng, n):
    count = world_count(gba)
    out = []
    for _ in range(n):
        c = Case(gba)
        g = set_globals(c, rng, 3)
        i = rng.randrange(4)
        set_racer(c, i, rng, count, g["laps"])
        el = rng.choice([0, 1, rng.randrange(1, 20000), rng.randrange(1, 400000), 0xFFFFFFFF])
        pre, post, r = c.run(FN["finish_estimate"], r0=WORLD, r1=c.entity(i), r2=el)
        out.append({"sprint": 0, "g": pre.globals(), "elapsed": el, "pre": pre.racer(c.entity(i)),
                    "post": post.racer(c.entity(i)), "post.g": post.globals(), "ret": s32(r.regs["r0"])})
    return out


def random_ranked(rng, opponents):
    t = bytearray(rng.randrange(256) for _ in range(0x40))
    ids = list(range(opponents + 1))
    rng.shuffle(ids)
    for k, e in enumerate(ids):
        t[4 + k] = e
        if rng.random() < 0.3:  # ties
            t[0x20 + 4 * k:0x24 + 4 * k] = struct.pack("<I", 5000)
            t[0x30 + 4 * k:0x34 + 4 * k] = struct.pack("<I", 0)
    return bytes(t)


def gen_rank_results(gba, rng, n):
    out = []
    for _ in range(n):
        c = Case(gba)
        opponents = rng.randrange(0, 4)
        c.put(0x03005784, "I", opponents)
        t = random_ranked(rng, opponents)
        c.raw(0x03005730, t)
        key, desc = rng.choice([1, 2, 4, 3]), rng.randrange(2)
        _, post, _ = c.run(FN["rank_results"], r0=key, r1=desc)
        out.append({"opponents": opponents, "key": key, "desc": desc, "pre.ranked": t.hex(),
                    "post.ranked": post.hex(0x03005730, 0x40)})
    return out


def gen_career_race_payout(gba, rng, n):
    """FUN_0812ee14 (called afterwards when 0x030000A0 is not 0) is stubbed: it is not part of the payout."""
    p, recs = gba_u32(gba, PROFILE_PTR), gba_u32(gba, RECORDS_PTR)
    out = []
    for _ in range(n):
        c = Case(gba)
        opponents = rng.randrange(1, 4)
        mode = rng.choice([0, 1, 2, 3, 3, 4])
        flag = rng.choice([1, 1, 1, 0, 2])
        zone = rng.randrange(6)
        slot = rng.randrange(6 if zone == 5 else 12)
        car = rng.randrange(15)
        for a, f, v in [(0x03005784, "I", opponents), (0x030056E0, "I", mode), (0x030000A0, "I", flag)]:
            c.put(a, f, v)
        c.put(p + 0x1FB, "B", zone)
        c.put(p + 0x1FC, "B", slot)
        c.put(p + 0x10, "b", car)
        c.put(p + 0xC, "I", rng.randrange(0, 200000))
        statuses = bytearray(random_statuses(rng, rng.randrange(1, 4)))
        if rng.random() < 0.15:  # status 0 and nothing done: the reward index before the zone's first event
            for e in range(12 * zone, 12 * zone + 12):
                statuses[e >> 2] &= ~(3 << (2 * (e & 3))) & 0xFF
        c.raw(p + 0x205, bytes(statuses))
        c.raw(recs + 17 * car, bytes([rng.randrange(21), rng.randrange(9), rng.randrange(17), rng.randrange(12),
                                      rng.randrange(64), rng.randrange(4)] + [0] * 11))
        c.raw(0x03005730, random_ranked(rng, opponents))
        called = []
        pre, post, _ = c.run(FN["career_race_payout"], stubs={0x0812EE14: lambda uc: called.append(1)})
        out.append({
            "career": flag, "mode": mode, "opponents": opponents, "zone": zone, "slot": slot, "car": car,
            "records": pre.hex(recs, 17 * 15), "pre.cash": pre.get(p + 0xC, "I"),
            "pre.events": pre.hex(p + 0x205, 18), "pre.ranked": pre.hex(0x03005730, 0x40),
            "order": post.hex(0x03005730, 8), "post.ranked": post.hex(0x03005730, 0x40),
            "post.cash": post.get(p + 0xC, "I"), "post.events": post.hex(p + 0x205, 18),
            "paid": post.get(p + 0x3B8, "i"), "after": len(called),
        })
    return out


def gen_save_encode(gba, rng, n):
    p, recs = gba_u32(gba, PROFILE_PTR), gba_u32(gba, RECORDS_PTR)
    buf = 0x0203F000
    glob = [0x030053E4, 0x03000040, 0x03005698, 0x03005798, 0x0300578C, 0x030053A4, 0x03005600, 0x03000050,
            0x03000070]
    out = []
    for k in range(n):
        c = Case(gba)
        wide = k % 5 == 0  # whole random words: every bit the encoder masks off
        for o, size in [(0, 8), (0xC, 4), (0x10, 1), (0x12, 2), (0x14, 225), (0xF5, 4), (0x1F8, 1), (0x1FB, 2),
                        (0x200, 4), (0x205, 18), (0x218, 60), (0x254, 2)]:
            c.raw(p + o, bytes(rng.randrange(256) for _ in range(size)))
        for o in [0x478, 0x47C, 0x480, 0x484, 0x488, 0x48C]:
            c.put(p + o, "I", rng.getrandbits(32) if wide else rng.randrange(2))
        c.raw(recs, bytes(rng.randrange(256 if wide else 64) for _ in range(17 * 15)))
        for a in glob:
            c.put(a, "I", rng.getrandbits(32) if wide else rng.randrange(4) << (3 if a in (0x0300578C, 0x030053A4) else 0))
        heap = bytes(rng.randrange(256) for _ in range(0x200))
        c.raw(buf, heap)
        pre, post, _ = c.run(FN["save_encode"], r0=buf)
        out.append({"globals": ",".join(str(pre.get(a, "I")) for a in glob), "heap": heap.hex(),
                    "profile": pre.hex(p, 0x490), "cars": pre.hex(recs, 17 * 15), "out": post.hex(buf, 0x200)})
    return out


GENERATORS = {
    "style_rating": gen_style_rating, "rebuild_unlocks": gen_rebuild_unlocks, "race_progress": gen_race_progress,
    "lap_crossing": gen_lap_crossing, "update_places": gen_update_places, "track_player": gen_track_player,
    "hunter_life_tick": gen_hunter_life_tick, "hunter_hit": gen_hunter_hit,
    "hunter_drain_a": gen_drain("hunter_drain_a"), "hunter_drain_b": gen_drain("hunter_drain_b"),
    "finish_estimate": gen_finish_estimate, "rank_results": gen_rank_results,
    "career_race_payout": gen_career_race_payout, "save_encode": gen_save_encode,
}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("names", nargs="*", default=list(GENERATORS))
    ap.add_argument("--n", type=int, default=2000)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--snapshot", default="mgba/race")
    args = ap.parse_args()
    _, sha8 = canonical()
    out_dir = data_dir() / "work" / sha8 / "race-rules"
    out_dir.mkdir(parents=True, exist_ok=True)
    for name in args.names:
        gba = Gba(args.snapshot)  # fresh per function (hunter_tuning_init keeps its writes)
        rng = random.Random(f"{args.seed}:{name}")
        cases = GENERATORS[name](gba, rng, args.n)
        path = out_dir / f"oracle-{name}.jsonl"
        with path.open("w", encoding="utf-8") as f:
            for case in cases:
                row = {"fn": name, "snapshot": args.snapshot, **{k: str(v) for k, v in case.items()}}
                f.write(json.dumps(row) + "\n")
        print(f"{name}: {len(cases)} cases -> {path}")


if __name__ == "__main__":
    main()
