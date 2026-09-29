//! Traffic cars: entity handler 0x36 (`FUN_081443fc`). A traffic car (spawned by `traffic::spawn`) drives its
//! lane of the main route at a fixed crawl, turning towards the next waypoint over 0x20 steps, until it is far
//! from the entity the camera follows; it is not a rigid body. A racer running into it knocks it away (state 3).
//!
//! Entity fields: `+0x18/+0x20` direction (1.0 = 0x1000), `+0x24` speed, `+0x2C`/`+0x32` heading, `+0x30`
//! pitch, `+0x38` heading wobble, `+0x4A` state (0 new, 1 driving, 2 remove, 3 knocked away), `+0x52` speed-up
//! counter, `+0x56` knocked-away timer, `+0x72` route segment, `+0x7C` traffic type, `+0x9A` driving direction
//! along the route (±1), `+0x9C` waypoint, `+0x9E` mode (1 lane driving, 2 follow the waypoints, else stop at
//! the waypoint). The 0x28-byte block at `+0x8C`: `+0x00/+0x04` target point, `+0x08..+0x1C` the turn (start
//! direction, change, progress, steps left), `+0x20` flags (bit 0: pitch set), `+0x24` lane.

use crate::math::{cos, div, isqrt, sin};
use crate::mem::Mem;
use crate::route::CIRCUIT;
use crate::sound::Command;
use crate::traffic::atan2_fast;
use crate::world::{self, NONE, W_ENTITIES, W_QUERY, W_QUERY_SECTOR, W_SECTORS, W_SEGMENTS, W_WALLS, W_WAYPOINTS};
use crate::{Result, Sim, Unported};

/// The live traffic cars (entity addresses, 0 = free) and how many there are (byte).
const SLOTS: u32 = 0x0300_6270;
const COUNT: u32 = 0x0300_6240;
const LANE_OFFSETS: u32 = 0x087F_5488;
/// Per traffic type: collision points (count, then two offsets along the car) and the hit radius.
const POINT_COUNT: u32 = 0x087F_5704;
const POINTS: u32 = 0x087F_5804;
const HIT_RADIUS: u32 = 0x087F_5784;
const AXLE_OFFSETS: u32 = 0x087F_55FC;

