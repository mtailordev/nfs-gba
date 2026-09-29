//! Car setup: the car handler's first step (`FUN_0814b98c`, entity state 0). Allocates the physics struct,
//! copies the handling record and upgrades into it, places the rigid body, lets the car settle on the ground
//! (20 steps) and builds the race's route distance tables.

use crate::body;
use crate::contact;
use crate::heap;
use crate::math::{atan2, cos, div, isqrt, mat_mul, mul12, quat_matrix, recip, recip_entry, sin, sub};
use crate::mem::Mem;
use crate::route::{self, CIRCUIT, OPPONENTS};
use crate::world::{self, NONE, PLAYER, PROFILE, W_ENTITIES, W_SEGMENTS, W_WAYPOINTS, WORLD};
use crate::{Result, Sim};

/// Career flag (0 quick race; 1 and 2 career variants).
const CAREER: u32 = 0x0300_00A0;
/// Upgrade weights: 10 categories × 5 attributes (acceleration?, torque, final drive, grip, brakes).
const UPGRADE_WEIGHTS: u32 = 0x087F_5988;
/// Per car in the save data (`*0x0300539C`, 0x11 bytes): +2 decal, +7 10 upgrade bytes (4 × 2 bits).
const CAR_SAVE: u32 = 0x0300_539C;
/// Upgrade totals of the player's car (5 words), for the HUD.
const PLAYER_UPGRADES: u32 = 0x0300_6000;
/// Decal records (0x10 bytes per car × 15 decals): +4 vehicle material.
const DECALS: u32 = 0x087E_F816;
/// Per entity: the decal pixels on the heap.
const DECAL_BUFFERS: u32 = 0x0300_6094;

/// `FUN_0814b98c`.
pub fn car_init(sim: &mut Sim, e: u32) -> Result<()> {
    let m = &mut sim.mem;
    let handling = crate::car::HANDLING + m.u8(e + 0x89) as u32 * 0x158;
    let p = heap::alloc_zeroed(m, 0x4FC);
    m.set_u32(e + 0x8C, p);
    m.set_u16(e + 0x4A, 0x100);
    for off in [0x4C, 0x90, 0x9E, 0x72] {
        m.set_u16(e + off, 0);
    }
    for off in [0x18, 0x1C, 0x20] {
        m.set_u32(e + off, 0);
    }
    let heading = div((m.i32(e + 0x2C) >> 8) << 15, 0xA30);
    m.set_i32(p, heading);
    m.set_i32(p + 4, heading >> 31);
    m.set_i32(p + 0x40, 1);
    let mass = m.i32(handling);
    let (i0, i1, i2) = (m.i32(handling + 8), m.i32(handling + 0xC), m.i32(handling + 0x10));
    m.set_i32(p + 0x34, div((i1 * 10).wrapping_mul(mass), i2));
    m.set_i32(p + 0x38, div((i0 * 10).wrapping_mul(mass), i2));
    m.set_i32(p + 0x48, 4);
    let laps = m.u32(0x0300_56E4) as u8;
    m.set_u8(p + 0xC5, laps);
    m.set_u8(p + 0xC6, laps);
    m.set_i32(p + 0xAC, 0);
    m.set_i32(p + 0xB0, 0x7FFF_FFFF);
    m.set_u16(p + 0xC2, 0);
    m.set_u16(p + 0x434, 0xFFFF);
    m.set_u16(p + 0x436, 0xFFFF);
    m.set_u16(p + 0x4D8, 1);
    m.set_u32(p + 0x4B8, 0x100);
    m.set_u16(p + 0x4E4, 0);
    m.set_u16(p + 0x4E6, 0);
    m.set_u32(p + 0x4E8, 0);
    m.set_u16(p + 0x4EC, 0);
    m.set_u16(p + 0x4EE, 0);
    m.set_u16(p + 0x4F0, 0);
    m.set_u32(p + 0x4B4, 0);
    let lane = route::nearest_lane(m, route::lateral(m, e), -1);
    m.set_u16(p + 0xC0, lane as u16);
    m.set_u32(p + 0x430, 1u32.checked_shl(m.i16(p + 0xC0) as u8 as u32).unwrap_or(0));
    upgrades(m, e);
    nitro_setup(m, e, p);
    let stats = m.u32(PROFILE);
    for k in 0..4 {
        m.set_u32(stats + 0x318 + 4 * k, 0);
    }
    if m.u16(e) as u32 == m.u32(PLAYER) {
        unpack_decal(m, e);
    }
    let floor = world::floor_height(m, m.u16(e + 0x78) as u32, m.i32(e + 0xC) >> 8, m.i32(e + 0x14) >> 8);
    m.set_i32(e + 0x10, floor);
    m.set_u32(0x0300_6030, 0x4F0);
    setup_handling(m, e, handling);
    let heading = atan2(m.i32(p + 0x140) >> 4, m.i32(p + 0x148) >> 4);
    m.set_i32(p, heading);
    m.set_i32(p + 4, heading >> 31);
    for k in 0..4 {
        let grip = p + 0x210 + 0x94 * k;
        m.set_i32(grip, (m.i32(grip) * 3) << 10 >> 12);
    }
    race_start_setup(sim, e)?;
    route_distances(&mut sim.mem);
    Ok(())
}

