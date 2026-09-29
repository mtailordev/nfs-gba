//! The world struct, the globals the car step uses, sector lookup, floor height and the control bindings.

use nfsgba_formats::{Sector, Wall, render::Piece};

use crate::Result;
use crate::layout::Field;
use crate::math::{mul64, shr64};
use crate::mem::Mem;
use crate::state::{SectorOffset, WorldHeader};

/// The world struct (IWRAM). Fields used here:
/// `+0x0C` sector entity-list heads (u16 per sector), `+0x10` walls (0x44 bytes), `+0x14` sectors (0x30 bytes),
/// `+0x18` dynamic wall states (0x20 bytes, walls whose `+0x2A` is not 0xFFFF), `+0x1C` sloped floor planes
/// (0x14 bytes), `+0x3C` entity array (0xA4 bytes), `+0x40` route segments (8 bytes), `+0x44` route
/// waypoints (0x18 bytes), `+0xC0/+0xC4/+0xC8` query point and `+0xEA` query sector for the sector search.
pub use crate::state::WORLD;
pub const W_SECTOR_HEADS: u32 = WORLD + 0x0C;
pub const W_WALLS: u32 = WORLD + 0x10;
pub const W_SECTORS: u32 = WORLD + 0x14;
pub const W_WALL_STATES: u32 = WORLD + 0x18;
pub const W_PLANES: u32 = WORLD + 0x1C;
pub const W_ENTITIES: u32 = WORLD + 0x3C;
pub const W_SEGMENTS: u32 = WORLD + 0x40;
pub const W_WAYPOINTS: u32 = WORLD + 0x44;
pub const W_QUERY: u32 = WORLD + 0xC0;
pub const W_QUERY_SECTOR: u32 = WORLD + 0xEA;

pub const NONE: u32 = 0xFFFF;

// Globals (IWRAM words unless noted); `docs/engine/physics.md` lists what is known about each.
/// Frame time: 25,500 / timer-3 ticks of the last frame, clamped to 10..100 (`main_frame` `FUN_0812ae64`); 15
/// when 0x03005624 is 2.
pub const DT: u32 = 0x0300_5640;
pub const RACE_PHASE: u32 = 0x0300_0048;
/// Entity index of the local player (sound, HUD and camera side effects happen only for it).
pub const PLAYER: u32 = 0x0300_0060;
/// Per-entity control word (u16): `~KEYINPUT` bits for the player, AI output for the others.
pub const INPUT: u32 = 0x0300_57D8;
/// Control binding set (byte): 0 when 0x03005798 is set, else 1 (`FUN_08144f1c`).
pub const BINDING_SET: u32 = 0x0300_629C;
/// Automatic gearbox when non-zero (read by the dynamics).
pub const AUTOMATIC: u32 = 0x0300_5798;
/// The profile pointer (EWRAM, saved to EEPROM). The car step updates `+0x2D0` distance, `+0x2D8` skid count,
/// `+0x2DC` top speed and the HUD flags `+0x2E0/+0x2E8/+0x2EC`, `+0x318` skid sounds.
pub const PROFILE: u32 = 0x0300_56EC;

/// The control binding table: per binding set 9 actions of (held mask, held value, pressed mask, pressed value).
const BINDINGS: u32 = 0x087F_5494;

pub fn entity(mem: &Mem, index: u32) -> u32 {
    mem.u32(W_ENTITIES) + index * 0xA4
}

pub fn sector_addr(mem: &Mem, sector: u32) -> u32 {
    mem.u32(W_SECTORS) + sector * 0x30
}

pub fn wall_addr(mem: &Mem, wall: u32) -> u32 {
    mem.u32(W_WALLS) + wall * 0x44
}

