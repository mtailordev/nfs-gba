//! The car step's adapters between the GBA RAM image and the typed [`CarWorld`]: load the racers' entities and
//! cars, the globals, the wall pieces and the racing line; run a typed function; store it all back. Also the
//! RAM-image twins of the typed functions that the unmigrated opponents' AI, the traffic and the game loop still
//! call (`route`, `car`, `init`, `contact`, `walls`, `body`): each loads what it needs, runs the typed function
//! and stores. They go when those callers are typed (`docs/engine/typed-state.md`).

use nfsgba_formats::career::{LinePoint, RacingLine, Section};

use crate::carworld::{CarWorld, NONE, Pending, PointExtra, Route, Slot};
use crate::data::GameData;
use crate::layout::{Field, Ptr};
use crate::mem::Mem;
use crate::sound::Command;
use crate::state::{Car, CarGlobals, CarProfile, Entity, Query, SectionRec, WORLD, WaypointRec, WorldHeader};
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
    let n = ((g.player + g.opponents + 1).max(g.racers + 1).max(g.opponents + 2) as usize)
        .max(who + 1)
        .min(total);
    let slots = (0..n as u32)
        .map(|k| {
            let e = Entity::load(m, hdr.entities.at(k).addr);
            let has_car = in_ewram(e.driver.addr, Car::SIZE);
            let c = if has_car { e.driver.read(m) } else { Car::default() };
            Slot { e, c, has_car }
        })
        .collect::<Vec<_>>();
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
        spawned: None,
        pending: None,
        sounds: Vec::new(),
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

