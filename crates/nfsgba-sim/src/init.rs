//! Car setup on typed state: the car handler's first step (`FUN_0814b98c`, entity state 0). The physics struct is
//! allocated by the adapter (`ram.rs`: the game's heap is a RAM structure) and so is the player's decal
//! (`decal.rs`: the pixels go onto the heap for the renderer). Here: the handling record and upgrades go into the
//! car, the rigid body is placed, the car settles on the ground (20 steps) and the race's route distances are
//! built.

use crate::body;
use crate::carworld::{CarWorld, NONE};
use crate::contact;
use crate::data::HANDLING_WORDS;
use crate::math::{atan2, cos, div, isqrt, mat_mul, mul12, quat_matrix, recip, recip_entry, sin, sub};
use crate::route::{lateral, nearest_lane};
use crate::state::RigidBody;

/// `FUN_0814b98c` (without the physics struct's allocation and the decal).
pub fn car_init(w: &mut CarWorld, i: usize) {
    let data = w.data;
    let e = &mut w.slots[i].e;
    let h = &data.car.handling[e.car as usize];
    e.race_state = 0x100;
    (e.u_4c, e.waypoint, e.traffic_mode, e.segment) = (0, 0, 0, 0);
    (e.dir_x, e.u_1c, e.dir_z) = (0, 0, 0);
    let heading = div((e.heading >> 8) << 15, 0xA30);
    let c = &mut w.slots[i].c;
    c.heading = heading;
    c.heading_sign = heading >> 31;
    c.gear = 1;
    let mass = h[0];
    let (i0, i1, i2) = (h[2], h[3], h[4]);
    c.inertia = [
        div((i1 * 10).wrapping_mul(mass), i2),
        div((i0 * 10).wrapping_mul(mass), i2),
    ];
    c.points_on_floor = 4;
    let laps = w.g.laps as u8;
    c.laps_left = laps as i8;
    c.laps = laps;
    c.progress = 0;
    c.u_0b0 = 0x7FFF_FFFF;
    c.u_0c2 = 0;
    c.u_434 = 0xFFFF;
    c.u_436 = 0xFFFF;
    c.route_flags = 1;
    c.u_4b8 = 0x100;
    c.tipped = 0;
    c.airborne = 0;
    c.hunter_life = 0;
    c.wrong_way = 0;
    c.stationary = 0;
    c.u_4f0 = 0;
    c.hard_hit = 0;
    let lane = nearest_lane(w, lateral(w, i), -1);
    let c = &mut w.slots[i].c;
    c.lane = lane as u16;
    c.lane_bit = 1u32.checked_shl(c.lane as i16 as u8 as u32).unwrap_or(0);
    upgrades(w, i);
    nitro_setup(w, i);
    for s in &mut w.profile.skid_sound[..4] {
        *s = 0;
    }
    let e = &w.slots[i].e;
    let floor = w.floor_height(e.sector as u32, e.pos[0] >> 8, e.pos[2] >> 8);
    w.slots[i].e.pos[1] = floor;
    w.g.gravity = 0x4F0;
    setup_handling(w, i, h);
    let c = &mut w.slots[i].c;
    let heading = atan2(c.body.rot[6] >> 4, c.body.rot[8] >> 4);
    c.heading = heading;
    c.heading_sign = heading >> 31;
    for wheel in &mut c.wheels {
        wheel.base_grip = (wheel.base_grip * 3) << 10 >> 12;
    }
    race_start_setup(w, i);
    route_distances(w);
}

/// `FUN_08140148`: upgrade levels (10 categories) from the car's save bytes (4 × 2 bits each).
fn upgrades(w: &mut CarWorld, i: usize) {
    let career = w.g.career;
    let c = &mut w.slots[i].c;
    for (k, &byte) in w.save.iter().enumerate() {
        let mut v = byte as u32;
        let mut level = 0;
        for _ in 0..4 {
            level += v & 3;
            v >>= 2;
        }
        c.upgrades[k] = level as i32;
    }
    if career == 2 {
        c.upgrades[9] = 1;
    }
}