/// `FUN_08140148`: upgrade levels (`+0x3E0`, 10 categories) from the car's save bytes.
fn upgrades(m: &mut Mem, e: u32) {
    let p = m.u32(e + 0x8C);
    let save = m.u32(CAR_SAVE) + m.u8(e + 0x89) as u32 * 0x11 + 7;
    for i in 0..10 {
        let mut v = m.u8(save + i) as u32;
        let mut level = 0;
        for _ in 0..4 {
            level += v & 3;
            v >>= 2;
        }
        m.set_u32(p + 0x3E0 + 4 * i, level);
    }
    if m.i32(CAREER) == 2 {
        m.set_u32(p + 0x404, 1);
    }
}

/// `FUN_0814f198`: the nitro tank and factors.
pub(crate) fn nitro_setup(m: &mut Mem, e: u32, p: u32) {
    let mut v = m.i32(p + 0x404).wrapping_mul(10);
    if e == m.u32(W_ENTITIES) {
        m.set_i32(PLAYER_UPGRADES + 0x10, v);
    }
    if m.i32(0x0300_6150) != 0 {
        v = 100;
    }
    m.set_u32(p + 0x4C4, 0);
    m.set_u16(p + 0x4D2, 0x200);
    m.set_u8(p + 0x4D0, 6);
    if v == 0 {
        m.set_u32(p + 0x4C8, 0);
        m.set_u16(p + 0x4CC, 0x915);
        m.set_u16(p + 0x4CE, 0x15CC);
    } else {
        m.set_u32(p + 0x4C8, 0x5_0000);
        m.set_u16(p + 0x4CC, (0x9C0 - div(v * 0x6B0, 100)) as u16);
        m.set_u16(p + 0x4CE, (div(v << 11, 100) + 0x1500) as u16);
    }
    m.set_u8(p + 0x4D1, 0);
}

