//! The race's typed state (`docs/engine/typed-state.md`): what `Game::frame` runs on. Entities and cars are
//! indexed, the sector lists are index lists, a traffic car's block and a racer's physics struct belong to their
//! entity. [`World::load`] builds it from a machine state (a trace, an emulator dump, the race start's result);
//! nothing stores it back. The subsystems' own frames (`CarWorld`, `CameraFrame`, `Slots`, `HudFrame`) are built
//! from it and written back into it here.

use nfsgba_audio::Engine;
use nfsgba_formats::{
    hud,
    render::{self, Piece},
    ui::{self, Object},
};
use nfsgba_sim::{
    Mem,
    carworld::{CarWorld, PointExtra, Route, Slot},
    data::GameData,
    layout::{Field, Ptr},
    sound::Command,
    state::{
        self, Camera, Car, CarGlobals, CarProfile, CarRecord, Entity, EntityRef, HudMessages, HudVars, Input,
        ListEntry, MaterialInfo, Query, Race, RaceSetup, Screen, SectionRec, SectorOffset, ShadowOam, SlotGlobals,
        Sprite, SpritePool, TrafficBlock, ViewPort, WORLD, WaypointRec, WorldHeader,
    },
};

use crate::{
    Machine,
    camera::{CameraFrame, Racer as CamRacer},
    slots::{M, RimFrame, Slots, rim_redraw},
};

nfsgba_sim::layout! {
    /// The game loop's own globals (IWRAM): the VBlank IRQ's counters and flag, the pause and race-start states,
    /// the game state, and the level descriptor (a ROM address; the sky gradient's start reads it).
    pub struct LoopGlobals: 0 {
        0x0300_0044 ticks: u32,
        0x0300_53B4 vblanks: u32,
        0x0300_5724 u_5724: u32,
        0x0300_5728 irq: u32,
        /// Non-zero: paused (the race time stops).
        0x0300_5398 paused: u32,
        /// 3: the race start set-up runs.
        0x0300_5714 start_state: u32,
        /// 5 in a race (`main_frame`'s state machine).
        0x0300_5808 game_state: u32,
        0x0300_5620 descriptor: u32,
        /// The music playing (-1: none; `snd_stop_all` and `music_stop` set it).
        0x0300_003C music_id: i32,
    }
}

/// The race HUD's own state (`hud::Globals` takes the rest from the car globals).
#[derive(Debug, Clone, PartialEq)]
pub struct Hud {
    /// Speed units, 0 = mph.
    pub units: u32,
    pub race_state_changed: u32,
    pub language: u32,
    /// Non-zero: the HUD is on.
    pub enabled: u32,
    /// The sprite screen's index and its objects.
    pub screen: u16,
    pub objects: Vec<Object>,
    pub messages: hud::Messages,
    /// The shadow OAM (copied to OAM each frame).
    pub oam: ui::Oam,
    /// The OBJ tile base.
    pub tile_base: u16,
}

