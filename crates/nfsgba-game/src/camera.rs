//! The race camera: `camera_dispatch` (`0x081389a0`) runs the view's function ([`GameData::views`]: views 0..6
//! `camera_update` `0x08137cb0`; 7 `camera_look_at_player`, not ported), always with `param_3 = 0`, so only that
//! branch of `camera_update` is here. It runs on typed state, a [`CameraFrame`]; `view::camera_frame` loads that
//! from the game's RAM and `view::store_camera_frame` stores it back (`docs/engine/typed-state.md`).

use nfsgba_fixed::{angle_diff, atan2_fast, cos_q14 as cos, sin_q14 as sin};
use nfsgba_formats::render::Piece;
use nfsgba_sim::{
    Result, Unported,
    data::{CameraFn, GameData},
    state::{Camera, Car, Entity, Input, ListEntry, Screen, SectorOffset, ViewPort},
    world::{Geometry, NONE},
};

use crate::slots::{rotate_vector, rotation_y, transform_point};

/// A car: its entity and its physics struct.
pub struct Racer {
    pub entity: Entity,
    pub car: Car,
}

/// Everything `camera_update` reads and writes.
pub struct CameraFrame {
    pub camera: Camera,
    pub screen: Screen,
    pub view: ViewPort,
    pub input: Input,
    pub phase: u32,
    /// The local player's car (the speed effect), the car the camera follows, and the entity the chase view
    /// resets behind.
    pub player: Racer,
    pub target: Racer,
    pub focus: Entity,
    /// The world's moving pieces and sector offsets (the wall push and the floor height read them).
    pub pieces: Vec<Piece>,
    pub offsets: Vec<SectorOffset>,
    /// The sector search's last start sector and point (world scratch).
    pub query: (u16, i32, i32),
    /// Entry 0 of the visible-sector list: the root portal.
    pub root: ListEntry,
    /// The world's screen rectangle (left, right, top, bottom).
    pub rect: [i16; 4],
    /// Whether the camera sector has a ceiling: this frame, last frame.
    pub ceiling: [u8; 2],
}

/// `camera_dispatch` for the race frame: the view's function.
pub fn dispatch(rom: &[u8], data: &GameData, f: &mut CameraFrame) -> Result<()> {
    match data.views.get(f.camera.view as usize).map(|v| v.function) {
        Some(CameraFn::None) => Ok(()),
        Some(CameraFn::Update) => update(rom, data, f),
        _ => Err(Unported("camera_look_at_player (view 7)")),
    }
}

