//! Play mode (`NFSGBA_PLAY=1` with `NFSGBA_DUMP`): the keyboard drives `nfsgba_game::Game`, one game frame every
//! four video frames (59.7275 Hz), from the dump's machine state. The viewer's race state (racers, camera, visible
//! list, palette, original frame) is read back from the game's RAM after every game frame, and the HUD is the
//! game's OAM drawn as a 2D layer. `docs/engine/game-loop.md`.
//!
//! Keys: arrows = D-pad, X = A (accelerate), Z = B (brake), A = L, S = R, Enter = START, Backspace = SELECT.
//! `NFSGBA_PLAY_KEYS=A*40,A+LEFT*12,...` plays a script instead (GBA key names, counts in game frames).
//!
//! NOT 1:1 (live play): each game frame gets the steady timing of the reference race (`Timing::steady`); the
//! camera is the viewer's `Chase` (R11 gaps), the matrix slots `standin::slots` (R25); opponents and traffic keep
//! their state (D4); the sound is not played yet.

use std::io;

use bevy::{asset::RenderAssetUsages, image::ImageSampler, prelude::*};
use nfsgba_formats as rom;
use nfsgba_game::{Checkpoint, Game, Machine, Timing, standin, view};

use crate::{
    Race, Tint,
    game::{Dump, RaceSetup},
    texture_2d,
};

pub const VIDEO_HZ: f32 = 59.7275;
const KEYS: [&str; 10] = ["A", "B", "SELECT", "START", "RIGHT", "LEFT", "UP", "DOWN", "R", "L"];

#[derive(Resource)]
pub struct Play {
    pub game: Game,
    clock: f32,
    pub frames: u64,
    /// Why play stopped (a game code path that is not ported), if it did.
    pub stopped: Option<String>,
    script: Option<Vec<u16>>,
    pub hud: Handle<Image>,
}

impl Play {
    pub fn load(rom_bytes: Vec<u8>, prefix: &str, hud: Handle<Image>) -> io::Result<Play> {
        let path = rom::data_dir().join("work/e5298b24").join(prefix);
        let script = std::env::var("NFSGBA_PLAY_KEYS").ok().map(|s| {
            s.split(',')
                .flat_map(|step| {
                    let (keys, n) = step.split_once('*').unwrap_or((step, "1"));
                    let mask = keys
                        .split('+')
                        .filter_map(|k| KEYS.iter().position(|&n| n == k.trim()))
                        .fold(0u16, |m, b| m | 1 << b);
                    std::iter::repeat_n(mask, n.trim().parse().unwrap_or(1))
                })
                .collect()
        });
        Ok(Play {
            game: Game::new(Machine::load_dump(rom_bytes, &path)?),
            clock: 0.0,
            frames: 0,
            stopped: None,
            script,
            hud,
        })
    }
}

pub fn hud_image() -> Image {
    let mut image = texture_2d(
        240,
        160,
        vec![0; 240 * 160 * 4],
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
    );
    image.sampler = ImageSampler::nearest();
    image.asset_usage = RenderAssetUsages::all();
    image
}

fn keyboard(k: &ButtonInput<KeyCode>) -> u16 {
    let map = [
        (KeyCode::KeyX, 0),
        (KeyCode::KeyZ, 1),
        (KeyCode::Backspace, 2),
        (KeyCode::Enter, 3),
        (KeyCode::ArrowRight, 4),
        (KeyCode::ArrowLeft, 5),
        (KeyCode::ArrowUp, 6),
        (KeyCode::ArrowDown, 7),
        (KeyCode::KeyS, 8),
        (KeyCode::KeyA, 9),
    ];
    map.iter()
        .filter(|(c, _)| k.pressed(*c))
        .fold(0, |m, (_, b)| m | 1 << b)
}

/// NOT 1:1 (R11), standing in for `camera_update`: the viewer's `Chase` (chase view) for the player, written where
/// the game keeps the camera: orbit and look yaw, position, sector, focal, the matrix `0x030057A0` and list entry 0.
fn camera(g: &mut Game, rom_bytes: &[u8], rt: &rom::render::Runtime) {
    let m = &g.sim.mem;
    let setup = RaceSetup::from_dump(&Dump::from_ram(m.iwram.clone(), m.ewram.clone()));
    let mut chase = setup.chase;
    let (frame, root) = chase.step(rom_bytes, rt, &setup.racers[0]);
    let m = &mut g.sim.mem;
    m.set_i32(0x0300_5F94, chase.yaw);
    m.set_i32(0x0300_0214, chase.look);
    m.set_i32(0x0300_5F9C, -chase.look & 0x3FFF);
    m.set_i32(0x0300_56A0, chase.x);
    m.set_i32(0x0300_00A4, chase.z);
    m.set_u32(0x0300_5614, chase.sector as u32);
    m.set_i32(0x0300_0080 + 0x1C, chase.focal);
    let cam = m.u32(view::WORLD + 0x54);
    for (k, v) in frame.camera.iter().enumerate() {
        m.set_i32(cam + 4 * k as u32, *v);
    }
    let list = m.u32(view::WORLD + 0x60);
    m.set_u16(list, root.sector);
    for (k, v) in [root.left, root.right, root.top, root.bottom].into_iter().enumerate() {
        m.set_i16(list + 2 + 2 * k as u32, v);
    }
}

