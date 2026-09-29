//! Who races and how they look, read from the race's [`World`] for the viewer: the racers as
//! `draw_sector_entities` sees them, their cars and paints, the player's car record, the route and the environment.

use crate::world::World;

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
    pub fn read(w: &World) -> RaceView {
        let racer = |i: u32| {
            let e = &w.slots[i as usize].e;
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
        let r = &w.records[w.cars[0] as usize];
        let mut record = [0; 0x11];
        record[..7].copy_from_slice(&[r.spoiler, r.u_01, r.rim, r.exhaust, r.u_04, r.paint, r.glass]);
        record[7..].copy_from_slice(&r.upgrades);
        RaceView {
            racers: [0, 1, 2, 3].map(|k| racer((w.g.player + k) % 4)),
            cars: w.cars,
            paints: w.paints,
            record,
            route: w.g.route_index as usize,
            env: w.g.level as usize,
        }
    }
}

/// Vehicle matrix slot `s`, built for the frame's camera.
pub fn matrix(w: &World, s: u8) -> [i32; 12] {
    w.matrices[s as usize]
}

/// The player's atlas as the race holds it (entity `+0x84`, when draw flag bit 3), `len` bytes, rim and all.
pub fn player_atlas(w: &World, len: usize) -> Option<Vec<u8>> {
    let e = &w.slots[w.g.player as usize].e;
    (e.flags & 8 != 0).then(|| {
        let at = (e.atlas & 0x3_FFFF) as usize;
        w.heap[at..at + len].to_vec()
    })
}

/// The race's base palette (the city's with the racers' car ramps, before the light tint).
pub fn base_palette(w: &World) -> Vec<u16> {
    w.palette_base.clone()
}