/// `FUN_0814b2a8`: engine, gearbox, rigid body and wheels from the handling record `h`, with the upgrades.
pub(crate) fn setup_handling(m: &mut Mem, e: u32, h: u32) {
    let p = m.u32(e + 0x8C);
    let hw = |m: &Mem, k: u32| m.i32(h + 4 * k);
    let (mut up, mut max) = ([0i32; 5], [0i32; 5]);
    for i in 0..10 {
        let row = UPGRADE_WEIGHTS + i * 0x14;
        let level = m.i32(p + 0x3E0 + 4 * i);
        for k in 0..5 {
            up[k as usize] += div(m.i32(row + 4 * k).wrapping_mul(level), 10);
            max[k as usize] += m.i32(row + 4 * k);
        }
    }
    let top = hw(m, 0x19);
    m.set_i32(p + 0x454, top);
    m.set_i32(p + 0x44C, top - (top * 500 >> 13));
    m.set_i32(p + 0x450, m.i32(p + 0x454) - (m.i32(p + 0x454) * 0xAF0 >> 13));
    m.set_i32(p + 0x458, hw(m, 0x19).wrapping_mul(recip(m, m.i32(p + 0x454))) >> 12);
    m.set_i32(p + 0x9C, 0);
    let final_drive = hw(m, 0x53) + div(up[2].wrapping_mul(hw(m, 0x55) - hw(m, 0x53)), max[2]);
    for k in 0..8 {
        let v = final_drive.wrapping_mul(hw(m, 0xD + k));
        m.set_i32(p + 0x408 + 4 * k, (if v < 0 { v + 0xFF } else { v }) >> 8);
    }
    if e == world::entity(m, m.u32(PLAYER)) {
        m.set_i32(0x0300_60A4, final_drive);
    }
    m.set_i32(
        p + 0x4BC,
        hw(m, 0x16) + div(up[1].wrapping_mul(hw(m, 0x54) - hw(m, 0x16)), max[1]),
    );
    let pos = [
        m.i32(e + 0xC),
        m.i32(e + 0x10) - hw(m, 0x3D) + hw(m, 0x3F),
        m.i32(e + 0x14),
    ];
    for k in 0..18 {
        m.set_i32(p + 0x464 + 4 * k, hw(m, 0x1B + k));
    }
    // The torque curve's peak: the first point after which it stops rising.
    let mut peak = 2;
    if m.i32(p + 0x468) <= m.i32(p + 0x46C) {
        let mut at = p + 0x46C;
        let mut v = m.i32(at);
        loop {
            at += 4;
            peak += 1;
            if peak > 0x11 {
                break;
            }
            let rising = v <= m.i32(at);
            v = m.i32(at);
            if !rising {
                break;
            }
        }
    }
    let top = m.i32(p + 0x454);
    let shift = (peak - 1) * (top >> 3) + hw(m, 0x1A) + (top >> 4);
    m.set_i32(p + 0x45C, shift.min(top - 400));
    body_init(m, p + 0xC8, pos, m.i32(e + 0x2C), h, hw(m, 0));
    let index = m.u16(e) as i32;
    let opponents = m.i32(OPPONENTS);
    let grid = if index as u32 > opponents as u32 {
        let mut u = (m.i32(0x0300_6104) - 1) as u32;
        if m.i32(CAREER) != 0 {
            u = (u & 1) + 6;
        }
        m.i32(0x087F_42E4 + u * 4)
    } else {
        match m.i32(CAREER) {
            0 => (m.i32(0x0300_5608) - 1) * 0x30 + (opponents - (index - 1)) * 10,
            1 => (m.i32(0x0300_00BC) - 0x28) * 3 + (opponents - (index - 1)) * 10,
            _ => (opponents - (index - 1)) * 10 - 0x30,
        }
    };
    m.set_i32(p + 0x188, grid);
    m.set_i32(p + 0x438, hw(m, 0x44));
    m.set_i32(p + 0x43C, hw(m, 0x45));
    m.set_i32(p + 0x440, hw(m, 0x46));
    m.set_i32(p + 0x43C, -0x1800);
    let wheel = |k: u32| p + contact::WHEELS + contact::WHEEL_SIZE * k;
    let (brakes, grip) = (up[4], up[3]);
    for (k, front) in [(0, true), (1, true), (2, false), (3, false)] {
        let w = wheel(k);
        let (drive, brake, g, spring, damping, ride, pos) = if front {
            (0x38, 0x39, 0x3A, 0x32, 0x33, 0x34, 0x35)
        } else {
            (0x41, 0x42, 0x43, 0x3B, 0x3C, 0x3D, 0x3E)
        };
        m.set_i32(w + 0x70, hw(m, drive));
        let extra = if front { 0x5_0000 } else { 0x6_0000 };
        m.set_i32(w + 0x74, hw(m, brake) + div(brakes.wrapping_mul(extra), max[4]));
        let base = hw(m, g) + div(grip * 0x580, max[3]);
        m.set_i32(w + 0x80, base);
        m.set_i32(w + 0x84, base);
        let x = hw(m, pos);
        m.set_vec3(
            w + 0x48,
            [
                if k % 2 == 0 { x } else { -x },
                hw(m, pos + 1) - m.i32(p + 0x43C),
                hw(m, pos + 2) - m.i32(p + 0x440),
            ],
        );
        m.set_i32(w + 0x68, hw(m, spring));
        m.set_i32(w + 0x78, hw(m, ride));
        m.set_i32(w + 0x6C, hw(m, damping));
    }
    for k in 0..4 {
        let w = wheel(k);
        m.set_i32(w + 0x88, 0x700);
        m.set_i32(w + 0x8C, recip(m, 0x700));
        if m.i32(0x0300_614C) == 0 {
            m.set_i32(w + 0x80, m.i32(w + 0x80) << 1);
        }
        m.set_i32(w + 0x84, m.i32(w + 0x84) * 0xD0 >> 8);
    }
    if e == m.u32(W_ENTITIES) {
        for (k, v) in up.into_iter().enumerate() {
            m.set_i32(PLAYER_UPGRADES + 4 * k as u32, v);
        }
    }
}

