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

/// The blocks the boot and the menus keep on the heap for the whole session, in the order they were allocated (the
/// profile first, its 15 car records at `+0xF9`). Identical in all 14 recorded race starts.
const MENU_BLOCKS: [u32; 12] = [1260, 264, 512, 512, 1024, 256, 256, 8, 54784, 9900, 3840, 24];
/// The heap's node table and data area (`heap_init(0x02000000, 0x02000800, 0x3F800)`).
const HEAP_NODES: u32 = 0x0200_0000;
const HEAP_DATA: u32 = 0x0200_0800;

impl Heap {
    /// NOT 1:1 (R24, G1c): the heap of a first race after power-on. The game's heap after `heap_init`, then the
    /// blocks the menus hold (`MENU_BLOCKS`) at the addresses the game's allocator gives them. The bytes of these
    /// blocks and of the blocks the menus allocated and freed are zero (the game's are menu scratch); the race's
    /// blocks land where they do in the game's heap (measured: `docs/FIDELITY.md` R24).
    pub fn menus(rom: &[u8]) -> Heap {
        let mut m = nfsgba_sim::mem::Mem::new(rom.to_vec(), vec![0; 0x4_0000], vec![0; 0x8000]);
        m.set_u32(0x0300_64CC, HEAP_NODES);
        m.set_u32(0x0300_64D0, HEAP_DATA);
        // `heap_init`: node 0 anchors the list (words 0..1), node 1 ends it at the last word.
        let last = 0xFDFF;
        for (node, [own, next, start, end]) in [(0, [0, 1, 0, 1]), (1, [1, 0xFFFF, last, last])] {
            for (k, v) in [own, next, start, end].into_iter().enumerate() {
                m.set_u16(HEAP_NODES + 8 * node + 2 * k as u32, v);
            }
        }
        let profile = nfsgba_sim::heap::alloc_zeroed(&mut m, MENU_BLOCKS[0]);
        for &size in &MENU_BLOCKS[1..] {
            nfsgba_sim::heap::alloc_zeroed(&mut m, size);
        }
        Heap {
            ewram: m.ewram,
            nodes: HEAP_NODES,
            data: HEAP_DATA,
            records: profile + 0xF9,
        }
    }

    /// The heap after a race: the race's blocks freed (their bytes stay, as in the game), the menus' blocks and the
    /// player's atlas and rim buffer kept, which the next race start frees or reuses. Returns them (heap offsets).
    pub fn after_race(rom: &[u8], w: &crate::world::World) -> Option<(Heap, [u32; 2])> {
        let mut m = nfsgba_sim::mem::Mem::new(rom.to_vec(), w.heap.clone(), vec![0; 0x8000]);
        let (nodes, data) = (w.arena[0], w.arena[1]);
        m.set_u32(0x0300_64CC, nodes);
        m.set_u32(0x0300_64D0, data);
        let (atlas, rim) = (w.atlases[0], w.rim_pixels[0]);
        let mut blocks = vec![];
        let mut node = m.u16(nodes + 2) as u32;
        while m.u16(nodes + 8 * node + 2) != 0xFFFF && blocks.len() < 256 {
            blocks.push((m.u16(nodes + 8 * node + 4) as u32, node));
            node = m.u16(nodes + 8 * node + 2) as u32;
        }
        let addr = |start: u32| data + 4 * start;
        let kept = |offset: u32| {
            blocks
                .iter()
                .any(|&(s, _)| offset != 0 && heap_offset(addr(s)) == offset)
        };
        if !(kept(atlas) && kept(rim)) || blocks.len() < MENU_BLOCKS.len() {
            return None;
        }
        for &(start, _) in &blocks[MENU_BLOCKS.len()..] {
            if ![atlas, rim].contains(&heap_offset(addr(start))) {
                nfsgba_sim::heap::free(&mut m, addr(start));
            }
        }
        let heap = Heap {
            ewram: m.ewram,
            nodes,
            data,
            records: w.arena[2],
        };
        Some((heap, [atlas, rim]))
    }