/// Loads the world for car `who`, runs `f`, stores the world back; returns `f`'s result and the sound commands
/// it issued.
pub fn with_world<R>(m: &mut Mem, who: usize, f: impl FnOnce(&mut CarWorld) -> R) -> (R, Vec<Command>) {
    let data = m.data().clone();
    let rom = std::mem::take(&mut m.rom);
    let mut w = load(m, &rom, &data, who);
    let r = f(&mut w);
    store(m, &w);
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

/// Runs `f` on the world for the entity at `e` without storing it (for the functions that only read).
fn read_car<R>(m: &Mem, e: u32, f: impl FnOnce(&CarWorld, usize) -> R) -> R {
    let i = m.u16(e) as usize;
    let data = m.data().clone();
    f(&load(m, &m.rom, &data, i), i)
}

/// `FUN_0814bd4c`: the car handler for the entity at `e`, on the RAM image. The sector list bookkeeping and the
/// physics struct's allocation (the game's heap), the traffic spawner and the player's decal are RAM-image code
/// that this adapter runs around the typed step.
pub fn car_handler(sim: &mut Sim, e: u32) -> Result<()> {
    let m = &mut sim.mem;
    let i = m.u16(e) as usize;
    world::unlink_entity(m, i as u32);
    let state = m.u16(e + 0x4A);
    if state == 0 {
        let p = heap::alloc_zeroed(m, 0x4FC);
        m.set_u32(e + 0x8C, p);
    }
    let (mut resume, mut pending, mut spawned) = (false, None::<Pending>, None);
    loop {
        let (flow, sounds) = with_world(&mut sim.mem, i, |w| {
            w.pending = pending.take();
            w.spawned = spawned.take();
            let flow = crate::car::handler(w, i, resume);
            pending = w.pending.take();
            flow
        });
        sim.sounds.extend(sounds);
        match flow {
            crate::car::Flow::Done => break,
            crate::car::Flow::Spawn => {
                // `FUN_08143b2c`'s spawn: near the entity the camera follows.
                let m = &mut sim.mem;
                let near = world::entity(m, m.u32(0x0300_57F8));
                spawned = Some(crate::traffic::spawn(m, near, 1)? != NONE);
                resume = true;
            }
        }
    }
    let m = &mut sim.mem;
    if state == 0 && i as u32 == m.u32(0x0300_0060) {
        crate::decal::unpack_decal(m, e);
    }
    world::link_entity(m, i as u32);
    Ok(())
}

/// The RAM-image twins of `route`.
pub mod route {
    use super::*;

    /// Non-zero for a circuit (waypoint indices wrap around section 0).
    pub const CIRCUIT: u32 = 0x0300_608C;
    pub const RACE_TIME: u32 = 0x0300_5800;
    pub const OPPONENTS: u32 = 0x0300_5784;

    fn line(m: &Mem) -> (Route, bool) {
        let hdr = WorldHeader::load(m, WORLD);
        let g = CarGlobals::load(m, 0);
        (load_route(m, &hdr, &g), g.circuit != 0)
    }

    /// The address of waypoint `index` of section `seg` (`FUN_0814007c`).
    pub fn waypoint(m: &Mem, seg: u32, index: i32) -> u32 {
        let first = m.i32(m.u32(world::W_SEGMENTS) + seg * 8 + 4);
        m.u32(world::W_WAYPOINTS)
            .wrapping_add((first.wrapping_add(index) as u32).wrapping_mul(0x18))
    }

    /// `FUN_0813e860`: the index and section of waypoint `index` of section `seg`.
    pub fn advance(m: &Mem, seg: u32, index: i32) -> (i32, u32) {
        let (r, lapped) = line(m);
        let (s, i) = r.line.step(lapped, &r.back, seg as usize, index);
        (i, s as u32)
    }

    /// `FUN_0814009c`: address of the normalised waypoint.
    pub fn waypoint_at(m: &Mem, seg: u32, index: i32) -> u32 {
        let (i, s) = advance(m, seg, index);
        waypoint(m, s, i)
    }

    /// `FUN_0813f098`.
    pub fn lap(m: &mut Mem, e: u32) -> Result<()> {
        with_car(m, e, crate::route::lap);
        Ok(())
    }

    /// `FUN_0814032c`.
    pub fn progress(m: &Mem, e: u32, _p: u32) -> i32 {
        read_car(m, e, crate::route::progress)
    }

    /// `FUN_081402bc`.
    pub fn lateral(m: &Mem, e: u32) -> i32 {
        read_car(m, e, crate::route::lateral)
    }

    /// `FUN_08140274`.
    pub fn nearest_lane(m: &Mem, x: i32, lanes: i32) -> u32 {
        let lane_offsets = m.data().car.lanes;
        let (mut best, mut lane) = (i32::MAX, 0);
        for k in 0..4u32 {
            if (lanes >> k) & 1 != 0 {
                let d = (lane_offsets[k as usize] - x).wrapping_abs();
                if d < best {
                    best = d;
                    lane = k;
                }
            }
        }
        lane
    }

    /// `FUN_0814078c`.
    pub fn wingman_command(m: &mut Mem) -> Result<()> {
        with_world(m, 0, crate::route::wingman_command);
        Ok(())
    }
}

/// The RAM-image twins of `car`.
pub mod car {
    use super::*;

    /// Handling records (0x158 bytes), one per car.
    pub const HANDLING: u32 = 0x087F_1100;

    pub fn auto_shift(m: &mut Mem, e: u32) {
        with_car(m, e, crate::car::auto_shift);
    }

    pub fn nitro(m: &mut Mem, e: u32) {
        let p = m.u32(e + 0x8C);
        let mut c = Car::load(m, p);
        let mut g = CarGlobals::load(m, 0);
        let dt = g.dt;
        crate::car::nitro(&mut c, &mut g, dt);
        c.store(m, p);
        g.store(m, 0);
    }

    pub fn torque(m: &Mem, p: u32, rpm: i32) -> i32 {
        crate::car::torque(&Car::load(m, p), rpm)
    }

    /// A piecewise-linear curve at `table` (count, x of the first and last point, pointer to the y words).
    pub fn curve(m: &Mem, table: u32, x: i32) -> i32 {
        let ys = m.u32(table + 0xC);
        let curve = crate::data::Curve {
            x0: m.i32(table + 4),
            x1: m.i32(table + 8),
            ys: (0..=m.i32(table) as u32).map(|k| m.i32(ys + 4 * k)).collect(),
        };
        curve.eval(x)
    }

    /// `FUN_0814efa8`: put car `e` back on the road at the waypoint at address `wp`.
    pub fn put_back_on_road(m: &mut Mem, e: u32, wp: u32) {
        let index = ((wp - m.u32(world::W_WAYPOINTS)) / 0x18) as usize;
        with_car(m, e, |w, i| crate::car::put_back_on_road(w, i, index));
    }
}

/// The RAM-image twins of `init`.
pub mod init {
    use super::*;

    pub fn nitro_setup(m: &mut Mem, e: u32, _p: u32) {
        with_car(m, e, crate::init::nitro_setup);
    }

    /// `FUN_0814b2a8` with the handling record at `h` (address in ROM).
    pub fn setup_handling(m: &mut Mem, e: u32, _h: u32) {
        let (car, data) = (m.u8(e + 0x89) as usize, m.data().clone());
        with_car(m, e, |w, i| crate::init::setup_handling(w, i, &data.car.handling[car]));
    }

    pub fn race_start_setup(sim: &mut Sim, e: u32) -> Result<()> {
        let (_, sounds) = with_car(&mut sim.mem, e, crate::init::race_start_setup);
        sim.sounds.extend(sounds);
        Ok(())
    }
}

/// The RAM-image twins of `contact`.
pub mod contact {
    use super::*;

    pub const WHEELS: u32 = 0x18C;
    pub const WHEEL_SIZE: u32 = 0x94;

    pub fn tipped(sim: &mut Sim, e: u32, dt: i32) {
        let (_, sounds) = with_car(&mut sim.mem, e, |w, i| crate::contact::tipped(w, i, dt));
        sim.sounds.extend(sounds);
    }

    pub fn suspension(m: &mut Mem, e: u32, pts: &mut [[i32; 3]], sectors: &mut [u16], dt: i32) -> i32 {
        with_car(m, e, |w, i| crate::contact::suspension(w, i, pts, sectors, dt)).0
    }
}

/// The RAM-image twins of `walls`.
pub mod walls {
    use super::*;

    /// `FUN_081459b8`: the largest impulse applied.
    pub fn walls(sim: &mut Sim, e: u32, x: i32, z: i32, _y: i32, sector: u32, recurse: bool) -> Result<i32> {
        let (hit, sounds) = with_car(&mut sim.mem, e, |w, i| crate::walls::walls(w, i, x, z, sector, recurse));
        sim.sounds.extend(sounds);
        Ok(hit)
    }

    pub fn racers(sim: &mut Sim, e: u32, dt: i32) -> Result<()> {
        let (_, sounds) = with_car(&mut sim.mem, e, |w, i| crate::walls::racers(w, i, dt));
        sim.sounds.extend(sounds);
        Ok(())
    }
}

/// The RAM-image twins of `body` (offsets in the physics struct's rigid body).
pub mod body {
    use super::*;
    use crate::state::RigidBody;

    pub const POS: u32 = 0x08;
    pub const MOMENTUM: u32 = 0x30;
    pub const ANG_MOMENTUM: u32 = 0x48;
    pub const VEL: u32 = 0x54;

    pub fn update_velocities(m: &mut Mem, b: u32) {
        let mut body = RigidBody::load(m, b);
        crate::body::update_velocities(&mut body);
        body.store(m, b);
    }

    pub fn integrate(m: &mut Mem, b: u32, dt: i32) {
        let mut body = RigidBody::load(m, b);
        crate::body::integrate(&m.rom, &mut body, dt);
        body.store(m, b);
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
    };
    s.set_racer(r);
    s.e.store(m, e);
    s.e.driver.write(m, &s.c);
}
