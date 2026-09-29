//! Car atlases: the player's texture as the race builds it (`unpack_player_atlas` `FUN_0813b828`, then the rim by
//! `unpack_decal` `FUN_0813bf58` / `draw_decal_on_atlas` `FUN_0813bd90`), and how the opponents' cars, paints and
//! textures are chosen (`pick_opponent_cars` `FUN_0813b634`, `setup_race_cars` `FUN_0813b9b8`). Exact.
//! Details and verification: `docs/formats/car-paint.md`.
//!
//! Material pixels come from `vehicle_textures`. The game decodes the compressed ones (flag `0x40`) with its own
//! LZ77 (ARM `0x08169208`, a 4 KiB ring prefilled with 0xFF), which equals `lz77` here: no vehicle stream refers back
//! before its start, and the only other format used is raw 8bpp.

use std::ops::RangeInclusive;

use crate::{Texture, i16_at, paint::remap_atlas, paint::sin_q14, u16_at, u32_at};

const CAR_TABLE: usize = 0x7F_0BD8;
/// i16 overlay material per `car·7 + record[1]`; −1 = none.
const OVERLAYS: usize = 0x7E_F5A0;
/// i16 (x, y) of the overlay in the atlas, 4 bytes per `car·7 + record[1]`.
const OVERLAY_AT: usize = 0x7E_F672;
/// Decal sets: three i16 vehicle materials per `record[4] − 1`; −1 = none.
const DECAL_SETS: usize = 0x7E_EBBC;
/// i16 (x, y) of decal-set material `m` for `car` at `DECAL_AT + 0x9C·car + 4·m` (materials 67.. start at `0x7EEC7C`).
const DECAL_AT: usize = 0x7E_EB70;
/// Rims: 0x10 bytes per `car·15 + record[2]`: `x0, y0, material, _, x1, y1, material, _` as i16.
const RIMS: usize = 0x7E_F816;
/// Per car id, 0xC bytes: u16 opponent material, u16 model index (entity `+0x36`), u16 car id (entity `+0x89`), …
const OPPONENT_LOOKS: usize = 0x7E_EA44;
/// i32 per wingman (1..=12): opponent 1's paint.
const WINGMAN_PAINTS: usize = 0x7F_4344;
const RAND_TABLE: usize = 0x7C_03F0;

/// `FUN_0815f988`: cosine, 0x4000 per turn.
fn cos_q14(rom: &[u8], angle: i32) -> i32 {
    sin_q14(rom, angle.wrapping_add(0x1000))
}

/// `blit_material_keyed` (`FUN_08163870`) and, with `only`, `FUN_08163bfc`: copies texture `t` into the 256-wide
/// atlas at (x, y) without clipping, skipping texel 0 and adding `add`; `only` writes over atlas pixels in that range
/// alone.
fn blit(atlas: &mut [u8], (x, y): (i32, i32), t: &Texture, add: u8, only: Option<RangeInclusive<u8>>) {
    for (i, &p) in t.pixels.iter().enumerate() {
        let d = ((y + (i / t.width) as i32) * 0x100 + x + (i % t.width) as i32) as usize;
        if p != 0 && only.as_ref().is_none_or(|r| r.contains(&atlas[d])) {
            atlas[d] = p.wrapping_add(add);
        }
    }
}

fn xy(rom: &[u8], at: usize) -> (i32, i32) {
    (i16_at(rom, at) as i32, i16_at(rom, at + 2) as i32)
}

/// `unpack_player_atlas` (`FUN_0813b828`) for `racer` (0: the player) driving `car` (`cars[racer]`) with its car
/// record (0x11 bytes): the material `car table +0x0C + record[3]` (the game stores it in the player's entity
/// `+0x48`), remapped into the car slots with body `0xD0 + 0x10·(racer % 3)` and trim `0xC0`, then the overlay
/// (`record[1]`, `+0xD0`) and the decal set (`record[4]`, `+0xF0`, only over body pixels `0xD0..=0xDF`). The game
/// does all this only while `*0x03005624 == 0`. Returns (material, atlas); the rim is drawn on it next (`draw_rim`).
pub fn player_atlas(rom: &[u8], textures: &[Texture], car: usize, racer: i32, record: &[u8]) -> (usize, Vec<u8>) {
    let material = (i16_at(rom, CAR_TABLE + 0x58 * car + 0x0C) as u16).wrapping_add(record[3] as u16) as usize;
    let mut atlas = textures[material].pixels.clone();
    remap_atlas(&mut atlas, (0xD0 + 0x10 * (racer % 3)) as u8, 0xC0);
    let part = 7 * car + record[1] as usize;
    let overlay = i16_at(rom, OVERLAYS + 2 * part);
    if overlay >= 0 {
        let at = xy(rom, OVERLAY_AT + 4 * part);
        blit(&mut atlas, at, &textures[overlay as usize], 0xD0, None);
    }
    if record[4] > 0 {
        for j in 0..3 {
            let m = i16_at(rom, DECAL_SETS + 6 * (record[4] as usize - 1) + 2 * j);
            if m >= 0 {
                let at = xy(rom, DECAL_AT + 0x9C * car + 4 * m as usize);
                blit(&mut atlas, at, &textures[m as usize], 0xF0, Some(0xD0..=0xDF));
            }
        }
    }
    (material, atlas)
}

