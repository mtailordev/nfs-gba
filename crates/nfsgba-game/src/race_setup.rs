//! What the race start takes and gives besides the [`World`](crate::world::World): the typed [`Setup`] (the race the
//! menus chose, the profile and car records, the RNG index and tick counter, and what the menus leave for the race
//! to keep) and the display memory ([`Display`]). [`Setup::load`] reads it from a machine state (a capture of the
//! menus); the menus will build it directly (FIDELITY G1c).

use std::{io, path::Path};

use nfsgba_audio::Engine;
use nfsgba_sim::{
    layout::{Field, Ptr},
    state::{
        Camera, CarGlobals, CarProfile, CarRecord, HudMessages, HudVars, Input, Query, Race, RaceSetup, Screen,
        ShadowOam, SlotGlobals, WORLD, WorldHeader,
    },
};

use crate::{
    Machine,
    world::{Hud, LoopGlobals, heap_offset, load_audio},
};

/// The GBA I/O registers (`0x04000000`, 0x400 bytes).
pub type Io = [u8; 0x400];

/// The heap the menus leave (EWRAM): which blocks are allocated (the node table) and what the freed blocks still
/// hold decide where the race's blocks land and what lies around them. NOT 1:1 (R24).
#[derive(Debug, Clone)]
pub struct Heap {
    pub ewram: Vec<u8>,
    /// The node table and data area addresses (`0x030064CC`, `0x030064D0`).
    pub nodes: u32,
    pub data: u32,
    /// The car records' address (`0x0300539C`).
    pub records: u32,
}

/// The race start's input. The race choice lives in [`CarGlobals`] (`level` = environment, `u_5388` = route number,
/// `route_index`, `mode`, `laps`, `opponents`, `difficulty`, `u_5604` = traffic, `link`, `career`, `player`, `rand`,
/// `circuit` = the previous race's lapped flag); the rest is what the race keeps as the menus left it.
#[derive(Debug, Clone)]
pub struct Setup {
    pub g: CarGlobals,
    /// The profile's wingman (`+0x200`).
    pub wingman: u32,
    pub cars: [i8; 4],
    pub paints: [i8; 4],
    pub records: Vec<CarRecord>,
    /// The music playing (`0x0300003C`).
    pub music: i32,
    pub profile: CarProfile,
    pub contact: u32,
    pub needle_scale: u8,
    pub camera: Camera,
    pub screen: Screen,
    pub input: Input,
    pub query: Query,
    pub rect: [i16; 4],
    pub sky: u16,
    /// Units, language, the HUD option and the message slots (the sprite screen is built by the start).
    pub hud: Hud,
    pub lp: LoopGlobals,
    pub audio: Engine,
    pub gradient: Vec<u16>,
    /// The previous race's rim buffers and atlases (heap offsets), and the effect slot counter.
    pub rim_pixels: [u32; 4],
    pub atlases: [u32; 3],
    pub slot_counter: u32,
    pub heap: Heap,
}

/// The display memory the race start writes: VRAM (page clears, sprite tiles), OAM and the I/O registers.
#[derive(Debug, Clone)]
pub struct Display {
    pub palette: Vec<u8>,
    pub vram: Vec<u8>,
    pub oam: Vec<u8>,
    pub io: Io,
}

impl Setup {
    /// The race the start builds: environment, route number and index, mode, and the player's car.
    pub fn choose(&mut self, env: u32, route: u32, mode: u32, car: u8) {
        self.g.level = env as i32;
        self.g.u_5388 = route;
        self.g.route_index = route;
        self.g.mode = mode;
        self.cars[0] = car as i8;
    }

    /// From a machine state the menus left (a capture at the entry of `race_start_from_table_a`).
    pub fn load(machine: &Machine) -> Setup {
        let m = &machine.mem;
        let hdr = WorldHeader::load(m, WORLD);
        let race = Race::load(m, 0);
        let (vars, setup, sg) = (HudVars::load(m, 0), RaceSetup::load(m, 0), SlotGlobals::load(m, 0));
        let profile = race.profile.read(m);
        let gradient = Ptr::<u16>::new(m.u32(0x0300_53B8)).read_n(m, 120);
        Setup {
            g: CarGlobals::load(m, 0),
            wingman: m.u32(race.profile.addr + 0x200),
            cars: setup.cars,
            paints: setup.paints,
            records: sg.records.read_n(m, 15),
            music: m.i32(0x0300_003C),
            profile: CarProfile::load(m, race.profile.addr),
            contact: m.u32(race.profile.addr + 0x2E4),
            needle_scale: profile.needle_scale,
            camera: Camera::load(m, 0),
            screen: Screen::load(m, 0),
            input: Input::load(m, 0),
            query: Query::load(m, 0),
            rect: hdr.rect,
            sky: hdr.sky,
            hud: Hud {
                units: vars.units,
                race_state_changed: vars.race_state_changed,
                language: vars.language,
                enabled: vars.hud,
                screen: vars.screen,
                objects: Vec::new(),
                messages: HudMessages::load(m, 0).slots,
                oam: ShadowOam::load(m, 0).entries,
                tile_base: vars.tile_base,
            },
            lp: LoopGlobals::load(m, 0),
            audio: load_audio(m),
            gradient,
            rim_pixels: sg.rim_pixels.map(heap_offset),
            atlases: [0, 1, 2].map(|k| heap_offset(sg.atlases[k])),
            slot_counter: sg.slot_counter,
            heap: Heap {
                ewram: m.ewram.clone(),
                nodes: m.u32(0x0300_64CC),
                data: m.u32(0x0300_64D0),
                records: m.u32(0x0300_539C),
            },
        }
    }
}

impl Display {
    pub fn new(machine: &Machine, io: Io) -> Display {
        Display {
            palette: machine.palette.clone(),
            vram: machine.vram.clone(),
            oam: machine.oam.clone(),
            io,
        }
    }
}

/// A pre-race capture (`PREFIX.<domain>.bin`, e.g. `race-init/circuit_pre`): the setup and the display memory.
pub fn load_pre(rom: Vec<u8>, prefix: &Path) -> io::Result<(Setup, Display)> {
    let machine = Machine::load_dump(rom, prefix)?;
    let mut io = prefix.as_os_str().to_owned();
    io.push(".io.bin");
    let io = std::fs::read(io)?;
    let io: Io = io
        .get(..0x400)
        .and_then(|b| b.try_into().ok())
        .ok_or(io::ErrorKind::InvalidData)?;
    Ok((Setup::load(&machine), Display::new(&machine, io)))
}
