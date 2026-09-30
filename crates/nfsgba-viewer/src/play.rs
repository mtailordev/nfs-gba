//! The race, in every viewer mode, is an `nfsgba_game::Game`: the viewer's race state (racers, camera, visible list,
//! palette, original frame) is read back from it (`nfsgba_game::{view, race_init}`), and the HUD is the game's OAM
//! drawn as a 2D layer. Four ways to get one:
//! - the full game (the default; `NFSGBA_GAME=1`): an `nfsgba_game::session::Session` from power-on with the save
//!   `viewer.sav`. The menus are the exact 240×160 frame ([`menu_layer`], integer-scaled, centred); each race the
//!   session starts is lent to [`Play::game`] (the session keeps a spare there) so every race system reads it as in the
//!   other modes, and the session gets it back for the next frame; results, pause and the next race are the session's;
//! - `NFSGBA_PLAY=1` with `NFSGBA_DUMP`: the keyboard drives the game, one game frame every four video frames
//!   (59.7275 Hz), from the dump's machine state (`docs/engine/game-loop.md`);
//! - `NFSGBA_DUMP` alone: the same machine, paused;
//! - a route (`NFSGBA_ROUTE`, R, K): the typed `race_init::start` on the pre-race capture's `Setup` with
//!   the choice made (`Setup::choose`), paused, or with `NFSGBA_PLAY=1` played from the grid (intro, countdown, GO).
//!
//! The camera and matrix slots are the game's (`race_init::pose`), the light tint the viewer's.
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
    path::PathBuf,
    sync::{Arc, Mutex},
};

use bevy::{
    asset::RenderAssetUsages,
    audio::{ChannelCount, Decodable, Sample, SampleRate, Source},
    image::ImageSampler,
    prelude::*,
    render::render_resource::TextureFormat,
};
use nfsgba_formats as rom;
use nfsgba_game::{Flow, Game, Machine, Timing, race_init, race_setup, session::Session, view};

use crate::{Race, texture_2d};

/// The car of a route's Quick Play race (the reference race's Cobalt).
pub const PLAYER_CAR: u8 = 2;
/// The VBlanks before `setup_race_cars` reads the tick counter (6 or 7 in the recorded race starts).
const SEED_VBLANKS: u32 = 7;

static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

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
    /// Distinguishes one `Play` from the next (a new race start).
    pub id: u64,
    pub frames: u64,
    /// Why play stopped (a game code path that is not ported), if it did.
    pub stopped: Option<String>,
    /// What to tell the player when play stopped at a hand-over to the menus (not connected yet).
    pub banner: Option<String>,
    script: Option<Vec<u16>>,
    pub hud: Handle<Image>,
    /// The samples the game's sound hardware played, waiting for the audio device.
    pub sound: Arc<Mutex<VecDeque<Sample>>>,
    /// The full game (`NFSGBA_GAME`): the session that owns the race while it is not lent to `game`.
    pub full: Option<Full>,
}

/// The whole game from power-on. A step is a video frame in the menus and a game frame (four video frames) in a race.
pub struct Full {
    pub session: Session<'static>,
    /// The race is in `Play::game` and the session holds a spare there.
    lent: bool,
    /// The samples of the last step.
    sound: Vec<u8>,
    /// The save file, and its bytes as last written (web: the page keeps the save, `crate::web`).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    sav: Option<PathBuf>,
    saved: Vec<u8>,
}