/// `FUN_0814f198`: the nitro tank and factors.
pub(crate) fn nitro_setup(w: &mut CarWorld, i: usize) {
    let c = &mut w.slots[i].c;
    let mut v = c.upgrades[9].wrapping_mul(10);
    if i == 0 {
        w.g.upgrade_totals[4] = v;
    }
    if w.g.nitro_free != 0 {
        v = 100;
    }
    c.u_4c4 = 0;
    c.u_4d2 = 0x200;
    c.u_4d0 = 6;
    if v == 0 {
        c.nitro_tank = 0;
        c.nitro_drain = 0x915;
        c.nitro_torque = 0x15CC;
    } else {
        c.nitro_tank = 0x5_0000;
        c.nitro_drain = (0x9C0 - div(v * 0x6B0, 100)) as u16;
        c.nitro_torque = (div(v << 11, 100) + 0x1500) as u16;
    }
    c.nitro_on = 0;
}

/// `FUN_0814b2a8`: engine, gearbox, rigid body and wheels from the handling record `h`, with the upgrades.
pub(crate) fn setup_handling(w: &mut CarWorld, i: usize, h: &[i32; HANDLING_WORDS]) {
    let (data, rom) = (w.data, w.rom);
    let (mut up, mut max) = ([0i32; 5], [0i32; 5]);
    for (row, &level) in data.car.upgrade_weights.iter().zip(&w.slots[i].c.upgrades) {
        for k in 0..5 {
            up[k] += div(row[k].wrapping_mul(level), 10);
            max[k] += row[k];
        }
    }
    let g = &mut w.g;
    let s = &mut w.slots[i];
    let (e, c) = (&mut s.e, &mut s.c);
    let top = h[0x19];
    c.max_rpm = top;
    c.upshift_rpm = top - (top * 500 >> 13);
    c.lower_rpm = c.max_rpm - (c.max_rpm * 0xAF0 >> 13);
    c.engine_braking = h[0x19].wrapping_mul(recip(rom, c.max_rpm)) >> 12;
    c.gearbox_pause = 0;
    let final_drive = h[0x53] + div(up[2].wrapping_mul(h[0x55] - h[0x53]), max[2]);
    for k in 0..8 {
        let v = final_drive.wrapping_mul(h[0xD + k]);
        c.gear_ratios[k] = (if v < 0 { v + 0xFF } else { v }) >> 8;
    }
    if e.index as u32 == g.player {
        g.final_drive = final_drive;
    }
    c.torque_scale = h[0x16] + div(up[1].wrapping_mul(h[0x54] - h[0x16]), max[1]);
    let pos = [e.pos[0], e.pos[1] - h[0x3D] + h[0x3F], e.pos[2]];
    c.torque_curve.copy_from_slice(&h[0x1B..0x1B + 18]);
    // The torque curve's peak: the first point after which it stops rising.
    let tc = &c.torque_curve;
    let mut peak = 2;
    if tc[1] <= tc[2] {
        let mut at = 2;
        let mut v = tc[at];
        loop {
            at += 1;
            peak += 1;
            if peak > 0x11 {
                break;
            }
            let rising = v <= tc[at];
            v = tc[at];
            if !rising {
                break;
            }
        }
    }
    let top = c.max_rpm;
    let shift = (peak - 1) * (top >> 3) + h[0x1A] + (top >> 4);
    c.shift_rpm = shift.min(top - 400);
    body_init(rom, &mut c.body, pos, e.heading, h, h[0], data.car.rest);
    let index = e.index as i32;
    let opponents = g.opponents as i32;
    let grid = if index as u32 > opponents as u32 {
        let mut u = (g.wingman - 1) as u32;
        if g.career != 0 {
            u = (u & 1) + 6;
        }
        data.car.wingman_grid[u.wrapping_add(1) as usize]
    } else {
        match g.career {
            0 => (g.difficulty as i32 - 1) * 0x30 + (opponents - (index - 1)) * 10,
            1 => (g.career_level - 0x28) * 3 + (opponents - (index - 1)) * 10,
            _ => (opponents - (index - 1)) * 10 - 0x30,
        }
    };
    c.grid = grid;
    c.centre_of_mass = [h[0x44], h[0x45], h[0x46]];
    c.centre_of_mass[1] = -0x1800;
    let (brakes, grip) = (up[4], up[3]);
    for (k, front) in [(0, true), (1, true), (2, false), (3, false)] {
        let (drive, brake, gr, spring, damping, ride, pos) = if front {
            (0x38, 0x39, 0x3A, 0x32, 0x33, 0x34, 0x35)
        } else {
            (0x41, 0x42, 0x43, 0x3B, 0x3C, 0x3D, 0x3E)
        };
        let wheel = &mut c.wheels[k];
        wheel.drive = h[drive];
        let extra = if front { 0x5_0000 } else { 0x6_0000 };
        wheel.brake = h[brake] + div(brakes.wrapping_mul(extra), max[4]);
        let base = h[gr] + div(grip * 0x580, max[3]);
        wheel.grip = base;
        wheel.base_grip = base;
        let x = h[pos];
        wheel.car_pos = [
            if k % 2 == 0 { x } else { -x },
            h[pos + 1] - c.centre_of_mass[1],
            h[pos + 2] - c.centre_of_mass[2],
        ];
        wheel.spring = h[spring];
        wheel.ride_height = h[ride];
        wheel.damping = h[damping];
    }
    for wheel in &mut c.wheels {
        wheel.u_88 = 0x700;
        wheel.u_8c = recip(rom, 0x700);
        if g.u_614c == 0 {
            wheel.grip <<= 1;
        }
        wheel.base_grip = wheel.base_grip * 0xD0 >> 8;
    }
    if index == 0 {
        for (k, v) in up.into_iter().enumerate() {
            g.upgrade_totals[k] = v;
        }
    }
}

