//! The whole game from power-on (FIDELITY G1): [`Session`] runs the typed menus (`menu::flow` over a
//! [`TypedHost`](crate::menu::typed::TypedHost) drawing into a typed `Screen`, the save in a typed EEPROM image), builds
//! the race's [`Setup`] from the menu state where the menus leave for a race, runs the race through [`Game::frame`],
//! and takes the [`Handover`] back into the menus (the results screens, the pause menu). The typed menu state and the
//! race's `World` are the only state; there is no RAM image. One frame API: keys in ([`Session::frame`]), the screen
//! ([`Session::view`]) and the sound ([`Session::sound`]) out. Code the loop reaches that is not ported stops with
//! `Unported` (see [`Session::frame`]).
//!
//! NOT 1:1 (T1): a session frame is one video frame and runs one `main_frame` (the original runs it back to back,
//! several per video frame while idle); the tick counter is the frame count. The race runs with `Timing::steady()`.
//! NOT 1:1 (R24, G1c): the race start also takes the heap arena, the sound engine and the display memory as a
//! previous menu session left them (a captured template); the choice of the race, the cars and the options are the
//! menu state's.

use nfsgba_sim::{
    Result, Unported,
    state::{CarRecord, MenuState},
};

use crate::{
    Flow, Game, Handover, Next, Timing,
    menu::{boot, flow, typed::TypedHost},
    race_init,
    race_setup::{Display, Setup},
};

/// The VBlanks that run before the race start reads the tick counter as the rand seed (T1).
const SEED_VBLANKS: u32 = 6;

/// Game functions the menu frames call that change nothing the session models: timer 3, the frame's tail
/// (effect sprites, OAM copy, key read, page flip: the session does them), the debug print and the tick reads.
const HARMLESS: [u32; 11] = [
    super::menu::VBLANK_INTR_WAIT,
    0x0816_2228,
    0x0816_223C,
    0x0816_21F0,
    0x0812_B084,
    0x0816_1F38,
    0x0816_102C,
    0x0812_B040,
    0x0814_2090,
    0x0816_0E74,
    0x0815_E9E8,
];

/// Sound and music requests of the menus: the session has no sound for the menus yet (FIDELITY U7).
const SOUND: [u32; 6] = [
    super::menu::flow::CARBON_PLAY_SOUND,
    0x0813_6054, // carbon_play_music
    0x0813_5EA4, // sound restart after a save
    0x0813_5DF8, // sound init
    0x0813_609C, // music_stop
    0x0813_5F38, // snd_stop_all
];

/// What is on the screen: the mode-4 page shown, its palettes (BG 0..256, OBJ 256..512) and the sprites (attr0..2 and
/// the affine word per entry).
pub struct View<'a> {
    pub page: &'a [u8],
    pub palette: Vec<u16>,
    pub oam: Vec<[u16; 4]>,
}

pub struct Session<'a> {
    rom: &'a [u8],
    pub st: MenuState,
    pub host: TypedHost<'a>,
    /// The race start's template: the parts of the machine the menus do not model (see the module docs).
    template: (Setup, Display),
    /// The race being run (or paused).
    pub race: Option<Game>,
    paused: bool,
    held: u16,
    frames: u32,
}

/// A game function the menus called that the session does not port, by address (the first such call of a frame).
fn unported_name(function: u32) -> &'static str {
    match function {
        0x0813_72E4 => "race_menu_palette_setup (0x081372E4)",
        _ => "a menu call to a game function that is not ported (Session::host.calls)",
    }
}

