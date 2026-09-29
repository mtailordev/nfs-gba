//! The world struct, the globals the car step uses, sector lookup, floor height and the control bindings.

use crate::Result;
use crate::math::{mul64, shr64};
use crate::mem::Mem;

/// The world struct (IWRAM). Fields used here:
/// `+0x0C` sector entity-list heads (u16 per sector), `+0x10` walls (0x44 bytes), `+0x14` sectors (0x30 bytes),
/// `+0x18` dynamic wall states (0x20 bytes, walls whose `+0x2A` is not 0xFFFF), `+0x1C` sloped floor planes
/// (0x14 bytes), `+0x3C` entity array (0xA4 bytes), `+0x40` route segments (8 bytes), `+0x44` route
/// waypoints (0x18 bytes), `+0xC0/+0xC4/+0xC8` query point and `+0xEA` query sector for the sector search.
pub const WORLD: u32 = 0x0300_00C0;
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

/// `FUN_030047a8` (IWRAM, ARM): whether (`x`, `z`) is inside the convex wall loop; returns `id` or 0xFFFF.
pub(crate) fn inside(mem: &Mem, x: i32, z: i32, walls: u32, count: u32, id: u32) -> u32 {
    let mut prev = walls + (count.wrapping_sub(1)) * 0x44;
    let mut cur = walls;
    for _ in 0..count {
        let (px, pz) = (mem.i32(prev), mem.i32(prev + 4));
        let (cx, cz) = (mem.i32(cur), mem.i32(cur + 4));
        if (cz.wrapping_sub(pz).wrapping_mul(x.wrapping_sub(px)))
            .wrapping_sub(z.wrapping_sub(pz).wrapping_mul(cx.wrapping_sub(px)))
            < 0
        {
            return NONE;
        }
        prev = cur;
        cur += 0x44;
    }
    id
}

/// `FUN_03000800` (IWRAM, ARM; called through `FUN_0815e674`): the sector containing the query point (world
/// `+0xC0/+0xC8`): the query sector (`+0xEA`) or one through its open portals, 0xFFFF if neither.
pub fn find_sector_near_query(mem: &Mem) -> u32 {
    let (x, z) = (mem.i32(W_QUERY), mem.i32(W_QUERY + 8));
    let start = mem.u16(W_QUERY_SECTOR) as u32;
    let s = sector_addr(mem, start);
    let count = mem.u16(s + 2) as u32;
    let first = wall_addr(mem, mem.u16(s) as u32);
    if inside(mem, x, z, first, count, start) != NONE {
        return start;
    }
    let mut w = first;
    for _ in 0..count {
        let link = mem.u16(w + 0x32) as u32;
        if wall_flags(mem, w) & 0x1000 == 0 && link != NONE {
            let t = sector_addr(mem, link);
            let found = inside(
                mem,
                x,
                z,
                wall_addr(mem, mem.u16(t) as u32),
                mem.u16(t + 2) as u32,
                link,
            );
            if found != NONE {
                let f = sector_addr(mem, found);
                if mem.i16(f + 8) != 0 || mem.u16(f + 0x20) as u32 == NONE {
                    return found;
                }
                return mem.u16(f + 0x20) as u32;
            }
        }
        w += 0x44;
    }
    NONE
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

/// `FUN_0814dbbc`: the query point's sector two portals away from the query sector: behind each open portal
/// (link `+0x32`, flag 0x1000 clear), the sectors behind that sector's open portals, tested in wall order with
/// `point_in_sector`. A found sector without a floor gives its floor sector (`+0x20`). 0xFFFF if none.
pub fn find_sector_far(mem: &Mem) -> u32 {
    let (x, z) = (mem.i32(W_QUERY), mem.i32(W_QUERY + 8));
    let open = |w: u32| {
        let link = mem.u16(w + 0x32) as u32;
        (link != NONE && wall_flags(mem, w) & 0x1000 == 0).then_some(link)
    };
    let s = sector_addr(mem, mem.u16(W_QUERY_SECTOR) as u32);
    let mut w = wall_addr(mem, mem.u16(s) as u32);
    for _ in 0..mem.u16(s + 2) {
        if let Some(near) = open(w) {
            let t = sector_addr(mem, near);
            let mut v = wall_addr(mem, mem.u16(t) as u32);
            for _ in 0..mem.u16(t + 2) {
                if let Some(far) = open(v) {
                    let u = sector_addr(mem, far);
                    let found = inside(mem, x, z, wall_addr(mem, mem.u16(u) as u32), mem.u16(u + 2) as u32, far);
                    if found != NONE {
                        let f = sector_addr(mem, found);
                        let floor = mem.u16(f + 0x20) as u32;
                        return if mem.i16(f + 8) != 0 || floor == NONE {
                            found
                        } else {
                            floor
                        };
                    }
                }
                v += 0x44;
            }
        }
        w += 0x44;
    }
    NONE
}

/// `FUN_0814f4a8`: a sector's record, or the record of the sector it defers its floor to (`+0x20`) when it has no
/// floor of its own (`+0x08` zero, flag 0x20 of `+0x12` clear).
pub fn floor_sector(mem: &Mem, sector: u32) -> u32 {
    let s = sector_addr(mem, sector);
    let link = mem.u16(s + 0x20) as u32;
    if mem.i16(s + 8) == 0 && mem.u8(s + 0x12) & 0x20 == 0 && link != NONE {
        sector_addr(mem, link)
    } else {
        s
    }
}

/// `FUN_0814ca84`: floor height (8.8, `-y` up) of `sector` at (`x`, `z`) city units: the first wall's height
/// (`+0x38`) plus the sector's (`+0x26`), then the floor plane (sector `+0x14/+0x16/+0x18`, or a sloped plane
/// from world `+0x1C`) through the first wall's corner.
pub fn floor_height(mem: &Mem, sector: u32, x: i32, z: i32) -> i32 {
    let s = floor_sector(mem, sector);
    let w = wall_addr(mem, mem.u16(s) as u32);
    let mut h = mem.i16(w + 0x38) as i32 + mem.i16(s + 0x26) as i32;
    let plane = mem.u16(s + 0xA) as u32;
    let (a, b, c) = if plane == NONE {
        (mem.i16(s + 0x14), mem.i16(s + 0x16), mem.i16(s + 0x18))
    } else {
        let p = mem.u32(W_PLANES) + plane * 0x14;
        let state = mem.u16(w + 0x2A) as u32;
        if state != NONE {
            h += mem.i16(mem.u32(W_WALL_STATES) + state * 0x20 + 6) as i32;
        }
        h += mem.i16(p + 6) as i32;
        (mem.i16(p + 0xC), mem.i16(p + 0xE), mem.i16(p + 0x10))
    };
    let b = if b == 0 { 1 } else { b as i32 };
    let inv = if b < 0 {
        -crate::math::recip_entry(mem, -b)
    } else {
        crate::math::recip_entry(mem, b)
    };
    let d = (a as i32)
        .wrapping_mul(x.wrapping_sub(mem.i32(w)))
        .wrapping_add((c as i32).wrapping_mul(z.wrapping_sub(mem.i32(w + 4))));
    (h << 8).wrapping_add(shr64(mul64(d, inv.wrapping_neg()), 16))
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