/// `FUN_081443fc`: entity handler 0x36.
pub fn handler(sim: &mut Sim, e: u32) -> Result<()> {
    let m = &mut sim.mem;
    let camera = m.u32(W_ENTITIES) + m.u32(0x0300_57F8) * 0xA4;
    let state = m.u16(e + 0x4A);
    let old_sector = m.u16(e + 0x78);
    let blk = m.u32(e + 0x8C);
    if m.u8(0x0300_6298) == 0 {
        return Ok(());
    }
    match state {
        1 => drive(sim, e, camera, old_sector, blk),
        0 => {
            // `FUN_0814f874`: on the floor, no matrix slot.
            let floor = world::floor_height(m, m.u16(e + 0x78) as u32, m.i32(e + 0xC) >> 8, m.i32(e + 0x14) >> 8);
            m.set_i32(e + 0x10, floor);
            m.set_u8(e + 0x88, 0xFF);
            m.set_u16(e + 0x64, 0);
            m.set_u16(e + 0x4A, 1);
            m.set_u16(e + 0xA0, 0);
            Ok(())
        }
        3 => knocked_away(sim, e, camera),
        2 => {
            if blk != 0 {
                crate::heap::free(m, blk);
                m.set_u32(e + 0x8C, 0);
            }
            m.set_u16(e + 8, m.u16(e + 8) & 0xFFFE);
            m.set_u8(e + 0x88, 0xFF);
            world::unlink_entity(m, m.u16(e) as u32);
            m.set_u8(COUNT, m.u8(COUNT).wrapping_sub(1));
            if let Some(k) = (0..8).find(|&k| m.u32(SLOTS + 4 * k) == e) {
                m.set_u32(SLOTS + 4 * k, 0);
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Distance measure used for "far from the camera's car": squared 1/256-scaled city units, absolute value.
fn far(m: &Mem, a: u32, e: u32) -> i32 {
    let dx = ((m.i32(a + 0xC) >> 8) - (m.i32(e + 0xC) >> 8)) >> 8;
    let dz = ((m.i32(a + 0x14) >> 8) - (m.i32(e + 0x14) >> 8)) >> 8;
    dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz)).wrapping_abs()
}

/// State 1: drive along the lane.
fn drive(sim: &mut Sim, e: u32, camera: u32, old_sector: u16, blk: u32) -> Result<()> {
    let m = &mut sim.mem;
    m.set_u32(e + 0x24, 3);
    let c = m.u16(e + 0x52).wrapping_add(1);
    m.set_u16(e + 0x52, c);
    if m.u32(0x0300_624C) <= c as u32 {
        if m.u32(e + 0x24) < m.u32(0x0300_6290) {
            m.set_u32(e + 0x24, m.u32(e + 0x24) + 1);
        }
        m.set_u16(e + 0x52, 0);
    }
    let speed = m.i32(e + 0x24);
    if far(m, camera, e) > 0x2000 {
        m.set_u16(e + 0x4A, 2);
        return Ok(());
    }
    if m.u32(W_SEGMENTS) == 0 {
        // The game would read its target waypoint through a null segment table (BIOS memory).
        return Err(Unported("traffic car without a route (world +0x40 is null)"));
    }
    let seg = m.u32(W_SEGMENTS) + m.u16(e + 0x72) as u32 * 8;
    let wp = |m: &Mem, i: i32| {
        m.u32(W_WAYPOINTS)
            .wrapping_add(m.u32(seg + 4).wrapping_mul(0x18))
            .wrapping_add((i as u32).wrapping_mul(0x18))
    };
    let target = wp(m, m.i16(e + 0x9C) as i32);
    // The turn in progress: the direction moves by 1/0x20 of the change per step.
    if m.i32(blk + 0x1C) != 0 {
        let t = m.i32(blk + 0x18) + 0x80;
        m.set_i32(blk + 0x18, t);
        m.set_i32(blk + 0x1C, m.i32(blk + 0x1C) - 1);
        m.set_i32(e + 0x18, m.i32(blk + 8) + (t.wrapping_mul(m.i32(blk + 0x10)) >> 12));
        m.set_i32(
            e + 0x20,
            m.i32(blk + 0xC) + (m.i32(blk + 0x18).wrapping_mul(m.i32(blk + 0x14)) >> 12),
        );
        let h = atan2_fast(m, m.i32(e + 0x18), m.i32(e + 0x20));
        m.set_u16(e + 0x32, h as u16);
        m.set_i32(e + 0x2C, m.i16(e + 0x32) as i32);
        if m.i32(blk + 0x1C) == 0 {
            let dx = m.i32(blk) - (m.i32(e + 0xC) >> 8);
            let dz = m.i32(blk + 4) - (m.i32(e + 0x14) >> 8);
            let len = isqrt(dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz)) as u32);
            m.set_i32(e + 0x18, div(dx.wrapping_mul(0x1000), len));
            m.set_i32(e + 0x20, div(dz.wrapping_mul(0x1000), len));
        }
    }
    m.set_i32(
        e + 0xC,
        m.i32(e + 0xC).wrapping_add(speed.wrapping_mul(m.i32(e + 0x18))),
    );
    m.set_i32(
        e + 0x14,
        m.i32(e + 0x14).wrapping_add(speed.wrapping_mul(m.i32(e + 0x20))),
    );
    let wobble = m.i32(e + 0x38);
    if wobble == 0 {
        // Ease the displayed heading towards the travel heading by halves.
        let cur = m.i32(e + 0x2C);
        let shown = m.i16(e + 0x32) as i32;
        if shown != cur {
            let d = shown - cur;
            let half = (d >> 1) as i16;
            m.set_i16(
                e + 0x32,
                if d < 0 {
                    (shown as i16).wrapping_add(half)
                } else {
                    (shown as i16).wrapping_sub(half)
                },
            );
        }
    } else {
        m.set_i16(e + 0x32, m.i16(e + 0x32).wrapping_add(wobble as i16));
        m.set_i32(e + 0x38, wobble >> 1);
        if wobble >> 1 == 0 {
            m.set_u32(e + 0x24, 1);
            m.set_u16(e + 0x52, 0);
        }
    }
    if seg == 0 {
        return Ok(());
    }
    let count = m.u16(seg) as i32;
    let d2 = |m: &Mem, x: i32, z: i32| {
        let dx = x.wrapping_mul(0x100).wrapping_sub(m.i32(e + 0xC)) >> 8;
        let dz = z.wrapping_mul(0x100).wrapping_sub(m.i32(e + 0x14)) >> 8;
        dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz)) >> 8
    };
    match m.i16(e + 0x9E) {
        1 => {
            if d2(m, m.i32(blk), m.i32(blk + 4)) < speed.wrapping_mul(0x41A) {
                let mut n = m.i16(e + 0x9C) as i32 + m.i16(e + 0x9A) as i32;
                if m.i32(CIRCUIT) == 0 && (count - 1 <= n || n < 1) {
                    m.set_u16(e + 0x4A, 3);
                    m.set_u16(e + 0x56, 600);
                }
                if m.i32(CIRCUIT) == 0 {
                    n = n.min(count - 1).max(0);
                } else {
                    if count - 1 <= n {
                        n = 0;
                    }
                    if n < 0 {
                        n += count - 1;
                    }
                }
                let next = wp(m, n);
                m.set_i16(e + 0x9C, n as i16);
                let dx = m.i32(next) - m.i32(target);
                let dz = m.i32(next + 4) - m.i32(target + 4);
                let len = isqrt(dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz)) as u32);
                let (ux, uz) = (div(dx.wrapping_mul(0x1000), len), div(dz.wrapping_mul(0x1000), len));
                let lane = m.i32(LANE_OFFSETS.wrapping_add(m.u32(blk + 0x24).wrapping_mul(4)));
                let px = m.i32(next) + (lane.wrapping_mul(uz) >> 8);
                let pz = m.i32(next + 4) - (ux.wrapping_mul(lane) >> 8);
                m.set_i32(blk, px);
                m.set_i32(blk + 4, pz);
                let nx = div((px - m.i32(target)).wrapping_mul(0x1000), len);
                let nz = div((pz - m.i32(target + 4)).wrapping_mul(0x1000), len);
                start_turn(m, e, blk, nx, nz, 0x20);
            }
        }
        2 => {
            if (d2(m, m.i32(target), m.i32(target + 4)) as u32) < (speed.wrapping_mul(m.i32(0x0300_6294))) as u32 {
                let n = m.i16(e + 0x9C) as i32 + 1;
                if n < count {
                    let next = wp(m, n);
                    m.set_i16(e + 0x9C, n as i16);
                    let dx = m.i32(next) - m.i32(target);
                    let dz = m.i32(next + 4) - m.i32(target + 4);
                    let len = isqrt(dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz)) as u32);
                    let (nx, nz) = (div(dx.wrapping_mul(0x1000), len), div(dz.wrapping_mul(0x1000), len));
                    start_turn(m, e, blk, nx, nz, 0x10);
                    m.set_i32(blk, m.i32(next));
                    m.set_i32(blk + 4, m.i32(next + 4));
                } else {
                    m.set_u16(e + 0x4A, 2);
                }
            }
        }
        _ => {
            if (d2(m, m.i32(target), m.i32(target + 4)) as u32) < (speed.wrapping_mul(m.i32(0x0300_6294))) as u32 {
                m.set_u16(e + 0x4A, 2);
            }
        }
    }
    world::unlink_entity(m, m.u16(e) as u32);
    m.set_i32(W_QUERY, m.i32(e + 0xC) >> 8);
    m.set_i32(W_QUERY + 8, m.i32(e + 0x14) >> 8);
    m.set_u16(W_QUERY_SECTOR, m.u16(e + 0x78));
    let s = find_sector(m);
    m.set_u16(e + 0x78, if s == NONE { old_sector } else { s as u16 });
    let sector = m.u16(e + 0x78) as u32;
    let floor = world::floor_height(m, sector, m.i32(e + 0xC) >> 8, m.i32(e + 0x14) >> 8);
    m.set_i32(e + 0x10, floor);
    if sector as u16 != old_sector || m.u32(blk + 0x20) & 1 == 0 {
        m.set_u32(blk + 0x20, m.u32(blk + 0x20) | 1);
        if m.i16(e + 0x9E) == 1 {
            // Pitch from the floor one turn-step ahead.
            let ahead = world::floor_height(
                m,
                sector,
                m.i32(e + 0xC)
                    .wrapping_add(m.i32(blk + 8))
                    .wrapping_add(m.i32(blk + 0x10))
                    >> 8,
                m.i32(e + 0x14)
                    .wrapping_add(m.i32(blk + 0xC))
                    .wrapping_add(m.i32(blk + 0x14))
                    >> 8,
            );
            let pitch = atan2_fast(m, ahead - m.i32(e + 0x10), 0xC00);
            m.set_i16(e + 0x30, (pitch as i16).wrapping_neg());
        }
    }
    world::link_entity(m, m.u16(e) as u32);
    let hit = contact(sim, e)?;
    if hit & 2 == 0 {
        sim.mem.set_u8(e + 0x88, 0xFF);
    }
    Ok(())
}

