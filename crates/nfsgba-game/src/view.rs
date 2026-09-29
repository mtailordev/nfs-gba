//! What the exact subsystems read, taken from (and written back to) the game's RAM: the renderer's frame,
//! runtime tables, entities and root portal; the sky's camera; the HUD's globals, racers and objects.

use nfsgba_formats::{
    LEVEL_TABLE, hud,
    render::{self, Entity, Frame, MaterialState, Piece, Portal, Runtime, Scene, View},
    sky::SkyCamera,
    ui,
};
use nfsgba_sim::Mem;

pub const WORLD: u32 = 0x0300_00C0;
/// Sprite screen of the race HUD (world `+0xA4`).
pub const HUD_SCREEN: u32 = WORLD + 0xA4;
pub const HUD_OBJECTS: usize = 55;
pub const MESSAGES: u32 = 0x0300_6210;
pub const TILE_BASE: u32 = 0x0300_64E0;

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
    SkyCamera {
        yaw: m.i32(0x0300_0214),
        shake: [m.i16(0x0300_5390), m.i16(0x0300_5392)],
        horizon: m.i32(0x0300_56B8),
        view: m.u32(0x0300_55F8),
    }
}

pub fn hud_globals(m: &Mem) -> hud::Globals {
    let w = |a: u32| m.u32(a);
    hud::Globals {
        hud: w(0x0300_5698),
        mode: w(0x0300_56E0) as i32,
        language: w(0x0300_5600),
        units: w(0x0300_0040),
        frames: w(0x0300_5800) as i32,
        split: w(0x0300_615C) as i32,
        opponents: w(0x0300_5784),
        ai_cars: w(0x0300_57EC),
        laps: w(0x0300_56E4),
        wingman: w(0x0300_6104),
        portrait: w(0x0300_61DC) as i32,
        portrait_blink: w(0x0300_61D4),
        bar: w(0x0300_61E4) as i32,
        bar_max: w(0x0300_6188) as i32,
        arrow: w(0x0300_601C) as i32,
        route: w(0x0300_5388),
        needle_scale: m.u8(m.u32(0x0300_56EC) + 0x402),
        player: w(0x0300_57F8) as usize,
        race_state: w(0x0300_0048),
        race_state_changed: w(0x0300_00AC),
    }
}

/// The globals `hud_update` changes (the race state past the time limit, the portrait and bar animation).
pub fn store_hud_globals(m: &mut Mem, g: &hud::Globals) {
    m.set_u32(0x0300_0048, g.race_state);
    m.set_u32(0x0300_00AC, g.race_state_changed);
    m.set_i32(0x0300_61DC, g.portrait);
    m.set_u32(0x0300_61D4, g.portrait_blink);
    m.set_i32(0x0300_61E4, g.bar);
}

pub fn hud_racers(m: &Mem) -> [hud::Racer; 4] {
    let ents = m.u32(WORLD + 0x3C);
    std::array::from_fn(|i| {
        let e = ents + 0xA4 * i as u32;
        let d = m.u32(e + 0x8C);
        hud::Racer {
            x: m.i32(e + 0x0C),
            z: m.i32(e + 0x14),
            heading: m.i32(e + 0x2C),
            driver: (d != 0).then(|| hud::Driver {
                revs: m.i32(d + 0x3C),
                gear: m.i32(d + 0x40),
                speed: m.i32(d + 0x44),
                position: m.i32(d + 0xA8),
                laps_left: m.i8(d + 0xC5),
                rev_scale: m.i32(d + 0x454),
                dial: m.i32(d + 0x4C8),
                flags: m.u16(d + 0x4D8),
                hunter_life: m.i32(d + 0x4E8),
            }),
        }
    })
}

pub fn hud_objects(m: &Mem) -> Vec<ui::Object> {
    let at = m.u32(HUD_SCREEN + 0x14);
    (0..HUD_OBJECTS as u32)
        .map(|k| ui::Object::from_bytes(m.bytes(at + 16 * k, 16)))
        .collect()
}

pub fn store_hud_objects(m: &mut Mem, objects: &[ui::Object]) {
    let at = m.u32(HUD_SCREEN + 0x14);
    for (k, o) in objects.iter().enumerate() {
        let a = at + 16 * k as u32;
        for (j, v) in [
            o.flags,
            o.scale[0] as u16,
            o.scale[1] as u16,
            o.frame as u16,
            o.loaded as u16,
            o.dy as u16,
            o.dx as u16,
            o.angle,
        ]
        .into_iter()
        .enumerate()
        {
            m.set_u16(a + 2 * j as u32, v);
        }
    }
}

pub fn messages(m: &Mem) -> hud::Messages {
    std::array::from_fn(|i| m.bytes(MESSAGES + 4 * i as u32, 4).try_into().unwrap())
}

pub fn store_messages(m: &mut Mem, msgs: &hud::Messages) {
    for (i, b) in msgs.iter().enumerate() {
        m.set_bytes(MESSAGES + 4 * i as u32, b);
    }
}

pub fn shadow_oam(m: &Mem) -> ui::Oam {
    std::array::from_fn(|i| std::array::from_fn(|j| m.u16(crate::oam::SHADOW_OAM + 8 * i as u32 + 2 * j as u32)))
}

pub fn store_shadow_oam(m: &mut Mem, oam: &ui::Oam) {
    for (i, e) in oam.iter().enumerate() {
        for (j, v) in e.iter().enumerate() {
            m.set_u16(crate::oam::SHADOW_OAM + 8 * i as u32 + 2 * j as u32, *v);
        }
    }
}

/// The visible-sector list the camera builds (`build_visible_sectors`, called at the end of `camera_update`).
pub fn visible(rom: &[u8], m: &Mem) -> render::Visibility {
    render::visible_sectors(rom, &frame(m), root(m))
}
