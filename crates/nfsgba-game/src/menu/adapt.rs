//! The typed flow ([`super::flow`]) on the RAM image (`Gba`): what the oracle-case tests call, and what the screens
//! still on the RAM image call back into.

use nfsgba_sim::{Mem, state::MenuState};

use super::*;

/// Runs `f` on the typed state loaded from `g` and stores it back.
pub(super) fn typed<R>(g: &mut Gba, f: impl FnOnce(&mut MenuState, &mut GbaHost) -> R) -> R {
    let mut st = load_state(g);
    let r = f(&mut st, &mut GbaHost(g));
    store_state(g, &st);
    r
}

/// The RAM of `g` as a `Mem` (the layouts' address space) for the duration of `f`, without copying it.
fn with_mem<R>(g: &mut Gba, f: impl FnOnce(&mut Mem) -> R) -> R {
    let mut m = Mem::new(Vec::new(), std::mem::take(&mut g.ewram), std::mem::take(&mut g.iwram));
    let r = f(&mut m);
    (g.ewram, g.iwram) = (m.ewram, m.iwram);
    r
}

fn load_state(g: &mut Gba) -> MenuState {
    with_mem(g, |m| MenuState::load(m))
}

fn store_state(g: &mut Gba, st: &MenuState) {
    with_mem(g, |m| st.store(m));
}

/// The typed flow's [`flow::Host`] on a `Gba`: handlers that are not typed yet run on the RAM image between a
/// store of the state and a load.
pub(super) struct GbaHost<'a>(&'a mut Gba);

impl GbaHost<'_> {
    fn on_ram<R>(&mut self, st: &mut MenuState, f: impl FnOnce(&mut Gba) -> R) -> R {
        store_state(self.0, st);
        let r = f(self.0);
        *st = load_state(self.0);
        r
    }
}

impl flow::Host for GbaHost<'_> {
    fn rom(&self) -> &[u8] {
        &self.0.rom
    }
    fn text_arg(&mut self, s: Vec<u8>) -> u32 {
        self.0.text_arg(s)
    }
    fn call(&mut self, function: u32, args: &[u32]) -> u32 {
        self.0.unported(function, args)
    }
    fn handler(&mut self, st: &mut MenuState, kind: Kind, phase: usize, args: &[u32]) -> u32 {
        if flow::is_typed(kind, phase) {
            return flow::run_typed(st, self, kind, phase, args);
        }
        self.on_ram(st, |g| run_handler(g, kind, phase, args))
    }
    fn career_opponents(&mut self, st: &mut MenuState) {
        self.on_ram(st, career_opponents);
    }
    fn scene_setup(&mut self, st: &mut MenuState, material: u32, palette: u32, sprite: u32) {
        self.on_ram(st, |g| menu_scene_setup(g, material, palette, sprite));
    }
    fn black_bg_palette(&mut self) {
        fill_bg_palette(self.0, 0, 0, 0x100);
    }
    fn copy_mem(&mut self, dst: u32, src: u32, n: u32, width: u32) {
        copy_mem(self.0, dst, src, n, width);
    }
    fn fade_step(&mut self, st: &mut MenuState) {
        self.on_ram(st, fade_step);
    }
}

/// `main_frame`'s palette fade (`FADE` ≠ 0): the sky gradient buffer (towards the level's gradient), BG palette RAM
/// (towards the second base palette) and OBJ palette RAM (towards world `+0x34`) move 4 per channel; `FADE` by 2.
fn fade_step(g: &mut Gba) {
    let fade = g.i32(FADE);
    let desc = g.u32(LEVEL_DESC);
    let src = if desc != 0 {
        let mat = g.u16(desc + 0x5E) as u32;
        g.u32(WORLD).wrapping_add(g.u32(g.u32(WORLD + 0x20) + 9 * 4 * mat + 8))
    } else {
        0
    };
    let buffer = g.u32(GRADIENT_BUFFER);
    let mut bg = g.u16s(0x0500_0000, 256);
    let mut obj = g.u16s(0x0500_0200, 256);
    if fade > 0 {
        if src != 0 {
            let mut buf = g.u16s(buffer, 0x200);
            fade_in_step(&mut buf, &g.u16s(src, 0x200), 0, 0x200, 4);
            g.set_u16s(buffer, &buf);
        }
        fade_in_step(&mut bg, &g.u16s(g.u32(SECOND_PALETTE), 256), 0, 0x100, 4);
        g.set_u16s(0x0500_0000, &bg);
        fade_in_step(&mut obj, &g.u16s(g.u32(WORLD + 0x34), 256), 0, 0x100, 4);
        g.set_u16s(0x0500_0200, &obj);
        g.set_u32(FADE, (g.i32(FADE) - 2).max(0) as u32);
    } else {
        if src != 0 {
            let mut buf = g.u16s(buffer, 0x200);
            fade_out_step(&mut buf, 0, 0x200, 4);
            g.set_u16s(buffer, &buf);
        }
        fade_out_step(&mut bg, 0, 0x100, 4);
        g.set_u16s(0x0500_0000, &bg);
        fade_out_step(&mut obj, 0, 0x100, 4);
        g.set_u16s(0x0500_0200, &obj);
        g.set_u32(FADE, (g.i32(FADE) + 2).min(0) as u32);
    }
}

