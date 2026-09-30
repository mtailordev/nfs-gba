//! The race start's intro and countdown on typed state: `race_start_from_table_b` (`0x0813af7c`), which
//! `race_frame_update` calls every frame, with `effect_sprite_alloc` (`0x08162048`), `obj_upload_tiles`
//! (`0x08161a98`) and the tile uploads `countdown_tiles_a..d` (`0x0813a054`, `a080`, `a0b0`, `a0e0`).

use nfsgba_sim::Unported;

use crate::Game;

/// The HUD materials the game reads at fixed ROM offsets (not chosen by the level): entries of the 0x24-byte table
/// at `0x36CF5C` (`TILES_A` is entry 0, read at `+0x2C..`, i.e. entry 1; `DIGITS` 23, `TILES_B` 11, `TILES_C` 26,
/// `TILES_D` 279, the traffic rear lights). Entry fields: `+8` texel offset, `+0xC` width, `+0xE` height, `+0x20` palette.
const DIGITS: usize = 0x36_D298;
const TILES_A: usize = 0x36_CF5C;
const TILES_B: usize = 0x36_D0E8;
const TILES_C: usize = 0x36_D304;
const TILES_D: usize = 0x36_F698;
/// The texel data every material's texel offset counts from.
const TEXELS: usize = 0x34_7B74;

impl Game {
    fn rom16(&self, at: usize) -> u32 {
        u16::from_le_bytes([self.rom[at], self.rom[at + 1]]) as u32
    }

    /// A material's texel offset (`+at`) as a ROM offset.
    fn texels(&self, at: usize) -> usize {
        u32::from_le_bytes(self.rom[at..at + 4].try_into().unwrap()) as usize + TEXELS
    }

    /// `obj_upload_tiles(tile, 0, src, n)`: `n` tiles of 32 bytes from ROM offset `src` to OBJ tile `tile` plus the
    /// tile base. The game does nothing unless DISPCNT's 1-D mapping is on.
    fn obj_upload_tiles(&mut self, tile: u32, src: usize, n: u32) -> nfsgba_sim::Result<()> {
        if self.dispcnt & 0x40 == 0 {
            return Ok(());
        }
        let at = 0x1_0000 + ((tile + self.world.hud.tile_base as u32) & 0xFFFF) as usize * 0x20;
        let len = 32 * n as usize;
        if at + len > self.vram.len() {
            return Err(Unported("an OBJ tile upload past VRAM"));
        }
        self.vram[at..at + len].copy_from_slice(&self.rom[src..src + len]);
        Ok(())
    }

    /// `race_start_from_table_b`: with the fade done, in the intro (phase 9) and the countdown (phase 1) the digit
    /// sprite and the countdown timer; at the count of 3 the race starts (phase 2); the frame after, the tiles the
    /// racing HUD needs.
    pub fn race_start_from_table_b(&mut self) -> nfsgba_sim::Result<()> {
        let (phase, fade) = (self.world.g.phase, self.world.g.fade);
        if (phase == 1 || phase == 9) && fade == 0 {
            let acc = self.world.hud.race_state_changed as i32;
            let palette = self.rom[DIGITS + 0x20];
            let w = &mut self.world;
            // effect_sprite_alloc(pool): the first free object.
            let Some(k) = w.pool.iter().position(|s| s.used == 0) else {
                return Err(Unported(
                    "no free effect sprite for the countdown (the game writes through null)",
                ));
            };
            let scale = ((acc >> 10) as i16 as i32 * 0x20 + 0x76) as i16;
            let s = &mut w.pool[k];
            s.used = 1;
            (
                s.kind, s.angle, s.frame, s.scale_x, s.scale_y, s.size, s.x, s.y, s.palette,
            ) = (3, 0, 0x1C0, scale, scale, 3, 0x38, 6, palette);
            let sum = acc.wrapping_add(nfsgba_sim::math::div(50_000, w.g.dt));
            w.hud.race_state_changed = sum as u32;
            if sum >> 10 > 0xF {
                w.hud.race_state_changed = 0;
                w.lp.start_state += 1;
                let n = w.lp.start_state;
                if n == 3 {
                    w.g.phase = 2;
                    w.pool[k].used = 0;
                } else {
                    let area = self.rom16(DIGITS + 0xE) * self.rom16(DIGITS + 0xC);
                    let src = self.texels(DIGITS + 8) + (n * area >> 1) as usize;
                    self.obj_upload_tiles(0x1C0, src, area >> 6)?;
                }
            }
        } else if self.world.lp.start_state == 3 {
            let n = self.rom16(TILES_A + 0x32) * self.rom16(TILES_A + 0x30) >> 6;
            self.obj_upload_tiles(0x1FC, self.texels(TILES_A + 0x2C), n)?;
            for (tile, m) in [(0x1E4, TILES_B), (0x1CC, TILES_C)] {
                let n = self.rom16(m + 0xC) * self.rom16(m + 0xE) * 6 >> 6;
                self.obj_upload_tiles(tile, self.texels(m + 8), n)?;
            }
            let n = self.rom16(TILES_D + 0xE) * self.rom16(TILES_D + 0xC) >> 6;
            self.obj_upload_tiles(0x1BC, self.texels(TILES_D + 8), n)?;
            // countdown_nop (0x0813a108) does nothing.
            self.world.lp.start_state += 1;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `obj_upload_tiles`' stop is unreachable (FIDELITY N1): with the race's tile base (0x200, set by `race_init`
    /// alone) every countdown upload of the ROM's HUD materials ends inside OBJ VRAM (0x10000..0x18000).
    #[test]
    fn countdown_uploads_fit_in_vram() {
        let Some(rom) = nfsgba_testkit::rom() else { return };
        let r16 = |a: usize| u16::from_le_bytes([rom[a], rom[a + 1]]) as u32;
        let uploads = [
            (0x1C0, r16(DIGITS + 0xE) * r16(DIGITS + 0xC) >> 6),
            (0x1FC, r16(TILES_A + 0x32) * r16(TILES_A + 0x30) >> 6),
            (0x1E4, r16(TILES_B + 0xC) * r16(TILES_B + 0xE) * 6 >> 6),
            (0x1CC, r16(TILES_C + 0xC) * r16(TILES_C + 0xE) * 6 >> 6),
            (0x1BC, r16(TILES_D + 0xE) * r16(TILES_D + 0xC) >> 6),
        ];
        for (tile, n) in uploads {
            let end = 0x1_0000 + (tile + 0x200) * 0x20 + 32 * n;
            assert!(end <= 0x1_8000, "tile {tile:#x}: {n} tiles end at {end:#x}");
        }
    }
}
