//! Race-route tracking: which route segment (entity `+0x72`) and waypoint (entity `+0x90`) a car is at, its
//! progress along the route, laps and the gap to the car ahead.
//!
//! World `+0x40` holds the route segments (8 bytes: u16 waypoint count, u32 first waypoint) and `+0x44` the
//! waypoints (0x18 bytes: x, z, ..., u16 `+0x0C` linked segment, u16 `+0x0E` waypoint in it, `+0x10`
//! distance). Segment 0 is the main route; the others are side branches. The per-waypoint lines at `*0x03005FB4`
//! (0x20 bytes) and the join table at `*0x03005FB8` are built at race start.

use crate::math::{cos, div, dot, sin};
use crate::mem::Mem;
use crate::world::{NONE, PLAYER, W_ENTITIES, W_SEGMENTS, W_WAYPOINTS};
use crate::{Result, Unported};

/// Non-zero for a circuit (waypoint indices wrap around segment 0).
pub const CIRCUIT: u32 = 0x0300_608C;
const WAYPOINT_LINES: u32 = 0x0300_5FB4;
const SEGMENT_JOINS: u32 = 0x0300_5FB8;
const SEGMENT_LENGTHS: u32 = 0x0300_6120;
const SEGMENT_VISITED: u32 = 0x0300_60C0;
const ROUTE_INDEX: u32 = 0x0300_5720;
/// Per route: pointer to the list of side segments and the sectors that belong to them (`FUN_0813f234`).
const SIDE_SEGMENT_SECTORS: u32 = 0x087F_37D8;
pub const RACE_TIME: u32 = 0x0300_5800;
pub const OPPONENTS: u32 = 0x0300_5784;
const LAPS: u32 = 0x0300_56E4;

fn segment(mem: &Mem, seg: u32) -> u32 {
    mem.u32(W_SEGMENTS) + seg * 8
}

/// `FUN_0814007c`: address of waypoint `index` of segment `seg`.
pub fn waypoint(mem: &Mem, seg: u32, index: i32) -> u32 {
    let first = mem.i32(segment(mem, seg) + 4);
    mem.u32(W_WAYPOINTS)
        .wrapping_add((first.wrapping_add(index) as u32).wrapping_mul(0x18))
}

/// `FUN_0813e860`: normalise waypoint `index` of segment `seg`, following links into the next or previous
/// segment and wrapping on circuits. Returns the index and the segment it lies in.
pub fn advance(mem: &Mem, seg: u32, index: i32) -> (i32, u32) {
    let rec = segment(mem, seg);
    let count = mem.u16(rec) as i32;
    let last = count - 1;
    let (next_seg, next_index) = if last < index {
        if seg == 0 {
            return (if mem.i32(CIRCUIT) != 0 { index + 1 - count } else { last }, seg);
        }
        let w = mem.u32(W_WAYPOINTS) + (mem.i32(rec + 4) + count - 1) as u32 * 0x18;
        if mem.u16(w + 0xE) as u32 == NONE {
            return (last, seg);
        }
        (mem.u16(w + 0xC) as u32, mem.u16(w + 0xE) as i32 + index - count + 1)
    } else {
        if index >= 0 {
            return (index, seg);
        }
        if seg == 0 {
            return (if mem.i32(CIRCUIT) == 0 { 0 } else { index - 1 + count }, seg);
        }
        let w = mem.u32(W_WAYPOINTS) + mem.i32(rec + 4) as u32 * 0x18;
        let back = mem.u16(w + 0xE) as u32;
        if back == NONE {
            (seg, mem.i32(mem.u32(SEGMENT_JOINS) + seg * 4) + index)
        } else {
            (mem.u16(w + 0xC) as u32, back as i32 + index)
        }
    };
    advance(mem, next_seg, next_index)
}

/// `FUN_0814009c`: address of the normalised waypoint.
pub fn waypoint_at(mem: &Mem, seg: u32, index: i32) -> u32 {
    let (i, s) = advance(mem, seg, index);
    waypoint(mem, s, i)
}