/// State 3: knocked away (or run off its route): slide with friction (1/32 per step), bounce off solid walls
/// and spin down the wobble, until the timer (`+0x56`, up by 600 / (`*0x03005934` / 0x1C) per step) passes
/// 0x1F4 while unseen, or the car is far from the camera's car; then it is removed.
fn knocked_away(sim: &mut Sim, e: u32, camera: u32) -> Result<()> {
    let m = &mut sim.mem;
    let step = div(600, div(m.i32(0x0300_5934), 0x1C));
    let t = (m.u16(e + 0x56) as i32).wrapping_add(step);
    m.set_u16(e + 0x56, t as u16);
    if t.wrapping_mul(0x1_0000) >= 0x1F4_0001 && m.u16(e + 0xA) & 4 == 0 {
        m.set_u16(e + 0x4A, 2);
        return Ok(());
    }
    if far(m, camera, e) > 1000 {
        m.set_u16(e + 0x4A, 2);
        return Ok(());
    }
    if m.i32(e + 0x18).wrapping_abs() >= 2 || m.i32(e + 0x20).wrapping_abs() > 1 {
        m.set_u16(W_QUERY_SECTOR, m.u16(e + 0x78));
        bounce_off_walls(m, e);
    }
    let (vx, vz) = (m.i32(e + 0x18), m.i32(e + 0x20));
    m.set_i32(e + 0xC, m.i32(e + 0xC).wrapping_add(vx));
    m.set_i32(e + 0x14, m.i32(e + 0x14).wrapping_add(vz));
    let slow = |v: i32| match v - (v >> 5) {
        -1 => 0,
        v => v,
    };
    m.set_i32(e + 0x18, slow(vx));
    m.set_i32(e + 0x20, slow(vz));
    m.set_i32(W_QUERY, m.i32(e + 0xC) >> 8);
    m.set_i32(W_QUERY + 4, m.i32(e + 0x10) >> 8);
    m.set_i32(W_QUERY + 8, m.i32(e + 0x14) >> 8);
    m.set_u16(W_QUERY_SECTOR, m.u16(e + 0x78));
    let wobble = m.i32(e + 0x38);
    if wobble != 0 {
        m.set_i16(e + 0x32, m.i16(e + 0x32).wrapping_add(wobble as i16));
        let w = wobble - (wobble >> 4);
        m.set_i32(e + 0x38, if w.wrapping_abs() < 0x10 { 0 } else { w });
    }
    let old = m.u16(e + 0x78);
    world::unlink_entity(m, m.u16(e) as u32);
    m.set_i32(W_QUERY, m.i32(e + 0xC) >> 8);
    m.set_i32(W_QUERY + 8, m.i32(e + 0x14) >> 8);
    m.set_u16(W_QUERY_SECTOR, m.u16(e + 0x78));
    let s = find_sector(m);
    m.set_u16(e + 0x78, if s == NONE { old } else { s as u16 });
    world::link_entity(m, m.u16(e) as u32);
    let floor = world::floor_height(m, m.u16(e + 0x78) as u32, m.i32(e + 0xC) >> 8, m.i32(e + 0x14) >> 8);
    m.set_i32(e + 0x10, floor);
    contact(sim, e)?;
    sim.mem.set_u8(e + 0x88, 0xFF);
    Ok(())
}