impl Full {
    /// The game at power-on with the save at `sav` (created on the first save).
    pub fn new(rom_bytes: Vec<u8>, sav: Option<PathBuf>) -> Full {
        #[cfg(not(target_arch = "wasm32"))]
        let saved = sav.as_ref().and_then(|p| std::fs::read(p).ok());
        #[cfg(target_arch = "wasm32")]
        let saved = crate::web::save();
        let saved = saved.unwrap_or_else(|| vec![0xFF; 512]);
        // ponytail: the ROM is leaked (8 MB, once per process) because the session borrows it for its lifetime.
        let rom: &'static [u8] = Box::leak(rom_bytes.into_boxed_slice());
        Full {
            session: Session::new(rom, saved.clone()),
            lent: false,
            sound: Vec::new(),
            sav,
            saved,
        }
    }
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
    /// How far the display is between the last two game frames (0..1; 1 when nothing runs).
    pub fn alpha(&self) -> f32 {
        if self.paused || self.stopped.is_some() {
            1.0
        } else {
            (self.clock / self.step_secs()).clamp(0.0, 1.0)
        }
    }

    /// A race dump (`prefix` under `$NFSGBA_DATA/work/e5298b24/`), running or paused.
    pub fn load(rom_bytes: Vec<u8>, prefix: &str, hud: Handle<Image>, running: bool) -> io::Result<Play> {
        let path = rom::data_dir().join("work/e5298b24").join(prefix);
        Ok(Play::new(
            Game::new(Machine::load_dump(rom_bytes, &path)?),
            hud,
            !running,
            None,
        ))
    }

    /// A Quick Play race start on `route` in environment `env` (`race_init::start`), paused or, with `running`,
    /// played from the grid (intro, countdown, GO).
    pub fn grid(rom_bytes: Vec<u8>, env: u32, route: u32, hud: Handle<Image>, running: bool) -> io::Result<Play> {
        let path = rom::data_dir().join("work/e5298b24/race-init/circuit_pre");
        let (mut setup, display) = race_setup::load_pre(rom_bytes.clone(), &path)?;
        setup.choose(env, route, 0, PLAYER_CAR);
        let game =
            race_init::start(rom_bytes, &setup, display, SEED_VBLANKS).map_err(|e| io::Error::other(e.to_string()))?;
        let mut play = Play::new(game, hud, !running, Some((env, route)));
        // The first race frame runs up to the countdown, which is not ported (G1): the drivers, camera, matrix slots
        // and the world are the game's; anything else that stops it is an error.
        match play.game.frame(0, &Timing::steady()) {
            Err(e) if e.to_string().contains("countdown") => Ok(play),
            Err(e) => Err(io::Error::other(e.to_string())),
            Ok(_) => Ok(play),
        }
    }

    /// The full game's spare race (`with_full`): a Quick Play start on `route` in environment `env` from the menus'
    /// setup (`Setup::menus`), so it needs no capture. It is never shown: the menus cover it until the first race.
    pub fn spare(rom_bytes: Vec<u8>, env: u32, route: u32, hud: Handle<Image>) -> Result<Play, String> {
        let engine = nfsgba_audio::Engine::new(nfsgba_audio::Rom(&rom_bytes), 16, 16);
        let mut setup = race_setup::Setup::menus(&rom_bytes, engine, 0, 0, &[[0; 17]; 15], None);
        setup.choose(env, route, 0, PLAYER_CAR);
        let display = race_setup::Display::from_screen(&Default::default());
        let game = race_init::start(rom_bytes, &setup, display, SEED_VBLANKS).map_err(|e| e.to_string())?;
        Ok(Play::new(game, hud, true, None))
    }

