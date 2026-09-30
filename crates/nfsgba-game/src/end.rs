//! The race's end, its exit to the menus and the pause (G1c): `race_frame_update`'s tail (phases 6-8 and 3),
//! `game_state_step`'s state-5 exit (`snd_stop_all`, `fill_results`, `race_cleanup`) and the START block. Each ends
//! in a [`Handover`]: the menus take over there (`goto_screen`/`menu_back`), and the frame's tail
//! ([`Game::finish_frame`]) runs after them. `docs/engine/game-loop.md`.

use nfsgba_audio::Rom;
use nfsgba_formats::hud;
use nfsgba_sim::{carworld::CarWorld, state::RaceResults};

use crate::{Game, Timing};

/// What one game frame ended in.
#[derive(Debug, Clone, PartialEq)]
pub enum Flow {
    /// The race goes on; the frame is complete.
    Racing,
    /// The menus take over; the frame is not complete ([`Game::finish_frame`]).
    Handover(Handover),
}

/// Where the race hands over to the menus.
#[derive(Debug, Clone, PartialEq)]
pub enum Handover {
    /// START: the race is paused (`0x03005398`, game state 1, HUD hidden, both pages cleared); the menus run
    /// `goto_screen(5)`.
    Pause,
    /// The race is over; the menus run [`Results::next`].
    Results(Box<Results>),
}

/// The race's result, as `game_state_step` leaves it for the menus.
#[derive(Debug, Clone, PartialEq)]
pub struct Results {
    /// The results block (`0x03005650`) `fill_results` filled (untouched in a race that ends in phase 5).
    pub results: RaceResults,
    /// The ranked block (`0x03005730`) after `results_tiebreak`. The caller's menu state holds the block the race
    /// started with (`Game::ranked`); the tie-break has changed its finish times.
    pub ranked: RaceResults,
    /// Profile `+0x32C`: the player's entity index.
    pub last_player: u32,
    pub next: Next,
}

/// The screen change after the race: `goto_screen(0xB)` (the results), or `menu_back` when the race ended in phase 5.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    Goto(i32),
    Back,
}

impl Game {
    /// `race_frame_update`'s countdown to the end (phases 6-8: the timer runs out, time limit, a blocking hit)
    /// and phase 3: the race-over flag and the palette fade.
    pub(crate) fn race_end_phases(&mut self) {
        let g = &mut self.world.g;
        if (6..9).contains(&g.phase) && g.race_over == 0 {
            let left = self.world.hud.race_state_changed as i32;
            if left < g.frame_ticks {
                (g.race_over, g.fade, self.world.hud.race_state_changed) = (1, -0x10, 0);
            } else {
                self.world.hud.race_state_changed = (left - g.frame_ticks) as u32;
            }
        }
        let g = &mut self.world.g;
        if g.phase == 3 && g.race_over == 0 {
            (g.race_over, g.fade) = (1, 0x10);
        }
    }

    /// `fill_results` (`0x0812eaac`): per racer the hunter life, the finish time (estimated for a car still
    /// racing) and the best lap into the results block, then (not in link play) `results_tiebreak`.
    pub fn fill_results(&mut self) {
        let mut r = self.results();
        if self.world.g.phase == 8 {
            r.finish[0] = 0x34BE7;
        }
        let (n, time) = (self.world.g.opponents, self.world.g.time);
        if n != u32::MAX {
            for i in 0..(n as usize + 1).min(4) {
                r.life[i] = self.world.slots[i].c.hunter_life as u32;
                if r.finish[i] == 0 {
                    let (rom, data) = (&self.rom, &self.data);
                    let (t, _) = self.world.with_cars(rom, data, i, |w| finish_time_estimate(w, i, time));
                    r.finish[i] = t;
                }
                let c = &mut self.world.slots[i].c;
                if c.best_lap == 0 {
                    c.best_lap = r.finish[i] as i32;
                }
                r.best_lap[i] = c.best_lap as u32;
            }
        }
        self.set_results(&r);
        if self.world.g.link == 0 {
            self.results_tiebreak();
        }
    }

