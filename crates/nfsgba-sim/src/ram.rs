//! The adapters between the GBA RAM image and the typed [`CarWorld`]: load every entity with its driver (a car's
//! physics struct or a traffic car's block), the globals, the wall pieces and the racing line; run a typed
//! function (the car step, the opponents' AI, the traffic); store it all back. The heap blocks and the sector
//! lists stay the RAM image's: the typed step records them and the adapter replays them (`HeapOp`, `ListOp`).
//! A few RAM-image twins of typed functions remain for the tests and the game loop (`route`, `contact`).

use nfsgba_formats::career::{LinePoint, RacingLine, Section};

use crate::carworld::{CarWorld, HeapOp, ListOp, PointExtra, Route, Slot};
use crate::data::GameData;
use crate::layout::{Field, Ptr};
use crate::mem::Mem;
use crate::sound::Command;
use crate::state::{
    Car, CarGlobals, CarProfile, Entity, Query, SectionRec, TrafficBlock, WORLD, WaypointRec, WorldHeader,
};
use crate::world;
use crate::{Result, Sim, heap};

/// The plane table and the back table of the racing line (`*0x03005FB4`, `*0x03005FB8`), and the profile.
const PLANES: u32 = 0x0300_5FB4;
const BACK: u32 = 0x0300_5FB8;
const PROFILE: u32 = 0x0300_56EC;
/// The player's entity (the camera follows it).
const PLAYER_ENTITY: u32 = 0x0300_53AC;
/// Per car in the save data: 0x11 bytes, the 10 upgrade bytes from `+7`.
const CAR_SAVE: u32 = crate::decal::CAR_SAVE;
/// Sections loaded from world `+0x40`: the table is a heap block of this many records.
const SECTIONS: u32 = 10;

fn in_ewram(addr: u32, size: u32) -> bool {
    addr >> 24 == 2 && (addr & 0x3_FFFF) + size <= 0x4_0000
}

/// Loads the car step's world for the car `who` (and the racers around it) from the RAM image. `rom` and `data`
/// are the image's ROM and tables, taken out of it by [`with_world`].
pub fn load<'a>(m: &Mem, rom: &'a [u8], data: &'a GameData, who: usize) -> CarWorld<'a> {
    let g = CarGlobals::load(m, 0);
    let hdr = WorldHeader::load(m, WORLD);
    let total = hdr.first_entity as usize + hdr.entity_count as usize;
    let slots = (0..total as u32)
        .map(|k| {
            let e = Entity::load(m, hdr.entities.at(k).addr);
            let has_car = matches!(e.handler, 0..=3 | 0x29) && in_ewram(e.driver.addr, Car::SIZE);
            let c = if has_car { e.driver.read(m) } else { Car::default() };
            let block = (e.handler == 0x36 && in_ewram(e.driver.addr, TrafficBlock::SIZE))
                .then(|| TrafficBlock::load(m, e.driver.addr));
            Slot { e, c, has_car, block }
        })
        .collect::<Vec<_>>();
    let grid = if hdr.templates.is_null() {
        Vec::new()
    } else {
        (0..(g.opponents + 2).min(total as u32))
            .map(|k| {
                // (The templates are in the ROM, which the adapter has taken out of `m`.)
                let at = hdr.templates.addr + 0xA4 * k;
                let word = |a: u32| match a >> 24 {
                    8 => i32::from_le_bytes(rom[(a & 0xFF_FFFF) as usize..][..4].try_into().unwrap()),
                    _ => m.i32(a),
                };
                [word(at + 0xC), word(at + 0x14)]
            })
            .collect()
    };
    let profile_at = m.u32(PROFILE);
    let pieces = if hdr.pieces.is_null() {
        Vec::new()
    } else {
        hdr.pieces.read_n(m, hdr.piece_count as u32)
    };
    let piece_kind = (0..pieces.len() as u32)
        .map(|k| m.i16(hdr.pieces.addr + 0x20 * k + 0x16))
        .collect();
    let offsets = if hdr.sector_offsets.is_null() {
        Vec::new()
    } else {
        hdr.sector_offsets.read_n(m, hdr.sector_offset_count as u32)
    };
    let save = slots
        .get(who)
        .filter(|s| m.u32(CAR_SAVE) != 0 && s.e.car < 15)
        .map_or([0; 10], |s| {
            std::array::from_fn(|k| m.u8(m.u32(CAR_SAVE) + s.e.car as u32 * 0x11 + 7 + k as u32))
        });
    CarWorld {
        rom,
        data,
        route: load_route(m, &hdr, &g),
        slots,
        profile: CarProfile::load(m, profile_at),
        query: Query::load(m, 0),
        pieces,
        piece_kind,
        offsets,
        walls_base: hdr.walls,
        entities: hdr.entities,
        camera_player: Ptr::<Entity>::new(m.u32(PLAYER_ENTITY)).index_from(hdr.entities),
        save,
        profile_addr: profile_at,
        grid,
        extra: hdr.first_entity as usize..total,
        sounds: Vec::new(),
        heap_ops: Vec::new(),
        list_ops: Vec::new(),
        g,
    }
}

