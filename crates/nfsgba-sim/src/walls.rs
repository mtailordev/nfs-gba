//! Collisions: the car against the sector walls (`FUN_08145ca8`, `FUN_081459b8`, `FUN_081457b8`) and the
//! proximity test against the other racers (`FUN_08145320`).
//!
//! Wall records (0x44 bytes, `world::W_WALLS`): `+0x00/+0x04` corner x/z, `+0x2A` dynamic state or 0xFFFF,
//! `+0x2E` flags (0x1000 solid, 0x4000 remembered at physics `+0x4C0`), `+0x32` neighbour sector through the
//! portal or 0xFFFF, `+0x34/+0x36` inward normal x/z (1.0 = 0x1000), `+0x38` floor height.

use crate::body;
use crate::math::{cross, div, udiv};
use crate::mem::Mem;
use crate::route::{is_player, lateral, nearest_lane};
use crate::sound::Command;
use crate::world::{NONE, PROFILE, W_WALL_STATES, sector_addr, wall_addr, wall_flags};
use crate::{Result, Sim, Unported};

/// Walls in the neighbouring sector are only tested this far (city units squared) past a wall's ends.
const END_RADIUS2: i32 = 0x270F;
/// The racers (player plus opponents) are the first entities; cars at or above this index are not checked.
const RACERS: u32 = 0x0300_57EC;
/// Car-to-car test: wheel-pair offsets along the car, and the hit radii (1/16 city units squared).
const AXLE_OFFSETS: u32 = 0x087F_55FC;
/// Partner table for breakable walls: the dynamic state that breaks together with each state.
const BREAK_PARTNERS: u32 = 0x087F_3CDA;

/// `FUN_08145ca8`: test the point 3/256 of the car's forward axis ahead of it against the walls, and for the
/// player start the scrape or crash sound on the first contact.
pub fn collide(sim: &mut Sim, e: u32) -> Result<()> {
    let m = &mut sim.mem;
    let p = m.u32(e + 0x8C);
    let x = m.i32(e + 0xC).wrapping_add(m.i32(p + 0x140) * 3) >> 8;
    let z = m.i32(e + 0x14).wrapping_add(m.i32(p + 0x148) * 3) >> 8;
    let (y, sector) = (m.i32(e + 0x10) >> 8, m.u16(e + 0x78) as u32);
    let hit = walls(sim, e, x, z, y, sector, true)?;
    let m = &mut sim.mem;
    let player = is_player(m, m.u16(e) as u32);
    let contact = m.u32(PROFILE) + 0x2EC;
    if hit == 0 {
        if player {
            m.set_u8(contact, 0);
        }
    } else if player && m.u8(contact) != 1 {
        m.set_u8(contact, 1);
        if hit < 0x4001 {
            let force = hit - 0x800;
            if force > 0 {
                let volume = m.u32(0x0300_53A4);
                let v = udiv((force as u32).wrapping_mul(volume), 0x3800);
                sim.sounds.push(Command::Stop(0x16));
                let pitch = div(force * 0xDB8, 0x3800);
                sim.sounds.push(Command::Start {
                    sample: 0x16,
                    pitch: pitch + 0x1B58,
                    channel: 3,
                    volume: v + volume * 3,
                });
            }
        } else {
            sim.sounds.push(Command::Stop(0x15));
            sim.sounds.push(Command::Play(0x15));
        }
    }
    Ok(())
}

/// `FUN_081459b8`: test (`x`, `z`) against the walls of `sector`, recursing once through open portals; returns
/// the largest impulse applied (0 for none). The unused `_y` is the game's fifth argument.
fn walls(sim: &mut Sim, e: u32, x: i32, z: i32, _y: i32, sector: u32, recurse: bool) -> Result<i32> {
    let m = &mut sim.mem;
    let p = m.u32(e + 0x8C);
    let s = sector_addr(m, sector);
    let first = wall_addr(m, m.u16(s) as u32);
    let count = m.u16(s + 2) as u32;
    let mut best = 0;
    m.set_u32(p + 0x4C0, 0);
    // Each wall runs from its corner to the next wall's corner.
    let mut prev = first + count.wrapping_sub(1).wrapping_mul(0x44);
    let mut next = first;
    for _ in 0..count {
        let w = prev;
        let m = &mut sim.mem;
        let flags = wall_flags(m, w);
        let (wx, wz, nx, nz) = (m.i32(w), m.i32(w + 4), m.i32(next), m.i32(next + 4));
        let (dx, dz) = (x.wrapping_sub(wx), z.wrapping_sub(wz));
        let near = |m: &Mem| {
            let dist = (m.i16(w + 0x34) as i32)
                .wrapping_mul(dx)
                .wrapping_add((m.i16(w + 0x36) as i32).wrapping_mul(dz))
                >> 12;
            if dist > 100 {
                return false;
            }
            if (nx - wx).wrapping_mul(dx).wrapping_add((nz - wz).wrapping_mul(dz)) < 0 {
                return dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz)) <= END_RADIUS2;
            }
            let (ex, ez) = (x.wrapping_sub(nx), z.wrapping_sub(nz));
            !((wx - nx).wrapping_mul(ex).wrapping_add((wz - nz).wrapping_mul(ez)) < 0
                && ex.wrapping_mul(ex).wrapping_add(ez.wrapping_mul(ez)) > END_RADIUS2)
        };
        if flags & 0x1000 != 0 {
            if near(m) {
                if flags & 0x4000 != 0 {
                    m.set_u32(p + 0x4C0, w);
                }
                let n = [m.i16(w + 0x34) as i32, 0, m.i16(w + 0x36) as i32];
                let point = [m.i32(e + 0xC) + n[0] * 4, m.i32(e + 0x10), m.i32(e + 0x14) + n[2] * 4];
                let j = respond(sim, e, point, n, w)?;
                best = best.max(j);
            }
        } else if recurse && near(m) {
            let mut link = m.u16(w + 0x32) as u32;
            if link != NONE {
                let t = sector_addr(m, link);
                if m.u16(t + 8) == 0 && m.u16(t + 0x20) as u32 != NONE {
                    link = m.u16(t + 0x20) as u32;
                }
                let j = walls(sim, e, x, z, _y, link, false)?;
                best = best.max(j);
            }
        }
        prev = next;
        next += 0x44;
    }
    Ok(best)
}

