//! The race camera: `camera_dispatch` (`0x081389a0`) calls the view's function from `0x087F399C` (views 0..6:
//! `camera_update` `0x08137cb0`; 7: `camera_look_at_player`, not ported), always with `param_3 = 0`, so only that
//! branch of `camera_update` is here. It owns the camera state in the game's RAM (`docs/engine/game-loop.md`).

use nfsgba_sim::{
    Mem, Result, Unported,
    math::{angle_diff, cos, sin},
    traffic::atan2_fast,
    world::{self, NONE, W_ENTITIES, W_QUERY, W_QUERY_SECTOR, W_SECTORS},
};

use crate::slots::{rotate_vector, rotation_y, store, transform_point};
use crate::view::WORLD;

const HELD: u32 = 0x0300_64C4;
const PRESSED: u32 = 0x0300_64C0;
/// Camera setting (SELECT toggles it): 0 chase, 1 bumper.
const SETTING: u32 = 0x0300_53E4;
/// Set while the bumper view is on; switching back to the chase view resets the camera behind the car.
const BUMPER_ON: u32 = 0x0300_5804;
pub const VIEW_MODE: u32 = 0x0300_55F8;
pub const ORBIT: u32 = 0x0300_5F94;
pub const LOOK: u32 = 0x0300_0214;
/// `-LOOK & 0x3FFF`, the camera matrix's yaw (read by the effects and the rim redraw).
pub const MATRIX_YAW: u32 = 0x0300_5F9C;
const HEIGHT: u32 = 0x0300_5FA4;
pub const CAMERA_X: u32 = 0x0300_56A0;
pub const CAMERA_Z: u32 = 0x0300_00A4;
pub const CAMERA_SECTOR: u32 = 0x0300_5614;
/// The smoothed floor height at the camera: the height limit of flag-0x4000 walls in the wall push.
const FLOOR: u32 = 0x0300_5778;
const HORIZON: u32 = 0x0300_56B8;
const RAISED: u32 = 0x0300_6148;
const MATRIX: u32 = 0x0300_57A0;
const RECT: u32 = 0x0300_53D0;
const FRAME_BUFFER: u32 = 0x0300_6410;
const VIEW: u32 = 0x0300_0080;
const PHASE: u32 = 0x0300_0048;
const PROFILE: u32 = 0x0300_56EC;
const VIEW_X: u32 = 0x087F_39BC;
const VIEW_HEIGHT: u32 = 0x087F_39D4;
const VIEW_DISTANCE: u32 = 0x087F_39EC;
/// The probe (0, 0, 72): the camera sector is the one 72 units ahead along the look direction.
const PROBE: u32 = 0x087B_FC68;

fn player(m: &Mem) -> u32 {
    m.u32(W_ENTITIES) + 0xA4 * m.u32(0x0300_0060)
}

/// `camera_dispatch` for the race frame: the view's function from `0x087F399C`.
pub fn dispatch(m: &mut Mem) -> Result<()> {
    match m.u32(0x087F_399C + 4 * m.u32(VIEW_MODE)) {
        0 => Ok(()),
        0x0813_7CB1 => update(m),
        _ => Err(Unported("camera_look_at_player (view 7)")),
    }
}

/// `FUN_03000800` on the query point (world `+0xC0/+0xC8`) from `sector`.
fn query(m: &mut Mem, sector: u32, x: i32, z: i32) -> u32 {
    m.set_u16(W_QUERY_SECTOR, sector as u16);
    m.set_i32(W_QUERY, x);
    m.set_i32(W_QUERY + 8, z);
    world::find_sector_near_query(m)
}