fn load_route(m: &Mem, hdr: &WorldHeader, g: &CarGlobals) -> Route {
    let sections: Vec<Section> = if hdr.sections == 0 {
        Vec::new()
    } else {
        (0..SECTIONS)
            .map(|k| SectionRec::load(m, hdr.sections + 8 * k))
            .map(|s| Section {
                count: s.count,
                flags: s.flags,
                first: s.first,
            })
            .collect()
    };
    let recs: Vec<WaypointRec> = if hdr.racing_line == 0 {
        Vec::new()
    } else {
        (0..0x100)
            .map(|k| WaypointRec::load(m, hdr.racing_line + 0x18 * k))
            .collect()
    };
    let points = recs
        .iter()
        .map(|r| LinePoint {
            x: r.x,
            z: r.z,
            link_section: r.link_section,
            link_index: r.link_index,
            distance: r.distance,
        })
        .collect();
    let extra = recs
        .iter()
        .map(|r| PointExtra {
            heading: r.heading,
            sector: r.sector,
        })
        .collect();
    let scales = g.scales[..sections.len()].to_vec();
    let (planes_at, back_at) = (m.u32(PLANES), m.u32(BACK));
    Route {
        line: RacingLine {
            sections,
            points,
            scales,
        },
        extra,
        planes: if planes_at == 0 {
            Vec::new()
        } else {
            Ptr::<[i32; 8]>::new(planes_at).read_n(m, 0x100)
        },
        back: if back_at == 0 {
            [0; 256]
        } else {
            <[i32; 256]>::load(m, back_at)
        },
    }
}

/// Stores the world back: everything the typed step may have changed.
pub fn store(m: &mut Mem, w: &CarWorld) {
    let hdr = WorldHeader::load(m, WORLD);
    for (k, s) in w.slots.iter().enumerate() {
        s.e.store(m, hdr.entities.at(k as u32).addr);
        if s.has_car {
            s.e.driver.write(m, &s.c);
        }
        if let Some(b) = &s.block {
            b.store(m, s.e.driver.addr);
        }
    }
    let mut g = w.g.clone();
    g.scales[..w.route.line.scales.len()].copy_from_slice(&w.route.line.scales);
    g.store(m, 0);
    w.profile.store(m, m.u32(PROFILE));
    w.query.store(m, 0);
    for (k, p) in w.pieces.iter().enumerate() {
        hdr.pieces.at(k as u32).write(m, p);
    }
    if hdr.racing_line != 0 {
        for (k, (p, x)) in w.route.line.points.iter().zip(&w.route.extra).enumerate() {
            let rec = WaypointRec {
                x: p.x,
                z: p.z,
                heading: x.heading,
                link_section: p.link_section,
                link_index: p.link_index,
                distance: p.distance,
                sector: x.sector,
            };
            rec.store(m, hdr.racing_line + 0x18 * k as u32);
        }
    }
}

/// `rand_table` (`FUN_0815fcfc`): the next entry of the 256-entry table at 0x087C03F0.
pub fn rand(m: &mut Mem) -> u32 {
    let mut k = m.u32(0x0300_64C8);
    let r = nfsgba_fixed::rand_table(&m.rom, &mut k);
    m.set_u32(0x0300_64C8, k);
    r
}

/// Loads the world for car `who`, runs `f`, replays what it recorded for the heap (the traffic cars' blocks),
/// stores the world back and replays the sector-list changes; returns `f`'s result and the sound commands it
/// issued.
pub fn with_world<R>(m: &mut Mem, who: usize, f: impl FnOnce(&mut CarWorld) -> R) -> (R, Vec<Command>) {
    let data = m.data().clone();
    let rom = std::mem::take(&mut m.rom);
    let mut w = load(m, &rom, &data, who);
    let r = f(&mut w);
    for op in std::mem::take(&mut w.heap_ops) {
        match op {
            HeapOp::Alloc(k) => w.slots[k].e.driver = Ptr::new(heap::alloc_zeroed(m, TrafficBlock::SIZE)),
            HeapOp::Free(k) => {
                heap::free(m, w.slots[k].e.driver.addr);
                w.slots[k].e.driver = Ptr::NULL;
            }
        }
    }
    store(m, &w);
    for op in std::mem::take(&mut w.list_ops) {
        match op {
            ListOp::Unlink(k, sector) => {
                // The entity's sector is the one it left.
                let e = world::entity(m, k as u32);
                let now = m.u16(e + 0x78);
                m.set_u16(e + 0x78, sector);
                world::unlink_entity(m, k as u32);
                m.set_u16(e + 0x78, now);
            }
            ListOp::Link(k) => world::link_entity(m, k as u32),
        }
    }
    let sounds = std::mem::take(&mut w.sounds);
    drop(w);
    m.rom = rom;
    (r, sounds)
}

/// Like [`with_world`], for the entity at `e`.
fn with_car<R>(m: &mut Mem, e: u32, f: impl FnOnce(&mut CarWorld, usize) -> R) -> (R, Vec<Command>) {
    let i = m.u16(e) as usize;
    with_world(m, i, |w| f(w, i))
}

