//! Car paint: how a race fills palette slots 160..=255 (`FUN_0813b6d0`), shades the glass (`shade_car_paint`,
//! `FUN_081387e4`) and moves the player's atlas into those slots (`FUN_08163e5c`). Raw BGR555, exact.
//! Details and verification: `docs/formats/car-paint.md`.

use crate::{i16_at, tint_palette, u16_at};

/// The second palette block (level record `+0x04`; the code uses the literal `0x0836C75C`).
pub const PAINT_BLOCK: usize = 0x36_C75C;
/// 16-colour paint ramps, 0x20 bytes each, indexed by paint number (`PAINT_BLOCK + 0x200`).
pub const PAINT_RAMPS: usize = PAINT_BLOCK + 0x200;
/// 8-colour rows for slots 240 and 248 (`PAINT_BLOCK + 0x600`).
pub const EXTRA_ROWS: usize = PAINT_BLOCK + 0x600;
/// Bytes at `0x7EEA24`: the ramp of a special car (id 15 and up) and of a paint code of 20 and up.
const SPECIAL_RAMPS: usize = 0x7E_EA24;
const CAR_TABLE: usize = 0x7F_0BD8;
pub use nfsgba_fixed::sin_q14;

/// `FUN_0813b6d0`: copies the car ramps into the base palette (the game writes both base buffers,
/// `*0x030055F0` and `*0x0300577C`, and flags them dirty). `cars` and `paints` are the per-racer arrays at
/// `0x0300611C` and `0x03005FEC` (index 0 is the player). `record` is a 0x11-byte car record: in a race (`race`)
/// the player's car's (`*0x0300539C + cars[0] * 0x11`), in the garage the edited one at `0x03005700`.
///
/// Slots: 160..=175 ramp `paints[1]` (or a special ramp when `cars[1] >= 15`), 176..=191 ramp `paints[2]`,
/// 208..=223 the player's ramp, 240..=247 extra row `record[4] - 1` (only when `record[4] > 0`), 248..=255 extra
/// row `cars[1] - 15`. 192..=207 keep the city palette (trim); `shade_car_paint` later overwrites 192 and 208.
pub fn load_car_palettes(rom: &[u8], base: &mut [u16], cars: [i8; 4], paints: [i8; 4], record: &[u8], race: bool) {
    let mut copy = |slot: usize, at: isize, n: usize| {
        for (i, c) in base[slot..slot + n].iter_mut().enumerate() {
            *c = u16_at(rom, at as usize + 2 * i);
        }
    };
    let ramp = |paint: isize| PAINT_RAMPS as isize + 0x20 * paint;
    let special = |i: isize| rom[(SPECIAL_RAMPS as isize + i) as usize] as isize;
    let (car0, car1) = (cars[0] as isize, cars[1] as isize);
    let first = if car1 > 14 {
        special(car1 - 15)
    } else {
        paints[1] as isize
    };
    copy(160, ramp(first), 16);
    copy(176, ramp(paints[2] as isize), 16);
    let player = if race && record[6] > 0x13 {
        ramp(special(car0))
    } else {
        // Car table +0x0E picks a 256-colour bank of the block (1 for every car: the ramps above).
        let bank = i16_at(rom, (CAR_TABLE as isize + 0x58 * car0 + 0x0E) as usize) as isize;
        PAINT_BLOCK as isize + 2 * (0x100 * bank + 0x10 * paints[0] as isize)
    };
    copy(208, player, 16);
    if record[4] > 0 {
        copy(240, EXTRA_ROWS as isize + 0x10 * (record[4] as isize - 1), 8);
    }
    // Read even when the opponent is an ordinary car (a row below EXTRA_ROWS), as the game does.
    copy(248, EXTRA_ROWS as isize + 0x10 * (car1 - 15), 8);
}

/// `shade_car_paint` (`FUN_081387e4`): the glass colours for slots 192 and 208. `glass` is the car record's `+5`
/// (colour 12 of ramp `glass`); `angle` is the heading (entity `+0x2C >> 8`, 0x4000 per turn) or, in the garage,
/// the turntable angle. Slot 192 uses `angle`, slot 208 `angle + 0x1000`: each channel (×8) times
/// `clamp(|sin >> 9|, 16, 28)`, `>> 8`.
///
/// The game writes both values into both base buffers and, unless a palette fade runs (`*0x03005630 != 0`),
/// straight into palette RAM; it does nothing once `*0x03005780` is set (race over).
pub fn glass_shades(rom: &[u8], glass: u8, angle: i32) -> [u16; 2] {
    let c = u16_at(rom, PAINT_BLOCK + 0x218 + 0x20 * glass as usize) as i32;
    let (r, g, b) = ((c & 0x1F) << 3, (c & 0x3E0) >> 2, (c & 0x7C00) >> 7);
    [angle, angle.wrapping_add(0x1000)].map(|a| {
        let s = (sin_q14(rom, a) >> 9).abs().clamp(16, 28);
        (((s * g) >> 8) << 5 | (s * r) >> 8 | ((s * b) >> 8) << 10) as u16
    })
}

/// Palette RAM during a race: `base` tinted by the light (`tint_palette`), except the glass slots 192 and 208,
/// which hold `base`'s raw shades (`shade_car_paint` runs after the previous frame's tint).
///
/// NOT 1:1 (R17): the tint (end of a game frame) also writes 192 and 208, and the next frame's shade replaces them 1–7
/// scanlines later (measured in mGBA); those few scanlines of tinted glass per game frame are not reproduced.
pub fn race_palette(base: &[u16], light: [i32; 3]) -> Vec<u16> {
    let mut ram = tint_palette(base, light);
    for i in [192, 208] {
        ram[i] = base[i];
    }
    ram
}