/// The race's state.
#[derive(Debug, Clone, PartialEq)]
pub struct World {
    /// Every entity with its driver (a car's physics struct, a traffic car's block).
    pub slots: Vec<Slot>,
    /// The sector lists: per sector its first entity (0xFFFF: none), linked through `Entity::next`.
    pub heads: Vec<u16>,
    /// The spare entities (effects and traffic): the first and how many; the racers come before them.
    pub spare: (usize, usize),
    /// The car globals; every global word another subsystem shares is kept here once.
    pub g: CarGlobals,
    pub profile: CarProfile,
    /// Profile `+0x2E4`: cleared each frame; with `gear_changed` it plays the contact sound.
    pub contact: u32,
    pub needle_scale: u8,
    pub query: Query,
    /// The moving wall pieces (with the word at `+0x16`: 1 = breakable) and the sector offsets.
    pub pieces: Vec<Piece>,
    pub piece_kind: Vec<i16>,
    pub offsets: Vec<SectorOffset>,
    pub route: Route,
    /// Per racer: its start position (x, z, 8.8).
    pub grid: Vec<[i32; 2]>,
    /// The city's walls (a ROM address): the car remembers a wall as this plus its index times 0x44.
    pub walls_base: u32,
    /// The entity the camera follows.
    pub camera_player: usize,
    pub camera: Camera,
    pub screen: Screen,
    pub view: ViewPort,
    pub input: Input,
    /// Entry 0 of the visible-sector list (the root portal) and the screen rectangle (left, right, top, bottom).
    pub root: ListEntry,
    pub rect: [i16; 4],
    /// The camera sector has a ceiling: this frame, last frame.
    pub ceiling: [u8; 2],
    /// The sky is visible this frame.
    pub sky: u16,
    /// The 64 matrix slots, the effect sprites and the first OAM entry they use.
    pub matrices: Vec<M>,
    pub pool: Vec<Sprite>,
    pub pool_first: i16,
    /// The slots handed out this frame; the effect lights are on.
    pub slot_counter: u32,
    pub lights: u32,
    /// Per racer: the rim buffer and the atlas, as offsets into `heap` (the fourth atlas is `g.u_6170`).
    pub rim_pixels: [u32; 4],
    pub atlases: [u32; 3],
    /// The car records (0x11 bytes per car).
    pub records: Vec<CarRecord>,
    pub hud: Hud,
    pub lp: LoopGlobals,
    /// Who races: each racer's car and paint.
    pub cars: [i8; 4],
    pub paints: [i8; 4],
    /// The base palette (the city's with the car ramps) and the fade's palette buffer.
    pub palette_base: Vec<u16>,
    pub palette_fade: Vec<u16>,
    /// What `main_frame`'s palette fade moves towards: the sky gradient's target (the level's, from its
    /// descriptor's material; empty without a descriptor) and the OBJ palette (world `+0x34`).
    pub fade_gradient: Vec<u16>,
    pub fade_obj: Vec<u16>,
    /// The sky gradient (the backdrop colour per line pair) and the entry the last VBlank chose.
    pub gradient: Vec<u16>,
    pub gradient_start: usize,
    /// Per city material: its frame and u/v scroll (world `+0x48`), and its size (the ROM's material table).
    pub materials: Vec<(u16, i16, i16)>,
    pub material_info: Vec<MaterialInfo>,
    /// The city's wall count (the renderer finds the moving pieces its walls name).
    pub wall_count: u16,
    pub audio: Engine,
    /// NOT 1:1 (R24): EWRAM, where the race start unpacked the racers' atlases and rim buffers. The renderer
    /// reads the atlases here and the rim redraw draws into them, reading bytes around its buffer as the game's
    /// heap lays them out, the next block's live car state included: each car's physics struct is kept at its
    /// place in the heap (`car_at`, 0: none) for it.
    pub heap: Vec<u8>,
    pub car_at: Vec<u32>,
    /// NOT 1:1 (R24): the IWRAM words the code that allocates in the arena reads: the heap's node table and data,
    /// the car records, the city's material table and texels (RAM and ROM addresses).
    pub arena: [u32; 5],
}

/// Where `World::arena`'s words live in IWRAM.
const ARENA: [u32; 5] = [0x0300_64CC, 0x0300_64D0, 0x0300_539C, WORLD + 0x24, WORLD + 4];
/// The rim buffers' pointers (`World::rim_pixels`) and a scratch place for an entity's index and car.
const RIM_PIXELS: u32 = 0x0300_6094;
const ENTITY: u32 = 0x0300_7000;

/// Samples of the gradient kept (the VBlank's start plus 40 line pairs).
const GRADIENT: usize = 120;
const AUDIO_GLOBALS: u32 = 0x0300_6370;
const WORK_AREA: usize = 0x160C;
/// Sections of the racing line (world `+0x40`).
const SECTIONS: u32 = 10;

fn in_ewram(addr: u32, size: u32) -> bool {
    addr >> 24 == 2 && (addr & 0x3_FFFF) + size <= 0x4_0000
}

/// The sound engine of a machine state.
pub(crate) fn load_audio(m: &Mem) -> Engine {
    let buffer = |k: u32| m.bytes(0x0300_5DEC + 0xB0 * k, 0xB0);
    nfsgba_audio::ram::load(
        m.bytes(m.u32(AUDIO_GLOBALS), WORK_AREA),
        m.bytes(AUDIO_GLOBALS, 16),
        [buffer(0), buffer(1)],
    )
}

/// The heap offset of an EWRAM address (0: none).
pub(crate) fn heap_offset(addr: u32) -> u32 {
    if addr == 0 { 0 } else { addr & 0x3_FFFF }
}