/// `FUN_081457b8`: push the car off wall `w` (normal `n`) at `point`: an impulse against the contact
/// velocity (restitution from a clamped linear map), damage and side flags, the lane, spin about y.
fn respond(sim: &mut Sim, e: u32, point: [i32; 3], n: [i32; 3], w: u32) -> Result<i32> {
    let m = &mut sim.mem;
    let p = m.u32(e + 0x8C);
    let b = p + 0xC8;
    let pos = m.vec3(b + body::POS);
    let r = [point[0] - pos[0], 0, point[2] - pos[2]];
    let spin = [0, m.i32(b + body::ANG_MOMENTUM + 4) >> 4, 0];
    let c = cross(r, spin);
    let vx = (c[0] >> 14) + m.i32(b + body::VEL);
    let vz = (c[2] >> 14) + m.i32(b + body::VEL + 8);
    let vn = (n[0].wrapping_mul(vx) >> 6) + (n[2].wrapping_mul(vz) >> 6);
    if vn >= 0 {
        return Ok(0);
    }
    let k = (-(vn.wrapping_mul(0x1A2).wrapping_add(0x19D1_0000)) >> 24).clamp(-25, -16);
    let mut j = k * vn >> 10;
    let mut broke = false;
    if j > 0x2000 {
        let state = m.u16(w + 0x2A) as u32;
        if state != NONE && m.i16(m.u32(W_WALL_STATES) + state * 0x20 + 0x16) == 1 {
            break_wall(m, w);
            j = 0x2000;
            broke = true;
        }
    }
    let impulse = [n[0] * j >> 12, 0, n[2] * j >> 12];
    if j > 0x4000 {
        m.set_u32(p + 0x4B4, m.u32(p + 0x4B4) | 1);
    }
    if m.u16(e) as u32 > m.u32(crate::route::OPPONENTS) && m.i32(0x0300_61F0) != 0 {
        // `FUN_081410a8` / `FUN_08141284`: a non-racer (traffic, cops) hitting a wall.
        m.set_i32(0x0300_61F0, 0);
    }
    m.set_u32(p + 0x448, m.u32(p + 0x448) | 0x10);
    if m.i32(0x0300_56E0) == 2 && !broke {
        // `FUN_0814136c` (`hunter_hit`): hunter races lose hunter life on wall hits.
        if m.u16(e + 0x4A) != 2 {
            let v = m.i32(p + 0x4E8) - (m.i32(0x0300_6184).wrapping_mul(j) >> 8);
            m.set_i32(p + 0x4E8, v.max(0));
            m.set_u16(p + 0x4F0, 0);
        }
    }
    if n[2].abs() <= 0x7FF {
        m.set_u32(p + 0x448, m.u32(p + 0x448) | if n[0] < 1 { 4 } else { 2 });
    }
    let lane = nearest_lane(m, lateral(m, e), -1);
    m.set_u16(p + 0xC0, lane as u16);
    m.set_i32(b + body::MOMENTUM, m.i32(b + body::MOMENTUM) + impulse[0]);
    m.set_i32(b + body::MOMENTUM + 8, m.i32(b + body::MOMENTUM + 8) + impulse[2]);
    let t = cross(r, impulse).map(|c| c >> 12);
    let l = m.vec3(b + body::ANG_MOMENTUM);
    m.set_vec3(b + body::ANG_MOMENTUM, crate::math::add(l, t));
    body::update_velocities(m, b);
    Ok(j)
}

