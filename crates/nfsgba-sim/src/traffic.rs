//! Traffic spawning (`FUN_08143d48`), run from the player's step by the traffic countdown (`FUN_08143b2c`).
//!
//! A traffic car takes a free entity (handler 0x36) and a 0x28-byte block on the heap, and appears on the
//! main route a little ahead of or behind the player, in a random lane, unless it would land near a racer or
//! another traffic car (the 8 slots at 0x03006270).

use crate::Result;
use crate::heap;
use crate::math::{div, isqrt};
use crate::mem::Mem;
use crate::route::CIRCUIT;
use crate::world::{self, NONE, W_ENTITIES, W_SEGMENTS, W_WAYPOINTS, WORLD};

/// The live traffic cars (entity addresses, 0 = free).
const SLOTS: u32 = 0x0300_6270;
const RACERS: u32 = 0x0300_57EC;
/// Lane offsets (4 words) and traffic models (u16 model, u16 paint per type).
const LANE_OFFSETS: u32 = 0x087F_5488;
const TRAFFIC_TYPES: u32 = 0x087F_546C;
/// Spawn distance from the player: past sqrt(0x18FFFFF), about 1,280 city units; clearance sqrt(0x8FFFF).
const SPAWN_DISTANCE2: i32 = 0x18F_FFFF;
const CLEARANCE2: i32 = 0x8_FFFF;

/// `rand_table` (`FUN_0815fcfc`): the next entry of the 256-entry table at 0x087C03F0.
pub fn rand(m: &mut Mem) -> u32 {
    let mut k = m.u32(0x0300_64C8);
    let r = nfsgba_fixed::rand_table(&m.rom, &mut k);
    m.set_u32(0x0300_64C8, k);
    r
}

/// `atan2_fast` (IWRAM 0x03004470) on the ROM in `m`.
pub fn atan2_fast(m: &Mem, x: i32, z: i32) -> i32 {
    nfsgba_fixed::atan2_fast(&m.rom, x, z)
}

/// `FUN_08137534`: the first entity at world `+0xF8` and after (world `+0xFA` of them) whose flag bit 0 is clear.
fn free_entity(m: &Mem) -> u32 {
    let (first, count) = (m.u16(WORLD + 0xF8) as u32, m.u16(WORLD + 0xFA) as u32);
    (0..count)
        .map(|k| world::entity(m, first + k))
        .find(|&e| m.u16(e + 8) & 1 == 0)
        .map_or(NONE, |e| m.u16(e) as u32)
}