/// Runs the game frames that are due and reads the race back from the game's RAM.
pub fn play(
    time: Res<Time>,
    input: Res<ButtonInput<KeyCode>>,
    mut play: ResMut<Play>,
    mut race: ResMut<Race>,
    tint: Res<Tint>,
) {
    if play.stopped.is_some() {
        return;
    }
    play.clock += time.delta_secs();
    let step = 4.0 / VIDEO_HZ;
    let mut ran = false;
    while play.clock >= step {
        play.clock -= step;
        let keys = match &play.script {
            Some(s) => s.get(play.frames as usize).copied().unwrap_or(0),
            None => keyboard(&input),
        };
        let (rom_bytes, rt) = (&tint.rom, &tint.rt);
        let r = play.game.frame_with(keys, &Timing::steady(), &mut |at, g| {
            match at {
                Checkpoint::Entities => {}
                Checkpoint::Camera => camera(g, rom_bytes, rt),
                Checkpoint::Slots => standin::slots(&mut g.sim.mem),
            }
            true
        });
        if let Err(e) = r {
            warn!("play stopped at game frame {}: {e}", play.frames);
            play.stopped = Some(e.to_string());
            break;
        }
        play.frames += 1;
        ran = true;
    }
    if ran {
        let m = play.game.mem();
        let dump = Dump::from_ram(m.iwram.clone(), m.ewram.clone());
        race.setup = RaceSetup::from_dump(&dump);
        race.dump = Some(dump);
    }
}

/// The game's frame, camera and visible list, straight from its RAM (play mode).
pub fn frame(play: &Play, rom_bytes: &[u8]) -> (rom::render::Frame, rom::render::Portal, rom::render::Visibility) {
    let m = play.game.mem();
    (view::frame(m), view::root(m), view::visible(rom_bytes, m))
}

/// `BLDALPHA` in the race (`0x0D0F`: EVA 15/16, EVB 13/16; `BLDCNT` `0x3F3F`, every layer a second target), read
/// from `mgba/race.io.bin`. The machine state does not carry the IO registers; the race sets them once.
const BLEND: (u32, u32) = (15, 13);

/// The GBA screen of play mode. Original frame: the whole screen composed as the GBA does, the mode-4 page through
/// the BG palette (index 0: the line's backdrop colour), then the top sprite pixel over it, semi-transparent sprites
/// (OBJ mode 1) blended `min(31, (obj·EVA + bg·EVB) >> 4)` per channel. High-resolution view: the sprites alone over
/// the GPU image.
/// NOT 1:1 (high-resolution view only): semi-transparent sprites are drawn at 50% alpha (the blend brightens the
/// GPU image, which the 2D layer cannot read). OBJ priority against the background is not modelled (the HUD is
/// always in front).
pub fn hud_layer(play: Res<Play>, race: Res<Race>, mut images: ResMut<Assets<Image>>) {
    let g = &play.game;
    let colour = |pal: &[u8], i: usize| u16::from_le_bytes([pal[2 * i], pal[2 * i + 1]]);
    let obj_palette: Vec<u16> = (0..256).map(|i| colour(&g.palette[0x200..], i)).collect();
    let objects = draw_objects(&g.oam, &g.vram[0x1_0000..], &obj_palette);
    let backdrop = g.backdrop();
    let mut out = vec![0u8; 240 * 160 * 4];
    for (p, o) in objects.into_iter().enumerate() {
        let bg = match g.screen()[p] {
            0 => backdrop[p / 240],
            i => colour(&g.palette, i as usize),
        };
        let rgb = |c: u16| {
            let [r, g, b, _] = rom::bgr555(c);
            [r, g, b]
        };
        let px: Option<[u8; 4]> = match (o, race.original) {
            (Some((c, true)), true) => {
                let ch = |s: u16| {
                    let (a, b) = (((c >> s) & 31) as u32, ((bg >> s) & 31) as u32);
                    ((a * BLEND.0 + b * BLEND.1) >> 4).min(31) as u16
                };
                let [r, g, b] = rgb(ch(0) | ch(5) << 5 | ch(10) << 10);
                Some([r, g, b, 255])
            }
            (Some((c, _)), true) => {
                let [r, g, b] = rgb(c);
                Some([r, g, b, 255])
            }
            (None, true) => {
                let [r, g, b] = rgb(bg);
                Some([r, g, b, 255])
            }
            (Some((c, semi)), false) => {
                let [r, g, b] = rgb(c);
                Some([r, g, b, if semi { 128 } else { 255 }])
            }
            (None, false) => None,
        };
        if let Some(px) = px {
            out[4 * p..4 * p + 4].copy_from_slice(&px);
        }
    }
    if let Some(mut image) = images.get_mut(&play.hud) {
        image.data = Some(out);
    }
}

