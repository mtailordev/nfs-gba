//! The car step's world as typed state: the racers' entities and cars, the globals, the moving wall pieces, the
//! racing line and the sound commands. `ram.rs` loads it from the GBA RAM image and stores it back; the modules
//! of the car step (`car`, `walls`, `contact`, `route`, `init`, `body`) run on it and know no address.
//! `docs/engine/typed-state.md`.

use nfsgba_formats::career::{BackTable, Plane, Race, Racer, RacingLine};
use nfsgba_formats::render::Piece;

use crate::data::GameData;
use crate::layout::Ptr;
use crate::sound::Command;
use crate::state::{Car, CarGlobals, CarProfile, Entity, Query, SectorOffset, TrafficBlock};
use crate::world::Geometry;

pub const NONE: u32 = 0xFFFF;

/// One entity of the racers' range and its driver.
#[derive(Debug, Clone, Default)]
pub struct Slot {
    pub e: Entity,
    pub c: Car,
    /// The entity's driver pointer named a car struct (only those are loaded and stored).
    pub has_car: bool,
    /// A traffic car's block (handler 0x36 with a block).
    pub block: Option<TrafficBlock>,
}

impl Slot {
    /// The fields of the race rules (`career::Racer`).
    pub fn racer(&self) -> Racer {
        let (e, c) = (&self.e, &self.c);
        Racer {
            id: e.index,
            section: e.segment,
            segment: e.waypoint,
            state: e.race_state,
            entity_flags: e.state,
            x: e.pos[0],
            z: e.pos[2],
            place: c.position,
            distance: c.progress,
            best_lap: c.best_lap as u32,
            lap_start: c.lap_start as u32,
            finish: c.u_0bc,
            laps_left: c.laps_left,
            flags: c.route_flags,
            life: c.hunter_life,
            wrong_way: c.wrong_way,
            wall: c.stationary,
            hit: c.u_4f0 as i16,
            knockout: c.body.momentum.map(|v| v as u32),
            side: c.launch,
            section_changed: c.u_4d6,
        }
    }

    /// Writes a `Racer` back (unchanged fields rewrite the same values).
    pub fn set_racer(&mut self, r: &Racer) {
        let (e, c) = (&mut self.e, &mut self.c);
        e.segment = r.section;
        e.waypoint = r.segment;
        e.race_state = r.state;
        e.state = r.entity_flags;
        c.position = r.place;
        c.progress = r.distance;
        c.best_lap = r.best_lap as i32;
        c.lap_start = r.lap_start as i32;
        c.u_0bc = r.finish;
        c.laps_left = r.laps_left;
        c.route_flags = r.flags;
        c.hunter_life = r.life;
        c.wrong_way = r.wrong_way;
        c.stationary = r.wall;
        c.u_4f0 = r.hit as u16;
        c.body.momentum = r.knockout.map(|v| v as i32);
        c.launch = r.side;
        c.u_4d6 = r.section_changed;
    }
}

/// The racing line as the race keeps it (`RacingLine` plus the columns the car step reads), its plane table
/// and the back table.
#[derive(Debug, Clone)]
pub struct Route {
    pub line: RacingLine,
    /// Per point: the heading of its line and the sector it lies in.
    pub extra: Vec<PointExtra>,
    pub planes: Vec<Plane>,
    pub back: BackTable,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PointExtra {
    pub heading: u16,
    pub sector: i32,
}

/// The world the car step runs on.
pub struct CarWorld<'a> {
    pub rom: &'a [u8],
    pub data: &'a GameData,
    /// Entities `0..n` (the racers, and every entity the step looks at) with their drivers.
    pub slots: Vec<Slot>,
    pub g: CarGlobals,
    pub profile: CarProfile,
    pub query: Query,
    pub pieces: Vec<Piece>,
    /// Per moving piece: the word at `+0x16` (1 = a breakable wall).
    pub piece_kind: Vec<i16>,
    pub offsets: Vec<SectorOffset>,
    pub route: Route,
    /// Where the city's walls are (a wall is named by this plus its index times 0x44): the words the car
    /// remembers of the last flag-0x4000 wall.
    pub walls_base: u32,
    /// The entity array (a pointer to entity `i` is `entities.at(i)`).
    pub entities: Ptr<Entity>,
    /// The entity the camera follows and the local player (world's `player_entity`), as an index.
    pub camera_player: u32,
    /// The upgrade bytes of the car being set up (its save data).
    pub save: [u8; 10],
    /// The address of the profile (`curve_ys` points into it).
    pub profile_addr: u32,
    /// Per racer: the grid position the opponents' setup copies (entity template x and z, 8.8).
    pub grid: Vec<[i32; 2]>,
    /// The entities `free_entity` searches for a traffic car.
    pub extra: std::ops::Range<usize>,
    pub sounds: Vec<Command>,
    /// What the step did that the RAM image keeps: heap blocks and sector-list bookkeeping, replayed in order by
    /// the adapter (`ram.rs`).
    pub heap_ops: Vec<HeapOp>,
    pub list_ops: Vec<ListOp>,
}

