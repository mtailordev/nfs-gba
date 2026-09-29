//! The effect-sprite list (`FUN_08161f38`): 20-byte sprite objects written into the shadow OAM from entry `first`
//! downwards, through the game's OAM attribute setters. It runs on typed state: the pool ([`Sprite`]) and the
//! shadow OAM ([`Oam`]); `view::hud::draw_effect_sprites` loads and stores them.

use nfsgba_formats::ui::Oam;
use nfsgba_sim::state::Sprite;

/// `attr = attr & !mask | bits` on the halfword `j` of OAM entry `i`.
fn set(oam: &mut Oam, i: u32, j: usize, mask: u16, bits: u16) {
    let v = &mut oam[i as usize][j];
    *v = *v & !mask | bits & mask;
}

/// `FUN_08161f38(list)`: `sprites` are the pool's objects, `first` its first OAM entry.
pub fn draw_effect_sprites(rom: &[u8], sprites: &mut [Sprite], first: i16, tile_base: u16, oam: &mut Oam) {
    let mut i = first as i32 as u32;
    for p in sprites.iter_mut() {
        // (The game writes past the OAM for an entry above 0x7F; that never happens in a race.)
        if i < 0x80 {
            if p.used == 1 {
                // FUN_081611c4: x into attr1 bits 0..8, y into attr0's low byte.
                set(oam, i, 1, 0x1FF, p.x as u16);
                set(oam, i, 0, 0xFF, p.y as u16);
                // FUN_08161384: tile + the OBJ tile base into attr2 bits 0..9.
                set(oam, i, 2, 0x3FF, (p.frame as u16).wrapping_add(tile_base));
                // FUN_08160f10: shape (attr0 bits 14..15) and size (attr1 bits 14..15) from the size code.
                let (shape, size) = match p.size as u16 {
                    c @ 0..=3 => (0, c),
                    c @ 4..=7 => (1, c - 4),
                    c @ 8..=11 => (2, c - 8),
                    _ => (0, 0),
                };
                set(oam, i, 0, 0xC000, shape << 14);
                set(oam, i, 1, 0xC000, size << 14);
                // FUN_08161204: OBJ mode (attr0 bits 10..11): semi-transparent when flag bit 1.
                set(oam, i, 0, 0x0C00, ((p.kind as u16 >> 1) & 1) << 10);
                if p.kind & 4 == 0 {
                    let matrix = i & 0x1F;
                    set_affine_index(oam, i, matrix);
                    set_affine(
                        rom,
                        oam,
                        matrix,
                        p.scale_x as i32,
                        p.scale_y as i32,
                        p.angle as u16 as u32,
                    );
                    set(oam, i, 0, 0x0300, 3 << 8);
                } else {
                    set(oam, i, 0, 0x0300, 0);
                    set_affine_index(oam, i, 0);
                }
                // FUN_08161264 (256 colours, attr0 bit 13) and FUN_081613f4 (palette bank, attr2 bits 12..15).
                if p.palette == 0xFF {
                    set(oam, i, 0, 0x2000, 0x2000);
                } else {
                    set(oam, i, 0, 0x2000, 0);
                    set(oam, i, 2, 0xF000, (p.palette as u16) << 12);
                }
            } else {
                // FUN_081610c8: y = 160, affine mode off.
                set(oam, i, 0, 0xFF, 0xA0);
                set(oam, i, 0, 0x0300, 0);
            }
        }
        if p.used == 1 && p.kind & 1 != 0 {
            p.used = 0;
        }
        i = i.wrapping_sub(1);
    }
}

/// `FUN_08160ebc`: affine matrix index, attr1 bits 9..13 (only indices below 0x20).
fn set_affine_index(oam: &mut Oam, i: u32, matrix: u32) {
    if matrix < 0x20 {
        let v = ((matrix & 7) << 1 | (matrix >> 3 & 1) << 4 | (matrix >> 4 & 1) << 5) as u16;
        set(oam, i, 1, 0x3E00, v << 8);
    }
}

/// `oam_set_affine` (`0x0816144c`): matrix `k` = scale × rotation, in the fourth halfword of entries 4k..4k+3.
fn set_affine(rom: &[u8], oam: &mut Oam, k: u32, sx: i32, sy: i32, angle: u32) {
    let (c, s) = (
        nfsgba_fixed::cos_q14(rom, angle as i32),
        nfsgba_fixed::sin_q14(rom, angle as i32),
    );
    let at = 4 * k as usize;
    oam[at][3] = (sx.wrapping_mul(c) >> 14) as i16 as u16;
    oam[at + 1][3] = (sx.wrapping_mul(s) >> 14) as i16 as u16;
    oam[at + 2][3] = (sy.wrapping_mul(-s) >> 14) as i16 as u16;
    oam[at + 3][3] = (sy.wrapping_mul(c) >> 14) as i16 as u16;
}
