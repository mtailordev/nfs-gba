//! Wheels on the ground (`FUN_08147ec4`): suspension, tyre friction and wheel spin for the four wheels.
//!
//! Wheel `i` lives at physics struct `+0x18C + 0x94 * i`: `+0x0C` contact point (body-relative), `+0x18`
//! wheel position rotated into world axes, `+0x48` wheel position in car axes, `+0x64` wheel spin, `+0x68`
//! spring rate, `+0x6C` damping, `+0x7C` squared slip speed, `+0x80` grip, `+0x90` sector under the wheel.
//! Wheels 0 and 1 steer.

use crate::body;
use crate::math::{add, cos, isqrt, mat_mul, mul12, recip, sin};
use crate::mem::Mem;
use crate::route::is_player;
use crate::sound::Command;
use crate::world::{NONE, W_QUERY, W_QUERY_SECTOR, floor_height, floor_sector};
use crate::{Result, Sim};

pub const WHEELS: u32 = 0x18C;
pub const WHEEL_SIZE: u32 = 0x94;
/// Grip per floor surface (sector `+0x08`, capped at 7).
const SURFACE_GRIP: u32 = 0x087F_5904;
/// Ride height: added to every wheel's height before the floor test.
const RIDE_HEIGHT: u32 = 0x204;

/// The eight body corners the tipped-over car rests on (`0x087F2528`, 3 words each, car axes).
const CORNERS: u32 = 0x087F_2528;

/// `FUN_081484f0`: the car tipped over (rotation `+0x138` below 0xF21): instead of the wheels, each of eight body
/// corners that is below its floor pushes back (load from depth and the floor-normal speed) with friction capped
/// by the surface grip. Clears the wheels' slip (`+0x7C`), counts the steps without a corner down (`+0x4E6`),
/// plays the landing sound when the player's car comes down, and updates velocity and angular velocity from the
/// momenta. The callers then zero the wheel spins and count the tipped steps (`+0x4E4`).
pub fn tipped(sim: &mut Sim, e: u32, dt: i32) {
    let m = &mut sim.mem;
    let p = m.u32(e + 0x8C);
    let b = p + 0xC8;
    for k in 0..4 {
        m.set_i32(p + WHEELS + WHEEL_SIZE * k + 0x7C, 0);
    }
    let sector = m.u16(e + 0x78) as u32;
    let rot: [i32; 9] = std::array::from_fn(|k| m.i32(p + 0x128 + 4 * k as u32));
    let mut down = 0;
    for i in 0..8 {
        let corner = mat_mul(m.vec3(CORNERS + 12 * i), &rot);
        let pos = m.vec3(b + body::POS);
        m.set_i32(W_QUERY, corner[0].wrapping_add(pos[0]) >> 8);
        m.set_i32(W_QUERY + 4, corner[1].wrapping_add(pos[1]) >> 8);
        m.set_i32(W_QUERY + 8, corner[2].wrapping_add(pos[2]) >> 8);
        m.set_u16(W_QUERY_SECTOR, sector as u16);
        let found = crate::world::find_sector_near_query(m);
        let mut s = if found == NONE { sector } else { found };
        let (qx, qz) = (m.i32(W_QUERY), m.i32(W_QUERY + 8));
        let depth = |m: &Mem, s: u32| {
            let h = floor_height(m, s, qx, qz);
            m.i32(p + 0xD4)
                .wrapping_add(corner[1])
                .wrapping_sub(h.wrapping_sub(0x2000))
        };
        let mut pen = depth(m, s);
        if pen > 0x8000 {
            s = sector;
            pen = depth(m, s);
        }
        if pen <= 0 {
            continue;
        }
        down += 1;
        let arm = [corner[0], corner[1].wrapping_add(0x2000).wrapping_sub(pen), corner[2]];
        let w = m.vec3(p + 0x158);
        let spin = [
            mul12(arm[1], w[2]).wrapping_sub(mul12(w[1], arm[2])),
            mul12(arm[2], w[0]).wrapping_sub(mul12(w[2], arm[0])),
            mul12(arm[0], w[1]).wrapping_sub(mul12(w[0], arm[1])),
        ];
        let v = add(m.vec3(p + 0x11C), spin);
        let f = floor_sector(m, s);
        let surface = (m.u16(f + 8) as u32).min(7);
        let mut n = [m.i16(f + 0x14) as i32, m.i16(f + 0x16) as i32, m.i16(f + 0x18) as i32];
        if n[1] < 1 {
            if n[1] == 0 {
                n[1] = 0x1000;
            } else {
                n = n.map(|c| -c);
            }
        }
        let vn = mul12(v[0], n[0])
            .wrapping_add(mul12(v[1], n[1]))
            .wrapping_add(mul12(v[2], n[2]));
        let damp = if v[1] < 1 {
            vn.wrapping_mul(5) >> 5
        } else {
            vn.wrapping_mul(15) >> 6
        };
        let load = ((pen.wrapping_mul(0x30) >> 8).wrapping_add(damp)).max(0);
        let (mut fx, mut fz) = (0, 0);
        let sq = v[0].wrapping_mul(v[0]).wrapping_add(v[2].wrapping_mul(v[2]));
        if sq != 0 {
            let grip = ((load << 10 >> 8).wrapping_mul(m.i32(SURFACE_GRIP + surface * 4)) >> 8).min(0x4000);
            if sq > grip.wrapping_mul(grip) {
                let r = recip(m, isqrt(sq as u32));
                let (ux, uz) = (mul12(r, v[0]), mul12(r, v[2]));
                let k = -(grip >> 4);
                (fx, fz) = (mul12(ux, k), mul12(uz, k));
            } else {
                (fx, fz) = (v[0].wrapping_neg() >> 4, v[2].wrapping_neg() >> 4);
            }
        }
        let f = [mul12(fx, dt), mul12(-load, dt), mul12(fz, dt)];
        m.set_vec3(b + body::MOMENTUM, add(m.vec3(b + body::MOMENTUM), f));
        // Torque arm × force; only its x component is quartered (reproduced as is).
        let torque = [
            mul12(arm[1], f[2]).wrapping_sub(mul12(f[1], arm[2])) >> 2,
            mul12(arm[2], f[0]).wrapping_sub(mul12(f[2], arm[0])),
            mul12(arm[0], f[1]).wrapping_sub(mul12(arm[1], f[0])),
        ];
        m.set_vec3(
            b + body::ANG_MOMENTUM,
            crate::math::sub(m.vec3(b + body::ANG_MOMENTUM), torque),
        );
    }
    let airborne = p + 0x4E6;
    if down == 0 {
        m.set_u16(airborne, m.u16(airborne).wrapping_add(1));
    } else {
        let steps = m.i16(airborne);
        if steps > 4 && is_player(m, m.u16(e) as u32) {
            sim.sounds.push(Command::Stop(0x19));
            sim.sounds.push(Command::Stop(0x16));
            sim.sounds.push(Command::Play(if steps > 8 { 0x19 } else { 0x16 }));
        }
        sim.mem.set_u16(airborne, 0);
    }
    let m = &mut sim.mem;
    let inv_mass = m.i32(p + 0xCC);
    m.set_vec3(p + 0x11C, m.vec3(b + body::MOMENTUM).map(|c| mul12(inv_mass, c)));
    let inv_inertia = m.i32(p + 0x178);
    m.set_vec3(p + 0x158, m.vec3(b + body::ANG_MOMENTUM).map(|c| mul12(inv_inertia, c)));
}

