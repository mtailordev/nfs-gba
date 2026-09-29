//! Traffic spawning (`FUN_08143d48`) on typed state, run from the car step by the traffic countdown
//! (`FUN_08143b2c`).
//!
//! A traffic car takes a free entity (handler 0x36) and a 0x28-byte block ([`TrafficBlock`]), and appears on the
//! main route a little ahead of or behind the player, in a random lane, unless it would land near a racer or
//! another traffic car (the 8 live slots, `CarGlobals::live`). The block belongs to the entity (`Slot::block`); a
//! failed attempt allocates and frees it in the game, which leaves nothing in the typed state.

use crate::carworld::CarWorld;

use crate::math::{div, isqrt};
/// `rand_table` on the RAM image, for the callers that still keep their state there (`slots.rs`).
pub use crate::ram::rand;
use crate::state::{Entity, EntityRef, TrafficBlock};

/// Spawn distance from the player: past sqrt(0x18FFFFF), about 1,280 city units; clearance sqrt(0x8FFFF).
const SPAWN_DISTANCE2: i32 = 0x18F_FFFF;
const CLEARANCE2: i32 = 0x8_FFFF;

/// `FUN_08137534`: the first entity of the traffic range whose state bit 0 is clear.
fn free_entity(w: &CarWorld) -> Option<usize> {
    w.extra.clone().find(|&k| w.slots[k].e.state & 1 == 0)
}

fn clamp_waypoint(w: &CarWorld, i: i32, count: i32) -> i32 {
    let mut i = i;
    if w.g.circuit == 0 {
        if count - 1 <= i {
            i = count - 1;
        }
        if i < 0 {
            i = 0;
        }
    } else {
        if count - 1 <= i {
            i = 0;
        }
        if i < 0 {
            i = i - 1 + count;
        }
    }
    i
}

fn dist2(dx: i32, dz: i32) -> i32 {
    dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz))
}

/// `FUN_08143d48`: a traffic car for spawn kind `kind` (1: ahead of or behind the entity `near`, the player) in
/// the first free entity; its index, or `None` when there is none or no place.
pub fn spawn(w: &mut CarWorld, near: usize, kind: u32) -> Option<usize> {
    let t = free_entity(w)?;
    if w.g.traffic_on == 0 {
        return None;
    }
    w.slots[t].e.traffic_mode = kind as i16;
    w.slots[t].block = Some(TrafficBlock::default());
    let placed = if kind != 1 {
        (kind != 0 && kind != 2) || at_section_start(w, near, t, kind)
    } else {
        ahead_or_behind(w, near, t)
    };
    if !placed {
        w.slots[t].block = None;
        return None;
    }
    finish(w, t);
    Some(t)
}

/// Kind 1: ahead of or behind `near` on the main route. `false` gives the car up.
fn ahead_or_behind(w: &mut CarWorld, near: usize, t: usize) -> bool {
    let n = w.slots[near].e.clone();
    let seg = n.segment as u32;
    if w.route.line.sections.is_empty() || seg != 0 {
        return false;
    }
    let (count, first) = {
        let s = &w.route.line.sections[0];
        (s.count as i32, s.first as i32)
    };
    let wp = |i: i32| (first + i) as usize;
    let pt = |w: &CarWorld, k: usize| {
        let p = &w.route.line.points[k];
        (p.x, p.z)
    };
    let mut i = n.waypoint as i32;
    let mut back = wp(i);
    i = clamp_waypoint(w, i + 1, count);
    let mut ahead = wp(i);
    let mut step = 1;
    let c = &w.slots[near].c;
    let (a, b) = (pt(w, ahead), pt(w, back));
    let along = c.body.rot[6]
        .wrapping_mul(a.0 - b.0)
        .wrapping_add((a.1 - b.1).wrapping_mul(c.body.rot[8]));
    if along < 1 {
        // Driving against the route: spawn behind.
        step = -1;
        i = n.waypoint as i32;
        (back, ahead) = (ahead, back);
    }
    let lane = if w.rand() & 1 == 0 {
        w.slots[t].e.direction = -1;
        2
    } else {
        w.slots[t].e.direction = 1;
        0
    };
    let (ex, ez) = (n.pos[0] >> 8, n.pos[2] >> 8);
    let a = pt(w, ahead);
    let (mut dx, mut dz) = (a.0 - ex, a.1 - ez);
    let mut tries = 0x14;
    while dist2(dx, dz) <= SPAWN_DISTANCE2 {
        back = ahead;
        i = clamp_waypoint(w, i + step, count);
        ahead = wp(i);
        let a = pt(w, ahead);
        (dx, dz) = (a.0 - ex, a.1 - ez);
        tries -= 1;
        if tries == 0 {
            return false;
        }
    }
    // Too close to a racer? (Compared with the free entity's stale position, as the game does.)
    let mut blocked = 0;
    let racers = w.g.racers as usize;
    if racers != 0 {
        let stale = w.slots[t].e.pos;
        let d = |w: &CarWorld, o: usize| {
            let p = &w.slots[o].e.pos;
            dist2((p[0] - stale[0]) >> 8, (p[2] - stale[2]) >> 8)
        };
        let mut k = 0;
        let mut clear = CLEARANCE2 < d(w, k);
        while clear {
            k += 1;
            if racers <= k {
                break;
            }
            clear = CLEARANCE2 < d(w, k);
        }
        if !clear {
            blocked = 1;
        }
    }
    let e = &mut w.slots[t].e;
    e.pos[0] = (dx << 8).wrapping_add(n.pos[0]);
    e.pos[2] = (dz << 8).wrapping_add(n.pos[2]);
    let pos = e.pos;
    for k in 0..8 {
        let live = w.g.live[k];
        if live.is_none() {
            continue;
        }
        let o = &w.slots[w.entity_of(live)].e.pos;
        if dist2((o[0] - pos[0]) >> 8, (o[2] - pos[2]) >> 8) <= CLEARANCE2 {
            blocked += 1;
            break;
        }
    }
    if blocked != 0 {
        return false;
    }
    let sector = w.route.extra[ahead].sector as u16;
    let mut to = ahead;
    if w.slots[t].e.direction as i32 != step {
        i -= step;
        to = back;
        back = ahead;
        i = clamp_waypoint(w, i, count);
    }
    let (tp, bp) = (pt(w, to), pt(w, back));
    let (dx, dz) = (tp.0 - bp.0, tp.1 - bp.1);
    let len = isqrt(dist2(dx, dz) as u32);
    let heading = w.atan2_fast(dx, dz) as u16 as i16;
    let (ux, uz) = (div(dx << 12, len), div(dz << 12, len));
    let offset = w.data.ai.traffic.lanes[lane as usize];
    let (ox, oz) = (offset.wrapping_mul(ux), offset.wrapping_mul(uz));
    let e = &mut w.slots[t].e;
    e.sector = sector;
    e.traffic_waypoint = i as i16;
    init_entity(e);
    e.pos[1] = n.pos[1];
    e.angles[1] = heading;
    e.heading = heading as i32;
    e.dir_x = ux;
    e.dir_z = uz;
    e.pos[0] = e.pos[0].wrapping_add(oz);
    e.pos[2] = e.pos[2].wrapping_sub(ox);
    e.segment = n.segment;
    let block = w.slots[t].block.as_mut().expect("the spawn allocated the block");
    block.target = [tp.0 + (oz >> 8), tp.1 - (ox >> 8)];
    block.lane = lane;
    true
}