/// `FUN_08147ca0`: the rigid body at `pos`, turned to `heading` (entity `heading` form), at rest.
fn body_init(
    rom: &[u8],
    b: &mut RigidBody,
    pos: [i32; 3],
    heading: i32,
    h: &[i32; HANDLING_WORDS],
    mass: i32,
    rest: [i32; 3],
) {
    b.pos = pos;
    b.momentum = rest;
    b.ang_momentum = rest;
    orient(rom, b, heading >> 8);
    // The game's inline reciprocal differs from `recip` for negative values; masses are positive.
    b.mass = mass * 3;
    b.inv_mass = recip(rom, mass * 3);
    let inertia = h[0x31] * 0x120 >> 8;
    b.inertia = inertia;
    b.inv_inertia = recip(rom, inertia);
    b.vel = rest;
    b.ang_vel = rest;
    b.quat_rate = [0; 4];
}

/// `FUN_08148f24`: turn body `b` upright to `heading` (14-bit, low 2 bits dropped): quaternion (renormalised
/// through the reciprocal table) and rotation matrix. Momenta and velocities are left alone.
pub(crate) fn orient(rom: &[u8], b: &mut RigidBody, heading: i32) {
    let a = (heading >> 2) * 4;
    let (c, s) = (cos(rom, a) >> 2, sin(rom, a) >> 2);
    let q = matrix_quat(rom, &[c, 0, -s, 0, 0x1000, 0, s, 0, c]);
    let norm2 = q.iter().map(|&v| mul12(v, v)).fold(0i32, i32::wrapping_add);
    let len = isqrt(norm2.wrapping_mul(0x1000) as u32);
    let inv = if len != 0 { recip_entry(rom, len >> 1) >> 1 } else { 0 };
    let q = q.map(|v| mul12(inv, v));
    b.quat = q;
    b.rot = quat_matrix(q);
}