/// Flags of a wall (`+0x2E`), or of its dynamic state when it has one (`+0x2A`); bit 0x1000 = solid.
/// The RAM-image twin of [`Geometry::wall_flags`] for `walls.rs`; it goes when the car step is typed.
pub fn wall_flags(mem: &Mem, wall: u32) -> u16 {
    match mem.u16(wall + 0x2A) as u32 {
        NONE => mem.u16(wall + 0x2E),
        state => mem.u16(mem.u32(W_WALL_STATES) + state * 0x20 + 0xE),
    }
}

/// `FUN_08144f38`: whether control action `action` is active for the held and newly pressed keys.
pub fn control(mem: &Mem, held: u32, pressed: u32, action: u32) -> bool {
    let b = BINDINGS + (action & 0xFFFF) * 8 + mem.u8(BINDING_SET) as u32 * 0x48;
    (mem.u16(b) as u32 & held) == mem.u16(b + 2) as u32 && (mem.u16(b + 4) as u32 & pressed) == mem.u16(b + 6) as u32
}

/// `FUN_08144f1c`: selects the binding set.
pub fn select_bindings(mem: &mut Mem, automatic: i32) {
    mem.set_u8(BINDING_SET, (automatic == 0) as u8);
}

/// The city geometry the sector searches and the floor lookup read: the ROM's sectors and walls
/// ([`GameData::city`](crate::data::GameData)), the world's moving pieces and sector offsets (world `+0x18`,
/// `+0x1C`), and the ROM image for the reciprocal table.
pub struct Geometry<'a> {
    pub rom: &'a [u8],
    pub city: &'a [Sector],
    pub pieces: &'a [Piece],
    pub offsets: &'a [SectorOffset],
}

/// Runs `f` on the typed geometry of the RAM image (the Mem-based code's way into it).
pub fn geometry<R>(mem: &Mem, f: impl FnOnce(&Geometry) -> R) -> R {
    let w = WorldHeader::load(mem, WORLD);
    let pieces = w.pieces.read_n(mem, w.piece_count as u32);
    let offsets = w.sector_offsets.read_n(mem, w.sector_offset_count as u32);
    f(&Geometry {
        rom: &mem.rom,
        city: &mem.data().city,
        pieces: &pieces,
        offsets: &offsets,
    })
}