/// `FUN_0813e93c`: how far the car is along its current waypoint's line (`*0x03005FB4` record: direction at
/// `+0x00/+0x04`, widening terms `+0x08/+0x0C`, length `+0x1C`), divided by 8.
pub fn along_line(mem: &Mem, e: u32) -> i32 {
    let (seg, wp) = (mem.u16(e + 0x72) as u32, mem.i16(e + 0x90) as i32);
    let (i1, s1) = advance(mem, seg, wp);
    let line = i1 + mem.i32(segment(mem, s1) + 4);
    let (i2, s2) = advance(mem, seg, wp);
    let w = waypoint(mem, s2, i2);
    let dx = (mem.i32(e + 0xC) >> 8).wrapping_sub(mem.i32(w));
    let dz = (mem.i32(e + 0x14) >> 8).wrapping_sub(mem.i32(w + 4));
    let r = mem.u32(WAYPOINT_LINES).wrapping_add((line as u32).wrapping_mul(0x20));
    let (a, b, c, d, len) = (
        mem.i32(r),
        mem.i32(r + 4),
        mem.i32(r + 8),
        mem.i32(r + 0xC),
        mem.i32(r + 0x1C),
    );
    let along = dx.wrapping_mul(a).wrapping_add(b.wrapping_mul(dz)) >> 8;
    let across = b.wrapping_mul(dx).wrapping_sub(a.wrapping_mul(dz)) >> 8;
    let num = along + (across.wrapping_mul(c) >> 12);
    let den = len + (across.wrapping_mul(c + d) >> 12);
    if den == 0 {
        0
    } else {
        div(num.wrapping_mul(len), den) >> 3
    }
}

/// `FUN_0814032c`: race progress (distance along the route), 0 before the start line.
pub fn progress(mem: &Mem, e: u32, p: u32) -> i32 {
    let (seg, wp) = (mem.u16(e + 0x72) as u32, mem.i16(e + 0x90));
    let w = waypoint_at(mem, seg, wp as i32);
    if (mem.i32(CIRCUIT) == 0 && wp == 0) || mem.u16(p + 0x4D8) & 1 != 0 {
        return 0;
    }
    let t = along_line(mem, e);
    mem.i32(w + 0x10) + (t * 8 * mem.i32(SEGMENT_LENGTHS + seg * 4) >> 8)
}

fn dist2_16(mem: &Mem, e: u32, w: u32) -> i32 {
    let dx = ((mem.i32(e + 0xC) >> 8) - mem.i32(w)) >> 4;
    let dz = ((mem.i32(e + 0x14) >> 8) - mem.i32(w + 4)) >> 4;
    dx * dx + dz * dz
}