/// `FUN_0814658c`: bounce the knocked-away car's velocity (`+0x18/+0x20`) off the solid walls of its sector
/// within 100 units (the same end tests as the car's walls, `walls::walls`): restitution 0x13/0x400 along the
/// wall normal when moving into it. Returns whether any wall was near.
fn bounce_off_walls(m: &mut Mem, e: u32) -> bool {
    let s = m.u32(W_SECTORS) + m.u16(e + 0x78) as u32 * 0x30;
    let first = m.u32(W_WALLS) + m.u16(s) as u32 * 0x44;
    let count = m.u16(s + 2) as u32;
    let (x, z) = (m.i32(e + 0xC) >> 8, m.i32(e + 0x14) >> 8);
    let mut near = false;
    let (mut w, mut next) = (first + count.wrapping_sub(1).wrapping_mul(0x44), first);
    for _ in 0..count {
        if m.u16(w + 0x2A) as u32 == NONE && m.u16(w + 0x2E) & 0x1000 != 0 {
            let (wx, wz) = (m.i32(w), m.i32(w + 4));
            let (dx, dz) = (x.wrapping_sub(wx), z.wrapping_sub(wz));
            let (nx, nz) = (m.i16(w + 0x34) as i32, m.i16(w + 0x36) as i32);
            if dx.wrapping_mul(nx).wrapping_add(dz.wrapping_mul(nz)) >> 12 <= 100 {
                let (cx, cz) = (m.i32(next), m.i32(next + 4));
                let d2 = |a: i32, b: i32| a.wrapping_mul(a).wrapping_add(b.wrapping_mul(b));
                let past = if (cx - wx).wrapping_mul(dx).wrapping_add(dz.wrapping_mul(cz - wz)) < 0 {
                    d2(dx, dz) > 0x270F
                } else {
                    let (ex, ez) = (x.wrapping_sub(cx), z.wrapping_sub(cz));
                    (wx - cx).wrapping_mul(ex).wrapping_add((wz - cz).wrapping_mul(ez)) < 0 && d2(ex, ez) > 0x270F
                };
                if !past {
                    near = true;
                    let vn = (m.i32(e + 0x18).wrapping_mul(nx) >> 6) + (m.i32(e + 0x20).wrapping_mul(nz) >> 6);
                    if vn < 0 {
                        let j = vn.wrapping_mul(-0x13) >> 10;
                        m.set_i32(e + 0x18, m.i32(e + 0x18) + (nx.wrapping_mul(j) >> 12));
                        m.set_i32(e + 0x20, m.i32(e + 0x20) + (j.wrapping_mul(nz) >> 12));
                    }
                }
            }
        }
        w = next;
        next += 0x44;
    }
    near
}