/// `camera_update` (`0x08137cb0`) with `param_2` the camera matrix and `param_3 = 0`.
fn update(rom: &[u8], data: &GameData, f: &mut CameraFrame) -> Result<()> {
    let geo = Geometry {
        rom,
        city: &data.city,
        pieces: &f.pieces,
        offsets: &f.offsets,
    };
    let (player, p) = (&f.player, &f.target.entity);
    let c = &mut f.camera;
    if f.phase == 4 {
        f.input.held &= 0xFF7F;
    }
    // DOWN alone looks back.
    let back = f.input.held & 0xB2 == 0x80;
    if f.input.pressed & 4 != 0 {
        match c.setting {
            0 => c.setting = 1,
            1 => c.setting = 0,
            _ => {}
        }
    }
    if c.setting == 0 && c.bumper_on != 0 {
        c.bumper_on = 0;
        c.view = 2;
        let (e, chase) = (&f.focus, data.views[2]);
        c.height = chase.height;
        let yaw = e.heading >> 8;
        c.orbit = yaw;
        let out = transform_point(&rotation_y(rom, yaw), [chase.x << 8, 0, chase.distance << 8]);
        c.x = e.pos[0].wrapping_add(out[0]);
        c.z = e.pos[2].wrapping_add(out[2]);
    } else if c.setting == 1 {
        c.bumper_on = 1;
        c.view = 0;
    }
    // The focal length: back to 150 in steps of 4, or towards 150 − max(0, (0x800 − g) >> 5) with nitro, g the
    // angle between the heading and the travel direction.
    let focal = f.view.focal as u32;
    let focal = if player.car.nitro_on == 0 {
        if focal < 0x96 { focal + 4 } else { 0x96 }
    } else {
        let vel = player.car.body.vel;
        let travel = atan2_fast(rom, vel[0] >> 8, vel[2] >> 8);
        let k = ((0x800 - angle_diff(player.entity.heading >> 8, travel).abs()) >> 5).max(0);
        let target = 0x96u32.wrapping_sub(k as u32);
        if target < focal { focal - 4 } else { target }
    };
    f.view.focal = focal as i32;
    let view = c.view;
    if view != 3 {
        let dx = (p.pos[0] >> 8) - (c.x >> 8);
        let dz = (p.pos[2] >> 8) - (c.z >> 8);
        if 0x10_0000 < dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz)) {
            (c.x, c.z, c.sector) = (p.pos[0], p.pos[2], p.sector);
        }
    }
    let (mut cx, mut cz, py) = (c.x >> 8, c.z >> 8, p.pos[1] >> 8);
    c.look = if view == 0 {
        p.heading >> 8
    } else {
        atan2_fast(rom, (p.pos[0] >> 8) - cx, (p.pos[2] >> 8) - cz)
    };
    let (focal, v) = (f.view.focal, data.views[view as usize]);
    let (v0, v2) = if !back {
        (v.x << 8, (v.distance * 0x100).wrapping_add((0x80 - focal) * 0x200))
    } else {
        c.look = c.look.wrapping_add(0x2000);
        (v.x * -0x100, ((0x80 - focal) * 0x200).wrapping_add(v.distance * -0x100))
    };
    c.height = v.height;
    let yaw_matrix = rotation_y(rom, c.look);
    let out = transform_point(&yaw_matrix, [v0, 0, v2]);
    cx = cx.wrapping_add(out[0] >> 8);
    cz = cz.wrapping_add(out[2] >> 8);
    if view == 0 {
        (c.x, c.z, c.sector) = (p.pos[0].wrapping_add(out[0]), p.pos[2].wrapping_add(out[2]), p.sector);
    } else if view != 3 && f.phase != 4 {
        let diff = angle_diff(f.target.car.heading, c.orbit).clamp(-0x600, 0x600);
        let mut orbit = c.orbit;
        if c.raised == 0 {
            orbit -= diff >> 3;
        }
        if 0x4000 < orbit {
            orbit -= 0x4000;
        }
        if orbit < -0x4000 {
            orbit += 0x4000;
        }
        c.orbit = orbit;
        let r = if back { v2.wrapping_neg() } else { v2 };
        let mut x = p.pos[0].wrapping_add(sin(rom, orbit).wrapping_mul(r) >> 14);
        let mut z = p.pos[2].wrapping_add(cos(rom, orbit).wrapping_mul(r) >> 14);
        push_out_of_walls(&geo, c.sector, &mut x, &mut z, c.floor);
        (c.x, c.z) = (x, z);
        f.query = (c.sector, x >> 8, z >> 8);
        if geo.find_sector_near(c.sector as u32, x >> 8, z >> 8) == NONE {
            c.sector = p.sector;
        }
    }
    // The camera sector is the one 72 units ahead along the look direction (or behind it).
    let probe = rotate_vector(&yaw_matrix, data.camera_probe);
    let sector = c.sector;
    let (x, z) = (c.x >> 8, c.z >> 8);
    let (ax, az) = (x.wrapping_add(probe[0]), z.wrapping_add(probe[2]));
    let mut ahead = geo.find_sector_near(sector as u32, ax, az);
    if ahead == NONE {
        return Err(Unported("find_sector_far (FUN_0814dbbc) for the camera's probe point"));
    }
    let (bx, bz) = (x.wrapping_sub(probe[0]), z.wrapping_sub(probe[2]));
    let behind = geo.find_sector_near(sector as u32, bx, bz);
    f.query = (sector, bx, bz);
    if view != 0 {
        let h = geo.floor_height(c.sector as u32, cx, cz);
        if view != 4 {
            c.floor = c.floor.wrapping_add((h.wrapping_sub(c.floor)) >> 1);
        }
    }
    let (cx, cz) = (c.x >> 8, c.z >> 8);
    let lift = py + if !back && c.raised != 0 { 0x82 } else { 0x10 };
    c.matrix_yaw = c.look.wrapping_neg() & 0x3FFF;
    let mut matrix = rotation_y(rom, c.matrix_yaw);
    matrix[9] = cx.wrapping_neg();
    matrix[10] = (c.height >> 8).wrapping_neg().wrapping_sub(lift);
    matrix[11] = cz.wrapping_neg();
    c.matrix = matrix;
    if view != 0 {
        c.horizon = 0;
    }
    c.horizon = c.horizon.clamp(-0x20, 0x20);
    let [width, height] = f.screen.size.map(i32::from);
    let rect = [0, 0, width, height - 1];
    f.screen.rect = rect;
    f.root.left = rect[0] as i16;
    f.root.right = rect[2] as i16;
    f.root.top = rect[1] as i16;
    f.root.bottom = rect[3] as i16;
    f.root.u_0a = [0; 3];
    let mut cy = ((rect[3] - rect[1]) >> 1) as i16 + c.horizon as i16 + (f.screen.centre_y >> 8) as i16;
    if view == 0 {
        cy += 8;
    }
    f.view.cx = ((width >> 1) as i16).wrapping_add(f.screen.shake[0]);
    f.view.cy = f.screen.shake[1].wrapping_add(cy);
    f.rect = [rect[0], rect[2], rect[1], rect[3]].map(|v| v as i16);
    if ahead == NONE {
        ahead = behind;
    }
    c.sector = ahead as u16;
    f.root.sector = match data.city[ahead as usize].start_alias {
        0xFFFF => ahead as u16,
        alias => alias,
    };
    // build_visible_sectors (IWRAM 0x03004828) runs from the list entry: `crate::view::visible`.
    f.ceiling = [(data.city[c.sector as usize].ceiling != 0) as u8, f.ceiling[0]];
    Ok(())
}