impl<'a> Session<'a> {
    /// The game at power-on with the cartridge's save (`eeprom`, empty: none). `template` is a captured race start
    /// (`race_setup::load_pre`).
    pub fn new(rom: &'a [u8], eeprom: Vec<u8>, template: (Setup, Display)) -> Session<'a> {
        let mut st = MenuState::default();
        boot::init(&mut st, rom, &eeprom);
        let mut host = TypedHost::new(rom, &st);
        host.eeprom = eeprom;
        Session {
            rom,
            st,
            host,
            template,
            race: None,
            paused: false,
            held: 0,
            frames: 0,
        }
    }

    /// One video frame with the keys held (GBA `KEYINPUT` layout, a set bit = held). Stops with `Unported(what)` at
    /// the first game code it reaches that is not ported: the menus' calls listed in `host.calls` (sound and
    /// music requests are dropped, U7), a race that leaves the paths `Game::frame` has.
    pub fn frame(&mut self, keys: u16) -> Result<()> {
        let edge = keys & !self.held;
        self.held = keys;
        self.frames += 1;
        if let (Some(game), false) = (&mut self.race, self.paused) {
            return match game.frame(keys, &Timing::steady())? {
                Flow::Racing => Ok(()),
                Flow::Handover(h) => self.handover(h, keys),
            };
        }
        self.menu_frame(edge, keys)
    }

    fn menu_frame(&mut self, edge: u16, held: u16) -> Result<()> {
        let st = &mut self.st;
        (st.g.keys, st.g.keys_held, st.g.ticks) = (edge, held, self.frames as i32);
        self.host.language = st.g.language;
        self.host.screen.dispcnt = self.host.screen.dispcnt & !0x10 | ((st.g.frame_counter as u16 & 1) << 4);
        flow::main_frame(st, &mut self.host);
        let calls = std::mem::take(&mut self.host.calls);
        if let Some(f) = calls
            .iter()
            .map(|c| c.0)
            .find(|f| !HARMLESS.contains(f) && !SOUND.contains(f) && !self.race_calls(*f))
        {
            self.host.calls = calls;
            return Err(Unported(unported_name(f)));
        }
        match (self.st.g.game_state, self.paused) {
            (4, false) => self.start_race(),
            (5, true) => self.resume(),
            _ => Ok(()),
        }
    }

    /// The calls the race start and the pause resume make (`game_state_step` states 4 and 5): the session runs them.
    fn race_calls(&self, function: u32) -> bool {
        matches!(
            function,
            0x0813_9E34 | 0x0813_A514 | 0x0813_A954 | 0x0813_72E4 | 0x0813_9E10 | 0x0814_3010
        )
    }

    /// `game_state_step` state 4: the race start from the menus' choice.
    fn start_race(&mut self) -> Result<()> {
        let (mut setup, display) = self.template.clone();
        apply_choice(&mut setup, &self.st);
        let mut game = race_init::start(self.rom.to_vec(), &setup, display, SEED_VBLANKS)?;
        game.ranked = self.st.g.ranked.clone();
        self.race = Some(game);
        Ok(())
    }

    /// The race hands over: the results (`goto_screen(0xB)` or `menu_back`) or the pause menu (`goto_screen(5)`).
    fn handover(&mut self, h: Handover, keys: u16) -> Result<()> {
        let (st, host) = (&mut self.st, &mut self.host);
        let game = self.race.as_mut().expect("a race hands over");
        st.g.game_state = 1;
        st.g.menu_exit = 0;
        st.g.race_outcome = game.world.g.phase as u32;
        st.g.fade = 0;
        st.g.screen_entered = 0;
        match &h {
            Handover::Results(r) => {
                (st.g.results, st.g.ranked, st.profile.last_player) =
                    (r.results.clone(), r.ranked.clone(), r.last_player);
                st.g.back_top = st.g.back_top_saved as i8;
                match r.next {
                    Next::Goto(s) => flow::goto_screen(st, host, s),
                    Next::Back => flow::menu_back(st, host),
                }
                self.race = None;
            }
            Handover::Pause => {
                flow::goto_screen(st, host, 5);
                self.paused = true;
                game.finish_frame(keys, &Timing::steady(), &h)?;
            }
        }
        Ok(())
    }

    /// The pause menu left the menus (`game_state` 5): resume the race, or quit (`race_outcome` 5).
    fn resume(&mut self) -> Result<()> {
        self.paused = false;
        if self.st.g.race_outcome == 5 {
            let mut game = self.race.take().expect("a paused race");
            game.snd_stop_all();
            self.st.g.game_state = 1;
            self.st.g.menu_exit = 0;
            self.st.g.screen_entered = 0;
            flow::menu_back(&mut self.st, &mut self.host);
        } else if let Some(game) = &mut self.race {
            game.resume(self.st.g.hud_on != 0, i32::from(self.st.profile.music));
        }
        Ok(())
    }

    /// The 240×160 screen: the race's frame during a race, else the menus'.
    pub fn view(&self) -> View<'_> {
        if let (Some(game), false) = (&self.race, self.paused) {
            let u16s = |b: &[u8]| {
                b.as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| u16::from_le_bytes(*c))
                    .collect::<Vec<_>>()
            };
            let oam = u16s(&game.oam).as_chunks::<4>().0.to_vec();
            return View {
                page: game.screen(),
                palette: u16s(&game.palette),
                oam,
            };
        }
        let s = &self.host.screen;
        View {
            page: &s.pages[(s.dispcnt >> 4 & 1) as usize],
            palette: s.palette.to_vec(),
            oam: s.oam.to_vec(),
        }
    }

    /// The samples the last frame played (signed 8-bit, 10,512 Hz): the race's; the menus have no sound yet (U7).
    pub fn sound(&self) -> &[u8] {
        match (&self.race, self.paused) {
            (Some(game), false) => &game.sound,
            _ => &[],
        }
    }
}

/// The race the menus chose, in the race start's input: the globals the menus and the race share (same IWRAM
/// words), the cars and paints, the car records and the options.
pub fn apply_choice(s: &mut Setup, st: &MenuState) {
    let (m, p) = (&st.g, &st.profile);
    let g = &mut s.g;
    g.level = m.environment as i32;
    g.career = m.career as i32;
    g.u_5388 = m.route;
    g.route_index = m.route_flag;
    g.mode = m.race_mode;
    g.laps = m.laps as i32;
    g.opponents = m.opponents;
    g.difficulty = m.difficulty;
    g.u_5604 = m.traffic;
    g.player = m.race_player;
    g.rand = m.rand_index;
    g.automatic = m.u_5798 as i32;
    g.volume = m.sound_volume;
    g.link = m.timing_mode as i32;
    s.cars[0] = m.race_car as i8;
    s.cars[1] = m.race_car_b as i8;
    s.paints = m.paints;
    s.wingman = p.wingman;
    s.music = m.music_id as i32;
    s.hud.units = m.units;
    s.hud.language = m.language;
    s.hud.enabled = m.hud_on;
    for (rec, b) in s.records.iter_mut().zip(&p.car_records) {
        *rec = CarRecord {
            spoiler: b[0],
            u_01: b[1],
            rim: b[2],
            exhaust: b[3],
            u_04: b[4],
            paint: b[5],
            glass: b[6],
            upgrades: b[7..17].try_into().unwrap(),
        };
    }
}
