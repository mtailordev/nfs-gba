//! The race game loop of Need for Speed Carbon: Own the City (GBA): one game frame in the game's exact order,
//! composed from the exact subsystems (`nfsgba-sim`, `render`, `sky`, `paint`, `hud`, `ui`, `nfsgba-audio`).
//!
//! The game has no frame pacing: `main_frame` (`0x0812ae64`) runs back to back, one game frame taking about four
//! video frames in a race. The VBlank IRQ (`vblank_irq` `0x0812ac14`) mixes the sound and counts the race time
//! while it runs, and timer 3 measures its length (the frame time the physics integrates). So besides the keys, a
//! frame's inputs are its timing ([`Timing`]); a replay takes them from a trace, live play from a model.
//! `docs/engine/game-loop.md` has the frame order, the RAM this crate owns and the verification.

// Fixed-point expressions are written the way the game's C writes them (see nfsgba-sim).
#![allow(clippy::precedence)]

pub mod camera;
pub mod oam;
pub mod race_init;
pub mod slots;
pub mod trace;
pub mod view;

use std::{fs, io, path::Path};

use nfsgba_audio::{Engine, Rom, ram};
use nfsgba_formats::{
    city, hud, paint, render, sector_light, sky, tint_palette,
    ui::{self, SpriteBank},
};
use nfsgba_sim::{Mem, Sim, Unported, ai, car, sound::Command, traffic_ai};

use view::WORLD;

/// A frame's hardware timing: timer 3's count when `main_frame` reads it (1,024-cycle ticks since the previous
/// frame), and where the VBlank IRQs land. Counts are cumulative from `main_frame`'s entry, at the points of the
/// frame that read what the IRQ changes (the race time, the sound mix).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timing {
    pub timer3: u16,
    /// At `update_entities`' entry.
    pub entities: u32,
    /// At the entry and the return of each sound call of the entity handlers, in order (an IRQ can land inside a
    /// call: `snd_set_sfx_rate` divides before it stores the rate).
    pub sounds: Vec<(u32, u32)>,
    /// At `route_gap`'s entry (inside the player's car step).
    pub gap: Option<u32>,
    /// At each of `route_gap`'s race-time reads: it reads it twice, around a division.
    pub gap_reads: Vec<u32>,
    /// At `hud_update`'s entry, and when `hud_timer` reads the race time.
    pub hud: u32,
    pub timer: Option<u32>,
    /// The whole frame.
    pub end: u32,
    /// When an opponent's AI reads the race time for its lane-change timer (`0x0813C95C`): (driver struct, count).
    pub lanes: Vec<(u32, u32)>,
}

impl Timing {
    /// NOT 1:1 (live play): a steady race frame as measured in the reference race (timer 3 at 1,098 ticks, four
    /// VBlanks, all while the world is drawn). The real timing depends on the CPU time the frame takes.
    pub fn steady() -> Timing {
        Timing {
            timer3: 1098,
            entities: 0,
            sounds: Vec::new(),
            gap: None,
            gap_reads: Vec::new(),
            hud: 4,
            timer: None,
            end: 4,
            lanes: Vec::new(),
        }
    }
}

/// The machine state: ROM, EWRAM and IWRAM ([`Mem`]), palette RAM, VRAM and OAM.
#[derive(Clone)]
pub struct Machine {
    pub mem: Mem,
    pub palette: Vec<u8>,
    pub vram: Vec<u8>,
    pub oam: Vec<u8>,
}

impl Machine {
    /// From a trace state (`trace::STATE` bytes: EWRAM, IWRAM, palette, VRAM, OAM).
    pub fn from_state(rom: Vec<u8>, s: &[u8]) -> Machine {
        let (ew, rest) = s.split_at(0x4_0000);
        let (iw, rest) = rest.split_at(0x8000);
        let (pal, rest) = rest.split_at(0x400);
        let (vram, oam) = rest.split_at(0x1_8000);
        Machine {
            mem: Mem::new(rom, ew.to_vec(), iw.to_vec()),
            palette: pal.to_vec(),
            vram: vram.to_vec(),
            oam: oam.to_vec(),
        }
    }

