//! Who races and how they look, read from the game's RAM for the viewer: the racers as `draw_sector_entities`
//! sees them, their cars and paints, the player's car record, the route and the environment.

use nfsgba_sim::{
    Mem,
    layout::Field,
    state::{Race, RaceSetup, SlotGlobals, WORLD, WorldHeader},
};

/// A racer as `draw_sector_entities` and the viewer see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Racer {
    /// Position, 8.8 fixed point, city units (`-y` is up).
    pub pos: [i32; 3],
    /// Heading, 0x4000 per turn.
    pub heading: i32,
    /// The sector the racer is in.
    pub sector: u16,
    /// Vehicle matrix slot; 0xFF = not drawn.
    pub slot: u8,
    /// Draw flags (bit 0 whole-screen clip, bit 1 always the near model, bit 6 far model beyond 0x1000).
    pub flags: u16,
    /// The far model (drawn at depth >= 0x200; the one before it nearer).
    pub model: i16,
    /// The spoiler model on the next matrix slot (0: none).
    pub extra: i16,
}

impl Racer {
    /// The models `draw_sector_entities` draws for this racer at camera depth `d` (`render::entities`), body
    /// then spoiler; none beyond depth 0x2000, nor beyond 0x1000 without flag bit 6, nor without a matrix slot.
    pub fn models_at(&self, d: i32) -> Vec<usize> {
        let mut d = d;
        if d as u32 >= 0x2000 || self.slot == 0xFF || self.model == 0 {
            return Vec::new();
        }
        if self.flags & 2 != 0 {
            d = 0;
        }
        if d > 0x1000 {
            if self.flags & 0x40 == 0 {
                return Vec::new();
            }
            d = 0x200;
        }
        let body = (self.model + (d >= 0x200) as i16 - 1) as usize;
        let spoiler = self.extra.unsigned_abs() as usize;
        match self.extra {
            0 => vec![body],
            n if n < 0 => vec![spoiler, body],
            _ => vec![body, spoiler],
        }
    }
}

/// The four racers (the player first), car and paint per racer, the player's car record, the route and the
/// environment.
#[derive(Debug, Clone)]
pub struct RaceView {
    pub racers: [Racer; 4],
    pub cars: [i8; 4],
    pub paints: [i8; 4],
    /// The player's car record (0x11 bytes at `records + 0x11*car`).
    pub record: [u8; 0x11],
    pub route: usize,
    pub env: usize,
}

impl RaceView {
    pub fn read(m: &Mem) -> RaceView {
        let (w, race, setup) = (WorldHeader::load(m, WORLD), Race::load(m, 0), RaceSetup::load(m, 0));
        let racer = |i: u32| {
            let e = w.entities.at(i).read(m);
            Racer {
                pos: e.pos,
                heading: e.heading >> 8,
                sector: e.sector,
                slot: e.slot,
                flags: e.flags,
                model: e.model,
                extra: e.extra_model,
            }
        };
        let records = SlotGlobals::load(m, 0).records;
        RaceView {
            racers: [0, 1, 2, 3].map(|k| racer((race.player + k) % 4)),
            cars: setup.cars,
            paints: setup.paints,
            record: std::array::from_fn(|k| u8::load(m, records.addr + 0x11 * setup.cars[0] as u32 + k as u32)),
            route: setup.route as usize,
            env: setup.env as usize,
        }
    }
}

/// Vehicle matrix slot `s` (world `+0xFC`), built for the frame's camera.
pub fn matrix(m: &Mem, s: u8) -> [i32; 12] {
    let w = WorldHeader::load(m, WORLD);
    <[i32; 12]>::load(m, w.matrix_slots + 0x30 * s as u32)
}

/// The player's atlas as the race holds it in EWRAM (entity `+0x84`, when draw flag bit 3), `len` bytes, rim and all.
pub fn player_atlas(m: &Mem, len: usize) -> Option<Vec<u8>> {
    let (w, race) = (WorldHeader::load(m, WORLD), Race::load(m, 0));
    let e = w.entities.at(race.player).read(m);
    (e.flags & 8 != 0).then(|| m.bytes(e.atlas, len).to_vec())
}

/// The race's base palette (the city's with the racers' car ramps, before the light tint).
pub fn base_palette(m: &Mem) -> Vec<u16> {
    RaceSetup::load(m, 0).palette.read_n(m, 256)
}