/// The turn towards direction (`nx`, `nz`) over `steps` steps, from the current direction.
fn start_turn(m: &mut Mem, e: u32, blk: u32, nx: i32, nz: i32, steps: i32) {
    m.set_i32(blk + 8, m.i32(e + 0x18));
    m.set_i32(blk + 0xC, m.i32(e + 0x20));
    m.set_i32(blk + 0x10, nx - m.i32(e + 0x18));
    m.set_i32(blk + 0x14, nz - m.i32(e + 0x20));
    m.set_i32(blk + 0x18, 0);
    m.set_i32(blk + 0x1C, steps);
}

/// `FUN_08144b7c`: the sector of the query point (world `+0xC0/+0xC8`): the query sector, else one through a
/// portal (solid walls only count with material 0, and walls with a dynamic state never count as solid).
fn find_sector(m: &Mem) -> u32 {
    let (x, z) = (m.i32(W_QUERY), m.i32(W_QUERY + 8));
    let start = m.u16(W_QUERY_SECTOR) as u32;
    let sector = |id: u32| m.u32(W_SECTORS) + id * 0x30;
    let walls = |s: u32| m.u32(W_WALLS) + m.u16(s) as u32 * 0x44;
    let s = sector(start);
    if world::inside(m, x, z, walls(s), m.u16(s + 2) as u32, start) != NONE {
        return start;
    }
    let mut w = walls(s);
    for _ in 0..m.u16(s + 2) {
        let solid = if m.u16(w + 0x2A) as u32 == NONE {
            m.u16(w + 0x2E) & 0x1000
        } else {
            0
        };
        let link = m.u16(w + 0x32) as u32;
        if link != NONE && (solid == 0 || m.i16(w + 0x2C) == 0) {
            let t = sector(link);
            let found = world::inside(m, x, z, walls(t), m.u16(t + 2) as u32, link);
            if found != NONE {
                let f = sector(found);
                return if m.i16(f + 8) == 0 && m.u16(f + 0x20) as u32 != NONE {
                    m.u16(f + 0x20) as u32
                } else {
                    found
                };
            }
        }
        w += 0x44;
    }
    NONE
}