/// What every traffic car starts with: not drawn yet, active, speed 1, handler 0x36.
fn init_entity(e: &mut Entity) {
    e.draw_next = 0xFFFF;
    e.state = 7;
    e.flags = 2;
    e.material_offset = 0;
    e.next = 0xFFFF;
    e.material_step = 0;
    e.race_state = 0;
    e.u_1c = 0;
    e.speed = 1;
    e.speed_up = 0;
    e.handler = 0x36;
}

/// Kinds 0 and 2 (the spawner handlers `FUN_08143ba8` and `FUN_08143c78`, table entries 0x2D/0x2E and 0x2A): the
/// car starts at the first waypoint of `near`'s racing-line section, heading for the second, at full (kind 2) or
/// half (kind 0) unit speed. `false` when the section has no record (the car is given up).
fn at_section_start(w: &mut CarWorld, near: usize, t: usize, kind: u32) -> bool {
    let n = w.slots[near].e.clone();
    w.slots[t].e.traffic_waypoint = 1;
    let Some(section) = w.route.line.sections.get(n.segment as usize) else {
        return false;
    };
    let first = section.first as usize;
    let (p, q) = (w.route.line.points[first], w.route.line.points[first + 1]);
    let sector = w.route.extra[first].sector as u16;
    let (dx, dz) = (q.x - p.x, q.z - p.z);
    let len = isqrt(dist2(dx, dz) as u32);
    let heading = w.atan2_fast(dx, dz) as u16 as i16;
    let unit = if kind == 2 { 0x1000 } else { 0x800 };
    let e = &mut w.slots[t].e;
    init_entity(e);
    e.sector = sector;
    e.angles = [0, heading];
    e.dir_x = div(dx.wrapping_mul(unit), len);
    e.dir_z = div(dz.wrapping_mul(unit), len);
    e.heading = heading as i32;
    e.pos = [p.x << 8, n.pos[1], p.z << 8];
    e.segment = n.segment;
    let block = w.slots[t].block.as_mut().expect("the spawn allocated the block");
    block.target = [q.x, q.z];
    block.turn_t = 0;
    true
}

/// The part of `FUN_08143d48` all kinds share: the traffic type, the sector list, a live-traffic slot, and the
/// car's block (`turn_by` the unit direction).
fn finish(w: &mut CarWorld, t: usize) {
    // Traffic type from the race's frame counter.
    let models = w.g.traffic_models;
    let mut r = crate::math::umod(w.g.race_frames, models);
    if r == models {
        r -= 1;
    }
    let (material, model) = w.data.ai.traffic.models[r as usize];
    let e = &mut w.slots[t].e;
    e.material = material;
    e.model = model as i16;
    e.traffic_type = r as u16;
    e.u_70 = 0x200;
    let (dir_x, dir_z) = (e.dir_x, e.dir_z);
    w.link(t);
    if let Some(k) = (0..8).find(|&k| w.g.live[k].is_none()) {
        w.g.live[k] = EntityRef::to(t);
    }
    let block = w.slots[t].block.as_mut().expect("the spawn allocated the block");
    block.turn_from = [0, 0];
    block.flags = 0;
    block.turn_steps = 0;
    block.turn_by = [dir_x, dir_z];
}
