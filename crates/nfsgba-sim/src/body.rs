//! The car's rigid body (physics struct `+0xC8`, offsets relative to it here): `+0x00` mass, `+0x04` inverse
//! mass, `+0x08` position (8.8), `+0x20` orientation quaternion (x, y, z, w; 1.0 = 0x1000), `+0x30` momentum,
//! `+0x48` angular momentum, `+0x54` velocity, `+0x60` rotation matrix (3x3 row-major, 20.12), `+0x90` angular
//! velocity, `+0x9C` quaternion rate, `+0xB0` inverse inertia (a scalar).

use crate::math::{isqrt, mul12, quat_matrix, quat_mul, recip_entry};
use crate::mem::Mem;

pub const MASS: u32 = 0x00;
pub const INV_MASS: u32 = 0x04;
pub const POS: u32 = 0x08;
pub const QUAT: u32 = 0x20;
pub const MOMENTUM: u32 = 0x30;
pub const ANG_MOMENTUM: u32 = 0x48;
pub const VEL: u32 = 0x54;
pub const ROT: u32 = 0x60;
pub const ANG_VEL: u32 = 0x90;
pub const QUAT_RATE: u32 = 0x9C;
pub const INV_INERTIA: u32 = 0xB0;

fn quat(mem: &Mem, a: u32) -> [i32; 4] {
    [mem.i32(a), mem.i32(a + 4), mem.i32(a + 8), mem.i32(a + 12)]
}

fn set_quat(mem: &mut Mem, a: u32, q: [i32; 4]) {
    for (k, c) in q.into_iter().enumerate() {
        mem.set_i32(a + 4 * k as u32, c);
    }
}

/// `FUN_08148fe8`: velocity and angular velocity from momentum and angular momentum.
pub fn update_velocities(mem: &mut Mem, b: u32) {
    let inv_mass = mem.i32(b + INV_MASS);
    let v = mem.vec3(b + MOMENTUM).map(|c| mul12(inv_mass, c));
    mem.set_vec3(b + VEL, v);
    let inv_inertia = mem.i32(b + INV_INERTIA);
    let w = mem.vec3(b + ANG_MOMENTUM).map(|c| mul12(inv_inertia, c));
    mem.set_vec3(b + ANG_VEL, w);
}

/// `FUN_08147b18`: one explicit Euler step of `dt` (4.12 seconds): position, orientation (renormalised through
/// the reciprocal table), rotation matrix, velocities.
pub fn integrate(mem: &mut Mem, b: u32, dt: i32) {
    let vel = mem.vec3(b + VEL);
    let pos = mem.vec3(b + POS);
    mem.set_vec3(b + POS, [0, 1, 2].map(|k| pos[k].wrapping_add(mul12(dt, vel[k]))));
    let w = mem.vec3(b + ANG_VEL);
    let half = [w[0] >> 1, w[1] >> 1, w[2] >> 1, 0];
    let rate = quat_mul(quat(mem, b + QUAT), half);
    set_quat(mem, b + QUAT_RATE, rate);
    let q = quat(mem, b + QUAT);
    let q = [0, 1, 2, 3].map(|k| q[k] + mul12(dt, rate[k]));
    let norm2 = q.iter().map(|&c| mul12(c, c)).fold(0i32, |a, c| a.wrapping_add(c));
    let len = isqrt(norm2.wrapping_mul(0x1000) as u32);
    let inv = if len != 0 { recip_entry(mem, len >> 1) >> 1 } else { 0 };
    let q = q.map(|c| mul12(inv, c));
    set_quat(mem, b + QUAT, q);
    let m = quat_matrix(q);
    for (k, c) in m.into_iter().enumerate() {
        mem.set_i32(b + ROT + 4 * k as u32, c);
    }
    update_velocities(mem, b);
}