/// `FUN_08163e5c`: moves atlas pixels into palette slots, 0..=15 to `+ body`, 16..=31 to `+ trim - 16` (8-bit
/// wrap), anything else unchanged. `FUN_0813b828` unpacks the player's atlas (entity `+0x84`) with
/// `body = 0xD0 + 0x10 * (player % 3)` and `trim = 0xC0`: for player 0, pixel `i` becomes `192 + (i ^ 16)`.
pub fn remap_atlas(pixels: &mut [u8], body: u8, trim: u8) {
    for p in pixels {
        if *p < 0x10 {
            *p = p.wrapping_add(body);
        } else if *p < 0x20 {
            *p = p.wrapping_add(trim.wrapping_sub(0x10));
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{city, city_palette_raw, environments, sector_light};
    use nfsgba_testkit::rom;

    /// IWRAM, EWRAM and palette RAM of an mGBA dump (`<dir>/<name>.<domain>.bin`).
    pub(crate) struct Dump(Vec<u8>, Vec<u8>, pub(crate) Vec<u16>);

    impl Dump {
        pub(crate) fn load(dir: &str, name: &str) -> Option<Dump> {
            let read = |domain: &str| nfsgba_testkit::read(&format!("{dir}/{name}.{domain}.bin"));
            let pal = read("palette")?
                .chunks(2)
                .take(256)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            Some(Dump(read("iwram")?, read("wram")?, pal))
        }
        pub(crate) fn at(&self, a: u32) -> &[u8] {
            match a {
                0x0300_0000.. => &self.0[(a - 0x0300_0000) as usize..],
                _ => &self.1[(a - 0x0200_0000) as usize..],
            }
        }
        pub(crate) fn word(&self, a: u32) -> u32 {
            u32::from_le_bytes(self.at(a)[..4].try_into().unwrap())
        }
        pub(crate) fn bytes4(&self, a: u32) -> [i8; 4] {
            <[u8; 4]>::try_from(&self.at(a)[..4]).unwrap().map(|b| b as i8)
        }
        /// Base palette buffer `*0x030055F0`.
        fn base(&self) -> Vec<u16> {
            self.at(self.word(0x0300_55F0))
                .chunks(2)
                .take(256)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect()
        }
        /// (glass index of entity 0's record, heading of the shaded entity `*0x030057F8`).
        fn glass_and_heading(&self) -> (u8, i32) {
            let entities = self.word(0x0300_00C0 + 0x3C);
            let record = self.word(0x0300_539C) + 0x11 * self.at(entities + 0x89)[0] as u32;
            let shaded = entities + 0xA4 * self.word(0x0300_57F8);
            (self.at(record + 5)[0], self.word(shaded + 0x2C) as i32 >> 8)
        }
    }

    /// Rebuilds the reference race's car slots from the ROM and the race state, then its palette RAM.
    #[test]
    fn car_slots_reproduce_the_race_palette() {
        let Some(rom) = rom() else { return };
        let Some(d) = Dump::load("mgba", "race") else { return };
        let (cars, paints) = (d.bytes4(0x0300_611C), d.bytes4(0x0300_5FEC));
        let record = d.at(d.word(0x0300_539C) + 0x11 * cars[0] as u32)[..0x11].to_vec();
        let mut base = city_palette_raw(&rom, environments(&rom)[11].palette);
        load_car_palettes(&rom, &mut base, cars, paints, &record, true);
        let (glass, heading) = d.glass_and_heading();
        [base[192], base[208]] = glass_shades(&rom, glass, heading);
        let want = d.base();
        for (i, (got, want)) in base.iter().zip(&want).enumerate().skip(160) {
            assert_eq!(got, want, "base palette entry {i}");
        }
        let entities = d.word(0x0300_00C0 + 0x3C);
        let player = entities + 0xA4 * d.word(0x0300_0060);
        let (px, pz) = (d.word(player + 0x0C) as i32 >> 8, d.word(player + 0x14) as i32 >> 8);
        let sector = &city(&rom)[d.word(0x0300_5614) as usize];
        let light = sector_light(&rom, sector, px, pz).expect("two walls straddle the player");
        let ram = race_palette(&base, light);
        for (i, (got, want)) in ram.iter().zip(&d.2).enumerate().skip(160) {
            assert_eq!(got, want, "palette RAM entry {i}");
        }
    }

    /// Glass shades at other headings (car-paint session: driving left). The shade uses the heading from
    /// before the game frame's physics step, so a dump's base holds the shades of the previous game frame's
    /// heading (game frames are 4 video frames here).
    #[test]
    fn glass_shades_follow_the_heading() {
        let Some(rom) = rom() else { return };
        for (before, after) in [("d0", "d3"), ("d3", "d7"), ("d15", "d19")] {
            let (Some(a), Some(b)) = (Dump::load("car-paint", before), Dump::load("car-paint", after)) else {
                return;
            };
            let (glass, heading) = a.glass_and_heading();
            let base = b.base();
            assert_eq!(
                glass_shades(&rom, glass, heading),
                [base[192], base[208]],
                "{before} -> {after}"
            );
            assert_eq!(
                [b.2[192], b.2[208]],
                [base[192], base[208]],
                "{after}: palette RAM holds the raw shades"
            );
        }
    }

    #[test]
    fn sine_table_quadrants() {
        let Some(rom) = rom() else { return };
        assert_eq!(
            [0, 0x1000, 0x2000, 0x3000, 0x4000].map(|a| sin_q14(&rom, a)),
            [0, 0x4000, 0, -0x4000, 0]
        );
    }
}