impl Geometry<'_> {
    fn sector(&self, s: u32) -> &Sector {
        &self.city[s as usize]
    }

    /// A wall's flags, or its moving piece's when it has one; bit 0x1000 = solid.
    pub fn wall_flags(&self, w: &Wall) -> u16 {
        match w.piece as u32 {
            NONE => w.flags,
            p => self.pieces[p as usize].flags,
        }
    }

    /// `FUN_030047a8` (IWRAM, ARM): whether (`x`, `z`) is inside the convex wall loop of `sector` (wrapping cross
    /// products).
    pub fn inside(&self, sector: u32, x: i32, z: i32) -> bool {
        let walls = &self.sector(sector).walls;
        let n = walls.len();
        (0..n).all(|k| {
            let (p, c) = (&walls[(k + n - 1) % n], &walls[k]);
            (c.z.wrapping_sub(p.z).wrapping_mul(x.wrapping_sub(p.x)))
                .wrapping_sub(z.wrapping_sub(p.z).wrapping_mul(c.x.wrapping_sub(p.x)))
                >= 0
        })
    }

    /// The open portal's sector behind wall `w` (link `+0x32`, flag 0x1000 clear).
    fn open(&self, w: &Wall) -> Option<u32> {
        let link = w.search_link as u32;
        (link != NONE && self.wall_flags(w) & 0x1000 == 0).then_some(link)
    }

    /// A found sector, or the sector it defers its floor to when it has none (`+0x20`).
    fn with_floor(&self, found: u32) -> u32 {
        let f = self.sector(found);
        if f.floor != 0 || f.alias as u32 == NONE {
            found
        } else {
            f.alias as u32
        }
    }

    /// `FUN_03000800` (IWRAM, ARM; `find_camera_sector`): the sector containing (`x`, `z`): `start`, or one
    /// through its open portals, 0xFFFF if neither.
    pub fn find_sector_near(&self, start: u32, x: i32, z: i32) -> u32 {
        if self.inside(start, x, z) {
            return start;
        }
        for w in &self.sector(start).walls {
            if let Some(link) = self.open(w)
                && self.inside(link, x, z)
            {
                return self.with_floor(link);
            }
        }
        NONE
    }

    /// `FUN_0814dbbc`: the sector containing (`x`, `z`) two portals away from `start`: behind each open portal,
    /// the sectors behind that sector's open portals, in wall order. 0xFFFF if none.
    pub fn find_sector_far(&self, start: u32, x: i32, z: i32) -> u32 {
        for w in &self.sector(start).walls {
            let Some(near) = self.open(w) else { continue };
            for v in &self.sector(near).walls {
                if let Some(far) = self.open(v)
                    && self.inside(far, x, z)
                {
                    return self.with_floor(far);
                }
            }
        }
        NONE
    }

    /// `FUN_08144b7c`: the traffic's sector search: `start`, else one through a portal (solid walls only count
    /// with material 0, and walls with a moving piece never count as solid).
    pub fn traffic_find_sector(&self, start: u32, x: i32, z: i32) -> u32 {
        if self.inside(start, x, z) {
            return start;
        }
        for w in &self.sector(start).walls {
            let solid = w.piece as u32 == NONE && w.flags & 0x1000 != 0;
            let link = w.search_link as u32;
            if link != NONE && (!solid || w.material == 0) && self.inside(link, x, z) {
                return self.with_floor(link);
            }
        }
        NONE
    }

    /// `FUN_0814f4a8`: a sector, or the sector it defers its floor to (`+0x20`) when it has no floor of its own
    /// (`+0x08` zero, flag 0x20 of `+0x12` clear).
    pub fn floor_sector(&self, sector: u32) -> u32 {
        let s = self.sector(sector);
        if s.floor == 0 && s.flags & 0x20 == 0 && s.alias as u32 != NONE {
            s.alias as u32
        } else {
            sector
        }
    }

    /// `FUN_0814ca84`: floor height (8.8, `-y` up) of `sector` at (`x`, `z`) city units: the first wall's height
    /// (`+0x38`) plus the sector's (`+0x26`), then the floor plane (the sector's, or a sloped plane from its
    /// offsets record, plus the moving piece's and the record's floor offsets) through the first wall's corner.
    pub fn floor_height(&self, sector: u32, x: i32, z: i32) -> i32 {
        let s = self.sector(self.floor_sector(sector));
        let w = &s.walls[0];
        let mut h = w.floor_y as i32 + s.height as i32;
        let [a, b, c] = if s.offset as u32 == NONE {
            s.plane
        } else {
            let o = &self.offsets[s.offset as usize];
            if w.piece as u32 != NONE {
                h += self.pieces[w.piece as usize].floor as i32;
            }
            h += o.floor as i32;
            o.plane
        };
        let b = if b == 0 { 1 } else { b as i32 };
        let inv = if b < 0 {
            -nfsgba_fixed::recip(self.rom, -b)
        } else {
            nfsgba_fixed::recip(self.rom, b)
        };
        let d = (a as i32)
            .wrapping_mul(x.wrapping_sub(w.x))
            .wrapping_add((c as i32).wrapping_mul(z.wrapping_sub(w.z)));
        (h << 8).wrapping_add(shr64(mul64(d, inv.wrapping_neg()), 16))
    }
}

/// `find_camera_sector` on the query point (world `+0xC0/+0xC8`) from the query sector (`+0xEA`).
pub fn find_sector_near_query(mem: &Mem) -> u32 {
    let (x, z, start) = (mem.i32(W_QUERY), mem.i32(W_QUERY + 8), mem.u16(W_QUERY_SECTOR) as u32);
    geometry(mem, |g| g.find_sector_near(start, x, z))
}