    /// From an mGBA dump (`PREFIX.wram.bin`, `.iwram.bin`, `.palette.bin`, `.vram.bin`, `.oam.bin`).
    pub fn load_dump(rom: Vec<u8>, prefix: &Path) -> io::Result<Machine> {
        let read = |d: &str| fs::read(format!("{}.{d}.bin", prefix.display()));
        let s = [
            read("wram")?,
            read("iwram")?,
            read("palette")?,
            read("vram")?,
            read("oam")?,
        ]
        .concat();
        Ok(Machine::from_state(rom, &s))
    }

    pub fn state(&self) -> Vec<u8> {
        [
            &self.mem.ewram[..],
            &self.mem.iwram,
            &self.palette,
            &self.vram,
            &self.oam,
        ]
        .concat()
    }
}

/// Points of the race frame where a caller may look at or override the state (`Game::frame_with`); returning
/// `true` at `Camera` or `Slots` replaces that code with the caller's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Checkpoint {
    /// After `update_entities`.
    Entities,
    /// Instead of the camera update (`camera_dispatch` → `camera_update`), before the visible list.
    Camera,
    /// Instead of the matrix slots (`build_player_matrices`, `assign_entity_slot`, the effect sprites).
    Slots,
}

pub struct Game {
    pub rom: Vec<u8>,
    pub sim: Sim,
    pub palette: Vec<u8>,
    pub vram: Vec<u8>,
    pub oam: Vec<u8>,
    /// DISPCNT's frame select: the mode-4 page on screen (0: `0x06000000`, 1: `0x0600A000`).
    pub display_page: u8,
    pub audio: Engine,
    /// Samples the sound hardware played during the last frame (signed 8-bit, 176 per VBlank, 10,512 Hz).
    pub sound: Vec<u8>,
    pub bank: SpriteBank,
    /// VBlank IRQs run so far in the current frame.
    irqs: u32,
}

const FRAME_COUNT: u32 = 0x0300_5628;
const GAME_STATE: u32 = 0x0300_5808;
const PHASE: u32 = 0x0300_0048;
const FADE: u32 = 0x0300_5630;
const RACE_OVER: u32 = 0x0300_5780;
const LINK: u32 = 0x0300_5624;
const PROFILE: u32 = 0x0300_56EC;
const PLAYER: u32 = 0x0300_0060;
const HELD: u32 = 0x0300_64C4;
const PRESSED: u32 = 0x0300_64C0;
const SFX_OPTION: u32 = 0x0300_53A4;
const AUDIO_GLOBALS: u32 = 0x0300_6370;
const WORK_AREA: usize = 0x160C;

fn is(r: bool, what: &'static str) -> nfsgba_sim::Result<()> {
    if r { Err(Unported(what)) } else { Ok(()) }
}

impl Game {
    pub fn new(m: Machine) -> Game {
        let rom = m.mem.rom.clone();
        let audio = {
            let mm = &m.mem;
            let eng = mm.u32(AUDIO_GLOBALS);
            let buffer = |k: u32| mm.bytes(0x0300_5DEC + 0xB0 * k, 0xB0);
            ram::load(
                mm.bytes(eng, WORK_AREA),
                mm.bytes(AUDIO_GLOBALS, 16),
                [buffer(0), buffer(1)],
            )
        };
        let descriptor = m.mem.u32(0x0300_5620) as usize - 0x0800_0000;
        Game {
            bank: ui::sprite_bank(&rom, descriptor),
            rom,
            sim: Sim::new(m.mem),
            palette: m.palette,
            vram: m.vram,
            oam: m.oam,
            display_page: 0,
            audio,
            sound: Vec::new(),
            irqs: 0,
        }
    }

    pub fn machine(&self) -> Machine {
        Machine {
            mem: self.sim.mem.clone(),
            palette: self.palette.clone(),
            vram: self.vram.clone(),
            oam: self.oam.clone(),
        }
    }