/// `FUN_08146094`: test the traffic car against the racers; bit 0 = a racer is near, bit 2 = it was hit (the
/// response `FUN_08145dac` knocks it away; not ported).
fn contact(sim: &mut Sim, e: u32) -> Result<u32> {
    let m = &sim.mem;
    let racers = m.u32(0x0300_57EC);
    let dt = crate::math::recip(m, m.i32(world::DT) << 8).min(0xC00);
    let (vx, vz) = if m.u16(e + 0x4A) == 3 {
        (m.i32(e + 0x18), m.i32(e + 0x20))
    } else {
        (
            m.i32(e + 0x24).wrapping_mul(m.i32(e + 0x18)),
            m.i32(e + 0x24).wrapping_mul(m.i32(e + 0x20)),
        )
    };
    let (vx, vz) = (vx >> 4, vz >> 4);
    let mut result = 0;
    // Once set, the hit flag stays set for the rest of the loop (the game's `local_90`).
    let mut hit = false;
    let mut o = sim.mem.u32(W_ENTITIES);
    for _ in 0..racers.wrapping_add(1) {
        let m = &sim.mem;
        let dx = m.i32(e + 0xC).wrapping_sub(m.i32(o + 0xC)) >> 12;
        let dz = m.i32(e + 0x14).wrapping_sub(m.i32(o + 0x14)) >> 12;
        let d2 = dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz));
        if d2 <= 0x1_D4C0 {
            result |= 1;
            let dy = m.i32(e + 0x10).wrapping_sub(m.i32(o + 0x10));
            let close_y = if dy < 0 {
                m.i32(o + 0x10).wrapping_sub(m.i32(e + 0x10)) <= 0xFFFF
            } else {
                dy <= 0xFFFF
            };
            if close_y && d2 < 0x801 {
                hit |= hits(m, e, o, [vx, vz], dt);
                if hit && respond(sim, o, e)? {
                    let m = &mut sim.mem;
                    let q = m.u32(o + 0x8C);
                    m.set_u32(q + 0x448, m.u32(q + 0x448) | 1);
                    m.set_u16(e + 0x4A, 3);
                    result |= 2;
                    m.set_u16(e + 0x56, 0);
                    if m.i16(0x087F_5924 + m.u16(e + 0x7C) as u32 * 2) != 0 && m.u16(o) as u32 == m.u32(world::PLAYER) {
                        m.set_u32(world::RACE_PHASE, 7);
                    }
                }
            }
        }
        o += 0xA4;
    }
    Ok(result)
}

