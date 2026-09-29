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
pub mod menu;
pub mod oam;
pub mod race_init;
pub mod slots;
pub mod trace;
pub mod view;
pub mod world;

use std::{io, path::Path, sync::Arc};

use nfsgba_audio::Rom;
use nfsgba_formats::{
    city, hud, paint, render, sector_light, sky, tint_palette,
    ui::{self, SpriteBank},
};
use nfsgba_sim::{
    Mem, Unported,
    carworld::{CarWorld, Slot},
    data::GameData,
    sound::Command,
};

use world::World;

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
    /// When an opponent's AI reads the race time for its lane-change timer (`0x0813C95C`): (entity, count).
    pub lanes: Vec<(usize, u32)>,
}

impl Timing {
    /// NOT 1:1 (T1, live play): a steady race frame as measured in the reference race (timer 3 at 1,098 ticks, four
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
        let d = nfsgba_formats::Dump::load(prefix)?.require(&["palette", "vram", "oam"])?;
        Ok(Machine::from_state(
            rom,
            &[d.ewram, d.iwram, d.palette, d.vram, d.oam].concat(),
        ))
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
    /// The ROM's tables, parsed once.
    pub data: Arc<GameData>,
    /// The race's state.
    pub world: World,
    /// What the frame puts out: palette RAM, VRAM (the two mode-4 pages and the OBJ tiles) and OAM.
    pub palette: Vec<u8>,
    pub vram: Vec<u8>,
    pub oam: Vec<u8>,
    /// DISPCNT's frame select: the mode-4 page on screen (0: `0x06000000`, 1: `0x0600A000`).
    pub display_page: u8,
    /// Samples the sound hardware played during the last frame (signed 8-bit, 176 per VBlank, 10,512 Hz).
    pub sound: Vec<u8>,
    pub bank: SpriteBank,
    /// VBlank IRQs run so far in the current frame.
    irqs: u32,
}

fn is(r: bool, what: &'static str) -> nfsgba_sim::Result<()> {
    if r { Err(Unported(what)) } else { Ok(()) }
}

impl Game {
    /// The game at a machine state (a trace, an emulator dump, the race start's result).
    pub fn new(m: Machine) -> Game {
        let world = World::load(&m);
        Game::with_world(m.mem.rom.clone(), m.mem.data().clone(), world, m.palette, m.vram, m.oam)
    }

    pub fn with_world(
        rom: Vec<u8>,
        data: Arc<GameData>,
        world: World,
        palette: Vec<u8>,
        vram: Vec<u8>,
        oam: Vec<u8>,
    ) -> Game {
        let descriptor = world.lp.descriptor as usize - 0x0800_0000;
        Game {
            bank: ui::sprite_bank(&rom, descriptor),
            rom,
            data,
            world,
            palette,
            vram,
            oam,
            display_page: 0,
            sound: Vec::new(),
            irqs: 0,
        }
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
        let g = &mut self.world.g;
        g.frame_ticks = t.timer3 as i32;
        if g.link == 2 {
            g.dt = 15;
        } else {
            if t.timer3 == 0 {
                g.frame_ticks = 0x200;
            }
            g.dt = nfsgba_sim::math::div(25_500, g.frame_ticks).clamp(10, 100);
        }
        self.irqs = 0;
        self.irqs_to(t.entities);
        self.flip_page();
        is(
            self.world.lp.game_state != 5,
            "game states other than the race (state machine FUN_0812acec)",
        )?;
        self.race_frame(t, assist)?;
        view::hud::draw_effect_sprites(&self.rom, &mut self.world);
        // FUN_0816102c: the shadow OAM to OAM.
        self.oam.copy_from_slice(&view::hud::shadow_oam_bytes(&self.world));
        let g = &mut self.world.g;
        g.race_frames = g.race_frames.wrapping_add(1);
        if self.world.lp.game_state == 5 {
            self.tint();
        }
        is(self.world.g.fade != 0, "palette fades (main_frame, counter 0x03005630)")?;
        self.read_keys(keys)?;
        self.irqs_to(t.end);
        Ok(())
    }

    /// `FUN_0812b084`: shows the page drawn last frame and draws into the other (the view's page).
    fn flip_page(&mut self) {
        let w = &mut self.world;
        let (mode, pages) = (w.screen.mode, w.screen.pages);
        let draw = if w.g.race_frames & 1 == 0 {
            self.display_page = 0;
            if mode == 0x0F { pages[0] } else { pages[1] }
        } else {
            self.display_page = 1;
            pages[0]
        };
        w.view.page = draw;
    }