/// `FUN_0813f234`: switch between the main route and side segments by the sector the car is in, using the
/// route's side-segment table (per entry: segment id, its sectors, -1). Returns 1 when nothing changed.
pub fn track_segment(mem: &mut Mem, e: u32) -> i32 {
    let list = mem.u32(SIDE_SEGMENT_SECTORS + mem.u32(ROUTE_INDEX) * 4);
    let cur = mem.u16(e + 0x72) as u32;
    mem.set_u32(SEGMENT_VISITED + cur * 4, 1);
    if list == 0 {
        return 0;
    }
    let sector = mem.u16(e + 0x78) as u32;
    let mut at = list + 4;
    let entries = mem.u32(list);
    // The waypoint to rejoin at: the first waypoint's join (`*0x03005FB8`) if the car is nearer the segment's
    // first waypoint than its last, else the last waypoint's link.
    let rejoin = |mem: &Mem, seg: u32| -> u16 {
        let first = waypoint(mem, seg, 0);
        let last = waypoint(mem, seg, mem.u16(segment(mem, seg)) as i32 - 1);
        if dist2_16(mem, e, first) < dist2_16(mem, e, last) {
            mem.u32(mem.u32(SEGMENT_JOINS) + seg * 4) as u16
        } else {
            mem.u16(last + 0xE)
        }
    };
    if cur == 0 {
        for _ in 0..entries {
            let seg = mem.u32(at);
            at += 4;
            while mem.u32(at) != u32::MAX {
                let s = mem.u32(at);
                at += 4;
                if s == sector {
                    mem.set_u16(e + 0x72, seg as u16);
                    let first = waypoint(mem, seg, 0);
                    let count = mem.u16(segment(mem, seg)) as i32;
                    let last = waypoint(mem, seg, count - 1);
                    let v = if dist2_16(mem, e, first) < dist2_16(mem, e, last) {
                        0
                    } else {
                        count - 1
                    };
                    mem.set_i16(e + 0x90, v as i16);
                    return 1;
                }
            }
            at += 4;
        }
        return 0;
    }
    for _ in 0..entries {
        let seg = mem.u32(at);
        at += 4;
        if seg == cur {
            // Still in one of this segment's sectors?
            while mem.u32(at) != u32::MAX {
                if mem.u32(at) == sector {
                    return 1;
                }
                at += 4;
            }
            let v = rejoin(mem, seg);
            mem.set_u16(e + 0x90, v);
            mem.set_u16(e + 0x72, 0);
            return 0;
        }
        while mem.u32(at) != u32::MAX {
            let s = mem.u32(at);
            at += 4;
            if s == sector {
                let v = rejoin(mem, cur);
                mem.set_u16(e + 0x90, v);
                mem.set_u16(e + 0x72, seg as u16);
                return 0;
            }
        }
        at += 4;
    }
    1
}

/// `FUN_0813edd8`: count steps driving against the route (`+0x4EC`, sets 0x03005384 past 27), then advance or
/// step back the waypoint when the car crosses a waypoint line, flagging the start/finish area in `+0x4D8`.
pub fn track_waypoint(mem: &mut Mem, e: u32) -> Result<()> {
    let seg = mem.u16(e + 0x72) as u32;
    let seg_rec = segment(mem, seg);
    let wp = mem.i16(e + 0x90) as i32;
    let p = mem.u32(e + 0x8C);
    let (next, s) = advance(mem, seg, wp + 1);
    let first = mem.i32(segment(mem, s) + 4);
    let lines = mem.u32(WAYPOINT_LINES);
    let line = lines.wrapping_add(((first + wp) as u32).wrapping_mul(0x20));
    let dir = [mem.i32(line), 0, mem.i32(line + 4)];
    let fwd = dot(mem.vec3(p + 0x11C), dir);
    let wrong = fwd < -10 || (fwd < 1 && dot(mem.vec3(p + 0x140), dir) < 0);
    let count = if wrong { mem.i16(p + 0x4EC) + 1 } else { 0 };
    mem.set_i16(p + 0x4EC, count);
    mem.set_u32(0x0300_5384, (mem.i16(p + 0x4EC) > 0x1B) as u32);
    let r = lines.wrapping_add(((next + first) as u32).wrapping_mul(0x20));
    let (x, z) = (mem.i32(e + 0xC) >> 8, mem.i32(e + 0x14) >> 8);
    let side = |mem: &Mem, r: u32| {
        x.wrapping_mul(mem.i32(r + 0x10))
            .wrapping_add(z.wrapping_mul(mem.i32(r + 0x14)))
            .wrapping_sub(mem.i32(r + 0x18))
    };
    if side(mem, r) < 1 {
        let r = lines.wrapping_add(((wp + mem.i32(seg_rec + 4)) as u32).wrapping_mul(0x20));
        if side(mem, r) >= 0 {
            return Ok(());
        }
        if seg != 0 {
            mem.set_i16(e + 0x90, mem.i16(e + 0x90) - 1);
            return Ok(());
        }
        let cur = mem.i16(e + 0x90);
        if cur == 2 {
            mem.set_u16(p + 0x4D8, mem.u16(p + 0x4D8) & 0xFFFD);
        }
        let start = if mem.i32(CIRCUIT) == 0 { 1 } else { 0 };
        if cur == start {
            mem.set_u16(p + 0x4D8, mem.u16(p + 0x4D8) | 1);
        }
        let (back, _) = advance(mem, seg, wp - 1);
        mem.set_i16(e + 0x90, back as i16);
        return Ok(());
    }
    if seg == 0 {
        if ((wp - 1) as u32) < 9 {
            mem.set_u16(p + 0x4D8, mem.u16(p + 0x4D8) | 2);
        }
        let count = mem.u16(seg_rec) as i32;
        if wp == count - 2 || wp == 0 {
            mem.set_u16(p + 0x4D8, mem.u16(p + 0x4D8) & 0xFFFE);
        }
        let (fwd, _) = advance(mem, seg, wp + 1);
        mem.set_i16(e + 0x90, fwd as i16);
        if fwd as i16 as i32 == count - 1 {
            mem.set_u16(e + 0x90, 0);
        }
    } else {
        mem.set_i16(e + 0x90, mem.i16(e + 0x90) + 1);
        mem.set_u16(p + 0x4D8, mem.u16(p + 0x4D8) | 2);
    }
    lap(mem, e)
}

