//! Wheels on the ground (`FUN_08147ec4`) on typed state: suspension, tyre friction and wheel spin for the four
//! wheels (`state::Wheel`: `contact` point (body-relative), `world_pos` the wheel rotated into world axes,
//! `car_pos` in car axes, `spin`, `spring` rate, `damping`, `slip` (squared slip speed), `grip`, `sector` under the
//! wheel). Wheels 0 and 1 steer.

use crate::body;
use crate::carworld::{CarWorld, NONE};
use crate::math::{add, cos, div, isqrt, mat_mul, mul12, recip, sin};
use crate::sound::Command;

/// `FUN_0814de40`: the suspension step of the racing step (only while `g.settled` is 0, see `car.rs`). Each
/// point (x, z in car axes, city units) is turned by the car's heading (`rotation_y`), and its sector and floor
/// height looked up (the point becomes the turned offset and that floor height; `sectors` gets the sector). Then,
/// per point `i`: vertical speed gains `0x320000 / dt`; height moves by speed / dt and stops at the floor (the
/// speed bounces back at −1/32, and the point counts as a hit); the spring and its rate follow through a 64-bit
/// division by a quarter of the car's mass; the rate is damped to 160/256. Returns the number of hits.
pub fn suspension(w: &mut CarWorld, i: usize, pts: &mut [[i32; 3]], sectors: &mut [u16], dt: i32) -> i32 {
    let e = w.slots[i].e.clone();
    let k = w.data.car.handling[e.car as usize][0] >> 2;
    let spring_rest = k.wrapping_mul(4).wrapping_add(k).wrapping_mul(2);
    let a = e.heading >> 8;
    let (c, s) = (cos(w.rom, a), sin(w.rom, a));
    for (pt, out) in pts.iter_mut().zip(sectors.iter_mut()) {
        let (px, pz) = (pt[0] << 8, pt[2] << 8);
        let x = px.wrapping_mul(c).wrapping_add(pz.wrapping_mul(s)) >> 14;
        let z = px.wrapping_mul(s.wrapping_neg()).wrapping_add(pz.wrapping_mul(c)) >> 14;
        w.query.pos = [
            e.pos[0].wrapping_add(x) >> 8,
            e.pos[1] >> 8,
            e.pos[2].wrapping_add(z) >> 8,
        ];
        w.query.sector = e.sector;
        let mut sector = w.near_query();
        if sector == NONE {
            sector = w.query.sector as u32;
        }
        pt[0] = x;
        pt[2] = z;
        if sector != NONE {
            pt[1] = w.floor_height(sector, w.query.pos[0], w.query.pos[2]);
        }
        *out = sector as u16;
    }
    let gravity = div(0x32_0000, dt);
    let mut hits = 0;
    let car = &mut w.slots[i].c;
    for (i, pt) in pts.iter().enumerate() {
        let v = car.point_speed[i].wrapping_add(gravity);
        car.point_speed[i] = v;
        let mut h = car.point_height[i].wrapping_add(div(v, dt));
        if h >= pt[1] {
            h = pt[1];
            car.point_speed[i] = div(v.wrapping_neg(), 32);
            hits += 1;
        }
        let d = div(car.spring[i].wrapping_sub(h).wrapping_mul(-0x70), 100);
        // __divdi3 of the 64-bit (d + rest) << 15 by a quarter of the mass (never 0: every handling record has a
        // mass); only the low word of the quotient is used.
        let q = ((d as i64).wrapping_add(spring_rest as i64).wrapping_shl(15) / k as i64) as i32;
        let r = car.spring_rate[i].wrapping_add(div(q, dt));
        car.spring_rate[i] = r;
        car.spring[i] = car.spring[i].wrapping_add(div(r, dt));
        car.spring_rate[i] = r.wrapping_mul(160) >> 8;
        car.point_height[i] = h;
    }
    hits
}