/// `camera_update` (`0x08137cb0`) with `param_2 = 0x030057A0` (the matrix) and `param_3 = 0`.
fn update(m: &mut Mem) -> Result<()> {
    let d = m.u32(player(m) + 0x8C);
    let list = m.u32(WORLD + 0x60);
    let p = m.u32(0x0300_53AC);
    if m.u32(PHASE) == 4 {
        m.set_u16(HELD, m.u16(HELD) & 0xFF7F);
    }
    // DOWN alone looks back.
    let back = m.u16(HELD) & 0xB2 == 0x80;
    if m.u16(PRESSED) & 4 != 0 {
        match m.u32(SETTING) {
            0 => m.set_u32(SETTING, 1),
            1 => m.set_u32(SETTING, 0),
            _ => {}
        }
    }
    if m.u32(SETTING) == 0 && m.u32(BUMPER_ON) != 0 {
        m.set_u32(BUMPER_ON, 0);
        m.set_u32(VIEW_MODE, 2);
        let e = m.u32(W_ENTITIES) + 0xA4 * m.u32(0x0300_57F8);
        let v = [m.i32(VIEW_X + 8) << 8, 0, m.i32(VIEW_DISTANCE + 8) << 8];
        m.set_i32(HEIGHT, m.i32(VIEW_HEIGHT + 8));
        let yaw = m.i32(e + 0x2C) >> 8;
        m.set_i32(ORBIT, yaw);
        let out = transform_point(&rotation_y(m, yaw), v);
        m.set_i32(CAMERA_X, m.i32(e + 0x0C).wrapping_add(out[0]));
        m.set_i32(CAMERA_Z, m.i32(e + 0x14).wrapping_add(out[2]));
    } else if m.u32(SETTING) == 1 {
        m.set_u32(BUMPER_ON, 1);
        m.set_u32(VIEW_MODE, 0);
    }
    // The focal length: back to 150 in steps of 4, or towards 150 − max(0, (0x800 − g) >> 5) with nitro, g the
    // angle between the heading and the travel direction.
    let focal = m.u32(VIEW + 0x1C);
    if m.u8(d + 0x4D1) == 0 {
        m.set_u32(VIEW + 0x1C, if focal < 0x96 { focal + 4 } else { 0x96 });
    } else {
        let h = m.i32(player(m) + 0x2C);
        let travel = atan2_fast(m, m.i32(d + 0x11C) >> 8, m.i32(d + 0x124) >> 8);
        let k = ((0x800 - angle_diff(h >> 8, travel).abs()) >> 5).max(0);
        let target = 0x96u32.wrapping_sub(k as u32);
        m.set_u32(VIEW + 0x1C, if target < focal { focal - 4 } else { target });
    }
    let view = m.u32(VIEW_MODE);
    if view != 3 {
        let dx = (m.i32(p + 0x0C) >> 8) - (m.i32(CAMERA_X) >> 8);
        let dz = (m.i32(p + 0x14) >> 8) - (m.i32(CAMERA_Z) >> 8);
        if 0x10_0000 < dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz)) {
            m.set_i32(CAMERA_X, m.i32(p + 0x0C));
            m.set_i32(CAMERA_Z, m.i32(p + 0x14));
            m.set_u16(CAMERA_SECTOR, m.u16(p + 0x78));
        }
    }
    let (mut cx, mut cz, py) = (m.i32(CAMERA_X) >> 8, m.i32(CAMERA_Z) >> 8, m.i32(p + 0x10) >> 8);
    let look = if view == 0 {
        m.i32(p + 0x2C) >> 8
    } else {
        atan2_fast(m, (m.i32(p + 0x0C) >> 8) - cx, (m.i32(p + 0x14) >> 8) - cz)
    };
    m.set_i32(LOOK, look);
    let focal = m.i32(VIEW + 0x1C);
    let (v0, v2) = if !back {
        (
            m.i32(VIEW_X + 4 * view) << 8,
            (m.i32(VIEW_DISTANCE + 4 * view) * 0x100).wrapping_add((0x80 - focal) * 0x200),
        )
    } else {
        m.set_i32(LOOK, m.i32(LOOK).wrapping_add(0x2000));
        (
            m.i32(VIEW_X + 4 * view) * -0x100,
            ((0x80 - focal) * 0x200).wrapping_add(m.i32(VIEW_DISTANCE + 4 * view) * -0x100),
        )
    };
    m.set_i32(HEIGHT, m.i32(VIEW_HEIGHT + 4 * view));
    let yaw_matrix = rotation_y(m, m.i32(LOOK));
    let out = transform_point(&yaw_matrix, [v0, 0, v2]);
    cx = cx.wrapping_add(out[0] >> 8);
    cz = cz.wrapping_add(out[2] >> 8);
    if view == 0 {
        m.set_i32(CAMERA_X, m.i32(p + 0x0C).wrapping_add(out[0]));
        m.set_i32(CAMERA_Z, m.i32(p + 0x14).wrapping_add(out[2]));
        m.set_u16(CAMERA_SECTOR, m.u16(p + 0x78));
    } else if view != 3 && m.u32(PHASE) != 4 {
        let diff = angle_diff(m.i32(m.u32(p + 0x8C)), m.i32(ORBIT)).clamp(-0x600, 0x600);
        let mut orbit = m.i32(ORBIT);
        if m.u32(RAISED) == 0 {
            orbit -= diff >> 3;
        }
        if 0x4000 < orbit {
            orbit -= 0x4000;
        }
        if orbit < -0x4000 {
            orbit += 0x4000;
        }
        m.set_i32(ORBIT, orbit);
        let r = if back { v2.wrapping_neg() } else { v2 };
        m.set_i32(
            CAMERA_X,
            m.i32(p + 0x0C).wrapping_add(sin(m, orbit).wrapping_mul(r) >> 14),
        );
        m.set_i32(
            CAMERA_Z,
            m.i32(p + 0x14).wrapping_add(cos(m, orbit).wrapping_mul(r) >> 14),
        );
        let (mut x, mut z) = (m.i32(CAMERA_X), m.i32(CAMERA_Z));
        push_out_of_walls(m, &mut x, &mut z, m.i32(FLOOR));
        m.set_i32(CAMERA_X, x);
        m.set_i32(CAMERA_Z, z);
        if query(m, m.u16(CAMERA_SECTOR) as u32, x >> 8, z >> 8) == NONE {
            m.set_u16(CAMERA_SECTOR, m.u16(p + 0x78));
        }
    }
    let probe = rotate_vector(&yaw_matrix, [m.i32(PROBE), m.i32(PROBE + 4), m.i32(PROBE + 8)]);
    let sector = m.u16(CAMERA_SECTOR) as u32;
    let (x, z) = (m.i32(CAMERA_X) >> 8, m.i32(CAMERA_Z) >> 8);
    let mut ahead = query(m, sector, x.wrapping_add(probe[0]), z.wrapping_add(probe[2]));
    if ahead == NONE {
        return Err(Unported("find_sector_far (FUN_0814dbbc) for the camera's probe point"));
    }
    let behind = query(m, sector, x.wrapping_sub(probe[0]), z.wrapping_sub(probe[2]));
    if view != 0 {
        let h = world::floor_height(m, m.u16(CAMERA_SECTOR) as u32, cx, cz);
        if view != 4 {
            let f = m.i32(FLOOR);
            m.set_i32(FLOOR, f.wrapping_add((h.wrapping_sub(f)) >> 1));
        }
    }
    let (cx, cz) = (m.i32(CAMERA_X) >> 8, m.i32(CAMERA_Z) >> 8);
    let lift = py + if !back && m.u32(RAISED) != 0 { 0x82 } else { 0x10 };
    let yaw = m.i32(LOOK).wrapping_neg() & 0x3FFF;
    m.set_i32(MATRIX_YAW, yaw);
    let mut matrix = rotation_y(m, yaw);
    matrix[9] = cx.wrapping_neg();
    matrix[10] = (m.i32(HEIGHT) >> 8).wrapping_neg().wrapping_sub(lift);
    matrix[11] = cz.wrapping_neg();
    store(m, MATRIX, &matrix);
    if view != 0 {
        m.set_i32(HORIZON, 0);
    }
    m.set_i32(HORIZON, m.i32(HORIZON).clamp(-0x20, 0x20));
    m.set_u32(WORLD + 0x54, MATRIX);
    m.set_u16(WORLD + 0xF0, 0);
    let (width, height) = (m.i16(FRAME_BUFFER) as i32, m.i16(FRAME_BUFFER + 2) as i32);
    let rect = [0, 0, width, height - 1];
    for (k, v) in rect.iter().enumerate() {
        m.set_i32(RECT + 4 * k as u32, *v);
    }
    for (k, v) in [rect[0], rect[2], rect[1], rect[3], 0, 0, 0].iter().enumerate() {
        m.set_u16(list + 2 + 2 * k as u32, *v as u16);
    }
    let mut cy = ((rect[3] - rect[1]) >> 1) as i16 + m.i32(HORIZON) as i16 + (m.u32(0x0300_53A0) >> 8) as i16;
    if view == 0 {
        cy += 8;
    }
    m.set_i16(VIEW + 8, ((width >> 1) as i16).wrapping_add(m.i16(0x0300_5390)));
    m.set_i16(VIEW + 0xA, m.i16(0x0300_5392).wrapping_add(cy));
    for (k, v) in [rect[0], rect[2], rect[1], rect[3]].iter().enumerate() {
        m.set_i16(WORLD + 0x58 + 2 * k as u32, *v as i16);
    }
    if ahead == NONE {
        ahead = behind;
    }
    m.set_u16(CAMERA_SECTOR, ahead as u16);
    m.set_u16(list, ahead as u16);
    let alias = m.u16(m.u32(W_SECTORS) + 0x30 * ahead + 0x22);
    if alias != 0xFFFF {
        m.set_u16(list, alias);
    }
    // build_visible_sectors (IWRAM 0x03004828) runs from the list entry: `crate::view::visible`.
    let profile = m.u32(PROFILE);
    m.set_u8(profile + 0x401, m.u8(profile + 0x400));
    let ceiling = m.i16(m.u32(W_SECTORS) + 0x30 * m.u16(CAMERA_SECTOR) as u32 + 4) != 0;
    m.set_u8(profile + 0x400, ceiling as u8);
    Ok(())
}