/// `camera_push_out_of_walls` (`0x08137744`): for each wall of `sector` that blocks the camera (flag `0x1000`, or
/// `0x4000` when its top at the start corner, plus a moving piece's top offset, is below `limit`; a moving
/// piece's flags replace the wall's), a camera nearer than 81 units to its line (and within its ends, or within
/// √0x18FF of them) is pushed out to 80 units along the wall normal (4.12). Walls in the game's order: the last one
/// (with the first as its end) first, then 0, 1, …
fn push_out_of_walls(g: &Geometry, sector: u16, x: &mut i32, z: &mut i32, limit: i32) {
    let walls = &g.city[sector as usize].walls;
    let n = walls.len();
    for k in 0..n {
        let (wall, next) = (&walls[(k + n - 1) % n], &walls[k]);
        let mut top = wall.top[0] as i32;
        let mut flags = match wall.piece {
            0xFFFF => wall.flags,
            piece => {
                let p = &g.pieces[piece as usize];
                top += p.top as i32;
                p.flags
            }
        };
        if flags & 0x4000 != 0 && top < limit {
            flags |= 0x1000;
        }
        if flags & 0x1000 != 0 {
            let (wx, wz) = (wall.x, wall.z);
            let [nx, nz] = wall.normal.map(i32::from);
            let (dx, dz) = ((*x >> 8) - wx, (*z >> 8) - wz);
            let dist = dx.wrapping_mul(nx).wrapping_add(dz.wrapping_mul(nz)) >> 12;
            if dist < 0x51 {
                let (ex, ez) = (next.x, next.z);
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
    }
}