    pub fn mem(&self) -> &Mem {
        &self.sim.mem
    }

    /// One game frame with the keys held when it ends (`KEYINPUT` as `main_frame` samples it; bit set = held).
    pub fn frame(&mut self, keys: u16, t: &Timing) -> nfsgba_sim::Result<()> {
        self.frame_with(keys, t, &mut |_, _| false)
    }

    /// `main_frame` (`0x0812ae64`) in a race, with `assist` called at each [`Checkpoint`].
    pub fn frame_with(
        &mut self,
        keys: u16,
        t: &Timing,
        assist: &mut dyn FnMut(Checkpoint, &mut Game) -> bool,
    ) -> nfsgba_sim::Result<()> {
        self.sound.clear();
        // Timer 3 (FUN_08162228 stops it, FUN_0816223c reads it): the frame time for the physics.
        let m = &mut self.sim.mem;
        m.set_u32(0x0300_5934, t.timer3 as u32);
        if m.u32(LINK) == 2 {
            m.set_u32(0x0300_5640, 15);
        } else {
            if t.timer3 == 0 {
                m.set_u32(0x0300_5934, 0x200);
            }
            let ft = nfsgba_sim::math::div(25_500, m.i32(0x0300_5934));
            m.set_i32(0x0300_5640, ft.clamp(10, 100));
        }
        self.irqs = 0;
        self.irqs_to(t.entities);
        self.flip_page();
        let m = &self.sim.mem;
        is(
            m.u32(GAME_STATE) != 5,
            "game states other than the race (state machine FUN_0812acec)",
        )?;
        self.race_frame(t, assist)?;
        oam::draw_effect_sprites(&self.rom, &mut self.sim.mem, 0x0300_0058);
        // FUN_0816102c: the shadow OAM to OAM.
        self.oam.copy_from_slice(self.sim.mem.bytes(oam::SHADOW_OAM, 0x400));
        let m = &mut self.sim.mem;
        m.set_u32(FRAME_COUNT, m.u32(FRAME_COUNT).wrapping_add(1));
        if m.u32(GAME_STATE) == 5 {
            self.tint();
        }
        is(
            self.sim.mem.i32(FADE) != 0,
            "palette fades (main_frame, counter 0x03005630)",
        )?;
        self.read_keys(keys)?;
        self.irqs_to(t.end);
        self.store_audio();
        Ok(())
    }

    /// `FUN_0812b084`: shows the page drawn last frame and draws into the other (view `+0x00`).
    fn flip_page(&mut self) {
        let m = &mut self.sim.mem;
        let fb = 0x0300_6410;
        let draw = if m.u32(FRAME_COUNT) & 1 == 0 {
            self.display_page = 0;
            if m.u8(fb + 8) == 0x0F {
                m.u32(fb + 0xC)
            } else {
                m.u32(fb + 0x10)
            }
        } else {
            self.display_page = 1;
            m.u32(fb + 0xC)
        };
        m.set_u32(0x0300_0080, draw);
    }

    /// `FUN_0812b040`: `KEYINPUT` into the held and newly pressed words, and the player's control word.
    fn read_keys(&mut self, keys: u16) -> nfsgba_sim::Result<()> {
        let m = &mut self.sim.mem;
        let raw = 0x3FF & !keys;
        let held = !raw;
        m.set_u16(PRESSED, held & !m.u16(HELD));
        m.set_u16(HELD, held);
        is(m.u32(LINK) == 2, "link play (FUN_0814737c, FUN_081469d8)")?;
        let player = m.u32(PLAYER);
        m.set_u16(0x0300_57D8 + 2 * player, held);
        Ok(())
    }

    /// The VBlank IRQs up to the frame's `target`-th.
    fn irqs_to(&mut self, target: u32) {
        while self.irqs < target {
            self.vblank();
            self.irqs += 1;
        }
    }

    /// Whether a VBlank IRQ now would count the race time: racing and not paused.
    fn race_time_runs(&self) -> bool {
        self.sim.mem.u32(0x0300_5398) == 0 && self.sim.mem.u32(PHASE) == 2
    }

