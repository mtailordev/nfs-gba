//! The race, in every viewer mode, is an `nfsgba_game::Game`: the viewer's race state (racers, camera, visible list,
//! palette, original frame) is read back from it (`nfsgba_game::{view, race_init}`), and the HUD is the game's OAM
//! drawn as a 2D layer. Three ways to get one:
//! - `NFSGBA_PLAY=1` with `NFSGBA_DUMP`: the keyboard drives the game, one game frame every four video frames
//!   (59.7275 Hz), from the dump's machine state (`docs/engine/game-loop.md`);
//! - `NFSGBA_DUMP` alone: the same machine, paused;
//! - a route (`NFSGBA_ROUTE`, R, K): `race_init::race_start` on the pre-race capture with the setup poked in
//!   (`apply_setup`), paused.
//!
//! NOT 1:1 (G1): a route's race start is paused; the handover to `Game::frame` (intro, countdown, fades) is not
//! ported. The camera and matrix slots are the game's (`race_init::pose`), the light tint the viewer's.
//!
//! Keys: arrows = D-pad, X = A (accelerate), Z = B (brake), A = L, S = R, Enter = START, Backspace = SELECT.
//! `NFSGBA_PLAY_KEYS=A*40,A+LEFT*12,...` plays a script instead (GBA key names, counts in game frames).
//!
//! NOT 1:1 (live play, T1): each game frame gets the steady timing of the reference race (`Timing::steady`); the
//! game's frame itself is the exact `Game::frame`.

use std::{
    collections::VecDeque,
    io,
    num::NonZero,
    sync::{Arc, Mutex},
};

use bevy::{
    asset::RenderAssetUsages,
    audio::{ChannelCount, Decodable, Sample, SampleRate, Source},
    image::ImageSampler,
    prelude::*,
};
use nfsgba_formats as rom;
use nfsgba_game::{Game, Machine, Timing, race_init, view};

use crate::{Race, texture_2d};

/// The car of a route's Quick Play race (the reference race's Cobalt).
pub const PLAYER_CAR: u8 = 2;
/// The VBlanks before `setup_race_cars` reads the tick counter (6 or 7 in the recorded race starts).
const SEED_VBLANKS: u32 = 7;

pub const VIDEO_HZ: f32 = 59.7275;
const KEYS: [&str; 10] = ["A", "B", "SELECT", "START", "RIGHT", "LEFT", "UP", "DOWN", "R", "L"];

#[derive(Resource)]
pub struct Play {
    pub game: Game,
    /// The game does not run (a dump without `NFSGBA_PLAY`, a route's race start).
    pub paused: bool,
    /// (environment, route) of a route's race start; `None` for a dump.
    pub grid: Option<(u32, u32)>,
    clock: f32,
    pub frames: u64,
    /// Why play stopped (a game code path that is not ported), if it did.
    pub stopped: Option<String>,
    script: Option<Vec<u16>>,
    pub hud: Handle<Image>,
    /// The samples the game's sound hardware played, waiting for the audio device.
    pub sound: Arc<Mutex<VecDeque<Sample>>>,
}

/// The GBA's sound output (Direct Sound A and B play the same buffer: mono, signed 8-bit, 10,512 Hz) as a Bevy
/// audio source that plays whatever `play` queued, and silence when the queue runs dry.
/// NOT 1:1 (A6): the hardware rate is 10,512.04 Hz; the DAC and `SOUNDBIAS` are not modelled.
#[derive(Asset, TypePath)]
pub struct GbaSound(Arc<Mutex<VecDeque<Sample>>>);

pub struct GbaStream(Arc<Mutex<VecDeque<Sample>>>);

impl Iterator for GbaStream {
    type Item = Sample;
    fn next(&mut self) -> Option<Sample> {
        Some(self.0.lock().map_or(0.0, |mut q| q.pop_front().unwrap_or(0.0)))
    }
}