/// A rim (`0x7EF816`): one material drawn into the atlas at two places, the car's two visible wheels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rim {
    pub material: usize,
    pub width: usize,
    pub height: usize,
    pub first: (i32, i32),
    pub second: (i32, i32),
}

/// The rim for `car` and its record (`record[2]`); `None` when the entry is empty (x0 = −1). The entry's second
/// material is not read.
pub fn rim(rom: &[u8], textures: &[Texture], car: usize, record: &[u8]) -> Option<Rim> {
    let at = RIMS + 0x10 * (15 * car + record[2] as usize);
    let material = i16_at(rom, at + 4) as usize;
    (i16_at(rom, at) != -1).then(|| Rim {
        material,
        width: textures[material].width,
        height: textures[material].height,
        first: xy(rom, at),
        second: xy(rom, at + 8),
    })
}

/// `unpack_decal` (`FUN_0813bf58`): the rim's pixels as the game keeps them (`0x03006094[entity]`): texel 0x10
/// becomes 0 (transparent), every other texel + 0xB0 (`remap_decal_pixels`).
pub fn rim_pixels(textures: &[Texture], rim: &Rim) -> Vec<u8> {
    let pixels = &textures[rim.material].pixels;
    pixels
        .iter()
        .map(|&p| if p == 0x10 { 0 } else { p.wrapping_add(0xB0) })
        .collect()
}

/// `draw_decal_on_atlas` (`FUN_0813bd90`, which runs ARM `0x08169988` = IWRAM `0x03004A74`): draws the rim rotated by
/// `angle` (0x4000 per turn: `*(entity[+0x8C] + 0x90) >> 8`, or 0 when the pointer is null) into the box inset by
/// 1/8 of its size at `rim.first`, and writes every non-zero texel again at the same offset from `rim.second`.
/// `unpack_decal` draws it once at race start (angle 0); races redraw it every game frame the side of the car is
/// seen (`FUN_0814a2..`, test `FUN_0814f9d0`), without clearing, so texel-0 pixels keep earlier frames' colours.
///
/// The game samples 16.16 coordinates around the rim's centre with no bounds check. At some angles it reads up to
/// 63 bytes before and 62 after the rim buffer, which are heap memory. So `memory` is the RAM around the buffer,
/// with the rim's pixels (`rim_pixels`) starting at `memory[at]`. See car-paint.md for what lies there; a window
/// too small panics.
pub fn draw_rim(rom: &[u8], atlas: &mut [u8], rim: &Rim, memory: &[u8], at: usize, angle: i32) {
    let (w, h) = (rim.width as i32, rim.height as i32);
    let (iw, ih) = (w >> 3, h >> 3);
    let (x0, y0) = (rim.first.0 + iw, rim.first.1 + ih);
    let (x1, y1) = (rim.first.0 + w - iw, rim.first.1 + h - ih);
    if x0 == x1 || y0 == y1 {
        return;
    }
    let (c, s) = (cos_q14(rom, angle), sin_q14(rom, angle));
    let (du_dx, dv_dx, du_dy, dv_dy) = (c << 2, s << 2, (-s) << 2, c << 2);
    let (dx, dy) = ((iw - (w - iw)) * 2, (ih - (h - ih)) * 2);
    let (bc, bs) = (cos_q14(rom, -angle), sin_q14(rom, -angle));
    let (mut u, mut v) = (bc * dx + bs * dy, -bs * dx + bc * dy);
    let second = (rim.second.1 - rim.first.1) * 0x100 + rim.second.0 - rim.first.0;
    let centre = at as i32 + (h >> 1) * w + (w >> 1);
    for row in y0..y1 {
        let (mut cu, mut cv) = (u, v);
        for x in x0..x1 {
            let p = memory[(centre + w * (cv >> 16) + (cu >> 16)) as usize];
            if p != 0 {
                let d = row * 0x100 + x;
                atlas[(d + second) as usize] = p;
                atlas[d as usize] = p;
            }
            cu += du_dx;
            cv += dv_dx;
        }
        u += du_dy;
        v += dv_dy;
    }
}