/// `FUN_081484f0`: the car tipped over (rotation `rot[4]` below 0xF21): instead of the wheels, each of eight body
/// corners that is below its floor pushes back (load from depth and the floor-normal speed) with friction capped
/// by the surface grip. Clears the wheels' slip, counts the steps without a corner down (`airborne`), plays the
/// landing sound when the player's car comes down, and updates velocity and angular velocity from the momenta.
/// The callers then zero the wheel spins and count the tipped steps.
pub fn tipped(w: &mut CarWorld, i: usize, dt: i32) {
    for wheel in &mut w.slots[i].c.wheels {
        wheel.slip = 0;
    }
    let sector = w.slots[i].e.sector as u32;
    let data = w.data;
    let rot = w.slots[i].c.body.rot;
    let mut down = 0;
    for corner in data.car.corners {
        let corner = mat_mul(corner, &rot);
        let pos = w.slots[i].c.body.pos;
        w.query.pos = [0, 1, 2].map(|k| corner[k].wrapping_add(pos[k]) >> 8);
        w.query.sector = sector as u16;
        let found = w.near_query();
        let mut s = if found == NONE { sector } else { found };
        let (qx, qz) = (w.query.pos[0], w.query.pos[2]);
        let depth = |w: &CarWorld, s: u32| {
            let h = w.floor_height(s, qx, qz);
            w.slots[i].c.body.pos[1]
                .wrapping_add(corner[1])
                .wrapping_sub(h.wrapping_sub(0x2000))
        };
        let mut pen = depth(w, s);
        if pen > 0x8000 {
            s = sector;
            pen = depth(w, s);
        }
        if pen <= 0 {
            continue;
        }
        down += 1;
        let arm = [corner[0], corner[1].wrapping_add(0x2000).wrapping_sub(pen), corner[2]];
        let b = &w.slots[i].c.body;
        let wv = b.ang_vel;
        let spin = [
            mul12(arm[1], wv[2]).wrapping_sub(mul12(wv[1], arm[2])),
            mul12(arm[2], wv[0]).wrapping_sub(mul12(wv[2], arm[0])),
            mul12(arm[0], wv[1]).wrapping_sub(mul12(wv[0], arm[1])),
        ];
        let v = add(b.vel, spin);
        let f = &data.city[w.geometry().floor_sector(s) as usize];
        let surface = (f.floor as usize).min(7);
        let mut n = f.plane.map(|c| c as i32);
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
            let grip = ((load << 10 >> 8).wrapping_mul(data.car.surface_grip[surface]) >> 8).min(0x4000);
            if sq > grip.wrapping_mul(grip) {
                let r = recip(w.rom, isqrt(sq as u32));
                let (ux, uz) = (mul12(r, v[0]), mul12(r, v[2]));
                let k = -(grip >> 4);
                (fx, fz) = (mul12(ux, k), mul12(uz, k));
            } else {
                (fx, fz) = (v[0].wrapping_neg() >> 4, v[2].wrapping_neg() >> 4);
            }
        }
        let f = [mul12(fx, dt), mul12(-load, dt), mul12(fz, dt)];
        let b = &mut w.slots[i].c.body;
        b.momentum = add(b.momentum, f);
        // Torque arm × force; only its x component is quartered (reproduced as is).
        let torque = [
            mul12(arm[1], f[2]).wrapping_sub(mul12(f[1], arm[2])) >> 2,
            mul12(arm[2], f[0]).wrapping_sub(mul12(f[2], arm[0])),
            mul12(arm[0], f[1]).wrapping_sub(mul12(arm[1], f[0])),
        ];
        b.ang_momentum = crate::math::sub(b.ang_momentum, torque);
    }
    let player = w.is_player(i);
    let c = &mut w.slots[i].c;
    if down == 0 {
        c.airborne = c.airborne.wrapping_add(1);
    } else {
        let steps = c.airborne;
        if steps > 4 && player {
            w.sounds.push(Command::Stop(0x19));
            w.sounds.push(Command::Stop(0x16));
            w.sounds.push(Command::Play(if steps > 8 { 0x19 } else { 0x16 }));
        }
        c.airborne = 0;
    }
    body::update_velocities(&mut c.body);
}

