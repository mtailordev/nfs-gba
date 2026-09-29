//! The matrix slots' adapters: load a [`Slots`] frame from the game's RAM, run a step, store it back. The
//! pointers (entities, drivers, traffic) become indices here.

use nfsgba_sim::{
    Mem, Result,
    data::GameData,
    layout::{Field, Ptr},
    state::{Camera, Race, SlotGlobals, SpritePool, WORLD, WorldHeader},
};

use crate::slots::{M, RimFrame, Slots, rim_redraw};

/// Car records the table holds (one per car).
const RECORDS: u32 = 15;
const SLOTS: u32 = 64;

pub fn load<'a>(rom: &'a [u8], data: &'a GameData, m: &Mem) -> Slots<'a> {
    let (w, race) = (WorldHeader::load(m, WORLD), Race::load(m, 0));
    let g = SlotGlobals::load(m, 0);
    let entities = w.entities.read_n(m, (w.first_entity + w.entity_count) as u32);
    // Cars, opponents and the wingman have a physics struct (traffic uses the word otherwise).
    let cars = entities
        .iter()
        .map(|e| (matches!(e.handler, 0..=3 | 0x29) && !e.driver.is_null()).then(|| e.driver.read(m)))
        .collect();
    let pool = SpritePool::load(m, 0);
    Slots {
        rom,
        data,
        phase: race.phase,
        player: race.player,
        link: race.link,
        camera: Camera::load(m, 0),
        view: w.view.read(m),
        slots: Ptr::<M>::new(w.matrix_slots).read_n(m, SLOTS),
        pool: pool.objects.read_n(m, pool.count.max(0) as u32),
        traffic: g
            .traffic
            .map(|t| (!t.is_null()).then(|| t.index_from(w.entities) as usize)),
        records: g.records.read_n(m, RECORDS),
        racer_hits: race.profile.read(m).racer_hits,
        spare: (w.first_entity as usize, w.entity_count as usize),
        query: (w.query, w.query_sector),
        heads: w.sector_heads.read_n(m, w.sector_count as u32),
        g,
        entities,
        cars,
    }
}

pub fn store(m: &mut Mem, f: &Slots) {
    let mut w = WorldHeader::load(m, WORLD);
    f.g.store(m, 0);
    f.camera.store(m, 0);
    for (k, s) in f.slots.iter().enumerate() {
        Ptr::<M>::new(w.matrix_slots).at(k as u32).write(m, s);
    }
    let pool = SpritePool::load(m, 0);
    for (k, s) in f.pool.iter().enumerate() {
        pool.objects.at(k as u32).write(m, s);
    }
    for (k, h) in f.heads.iter().enumerate() {
        w.sector_heads.at(k as u32).write(m, h);
    }
    for (k, e) in f.entities.iter().enumerate() {
        let at = w.entities.at(k as u32);
        at.write(m, e);
        if let Some(c) = &f.cars[k] {
            e.driver.write(m, c);
        }
    }
    (w.query, w.query_sector) = f.query;
    w.store(m, WORLD);
}

/// Runs `step` on the loaded frame and stores it (also when the step stops with `Unported`).
fn run<R>(rom: &[u8], data: &GameData, m: &mut Mem, step: impl FnOnce(&mut Slots) -> R) -> R {
    let mut f = load(rom, data, m);
    let r = step(&mut f);
    store(m, &f);
    r
}

/// The slot part of `race_frame_update` after the camera.
pub fn race_slots(rom: &[u8], data: &GameData, m: &mut Mem) -> Result<()> {
    run(rom, data, m, |f| f.race_slots())
}

/// `opponent_effects` for entity `i`.
pub fn opponent_effects(rom: &[u8], data: &GameData, m: &mut Mem, i: usize, heading: i32, view: i32, size: i32) {
    run(rom, data, m, |f| f.opponent_effects(i, heading, view, size))
}

/// `effect_handler` for the spark entity `i`.
pub fn effect_handler(rom: &[u8], data: &GameData, m: &mut Mem, i: usize) {
    run(rom, data, m, |f| f.effect_handler(i))
}

/// The slot counter, back to 0 at the start of each frame (`FUN_0814f8a0`).
pub fn reset_counter(m: &mut Mem) {
    let mut g = SlotGlobals::load(m, 0);
    g.slot_counter = 0;
    g.store(m, 0);
}

/// The player's rim redraw for entity `i` (`car_racing_step`'s start).
pub fn rim_redraw_for(rom: &[u8], data: &GameData, m: &mut Mem, i: usize) -> Result<()> {
    let (w, race, g) = (WorldHeader::load(m, WORLD), Race::load(m, 0), SlotGlobals::load(m, 0));
    let entity = w.entities.at(i as u32).read(m);
    let heap = |addr: u32| if addr == 0 { 0 } else { (addr & 0x3_FFFF) as usize };
    let index = entity.index as usize;
    let f = RimFrame {
        phase: race.phase,
        player: race.player,
        view: Camera::load(m, 0).view,
        matrix_yaw: Camera::load(m, 0).matrix_yaw,
        wheel_angle: (!entity.driver.is_null()).then(|| entity.driver.read(m).wheel_angle),
        record: g.records.at(entity.car as u32).read(m),
        atlas: heap(g.atlases.get(index).copied().unwrap_or(0)),
        pixels: heap(g.rim_pixels.get(index).copied().unwrap_or(0)),
        materials: w.material_info.read_n(m, w.material_count as u32),
        entity,
    };
    rim_redraw(rom, data, &f, &mut m.ewram)
}