/// `FUN_0813f098`: crossing the start line on segment 0 with flag 2 set completes a lap: best lap (`+0xB4`),
/// lap start (`+0xB8`), laps left (`+0xC5`).
pub(crate) fn lap(mem: &mut Mem, e: u32) -> Result<()> {
    let p = mem.u32(e + 0x8C);
    let last = if mem.i32(CIRCUIT) == 0 { -2 } else { -1 };
    let wp = mem.i16(e + 0x90) as i32;
    let main_count = mem.u16(mem.u32(W_SEGMENTS)) as i32;
    if !(mem.i16(e + 0x72) == 0 && (wp == main_count + last || wp == 0) && mem.u16(p + 0x4D8) & 2 != 0) {
        return Ok(());
    }
    mem.set_u16(p + 0x4D8, mem.u16(p + 0x4D8) & 0xFFFD);
    let time = mem.u32(RACE_TIME);
    let lap_time = time.wrapping_sub(mem.u32(p + 0xB8));
    let best = mem.u32(p + 0xB4);
    if lap_time < best || best == 0 {
        mem.set_u32(p + 0xB4, lap_time);
    }
    mem.set_u32(p + 0xB8, time);
    mem.set_u8(p + 0xC5, mem.u8(p + 0xC5).wrapping_sub(1));
    Err(Unported("FUN_0813f098 lap completion (race order, finish)"))
}

/// `FUN_0813ebac`: the time gap to the car one place ahead (or, for the leader, behind), into 0x0300615C.
pub fn gap(mem: &mut Mem, e: u32) {
    let opponents = mem.i32(OPPONENTS);
    mem.set_i32(0x0300_615C, 0);
    let p = mem.u32(e + 0x8C);
    let place = mem.i32(p + 0xA8);
    let target = if place == 1 { 2 } else { place - 1 };
    let entities = mem.u32(W_ENTITIES);
    let distance = |mem: &Mem, q: u32| {
        if mem.i32(CIRCUIT) == 0 {
            mem.i32(q + 0xAC)
        } else {
            let segs = mem.u32(W_SEGMENTS);
            let lap_len =
                mem.i32(mem.u32(W_WAYPOINTS) + mem.i32(segs + 4) as u32 * 0x18 + mem.u16(segs) as u32 * 0x18 - 8);
            (mem.i32(LAPS) - mem.i8(q + 0xC5) as i32) * lap_len + mem.i32(q + 0xAC)
        }
    };
    for k in 0..opponents.max(0) as u32 {
        // The game looks at entity k + 1's physics struct (`entities + 0x130 + k * 0xA4`).
        let q = mem.u32(entities + 0x130 + k * 0xA4);
        if mem.i32(q + 0xA8) != target {
            continue;
        }
        let other = distance(mem, q);
        let mine = distance(mem, p);
        let time = mem.i32(RACE_TIME);
        if place == 1 {
            let mine = match mine >> 4 {
                0 => 1,
                v => v,
            };
            if mine <= other >> 4 {
                return;
            }
            mem.set_i32(0x0300_615C, time - div((other >> 4) * time, mine));
        } else {
            let other = match other >> 4 {
                0 => 1,
                v => v,
            };
            if other <= mine >> 4 {
                return;
            }
            mem.set_i32(0x0300_615C, time - div((mine >> 4) * time, other));
        }
        return;
    }
}

