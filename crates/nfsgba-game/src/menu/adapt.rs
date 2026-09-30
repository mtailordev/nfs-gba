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

pub(super) fn load_state(g: &mut Gba) -> MenuState {
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
        match self.0.draw_call(function, args) {
            Some(r) => r,
            None => self.0.unported(function, args),
        }
    }
    fn button_prompts(&mut self, st: &MenuState, a: &[u32; 3]) {
        let page = self.0.draw_page();
        let unpacked = self
            .0
            .on_page(page, |c, p, _| c.button_prompts(p, st, a[0], a[1], a[2]));
        self.0.log_unpacks(unpacked);
    }
    fn message_box_draw(&mut self, st: &mut MenuState) {
        let page = self.0.draw_page();
        let text = if st.g.message_text > 0xFFFF {
            self.0.cstring(st.g.message_text)
        } else {
            Vec::new()
        };
        let unpacked = self.0.on_page(page, |c, p, _| c.message_box(p, st, |_| text));
        self.0.log_unpacks(unpacked);
    }
    fn handler(&mut self, st: &mut MenuState, kind: Kind, phase: usize, args: &[u32]) -> u32 {
        // NOT 1:1 (U3): the top-level oracle sets keep the garage screens stubbed; `tools/oracle/garage.py` checks them.
        if flow::is_typed(kind, phase) && (kind != Kind::Kind18 || self.0.garage_typed) {
            // The drawing primitives read the language and the text pointers they are given (the name buffer) from
            // the RAM image; an earlier typed handler may have changed them.
            store_state(self.0, st);
            return flow::run_typed(st, self, kind, phase, args);
        }
        self.on_ram(st, |g| stubbed_handler(g, kind, phase, args))
    }
    fn scene_setup(&mut self, st: &mut MenuState, material: u32, palette: u32, sprite: u32) {
        self.on_ram(st, |g| menu_scene_setup(g, material, palette, sprite));
    }
    fn scene_setup_ab(&mut self, st: &mut MenuState, first: bool, material: u32, palette: u32, sprite: u32) {
        self.on_ram(st, |g| menu_scene_setup_ab(g, first, material, palette, sprite));
    }
    fn world_palette(&self) -> u32 {
        self.0.u32(WORLD + 0x30)
    }
    fn page_buffer(&self) -> u32 {
        self.0.u32(self.0.u32(WORLD + 0x50))
    }
    fn clear_frame_buffers(&mut self) {
        for buffer in [0x0300_641C, 0x0300_6420] {
            let (dst, n) = (self.0.u32(buffer), screen_bytes(self.0));
            fill32(self.0, dst, 0x0101_0101, n);
        }
    }
    fn fill_page(&mut self) {
        let (page, n) = (self.0.u32(self.0.u32(WORLD + 0x50)), screen_bytes(self.0));
        fill32(self.0, page, 0x0101_0101, n);
    }
    fn peek16(&self, addr: u32) -> u16 {
        self.0.u16(addr)
    }
    fn second_colour(&self, i: u32) -> u16 {
        self.0.u16(self.0.u32(SECOND_PALETTE) + 2 * i)
    }
    fn set_second_colour(&mut self, i: u32, c: u16) {
        self.0.set_u16(self.0.u32(SECOND_PALETTE) + 2 * i, c);
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
    // The garage oracle set runs the rules and the map for real; the older sets stub them (default: logged calls).
    fn unlock_state(&mut self, st: &mut MenuState, id: u32) -> u32 {
        if self.0.garage_typed {
            return garage::state(st, &self.0.rom, id);
        }
        self.call(0x0812_C81C, &[id])
    }
    fn unlock_owned(&mut self, st: &mut MenuState, id: u32) -> u32 {
        if self.0.garage_typed {
            return garage::owned(st, &self.0.rom, id);
        }
        self.call(0x0812_C5C4, &[id])
    }
    fn unlock_price(&mut self, st: &mut MenuState, id: u32) -> u32 {
        if self.0.garage_typed {
            return garage::price(st, &self.0.rom, id);
        }
        self.call(0x0812_D960, &[id])
    }
    fn buy_unlock(&mut self, st: &mut MenuState, id: u32) {
        if self.0.garage_typed {
            garage::buy(st, &self.0.rom, id);
        } else {
            self.call(0x0812_C8A8, &[id]);
        }
    }
    fn upgrades_changed(&mut self, st: &mut MenuState, second: u32) -> u32 {
        if self.0.garage_typed {
            return garage::upgrades_changed(st, self, second != 0);
        }
        self.call(0x0813_02C4, &[second])
    }
    fn new_mark(&mut self, st: &mut MenuState, screen: u32, item: u32) -> u32 {
        if self.0.garage_typed {
            return garage::list_item_new(st, self, screen, item);
        }
        self.call(0x0813_00E0, &[screen, item])
    }
    fn car_stats(&mut self, st: &mut MenuState, car: u32, x: u32, y: u32, rows: u32) {
        if self.0.garage_typed {
            garage::car_stats_draw(st, self, car as i32, x as i32, y as i32, rows);
        } else {
            self.call(0x0813_3D30, &[car, x, y, rows]);
        }
    }
    fn map_draw(&mut self, st: &mut MenuState) {
        if self.0.map_typed {
            map::draw_map(st, self);
        } else {
            self.call(0x0814_35C4, &[]);
        }
    }
    fn map_background(&mut self, src: u32) {
        let page = self.page_buffer();
        for y in 0..160u32 {
            for x in 0..240u32 {
                let v = self.0.u8(src + y * 0x200 + x);
                self.0.set_u8(page + y * 240 + x, v);
            }
        }
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

/// `fill32` (IWRAM `0x030002C0`, through `*0x0300649C`): `n >> 5` blocks of 32 bytes of `value`.
fn fill32(g: &mut Gba, dst: u32, value: u32, n: u32) {
    for i in 0..(n >> 5) * 8 {
        g.set_u32(dst + 4 * i, value);
    }
}

/// The screen size (`0x03006410`: width, height) for the frame-buffer fills.
fn screen_bytes(g: &Gba) -> u32 {
    (g.u16(0x0300_6410) as i16 as i32 * g.u16(0x0300_6412) as i16 as i32) as u32
}

// The drawing primitives (`draw.rs`) on the RAM image: the page is wherever the game's pointer says (VRAM or EWRAM).

const UNPACK_TO_BUFFER: u32 = 0x0816_3D30; // (source, destination): stays a logged call until the scene port
const MENU_TEXELS: u32 = 0x0816_C244;
const TEXT_MENU_7: u32 = 0x0814_19C0;

impl Gba {
    /// The C string a text primitive's pointer argument names: a stack string the port made ([`Gba::text_arg`]) or
    /// bytes in the game's memory. Stack strings are marked as used, so the stub-call comparison skips them.
    fn cstring(&mut self, addr: u32) -> Vec<u8> {
        let i = addr.wrapping_sub(STACK_TEXT) as usize;
        if i < self.texts.len() {
            self.consumed.insert(i);
            return self.texts[i].clone();
        }
        // ponytail: the BIOS (below 0x02000000) reads as zeros, so a null text pointer is an empty string.
        (addr..)
            .map(|a| if a >> 24 < 2 { 0 } else { self.u8(a) })
            .take_while(|&b| b != 0)
            .collect()
    }

    fn draw_ctx(&self) -> draw::Ctx<'_> {
        draw::Ctx {
            rom: &self.rom,
            language: self.u32(LANGUAGE),
            pitch: self.u16(0x0300_6410) as i16 as i32,
        }
    }

    /// Runs `f` on the primitives' context and the page at `addr` (`f` also learns whether byte stores duplicate).
    fn on_page<R>(&mut self, addr: u32, f: impl FnOnce(&draw::Ctx, &mut [u8], bool) -> R) -> R {
        let (language, pitch) = (self.u32(LANGUAGE), self.u16(0x0300_6410) as i16 as i32);
        let rom = std::mem::take(&mut self.rom);
        let Some((buf, at)) = self.at_mut(addr) else {
            panic!("a page at {addr:#010x}")
        };
        let r = f(
            &draw::Ctx {
                rom: &rom,
                language,
                pitch,
            },
            &mut buf[at..],
            false, // no BG VRAM duplication of byte stores here: the oracle (unicorn) does not model it; `Screen` does
        );
        self.rom = rom;
        r
    }

    /// The page the menus draw on: `**(world + 0x50)`.
    fn draw_page(&self) -> u32 {
        self.u32(self.u32(WORLD + 0x50))
    }

    /// The game's `unpack_to_buffer` calls for the blits that unpacked a material.
    fn log_unpacks(&mut self, offsets: impl IntoIterator<Item = usize>) {
        for o in offsets {
            let buffer = self.u32(0x0300_57F0) + 0x9608;
            self.unported(UNPACK_TO_BUFFER, &[MENU_TEXELS + o as u32, buffer]);
        }
    }

    /// The drawing primitives that take only their arguments; `None` for any other function.
    pub(super) fn draw_call(&mut self, function: u32, a: &[u32]) -> Option<u32> {
        let text = |g: &mut Gba, arg: u32| {
            if arg > 0xFFFF {
                g.cstring(arg)
            } else {
                let p = g.draw_ctx().table_pointer(arg); // may point outside the ROM for a key past the table
                g.cstring(p)
            }
        };
        let drawn = [
            FILL_RECT,
            MENU_BLIT_MATERIAL,
            MENU_BLIT_MATERIAL_ALT,
            TEXT_MENU,
            TEXT_MENU_WRAPPED,
            TEXT_BOX,
            TEXT_MENU_7,
        ];
        if !drawn.contains(&function) {
            return None;
        }
        let page = if function == FILL_RECT { 0 } else { self.draw_page() };
        Some(match function {
            FILL_RECT => {
                let i = a[0].wrapping_sub(STACK_TEXT) as usize;
                let rect = if i < self.texts.len() {
                    self.consumed.insert(i);
                    self.texts[i].clone()
                } else {
                    (0..16).map(|k| self.u8(a[0] + k)).collect()
                };
                let r = |i: usize| i32::from_le_bytes(rect[4 * i..][..4].try_into().unwrap());
                self.on_page(a[1], |_, p, bg| {
                    nfsgba_formats::ui::fill_rect8(p, a[2] as i32, [r(0), r(1), r(2), r(3)], a[3], bg)
                });
                0
            }
            MENU_BLIT_MATERIAL | MENU_BLIT_MATERIAL_ALT => {
                let alt = function == MENU_BLIT_MATERIAL_ALT;
                let unpacked = self.on_page(page, |c, p, _| c.blit_material(p, a[1], a[2] as i32, a[3] as i32, alt));
                self.log_unpacks(unpacked);
                0
            }
            TEXT_MENU => {
                let t = text(self, a[1]);
                self.on_page(page, |c, p, _| {
                    c.text_menu(p, a[0], &t, a[2] as i32, a[3], a[4] as i32, a[5] as i16)
                })
            }
            TEXT_MENU_WRAPPED => {
                let t = text(self, a[1]);
                self.on_page(page, |c, p, _| {
                    c.text_wrapped(
                        p,
                        a[0],
                        &t,
                        a[2] as i32,
                        a[3] as i32,
                        a[4] as i32,
                        a[5] as i32,
                        a[6] as i16,
                    )
                });
                0
            }
            TEXT_BOX => {
                let t = text(self, a[1]);
                self.on_page(page, |c, p, _| {
                    c.text_box(
                        p,
                        a[0],
                        &t,
                        a[2] as i32,
                        a[3] as i32,
                        a[4] as i32,
                        a[5] as i32,
                        a[6] as i16,
                    )
                });
                0
            }
            TEXT_MENU_7 => {
                let t = text(self, a[1]);
                for buffer in [0x0300_641C, 0x0300_6420] {
                    let page = self.u32(buffer);
                    self.on_page(page, |c, p, _| {
                        c.text_box(
                            p,
                            a[0],
                            &t,
                            a[2] as i32,
                            a[3] as i32,
                            a[4] as i32,
                            a[5] as i32,
                            a[6] as i16,
                        )
                    });
                }
                0
            }
            _ => return None,
        })
    }
}

/// The typed [`draw::Screen`] a RAM image shows: both pages, the palettes, the shadow OAM, OBJ tiles and the
/// registers the menus set (for the checks of `scene.rs` and the frame comparisons against a headless run).
pub fn screen_of(g: &Gba) -> draw::Screen {
    let u16s = |b: &[u8]| -> Vec<u16> { b.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect() };
    let mut s = draw::Screen::default();
    s.pages[0].copy_from_slice(&g.vram[..240 * 160]);
    s.pages[1].copy_from_slice(&g.vram[0xA000..0xA000 + 240 * 160]);
    s.dispcnt = g.u16(0x0400_0000);
    s.palette.copy_from_slice(&u16s(&g.pal[..0x400]));
    for (i, e) in s.oam.iter_mut().enumerate() {
        *e = std::array::from_fn(|k| g.u16(0x0300_64F0 + 8 * i as u32 + 2 * k as u32));
    }
    s.obj_tiles.copy_from_slice(&g.vram[0x14000..0x18000]);
    s.tile_base = g.u16(0x0300_64E0);
    (s.bldcnt, s.bldalpha, s.dispstat, s.timer3) = (
        g.u16(0x0400_0050),
        g.u16(0x0400_0052),
        g.u16(0x0400_0004),
        g.u16(0x0400_010E),
    );
    s
}