fn clamp_waypoint(m: &Mem, i: i32, count: i32) -> i32 {
    let mut i = i;
    if m.i32(CIRCUIT) == 0 {
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

/// `FUN_08143d48` for `kind` 1 (ahead of or behind `near`, the player): the new entity's index, or 0xFFFF.
pub fn spawn(m: &mut Mem, near: u32, kind: u32) -> Result<u32> {
    let slot = free_entity(m);
    if slot == NONE || m.u8(0x0300_6298) == 0 {
        return Ok(NONE);
    }
    let t = world::entity(m, slot);
    m.set_u16(t + 0x9E, kind as u16);
    let block = heap::alloc_zeroed(m, 0x28);
    m.set_u32(t + 0x8C, block);
    let give_up = |m: &mut Mem| {
        if m.u32(t + 0x8C) != 0 {
            heap::free(m, block);
            m.set_u32(t + 0x8C, 0);
        }
        Ok(NONE)
    };
    if kind != 1 {
        if (kind == 0 || kind == 2) && !at_section_start(m, near, t, block, kind) {
            return give_up(m);
        }
        return Ok(finish(m, t, slot, block));
    }
    let seg = m.u16(near + 0x72) as u32;
    let segs = m.u32(W_SEGMENTS);
    if segs.wrapping_add(seg * 8) == 0 || seg != 0 {
        return give_up(m);
    }
    let count = m.u16(segs) as i32;
    let wp = |m: &Mem, i: i32| m.u32(W_WAYPOINTS).wrapping_add((m.i32(segs + 4) + i) as u32 * 0x18);
    let mut i = m.i16(near + 0x90) as i32;
    let mut back = wp(m, i);
    i = clamp_waypoint(m, i + 1, count);
    let mut ahead = wp(m, i);
    let mut step = 1;
    let p = m.u32(near + 0x8C);
    let along = m
        .i32(p + 0x140)
        .wrapping_mul(m.i32(ahead) - m.i32(back))
        .wrapping_add((m.i32(ahead + 4) - m.i32(back + 4)).wrapping_mul(m.i32(p + 0x148)));
    if along < 1 {
        // Driving against the route: spawn behind.
        step = -1;
        i = m.i16(near + 0x90) as i32;
        (back, ahead) = (ahead, back);
    }
    let lane = if rand(m) & 1 == 0 {
        m.set_u16(t + 0x9A, 0xFFFF);
        2
    } else {
        m.set_u16(t + 0x9A, 1);
        0
    };
    let (ex, ez) = (m.i32(near + 0xC) >> 8, m.i32(near + 0x14) >> 8);
    let (mut dx, mut dz) = (m.i32(ahead) - ex, m.i32(ahead + 4) - ez);
    let mut tries = 0x14;
    while dist2(dx, dz) <= SPAWN_DISTANCE2 {
        back = ahead;
        i = clamp_waypoint(m, i + step, count);
        ahead = wp(m, i);
        (dx, dz) = (m.i32(ahead) - ex, m.i32(ahead + 4) - ez);
        tries -= 1;
        if tries == 0 {
            return give_up(m);
        }
    }
    // Too close to a racer? (Compared with the free entity's stale position, as the game does.)
    let mut blocked = 0;
    let racers = m.u32(RACERS);
    if racers != 0 {
        let entities = m.u32(W_ENTITIES);
        let d = |m: &Mem, o: u32| {
            dist2(
                (m.i32(o + 0xC) - m.i32(t + 0xC)) >> 8,
                (m.i32(o + 0x14) - m.i32(t + 0x14)) >> 8,
            )
        };
        let mut k = 0;
        let mut o = entities;
        let mut clear = CLEARANCE2 < d(m, o);
        while clear {
            k += 1;
            if racers <= k {
                break;
            }
            o += 0xA4;
            clear = CLEARANCE2 < d(m, o);
        }
        if !clear {
            blocked = 1;
        }
    }
    m.set_i32(t + 0xC, (dx << 8).wrapping_add(m.i32(near + 0xC)));
    m.set_i32(t + 0x14, (dz << 8).wrapping_add(m.i32(near + 0x14)));
    for k in 0..8 {
        let o = m.u32(SLOTS + 4 * k);
        if o != 0
            && dist2(
                (m.i32(o + 0xC) - m.i32(t + 0xC)) >> 8,
                (m.i32(o + 0x14) - m.i32(t + 0x14)) >> 8,
            ) <= CLEARANCE2
        {
            blocked += 1;
            break;
        }
    }
    if blocked != 0 {
        return give_up(m);
    }
    m.set_u16(t + 0x78, m.u32(ahead + 0x14) as u16);
    let mut to = ahead;
    if m.i16(t + 0x9A) as i32 != step {
        i -= step;
        to = back;
        back = ahead;
        i = clamp_waypoint(m, i, count);
    }
    m.set_u16(t + 0x9C, i as u16);
    m.set_u16(t + 4, 0xFFFF);
    m.set_u16(t + 8, 7);
    m.set_u16(t + 0xA, 2);
    m.set_u16(t + 0x46, 0);
    m.set_u16(t + 2, 0xFFFF);
    m.set_u16(t + 0x44, 0);
    m.set_i32(t + 0x10, m.i32(near + 0x10));
    m.set_u16(t + 0x4A, 0);
    m.set_u32(t + 0x1C, 0);
    m.set_u32(t + 0x24, 1);
    m.set_u16(t + 0x52, 0);
    m.set_u16(t + 0x4E, 0x36);
    let (dx, dz) = (m.i32(to) - m.i32(back), m.i32(to + 4) - m.i32(back + 4));
    let len = isqrt(dist2(dx, dz) as u32);
    m.set_u16(t + 0x32, atan2_fast(m, dx, dz) as u16);
    m.set_i32(t + 0x2C, m.i16(t + 0x32) as i32);
    let (ux, uz) = (div(dx << 12, len), div(dz << 12, len));
    m.set_i32(t + 0x18, ux);
    m.set_i32(t + 0x20, uz);
    let offset = m.i32(LANE_OFFSETS + lane * 4);
    let (ox, oz) = (offset.wrapping_mul(ux), offset.wrapping_mul(uz));
    m.set_i32(t + 0xC, m.i32(t + 0xC).wrapping_add(oz));
    m.set_i32(t + 0x14, m.i32(t + 0x14).wrapping_sub(ox));
    m.set_i32(block, m.i32(to) + (oz >> 8));
    m.set_i32(block + 4, m.i32(to + 4) - (ox >> 8));
    m.set_u32(block + 0x24, lane);
    m.set_u16(t + 0x72, m.u16(near + 0x72));
    Ok(finish(m, t, slot, block))
}

/// Kinds 0 and 2 (the spawner handlers `FUN_08143ba8` and `FUN_08143c78`, table entries 0x2D/0x2E and 0x2A): the
/// car starts at the first waypoint of `near`'s racing-line section, heading for the second, at full (kind 2) or
/// half (kind 0) unit speed. `false` when the section has no record (the car is given up).
fn at_section_start(m: &mut Mem, near: u32, t: u32, block: u32, kind: u32) -> bool {
    m.set_u16(t + 0x9C, 1);
    let rec = m.u32(W_SEGMENTS).wrapping_add(m.u16(near + 0x72) as u32 * 8);
    if rec == 0 {
        return false;
    }
    let w = m.u32(W_WAYPOINTS).wrapping_add(m.i32(rec + 4) as u32 * 0x18);
    m.set_u16(t + 4, 0xFFFF);
    m.set_u16(t + 8, 7);
    m.set_u16(t + 0xA, 2);
    m.set_u16(t + 0x46, 0);
    m.set_u16(t + 2, 0xFFFF);
    m.set_u16(t + 0x44, 0);
    m.set_u16(t + 0x78, m.i32(w + 0x14) as u16);
    m.set_u16(t + 0x4A, 0);
    m.set_u32(t + 0x1C, 0);
    m.set_u16(t + 0x30, 0);
    m.set_u32(t + 0x24, 1);
    m.set_u16(t + 0x52, 0);
    m.set_u16(t + 0x4E, 0x36);
    // (The game first sets +0x32/+0x2C from the next waypoint's +0x0A; both are overwritten below.)
    let (x, z) = (m.i32(w), m.i32(w + 4));
    let (dx, dz) = (m.i32(w + 0x18) - x, m.i32(w + 0x1C) - z);
    let len = isqrt(dist2(dx, dz) as u32);
    m.set_u16(t + 0x32, atan2_fast(m, dx, dz) as u16);
    let unit = if kind == 2 { 0x1000 } else { 0x800 };
    m.set_i32(t + 0x18, div(dx.wrapping_mul(unit), len));
    m.set_i32(t + 0x20, div(dz.wrapping_mul(unit), len));
    m.set_i32(t + 0x2C, m.i16(t + 0x32) as i32);
    m.set_i32(block, m.i32(w + 0x18));
    m.set_i32(block + 4, m.i32(w + 0x1C));
    m.set_i32(t + 0xC, x << 8);
    m.set_i32(t + 0x10, m.i32(near + 0x10));
    m.set_i32(t + 0x14, z << 8);
    m.set_u16(t + 0x72, m.u16(near + 0x72));
    m.set_u32(block + 0x18, 0);
    true
}

/// The part of `FUN_08143d48` all kinds share: the traffic type, the sector list, a live-traffic slot, and the
/// car's block (`+0x10/+0x14` the unit direction).
fn finish(m: &mut Mem, t: u32, slot: u32, block: u32) -> u32 {
    // Traffic type from the race's frame counter.
    let types = m.u32(0x0300_625C);
    let mut r = crate::math::umod(m.u32(0x0300_5628), types);
    if r == types {
        r -= 1;
    }
    m.set_u16(t + 0x48, m.u16(TRAFFIC_TYPES + r * 4));
    m.set_u16(t + 0x36, m.u16(TRAFFIC_TYPES + r * 4 + 2));
    m.set_u16(t + 0x7C, r as u16);
    m.set_u16(t + 0x70, 0x200);
    world::link_entity(m, slot);
    if let Some(k) = (0..8).find(|&k| m.u32(SLOTS + 4 * k) == 0) {
        m.set_u32(SLOTS + 4 * k, t);
    }
    for off in [8, 0xC, 0x20, 0x1C] {
        m.set_u32(block + off, 0);
    }
    m.set_i32(block + 0x10, m.i32(t + 0x18));
    m.set_i32(block + 0x14, m.i32(t + 0x20));
    slot
}
