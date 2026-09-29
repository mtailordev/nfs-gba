//! The race sky, exact to `BN7E` (`docs/engine/sky.md`). Two layers, both driven by the camera:
//!
//! - **Gradient:** the backdrop (palette entry 0, shown behind framebuffer index 0). The VCount IRQ
//!   (`0x030001C0`) writes the next gradient colour to palette entry 0 every 2 scanlines, lines 0..=78.
//! - **Skyline:** a 240×64 panorama that `FUN_0813A318` copies into the top of the mode-4 framebuffer
//!   before the 3D world is drawn over it.

use crate::{LEVEL_TABLE, i16_at, ptr, u16_at, u32_at};

pub const SCREEN_W: usize = 240;
pub const SCREEN_H: usize = 160;

/// The camera state the sky reads (IWRAM addresses from the race).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SkyCamera {
    /// Camera yaw, `0x4000` per turn (`0x03000214`); only the low 14 bits count.
    pub yaw: i32,
    /// Screen shake x, y (`0x03005390`, `0x03005392`). Zeroed by the camera init `FUN_081378CC`; no other
    /// writer found, so it stays 0 in play.
    pub shake: [i16; 2],
    /// Horizon shift in screen rows (`0x030056B8`). In the bumper view it follows the car's pitch
    /// (`FUN_0814BC30`) and is clamped to ±32; the camera update `FUN_08137CB0` resets it to 0 in other views.
    pub horizon: i32,
    /// Camera view (`0x030055F8`): 0 bumper, 2 chase (race default); SELECT switches them (`FUN_0813867C`).
    pub view: u32,
}

/// An environment's sky, from its level descriptor (`0x7F2B08 + 0x68 * env`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkyDesc {
    /// ROM offset of the gradient material's texels (`+0x5E`): 64 BGR555 colours.
    pub gradient: usize,
    /// ROM offset of the skyline material's texels (`+0x60`), row-major 8bpp.
    pub skyline: usize,
    pub width: i32,
    pub height: i32,
    /// `+0x62`: skyline row offset (4 in every environment).
    pub skyline_y: i16,
    /// `+0x64`: gradient entry at the unshifted horizon, plus 4 (25 in every environment).
    pub gradient_top: i16,
}

pub fn sky_desc(rom: &[u8], env: usize) -> SkyDesc {
    let d = LEVEL_TABLE + 0x68 * env;
    let (materials, texels) = (ptr(rom, LEVEL_TABLE + 0x1C), ptr(rom, LEVEL_TABLE + 0x08));
    let material = |at: usize| materials + 0x24 * u16_at(rom, d + at) as usize;
    let (g, s) = (material(0x5E), material(0x60));
    SkyDesc {
        gradient: texels + u32_at(rom, g + 8) as usize,
        skyline: texels + u32_at(rom, s + 8) as usize,
        width: u16_at(rom, s + 0x0C) as i32,
        height: u16_at(rom, s + 0x0E) as i32,
        skyline_y: i16_at(rom, d + 0x62),
        gradient_top: i16_at(rom, d + 0x64),
    }
}

/// The 0x200-entry gradient buffer (EWRAM `0x0200120C`, pointer at `0x030053B8`) once the race fade-in is
/// done: `FUN_0812AE64` fades 0x200 entries from the gradient texels, so entries 64 and up are the ROM bytes
/// that follow the 64 colours. The camera can reach entry 76.
pub fn gradient_buffer(rom: &[u8], sky: &SkyDesc) -> Vec<u16> {
    (0..0x200).map(|i| u16_at(rom, sky.gradient + 2 * i)).collect()
}

/// First gradient entry of a frame, set in VBlank (`FUN_0812AC14`) from the camera at that moment.
pub fn gradient_start(sky: &SkyDesc, cam: &SkyCamera) -> usize {
    let k = sky.gradient_top as i32 - (cam.horizon >> 1) - ((cam.shake[1] as i32 >> 1) + 4);
    k.max(0) as usize
}