    pub fn new(game: Game, hud: Handle<Image>, paused: bool, grid: Option<(u32, u32)>) -> Play {
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
            game,
            paused,
            grid,
            clock: 0.0,
            id: NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            frames: 0,
            stopped: None,
            banner: None,
            script,
            hud,
            sound: Arc::default(),
            full: None,
        }
    }

    /// The full game from power-on; `self` (any race start, paused) is the spare the session's races are lent through.
    pub fn with_full(mut self, full: Full) -> Play {
        (self.full, self.paused) = (Some(full), false);
        self
    }

    /// A race is on the screen (always, but in the full game's menus).
    pub fn in_race(&self) -> bool {
        self.full.as_ref().is_none_or(|f| f.lent)
    }

    /// The video time one step takes.
    fn step_secs(&self) -> f32 {
        (if self.in_race() { 4.0 } else { 1.0 }) / VIDEO_HZ
    }

    /// The samples the last step played.
    fn samples(&self) -> &[u8] {
        self.full.as_ref().map_or(&self.game.sound, |f| &f.sound)
    }

    /// One step with `keys`. In the full game: the session's frame, the race lent to `game` while it runs, the save
    /// written when the game changed it. A new race gets a new `id`; the second value says so.
    fn step(&mut self, keys: u16) -> Result<(Flow, bool), String> {
        let Some(full) = &mut self.full else {
            return Ok((
                self.game.frame(keys, &Timing::steady()).map_err(|e| e.to_string())?,
                false,
            ));
        };
        let had_race = full.session.race.is_some() || full.lent;
        if full.lent {
            std::mem::swap(&mut self.game, full.session.race.as_mut().expect("the lent race"));
        }
        let result = full.session.frame(keys).map_err(|e| e.to_string());
        full.sound = full.session.sound().to_vec();
        full.lent = full.session.racing();
        if full.lent {
            std::mem::swap(&mut self.game, full.session.race.as_mut().expect("a race"));
        }
        let eeprom = &full.session.host.eeprom;
        #[cfg(target_arch = "wasm32")]
        if *eeprom != full.saved {
            crate::web::saved(eeprom);
            full.saved.clone_from(eeprom);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let (Some(path), true) = (&full.sav, *eeprom != full.saved) {
            match std::fs::write(path, eeprom) {
                Ok(()) => full.saved.clone_from(eeprom),
                Err(e) => warn!("save {}: {e}", path.display()),
            }
        }
        result.map(|()| (Flow::Racing, full.lent && !had_race))
    }
}

/// The text shown when the race hands over to the menus, which are not connected: for the results the player's
/// place and finish time (`ranked` is in finishing order; times are 1/60 s), else a note.
fn banner(h: &nfsgba_game::Handover) -> String {
    use nfsgba_game::Handover;
    let next = "R or Enter: next race";
    match h {
        Handover::Results(r) => {
            let k = r
                .ranked
                .ids
                .iter()
                .position(|&id| u32::from(id) == r.last_player)
                .unwrap_or(0);
            let cs = r.ranked.finish[k] as u64 * 100 / 60;
            format!(
                "Race over: place {} of 4, {:02}:{:02}.{:02}
{next}",
                k + 1,
                cs / 6000,
                cs / 100 % 60,
                cs % 100
            )
        }
        Handover::Pause => format!(
            "Paused (the menus are not connected)
{next}"
        ),
    }
}

/// A 240×160 RGBA8 layer: the HUD (raw bytes, read by `composite.wgsl`) or the menus' screen (sRGB, shown by the UI).
pub fn hud_image(format: TextureFormat) -> Image {
    let mut image = texture_2d(240, 160, vec![0; 240 * 160 * 4], format);
    image.sampler = ImageSampler::nearest();
    image.asset_usage = RenderAssetUsages::all();
    image
}

