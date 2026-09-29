//! The car's rigid body (`state::RigidBody`): position (8.8), orientation quaternion (x, y, z, w; 1.0 = 0x1000),
//! momentum and angular momentum, velocity, the 3x3 rotation matrix (row-major, 20.12), angular velocity and the
//! inverse inertia (a scalar).

use crate::math::{isqrt, mul12, quat_matrix, quat_mul, recip_entry};
use crate::state::RigidBody;

/// `FUN_08148fe8`: velocity and angular velocity from momentum and angular momentum.
pub fn update_velocities(b: &mut RigidBody) {
    b.vel = b.momentum.map(|c| mul12(b.inv_mass, c));
    b.ang_vel = b.ang_momentum.map(|c| mul12(b.inv_inertia, c));
}

/// `FUN_08147b18`: one explicit Euler step of `dt` (4.12 seconds): position, orientation (renormalised through
/// the reciprocal table), rotation matrix, velocities.
pub fn integrate(rom: &[u8], b: &mut RigidBody, dt: i32) {
    b.pos = [0, 1, 2].map(|k| b.pos[k].wrapping_add(mul12(dt, b.vel[k])));
    let w = b.ang_vel;
    let half = [w[0] >> 1, w[1] >> 1, w[2] >> 1, 0];
    let rate = quat_mul(b.quat, half);
    b.quat_rate = rate;
    let q = [0, 1, 2, 3].map(|k| b.quat[k] + mul12(dt, rate[k]));
    let norm2 = q.iter().map(|&c| mul12(c, c)).fold(0i32, |a, c| a.wrapping_add(c));
    let len = isqrt(norm2.wrapping_mul(0x1000) as u32);
    let inv = if len != 0 { recip_entry(rom, len >> 1) >> 1 } else { 0 };
    let q = q.map(|c| mul12(inv, c));
    b.quat = q;
    b.rot = quat_matrix(q);
    update_velocities(b);
}