    /// A VBlank IRQ (`vblank_irq`): the sound DMA, mix and buffer swap, the counters (the race time while
    /// racing and not paused) and the sky gradient's start for the next display frame.
    fn vblank(&mut self) {
        {
            self.sound.extend(self.audio.vblank(Rom(&self.rom)));
            let m = &mut self.sim.mem;
            let inc = |m: &mut Mem, a: u32| m.set_u32(a, m.u32(a).wrapping_add(1));
            inc(m, 0x0300_53B4);
            if self.race_time_runs() {
                let m = &mut self.sim.mem;
                inc(m, nfsgba_sim::route::RACE_TIME);
            }
            let m = &mut self.sim.mem;
            inc(m, 0x0300_0044);
            inc(m, 0x0300_5724);
            let desc = m.u32(0x0300_5620);
            if desc != 0 {
                let k = (m.i16(desc + 100) as i32 - (m.i32(0x0300_56B8) >> 1) - ((m.i16(0x0300_5392) as i32 >> 1) + 4))
                    .max(0);
                m.set_u32(0x0300_56E8, m.u32(0x0300_53B8).wrapping_add(2 * k as u32));
            }
            m.set_u16(0x0300_7FF8, m.u16(0x0300_7FF8) | 1);
            m.set_u32(0x0300_5728, m.u32(0x0300_5728) | 1);
        }
    }

    fn store_audio(&mut self) {
        let m = &mut self.sim.mem;
        let eng_at = m.u32(AUDIO_GLOBALS);
        let (mut eng, mut globals) = (m.bytes(eng_at, WORK_AREA).to_vec(), m.bytes(AUDIO_GLOBALS, 16).to_vec());
        ram::store(&self.audio, &mut eng, &mut globals);
        m.set_bytes(eng_at, &eng);
        m.set_bytes(AUDIO_GLOBALS, &globals);
        for k in 0..2 {
            let at = self.audio.buffer_addr[k];
            m.set_bytes(at, &self.audio.buffers[k]);
        }
    }

    fn sfx_option(&self) -> u32 {
        self.sim.mem.u32(SFX_OPTION)
    }

    /// The sound commands the entity handlers issued, in order (`nfsgba_sim::sound`), each after the VBlank IRQs
    /// that came before its effect in the frame (`t.sounds`; the sim records the commands instead of running
    /// them): a rate change takes effect when the call returns (after its division), a stop at its entry (it
    /// zeroes the volume first). A play that an IRQ interrupted half-way is not modelled.
    fn play_commands(&mut self, t: &Timing, done: &mut usize) -> nfsgba_sim::Result<()> {
        for c in std::mem::take(&mut self.sim.sounds) {
            if let Some(&(entry, ret)) = t.sounds.get(*done) {
                match c {
                    Command::Pitch(..) => self.irqs_to(ret),
                    Command::Stop(_) => self.irqs_to(entry),
                    _ => {
                        is(entry != ret, "an IRQ inside snd_play_sfx (a half-set voice)")?;
                        self.irqs_to(entry)
                    }
                }
            }
            *done += 1;
            let rom = Rom(&self.rom);
            match c {
                Command::Play(id) => {
                    self.audio.carbon_play_sound(rom, id, self.sim.mem.u32(SFX_OPTION));
                }
                Command::Stop(id) => self.audio.carbon_stop_sound(rom, id),
                Command::Pitch(id, rate8) => self.audio.carbon_set_sound_rate(rom, id, rate8),
                Command::Start {
                    sample,
                    pitch,
                    channel,
                    volume,
                } => {
                    self.audio.play_sfx(rom, sample, pitch, channel as i32, volume as i32);
                }
            }
        }
        Ok(())
    }