/// `camera_push_out_of_walls` (`0x08137744`): for each wall of the camera sector that blocks the camera (flag
/// `0x1000`, or `0x4000` when its top at the start corner, plus a moving piece's top offset, is below `limit`; a
/// moving piece's flags replace the wall's), a camera nearer than 81 units to its line (and within its ends, or
/// within √0x18FF of them) is pushed out to 80 units along the wall normal (`+0x34`/`+0x36`, 4.12). Walls in the
/// game's order: the last one (with the first as its end) first, then 0, 1, …
fn push_out_of_walls(m: &Mem, x: &mut i32, z: &mut i32, limit: i32) {
    let s = m.u32(W_SECTORS) + 0x30 * m.u16(CAMERA_SECTOR) as u32;
    let first = m.u32(WORLD + 0x10) + 0x44 * m.u16(s) as u32;
    let count = m.u16(s + 2) as u32;
    if count == 0 {
        return;
    }
    let mut wall = first + 0x44 * (count - 1);
    let mut next = first;
    for _ in 0..count {
        let mut top = m.i16(wall + 8) as i32;
        let piece = m.u16(wall + 0x2A);
        let mut flags = if piece == 0xFFFF {
            m.u16(wall + 0x2E)
        } else {
            let p = m.u32(WORLD + 0x18) + 0x20 * piece as u32;
            top += m.i16(p + 8) as i32;
            m.u16(p + 0xE)
        };
        if flags & 0x4000 != 0 && top < limit {
            flags |= 0x1000;
        }
        if flags & 0x1000 != 0 {
            let (wx, wz) = (m.i32(wall), m.i32(wall + 4));
            let (nx, nz) = (m.i16(wall + 0x34) as i32, m.i16(wall + 0x36) as i32);
            let (dx, dz) = ((*x >> 8) - wx, (*z >> 8) - wz);
            let dist = dx.wrapping_mul(nx).wrapping_add(dz.wrapping_mul(nz)) >> 12;
            if dist < 0x51 {
                let (ex, ez) = (m.i32(next), m.i32(next + 4));
                let outside = if (ex - wx).wrapping_mul(dx).wrapping_add(dz.wrapping_mul(ez - wz)) < 0 {
                    0x18FF < dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz))
                } else {
                    let (fx, fz) = ((*x >> 8) - ex, (*z >> 8) - ez);
                    (wx - ex).wrapping_mul(fx).wrapping_add(fz.wrapping_mul(wz - ez)) < 0
                        && 0x18FF < fx.wrapping_mul(fx).wrapping_add(fz.wrapping_mul(fz))
                };
                if !outside {
                    let push = 0x50 - dist;
                    *x = x.wrapping_add(push.wrapping_mul(nx) >> 4);
                    *z = z.wrapping_add(nz.wrapping_mul(push) >> 4);
                }
            }
        }
        wall = next;
        next += 0x44;
    }
}