/// `FUN_081477ac`: unit quaternion (x, y, z, w; 1.0 = 0x1000) of a rotation matrix (row-major, 20.12).
fn matrix_quat(rom: &[u8], r: &[i32; 9]) -> [i32; 4] {
    let trace = r[0] + r[4] + r[8] + 0x1000;
    let half_recip = |v: i32| recip(rom, v) >> 1;
    if trace >= 0x41 {
        let v = isqrt((trace * 0x40) as u32) * 8;
        let k = half_recip(v);
        return [
            mul12(k, r[7] - r[5]),
            mul12(k, r[2] - r[6]),
            mul12(k, r[3] - r[1]),
            v >> 1,
        ];
    }
    // The largest diagonal element picks the component computed from the square root.
    let (big, d, (o1, v1), (o2, v2), w) = if r[4] < r[0] && r[8] < r[0] {
        (
            0,
            r[0] - (r[4] - 0x1000) - r[8],
            (1, r[3] + r[1]),
            (2, r[2] + r[6]),
            r[7] - r[5],
        )
    } else if r[8] < r[4] {
        (
            1,
            r[4] - (r[0] - 0x1000) - r[8],
            (0, r[3] + r[1]),
            (2, r[7] + r[5]),
            r[2] - r[6],
        )
    } else {
        (
            2,
            r[8] - (r[0] - 0x1000) - r[4],
            (0, r[2] + r[6]),
            (1, r[7] + r[5]),
            r[3] - r[1],
        )
    };
    let v = isqrt(d as u32) * 0x40;
    let k = if v != 0 { half_recip(v) } else { 0 };
    let mut q = [0; 4];
    q[big] = v >> 1;
    q[o1] = mul12(k, v1);
    q[o2] = mul12(k, v2);
    q[3] = mul12(k, w);
    q
}

/// `FUN_0813e430`: race-controller globals for the start, then 20 settling steps of the car on the ground.
pub(crate) fn race_start_setup(w: &mut CarWorld, i: usize) {
    let data = w.data;
    let g = &mut w.g;
    g.u_6078 = -1;
    g.visited = [0; 16];
    if g.route_index == 0x13 {
        g.visited[1] = 1;
    }
    g.circuit = (g.mode != 3) as u32;
    // `FUN_081400bc`: the time limit.
    g.time_limit = if g.career != 0 { 0x2328 } else { 0x4650 };
    g.u_6088 = 0;
    g.u_6084 = data.car.start_bytes[g.u_5388.wrapping_mul(4) as usize] as i32;
    (g.shift_state, g.u_6076, g.u_6020, g.u_5fe0) = (0, 0, 0, 0);
    g.u_60a8 = 0xC;
    g.u_6080 = 0;
    g.u_5fe4 = 0;
    let k = g.level * 2 + g.u_5610;
    g.u_6028 = data.car.time_scale[k as usize];
    g.u_60ac = 0;
    g.u_6090 = g.u_5604;
    if g.career == 0 {
        let v = g.difficulty as i32;
        g.u_6158 = v;
        g.u_6190 = g.u_6170 + v * g.u_6194;
    } else {
        g.u_6158 = 1;
        g.u_6190 = g.u_6170 + g.u_6194;
    }
    g.gap = 0;
    g.finished = 0;
    w.slots[i].c.torque_timer = 0;
    for _ in 0..20 {
        let e = &w.slots[i].e;
        let old = e.sector;
        w.query.sector = old;
        let s = w.find_sector(old as u32, e.pos[0], e.pos[1], e.pos[2]);
        w.slots[i].e.sector = if s & 0xFFFF == NONE { old } else { s as u16 };
        let gravity = w.g.gravity;
        let c = &mut w.slots[i].c;
        let pull = gravity.wrapping_mul(c.body.mass) >> 12;
        c.body.momentum[1] += pull * 0x800 >> 11;
        for wheel in &mut c.wheels {
            wheel.spin = 0;
        }
        c.airborne = 0;
        contact::wheels(w, i, 0x800);
        let s = &mut w.slots[i];
        let (e, c) = (&mut s.e, &mut s.c);
        body::integrate(w.rom, &mut c.body, 0x800);
        body::integrate(w.rom, &mut c.body, 0x800);
        let offset = [0, c.centre_of_mass[1], c.centre_of_mass[2]];
        e.pos = sub(c.body.pos, mat_mul(offset, &c.body.rot));
    }
    let e = &mut w.slots[i].e;
    let y = ((e.pos[1] as u32) >> 8) as u16;
    e.u_98 = y;
    e.direction = y as i16;
}

/// `FUN_0813f744`: cumulative distances along the main route and the side sections, scaled so each side section
/// spans the main-route distance between where it leaves and rejoins: `RacingLine::measure` (the scale factors
/// go to `g.scales`).
fn route_distances(w: &mut CarWorld) {
    w.route.line.measure(w.g.circuit != 0);
    // `route_gap` reads the scale from the globals' copy.
    w.g.scales[..w.route.line.scales.len()].copy_from_slice(&w.route.line.scales);
}