/// OBJ layer of the GBA (1-D tile mapping, as the game sets it): regular and affine sprites, 16 and 256 colours;
/// per screen pixel the top sprite's colour (earlier OAM entries over later ones) and whether it is
/// semi-transparent.
pub fn draw_objects(oam: &[u8], tiles: &[u8], palette: &[u16]) -> Vec<Option<(u16, bool)>> {
    let mut out = vec![None; 240 * 160];
    const SIZES: [[(i32, i32); 4]; 3] = [
        [(8, 8), (16, 16), (32, 32), (64, 64)],
        [(16, 8), (32, 8), (32, 16), (64, 32)],
        [(8, 16), (8, 32), (16, 32), (32, 64)],
    ];
    let h = |i: usize| u16::from_le_bytes([oam[i], oam[i + 1]]);
    for i in (0..128).rev() {
        let (a0, a1, a2) = (h(8 * i), h(8 * i + 2), h(8 * i + 4));
        let mode = (a0 >> 8) & 3;
        let shape = (a0 >> 14) as usize;
        if mode == 2 || shape == 3 || (a0 >> 10) & 3 == 2 {
            continue;
        }
        let (w, ht) = SIZES[shape][(a1 >> 14) as usize];
        let (bw, bh) = if mode == 3 { (2 * w, 2 * ht) } else { (w, ht) };
        let (mut x, mut y) = ((a1 & 0x1FF) as i32, (a0 & 0xFF) as i32);
        if x >= 256 {
            x -= 512;
        }
        if y >= 160 {
            y -= 256;
        }
        let colours256 = a0 & 0x2000 != 0;
        let semi = (a0 >> 10) & 3 == 1;
        let (tile, bank) = ((a2 & 0x3FF) as usize, (a2 >> 12) as usize);
        let matrix = |k: usize| h(8 * (4 * ((a1 >> 9) & 0x1F) as usize + k) + 6) as i16 as i32;
        for py in 0..bh {
            for px in 0..bw {
                let (sx, sy) = (x + px, y + py);
                if !(0..240).contains(&sx) || !(0..160).contains(&sy) {
                    continue;
                }
                let (tx, ty) = if mode & 1 == 1 {
                    let (dx, dy) = (px - bw / 2, py - bh / 2);
                    (
                        ((matrix(0) * dx + matrix(1) * dy) >> 8) + w / 2,
                        ((matrix(2) * dx + matrix(3) * dy) >> 8) + ht / 2,
                    )
                } else {
                    (
                        if a1 & 0x1000 != 0 { w - 1 - px } else { px },
                        if a1 & 0x2000 != 0 { ht - 1 - py } else { py },
                    )
                };
                if !(0..w).contains(&tx) || !(0..ht).contains(&ty) {
                    continue;
                }
                let (tx, ty) = (tx as usize, ty as usize);
                let index = if colours256 {
                    let t = tile + 2 * ((ty / 8) * (w as usize / 8) + tx / 8);
                    tiles.get(32 * t + (ty % 8) * 8 + tx % 8).copied().unwrap_or(0) as usize
                } else {
                    let t = tile + (ty / 8) * (w as usize / 8) + tx / 8;
                    let b = tiles.get(32 * t + (ty % 8) * 4 + (tx % 8) / 2).copied().unwrap_or(0);
                    (if tx % 2 == 0 { b & 0xF } else { b >> 4 }) as usize + 16 * bank
                };
                if index % if colours256 { 256 } else { 16 } == 0 {
                    continue;
                }
                out[sy as usize * 240 + sx as usize] = Some((palette[index], semi));
            }
        }
    }
    out
}