    /// Runs `step` with the race time the game read `at` IRQs into the frame (the IRQs themselves run later, where
    /// the frame's other timing points put them): the ticks between now and then are lent to it.
    fn lend<R>(&mut self, at: Option<u32>, step: impl FnOnce(&mut Sim) -> R) -> R {
        let lent = match at {
            Some(n) if self.race_time_runs() => n.saturating_sub(self.irqs),
            _ => 0,
        };
        let rt = nfsgba_sim::route::RACE_TIME;
        let m = &mut self.sim.mem;
        m.set_u32(rt, m.u32(rt).wrapping_add(lent));
        let r = step(&mut self.sim);
        let m = &mut self.sim.mem;
        m.set_u32(rt, m.u32(rt).wrapping_sub(lent));
        r
    }

    fn entity(&self, i: u32) -> u32 {
        self.sim.mem.u32(WORLD + 0x3C) + 0xA4 * i
    }

    /// `race_frame_update` (`0x0813a954`) while racing.
    fn race_frame(
        &mut self,
        t: &Timing,
        assist: &mut dyn FnMut(Checkpoint, &mut Game) -> bool,
    ) -> nfsgba_sim::Result<()> {
        let m = &mut self.sim.mem;
        m.set_u32(0x0300_56F0, m.u32(0x0300_56F0).wrapping_add(1));
        // FUN_081360dc(1) / FUN_08139e10: restart the engine loop when its slot fell silent.
        if !self.audio.sfx_playing(1) {
            let id = self.sim.mem.i8(self.sim.mem.u32(PROFILE) + 0x2EF) as i32 as u32;
            let opt = self.sfx_option();
            self.audio.carbon_play_sound(Rom(&self.rom), id, opt);
        }
        self.shade_car_paint();
        let m = &mut self.sim.mem;
        m.set_u32(0x0300_5394, 0); // FUN_0814f8a0: the matrix slot counter
        let profile = m.u32(PROFILE);
        m.set_u32(profile + 0x2E0, 0);
        m.set_u32(profile + 0x2E4, 0);
        m.set_u16(WORLD + 0xF6, 0);
        self.update_entities(t)?;
        assist(Checkpoint::Entities, self);
        if !assist(Checkpoint::Camera, self) {
            camera::dispatch(&mut self.sim.mem)?;
        }
        is(
            !matches!(self.sim.mem.u32(camera::VIEW_MODE), 0 | 2),
            "camera views other than the bumper and chase views",
        )?;
        let mut vis = view::visible(&self.rom, &self.sim.mem);
        if vis.sky {
            self.sim.mem.set_u16(WORLD + 0xF6, 1);
        }
        if !assist(Checkpoint::Slots, self) {
            slots::race_slots(&mut self.sim.mem)?;
        }
        let page = (self.sim.mem.u32(0x0300_0080) - 0x0600_0000) as usize;
        if self.sim.mem.u16(WORLD + 0xF6) != 0 {
            let m = &self.sim.mem;
            let desc = sky::sky_desc(&self.rom, m.u32(0x0300_006C) as usize);
            let clip = [m.i32(0x0300_53D4), m.i32(0x0300_53DC)];
            sky::draw_skyline(
                &self.rom,
                &desc,
                &view::sky_camera(m),
                clip,
                &mut self.vram[page..page + 240 * 160],
            );
        }
        self.draw_world(&mut vis, page);
        let m = &self.sim.mem;
        let (phase, fade) = (m.u32(PHASE), m.i32(FADE));
        is(
            (phase == 1 || phase == 9) && fade == 0,
            "the race countdown (race_start_from_table_b)",
        )?;
        is(
            m.u32(0x0300_5714) == 3,
            "the race start set-up (FUN_0813a054..FUN_0813a108)",
        )?;
        self.irqs_to(t.timer.unwrap_or(t.hud));
        self.hud();
        let m = &self.sim.mem;
        let over = m.u32(RACE_OVER);
        is(phase.wrapping_sub(6) < 3 && over == 0, "the race-end countdown")?;
        is(phase == 3 && over == 0, "the race end")?;
        self.positions();
        // A car-to-car contact this frame (profile +0x2E0, +0x2E4 set by the car steps).
        let m = &self.sim.mem;
        let driver = m.u32(self.entity(m.u32(PLAYER)) + 0x8C);
        if m.u32(profile + 0x2E0) != 0 && m.u32(profile + 0x2E4) != 0 && m.u8(driver + 0x4D1) == 0 {
            let rom = Rom(&self.rom);
            self.audio.carbon_play_sound(rom, 0x20, self.sim.mem.u32(SFX_OPTION));
            self.audio.carbon_set_sound_rate(rom, 0x20, 0x4B0);
        }
        let m = &self.sim.mem;
        is(over != 0 && fade == 0, "the race end")?;
        if phase != 1 {
            let g = view::hud_globals(m);
            let (mut objects, mut messages) = (view::hud_objects(m), view::messages(m));
            if m.u32(0x0300_5384) == 0 {
                hud::message_cancel(&self.rom, &g, &mut objects, &mut messages, 2);
            } else {
                hud::message_show(&self.rom, &g, &mut messages, 2, 0x3C, false);
            }
            view::store_hud_objects(&mut self.sim.mem, &objects);
            view::store_messages(&mut self.sim.mem, &messages);
        }
        let m = &self.sim.mem;
        is(
            over == 0 && m.u16(PRESSED) & 8 != 0 && fade == 0 && m.u32(0x0300_5398) == 0,
            "the pause menu (START)",
        )
    }