/// `FUN_08145dac`: racer `o` hits traffic car `t` at the midpoint of their centres: an impulse along the line
/// between them (restitution clamped to −0x16..−0x11), split by the traffic type's shifts (`0x087F5604`,
/// `0x087F5684`) between the traffic car (velocity, wobble `+0x38`) and the racer's body (momentum, spin);
/// the racer's lane, hunter life and the crash sound for the player. Returns whether they were closing.
fn respond(sim: &mut Sim, o: u32, t: u32) -> Result<bool> {
    let m = &mut sim.mem;
    let q = m.u32(o + 0x8C);
    let b = q + 0xC8;
    let dt = crate::math::recip(m, m.i32(world::DT) << 8).min(0xC00);
    let mut n = [
        (m.i32(o + 0xC) - m.i32(t + 0xC)) >> 2,
        0,
        (m.i32(o + 0x14) - m.i32(t + 0x14)) >> 2,
    ];
    crate::math::normalize14(&mut n);
    let n = [n[0] >> 8, 0, n[2] >> 8];
    let point = [
        (m.i32(t + 0xC) + m.i32(o + 0xC)) >> 1,
        (m.i32(t + 0x14) + m.i32(o + 0x14)) >> 1,
    ];
    let r_o = [point[0] - m.i32(o + 0xC), 0, point[1] - m.i32(o + 0x14)];
    let r_t = [point[0] - m.i32(t + 0xC), 0, point[1] - m.i32(t + 0x14)];
    let knocked = m.u16(t + 0x4A) == 3;
    let speed = m.i32(t + 0x24);
    let (tx, tz) = if knocked {
        (m.i32(t + 0x18), m.i32(t + 0x20))
    } else {
        (speed.wrapping_mul(m.i32(t + 0x18)), speed.wrapping_mul(m.i32(t + 0x20)))
    };
    let rel = [
        (dt.wrapping_mul(m.i32(b + crate::body::VEL)) >> 11) - tx,
        (dt.wrapping_mul(m.i32(b + crate::body::VEL + 8)) >> 11) - tz,
    ];
    let vn = (rel[0].wrapping_mul(n[0]) >> 6) + (rel[1].wrapping_mul(n[2]) >> 6);
    if vn >= 0 {
        return Ok(false);
    }
    let k = (-0x11 - ((vn + 0x3000) >> 11)).clamp(-0x16, -0x11);
    if !knocked {
        m.set_i32(t + 0x18, speed.wrapping_mul(m.i32(t + 0x18)));
        m.set_i32(t + 0x20, speed.wrapping_mul(m.i32(t + 0x20)));
    }
    let j = vn.wrapping_mul(k) >> 4;
    let imp = [n[0].wrapping_mul(j) >> 6, 0, n[2].wrapping_mul(j) >> 6];
    let kind = m.u16(t + 0x7C) as u32;
    let shift = m.u32(0x087F_5604 + kind * 4) & 0xFF;
    let asr = |v: i32, s: u32| crate::math::asr(v, s);
    if knocked {
        m.set_i32(t + 0x18, m.i32(t + 0x18) - asr(imp[0], shift));
        m.set_i32(t + 0x20, m.i32(t + 0x20) - asr(imp[2], shift));
    } else {
        m.set_i32(t + 0x18, m.i32(t + 0x18) - div(asr(imp[0], shift), speed));
        m.set_i32(t + 0x20, m.i32(t + 0x20) - div(asr(imp[2], shift), speed));
    }
    let spin = crate::math::cross(r_t, imp);
    let wobble_shift = m.u32(0x087F_5604 + kind * 4).wrapping_add(0xD) & 0xFF;
    m.set_i32(t + 0x38, m.i32(t + 0x38).wrapping_add(asr(spin[1], wobble_shift)));
    if j > 0x4000 {
        m.set_u32(q + 0x4B4, m.u32(q + 0x4B4) | 4);
    }
    if m.u16(o) as u32 > m.u32(crate::route::OPPONENTS) && m.i32(0x0300_61F0) != 0 {
        m.set_u32(0x0300_61F0, 0);
    }
    if m.i32(0x0300_56E0) == 2 && m.u16(o + 0x4A) != 2 {
        // `FUN_081413b0`: hunter races: the hit costs the racer hunter life.
        let v = m.i32(q + 0x4E8) - (m.i32(0x0300_61A0).wrapping_mul(j) >> 8);
        m.set_i32(q + 0x4E8, v.max(0));
        m.set_u16(q + 0x4F0, 0);
    }
    let shift = m.u32(0x087F_5684 + kind * 4) & 0xFF;
    let imp = [asr(imp[0], shift), 0, asr(imp[2], shift)];
    let mom = crate::body::MOMENTUM;
    m.set_i32(b + mom, m.i32(b + mom).wrapping_add(imp[0]));
    m.set_i32(b + mom + 8, m.i32(b + mom + 8).wrapping_add(imp[2]));
    let turn = crate::math::cross(r_o, [imp[0] >> 8, 0, imp[2] >> 8]);
    let ang = b + crate::body::ANG_MOMENTUM;
    m.set_vec3(ang, crate::math::sub(m.vec3(ang), turn));
    crate::body::update_velocities(m, b);
    let lane = crate::route::nearest_lane(m, crate::route::lateral(m, o), -1);
    m.set_u16(q + 0xC0, lane as u16);
    if m.u16(o) as u32 == m.u32(world::PLAYER) {
        sim.sounds.push(Command::Stop(0x15));
        sim.sounds.push(Command::Stop(0x16));
        if j > 0x800 {
            sim.sounds.push(Command::Play(if j < 0x5001 { 0x16 } else { 0x15 }));
        }
    }
    Ok(true)
}