    /// `FUN_0812b040`: `KEYINPUT` into the held and newly pressed words, and the player's control word.
    fn read_keys(&mut self, keys: u16) -> nfsgba_sim::Result<()> {
        let w = &mut self.world;
        let held = !(0x3FF & !keys);
        w.input.pressed = held & !w.input.held;
        w.input.held = held;
        is(w.g.link == 2, "link play (FUN_0814737c, FUN_081469d8)")?;
        w.g.input[w.g.player as usize] = held;
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
        self.world.lp.paused == 0 && self.world.g.phase == 2
    }

    /// A VBlank IRQ (`vblank_irq`): the sound DMA, mix and buffer swap, the counters (the race time while
    /// racing and not paused) and the sky gradient's start for the next display frame.
    fn vblank(&mut self) {
        self.sound.extend(self.world.audio.vblank(Rom(&self.rom)));
        let runs = self.race_time_runs();
        let w = &mut self.world;
        w.lp.vblanks = w.lp.vblanks.wrapping_add(1);
        if runs {
            w.g.time = w.g.time.wrapping_add(1);
        }
        w.lp.ticks = w.lp.ticks.wrapping_add(1);
        w.lp.u_5724 = w.lp.u_5724.wrapping_add(1);
        if w.lp.descriptor != 0 {
            let at = (w.lp.descriptor & 0xFF_FFFF) as usize + 100;
            let line = i16::from_le_bytes([self.rom[at], self.rom[at + 1]]) as i32;
            let k = (line - (w.camera.horizon >> 1) - ((w.screen.shake[1] as i32 >> 1) + 4)).max(0);
            w.gradient_start = k as usize;
        }
        w.lp.irq |= 1;
    }

    /// The sound commands the entity handlers issued, in order (`nfsgba_sim::sound`), each after the VBlank IRQs
    /// that came before its effect in the frame (`t.sounds`; the sim records the commands instead of running
    /// them): a rate change takes effect when the call returns (after its division), a stop at its entry (it
    /// zeroes the volume first). A play that an IRQ interrupted half-way is not modelled.
    fn play_commands(&mut self, commands: Vec<Command>, t: &Timing, done: &mut usize) -> nfsgba_sim::Result<()> {
        for c in commands {
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
            let (rom, option) = (Rom(&self.rom), self.world.g.volume);
            let audio = &mut self.world.audio;
            match c {
                Command::Play(id) => {
                    audio.carbon_play_sound(rom, id, option);
                }
                Command::Stop(id) => audio.carbon_stop_sound(rom, id),
                Command::Pitch(id, rate8) => audio.carbon_set_sound_rate(rom, id, rate8),
                Command::Start {
                    sample,
                    pitch,
                    channel,
                    volume,
                } => {
                    audio.play_sfx(rom, sample, pitch, channel as i32, volume as i32);
                }
            }
        }
        Ok(())
    }

    /// Runs `step` on the car world for car `who` with the race time the game read `at` IRQs into the frame (the
    /// IRQs themselves run later, where the frame's other timing points put them): the ticks between now and
    /// then are lent to it.
    fn lend<R>(&mut self, at: Option<u32>, who: usize, step: impl FnOnce(&mut CarWorld) -> R) -> (R, Vec<Command>) {
        let lent = match at {
            Some(n) if self.race_time_runs() => n.saturating_sub(self.irqs),
            _ => 0,
        };
        let w = &mut self.world;
        w.g.time = w.g.time.wrapping_add(lent);
        let r = w.with_cars(&self.rom, &self.data, who, step);
        w.g.time = w.g.time.wrapping_sub(lent);
        r
    }