    /// `update_entities` (`0x0813765c`): the handler of every entity with state bits 0 and 1 set, from the entity
    /// handler table `0x087F38B8`: the cars (0..3), the opponents and the wingman (0x29), the sparks (0x34) and
    /// traffic (0x36).
    fn update_entities(&mut self, t: &Timing) -> nfsgba_sim::Result<()> {
        let mut sounds = 0;
        for i in 0..view::entity_count(&self.sim.mem) {
            let e = self.entity(i);
            let m = &self.sim.mem;
            if m.u16(e + 8) & 3 != 3 || m.u16(e + 0x78) == 0xFFFF {
                continue; // (sector 0xFFFF: the game prints a debug message)
            }
            match m.u16(e + 0x4E) {
                0..=3 => {
                    if matches!(m.u16(e + 0x4A), 2 | 0x100) {
                        slots::rim_redraw(&mut self.sim.mem, e)?;
                    }
                    // The step reads the race time (route_gap, lap crossing) with the IRQs before route_gap
                    // counted; the sim runs the step whole, so those IRQs' race-time ticks are lent to it and
                    // the IRQs themselves run after it, between the sound commands they fell between.
                    let at = t.gap_reads.first().copied().or(t.gap);
                    self.lend(at, |sim| car::handler(sim, e))?;
                    // route_gap reads the race time again after its division (split = rt₂ − x·rt₁ / y); the sim
                    // reads it once, so an IRQ in between adds its tick afterwards.
                    if let [first, second, ..] = t.gap_reads[..]
                        && self.race_time_runs()
                    {
                        let m = &mut self.sim.mem;
                        m.set_u32(0x0300_615C, m.u32(0x0300_615C).wrapping_add(second - first));
                    }
                    self.play_commands(t, &mut sounds)?;
                }
                0x29 => {
                    // The lane-change timer reads the race time the IRQs have counted by then.
                    let d = m.u32(e + 0x8C);
                    let at = t.lanes.iter().find(|(driver, _)| *driver == d).map(|&(_, n)| n);
                    let effects = self.lend(at, |sim| ai::handler(sim, e))?;
                    self.play_commands(t, &mut sounds)?;
                    if let Some(f) = effects {
                        let m = &mut self.sim.mem;
                        slots::opponent_effects(m, e, f.heading as i32, f.view as i32, f.size);
                    }
                }
                0x34 => slots::effect_handler(&mut self.sim.mem, e),
                0x36 => {
                    traffic_ai::handler(&mut self.sim, e)?;
                    self.play_commands(t, &mut sounds)?;
                }
                _ => return Err(Unported("an entity handler other than 0..3, 0x29, 0x34 and 0x36")),
            }
        }
        Ok(())
    }