/// `rand_table` (`FUN_0815fcfc`): `index = (index + 1) & 0xFF`, then `u16 0x7C03F0[index]`. The index lives at
/// `0x030064C8`; `setup_race_cars` reseeds it with `*0x03000044 & 0xFF` (`FUN_0815fd1c`).
pub fn rand_table(rom: &[u8], index: &mut u32) -> u16 {
    *index = (*index + 1) & 0xFF;
    u16_at(rom, RAND_TABLE + 2 * *index as usize)
}

/// `pick_opponent_cars` (`FUN_0813b634`) fills racers 1..=3 of `cars` (`0x0300611C`) and `paints` (`0x03005FEC`):
/// `k = rand % 5`, then for each of the `opponents` (`0x03005784`) racer `i + 1` drives car `3k + i` in paint
/// `rand % 15`; the other slots get 0. Opponent 1's paint is then replaced by `i32 0x7F4344[wingman − 1]`
/// (wingman `0x03006104`), and with no wingman (0) the game reads the word before the table, `0x7F4340` (= 11).
pub fn pick_opponent_cars(
    rom: &[u8],
    rand: &mut u32,
    opponents: u32,
    wingman: u32,
    cars: &mut [i8; 4],
    paints: &mut [i8; 4],
) {
    let k = rand_table(rom, rand) as u32 % 5;
    for i in 0..3u32 {
        let r = i as usize + 1;
        if i < opponents {
            cars[r] = (3 * k + i) as i8;
            paints[r] = (rand_table(rom, rand) as u32 % 15) as i8;
            if i == 0 {
                let at = WINGMAN_PAINTS as isize + 4 * (wingman as i32 - 1) as isize;
                paints[1] = u32_at(rom, at as usize) as i8; // strb: the word's low byte
            }
        } else {
            (cars[r], paints[r]) = (0, 0);
        }
    }
}

/// How `setup_race_cars` (`FUN_0813b9b8`) dresses a racer from `0x7EEA44`: entity `+0x48` material (for opponents a
/// raw 128×100 atlas already in final palette slots), `+0x36` model index and `+0x89` car id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Look {
    pub material: u16,
    pub model: u16,
    pub car: u8,
}