impl World {
    /// The world of a machine state.
    pub fn load(machine: &Machine) -> World {
        let m = &machine.mem;
        let hdr = WorldHeader::load(m, WORLD);
        let g = CarGlobals::load(m, 0);
        let total = hdr.first_entity as usize + hdr.entity_count as usize;
        let slots = (0..total as u32)
            .map(|k| {
                let at = hdr.entities.at(k).addr;
                let e = Entity::load(m, at);
                let d = state::driver(m, at);
                let has_car = matches!(e.handler, 0..=3 | 0x29) && in_ewram(d.addr, Car::SIZE);
                let c = if has_car { d.read(m) } else { Car::default() };
                let block =
                    (e.handler == 0x36 && in_ewram(d.addr, TrafficBlock::SIZE)).then(|| TrafficBlock::load(m, d.addr));
                Slot { e, c, has_car, block }
            })
            .collect::<Vec<_>>();
        let grid = if hdr.templates.is_null() {
            Vec::new()
        } else {
            (0..(g.opponents + 2).min(total as u32))
                .map(|k| {
                    let at = hdr.templates.addr + 0xA4 * k;
                    [m.i32(at + 0xC), m.i32(at + 0x14)]
                })
                .collect()
        };
        let pieces: Vec<Piece> = if hdr.pieces.is_null() {
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
        let race = Race::load(m, 0);
        let profile = race.profile.read(m);
        let (vars, setup, sg, pool) = (
            HudVars::load(m, 0),
            RaceSetup::load(m, 0),
            SlotGlobals::load(m, 0),
            SpritePool::load(m, 0),
        );
        let audio = load_audio(m);
        let (gradient_at, gradient_ptr) = (m.u32(0x0300_56E8), m.u32(0x0300_53B8));
        let palette = |p: u32| Ptr::<u16>::new(p).read_n(m, 256);
        // main_frame's fade target for the gradient: world[0] + level table (world +0x20) [material] +8.
        let descriptor = m.u32(0x0300_5620);
        let fade_gradient = if descriptor == 0 {
            Vec::new()
        } else {
            let mat = m.u16(descriptor + 0x5E) as u32;
            let src = m
                .u32(0x0300_00C0)
                .wrapping_add(m.u32(m.u32(0x0300_00C0 + 0x20) + 36 * mat + 8));
            Ptr::<u16>::new(src).read_n(m, GRADIENT as u32)
        };
        World {
            heads: hdr.sector_heads.read_n(m, hdr.sector_count as u32),
            spare: (hdr.first_entity as usize, hdr.entity_count as usize),
            profile: CarProfile::load(m, race.profile.addr),
            contact: m.u32(race.profile.addr + 0x2E4),
            needle_scale: profile.needle_scale,
            query: Query::load(m, 0),
            route: load_route(m, &hdr, &g),
            pieces,
            piece_kind,
            offsets,
            grid,
            walls_base: hdr.walls,
            camera_player: race.player_entity.index().unwrap_or(0),
            camera: Camera::load(m, 0),
            screen: Screen::load(m, 0),
            view: hdr.view.read(m),
            input: Input::load(m, 0),
            root: hdr.visible.read(m),
            rect: hdr.rect,
            ceiling: profile.ceiling,
            sky: hdr.sky,
            matrices: Ptr::<M>::new(hdr.matrix_slots).read_n(m, 64),
            pool: pool.objects.read_n(m, pool.count.max(0) as u32),
            pool_first: pool.first,
            slot_counter: sg.slot_counter,
            lights: sg.lights,
            rim_pixels: sg.rim_pixels.map(heap_offset),
            atlases: [0, 1, 2].map(|k| heap_offset(sg.atlases[k])),
            records: sg.records.read_n(m, 15),
            hud: Hud {
                units: vars.units,
                race_state_changed: vars.race_state_changed,
                language: vars.language,
                enabled: vars.hud,
                screen: vars.screen,
                objects: vars.objects.read_n(m, crate::view::hud::OBJECTS),
                messages: HudMessages::load(m, 0).slots,
                oam: ShadowOam::load(m, 0).entries,
                tile_base: vars.tile_base,
            },
            lp: LoopGlobals::load(m, 0),
            cars: setup.cars,
            paints: setup.paints,
            palette_base: palette(setup.palette.addr),
            palette_fade: palette(m.u32(0x0300_577C)),
            fade_gradient,
            fade_obj: palette(m.u32(0x0300_00C0 + 0x34)),
            gradient: Ptr::<u16>::new(gradient_ptr).read_n(m, GRADIENT as u32),
            gradient_start: (gradient_at.wrapping_sub(gradient_ptr) / 2) as usize,
            materials: (0..hdr.material_count as u32)
                .map(|k| {
                    let at = hdr.materials + 8 * k;
                    (m.u16(at + 2), m.i16(at + 4), m.i16(at + 6))
                })
                .collect(),
            material_info: hdr.material_info.read_n(m, hdr.material_count as u32),
            wall_count: hdr.wall_count,
            audio,
            heap: m.ewram.clone(),
            arena: ARENA.map(|a| m.u32(a)),
            car_at: slots
                .iter()
                .enumerate()
                .map(|(k, s)| {
                    if s.has_car {
                        heap_offset(state::driver(m, hdr.entities.at(k as u32).addr).addr)
                    } else {
                        0
                    }
                })
                .collect(),
            slots,
            g,
        }
    }

    /// Entity `i`'s unpacked atlas (draw flag bit 3), in the heap: its material's size.
    pub fn atlas(&self, i: usize) -> Option<&[u8]> {
        let e = &self.slots[i].e;
        let info = self.material_info.get(e.material as usize)?;
        let at = (e.atlas & 0x3_FFFF) as usize;
        (e.flags & 8 != 0).then(|| &self.heap[at..at + info.width as usize * info.height as usize])
    }

    /// NOT 1:1 (R24): runs `f` on the heap arena as the game's RAM (EWRAM, the IWRAM words its code reads, and
    /// entity `i`'s index and car at the address `f` gets), then keeps the arena and the rim buffers.
    pub fn on_arena<R>(&mut self, rom: &[u8], i: usize, f: impl FnOnce(&mut Mem, u32) -> R) -> R {
        let mut m = Mem::new(rom.to_vec(), std::mem::take(&mut self.heap), vec![0; 0x8000]);
        for (at, v) in ARENA.into_iter().zip(self.arena) {
            m.set_u32(at, v);
        }
        for (k, &p) in self.rim_pixels.iter().enumerate() {
            m.set_u32(RIM_PIXELS + 4 * k as u32, if p == 0 { 0 } else { 0x0200_0000 | p });
        }
        m.set_u16(ENTITY, self.slots[i].e.index);
        m.set_u8(ENTITY + 0x89, self.slots[i].e.car);
        let r = f(&mut m, ENTITY);
        for (k, p) in self.rim_pixels.iter_mut().enumerate() {
            *p = heap_offset(m.u32(RIM_PIXELS + 4 * k as u32));
        }
        self.heap = m.ewram;
        r
    }

    /// A car's first step (`FUN_0814bd4c`, `FUN_0814a2a0` in state 0): its physics struct, zeroed. NOT 1:1 (R24): it
    /// is allocated in the heap arena, whose layout decides what the rim redraw reads.
    pub fn new_car(&mut self, rom: &[u8], i: usize) {
        let at = self.on_arena(rom, i, |m, _| nfsgba_sim::heap::alloc_zeroed(m, Car::SIZE));
        self.car_at[i] = heap_offset(at);
        (self.slots[i].c, self.slots[i].has_car) = (Car::default(), true);
    }

    pub fn entity_count(&self) -> usize {
        self.spare.0 + self.spare.1
    }

    /// Runs `f` on the car step's world for car `who`: the world's cars, globals, pieces and route move into it
    /// and back. Returns `f`'s result and the sound commands it issued.
    pub fn with_cars<R>(
        &mut self,
        rom: &[u8],
        data: &GameData,
        who: usize,
        f: impl FnOnce(&mut CarWorld) -> R,
    ) -> (R, Vec<Command>) {
        use std::mem::take;
        let save = self
            .slots
            .get(who)
            .and_then(|s| self.records.get(s.e.car as usize))
            .map_or([0; 10], |r| r.upgrades);
        let mut w = CarWorld {
            rom,
            data,
            slots: take(&mut self.slots),
            heads: take(&mut self.heads),
            g: take(&mut self.g),
            profile: take(&mut self.profile),
            query: take(&mut self.query),
            pieces: take(&mut self.pieces),
            piece_kind: take(&mut self.piece_kind),
            offsets: take(&mut self.offsets),
            route: take(&mut self.route),
            walls_base: self.walls_base,
            camera_player: self.camera_player as u32,
            view: self.camera.view,
            matrix_yaw: self.camera.matrix_yaw,
            save,
            grid: take(&mut self.grid),
            extra: self.spare.0..self.entity_count(),
            sounds: Vec::new(),
        };
        let r = f(&mut w);
        self.slots = w.slots;
        self.heads = w.heads;
        self.g = w.g;
        self.profile = w.profile;
        self.query = w.query;
        self.pieces = w.pieces;
        self.piece_kind = w.piece_kind;
        self.offsets = w.offsets;
        self.route = w.route;
        self.grid = w.grid;
        (r, w.sounds)
    }

    /// Runs `f` on the matrix slots' frame (entities and cars copied in and back, the slots and lists moved).
    pub fn with_slots<R>(&mut self, rom: &[u8], data: &GameData, f: impl FnOnce(&mut Slots) -> R) -> R {
        use std::mem::take;
        let g = &self.g;
        let mut f_ = Slots {
            rom,
            data,
            g: SlotGlobals {
                timer3: g.frame_ticks,
                slot_counter: self.slot_counter,
                lights: self.lights,
                records: Ptr::NULL,
                traffic_on: g.u_6090,
                traffic: g.live,
                ai_cars: g.racers as i32,
                physics_orientation: g.settled,
                sparks_from_ai: g.u_618c,
                rand: g.rand,
                rim_pixels: self.rim_pixels,
                atlases: [self.atlases[0], self.atlases[1], self.atlases[2], g.u_6170 as u32],
            },
            phase: g.phase as u32,
            player: g.player,
            link: g.link as u32,
            camera: self.camera.clone(),
            view: self.view.clone(),
            slots: take(&mut self.matrices),
            pool: take(&mut self.pool),
            entities: self.slots.iter().map(|s| s.e.clone()).collect(),
            cars: self.slots.iter().map(|s| s.has_car.then(|| s.c.clone())).collect(),
            heads: take(&mut self.heads),
            traffic: g.live.map(EntityRef::index),
            records: self.records.clone(),
            racer_hits: std::array::from_fn(|k| self.profile.skid_sound[k] as i32),
            spare: self.spare,
            query: (self.query.pos, self.query.sector),
        };
        let r = f(&mut f_);
        let s = f_;
        (self.slot_counter, self.lights) = (s.g.slot_counter, s.g.lights);
        self.g.rand = s.g.rand;
        self.camera = s.camera;
        self.matrices = s.slots;
        self.pool = s.pool;
        self.heads = s.heads;
        (self.query.pos, self.query.sector) = s.query;
        for (k, (e, c)) in s.entities.into_iter().zip(s.cars).enumerate() {
            let slot = &mut self.slots[k];
            slot.e = e;
            match c {
                Some(c) => slot.c = c,
                // A spark took the entity (its driver word cleared).
                None if slot.e.handler == 0x34 => (slot.has_car, slot.block) = (false, None),
                None => {}
            }
        }
        r
    }

    /// `car_racing_step`'s rim redraw for entity `i` (the player's wheel rims in the atlas).
    pub fn rim_redraw(&mut self, rom: &[u8], data: &GameData, i: usize) -> nfsgba_sim::Result<()> {
        let s = &self.slots[i];
        let index = s.e.index as usize;
        let atlas = match index {
            0..=2 => self.atlases[index],
            3 => heap_offset(self.g.u_6170 as u32),
            _ => 0,
        };
        let f = RimFrame {
            phase: self.g.phase as u32,
            player: self.g.player,
            view: self.camera.view,
            matrix_yaw: self.camera.matrix_yaw,
            wheel_angle: s.has_car.then_some(s.c.wheel_angle),
            record: self.records[s.e.car as usize].clone(),
            atlas: atlas as usize,
            pixels: self.rim_pixels.get(index).copied().unwrap_or(0) as usize,
            materials: self.material_info.clone(),
            entity: s.e.clone(),
        };
        // NOT 1:1 (R24): the live cars into the heap, where the rim redraw may read them.
        let mut m = Mem::new(Vec::new(), std::mem::take(&mut self.heap), vec![0; 0x8000]);
        for (s, &at) in self.slots.iter().zip(&self.car_at) {
            if s.has_car && at != 0 {
                s.c.store(&mut m, 0x0200_0000 + at);
            }
        }
        self.heap = m.ewram;
        rim_redraw(rom, data, &f, &mut self.heap)
    }

    /// What `camera_update` reads and writes.
    pub fn camera_frame(&self) -> CameraFrame {
        let racer = |i: usize| CamRacer {
            entity: self.slots[i].e.clone(),
            car: self.slots[i].c.clone(),
        };
        CameraFrame {
            camera: self.camera.clone(),
            screen: self.screen.clone(),
            view: self.view.clone(),
            input: self.input.clone(),
            phase: self.g.phase as u32,
            player: racer(self.g.player as usize),
            target: racer(self.camera_player),
            focus: self.slots[self.g.focus as usize].e.clone(),
            pieces: self.pieces.clone(),
            offsets: self.offsets.clone(),
            query: (self.query.sector, self.query.pos[0], self.query.pos[2]),
            root: self.root.clone(),
            rect: self.rect,
            ceiling: self.ceiling,
        }
    }

    pub fn set_camera_frame(&mut self, f: CameraFrame) {
        (self.camera, self.screen, self.input, self.view, self.root) = (f.camera, f.screen, f.input, f.view, f.root);
        (self.query.sector, self.query.pos[0], self.query.pos[2]) = f.query;
        (self.rect, self.ceiling) = (f.rect, f.ceiling);
    }

    /// The HUD's frame.
    pub fn hud_frame(&mut self) -> crate::view::hud::HudFrame {
        let g = &self.g;
        let globals = hud::Globals {
            hud: self.hud.enabled,
            mode: g.mode as i32,
            language: self.hud.language,
            units: self.hud.units,
            frames: g.time as i32,
            split: g.gap,
            opponents: g.opponents,
            ai_cars: g.racers,
            laps: g.laps as u32,
            wingman: g.wingman as u32,
            portrait: g.wingman_commands,
            portrait_blink: g.u_61d4,
            bar: g.u_61e4 as i32,
            bar_max: g.u_6188 as i32,
            arrow: g.off_route,
            route: g.u_5388,
            needle_scale: self.needle_scale,
            player: g.focus as usize,
            race_state: g.phase as u32,
            race_state_changed: self.hud.race_state_changed,
        };
        let racers = std::array::from_fn(|i| {
            let s = &self.slots[i];
            hud::Racer {
                x: s.e.pos[0],
                z: s.e.pos[2],
                heading: s.e.heading,
                driver: s.has_car.then(|| {
                    let c = &s.c;
                    hud::Driver {
                        revs: c.revs,
                        gear: c.gear,
                        speed: c.speed,
                        position: c.position,
                        laps_left: c.laps_left,
                        rev_scale: c.max_rpm,
                        dial: c.nitro_tank,
                        flags: c.route_flags,
                        hunter_life: c.hunter_life,
                    }
                }),
            }
        });
        crate::view::hud::HudFrame {
            g: globals,
            racers,
            objects: std::mem::take(&mut self.hud.objects),
            messages: self.hud.messages,
            oam: self.hud.oam,
        }
    }

    /// Writes back what the HUD changes: the race state past the time limit, the portrait and bar animation,
    /// the objects, the messages and the shadow OAM.
    pub fn set_hud_frame(&mut self, f: crate::view::hud::HudFrame) {
        let g = &mut self.g;
        g.phase = f.g.race_state as i32;
        self.hud.race_state_changed = f.g.race_state_changed;
        g.wingman_commands = f.g.portrait;
        g.u_61d4 = f.g.portrait_blink;
        g.u_61e4 = f.g.bar as u32;
        (self.hud.objects, self.hud.messages, self.hud.oam) = (f.objects, f.messages, f.oam);
    }

    /// The renderer's entities.
    pub fn render_entities(&self) -> Vec<render::Entity> {
        self.slots
            .iter()
            .map(|s| {
                let e = &s.e;
                render::Entity {
                    index: e.index,
                    next: e.next,
                    draw_next: e.draw_next,
                    state: e.state,
                    flags: e.flags,
                    pos: e.pos,
                    key: e.key,
                    model: e.model,
                    material_step: e.material_step,
                    material_offset: e.material_offset,
                    material: e.material,
                    handler: e.handler,
                    extra_model: e.extra_model,
                    atlas: e.atlas,
                    slot: e.slot,
                }
            })
            .collect()
    }
}

fn load_route(m: &Mem, hdr: &WorldHeader, g: &CarGlobals) -> Route {
    use nfsgba_formats::career::{LinePoint, RacingLine, Section};
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
    let (planes_at, back_at) = (m.u32(0x0300_5FB4), m.u32(0x0300_5FB8));
    Route {
        line: RacingLine {
            points: recs
                .iter()
                .map(|r| LinePoint {
                    x: r.x,
                    z: r.z,
                    link_section: r.link_section,
                    link_index: r.link_index,
                    distance: r.distance,
                })
                .collect(),
            scales: g.scales[..sections.len()].to_vec(),
            sections,
        },
        extra: recs
            .iter()
            .map(|r| PointExtra {
                heading: r.heading,
                sector: r.sector,
            })
            .collect(),
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