/// `FUN_0814fa04`: the sector containing the 8.8 point (`x`, `y`, `z`), searching from `sector`; 0xFFFF if none.
pub fn find_sector(mem: &mut Mem, sector: u32, x: i32, y: i32, z: i32) -> Result<u32> {
    mem.set_i32(W_QUERY, x >> 8);
    mem.set_i32(W_QUERY + 4, y >> 8);
    mem.set_i32(W_QUERY + 8, z >> 8);
    mem.set_u16(W_QUERY_SECTOR, sector as u16);
    Ok(match find_sector_near_query(mem) {
        NONE => find_sector_far(mem),
        s => s,
    })
}

/// `FUN_0814dbbc` on the query point.
pub fn find_sector_far(mem: &Mem) -> u32 {
    let (x, z, start) = (mem.i32(W_QUERY), mem.i32(W_QUERY + 8), mem.u16(W_QUERY_SECTOR) as u32);
    geometry(mem, |g| g.find_sector_far(start, x, z))
}

/// [`Geometry::floor_sector`], as the sector record's address.
pub fn floor_sector(mem: &Mem, sector: u32) -> u32 {
    sector_addr(mem, geometry(mem, |g| g.floor_sector(sector)))
}

/// [`Geometry::floor_height`].
pub fn floor_height(mem: &Mem, sector: u32, x: i32, z: i32) -> i32 {
    geometry(mem, |g| g.floor_height(sector, x, z))
}

/// `FUN_081375ac`: unlink entity `index` from its sector's entity list (world `+0x0C`, links at entity `+0x02`).
pub fn unlink_entity(mem: &mut Mem, index: u32) {
    let e = entity(mem, index);
    let sector = mem.u16(e + 0x78) as u32;
    if sector == NONE {
        return;
    }
    let head = mem.u32(W_SECTOR_HEADS) + sector * 2;
    let mut cur = mem.u16(head) as u32;
    if cur == index {
        mem.set_u16(head, mem.u16(e + 2));
        return;
    }
    loop {
        let next = mem.u16(entity(mem, cur) + 2) as u32;
        if next == NONE {
            return;
        }
        if next == index {
            let link = mem.u16(e + 2);
            mem.set_u16(entity(mem, cur) + 2, link);
            return;
        }
        cur = next;
    }
}

/// `FUN_08137578`: push entity `index` onto its sector's entity list.
pub fn link_entity(mem: &mut Mem, index: u32) {
    let e = entity(mem, index);
    let sector = mem.u16(e + 0x78) as u32;
    if sector != NONE {
        let head = mem.u32(W_SECTOR_HEADS) + sector * 2;
        mem.set_u16(e + 2, mem.u16(head));
        mem.set_u16(head, index as u16);
    }
}

/// The IWRAM division at 0x03000220 (`nfsgba_fixed::iwram_divmod`): returns `a / b` and stores the remainder
/// at `rem`, as the game does.
pub fn iwram_divmod(mem: &mut Mem, a: i32, b: i32, rem: u32) -> i32 {
    let (q, r) = nfsgba_fixed::iwram_divmod(a, b);
    mem.set_i32(rem, r);
    q
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iwram_division_keeps_its_quirks() {
        let mut m = Mem::new(Vec::new(), vec![0; 0x4_0000], vec![0; 0x8000]);
        let rem = 0x0300_6480;
        assert_eq!((iwram_divmod(&mut m, 100, 7, rem), m.i32(rem)), (14, 2));
        // The remainder uses the unsigned quotient: -100 - 14 * 7.
        assert_eq!((iwram_divmod(&mut m, -100, 7, rem), m.i32(rem)), (-14, -198));
        assert_eq!(iwram_divmod(&mut m, 0, 0, rem), 1);
        // A negative divisor is replaced by -|a|, which as unsigned exceeds every dividend: 100 / -7 comes out as 0.
        assert_eq!((iwram_divmod(&mut m, 100, -7, rem), m.i32(rem)), (0, 100));
    }
}