    /// `results_tiebreak` (`0x0812e9e8`) on the ranked block's finish times (`0x03005750`): while two racers share
    /// one, the one further behind (driver `+0xAC`) gets one more.
    pub fn results_tiebreak(&mut self) {
        let n = self.world.g.opponents as usize;
        let progress: Vec<i32> = self.world.slots.iter().map(|s| s.c.progress).collect();
        let f = &mut self.ranked.finish;
        loop {
            let mut again = false;
            let mut i = 0;
            while n != 0 && i < n {
                for j in i + 1..=n {
                    if f[i] == f[j] {
                        if progress[j] < progress[i] {
                            f[j] += 1;
                        } else {
                            f[i] += 1;
                        }
                        again = true;
                    }
                }
                i += 1;
            }
            if !again {
                break;
            }
        }
    }

    /// The results block `0x03005650..0x03005690` as the menus type it.
    pub fn results(&self) -> RaceResults {
        let g = &self.world.g;
        let mut b = [0u8; 0x40];
        b[..32].copy_from_slice(&g.results);
        b[32..].copy_from_slice(&g.results_b);
        RaceResults::from_bytes(&b)
    }

    fn set_results(&mut self, r: &RaceResults) {
        let b = r.to_bytes();
        let g = &mut self.world.g;
        g.results.copy_from_slice(&b[..32]);
        g.results_b.copy_from_slice(&b[32..]);
    }

    /// `race_cleanup` (`0x081396c4`): the race's blocks are freed (the typed state goes with the `Game`, dropped by
    /// whoever takes over), the HUD's sprites (shadow OAM 0..0x37) are hidden. NOT 1:1 (G1): the effect-sprite list
    /// is freed too, but the frame's tail still draws it from the freed block; kept as it is.
    fn race_cleanup(&mut self) {
        for e in &mut self.world.hud.oam[..0x37] {
            e[0] = e[0] & 0xFC00 | 0x00A0;
        }
    }

    /// `game_state_step`'s state-5 exit, the race returned 0: the sounds stop, the results are filled and the race
    /// cleaned up; the menus run `next` (`goto_screen(0xB)`, or `menu_back` after phase 5).
    pub(crate) fn exit_race(&mut self, t: &Timing) -> nfsgba_sim::Result<Handover> {
        self.irqs_to(t.exit.unwrap_or(self.irqs));
        self.snd_stop_all();
        let phase = self.world.g.phase;
        if phase != 5 {
            self.fill_results();
        }
        self.race_cleanup();
        self.irqs_to(t.handover.unwrap_or(self.irqs));
        Ok(Handover::Results(Box::new(Results {
            results: self.results(),
            ranked: self.ranked.clone(),
            last_player: self.world.g.player,
            next: if phase == 5 { Next::Back } else { Next::Goto(0xB) },
        })))
    }

    /// `race_frame_update`'s START block: the music and sounds stop, the VBlank wait, the VCount IRQ off, the
    /// sky gradient and both pages cleared, game state 1 and the pause flag, the HUD hidden; the menus run
    /// `goto_screen(5)`.
    pub(crate) fn pause(&mut self, t: &Timing) -> nfsgba_sim::Result<Handover> {
        self.irqs_to(t.pause.unwrap_or(self.irqs));
        self.music_stop();
        self.snd_stop_all();
        // carbon_play_music(0): the music id is -1 now, so module 0 is requested.
        self.world.audio.carbon_play_music(Rom(&self.rom), 0);
        self.world.lp.music_id = 0;
        // vblank_intr_wait: one more VBlank IRQ.
        match t.handover {
            Some(n) => self.irqs_to(n.max(self.irqs + 1)),
            None => self.irqs_to(self.irqs + 1),
        }
        self.vcount_irq = false;
        self.world.gradient.fill(0);
        let w = &self.world;
        let bytes = (w.screen.size[0] as usize) * (w.screen.size[1] as usize);
        for p in w.screen.pages {
            let at = (p & 0x1_FFFF) as usize;
            self.vram[at..at + bytes].fill(0);
        }
        let w = &mut self.world;
        (w.lp.game_state, w.lp.paused) = (1, 1);
        let mut h = w.hud_frame();
        let count = self.bank.screens[w.hud.screen as usize].count;
        hud::toggle(&self.rom, &h.g, false, &mut h.objects, count, &mut h.messages);
        self.sprite_update(&mut h);
        self.world.set_hud_frame(h);
        Ok(Handover::Pause)
    }