/// `FUN_08147ca0`: the rigid body at `pos`, turned to `heading` (entity `+0x2C` form), at rest.
fn body_init(m: &mut Mem, b: u32, pos: [i32; 3], heading: i32, h: u32, mass: i32) {
    m.set_vec3(b + body::POS, pos);
    let rest = m.vec3(0x087F_3DD0);
    m.set_vec3(b + body::MOMENTUM, rest);
    m.set_vec3(b + body::ANG_MOMENTUM, rest);
    orient(m, b, heading >> 8);
    // The game's inline reciprocal differs from `recip` for negative values; masses are positive.
    m.set_i32(b + body::MASS, mass * 3);
    m.set_i32(b + body::INV_MASS, recip(m, mass * 3));
    let inertia = m.i32(h + 0xC4) * 0x120 >> 8;
    m.set_i32(b + 0xAC, inertia);
    m.set_i32(b + body::INV_INERTIA, recip(m, inertia));
    let rest = m.vec3(0x087F_3DD0);
    m.set_vec3(b + body::VEL, rest);
    m.set_vec3(b + body::ANG_VEL, rest);
    for off in [0xA8, 0x9C, 0xA0, 0xA4] {
        m.set_i32(b + off, 0);
    }
}

/// `FUN_08148f24`: turn body `b` upright to `heading` (14-bit, low 2 bits dropped): quaternion (renormalised
/// through the reciprocal table) and rotation matrix. Momenta and velocities are left alone.
pub(crate) fn orient(m: &mut Mem, b: u32, heading: i32) {
    let a = (heading >> 2) * 4;
    let (c, s) = (cos(m, a) >> 2, sin(m, a) >> 2);
    let q = matrix_quat(m, &[c, 0, -s, 0, 0x1000, 0, s, 0, c]);
    let norm2 = q.iter().map(|&v| mul12(v, v)).fold(0i32, i32::wrapping_add);
    let len = isqrt(norm2.wrapping_mul(0x1000) as u32);
    let inv = if len != 0 { recip_entry(m, len >> 1) >> 1 } else { 0 };
    let q = q.map(|v| mul12(inv, v));
    for (k, v) in q.into_iter().enumerate() {
        m.set_i32(b + body::QUAT + 4 * k as u32, v);
    }
    for (k, v) in quat_matrix(q).into_iter().enumerate() {
        m.set_i32(b + body::ROT + 4 * k as u32, v);
    }
}