impl Source for GbaStream {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> ChannelCount {
        NonZero::new(1).unwrap()
    }
    fn sample_rate(&self) -> SampleRate {
        NonZero::new(10_512).unwrap()
    }
    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

impl Decodable for GbaSound {
    type Decoder = GbaStream;
    fn decoder(&self) -> GbaStream {
        GbaStream(self.0.clone())
    }
}

/// Starts the game's sound stream (play mode).
pub fn start_sound(mut commands: Commands, play: Res<Play>, mut sounds: ResMut<Assets<GbaSound>>) {
    commands.spawn(AudioPlayer(sounds.add(GbaSound(play.sound.clone()))));
}

impl Play {
    /// A race dump (`prefix` under `$NFSGBA_DATA/work/e5298b24/`), running or paused.
    pub fn load(rom_bytes: Vec<u8>, prefix: &str, hud: Handle<Image>, running: bool) -> io::Result<Play> {
        let path = rom::data_dir().join("work/e5298b24").join(prefix);
        Ok(Play::new(Machine::load_dump(rom_bytes, &path)?, hud, !running, None))
    }

    /// A Quick Play race start on `route` in environment `env`, paused (`race_init::start`).
    pub fn grid(rom_bytes: Vec<u8>, env: u32, route: u32, hud: Handle<Image>) -> io::Result<Play> {
        let path = rom::data_dir().join("work/e5298b24/race-init/circuit_pre");
        let (mut machine, mut io) = race_init::load_pre(rom_bytes, &path)?;
        race_init::apply_setup(&mut machine, env, route, 0, PLAYER_CAR);
        race_init::race_start(&mut machine, &mut io, SEED_VBLANKS).map_err(|e| io::Error::other(e.to_string()))?;
        race_init::enter_race(&mut machine.mem);
        let mut play = Play::new(machine, hud, true, Some((env, route)));
        // The first race frame runs up to the countdown, which is not ported (G1): the drivers, camera, matrix slots
        // and the world are the game's; anything else that stops it is an error.
        match play.game.frame(0, &Timing::steady()) {
            Err(e) if e.to_string().contains("countdown") => Ok(play),
            Err(e) => Err(io::Error::other(e.to_string())),
            Ok(()) => Ok(play),
        }
    }

    fn new(machine: Machine, hud: Handle<Image>, paused: bool, grid: Option<(u32, u32)>) -> Play {
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
        Play {
            game: Game::new(machine),
            paused,
            grid,
            clock: 0.0,
            frames: 0,
            stopped: None,
            script,
            hud,
            sound: Arc::default(),
        }
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

/// Runs the game frames that are due (unless paused) and reads the race back from the game's RAM.
pub fn play(time: Res<Time>, input: Res<ButtonInput<KeyCode>>, mut play: ResMut<Play>, mut race: ResMut<Race>) {
    if !play.paused && play.stopped.is_none() {
        play.clock += time.delta_secs();
    }
    let step = 4.0 / VIDEO_HZ;
    while play.clock >= step && !play.paused && play.stopped.is_none() {
        play.clock -= step;
        let keys = match &play.script {
            Some(s) => s.get(play.frames as usize).copied().unwrap_or(0),
            None => keyboard(&input),
        };
        if let Err(e) = play.game.frame(keys, &Timing::steady()) {
            warn!("play stopped at game frame {}: {e}", play.frames);
            play.stopped = Some(e.to_string());
            break;
        }
        if let Ok(mut q) = play.sound.lock() {
            q.extend(play.game.sound.iter().map(|&s| s as i8 as Sample / 128.0));
            // Keep at most a quarter of a second queued, so the sound stays with the picture.
            let excess = q.len().saturating_sub(10_512 / 4);
            q.drain(..excess);
        }
        play.frames += 1;
    }
    race.setup = view::RaceView::read(play.game.mem());
    race.current = race.setup.route % race.routes.len();
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
/// NOT 1:1 (G2, high-resolution view only): semi-transparent sprites are drawn at 50% alpha (the blend brightens the
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
        let px: Option<[u8; 4]> = match (o, race.original && !play.paused) {
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
