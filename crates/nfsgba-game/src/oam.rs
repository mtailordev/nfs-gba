//! The effect-sprite list (`FUN_08161f38`, list header at `0x03000058`): 20-byte sprite objects written into the
//! shadow OAM (`0x030064F0`) from entry `start` downwards, through the game's OAM attribute setters.

use nfsgba_sim::Mem;

pub const SHADOW_OAM: u32 = 0x0300_64F0;
const TILE_BASE: u32 = 0x0300_64E0;

fn entry(i: u32) -> u32 {
    SHADOW_OAM + 8 * i
}

fn set_byte(m: &mut Mem, at: u32, keep: u8, bits: u8) {
    let v = m.u8(at) & keep | bits;
    m.set_u8(at, v);
}

/// `FUN_08161f38(list)`: `list` = {u32 objects, i16 first OAM entry, i16 count}; object = {u16 x, u16 y,
/// u16 active, u16 ?, u16 tile, i16 scale x, i16 scale y, u16 angle, u16 size code, u8 flags, i8 palette}.
pub fn draw_effect_sprites(rom: &[u8], m: &mut Mem, list: u32) {
    let (mut p, mut i, mut n) = (m.u32(list), m.i16(list + 4) as i32 as u32, m.i16(list + 6) as i32);
    while n != 0 {
        let h = |m: &Mem, k: u32| m.u16(p + 2 * k);
        if h(m, 2) == 1 {
            let e = entry(i);
            if i < 0x80 {
                // FUN_081611c4: x into attr1 bits 0..8, y into attr0's low byte.
                let a1 = m.u16(e + 2) & 0xFE00 | h(m, 0) & 0x1FF;
                m.set_u16(e + 2, a1);
                m.set_u8(e, h(m, 1) as u8);
                // FUN_08161384: tile + the OBJ tile base into attr2 bits 0..9.
                let a2 = m.u16(e + 4) & 0xFC00 | h(m, 4).wrapping_add(m.u16(TILE_BASE)) & 0x3FF;
                m.set_u16(e + 4, a2);
                // FUN_08160f10: shape (attr0 bits 14..15) and size (attr1 bits 14..15) from the size code.
                let (shape, size) = match h(m, 8) {
                    c @ 0..=3 => (0, c),
                    c @ 4..=7 => (1, c - 4),
                    c @ 8..=11 => (2, c - 8),
                    _ => (0, 0),
                };
                set_byte(m, e + 1, 0x3F, (shape as u8) << 6);
                set_byte(m, e + 3, 0x3F, (size as u8) << 6);
            }
            let flags = m.u8(p + 0x12);
            // FUN_08161204: OBJ mode (attr0 bits 10..11): semi-transparent when flag bit 1.
            if i < 0x80 {
                set_byte(m, entry(i) + 1, 0xF3, ((flags >> 1) & 1) << 2);
            }
            if flags & 4 == 0 {
                let matrix = i & 0x1F;
                set_affine_index(m, i, matrix);
                set_affine(
                    rom,
                    m,
                    matrix,
                    h(m, 5) as i16 as i32,
                    h(m, 6) as i16 as i32,
                    h(m, 7) as u32,
                );
                set_affine_mode(m, i, 3);
            } else {
                set_affine_mode(m, i, 0);
                set_affine_index(m, i, 0);
            }
            let palette = m.u8(p + 0x13);
            if i < 0x80 {
                // FUN_08161264 (256 colours, attr0 bit 13) and FUN_081613f4 (palette bank, attr2 bits 12..15).
                if palette == 0xFF {
                    set_byte(m, entry(i) + 1, 0xDF, 1 << 5);
                } else {
                    set_byte(m, entry(i) + 1, 0xDF, 0);
                    set_byte(m, entry(i) + 5, 0x0F, palette << 4);
                }
            }
            if flags & 1 != 0 {
                m.set_u16(p + 4, 0);
            }
        } else {
            // FUN_081610c8: y = 160, affine mode off. It has no range check on the entry.
            m.set_u8(entry(i), 0xA0);
            set_affine_mode(m, i, 0);
        }
        p += 20;
        i = i.wrapping_sub(1);
        n -= 1;
    }
}

/// `FUN_08161000`: affine mode, attr0 bits 8..9.
fn set_affine_mode(m: &mut Mem, i: u32, mode: u8) {
    if i < 0x80 {
        set_byte(m, entry(i) + 1, 0xFC, mode & 3);
    }
}

/// `FUN_08160ebc`: affine matrix index, attr1 bits 9..13 (only indices below 0x20).
fn set_affine_index(m: &mut Mem, i: u32, matrix: u32) {
    if i < 0x80 && matrix < 0x20 {
        let v = ((matrix & 7) << 1 | (matrix >> 3 & 1) << 4 | (matrix >> 4 & 1) << 5) as u8;
        set_byte(m, entry(i) + 3, 0xC1, v);
    }
}

/// `oam_set_affine` (`0x0816144c`): matrix `k` = scale × rotation, in the fourth halfword of entries 4k..4k+3.
fn set_affine(rom: &[u8], m: &mut Mem, k: u32, sx: i32, sy: i32, angle: u32) {
    let (c, s) = (
        nfsgba_fixed::cos_q14(rom, angle as i32),
        nfsgba_fixed::sin_q14(rom, angle as i32),
    );
    if k < 0x20 {
        let at = SHADOW_OAM + 0x20 * k;
        m.set_i16(at + 6, (sx.wrapping_mul(c) >> 14) as i16);
        m.set_i16(at + 0xE, (sx.wrapping_mul(s) >> 14) as i16);
        m.set_i16(at + 0x16, (sy.wrapping_mul(-s) >> 14) as i16);
        m.set_i16(at + 0x1E, (sy.wrapping_mul(c) >> 14) as i16);
    }
}