fn sub2(a: [i32; 2], b: [i32; 2]) -> [i32; 2] {
    [a[0].wrapping_sub(b[0]), a[1].wrapping_sub(b[1])]
}

fn dot2(a: [i32; 2], b: [i32; 2]) -> i32 {
    a[0].wrapping_mul(b[0]).wrapping_add(a[1].wrapping_mul(b[1]))
}

/// The squared distance of the closest approach when the gap goes from `d0` to `d1` and closes, else `None`.
fn closest(d0: [i32; 2], d1: [i32; 2]) -> Option<i32> {
    let dd = sub2(d1, d0);
    if dot2(dd, d0) >= 1 {
        return None;
    }
    let t = dot2(dd, d1);
    if t < 0 {
        return Some(dot2(d1, d1));
    }
    let l = dot2(dd, dd);
    if l == 0 {
        return None;
    }
    let c = [
        d1[0].wrapping_sub(div(dd[0].wrapping_mul(t), l)),
        d1[1].wrapping_sub(div(dd[1].wrapping_mul(t), l)),
    ];
    Some(dot2(c, c))
}

/// The swept test of traffic car `e` (moving by `v`) against racer `o` over the frame: first the centres, then
/// the traffic type's collision points against the racer's two axle points (`FUN_08146094`).
fn hits(m: &Mem, e: u32, o: u32, v: [i32; 2], dt: i32) -> bool {
    let q = m.u32(o + 0x8C);
    let a0 = [m.i32(e + 0xC) >> 4, m.i32(e + 0x14) >> 4];
    let a1 = [a0[0].wrapping_add(v[0]), a0[1].wrapping_add(v[1])];
    let b0 = [m.i32(o + 0xC) >> 4, m.i32(o + 0x14) >> 4];
    let b1 = [
        (dt.wrapping_mul(m.i32(q + 0x11C)) >> 15).wrapping_add(b0[0]),
        (dt.wrapping_mul(m.i32(q + 0x124)) >> 15).wrapping_add(b0[1]),
    ];
    if !closest(sub2(b0, a0), sub2(b1, a1)).is_some_and(|d| d < 0xF4_2400) {
        return false;
    }
    let kind = m.u16(e + 0x7C) as u32;
    let points = m.i32(POINT_COUNT + kind * 4);
    let heading = m.i16(e + 0x32) as i32;
    let mut hit = false;
    for i in 0..points.max(0) as u32 {
        let off = m.i32(POINTS + kind * 8 + i * 4);
        let ox = off.wrapping_mul(sin(m, heading)) >> 14;
        let oz = off.wrapping_mul(cos(m, heading)) >> 14;
        let (c0, c1) = ([a0[0] + ox, a0[1] + oz], [a1[0] + ox, a1[1] + oz]);
        for j in 0..2 {
            let axle = m.i32(AXLE_OFFSETS + j * 4);
            let rx = m.i32(q + 0x140).wrapping_mul(axle) >> 12;
            let rz = m.i32(q + 0x148).wrapping_mul(axle) >> 12;
            let (r0, r1) = ([b0[0] + rx, b0[1] + rz], [b1[0] + rx, b1[1] + rz]);
            let radius = m.i32(HIT_RADIUS + kind * 4);
            hit |= closest(sub2(r0, c0), sub2(r1, c1)).is_some_and(|d| d < radius);
        }
    }
    hit
}