/// `FUN_081477ac`: unit quaternion (x, y, z, w; 1.0 = 0x1000) of a rotation matrix (row-major, 20.12).
fn matrix_quat(m: &Mem, r: &[i32; 9]) -> [i32; 4] {
    let trace = r[0] + r[4] + r[8] + 0x1000;
    let half_recip = |v: i32| recip(m, v) >> 1;
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
pub(crate) fn race_start_setup(sim: &mut Sim, e: u32) -> Result<()> {
    let m = &mut sim.mem;
    let p = m.u32(e + 0x8C);
    let b = p + 0xC8;
    m.set_i32(0x0300_6078, -1);
    for k in 0..16 {
        m.set_u32(0x0300_60C0 + 4 * k, 0);
    }
    if m.i32(0x0300_5720) == 0x13 {
        m.set_u32(0x0300_60C4, 1);
    }
    m.set_u32(CIRCUIT, (m.i32(0x0300_56E0) != 3) as u32);
    // `FUN_081400bc`: the time limit.
    m.set_i32(0x0300_6154, if m.i32(CAREER) != 0 { 0x2328 } else { 0x4650 });
    m.set_u32(0x0300_6088, 0);
    let v = m.i8(0x087F_3050 + m.u32(0x0300_5388).wrapping_mul(4)) as i32;
    m.set_i32(0x0300_6084, v);
    for a in [0x0300_6074, 0x0300_6020, 0x0300_5FE0] {
        m.set_u32(a, 0);
    }
    m.set_u32(0x0300_60A8, 0xC);
    m.set_u32(0x0300_6080, 0);
    m.set_u32(0x0300_5FE4, 0);
    let k = m.i32(0x0300_006C) * 2 + m.i32(0x0300_5610);
    m.set_i32(0x0300_6028, m.i32(0x087F_40D0u32.wrapping_add((k * 4) as u32)));
    m.set_u32(0x0300_60AC, 0);
    m.set_u32(0x0300_6090, m.u32(0x0300_5604));
    if m.i32(CAREER) == 0 {
        let v = m.i32(0x0300_5608);
        m.set_i32(0x0300_6158, v);
        m.set_i32(0x0300_6190, m.i32(0x0300_6170) + v * m.i32(0x0300_6194));
    } else {
        m.set_i32(0x0300_6158, 1);
        m.set_i32(0x0300_6190, m.i32(0x0300_6170) + m.i32(0x0300_6194));
    }
    m.set_u32(0x0300_615C, 0);
    m.set_u32(0x0300_61A4, 0);
    m.set_u32(p + 0x42C, 0);
    for _ in 0..20 {
        let m = &mut sim.mem;
        let old = m.u16(e + 0x78);
        m.set_u16(WORLD + 0xEA, old);
        let s = world::find_sector(m, old as u32, m.i32(e + 0xC), m.i32(e + 0x10), m.i32(e + 0x14))?;
        m.set_u16(e + 0x78, if s & 0xFFFF == NONE { old } else { s as u16 });
        let g = m.i32(0x0300_6030).wrapping_mul(m.i32(b)) >> 12;
        m.set_i32(p + 0xFC, m.i32(p + 0xFC) + (g * 0x800 >> 11));
        for k in 0..4 {
            m.set_u32(p + 0x3AC - 0x94 * k, 0);
        }
        m.set_u16(p + 0x4E6, 0);
        contact::wheels(sim, e, 0x800)?;
        let m = &mut sim.mem;
        body::integrate(m, b, 0x800);
        body::integrate(m, b, 0x800);
        let offset = [0, m.i32(p + 0x43C), m.i32(p + 0x440)];
        let rot: [i32; 9] = std::array::from_fn(|k| m.i32(p + 0x128 + 4 * k as u32));
        m.set_vec3(e + 0xC, sub(m.vec3(p + 0xD0), mat_mul(offset, &rot)));
    }
    let m = &mut sim.mem;
    let y = (m.u32(e + 0x10) >> 8) as u16;
    m.set_u16(e + 0x98, y);
    m.set_u16(e + 0x9A, y);
    Ok(())
}

/// `FUN_0813f744`: cumulative distances (`+0x10`) along the main route and the side segments, scaled so each
/// side segment spans the main-route distance between where it leaves and rejoins; segment scale factors at
/// 0x03006120.
fn route_distances(m: &mut Mem) {
    let segs = m.u32(W_SEGMENTS);
    let count = m.u16(segs) as i32;
    m.set_u32(0x0300_6120, 0x100);
    let wp = |m: &Mem, seg: u32, k: i32| -> u32 {
        m.u32(W_WAYPOINTS)
            .wrapping_add((m.i32(segs + seg * 8 + 4) + k) as u32 * 0x18)
    };
    let dist = |m: &Mem, a: u32, bx: i32, bz: i32| {
        let (dx, dz) = (m.i32(a) - bx, m.i32(a + 4) - bz);
        isqrt(dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz)) as u32)
    };
    let first = wp(m, 0, 0);
    let (mut px, mut pz, mut sum) = (m.i32(first), m.i32(first + 4), 0);
    for i in 0..count {
        let w = wp(m, 0, i);
        sum += dist(m, w, px, pz);
        if i == 1 && m.i32(CIRCUIT) == 0 {
            sum = 0;
        }
        m.set_i32(w + 0x10, sum);
        (px, pz) = (m.i32(w), m.i32(w + 4));
    }
    for j in 0..count {
        let w = wp(m, 0, j);
        if m.i16(w + 0xE) != 0 {
            continue;
        }
        let seg = m.u16(w + 0xC) as u32;
        let n = m.u16(segs + seg * 8) as i32;
        let last = wp(m, seg, n - 1);
        let rejoin = wp(m, 0, m.u16(last + 0xE) as i32);
        let span = (m.i32(rejoin + 0x10) - m.i32(w + 0x10)) >> 8;
        let start = wp(m, seg, 0);
        let (mut px, mut pz, mut acc) = (m.i32(start), m.i32(start + 4), 0);
        for k in 1..n {
            let wk = wp(m, seg, k);
            acc += dist(m, wk, px, pz);
            m.set_i32(wk + 0x10, acc);
            (px, pz) = (m.i32(wk), m.i32(wk + 4));
        }
        let len = m.i32(last + 0x10) >> 8;
        for k in 0..n {
            let wk = wp(m, seg, k);
            m.set_i32(
                wk + 0x10,
                div(span.wrapping_mul(m.i32(wk + 0x10)), len) + m.i32(w + 0x10),
            );
        }
        m.set_i32(0x0300_6120 + seg * 4, div(span << 8, len));
    }
}