/// `FUN_081402bc`: signed distance of the car from its waypoint across the route direction (waypoint `+0x0A`
/// angle), in city units.
pub fn lateral(mem: &Mem, e: u32) -> i32 {
    let w = waypoint_at(mem, mem.u16(e + 0x72) as u32, mem.i16(e + 0x90) as i32);
    if mem.u32(W_SEGMENTS) == 0 {
        return 0;
    }
    let a = mem.u16(w + 0xA) as i32 - 0x1000;
    let v = cos(mem, a)
        .wrapping_mul((mem.i32(e + 0xC) >> 8) - mem.i32(w))
        .wrapping_add(((mem.i32(e + 0x14) >> 8) - mem.i32(w + 4)).wrapping_mul(sin(mem, a)));
    (if v < 0 { v + 0x3FFF } else { v }) >> 14
}

/// `FUN_08140274`: which of the four lane offsets at 0x087F4120 (enabled by bit mask `lanes`) is nearest `x`.
pub fn nearest_lane(mem: &Mem, x: i32, lanes: i32) -> u32 {
    let (mut best, mut lane) = (i32::MAX, 0);
    for k in 0..4u32 {
        if (lanes >> k) & 1 != 0 {
            let d = (mem.i32(0x087F_4120 + 4 * k) - x).wrapping_abs();
            if d < best {
                best = d;
                lane = k;
            }
        }
    }
    lane
}

/// `FUN_08143b2c` (player only): the traffic-spawn countdown.
pub fn traffic_countdown(mem: &mut Mem) -> Result<()> {
    if mem.u8(0x0300_6250) != 0 {
        mem.set_u8(0x0300_6250, 0);
        return Ok(());
    }
    if mem.u8(0x0300_6298) != 0 && mem.u8(0x0300_6240) < 4 {
        let near = crate::world::entity(mem, mem.u32(0x0300_57F8));
        let c = mem.u8(0x0300_6264).wrapping_sub(1);
        mem.set_u8(0x0300_6264, c);
        if c == 0 {
            if crate::traffic::spawn(mem, near, 1)? != NONE {
                mem.set_u8(0x0300_6240, mem.u8(0x0300_6240).wrapping_add(1));
            }
            mem.set_u8(0x0300_6264, mem.u32(0x0300_6260) as u8);
        }
    }
    Ok(())
}

/// `FUN_0814078c` (control action 8, R+L): the wingman command; only with a wingman (0x03006104 = 1..=12).
pub fn wingman_command(mem: &mut Mem) -> Result<()> {
    if mem.i32(0x0300_6104).wrapping_sub(1) as u32 >= 0xC {
        return Ok(());
    }
    if mem.i32(0x0300_61DC) != 0 && mem.i32(0x0300_61E8) == 0 && mem.i32(0x0300_61D8) == 0 {
        return Err(Unported("FUN_0814078c (wingman command)"));
    }
    Ok(())
}

/// Whether entity `index` is the local player.
pub fn is_player(mem: &Mem, index: u32) -> bool {
    index == mem.u32(PLAYER)
}