pub use flow::CARBON_PLAY_SOUND;

// The typed helpers for the screens still on the RAM image.

const DIVIDEND_REMAINDER: u32 = 0x0300_6480;

/// A text helper of [`text`] with the IWRAM divide routine's remainder taken from and put back into `g`.
fn with_remainder<R>(g: &mut Gba, f: impl FnOnce(&mut u32) -> R) -> R {
    let mut rem = g.u32(DIVIDEND_REMAINDER);
    let r = f(&mut rem);
    g.set_u32(DIVIDEND_REMAINDER, rem);
    r
}

pub(super) fn number_text(g: &mut Gba, n: i32) -> Vec<u8> {
    with_remainder(g, |rem| text::number_text(rem, n))
}

pub(super) fn thousands(g: &Gba, s: &mut Vec<u8>, n: i32) {
    text::thousands(g.u32(LANGUAGE), s, n);
}

pub(super) fn time_text(g: &mut Gba, cs: i32) -> Vec<u8> {
    let language = g.u32(LANGUAGE);
    with_remainder(g, |rem| text::time_text(rem, language, cs))
}

pub(super) use text::frames_to_centiseconds;

pub(super) fn hint_due(g: &mut Gba, screen: i32, event: i32) -> u32 {
    let mut st = load_state(g);
    let r = event::hint_due(&mut st, &g.rom, screen, event);
    store_state(g, &st);
    r
}

pub(super) fn career_event_to_globals(g: &mut Gba) {
    let mut st = load_state(g);
    event::career_event_to_globals(&mut st, &g.rom);
    store_state(g, &st);
}

/// `list_slot` (`0x0812FD04`): the List screen's cursor slot (profile `+0x350 + slot`), −1 for other screens.
pub fn list_slot(g: &Gba) -> i32 {
    flow::list_slot(g.u32(SCREEN), g.u16(g.u32(PROFILE) + 0x12))
}

/// `rand_table` (`0x0815FCFC`): the next of the 256 numbers at `0x7C03F0`.
pub fn rand_table(g: &mut Gba) -> u32 {
    let mut i = g.u32(RAND_INDEX);
    let r = nfsgba_fixed::rand_table(&g.rom, &mut i);
    g.set_u32(RAND_INDEX, i);
    r
}

/// `unlock_is_locked` (`0x0812D784`): 1 when bit `id` of the profile's unlock bits (`+0x42D`) is clear.
pub fn unlock_is_locked(g: &Gba, id: i32) -> u32 {
    let byte = g.u8(g.u32(PROFILE).wrapping_add(0x42D).wrapping_add((id >> 3) as u32));
    ((byte as i32 >> (id & 7)) & 1 == 0) as u32
}

/// `mark_screen_changed` (`0x0812D4D8`).
pub fn mark_screen_changed(g: &mut Gba) {
    g.set_u32(SCREEN_CHANGED, 1);
}

pub fn enter_screen(g: &mut Gba) {
    typed(g, |st, h| flow::enter_screen(st, h));
}
pub fn draw_screen(g: &mut Gba, full: u32) {
    typed(g, |st, h| flow::draw_screen(st, h, full));
}
pub fn goto_screen(g: &mut Gba, s: i32) {
    typed(g, |st, h| flow::goto_screen(st, h, s));
}
pub fn menu_back(g: &mut Gba) {
    typed(g, |st, h| flow::menu_back(st, h));
}
pub fn message_box_input(g: &mut Gba) -> i32 {
    typed(g, |st, h| flow::message_box_input(st, h))
}
pub fn message_box_close(g: &mut Gba) {
    typed(g, |st, _| flow::message_box_close(st));
}
pub fn menu_frame(g: &mut Gba) -> u32 {
    typed(g, |st, h| flow::menu_frame(st, h))
}
pub fn game_state_step(g: &mut Gba) {
    typed(g, |st, h| flow::game_state_step(st, h));
}
pub fn main_frame(g: &mut Gba) {
    typed(g, |st, h| flow::main_frame(st, h));
}