/// `unpack_decal` (`FUN_0813bf58`) without its last call: the player's decal pixels onto the heap.
/// NOT 1:1 (rendering): the blit onto the car's texture atlas (`draw_decal_on_atlas`, `FUN_0813bd90`) belongs
/// to the renderer.
fn unpack_decal(m: &mut Mem, e: u32) {
    let car = m.u8(e + 0x89) as u32;
    let save = m.u32(CAR_SAVE) + car * 0x11;
    let rec = DECALS + (car * 0xF + m.u8(save + 2) as u32) * 0x10;
    let material = m.u32(WORLD + 0x24).wrapping_add((m.i16(rec + 4) as i32 * 0x24) as u32);
    let src = m.u32(WORLD + 4) + m.u32(material + 8);
    let size = m.u16(material + 0xE) as u32 * m.u16(material + 0xC) as u32;
    let slot = DECAL_BUFFERS + m.u16(e) as u32 * 4;
    if m.u32(slot) != 0 {
        heap::free(m, m.u32(slot));
        m.set_u32(slot, 0);
    }
    let buf = heap::alloc(m, size);
    m.set_u32(slot, buf);
    let ring = heap::alloc(m, 0x1011);
    lz77_ring_decode(m, src, buf, ring);
    heap::free(m, ring);
    // `remap_decal_pixels`: index 0x10 becomes transparent, the rest move up to palette 0xB0.
    for k in 0..size {
        let v = m.u8(buf + k);
        m.set_u8(buf + k, if v == 0x10 { 0 } else { v.wrapping_add(0xB0) });
    }
}

/// `lz77_ring_decode` (IWRAM 0x030042F4, ARM): LZ77 through a 0x1000-byte ring prefilled with 0xFF up to
/// 0xFED, writing position 0xFEE on. Reproduces the decoder's exits exactly (it may read on after the last
/// output byte when that byte came from a back-reference).
fn lz77_ring_decode(m: &mut Mem, mut src: u32, mut dst: u32, ring: u32) {
    let header = u32::from_le_bytes([m.u8(src), m.u8(src + 1), m.u8(src + 2), m.u8(src + 3)]);
    src += 4;
    let size = header >> 8;
    let mut left = size as i32;
    for k in 0..=0xFED {
        m.set_u8(ring + k, 0xFF);
    }
    let (mut ip, mut flags, mut count, mut produced) = (0xFEEu32, 7u32, 7, 0u32);
    loop {
        flags <<= 1;
        count += 1;
        if count == 8 {
            count = 0;
            flags = m.u8(src) as u32;
            src += 1;
        }
        if flags & 0x80 == 0 {
            let c = m.u8(src);
            src += 1;
            if produced < size {
                left -= 1;
                m.set_u8(dst, c);
                dst += 1;
                if left <= 0 {
                    return;
                }
            }
            m.set_u8(ring + ip, c);
            produced += 1;
            ip = (ip + 1) & 0xFFF;
            continue;
        }
        let (b1, b2) = (m.u8(src) as u32, m.u8(src + 1) as u32);
        src += 2;
        let len = (b1 >> 4) + 2;
        let disp = b2 | (b1 << 8) & 0xF00;
        let at = |ip: u32| ip.wrapping_sub(disp).wrapping_sub(1) & 0xFFF;
        let mut c = m.u8(ring + at(ip));
        if produced < size {
            left -= 1;
            m.set_u8(dst, c);
            dst += 1;
            if left <= 0 {
                continue;
            }
        }
        let mut n = 0;
        loop {
            produced += 1;
            n += 1;
            m.set_u8(ring + ip, c);
            ip = (ip + 1) & 0xFFF;
            if n > len {
                break;
            }
            c = m.u8(ring + at(ip));
            if produced >= size {
                continue;
            }
            left -= 1;
            m.set_u8(dst, c);
            dst += 1;
            if left <= 0 {
                break;
            }
        }
    }
}