    /// `race_frame_update` (`0x0813a954`) while racing.
    fn race_frame(
        &mut self,
        t: &Timing,
        assist: &mut dyn FnMut(Checkpoint, &mut Game) -> bool,
    ) -> nfsgba_sim::Result<()> {
        let w = &mut self.world;
        w.g.steps = w.g.steps.wrapping_add(1);
        // FUN_081360dc(1) / FUN_08139e10: restart the engine loop when its slot fell silent.
        if !w.audio.sfx_playing(1) {
            let id = w.profile.engine_sound as i32 as u32;
            w.audio.carbon_play_sound(Rom(&self.rom), id, w.g.volume);
        }
        self.shade_car_paint();
        let w = &mut self.world;
        w.slot_counter = 0; // FUN_0814f8a0
        (w.profile.gear_changed, w.contact, w.sky) = (0, 0, 0);
        self.update_entities(t)?;
        assist(Checkpoint::Entities, self);
        if !assist(Checkpoint::Camera, self) {
            let mut f = self.world.camera_frame();
            camera::dispatch(&self.rom, &self.data, &mut f)?;
            self.world.set_camera_frame(f);
        }
        is(
            !matches!(self.world.camera.view, 0 | 2),
            "camera views other than the bumper and chase views",
        )?;
        let mut vis = view::visible(&self.rom, &self.world);
        if vis.sky {
            self.world.sky = 1;
        }
        if !assist(Checkpoint::Slots, self) {
            let (rom, data) = (&self.rom, &self.data);
            self.world.with_slots(rom, data, |f| f.race_slots())?;
        }
        let page = (self.world.view.page - 0x0600_0000) as usize;
        if self.world.sky != 0 {
            let w = &self.world;
            let desc = sky::sky_desc(&self.rom, w.g.level as usize);
            let clip = [w.screen.rect[1], w.screen.rect[3]];
            sky::draw_skyline(
                &self.rom,
                &desc,
                &view::sky_camera(w),
                clip,
                &mut self.vram[page..page + 240 * 160],
            );
        }
        self.draw_world(&mut vis, page);
        let w = &self.world;
        let (phase, fade) = (w.g.phase as u32, w.g.fade);
        is(
            (phase == 1 || phase == 9) && fade == 0,
            "the race countdown (race_start_from_table_b)",
        )?;
        is(
            w.lp.start_state == 3,
            "the race start set-up (FUN_0813a054..FUN_0813a108)",
        )?;
        self.irqs_to(t.timer.unwrap_or(t.hud));
        self.hud();
        let over = self.world.g.race_over;
        is(phase.wrapping_sub(6) < 3 && over == 0, "the race-end countdown")?;
        is(phase == 3 && over == 0, "the race end")?;
        self.positions();
        // A contact this frame (profile +0x2E0, +0x2E4 set by the car steps).
        let w = &self.world;
        let nitro = w.slots[w.g.player as usize].c.nitro_on;
        if w.profile.gear_changed != 0 && w.contact != 0 && nitro == 0 {
            let (rom, option) = (Rom(&self.rom), w.g.volume);
            self.world.audio.carbon_play_sound(rom, 0x20, option);
            self.world.audio.carbon_set_sound_rate(rom, 0x20, 0x4B0);
        }
        is(over != 0 && fade == 0, "the race end")?;
        if phase != 1 {
            let mut h = self.world.hud_frame();
            if self.world.g.wrong_way == 0 {
                hud::message_cancel(&self.rom, &h.g, &mut h.objects, &mut h.messages, 2);
            } else {
                hud::message_show(&self.rom, &h.g, &mut h.messages, 2, 0x3C, false);
            }
            self.world.set_hud_frame(h);
        }
        let w = &self.world;
        is(
            over == 0 && w.input.pressed & 8 != 0 && fade == 0 && w.lp.paused == 0,
            "the pause menu (START)",
        )
    }