/// A traffic car's block was allocated or freed (entity index).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeapOp {
    Alloc(usize),
    Free(usize),
}

/// An entity leaves the list of `sector` (its sector when the step started) or joins the list of its sector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListOp {
    Unlink(usize, u16),
    Link(usize),
}

impl CarWorld<'_> {
    pub fn geometry(&self) -> Geometry<'_> {
        Geometry {
            rom: self.rom,
            city: &self.data.city,
            pieces: &self.pieces,
            offsets: &self.offsets,
        }
    }

    /// `FUN_0814fa04`: the sector containing the 8.8 point (`x`, `y`, `z`), searching from `sector`; 0xFFFF if
    /// none. Leaves the query point in the world.
    pub fn find_sector(&mut self, sector: u32, x: i32, y: i32, z: i32) -> u32 {
        self.query.pos = [x >> 8, y >> 8, z >> 8];
        self.query.sector = sector as u16;
        match self.near_query() {
            NONE => self
                .geometry()
                .find_sector_far(self.query.sector as u32, self.query.pos[0], self.query.pos[2]),
            s => s,
        }
    }

    /// `find_camera_sector` on the query point from the query sector.
    pub fn near_query(&self) -> u32 {
        let (q, s) = (self.query.pos, self.query.sector as u32);
        self.geometry().find_sector_near(s, q[0], q[2])
    }

    pub fn floor_height(&self, sector: u32, x: i32, z: i32) -> i32 {
        self.geometry().floor_height(sector, x, z)
    }

    /// The rand table's next value (`FUN_0815fcfc`).
    pub fn rand(&mut self) -> u32 {
        nfsgba_fixed::rand_table(self.rom, &mut self.g.rand)
    }

    /// `atan2_fast` (IWRAM `0x03004470`).
    pub fn atan2_fast(&self, x: i32, z: i32) -> i32 {
        nfsgba_fixed::atan2_fast(self.rom, x, z)
    }

    /// The entity a pointer to the entity array names.
    pub fn entity_of(&self, p: Ptr<Entity>) -> usize {
        p.index_from(self.entities) as usize
    }

    pub fn is_player(&self, i: usize) -> bool {
        i as u32 == self.g.player
    }

    /// The race globals of the rules (`career::Race`).
    pub fn race(&self) -> Race {
        let g = &self.g;
        let mut results = [0; 0x40];
        results[..32].copy_from_slice(&g.results);
        results[32..].copy_from_slice(&g.results_b);
        Race {
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

    /// Writes back what the rules change: someone finished, the results, the rand index.
    pub fn set_race(&mut self, r: &Race) {
        if r.finished != (self.g.finished != 0) {
            self.g.finished = r.finished as i32;
        }
        self.g.results.copy_from_slice(&r.results[..32]);
        self.g.results_b.copy_from_slice(&r.results[32..]);
        self.g.rand = r.rand;
    }
}

/// Two different slots at once.
pub fn pair(slots: &mut [Slot], a: usize, b: usize) -> (&mut Slot, &mut Slot) {
    assert_ne!(a, b);
    if a < b {
        let (l, r) = slots.split_at_mut(b);
        (&mut l[a], &mut r[0])
    } else {
        let (l, r) = slots.split_at_mut(a);
        (&mut r[0], &mut l[b])
    }
}
