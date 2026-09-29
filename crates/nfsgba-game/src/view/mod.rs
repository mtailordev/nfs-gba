//! What the exact subsystems and the viewer read from the race's [`World`]: the renderer's frame, runtime tables,
//! entities and root portal, the sky's camera, the racers (`race`).

pub mod hud;
pub mod race;

pub use race::{RaceView, Racer, base_palette, matrix, player_atlas};

use nfsgba_formats::{
    LEVEL_TABLE,
    render::{self, Entity, Frame, MaterialState, Portal, Runtime, Scene, View},
    sky::SkyCamera,
};

use crate::world::World;

pub fn frame(w: &World) -> Frame {
    let v = &w.view;
    Frame {
        view: View {
            cx: v.cx,
            cy: v.cy,
            near: v.near,
            focal: v.focal,
        },
        camera: w.camera.matrix,
        rect: w.rect,
    }
}

/// The material animation and scroll, and the moving wall pieces the city's walls name.
pub fn runtime(rom: &[u8], w: &World) -> Runtime {
    let walls =
        u32::from_le_bytes(rom[LEVEL_TABLE + 0x14..LEVEL_TABLE + 0x18].try_into().unwrap()) as usize - 0x0800_0000;
    let pieces = (0..w.wall_count as usize)
        .map(|k| walls + 0x44 * k + 0x2A)
        .map(|a| u16::from_le_bytes([rom[a], rom[a + 1]]))
        .filter(|&p| p != 0xFFFF)
        .max()
        .map_or(0, |p| p as usize + 1);
    Runtime {
        pieces: w.pieces[..pieces].to_vec(),
        sector_offsets: Vec::new(),
        materials: w
            .materials
            .iter()
            .map(|&(frame, u_scroll, v_scroll)| MaterialState {
                frame,
                u_scroll,
                v_scroll,
            })
            .collect(),
    }
}

pub fn scene(w: &World) -> Scene<'_> {
    Scene::new(w.render_entities(), w.heads.clone(), w.matrices.clone(), &w.heap)
}

/// The fields `draw_world` writes: the draw order, the flags (bit 2: drawn) and the sort key.
pub fn store_entities(w: &mut World, entities: &[Entity]) {
    for (s, e) in w.slots.iter_mut().zip(entities) {
        (s.e.draw_next, s.e.flags, s.e.key) = (e.draw_next, e.flags, e.key);
    }
}

/// Entry 0 of the visible-sector list as `camera_update` leaves it.
pub fn root(w: &World) -> Portal {
    let r = &w.root;
    Portal {
        sector: r.sector,
        left: r.left,
        right: r.right,
        top: r.top,
        bottom: r.bottom,
        flags: 0,
        depth: 0,
    }
}

pub fn sky_camera(w: &World) -> SkyCamera {
    SkyCamera {
        yaw: w.camera.look,
        shake: w.screen.shake,
        horizon: w.camera.horizon,
        view: w.camera.view,
    }
}

/// The visible-sector list the camera builds (`build_visible_sectors`, called at the end of `camera_update`).
pub fn visible(rom: &[u8], w: &World) -> render::Visibility {
    render::visible_sectors(rom, &frame(w), root(w))
}