/// The look of racer `i`, an opponent (or any racer while `flag_5624`, `*0x03005624 != 0`, when nobody gets an
/// unpacked atlas). Material and model come from `cars[i]`'s entry. The car id comes from the player's entry,
/// `cars[0]`, or entry 0 while `*0x030000A0 == 1` (`flag_a0`), and only with `flag_5624` from `cars[i]`'s own. So in
/// an ordinary race every opponent carries the player's car id. Car 14's entry names car 0's texture and model.
pub fn look(rom: &[u8], cars: [i8; 4], i: usize, flag_5624: bool, flag_a0: bool) -> Look {
    let entry = |car: i8| (OPPONENT_LOOKS as isize + 0xC * car as isize) as usize;
    let own = entry(cars[i]);
    let id = match (flag_5624, flag_a0) {
        (true, _) => own,
        (false, true) => entry(0),
        (false, false) => entry(cars[0]),
    };
    Look {
        material: u16_at(rom, own),
        model: u16_at(rom, own + 2),
        car: u16_at(rom, id + 4) as u8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_dir;
    use crate::paint::tests::{Dump, rom};
    use crate::vehicle_textures;
    use std::fs;

    /// Race starts: the reference race, a Quick Play race from `mainmenu.ss` (`start1`), and two with RAM-poked car
    /// records (`pokev1`: car 0 with overlay 3, rim 5, decal set 1; `pokev2`: car 7 with its default overlay, rim 3,
    /// decal set 2). Rand index at `pick_opponent_cars` from the trace logs (`None`: not traced).
    const STARTS: [(&str, &str, Option<u32>); 4] = [
        ("mgba", "race", None),
        ("car-atlas", "start1-d1", Some(0x5A)),
        ("car-atlas", "pokev1-d1", Some(0x59)),
        ("car-atlas", "pokev2-d1", Some(0x59)),
    ];

    fn entity(d: &Dump, i: u32) -> u32 {
        d.word(0x0300_00C0 + 0x3C) + 0xA4 * i
    }

    fn u16_ram(d: &Dump, a: u32) -> u16 {
        u16::from_le_bytes([d.at(a)[0], d.at(a)[1]])
    }

    /// Every pixel of the player's atlas at race start, rim included (drawn at angle 0 by `unpack_decal`).
    #[test]
    fn player_atlas_matches_every_race_start() {
        let Some(rom) = rom() else { return };
        let textures = vehicle_textures(&rom);
        for (dir, name, _) in STARTS {
            let Some(d) = Dump::load(dir, name) else {
                eprintln!("skipping {dir}/{name}: no dump");
                continue;
            };
            let player = entity(&d, d.word(0x0300_0060));
            let car = d.bytes4(0x0300_611C)[0] as usize;
            let record = &d.at(d.word(0x0300_539C) + 0x11 * car as u32)[..0x11];
            let (material, mut atlas) = player_atlas(&rom, &textures, car, 0, record);
            assert_eq!(material, u16_ram(&d, player + 0x48) as usize, "{name}: material");
            let rim = rim(&rom, &textures, car, record).expect("every car has rims");
            draw_rim(&rom, &mut atlas, &rim, &rim_pixels(&textures, &rim), 0, 0);
            let got = &d.at(d.word(player + 0x84))[..atlas.len()];
            let bad: Vec<_> = (0..atlas.len()).filter(|&i| got[i] != atlas[i]).collect();
            assert!(
                bad.is_empty(),
                "{name}: {} atlas pixels differ, first at {:?}",
                bad.len(),
                bad.first()
            );
        }
    }

    /// In-race rim redraws, traced in mGBA while driving (`drive.log`, `drive-rimNNN.bin`: the atlas and the RAM
    /// from 4096 bytes before the rim buffer at each `draw_decal_on_atlas` call). Redrawing on call k's atlas at call
    /// k's angle must give call k+1's atlas, including the texels read from outside the rim buffer.
    #[test]
    fn rim_redraws_match_the_drive() {
        let Some(rom) = rom() else { return };
        let dir = data_dir().join("work/e5298b24/car-atlas");
        let (Ok(log), Some(d)) = (
            fs::read_to_string(dir.join("drive.log")),
            Dump::load("car-atlas", "drive-end"),
        ) else {
            eprintln!("skipping: no car-atlas drive trace");
            return;
        };
        let textures = vehicle_textures(&rom);
        let car = d.bytes4(0x0300_611C)[0] as usize;
        let rim = rim(
            &rom,
            &textures,
            car,
            &d.at(d.word(0x0300_539C) + 0x11 * car as u32)[..0x11],
        )
        .unwrap();
        let angles: Vec<i32> = log
            .lines()
            .filter_map(|l| l.split_once("ang90=")?.1.get(..8))
            .map(|h| u32::from_str_radix(h, 16).unwrap() as i32 >> 8)
            .collect();
        let capture = |k: usize| fs::read(dir.join(format!("drive-rim{k:03}.bin"))).ok();
        let mut checked = 0;
        for (k, &angle) in angles.iter().enumerate().map(|(i, a)| (i + 1, a)) {
            let (Some(now), Some(next)) = (capture(k), capture(k + 1)) else {
                continue;
            };
            let (mut atlas, memory) = (now[..0xC800].to_vec(), &now[0xC800..]);
            draw_rim(&rom, &mut atlas, &rim, memory, 4096, angle);
            assert!(atlas == next[..0xC800], "redraw {k} at angle {angle}");
            checked += 1;
        }
        assert!(checked > 50, "only {checked} redraws checked");
    }

    /// Opponents: cars and paints from the traced rand index, and every opponent entity's material, model and car id.
    #[test]
    fn opponents_match_every_race_start() {
        let Some(rom) = rom() else { return };
        for (dir, name, rand) in STARTS {
            let Some(d) = Dump::load(dir, name) else { continue };
            let (cars, paints) = (d.bytes4(0x0300_611C), d.bytes4(0x0300_5FEC));
            if let Some(mut index) = rand {
                let (mut c, mut p) = ([cars[0], 0, 0, 0], [paints[0], 0, 0, 0]);
                pick_opponent_cars(
                    &rom,
                    &mut index,
                    d.word(0x0300_5784),
                    d.word(0x0300_6104),
                    &mut c,
                    &mut p,
                );
                assert_eq!((c, p), (cars, paints), "{name}: cars and paints");
                assert_eq!(index, rand.unwrap() + 4, "{name}: four rand calls");
            }
            let (flag_5624, flag_a0) = (d.word(0x0300_5624) != 0, d.word(0x0300_00A0) == 1);
            for i in 1..4 {
                let e = entity(&d, i as u32);
                let want = Look {
                    material: u16_ram(&d, e + 0x48),
                    model: u16_ram(&d, e + 0x36),
                    car: d.at(e + 0x89)[0],
                };
                assert_eq!(look(&rom, cars, i, flag_5624, flag_a0), want, "{name}: racer {i}");
            }
        }
    }

    /// With no wingman the game reads index −1, the word before the table.
    #[test]
    fn wingman_zero_reads_before_the_paint_table() {
        let Some(rom) = rom() else { return };
        let (mut cars, mut paints) = ([0; 4], [0; 4]);
        pick_opponent_cars(&rom, &mut 0, 3, 0, &mut cars, &mut paints);
        assert_eq!(paints[1], 11);
        pick_opponent_cars(&rom, &mut 0, 1, 5, &mut cars, &mut paints);
        assert_eq!((paints[1], cars[2], paints[3]), (4, 0, 0));
    }
}