    /// The car records in the profile block.
    pub fn set_records(&mut self, records: &[[u8; 17]]) {
        let at = (self.records & 0x3_FFFF) as usize;
        for (k, r) in records.iter().enumerate() {
            self.ewram[at + 17 * k..][..17].copy_from_slice(r);
        }
    }
}

impl Setup {
    /// NOT 1:1 (R24, G1c): the race start's input for a game that has run only the boot and the menus since power-on
    /// (and perhaps a race: `previous`, [`Heap::after_race`]), without a capture. The race choice is
    /// [`crate::session::apply_choice`]'s; `audio` is the running sound engine and `ticks` the tick counter (the rand
    /// seed); `keys` the keys held (GBA layout); `records` the car records. Everything else is the game's constant or
    /// zero: `dt` and the AI/HUD constants the boot sets (the same in all recorded captures), no earlier race's
    /// camera, results or messages.
    pub fn menus(
        rom: &[u8],
        audio: Engine,
        ticks: u32,
        keys: u16,
        records: &[[u8; 17]],
        previous: Option<&(Heap, [u32; 2])>,
    ) -> Setup {
        let (mut heap, [atlas, rim]) = previous.cloned().unwrap_or_else(|| (Heap::menus(rom), [0; 2]));
        heap.set_records(records);
        let mut g = CarGlobals {
            catch_up: 1,
            spin_bias: 240,
            dt: 100,
            u_6170: 128,
            u_6194: 32,
            u_6198: 400,
            hunter_damage: 576,
            ..CarGlobals::default()
        };
        g.input[0] = keys | 0xFC00;
        Setup {
            g,
            wingman: 0,
            cars: [0; 4],
            paints: [0; 4],
            records: vec![CarRecord::default(); 15],
            music: 0,
            profile: CarProfile::default(),
            contact: 0,
            needle_scale: 0,
            camera: Camera::default(),
            screen: Screen {
                size: [240, 160],
                mode: 8,
                pages: [0x0600_0000, 0x0600_A000],
                ..Screen::default()
            },
            input: Input {
                pressed: 0,
                held: keys | 0xFC00,
            },
            query: Query::default(),
            rect: [0; 4],
            sky: 0,
            hud: Hud {
                units: 0,
                race_state_changed: 0,
                language: 0,
                enabled: 0,
                screen: 0,
                objects: Vec::new(),
                messages: Default::default(),
                oam: [[0; 4]; 128],
                tile_base: 0x200,
            },
            lp: LoopGlobals {
                ticks,
                vblanks: ticks,
                u_5724: ticks,
                irq: 1,
                game_state: 5,
                ..LoopGlobals::default()
            },
            audio,
            gradient: vec![0; 120],
            rim_pixels: [rim, 0, 0, 0],
            atlases: [atlas, 0, 0],
            slot_counter: 0,
            heap,
        }
    }
}

impl Display {
    /// The display memory of a menu screen: its palettes, the two pages and the OBJ tiles in VRAM, the shadow OAM
    /// and the registers the menus set.
    pub fn from_screen(s: &crate::menu::draw::Screen) -> Display {
        let mut vram = vec![0u8; 0x1_8000];
        for (k, page) in s.pages.iter().enumerate() {
            vram[0xA000 * k..][..page.len()].copy_from_slice(page);
        }
        vram[0x1_4000..][..s.obj_tiles.len()].copy_from_slice(&s.obj_tiles);
        let mut io = [0u8; 0x400];
        for (at, v) in [
            (0, s.dispcnt),
            (4, s.dispstat),
            (0x50, s.bldcnt),
            (0x52, s.bldalpha),
            (0x10E, s.timer3),
        ] {
            io[at..at + 2].copy_from_slice(&v.to_le_bytes());
        }
        let le = |h: &[u16]| h.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>();
        Display {
            palette: le(&s.palette),
            vram,
            oam: le(&s.oam.concat()),
            io,
        }
    }
}