/// `FUN_0814bd4c`: the car handler for the entity at `e`, on the RAM image. The sector list bookkeeping and the
/// physics struct's allocation (the game's heap) and the player's decal are RAM-image code that this adapter runs
/// around the typed step.
pub fn car_handler(sim: &mut Sim, e: u32) -> Result<()> {
    let m = &mut sim.mem;
    let i = m.u16(e) as usize;
    world::unlink_entity(m, i as u32);
    let state = m.u16(e + 0x4A);
    if state == 0 {
        let p = heap::alloc_zeroed(m, 0x4FC);
        m.set_u32(e + 0x8C, p);
    }
    let ((), sounds) = with_world(m, i, |w| crate::car::handler(w, i));
    sim.sounds.extend(sounds);
    let m = &mut sim.mem;
    if state == 0 && i as u32 == m.u32(0x0300_0060) {
        crate::decal::unpack_decal(m, e);
    }
    world::link_entity(m, i as u32);
    Ok(())
}

/// `FUN_0814a2a0`: the opponent handler (0x29) for the entity at `e`: the sector list and the physics struct's
/// allocation around the typed AI.
pub fn ai_handler(sim: &mut Sim, e: u32) -> Result<Option<crate::ai::Effects>> {
    let m = &mut sim.mem;
    let i = m.u16(e) as usize;
    let state = m.u16(e + 0x4A);
    let driving = state.wrapping_sub(1) < 2;
    if driving {
        world::unlink_entity(m, i as u32);
    } else if state == 0 {
        let p = heap::alloc_zeroed(m, 0x4FC);
        m.set_u32(e + 0x8C, p);
    }
    let (r, sounds) = with_world(m, i, |w| crate::ai::handler(w, i));
    sim.sounds.extend(sounds);
    if driving {
        world::link_entity(&mut sim.mem, i as u32);
    }
    r
}

/// `FUN_081443fc`: the traffic handler (0x36) for the entity at `e`.
pub fn traffic_handler(sim: &mut Sim, e: u32) -> Result<()> {
    let i = sim.mem.u16(e) as usize;
    let (r, sounds) = with_world(&mut sim.mem, 0, |w| crate::traffic_ai::handler(w, i));
    sim.sounds.extend(sounds);
    r
}

/// `FUN_08143d48`: spawn a traffic car of `kind` near the entity at `near`; its entity index, or 0xFFFF.
pub fn traffic_spawn(m: &mut Mem, near: u32, kind: u32) -> Result<u32> {
    let near = m.u16(near) as usize;
    let (slot, _) = with_world(m, 0, |w| crate::traffic::spawn(w, near, kind));
    Ok(slot.map_or(0xFFFF, |k| k as u32))
}

/// The RAM-image twins of `route`.
pub mod route {
    use super::*;

    /// The race time (frames), which the game loop lends the AI.
    pub const RACE_TIME: u32 = 0x0300_5800;

    /// `FUN_0813f098`.
    pub fn lap(m: &mut Mem, e: u32) -> Result<()> {
        with_car(m, e, crate::route::lap);
        Ok(())
    }

    /// `FUN_0814078c`.
    pub fn wingman_command(m: &mut Mem) -> Result<()> {
        with_world(m, 0, crate::route::wingman_command);
        Ok(())
    }
}

/// The RAM-image twins of `contact`.
pub mod contact {
    use super::*;

    pub fn suspension(m: &mut Mem, e: u32, pts: &mut [[i32; 3]], sectors: &mut [u16], dt: i32) -> i32 {
        with_car(m, e, |w, i| crate::contact::suspension(w, i, pts, sectors, dt)).0
    }
}

/// `career::Race` and `career::Racer` of the RAM image (for the callers that still keep the racers there).
pub fn race(m: &Mem) -> nfsgba_formats::career::Race {
    let g = CarGlobals::load(m, 0);
    let mut results = [0; 0x40];
    results[..32].copy_from_slice(&g.results);
    results[32..].copy_from_slice(&g.results_b);
    nfsgba_formats::career::Race {
        mode: g.mode,
        lapped: g.circuit != 0,
        laps: g.laps,
        opponents: g.opponents,
        time: g.time,
        finished: g.finished != 0,
        view: g.focus,
        player: g.player,
        difficulty: g.difficulty,
        state48: g.phase as u32,
        rand: g.rand,
        wrong_way: g.wrong_way != 0,
        results,
    }
}

pub fn racer(m: &Mem, e: u32) -> nfsgba_formats::career::Racer {
    let ent = Entity::load(m, e);
    Slot {
        c: ent.driver.read(m),
        e: ent,
        has_car: true,
        block: None,
    }
    .racer()
}

/// Writes car `e`'s `Racer` fields back.
pub fn store_racer(m: &mut Mem, e: u32, r: &nfsgba_formats::career::Racer) {
    let ent = Entity::load(m, e);
    let mut s = Slot {
        c: ent.driver.read(m),
        e: ent,
        has_car: true,
        block: None,
    };
    s.set_racer(r);
    s.e.store(m, e);
    s.e.driver.write(m, &s.c);
}