/// `FUN_0813b5a0`: a breakable wall gives way: its dynamic state and its partner lose the solid flag (and
/// bit 0) and count one more hit.
fn break_wall(m: &mut Mem, w: u32) {
    let state = m.u16(w + 0x2A) as u32;
    if state == NONE {
        return;
    }
    let states = m.u32(W_WALL_STATES);
    let s = states + state * 0x20;
    let partner = m.i16(BREAK_PARTNERS + state * 2);
    if m.u16(s + 0xE) & 0x1000 != 0 && partner != -1 {
        for s in [s, states.wrapping_add((partner as i32 as u32).wrapping_mul(0x20))] {
            m.set_u16(s + 0xE, m.u16(s + 0xE) & 0xEFFE);
            m.set_i16(s + 0xC, m.i16(s + 0xC).wrapping_add(1));
        }
    }
}

/// `FUN_08145320`: the swept proximity test against the racers after this one; a hit needs the car-to-car
/// response (`FUN_08144fa4`), which is not ported.
pub fn racers(sim: &mut Sim, e: u32, dt: i32) -> Result<()> {
    let m = &sim.mem;
    let racers = m.u32(RACERS);
    if m.u16(e) as u32 >= racers {
        return Ok(());
    }
    let p = m.u32(e + 0x8C);
    let (mut mask, mut bit) = (0i32, 1i32);
    let mut o = e + 0xA4;
    loop {
        let state = m.u16(o + 0x4A);
        if m.u16(o + 8) & 4 != 0 && (state == 1 || state == 2 || state > 0xFF) {
            let dy = m.i32(e + 0x10).wrapping_sub(m.i32(o + 0x10));
            let close_y = if dy < 0 {
                m.i32(o + 0x10).wrapping_sub(m.i32(e + 0x10)) <= 0xFFFF
            } else {
                dy <= 0xFFFF
            };
            if close_y {
                let dx = m.i32(e + 0xC).wrapping_sub(m.i32(o + 0xC)) >> 12;
                let dz = m.i32(e + 0x14).wrapping_sub(m.i32(o + 0x14)) >> 12;
                if (dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz)) as u32) < 0x301 {
                    let q = m.u32(o + 0x8C);
                    let a0 = [m.i32(e + 0xC) >> 4, m.i32(e + 0x14) >> 4];
                    let a1 = [
                        (dt.wrapping_mul(m.i32(p + 0x11C)) >> 16) + a0[0],
                        (dt.wrapping_mul(m.i32(p + 0x124)) >> 16) + a0[1],
                    ];
                    let b0 = [m.i32(o + 0xC) >> 4, m.i32(o + 0x14) >> 4];
                    let b1 = [
                        (dt.wrapping_mul(m.i32(q + 0x11C)) >> 16) + b0[0],
                        (dt.wrapping_mul(m.i32(q + 0x124)) >> 16) + b0[1],
                    ];
                    if swept_close(sub2(b0, a0), sub2(b1, a1), 0xF4_2400) {
                        for i in 0..2u32 {
                            let off = m.i32(AXLE_OFFSETS + 4 * i);
                            let ax = m.i32(p + 0x140).wrapping_mul(off) >> 12;
                            let az = m.i32(p + 0x148).wrapping_mul(off) >> 12;
                            let (ea0, ea1) = ([a0[0] + ax, a0[1] + az], [a1[0] + ax, a1[1] + az]);
                            for j in 0..2u32 {
                                let off = m.i32(AXLE_OFFSETS + 4 * j);
                                let bx = m.i32(q + 0x140).wrapping_mul(off) >> 12;
                                let bz = m.i32(q + 0x148).wrapping_mul(off) >> 12;
                                let (eb0, eb1) = ([b0[0] + bx, b0[1] + bz], [b1[0] + bx, b1[1] + bz]);
                                if swept_close(sub2(eb0, ea0), sub2(eb1, ea1), 0x33_A900) {
                                    mask += bit;
                                }
                                bit <<= 1;
                            }
                        }
                    }
                    if mask != 0 {
                        return Err(Unported("FUN_08144fa4 (car-to-car collision response)"));
                    }
                }
            }
        }
        if racers <= m.u16(o) as u32 {
            return Ok(());
        }
        o += 0xA4;
    }
}

fn sub2(a: [i32; 2], b: [i32; 2]) -> [i32; 2] {
    [a[0].wrapping_sub(b[0]), a[1].wrapping_sub(b[1])]
}

/// Whether the gap going from `d0` to `d1` closes and comes within `radius2` (the nearest point of the
/// segment d0..d1 to the origin, `FUN_08145320`).
fn swept_close(d0: [i32; 2], d1: [i32; 2], radius2: i32) -> bool {
    let dd = sub2(d1, d0);
    let dot = |a: [i32; 2], b: [i32; 2]| a[0].wrapping_mul(b[0]).wrapping_add(a[1].wrapping_mul(b[1]));
    if dot(dd, d0) >= 1 {
        return false;
    }
    let t = dot(dd, d1);
    let (cx, cz) = if t < 0 {
        (d1[0], d1[1])
    } else {
        let l = dot(dd, dd);
        if l == 0 {
            return false;
        }
        (
            d1[0] - div(dd[0].wrapping_mul(t), l),
            d1[1] - div(dd[1].wrapping_mul(t), l),
        )
    };
    cx.wrapping_mul(cx).wrapping_add(cz.wrapping_mul(cz)) < radius2
}