/// Returns the number of wheels touching the ground.
pub fn wheels(sim: &mut Sim, e: u32, dt: i32) -> Result<i32> {
    let m = &mut sim.mem;
    let p = m.u32(e + 0x8C);
    let b = p + 0xC8;
    let mut grounded = 0;
    let steer_shift = if m.i32(p + 0x40) == 0 { 9 } else { 8 };
    let sector = m.u16(e + 0x78) as u32;
    let h = floor_height(m, sector, m.i32(p + 0xD0) >> 8, m.i32(p + 0xD8) >> 8);
    // Below the floor and still falling: drop the vertical momentum's last velocity step.
    if m.i32(p + 0xD4) - h > 0x800 && m.i32(p + 0x120) > 0 {
        let my = m.i32(p + 0xFC) - m.i32(p + 0x120);
        m.set_i32(p + 0xFC, my);
        m.set_i32(p + 0x120, mul12(m.i32(p + 0xCC), my));
    }
    let (rear_x, rear_z) = (m.i32(p + 0x128), m.i32(p + 0x130));
    let c = cos(m, m.i32(p + 0x20) >> steer_shift);
    let s = sin(m, m.i32(p + 0x20) >> steer_shift);
    // The wheel's rolling direction: the car's x axis turned by the steering angle.
    let mut fx = ((c >> 2) * m.i32(p + 0x128) + m.i32(p + 0x140) * (-s >> 2)) >> 12;
    let mut fz = ((c >> 2) * m.i32(p + 0x130) + m.i32(p + 0x148) * (-s >> 2)) >> 12;
    m.set_i32(p + 0x3DC, 0);
    let rot: [i32; 9] = std::array::from_fn(|k| m.i32(p + 0x128 + 4 * k as u32));
    for i in 0..4 {
        let w = p + WHEELS + WHEEL_SIZE * i;
        m.set_i32(w + 0x7C, 0);
        let local = m.vec3(w + 0x48);
        let world_pos =
            [0, 1, 2].map(|j| mul12(local[0], rot[j]) + mul12(rot[3 + j], local[1]) + mul12(rot[6 + j], local[2]));
        m.set_vec3(w + 0x18, world_pos);
        if i == 2 {
            fx = rear_x;
            fz = rear_z;
        }
        let pos = m.vec3(b + body::POS);
        m.set_i32(W_QUERY, (world_pos[0] + pos[0]) >> 8);
        m.set_i32(W_QUERY + 4, (world_pos[1] + pos[1]) >> 8);
        m.set_i32(W_QUERY + 8, (world_pos[2] + pos[2]) >> 8);
        m.set_u16(W_QUERY_SECTOR, sector as u16);
        let found = crate::world::find_sector_near_query(m);
        m.set_u32(w + 0x90, if found == NONE { sector } else { found });
        let (qx, qz) = (m.i32(W_QUERY), m.i32(W_QUERY + 8));
        let depth = |m: &Mem| {
            let h = floor_height(m, m.u32(w + 0x90), qx, qz);
            m.i32(p + 0xD4) + world_pos[1] + m.i32(p + RIDE_HEIGHT) - h
        };
        let mut pen = depth(m);
        if pen > 0x8000 {
            m.set_u32(w + 0x90, sector);
            pen = depth(m);
        }
        if pen > 0 {
            grounded += 1;
            let contact = [world_pos[0], world_pos[1] + m.i32(p + RIDE_HEIGHT) - pen, world_pos[2]];
            m.set_vec3(w + 0x0C, contact);
            let omega = m.vec3(p + 0x158);
            let spin = [
                mul12(omega[2], contact[1]) - mul12(omega[1], contact[2]),
                mul12(omega[0], contact[2]) - mul12(omega[2], contact[0]),
                mul12(omega[1], contact[0]) - mul12(omega[0], contact[1]),
            ];
            let mut vx = m.i32(p + 0x11C) + spin[0];
            let vy = m.i32(p + 0x120) + spin[1];
            let mut vz = m.i32(p + 0x124) + spin[2];
            let fs = floor_sector(m, m.u32(w + 0x90));
            let surface = (m.u16(fs + 8) as u32).min(7);
            let (mut nx, mut ny, mut nz) = (
                m.i16(fs + 0x14) as i32,
                m.i16(fs + 0x16) as i32,
                m.i16(fs + 0x18) as i32,
            );
            if ny < 1 {
                if ny == 0 {
                    ny = 0x1000;
                } else {
                    (nx, ny, nz) = (-nx, -ny, -nz);
                }
            }
            let damping = if vy < 1 { m.i32(w + 0x6C) - 10 } else { m.i32(w + 0x6C) };
            let mut load = (m.i32(w + 0x68).wrapping_mul(pen) >> 8)
                + ((mul12(nx, vx) + mul12(vy, ny) + mul12(vz, nz)).wrapping_mul(damping) >> 7);
            if load < 0 {
                load = 0;
            }
            let (mut ax, mut az) = (0, 0);
            let wheel_spin = m.i32(w + 0x64);
            vx += (wheel_spin >> 7) * fz >> 12;
            vz += -fx * (wheel_spin >> 7) >> 12;
            let slip2 = vx.wrapping_mul(vx).wrapping_add(vz.wrapping_mul(vz));
            if slip2 != 0 {
                m.set_i32(w + 0x7C, slip2 >> 8);
                let grip =
                    (m.i32(w + 0x80).wrapping_mul(load) >> 8).wrapping_mul(m.i32(SURFACE_GRIP + surface * 4)) >> 8;
                let grip = grip.min(0x4000);
                if grip * grip < slip2 {
                    let r = recip(m, isqrt(slip2 as u32));
                    let (ux, uz) = (mul12(r, vx), mul12(r, vz));
                    ax = -(grip >> 4) * ux >> 12;
                    az = -(grip >> 4) * uz >> 12;
                } else {
                    ax = -vx >> 4;
                    az = -vz >> 4;
                }
                m.set_i32(w + 0x64, wheel_spin + (dt * (mul12(ax, fz) + mul12(az, -fx)) >> 1));
            }
            let ix = dt * ax >> 12;
            let iy = dt * -load >> 12;
            let iz = dt * az >> 12;
            m.set_i32(p + 0xF8, m.i32(p + 0xF8) + ix);
            m.set_i32(p + 0xFC, m.i32(p + 0xFC) + iy);
            m.set_i32(p + 0x100, m.i32(p + 0x100) + iz);
            let [cx, cy, cz] = contact;
            m.set_i32(p + 0x110, m.i32(p + 0x110) - (mul12(iz, cy) - mul12(iy, cz)));
            m.set_i32(p + 0x114, m.i32(p + 0x114) - (mul12(ix, cz) - mul12(iz, cx)));
            m.set_i32(p + 0x118, m.i32(p + 0x118) - (mul12(iy, cx) - mul12(ix, cy)));
        }
        m.set_i32(p + 0x3DC, m.i32(p + 0x3DC) + m.i32(w + 0x64));
    }
    if grounded == 0 {
        m.set_i16(p + 0x4E6, m.i16(p + 0x4E6) + 1);
    } else {
        let airborne = m.i16(p + 0x4E6);
        if airborne > 4 && is_player(m, m.u16(e) as u32) {
            sim.sounds.push(Command::Stop(0x19));
            sim.sounds.push(Command::Stop(0x16));
            sim.sounds.push(Command::Play(if airborne < 9 { 0x16 } else { 0x19 }));
        }
        sim.mem.set_i16(p + 0x4E6, 0);
    }
    body::update_velocities(&mut sim.mem, b);
    Ok(grounded)
}