    /// `update_entities` (`0x0813765c`): the handler of every entity with state bits 0 and 1 set, from the entity
    /// handler table `0x087F38B8`: the cars (0..3), the opponents and the wingman (0x29), the sparks (0x34) and
    /// traffic (0x36).
    fn update_entities(&mut self, t: &Timing) -> nfsgba_sim::Result<()> {
        let mut sounds = 0;
        for i in 0..self.world.entity_count() {
            let e = &self.world.slots[i].e;
            if e.state & 3 != 3 || e.sector == 0xFFFF {
                continue; // (sector 0xFFFF: the game prints a debug message)
            }
            let (state, handler) = (e.race_state, e.handler);
            match handler {
                0..=3 => {
                    if state == 0 {
                        self.world.new_car(&self.rom, i);
                    }
                    if matches!(state, 2 | 0x100) {
                        self.world.rim_redraw(&self.rom, &self.data, i)?;
                    }
                    // The step reads the race time (route_gap, lap crossing) with the IRQs before route_gap
                    // counted; the sim runs the step whole, so those IRQs' race-time ticks are lent to it and
                    // the IRQs themselves run after it, between the sound commands they fell between.
                    let at = t.gap_reads.first().copied().or(t.gap);
                    // FUN_0814bd4c: the entity leaves its sector's list for the step.
                    let ((), commands) = self.lend(at, i, |w| {
                        w.unlink(i);
                        nfsgba_sim::car::handler(w, i);
                        w.link(i);
                    });
                    // The player's decal, unpacked into its rim buffer: NOT 1:1 (R24), in the heap arena.
                    if state == 0 && i as u32 == self.world.g.player {
                        self.world.on_arena(&self.rom, i, nfsgba_sim::decal::unpack_decal);
                    }
                    // route_gap reads the race time again after its division (split = rt₂ − x·rt₁ / y); the sim
                    // reads it once, so an IRQ in between adds its tick afterwards.
                    if let [first, second, ..] = t.gap_reads[..]
                        && self.race_time_runs()
                    {
                        let g = &mut self.world.g;
                        g.gap = g.gap.wrapping_add((second - first) as i32);
                    }
                    self.play_commands(commands, t, &mut sounds)?;
                }
                0x29 => {
                    if state == 0 {
                        self.world.new_car(&self.rom, i);
                    }
                    // The lane-change timer reads the race time the IRQs have counted by then.
                    let at = t.lanes.iter().find(|(e, _)| *e == i).map(|&(_, n)| n);
                    let driving = state.wrapping_sub(1) < 2;
                    // FUN_0814a2a0: a driving car leaves its sector's list for the step.
                    let (effects, commands) = self.lend(at, i, |w| {
                        if driving {
                            w.unlink(i);
                        }
                        let r = nfsgba_sim::ai::handler(w, i);
                        if driving {
                            w.link(i);
                        }
                        r
                    });
                    self.play_commands(commands, t, &mut sounds)?;
                    if let Some(f) = effects? {
                        let (rom, data) = (&self.rom, &self.data);
                        self.world.with_slots(rom, data, |s| {
                            s.opponent_effects(i, f.heading as i32, f.view as i32, f.size)
                        });
                    }
                }
                0x34 => {
                    let (rom, data) = (&self.rom, &self.data);
                    self.world.with_slots(rom, data, |s| s.effect_handler(i));
                }
                0x36 => {
                    // FUN_081443fc.
                    let (r, commands) = self
                        .world
                        .with_cars(&self.rom, &self.data, 0, |w| nfsgba_sim::traffic_ai::handler(w, i));
                    r?;
                    self.play_commands(commands, t, &mut sounds)?;
                }
                _ => return Err(Unported("an entity handler other than 0..3, 0x29, 0x34 and 0x36")),
            }
        }
        Ok(())
    }

    /// `shade_car_paint(world, entities[*0x030057F8], 0, 0)`: the glass colours from the heading.
    fn shade_car_paint(&mut self) {
        let w = &mut self.world;
        if w.g.race_over != 0 {
            return;
        }
        let record = &w.records[w.slots[0].e.car as usize];
        let e = &w.slots[w.g.focus as usize].e;
        let shades = paint::glass_shades(&self.rom, record.paint, e.heading >> 8);
        let direct = w.g.fade == 0;
        for (k, s) in shades.into_iter().enumerate() {
            let i = 0xC0 + 0x10 * k;
            (w.palette_fade[i], w.palette_base[i]) = (s, s);
            if direct {
                self.palette[2 * i..2 * i + 2].copy_from_slice(&s.to_le_bytes());
            }
        }
    }

    /// `apply_sector_light_to_palette` (`0x0813a514`): palette RAM = the base palette tinted by the light at the
    /// player's position in the camera sector; unchanged when no light is found.
    fn tint(&mut self) {
        let w = &self.world;
        let e = &w.slots[w.g.player as usize].e;
        let sectors = city(&self.rom);
        let sector = &sectors[w.camera.sector as usize];
        let Some(light) = sector_light(&self.rom, sector, e.pos[0] >> 8, e.pos[2] >> 8) else {
            return;
        };
        for (i, c) in tint_palette(&w.palette_base, light).into_iter().enumerate() {
            if (1..=143).contains(&i) || (149..=255).contains(&i) {
                self.palette[2 * i..2 * i + 2].copy_from_slice(&c.to_le_bytes());
            }
        }
    }

    /// `draw_visible_sectors` (`iwram_call` to `0x030048c8`) into the draw page.
    fn draw_world(&mut self, vis: &mut render::Visibility, page: usize) {
        let w = &self.world;
        let (frame, rt) = (view::frame(w), view::runtime(&self.rom, w));
        let mut scene = view::scene(w);
        render::draw_world(
            &self.rom,
            &frame,
            &rt,
            &mut scene,
            vis,
            &mut self.vram[page..page + 240 * 160],
        );
        let entities = std::mem::take(&mut scene.entities);
        drop(scene);
        view::store_entities(&mut self.world, &entities);
    }