/// The GBA keys held on the keyboard and any gamepad (South = A, East = B, the shoulders and triggers = L and R, the
/// D-pad or left stick = the D-pad).
fn keys_held(k: &ButtonInput<KeyCode>, gamepads: &Query<&Gamepad>) -> u16 {
    use GamepadButton as B;
    let buttons = [
        (B::South, 0),
        (B::East, 1),
        (B::Select, 2),
        (B::Start, 3),
        (B::DPadRight, 4),
        (B::DPadLeft, 5),
        (B::DPadUp, 6),
        (B::DPadDown, 7),
        (B::RightTrigger, 8),
        (B::RightTrigger2, 8),
        (B::LeftTrigger, 9),
        (B::LeftTrigger2, 9),
    ];
    gamepads.iter().fold(keyboard(k), |mut m, pad| {
        for (b, bit) in buttons {
            if pad.pressed(b) {
                m |= 1 << bit;
            }
        }
        let s = pad.left_stick();
        for (on, bit) in [(s.x > 0.5, 4), (s.x < -0.5, 5), (s.y > 0.5, 6), (s.y < -0.5, 7)] {
            m |= u16::from(on) << bit;
        }
        m
    })
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
pub fn play(
    time: Res<Time>,
    input: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut play: ResMut<Play>,
    mut race: ResMut<Race>,
    mut stats: Local<(f32, u32, u32, f32, f32)>,
) {
    if !play.paused && play.stopped.is_none() {
        // A slow game frame must not make the next display frames run several (a spiral): at most 3 are owed.
        play.clock = (play.clock + time.delta_secs()).min(3.0 * play.step_secs());
        // Display fps and game-frame time, logged every 3 s (display frames, game frames, seconds, ms sum, ms max).
        stats.0 += time.delta_secs();
        stats.1 += 1;
        if stats.0 >= 3.0 {
            let (secs, shown, games, sum, max) = *stats;
            info!(
                "display {:.1} fps, {games} game frames ({:.1}/s), game frame {:.2} ms avg, {max:.2} ms max",
                shown as f32 / secs,
                games as f32 / secs,
                sum / games.max(1) as f32
            );
            *stats = (0.0, 0, 0, 0.0, 0.0);
        }
    }
    while play.clock >= play.step_secs() && !play.paused && play.stopped.is_none() {
        play.clock -= play.step_secs();
        let keys = match &play.script {
            Some(s) => s.get(play.frames as usize).copied().unwrap_or(0),
            None => keys_held(&input, &gamepads),
        };
        let began = bevy::platform::time::Instant::now();
        let result = play.step(keys);
        let ms = began.elapsed().as_secs_f32() * 1000.0;
        (stats.2, stats.3, stats.4) = (stats.2 + 1, stats.3 + ms, stats.4.max(ms));
        match result {
            Err(e) => {
                warn!("play stopped at game frame {}: {e}", play.frames);
                play.banner = play.full.as_ref().map(|_| format!("Stopped: {e}"));
                play.stopped = Some(e);
                break;
            }
            Ok((_, true)) => {
                // A new race: a new identity (the viewer remakes its cars), the game camera.
                play.id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                race.game_camera = true;
            }
            Ok((Flow::Handover(h), _)) => {
                play.banner = Some(banner(&h));
                warn!(
                    "play stopped at game frame {}: the race hands over to the menus ({h:?})",
                    play.frames
                );
                play.stopped = Some("hand-over to the menus".into());
                break;
            }
            Ok(_) => {}
        }
        if let Ok(mut q) = play.sound.lock() {
            q.extend(play.samples().iter().map(|&s| s as i8 as Sample / 128.0));
            // Keep at most a quarter of a second queued, so the sound stays with the picture.
            let excess = q.len().saturating_sub(10_512 / 4);
            q.drain(..excess);
        }
        play.frames += 1;
    }
    if play.full.is_some() {
        race.active = play.in_race();
    }
    race.setup = view::RaceView::read(&play.game.world);
    race.current = race.setup.route % race.routes.len();
}

/// The game's frame, camera and visible list, straight from its RAM (play mode).
#[cfg(test)]
pub fn frame(play: &Play, rom_bytes: &[u8]) -> (rom::render::Frame, rom::render::Portal, rom::render::Visibility) {
    let m = &play.game.world;
    (view::frame(m), view::root(m), view::visible(rom_bytes, m))
}
// `BLDCNT` is `0x3F3F` in the race (every layer a second target), so every semi-transparent sprite pixel blends with
// what is under it; `Game::bldalpha` gives EVA and EVB.

/// (EVA, EVB) of a `BLDALPHA` value: each field's 5 bits, at most 16.
pub fn blend_of(bldalpha: u16) -> (u32, u32) {
    (
        u32::from(bldalpha & 0x1F).min(16),
        u32::from(bldalpha >> 8 & 0x1F).min(16),
    )
}

/// The game's alpha blend of a semi-transparent sprite colour over what is under it, both BGR555:
/// `min(31, (obj·EVA + bg·EVB) >> 4)` per 5-bit channel.
pub fn blend_555(obj: u16, under: u16, (eva, evb): (u32, u32)) -> u16 {
    let ch = |s: u16| {
        let (a, b) = (((obj >> s) & 31) as u32, ((under >> s) & 31) as u32);
        ((a * eva + b * evb) >> 4).min(31) as u16
    };
    ch(0) | ch(5) << 5 | ch(10) << 10
}

/// Tests: the HUD layer is left empty (the R27 check compares the world alone).
#[cfg(test)]
#[derive(Resource)]
pub struct NoHud;

/// The sprites of the game's OAM as they are drawn now (the effect sprites follow their cars between game frames).
pub fn hud_objects(play: &Play, race: &Race, smooth: &crate::Smooth) -> Vec<Option<(u16, bool)>> {
    let g = &play.game;
    let obj_palette: Vec<u16> = (0..256)
        .map(|i| u16::from_le_bytes([g.palette[0x200 + 2 * i], g.palette[0x201 + 2 * i]]))
        .collect();
    let mut oam = g.oam.clone();
    if !race.original && !play.paused {
        lock_effects(&mut oam, play, race, smooth);
    }
    draw_objects(&oam, &g.vram[0x1_0000..], &obj_palette)
}

/// A paused race's palette: palette RAM with the game's own light tint on it (`Game::tinted_palette`, slots 1..=143 and
/// 149..=255 as `Game::tint` writes them), shown as if the start's fade had finished.
pub fn paused_palette(g: &Game) -> Vec<u16> {
    let mut palette: Vec<u16> = (0..256)
        .map(|i| u16::from_le_bytes([g.palette[2 * i], g.palette[2 * i + 1]]))
        .collect();
    if let Some(tinted) = g.tinted_palette() {
        for i in (1..=143).chain(149..=255) {
            palette[i] = tinted[i];
        }
    }
    palette
}

/// The GBA screen of play mode. Original frame: the whole screen composed as the GBA does, the mode-4 page through
/// the BG palette (index 0: the line's backdrop colour), then the top sprite pixel over it, semi-transparent sprites
/// (OBJ mode 1) blended `min(31, (obj·EVA + bg·EVB) >> 4)` per channel. High-resolution view: the sprites alone,
/// mixed into the GPU image by the last pass (`composite.wgsl`), which does the same blend on the window's pixels
/// (EVA and EVB from `Game::bldalpha`).
pub fn hud_layer(
    play: Res<Play>,
    race: Res<Race>,
    smooth: Res<crate::Smooth>,
    fin: Res<crate::composite::Final>,
    mut composites: ResMut<Assets<crate::composite::Composite>>,
    mut images: ResMut<Assets<Image>>,
    #[cfg(test)] no_hud: Option<Res<NoHud>>,
) {
    #[cfg(test)]
    if no_hud.is_some() {
        return;
    }
    if !play.in_race() {
        return;
    }
    let g = &play.game;
    let (eva, evb) = blend_of(play.game.bldalpha);
    let want = UVec4::new(eva, evb, 0, 0);
    if composites.get(&fin.material).is_some_and(|m| m.blend != want)
        && let Some(mut m) = composites.get_mut(&fin.material)
    {
        m.blend = want;
    }
    let colour = |pal: &[u8], i: usize| u16::from_le_bytes([pal[2 * i], pal[2 * i + 1]]);
    let objects = hud_objects(&play, &race, &smooth);
    let backdrop = g.backdrop();
    let bg: Vec<u16> = (0..256).map(|i| colour(&g.palette, i)).collect();
    let out = compose(
        g.screen(),
        &bg,
        |line| backdrop[line],
        objects,
        (eva, evb),
        race.original && !play.paused,
    );
    if let Some(mut image) = images.get_mut(&play.hud) {
        image.data = Some(out);
    }
}

/// One RGBA8 GBA screen from the mode-4 `page` through the BG palette `bg` (index 0: `backdrop(line)`) and the sprite
/// layer `objects` (`draw_objects`). `exact`: the whole screen, semi-transparent sprites blended with `blend`
/// (EVA, EVB); else the sprites alone over transparent, semi-transparent ones at 50% alpha.
fn compose(
    page: &[u8],
    bg: &[u16],
    backdrop: impl Fn(usize) -> u16,
    objects: Vec<Option<(u16, bool)>>,
    blend: (u32, u32),
    exact: bool,
) -> Vec<u8> {
    let mut out = vec![0u8; 240 * 160 * 4];
    for (p, o) in objects.into_iter().enumerate() {
        let back = match page[p] {
            0 => backdrop(p / 240),
            i => bg[i as usize],
        };
        let rgb = |c: u16| {
            let [r, g, b, _] = rom::bgr555(c);
            [r, g, b]
        };
        let px: Option<[u8; 4]> = match (o, exact) {
            (Some((c, true)), true) => {
                let [r, g, b] = rgb(blend_555(c, back, blend));
                Some([r, g, b, 255])
            }
            (Some((c, _)), true) => {
                let [r, g, b] = rgb(c);
                Some([r, g, b, 255])
            }
            (None, true) => {
                let [r, g, b] = rgb(back);
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
    out
}

/// The full game's menus: a black layer over the window with the session's 240×160 screen (page, palettes and sprites
/// composed as the game shows them) in the middle, scaled by the largest whole number that fits.
#[derive(Component)]
pub struct MenuLayer;

/// The menu screen's image node (a child of the [`MenuLayer`]).
#[derive(Component)]
pub struct MenuScreen(Handle<Image>);

pub fn spawn_menu(commands: &mut Commands, images: &mut Assets<Image>, camera: UiTargetCamera) {
    let image = images.add(hud_image(TextureFormat::Rgba8UnormSrgb));
    commands
        .spawn((
            (MenuLayer, camera),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::BLACK),
            Visibility::Hidden,
        ))
        .with_child((MenuScreen(image.clone()), ImageNode::new(image), Node::default()));
}

pub fn menu_layer(
    play: Res<Play>,
    mut images: ResMut<Assets<Image>>,
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    mut layer: Single<&mut Visibility, With<MenuLayer>>,
    mut screen: Single<(&MenuScreen, &mut Node)>,
) {
    let menus = play.full.as_ref().filter(|_| !play.in_race());
    layer.set_if_neq(if menus.is_some() {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    });
    let Some(full) = menus else { return };
    let scale = (window.width() / 240.0).min(window.height() / 160.0).floor().max(1.0);
    (screen.1.width, screen.1.height) = (Val::Px(240.0 * scale), Val::Px(160.0 * scale));
    let v = full.session.view();
    let mut tiles = vec![0u8; 0x4000];
    tiles.extend_from_slice(v.obj_tiles);
    let oam: Vec<u8> = v.oam.iter().flatten().flat_map(|x| x.to_le_bytes()).collect();
    let objects = draw_objects(&oam, &tiles, &v.palette[256..]);
    let a = full.session.blend();
    let out = compose(v.page, &v.palette[..256], |_| v.palette[0], objects, blend_of(a), true);
    if let Some(mut image) = images.get_mut(&screen.0.0) {
        image.data = Some(out);
    }
}

/// Sprite sizes by shape and size code.
const SIZES: [[(i32, i32); 4]; 3] = [
    [(8, 8), (16, 16), (32, 32), (64, 64)],
    [(16, 8), (32, 8), (32, 16), (64, 32)],
    [(8, 16), (8, 32), (16, 32), (32, 64)],
];

/// A point in the world on the 240×160 screen as the game projects it (`None` behind the near plane).
fn screen_of(cam: &Transform, p: Vec3) -> Option<Vec2> {
    let v = cam.to_matrix().inverse().transform_point3(p) / crate::SCALE;
    let d = -v.z;
    (d > 1.0).then(|| Vec2::new(120.0 + 150.0 * v.x / (d + 1.0), 79.0 - 150.0 * v.y / (d + 1.0)))
}

/// The effect sprites (lights and flames: the game's sprite pool) sit on screen positions the game worked out for
/// its own frame's cars. While the display shows the cars between two game frames, each pool sprite that belongs to a
/// car (`World::pool_owner`: the matrix slot the game placed it through) moves by how far that car's screen position
/// changed from the game frame to the blended view. Sprites without a car (sparks, billboards) stay put.
fn lock_effects(oam: &mut [u8], play: &Play, race: &Race, smooth: &crate::Smooth) {
    let (Some(fc), Some((fb, _))) = (&smooth.curr.frame, &race.frame) else {
        return;
    };
    let (cam_n, cam_b) = (
        crate::game::frame_transform(fc, crate::world),
        crate::game::frame_transform(fb, crate::world),
    );
    let alpha = play.alpha();
    // Per entity: where the game frame put its origin on screen, and how far the blended view moves it.
    let delta_of = |entity: usize| -> Option<Vec2> {
        let (a, b) = (
            smooth.prev.ents.get(entity)?.as_ref()?,
            smooth.curr.ents.get(entity)?.as_ref()?,
        );
        let n = screen_of(&cam_n, b.translation)?;
        Some(screen_of(&cam_b, crate::blend(a, b, alpha).translation)? - n)
    };
    let world = &play.game.world;
    let first = world.pool_first as i32;
    for k in 0..world.pool.len() as i32 {
        let i = first - k;
        if !(0..128).contains(&i) {
            continue;
        }
        let at = 8 * i as usize;
        let h = |o: usize| u16::from_le_bytes([oam[at + o], oam[at + o + 1]]);
        let (a0, a1) = (h(0), h(2));
        if a0 & 0xFF == 0xA0 || (a0 >> 8) & 3 == 2 || a0 >> 14 == 3 {
            continue;
        }
        let (mut x, mut y) = ((a1 & 0x1FF) as i32, (a0 & 0xFF) as i32);
        if x >= 256 {
            x -= 512;
        }
        if y >= 160 {
            y -= 256;
        }
        let owner = world.pool_owner.0.get(k as usize).copied().unwrap_or(0xFF);
        let Some(delta) = (owner != 0xFF)
            .then(|| world.slots.iter().position(|s| s.e.slot == owner))
            .flatten()
            .and_then(delta_of)
        else {
            continue;
        };
        let (x, y) = (x + delta.x.round() as i32, y + delta.y.round() as i32);
        oam[at..at + 2].copy_from_slice(&((a0 & !0xFF) | (y & 0xFF) as u16).to_le_bytes());
        oam[at + 2..at + 4].copy_from_slice(&((a1 & !0x1FF) | (x & 0x1FF) as u16).to_le_bytes());
    }
}

/// OBJ layer of the GBA (1-D tile mapping, as the game sets it): regular and affine sprites, 16 and 256 colours;
/// per screen pixel the top sprite's colour (earlier OAM entries over later ones) and whether it is
/// semi-transparent.
pub fn draw_objects(oam: &[u8], tiles: &[u8], palette: &[u16]) -> Vec<Option<(u16, bool)>> {
    let mut out = vec![None; 240 * 160];
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

#[cfg(test)]
#[path = "play_test.rs"]
mod tests;