    /// The pause menu's resume (`goto_screen(0x82)`): the BG palette black, game state 5 and the pause flag off, the
    /// palette fade in (16), the race palettes (`race_menu_palette_setup`), the HUD back when the option is on, the
    /// race music (`music`, the profile's + 1) and with a route the light tint. NOT 1:1 (G1): the 15 VBlanks the
    /// game waits and the engine loop's restart are not run.
    pub fn resume(&mut self, hud_on: bool, music: i32) {
        self.palette[..0x200].fill(0);
        let w = &mut self.world;
        (w.lp.game_state, w.lp.paused, w.g.fade) = (5, 0, 0x10);
        self.race_menu_palette_setup();
        if hud_on {
            let w = &mut self.world;
            let mut h = w.hud_frame();
            let count = self.bank.screens[w.hud.screen as usize].count;
            hud::toggle(&self.rom, &h.g, true, &mut h.objects, count, &mut h.messages);
            self.sprite_update(&mut h);
            self.world.set_hud_frame(h);
        }
        self.world.audio.carbon_play_music(Rom(&self.rom), (music + 1) as u32);
        self.world.lp.music_id = music + 1;
        if self.world.g.u_5388 != 0 {
            self.world.palette_fade = self.world.palette_base.clone();
            self.tint();
        }
    }

    /// `race_menu_palette_setup` (`0x081372e4`, the resume from the pause menu): both pages cleared, both palette buffers are the level's with the racers' car palettes, and outside the menus' preview (phase 5) the VCount
    /// IRQ comes back on (`0x0813a4c0`: the sky gradient restarts). The fade's target gets the same palette.
    pub fn race_menu_palette_setup(&mut self) {
        let w = &self.world;
        let bytes = (w.screen.size[0] as usize) * (w.screen.size[1] as usize);
        for p in w.screen.pages {
            let at = (p & 0x1_FFFF) as usize;
            self.vram[at..at + bytes].fill(0);
        }
        let d = (self.world.lp.descriptor & 0xFF_FFFF) as usize;
        let u32_at = |o: usize| u32::from_le_bytes(self.rom[o..o + 4].try_into().unwrap()) as usize;
        let u16_at = |o: usize| u16::from_le_bytes(self.rom[o..o + 2].try_into().unwrap()) as usize;
        let at = (u32_at(d) & 0xFF_FFFF) + 2 * u16_at(d + 0x5A);
        let palette: Vec<u16> = (0..256).map(|i| u16_at(at + 2 * i) as u16).collect();
        self.world.palette_fade = palette.clone();
        self.world.palette_base = palette;
        let mut base = self.world.palette_base.clone();
        crate::race_init::load_car_palettes(&self.rom, &mut base, &self.world);
        self.world.palette_fade = base.clone();
        self.world.palette_base = base;
        if self.world.g.phase != 5 {
            self.vcount_irq_on();
        }
    }

    /// `FUN_0813a4c0`: the sky gradient restarts and DISPSTAT's VCount IRQ is on.
    pub fn vcount_irq_on(&mut self) {
        self.world.gradient_start = 0;
        self.vcount_irq = true;
    }

    /// The rest of the frame a [`Handover`] cut short, after the menus ran their screen change: the game state
    /// (1, and after the results the menu music request `carbon_play_music(0)`), then the frame's tail.
    pub fn finish_frame(&mut self, keys: u16, t: &Timing, h: &Handover) -> nfsgba_sim::Result<()> {
        if matches!(h, Handover::Results(_)) {
            self.world.lp.game_state = 1;
            if self.world.lp.music_id != 0 {
                self.world.audio.carbon_play_music(Rom(&self.rom), 0);
                self.world.lp.music_id = 0;
            }
        }
        self.frame_tail(keys, t)
    }
}

/// `finish_time_estimate` (`0x0814f050`) for racer `i` at race time `elapsed`; a sprint's set laps come back to
/// the globals.
fn finish_time_estimate(w: &mut CarWorld, i: usize, elapsed: u32) -> u32 {
    let mut race = w.race();
    let mut r = w.slots[i].racer();
    let t = w.route.line.finish_estimate(&mut race, &mut r, elapsed);
    w.slots[i].set_racer(&r);
    w.set_race(&race);
    w.g.laps = race.laps;
    t
}