/// The four wheels on the ground; returns the number touching it.
pub fn wheels(w: &mut CarWorld, i: usize, dt: i32) -> i32 {
    let data = w.data;
    let mut grounded = 0;
    let steer_shift = if w.slots[i].c.gear == 0 { 9 } else { 8 };
    let sector = w.slots[i].e.sector as u32;
    let c = &w.slots[i].c;
    let h = w.floor_height(sector, c.body.pos[0] >> 8, c.body.pos[2] >> 8);
    // Below the floor and still falling: drop the vertical momentum's last velocity step.
    if c.body.pos[1] - h > 0x800 && c.body.vel[1] > 0 {
        let c = &mut w.slots[i].c;
        let my = c.body.momentum[1] - c.body.vel[1];
        c.body.momentum[1] = my;
        c.body.vel[1] = mul12(c.body.inv_mass, my);
    }
    let rom = w.rom;
    let c = &mut w.slots[i].c;
    let rot = c.body.rot;
    let (rear_x, rear_z) = (rot[0], rot[2]);
    let (cs, sn) = (cos(rom, c.steering >> steer_shift), sin(rom, c.steering >> steer_shift));
    // The wheel's rolling direction: the car's x axis turned by the steering angle.
    let mut fx = ((cs >> 2) * rot[0] + rot[6] * (-sn >> 2)) >> 12;
    let mut fz = ((cs >> 2) * rot[2] + rot[8] * (-sn >> 2)) >> 12;
    c.wheel_spin = 0;
    // The tyre state does not change how wheels find their sectors: gather the queries first, wheel by wheel.
    for k in 0..4 {
        let c = &mut w.slots[i].c;
        let wheel = &mut c.wheels[k];
        wheel.slip = 0;
        let local = wheel.car_pos;
        let world_pos =
            [0, 1, 2].map(|j| mul12(local[0], rot[j]) + mul12(rot[3 + j], local[1]) + mul12(rot[6 + j], local[2]));
        wheel.world_pos = world_pos;
        if k == 2 {
            fx = rear_x;
            fz = rear_z;
        }
        let pos = c.body.pos;
        let ride = c.wheels[0].ride_height;
        w.query.pos = [0, 1, 2].map(|j| (world_pos[j] + pos[j]) >> 8);
        w.query.sector = sector as u16;
        let found = w.near_query();
        let wsector = if found == NONE { sector } else { found };
        let (qx, qz) = (w.query.pos[0], w.query.pos[2]);
        let depth = |w: &CarWorld, s: u32| {
            let h = w.floor_height(s, qx, qz);
            w.slots[i].c.body.pos[1] + world_pos[1] + ride - h
        };
        let mut wsector = wsector;
        let mut pen = depth(w, wsector);
        if pen > 0x8000 {
            wsector = sector;
            pen = depth(w, wsector);
        }
        let fs = &data.city[w.geometry().floor_sector(wsector) as usize];
        let c = &mut w.slots[i].c;
        c.wheels[k].sector = wsector as u16;
        c.wheels[k].u_92 = (wsector >> 16) as u16;
        if pen > 0 {
            grounded += 1;
            let contact = [world_pos[0], world_pos[1] + ride - pen, world_pos[2]];
            c.wheels[k].contact = contact;
            let omega = c.body.ang_vel;
            let spin = [
                mul12(omega[2], contact[1]) - mul12(omega[1], contact[2]),
                mul12(omega[0], contact[2]) - mul12(omega[2], contact[0]),
                mul12(omega[1], contact[0]) - mul12(omega[0], contact[1]),
            ];
            let mut vx = c.body.vel[0] + spin[0];
            let vy = c.body.vel[1] + spin[1];
            let mut vz = c.body.vel[2] + spin[2];
            let surface = (fs.floor as usize).min(7);
            let (mut nx, mut ny, mut nz) = (fs.plane[0] as i32, fs.plane[1] as i32, fs.plane[2] as i32);
            if ny < 1 {
                if ny == 0 {
                    ny = 0x1000;
                } else {
                    (nx, ny, nz) = (-nx, -ny, -nz);
                }
            }
            let wheel = &mut c.wheels[k];
            let damping = if vy < 1 { wheel.damping - 10 } else { wheel.damping };
            let mut load = (wheel.spring.wrapping_mul(pen) >> 8)
                + ((mul12(nx, vx) + mul12(vy, ny) + mul12(vz, nz)).wrapping_mul(damping) >> 7);
            if load < 0 {
                load = 0;
            }
            let (mut ax, mut az) = (0, 0);
            let wheel_spin = wheel.spin;
            vx += (wheel_spin >> 7) * fz >> 12;
            vz += -fx * (wheel_spin >> 7) >> 12;
            let slip2 = vx.wrapping_mul(vx).wrapping_add(vz.wrapping_mul(vz));
            if slip2 != 0 {
                wheel.slip = slip2 >> 8;
                let grip = (wheel.grip.wrapping_mul(load) >> 8).wrapping_mul(data.car.surface_grip[surface]) >> 8;
                let grip = grip.min(0x4000);
                if grip * grip < slip2 {
                    let r = recip(rom, isqrt(slip2 as u32));
                    let (ux, uz) = (mul12(r, vx), mul12(r, vz));
                    ax = -(grip >> 4) * ux >> 12;
                    az = -(grip >> 4) * uz >> 12;
                } else {
                    ax = -vx >> 4;
                    az = -vz >> 4;
                }
                wheel.spin = wheel_spin + (dt * (mul12(ax, fz) + mul12(az, -fx)) >> 1);
            }
            let ix = dt * ax >> 12;
            let iy = dt * -load >> 12;
            let iz = dt * az >> 12;
            let b = &mut c.body;
            b.momentum[0] += ix;
            b.momentum[1] += iy;
            b.momentum[2] += iz;
            let [cx, cy, cz] = contact;
            b.ang_momentum[0] -= mul12(iz, cy) - mul12(iy, cz);
            b.ang_momentum[1] -= mul12(ix, cz) - mul12(iz, cx);
            b.ang_momentum[2] -= mul12(iy, cx) - mul12(ix, cy);
        }
        let c = &mut w.slots[i].c;
        c.wheel_spin += c.wheels[k].spin;
    }
    let player = w.is_player(i);
    let c = &mut w.slots[i].c;
    if grounded == 0 {
        c.airborne += 1;
    } else {
        let airborne = c.airborne;
        if airborne > 4 && player {
            w.sounds.push(Command::Stop(0x19));
            w.sounds.push(Command::Stop(0x16));
            w.sounds.push(Command::Play(if airborne < 9 { 0x16 } else { 0x19 }));
        }
        c.airborne = 0;
    }
    body::update_velocities(&mut w.slots[i].c.body);
    grounded
}
