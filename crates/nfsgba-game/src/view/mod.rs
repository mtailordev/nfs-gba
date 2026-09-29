//! What the exact subsystems read, taken from (and written back to) the game's RAM: the renderer's frame,
//! runtime tables, entities and root portal; the sky's camera; the camera's frame. The matrix slots, the HUD and
//! the race's setup have their own adapters (`slots`, `hud`, `race`).

pub mod ai;
pub mod car;
pub mod hud;
pub mod race;
pub mod slots;

pub use race::{RaceView, Racer, base_palette, matrix, player_atlas};

use nfsgba_formats::{
    LEVEL_TABLE,
    render::{self, Entity, Frame, MaterialState, Piece, Portal, Runtime, Scene, View},
    sky::SkyCamera,
};
use nfsgba_sim::{
    Mem,
    layout::{Field, Ptr},
    state::{self, CAMERA_MATRIX, Camera, Input, Race, Screen, WorldHeader},
};

use crate::camera::{CameraFrame, Racer as CamRacer};

pub use nfsgba_sim::state::WORLD;

pub fn frame(m: &Mem) -> Frame {
    let (view, cam) = (m.u32(WORLD + 0x50), m.u32(WORLD + 0x54));
    Frame {
        view: View {
            cx: m.i16(view + 8),
            cy: m.i16(view + 0xA),
            near: m.i32(view + 0x10),
            focal: m.i32(view + 0x1C),
        },
        camera: std::array::from_fn(|k| m.i32(cam + 4 * k as u32)),
        rect: std::array::from_fn(|k| m.i16(WORLD + 0x58 + 2 * k as u32)),
    }
}

/// World `+0x48` (material animation and scroll, count world `+0xD8`) and the moving wall pieces (world `+0x18`)
/// the city's walls name.
pub fn runtime(rom: &[u8], m: &Mem) -> Runtime {
    let walls =
        u32::from_le_bytes(rom[LEVEL_TABLE + 0x14..LEVEL_TABLE + 0x18].try_into().unwrap()) as usize - 0x0800_0000;
    let pieces = (0..m.u16(WORLD + 0xDC) as usize)
        .map(|k| walls + 0x44 * k + 0x2A)
        .map(|a| u16::from_le_bytes([rom[a], rom[a + 1]]))
        .filter(|&p| p != 0xFFFF)
        .max()
        .map_or(0, |p| p as u32 + 1);
    let (piece_at, materials) = (m.u32(WORLD + 0x18), m.u32(WORLD + 0x48));
    Runtime {
        pieces: (0..pieces)
            .map(|k| {
                let h = |o: u32| m.i16(piece_at + 0x20 * k + o);
                Piece {
                    dx: h(0),
                    dz: h(2),
                    ceiling: h(4),
                    floor: h(6),
                    top: h(8),
                    bottom: h(0xA),
                    material: h(0xC) as u16,
                    flags: h(0xE) as u16,
                }
            })
            .collect(),
        sector_offsets: Vec::new(),
        materials: (0..m.u16(WORLD + 0xD8) as u32)
            .map(|k| MaterialState {
                frame: m.u16(materials + 8 * k + 2),
                u_scroll: m.i16(materials + 8 * k + 4),
                v_scroll: m.i16(materials + 8 * k + 6),
            })
            .collect(),
    }
}

pub fn entity_count(m: &Mem) -> u32 {
    m.u16(WORLD + 0xF8) as u32 + m.u16(WORLD + 0xFA) as u32
}

pub fn scene(m: &Mem) -> Scene<'_> {
    let (ents, heads, mats) = (m.u32(WORLD + 0x3C), m.u32(WORLD + 0xC), m.u32(WORLD + 0xFC));
    Scene::new(
        (0..entity_count(m))
            .map(|i| Entity::read(m.bytes(ents + 0xA4 * i, 0xA4)))
            .collect(),
        (0..m.u16(WORLD + 0xDA) as u32).map(|s| m.u16(heads + 2 * s)).collect(),
        (0..64)
            .map(|s| std::array::from_fn(|k| m.i32(mats + 0x30 * s + 4 * k as u32)))
            .collect(),
        &m.ewram,
    )
}

/// The fields `draw_world` writes: `+0x04` draw order, `+0x0A` (bit 2: drawn), `+0x28` sort key.
pub fn store_entities(m: &mut Mem, entities: &[Entity]) {
    let ents = m.u32(WORLD + 0x3C);
    for (i, e) in entities.iter().enumerate() {
        let at = ents + 0xA4 * i as u32;
        m.set_u16(at + 4, e.draw_next);
        m.set_u16(at + 0xA, e.flags);
        m.set_i32(at + 0x28, e.key);
    }
}

/// List entry 0 as `camera_update` leaves it (world `+0x60`).
pub fn root(m: &Mem) -> Portal {
    let list = m.u32(WORLD + 0x60);
    Portal {
        sector: m.u16(list),
        left: m.i16(list + 2),
        right: m.i16(list + 4),
        top: m.i16(list + 6),
        bottom: m.i16(list + 8),
        flags: 0,
        depth: 0,
    }
}

pub fn sky_camera(m: &Mem) -> SkyCamera {
    let (c, s) = (Camera::load(m, 0), Screen::load(m, 0));
    SkyCamera {
        yaw: c.look,
        shake: s.shake,
        horizon: c.horizon,
        view: c.view,
    }
}

/// What `camera_update` reads and writes.
pub fn camera_frame(m: &Mem) -> CameraFrame {
    let (w, race) = (WorldHeader::load(m, WORLD), Race::load(m, 0));
    let racer = |e: Ptr<state::Entity>| {
        let entity = e.read(m);
        let car = entity.driver.read(m);
        CamRacer { entity, car }
    };
    CameraFrame {
        camera: Camera::load(m, 0),
        screen: Screen::load(m, 0),
        view: w.view.read(m),
        input: Input::load(m, 0),
        phase: race.phase,
        player: racer(w.entities.at(race.player)),
        target: racer(race.player_entity),
        focus: w.entities.at(race.focus).read(m),
        pieces: w.pieces.read_n(m, w.piece_count as u32),
        offsets: w.sector_offsets.read_n(m, w.sector_offset_count as u32),
        query: (w.query_sector, w.query[0], w.query[2]),
        root: w.visible.read(m),
        rect: w.rect,
        ceiling: race.profile.read(m).ceiling,
    }
}

pub fn store_camera_frame(m: &mut Mem, f: &CameraFrame) {
    f.camera.store(m, 0);
    f.screen.store(m, 0);
    f.input.store(m, 0);
    let mut w = WorldHeader::load(m, WORLD);
    w.view.write(m, &f.view);
    w.visible.write(m, &f.root);
    // The camera points the world at its matrix and clears `+0xF0`.
    w.camera_matrix = Ptr::new(CAMERA_MATRIX);
    w.u_f0 = 0;
    (w.query_sector, w.query[0], w.query[2]) = f.query;
    w.rect = f.rect;
    w.store(m, WORLD);
    let profile = Race::load(m, 0).profile;
    let mut p = profile.read(m);
    p.ceiling = f.ceiling;
    profile.write(m, &p);
}

/// The visible-sector list the camera builds (`build_visible_sectors`, called at the end of `camera_update`).
pub fn visible(rom: &[u8], m: &Mem) -> render::Visibility {
    render::visible_sectors(rom, &frame(m), root(m))
}