    /// `shade_car_paint(world, entities[*0x030057F8], 0, 0)`: the glass colours from the heading.
    fn shade_car_paint(&mut self) {
        let m = &mut self.sim.mem;
        if m.u32(RACE_OVER) != 0 {
            return;
        }
        let ents = m.u32(WORLD + 0x3C);
        let record = m.u32(0x0300_539C) + 0x11 * m.u8(ents + 0x89) as u32;
        let e = ents + 0xA4 * m.u32(0x0300_57F8);
        let shades = paint::glass_shades(&self.rom, m.u8(record + 5), m.i32(e + 0x2C) >> 8);
        let direct = m.i32(FADE) == 0;
        for (k, s) in shades.into_iter().enumerate() {
            let o = 0x180 + 0x20 * k as u32;
            for buffer in [0x0300_577C, 0x0300_55F0] {
                m.set_u16(m.u32(buffer) + o, s);
            }
            if direct {
                let at = 2 * (0xC0 + 0x10 * k);
                self.palette[at..at + 2].copy_from_slice(&s.to_le_bytes());
            }
        }
    }

    /// `apply_sector_light_to_palette` (`0x0813a514`): palette RAM = the base buffer tinted by the light at the
    /// player's position in the camera sector; unchanged when no light is found.
    fn tint(&mut self) {
        let m = &self.sim.mem;
        let e = self.entity(m.u32(PLAYER));
        let sector = m.u32(0x0300_5614) as usize;
        let sectors = city(&self.rom);
        let Some(light) = sector_light(&self.rom, &sectors[sector], m.i32(e + 0x0C) >> 8, m.i32(e + 0x14) >> 8) else {
            return;
        };
        let base = m.u32(0x0300_55F0);
        let raw: Vec<u16> = (0..256).map(|i| m.u16(base + 2 * i)).collect();
        for (i, c) in tint_palette(&raw, light).into_iter().enumerate() {
            if (1..=143).contains(&i) || (149..=255).contains(&i) {
                self.palette[2 * i..2 * i + 2].copy_from_slice(&c.to_le_bytes());
            }
        }
    }

    /// `draw_visible_sectors` (`iwram_call` to `0x030048c8`) into the draw page.
    fn draw_world(&mut self, vis: &mut render::Visibility, page: usize) {
        let m = &self.sim.mem;
        let (frame, rt) = (view::frame(m), view::runtime(&self.rom, m));
        let mut scene = view::scene(m);
        render::draw_world(
            &self.rom,
            &frame,
            &rt,
            &mut scene,
            vis,
            &mut self.vram[page..page + 240 * 160],
        );
        let entities = scene.entities.clone();
        drop(scene);
        view::store_entities(&mut self.sim.mem, &entities);
    }

    /// `hud_update(world + 0xA4)` then `sprite_screen_update(world + 0xA4, 0)`.
    fn hud(&mut self) {
        let m = &self.sim.mem;
        let mut g = view::hud_globals(m);
        let racers = view::hud_racers(m);
        let (mut objects, mut messages) = (view::hud_objects(m), view::messages(m));
        let mut obj_palette: Vec<u16> = (0..256)
            .map(|i| u16::from_le_bytes([self.palette[0x200 + 2 * i], self.palette[0x201 + 2 * i]]))
            .collect();
        let minimap = hud::update(
            &self.rom,
            &mut g,
            &racers,
            &mut objects,
            &mut messages,
            &mut obj_palette,
        );
        let (screen, tile_base) = (m.u16(view::HUD_SCREEN + 0x18) as usize, m.u16(view::TILE_BASE));
        if let Some(tiles) = minimap {
            let e = self.bank.elements[self.bank.screens[screen].first + 38];
            let at = 0x1_0000 + 32 * e.tile.wrapping_add(tile_base) as usize;
            self.vram[at..at + tiles.len()].copy_from_slice(&tiles);
        }
        let mut shadow = view::shadow_oam(m);
        for u in ui::update_sprites(
            &self.rom,
            &self.bank,
            screen,
            &mut objects,
            &mut shadow,
            false,
            tile_base,
        ) {
            let at = 0x1_0000 + 32 * u.tile;
            self.vram[at..at + u.len].copy_from_slice(&self.rom[u.src..u.src + u.len]);
        }
        for (i, c) in obj_palette.into_iter().enumerate() {
            self.palette[0x200 + 2 * i..0x202 + 2 * i].copy_from_slice(&c.to_le_bytes());
        }
        // The HUD's digits divide through the IWRAM routine (0x03000220), which stores its remainder at
        // 0x03006480; the frame's last such division is the speed's ones digit (`hud_speed`).
        if g.hud != 0 && (0..=3).contains(&g.mode) {
            let speed = racers[g.player].driver.map_or(0, |d| d.speed);
            let mut v = nfsgba_formats::div(speed, 0x163C);
            if g.units == 0 {
                v = hud::divmod(v << 8, 0x19B).0;
            }
            let rest = hud::divmod(v, 100).1;
            self.sim.mem.set_i32(0x0300_6480, hud::divmod(rest, 10).1);
        }
        let m = &mut self.sim.mem;
        view::store_hud_globals(m, &g);
        view::store_hud_objects(m, &objects);
        view::store_messages(m, &messages);
        view::store_shadow_oam(m, &shadow);
    }