/// Gradient entry behind screen line `y`: the VCount IRQ steps it at lines 0, 2, …, 78 and stops at 80
/// (the reference emulator applies each write to its whole line).
pub fn backdrop_entry(start: usize, y: usize) -> usize {
    start + y.min(79) / 2
}

/// Skyline column at screen x = 0: the panorama repeats 4 times per turn.
pub fn skyline_column(yaw: i32, width: i32) -> i32 {
    (((yaw & 0x3FFF) * width * 4) >> 14) % width
}

/// Screen row of the skyline's first line (before clipping).
pub fn skyline_top(sky: &SkyDesc, cam: &SkyCamera) -> i32 {
    let top = cam.horizon + 0x50 - sky.height + sky.skyline_y as i32 + cam.shake[1] as i32;
    if cam.view == 0 { top + 4 } else { top }
}

/// `FUN_0813A318`: writes the skyline into `fb` (240 × 160, 8bpp), clipped to rows `clip_y` (`0x030053D4`,
/// `0x030053DC`: 0 and 159 in the race). The game skips it when world `+0xF6` is 0. Quirks kept:
/// - only `height - 1` rows are copied, each as 256 bytes read straight on from the start column, so a row
///   runs into the next texture row (no wrap-around) and the last one spills 16 bytes into the next screen row;
/// - rows above the skyline (and below it, up to row 63) are cleared to 0 by a fill that drops the last
///   `bytes % 32`, which keep what the page held before.
pub fn draw_skyline(rom: &[u8], sky: &SkyDesc, cam: &SkyCamera, clip_y: [i32; 2], fb: &mut [u8]) {
    const W: i32 = SCREEN_W as i32;
    let fill = |fb: &mut [u8], at: i32, bytes: i32| {
        let at = at as usize;
        fb[at..at + (bytes as usize & !31)].fill(0);
    };
    let (w, h) = (sky.width, sky.height);
    let [y0, y1] = clip_y;
    let mut top = skyline_top(sky, cam);
    if !(y0 - h < top && top < y1) {
        fill(fb, 0, W << 6);
        return;
    }
    let (mut src, mut rows, mut y) = (sky.skyline as i32, h - 1, top);
    if top < y0 {
        src += (y0 - top) * ((w >> 1) * 2);
        rows -= y0 - top;
        y = y0;
    } else if top > y1 - h {
        rows -= top - (y1 - h);
    }
    if rows > 0 {
        let column = skyline_column(cam.yaw, w) - cam.shake[0] as i32;
        let (mut s, mut d) = ((src + (column >> 1) * 2) as usize, (W * y) as usize);
        let copy = ((SCREEN_W >> 5) + 1) << 5;
        for _ in 0..rows {
            fb[d..d + copy].copy_from_slice(&rom[s..s + copy]);
            s += (w as usize >> 1) * 2;
            d += SCREEN_W;
        }
    }
    if top > y0 {
        fill(fb, W * y0, W * (top - y0));
    }
    top += h;
    if top <= 63 {
        fill(fb, W * (top - 1), W * (64 - top));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{canonical_rom, data_dir};
    use std::{collections::HashMap, fs, path::PathBuf};

    /// ROM plus the sky reference captures (`docs/engine/sky.md`, "Verification"); skipped when missing.
    fn setup() -> Option<(Vec<u8>, PathBuf)> {
        std::env::set_current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")).unwrap();
        let rom = canonical_rom()
            .map_err(|e| eprintln!("skipping: no ROM vault ({e})"))
            .ok()?;
        let dir = data_dir().join("work/e5298b24/sky");
        dir.is_dir().then_some((rom, dir)).or_else(|| {
            eprintln!("skipping: no sky captures");
            None
        })
    }

    fn fields(text: &str) -> HashMap<String, i64> {
        text.split_whitespace()
            .filter_map(|f| f.split_once('='))
            .map(|(k, v)| {
                let n = i64::from_str_radix(v, 16)
                    .ok()
                    .filter(|_| v.len() == 8 || k == "dispcnt");
                (k.to_owned(), n.unwrap_or_else(|| v.parse().unwrap()))
            })
            .collect()
    }

    fn env_of(desc: i64) -> usize {
        (desc as usize - 0x0800_0000 - LEVEL_TABLE) / 0x68
    }

    fn captures(dir: &PathBuf, ext: &str) -> Vec<(String, HashMap<String, i64>)> {
        let mut out: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| {
                let name = e.unwrap().file_name().into_string().unwrap();
                let stem = name.strip_suffix(ext)?.to_owned();
                Some((stem, fields(&fs::read_to_string(dir.join(&name)).unwrap())))
            })
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// The skyline rows 0..64 at the end of `FUN_0813A318`, captured by a breakpoint at several headings, both
    /// views and horizon shifts, must match byte for byte; bytes the fill leaves alone must be exactly the
    /// `% 32` remainder.
    #[test]
    fn skyline_blit_matches_the_reference_captures() {
        let Some((rom, dir)) = setup() else { return };
        let caps = captures(&dir, ".sky.txt");
        assert!(caps.len() >= 10, "{} captures", caps.len());
        for (name, f) in caps {
            let want = fs::read(dir.join(format!("{name}.sky.bin"))).unwrap();
            let sky = sky_desc(&rom, env_of(f["desc"]));
            let cam = SkyCamera {
                yaw: f["yaw"] as u32 as i32,
                shake: [f["shake_x"] as i16, f["shake_y"] as i16],
                horizon: f["horizon"] as i32,
                view: f["mode"] as u32,
            };
            let clip = [f["clip_y0"] as i32, f["clip_y1"] as i32];
            let mut fb = vec![0u8; SCREEN_W * SCREEN_H];
            for (d, s) in fb.iter_mut().zip(&want) {
                *d = !s;
            }
            draw_skyline(&rom, &sky, &cam, clip, &mut fb);
            let untouched = (0..want.len()).filter(|&i| fb[i] == !want[i]).count();
            assert!(
                (0..want.len()).all(|i| fb[i] == want[i] || fb[i] == !want[i]),
                "{name}: wrong bytes"
            );
            let top = skyline_top(&sky, &cam);
            let expect = if top > clip[0] {
                (240 * (top - clip[0]) % 32) as usize
            } else {
                0
            };
            assert_eq!(untouched, expect, "{name}: bytes left alone (top {top})");
        }
    }

    /// Every (start, horizon) pair the VBlank handler produced in both views.
    #[test]
    fn gradient_start_matches_the_vblank_handler() {
        let Some((rom, dir)) = setup() else { return };
        let Ok(log) = fs::read_to_string(dir.join("vblank.txt")) else {
            return;
        };
        let mut checked = 0;
        for f in log.lines().map(fields).filter(|f| f["desc"] != 0 && f["mode"] <= 2) {
            let cam = SkyCamera {
                horizon: f["horizon"] as i32,
                shake: [0, f["shake_y"] as i16],
                view: f["mode"] as u32,
                ..Default::default()
            };
            let sky = sky_desc(&rom, env_of(f["desc"]));
            assert_eq!(gradient_start(&sky, &cam) as i64, f["start"], "{f:?}");
            checked += 1;
        }
        assert!(checked >= 10, "{checked} samples");
    }

    /// The screen's backdrop pixels (framebuffer index 0, no sprite over them) show
    /// `gradient_buffer[backdrop_entry(start, y)]`, with the buffer equal to the ROM bytes. The game flips pages
    /// mid-frame, so each row is checked against whichever page it was scanned out from.
    #[test]
    fn backdrop_matches_the_reference_screens() {
        let Some((rom, dir)) = setup() else { return };
        let caps = captures(&dir, ".snap.txt");
        assert!(caps.len() >= 6, "{} snaps", caps.len());
        let rgb = |c: u16| [0, 5, 10].map(|s| ((c >> s & 31) << 3 | (c >> s & 31) >> 2) as u8);
        let (mut checked, mut past_64) = (0, 0);
        for (name, f) in caps {
            let bin = |ext: &str| fs::read(dir.join(format!("{name}.{ext}.bin"))).unwrap();
            let halves = |b: Vec<u8>| -> Vec<u16> { b.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect() };
            let sky = sky_desc(&rom, env_of(f["desc"]));
            let buffer = gradient_buffer(&rom, &sky);
            assert_eq!(halves(bin("gradbuf")), buffer, "{name}: gradient buffer");
            let start = ((f["grad_ptr"] - f["grad_base"]) / 2 - 40) as usize;
            let (pages, pal) = ([bin("fb"), bin("fb2")], halves(bin("pal")));
            let screen: Vec<[u8; 3]> = bin("screen").chunks(4).map(|p| [p[2], p[1], p[0]]).collect();
            let covered = sprite_mask(&bin("oam"));
            for y in 0..SCREEN_H {
                let entry = backdrop_entry(start, y);
                let back = rgb(buffer[entry]);
                let row = |fb: &Vec<u8>| {
                    (y * SCREEN_W..(y + 1) * SCREEN_W)
                        .filter(|&i| !covered[i])
                        .filter(|&i| screen[i] == if fb[i] == 0 { back } else { rgb(pal[fb[i] as usize]) })
                        .count()
                };
                let fb = &pages[(row(&pages[1]) > row(&pages[0])) as usize];
                for i in (y * SCREEN_W..(y + 1) * SCREEN_W).filter(|&i| fb[i] == 0 && !covered[i]) {
                    assert_eq!(screen[i], back, "{name}: line {y}, x {}", i % SCREEN_W);
                    checked += 1;
                    past_64 += (entry >= 64) as usize;
                }
            }
        }
        assert!(
            checked > 10_000 && past_64 > 0,
            "{checked} pixels, {past_64} beyond entry 63"
        );
    }

    /// Screen pixels inside any displayed OBJ's box (the HUD), from OAM.
    fn sprite_mask(oam: &[u8]) -> Vec<bool> {
        const SIZES: [[(usize, usize); 4]; 3] = [
            [(8, 8), (16, 16), (32, 32), (64, 64)],
            [(16, 8), (32, 8), (32, 16), (64, 32)],
            [(8, 16), (8, 32), (16, 32), (32, 64)],
        ];
        let mut mask = vec![false; SCREEN_W * SCREEN_H];
        for o in oam.chunks(8).take(128) {
            let (a0, a1) = (u16::from_le_bytes([o[0], o[1]]), u16::from_le_bytes([o[2], o[3]]));
            let (affine, double) = (a0 >> 8 & 1 == 1, a0 >> 9 & 1 == 1);
            if (!affine && double) || a0 >> 14 == 3 {
                continue;
            }
            let (mut w, mut h) = SIZES[(a0 >> 14) as usize][(a1 >> 14) as usize];
            if double {
                (w, h) = (2 * w, 2 * h);
            }
            for dy in 0..h {
                let y = ((a0 & 255) as usize + dy) & 255;
                for dx in 0..w {
                    let x = ((a1 & 511) as usize + dx) & 511;
                    if y < SCREEN_H && x < SCREEN_W {
                        mask[y * SCREEN_W + x] = true;
                    }
                }
            }
        }
        mask
    }

    /// Pure maths, no data needed: the four-fold panorama and the ±32 horizon range.
    #[test]
    fn column_and_start_ranges() {
        assert_eq!(skyline_column(0x0FFD, 240), 239);
        assert_eq!(skyline_column(0x1000, 240), 0);
        assert_eq!(skyline_column(-0x1C1A, 240), 58);
        let sky = SkyDesc {
            gradient: 0,
            skyline: 0,
            width: 240,
            height: 64,
            skyline_y: 4,
            gradient_top: 25,
        };
        let at = |horizon| {
            gradient_start(
                &sky,
                &SkyCamera {
                    horizon,
                    ..Default::default()
                },
            )
        };
        assert_eq!((at(-32), at(0), at(32)), (37, 21, 5));
        assert_eq!(backdrop_entry(at(-32), 159), 76);
    }
}