    /// `hud_update(world + 0xA4)` then `sprite_screen_update(world + 0xA4, 0)`.
    fn hud(&mut self) {
        let mut h = self.world.hud_frame();
        let mut obj_palette: Vec<u16> = (0..256)
            .map(|i| u16::from_le_bytes([self.palette[0x200 + 2 * i], self.palette[0x201 + 2 * i]]))
            .collect();
        let minimap = hud::update(
            &self.rom,
            &mut h.g,
            &h.racers,
            &mut h.objects,
            &mut h.messages,
            &mut obj_palette,
        );
        let (screen, tile_base) = (self.world.hud.screen as usize, self.world.hud.tile_base);
        if let Some(tiles) = minimap {
            let e = self.bank.elements[self.bank.screens[screen].first + 38];
            let at = 0x1_0000 + 32 * e.tile.wrapping_add(tile_base) as usize;
            self.vram[at..at + tiles.len()].copy_from_slice(&tiles);
        }
        for u in ui::update_sprites(
            &self.rom,
            &self.bank,
            screen,
            &mut h.objects,
            &mut h.oam,
            false,
            tile_base,
        ) {
            let at = 0x1_0000 + 32 * u.tile;
            self.vram[at..at + u.len].copy_from_slice(&self.rom[u.src..u.src + u.len]);
        }
        for (i, c) in obj_palette.into_iter().enumerate() {
            self.palette[0x200 + 2 * i..0x202 + 2 * i].copy_from_slice(&c.to_le_bytes());
        }
        // The HUD's digits divide through the IWRAM routine, which keeps its remainder; the frame's last such
        // division is the speed's ones digit (`hud_speed`).
        let g = &h.g;
        if g.hud != 0 && (0..=3).contains(&g.mode) {
            let speed = h.racers[g.player].driver.map_or(0, |d| d.speed);
            let mut v = nfsgba_formats::div(speed, 0x163C);
            if g.units == 0 {
                v = nfsgba_fixed::iwram_divmod(v << 8, 0x19B).0;
            }
            let rest = nfsgba_fixed::iwram_divmod(v, 100).1;
            self.world.g.div_rem = nfsgba_fixed::iwram_divmod(rest, 10).1;
        }
        self.world.set_hud_frame(h);
    }

    /// `FUN_0813ea04`: race positions, 1-based, from the race progress.
    fn positions(&mut self) {
        let w = &mut self.world;
        let n = w.g.opponents as usize + 1;
        let player = w.g.player as usize;
        if w.g.phase == 9 {
            for i in 0..n {
                w.slots[player + i].c.position = i as i32 + 1;
            }
            return;
        }
        let lapped = w.g.circuit != 0;
        let last = {
            let s = &w.route.line.sections[0];
            let k = (s.first as usize).wrapping_add(s.count as usize).wrapping_sub(1);
            w.route.line.points[k].distance
        };
        let laps = w.g.laps;
        let progress = |s: &Slot| {
            if !lapped {
                return s.c.progress;
            }
            (laps.wrapping_sub(s.c.laps_left as i32))
                .wrapping_mul(last)
                .wrapping_add(s.c.progress)
        };
        for i in 0..n {
            let e = &w.slots[player + i];
            if e.e.race_state == 2 {
                continue;
            }
            let mut position = 1;
            for (j, o) in w.slots.iter().enumerate().take(n) {
                if o.c.route_flags & 8 != 0 || player + i == j {
                    continue;
                }
                let (pe, po) = (progress(e), progress(o));
                if pe.wrapping_sub(po) < 0 || (pe == po && j < i) || o.e.race_state == 2 {
                    position += 1;
                }
            }
            w.slots[player + i].c.position = position;
        }
    }

    /// The backdrop colour of each screen line (palette entry 0 as the VCount IRQ sets it every 2 lines from
    /// the sky gradient, starting at the entry the last VBlank chose).
    pub fn backdrop(&self) -> [u16; 160] {
        let w = &self.world;
        std::array::from_fn(|y| w.gradient[sky::backdrop_entry(w.gradient_start, y)])
    }

    /// The 240×160 mode-4 page on screen (palette indices; 0 shows the backdrop).
    pub fn screen(&self) -> &[u8] {
        let at = if self.display_page == 0 { 0 } else { 0xA000 };
        &self.vram[at..at + 240 * 160]
    }
}