    /// `FUN_0813ea04`: race positions (driver `+0xA8`), 1-based, from the race progress.
    fn positions(&mut self) {
        let m = &self.sim.mem;
        let n = m.u32(0x0300_5784) + 1;
        let player = m.u32(PLAYER);
        let driver = |m: &Mem, e: u32| m.u32(e + 0x8C);
        if m.u32(PHASE) == 9 {
            for i in 0..n {
                let d = driver(&self.sim.mem, self.entity(player + i));
                self.sim.mem.set_u32(d + 0xA8, i + 1);
            }
            return;
        }
        let lapped = m.u32(0x0300_608C) != 0;
        let progress = |m: &Mem, e: u32| {
            let d = driver(m, e);
            if !lapped {
                return m.i32(d + 0xAC);
            }
            let list = m.u32(WORLD + 0x40);
            let last = m.u32(WORLD + 0x44)
                + (m.i32(list + 4) as u32).wrapping_mul(0x18)
                + (m.u16(list) as u32).wrapping_mul(0x18)
                - 8;
            (m.i32(0x0300_56E4).wrapping_sub(m.i8(d + 0xC5) as i32))
                .wrapping_mul(m.i32(last))
                .wrapping_add(m.i32(d + 0xAC))
        };
        for i in 0..n {
            let e = self.entity(player + i);
            let m = &self.sim.mem;
            if m.u16(e + 0x4A) == 2 {
                continue;
            }
            let mut position = 1u32;
            for j in 0..n {
                let o = self.entity(j);
                if m.u16(driver(m, o) + 0x4D8) & 8 != 0 || e == o {
                    continue;
                }
                let (pe, po) = (progress(m, e), progress(m, o));
                if pe.wrapping_sub(po) < 0 || (pe == po && j < i) || m.u16(o + 0x4A) == 2 {
                    position += 1;
                }
            }
            let d = driver(m, e);
            self.sim.mem.set_u32(d + 0xA8, position);
        }
    }

    /// The backdrop colour of each screen line (palette entry 0 as the VCount IRQ sets it every 2 lines from
    /// the gradient buffer `*0x030053B8`, starting at the entry the last VBlank chose).
    pub fn backdrop(&self) -> [u16; 160] {
        let m = &self.sim.mem;
        let (ptr, base) = (m.u32(0x0300_56E8), m.u32(0x0300_53B8));
        let start = (ptr.wrapping_sub(base) / 2) as usize;
        std::array::from_fn(|y| m.u16(base + 2 * sky::backdrop_entry(start, y) as u32))
    }

    /// The 240×160 mode-4 page on screen (palette indices; 0 shows the backdrop).
    pub fn screen(&self) -> &[u8] {
        let at = if self.display_page == 0 { 0 } else { 0xA000 };
        &self.vram[at..at + 240 * 160]
    }
}
