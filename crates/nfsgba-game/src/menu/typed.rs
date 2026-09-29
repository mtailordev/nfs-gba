//! The typed menu host: [`flow::Host`] over a [`Screen`], a [`Scene`] and owned strings, with no RAM image. The scene
//! setup, sprites, palettes and drawing primitives run on the typed state (`scene.rs`, `draw.rs`); calls to game
//! functions that are not ported yet (sound, saves, the garage screens) are logged in `calls` and answer 0.
//!
//! Pointers the flow code still holds (`race_palette`, `second_palette`, the page addresses) are handles here: the
//! host maps the values in the state it was built from to the scene's buffers (`Memory`).

use nfsgba_sim::state::MenuState;

use super::draw::{Ctx, Screen};
use super::flow::{self, Host};
use super::scene::Scene;
use super::{Kind, STACK_TEXT};

const HEALTH_SCREEN_IMAGE: u32 = 0x0813_644C; // (world, material)
const COPY_PALETTE_TO_RAM: u32 = 0x0815_DFD8; // (source)
const PAGES: [u32; 2] = [0x0600_0000, 0x0600_A000];

pub struct TypedHost<'a> {
    pub rom: &'a [u8],
    pub screen: Screen,
    pub scene: Scene,
    pub texts: Vec<Vec<u8>>,
    /// Unported calls: address and arguments, in order.
    pub calls: Vec<(u32, Vec<u32>)>,
    language: u32,
    /// The state's pointers to the second base palette and the race palette buffers (the scene's `palettes`).
    palette_handles: [u32; 2],
}

impl<'a> TypedHost<'a> {
    pub fn new(rom: &'a [u8], st: &MenuState) -> Self {
        TypedHost {
            rom,
            screen: Screen::default(),
            scene: Scene::default(),
            texts: Vec::new(),
            calls: Vec::new(),
            language: st.g.language,
            palette_handles: [st.g.second_palette, st.g.race_palette],
        }
    }

    fn ctx(&self) -> Ctx<'a> {
        Ctx {
            rom: self.rom,
            language: self.language,
            pitch: 240,
        }
    }

    /// The text of a primitive's argument: a table key, a stack string of the flow, or a string in the ROM.
    fn text(&self, arg: u32) -> Vec<u8> {
        if arg <= 0xFFFF {
            return self.ctx().table_text(arg);
        }
        let i = arg.wrapping_sub(STACK_TEXT) as usize;
        if let Some(t) = self.texts.get(i) {
            return t.clone();
        }
        if arg >> 24 == 8 {
            let s = &self.rom[arg as usize & 0x1FF_FFFF..];
            return s[..s.iter().position(|&b| b == 0).unwrap_or(s.len())].to_vec();
        }
        Vec::new() // ponytail: the game reads stale RAM through a pointer the flow never makes
    }

    fn read8(&self, addr: u32) -> u8 {
        if addr >> 24 == 8 {
            return self.rom[addr as usize & 0x1FF_FFFF];
        }
        for (k, base) in self.palette_handles.iter().enumerate() {
            if let Some(o) = addr.checked_sub(*base).filter(|&o| o < 0x200 && *base != 0) {
                return self
                    .scene
                    .palettes
                    .get(k)
                    .and_then(|p| p.get(o as usize / 2))
                    .map_or(0, |c| c.to_le_bytes()[o as usize & 1]);
            }
        }
        0
    }

    fn write8(&mut self, addr: u32, v: u8) {
        for (k, base) in self.palette_handles.iter().enumerate() {
            if let Some(o) = addr.checked_sub(*base).filter(|&o| o < 0x200 && *base != 0)
                && let Some(c) = self.scene.palettes.get_mut(k).and_then(|p| p.get_mut(o as usize / 2))
            {
                let mut b = c.to_le_bytes();
                b[o as usize & 1] = v;
                *c = u16::from_le_bytes(b);
            }
        }
    }
}

impl Host for TypedHost<'_> {
    fn rom(&self) -> &[u8] {
        self.rom
    }

    fn text_arg(&mut self, s: Vec<u8>) -> u32 {
        self.texts.push(s);
        STACK_TEXT + self.texts.len() as u32 - 1
    }

    fn call(&mut self, function: u32, a: &[u32]) -> u32 {
        let ctx = self.ctx();
        let hidden = (self.screen.dispcnt >> 4 & 1) as usize ^ 1;
        match function {
            super::MENU_BLIT_MATERIAL | super::MENU_BLIT_MATERIAL_ALT => {
                let alt = function == super::MENU_BLIT_MATERIAL_ALT;
                self.screen
                    .draw(&ctx, |c, p| c.blit_material(p, a[1], a[2] as i32, a[3] as i32, alt));
            }
            super::TEXT_MENU => {
                let t = self.text(a[1]);
                return self.screen.draw(&ctx, |c, p| {
                    c.text_menu(p, a[0], &t, a[2] as i32, a[3], a[4] as i32, a[5] as i16)
                });
            }
            super::TEXT_MENU_WRAPPED => {
                let t = self.text(a[1]);
                let (x, y, w, n, col) = (a[2] as i32, a[3] as i32, a[4] as i32, a[5] as i32, a[6] as i16);
                self.screen
                    .draw(&ctx, |c, p| c.text_wrapped(p, a[0], &t, x, y, w, n, col));
            }
            super::TEXT_BOX | TEXT_MENU_7 => {
                let t = self.text(a[1]);
                let (x, y, w, n, col) = (a[2] as i32, a[3] as i32, a[4] as i32, a[5] as i32, a[6] as i16);
                if function == super::TEXT_BOX {
                    self.screen.draw(&ctx, |c, p| c.text_box(p, a[0], &t, x, y, w, n, col));
                } else {
                    for page in &mut self.screen.pages {
                        ctx.text_box(page, a[0], &t, x, y, w, n, col);
                    }
                }
            }
            super::FILL_RECT => {
                let rect = self
                    .texts
                    .get(a[0].wrapping_sub(STACK_TEXT) as usize)
                    .cloned()
                    .unwrap_or_default();
                if rect.len() >= 16 {
                    let r = |i: usize| i32::from_le_bytes(rect[4 * i..][..4].try_into().unwrap());
                    let page = PAGES.iter().position(|&p| p == a[1]).unwrap_or(hidden);
                    nfsgba_formats::ui::fill_rect8(
                        &mut self.screen.pages[page],
                        a[2] as i32,
                        [r(0), r(1), r(2), r(3)],
                        a[3],
                        true,
                    );
                }
            }
            super::INTRO_PAGE_SETUP => self.scene.intro_page_setup(self.screen.page()),
            HEALTH_SCREEN_IMAGE => self.scene.health_screen_image(self.rom, a[1]),
            COPY_PALETTE_TO_RAM => {
                let src = (a[0] != 0).then(|| {
                    (0..256)
                        .map(|i| u16::from_le_bytes([self.read8(a[0] + 2 * i), self.read8(a[0] + 2 * i + 1)]))
                        .collect::<Vec<_>>()
                });
                return super::scene::copy_palette_to_ram(&mut self.screen, src.as_deref()) as u32;
            }
            _ => self.calls.push((function, a.to_vec())),
        }
        0
    }

    fn handler(&mut self, st: &mut MenuState, kind: Kind, phase: usize, args: &[u32]) -> u32 {
        if flow::is_typed(kind, phase) {
            return flow::run_typed(st, self, kind, phase, args);
        }
        if phase == 3 {
            self.scene.free(&mut self.screen); // every kind's exit handler is the scene's teardown
            return 0;
        }
        self.calls.push((kind.handlers()[phase], args.to_vec()));
        0
    }

    fn scene_setup(&mut self, st: &mut MenuState, material: u32, palette: u32, sprite: u32) {
        let first = st.g.screen_entered == 0;
        self.scene_setup_ab(st, first, material, palette, sprite);
    }

    fn scene_setup_ab(&mut self, st: &mut MenuState, first: bool, material: u32, palette: u32, sprite: u32) {
        if first {
            st.g.menu_exit = 0;
        }
        self.scene
            .setup(self.rom, &mut self.screen, first, material, palette, sprite);
    }

    fn world_palette(&self) -> u32 {
        0x0800_0000 + self.scene.base_palette as u32
    }

    fn page_buffer(&self) -> u32 {
        PAGES[(self.screen.dispcnt >> 4 & 1) as usize ^ 1]
    }

    fn clear_frame_buffers(&mut self) {
        self.screen.pages.iter_mut().for_each(|p| p.fill(1));
    }

    fn fill_page(&mut self) {
        self.screen.page().fill(1);
    }

    fn peek16(&self, addr: u32) -> u16 {
        u16::from_le_bytes([self.read8(addr), self.read8(addr + 1)])
    }

    fn second_colour(&self, i: u32) -> u16 {
        self.scene.palettes[0].get(i as usize).copied().unwrap_or(0)
    }

    fn set_second_colour(&mut self, i: u32, c: u16) {
        if let Some(p) = self.scene.palettes[0].get_mut(i as usize) {
            *p = c;
        }
    }

    fn black_bg_palette(&mut self) {
        self.screen.palette[..256].fill(0);
    }

    fn copy_mem(&mut self, dst: u32, src: u32, n: u32, width: u32) {
        if dst == 0 || src == 0 {
            return;
        }
        let unit = match width {
            0x20 => 4,
            0x10 => 2,
            8 => 1,
            _ => return,
        };
        let bytes: Vec<u8> = (0..n / unit * unit).map(|i| self.read8(src + i)).collect();
        bytes
            .into_iter()
            .enumerate()
            .for_each(|(i, b)| self.write8(dst + i as u32, b));
    }

    fn fade_step(&mut self, st: &mut MenuState) {
        // NOT 1:1 (U7): the sky gradient buffer's step is a race matter and not modelled here.
        let obj: Vec<u16> = nfsgba_formats::ui::palette_at(self.rom, self.scene.obj_palette);
        let pal = &mut self.screen.palette;
        if st.g.fade > 0 {
            super::fade_in_step(&mut pal[..256], &self.scene.palettes[0], 0, 0x100, 4);
            super::fade_in_step(&mut pal[256..], &obj, 0, 0x100, 4);
            st.g.fade = (st.g.fade - 2).max(0);
        } else {
            super::fade_out_step(pal, 0, 0x100, 4);
            super::fade_out_step(pal, 0x100, 0x100, 4);
            st.g.fade = (st.g.fade + 2).min(0);
        }
    }

    fn button_prompts(&mut self, st: &MenuState, a: &[u32; 3]) {
        let ctx = self.ctx();
        self.screen.draw(&ctx, |c, p| c.button_prompts(p, st, a[0], a[1], a[2]));
    }

    fn message_box_draw(&mut self, st: &mut MenuState) {
        let ctx = self.ctx();
        let text = if st.g.message_text > 0xFFFF {
            self.text(st.g.message_text)
        } else {
            Vec::new()
        };
        self.screen.draw(&ctx, |c, p| c.message_box(p, st, |_| text));
    }
}

const TEXT_MENU_7: u32 = 0x0814_19C0;

/// The typed frame of a settled menu screen for the whole-scene check: the screen entered from `st` on a fresh host.
pub fn enter<'a>(rom: &'a [u8], st: &mut MenuState) -> TypedHost<'a> {
    let mut h = TypedHost::new(rom, st);
    st.g.screen_entered = 0;
    flow::enter_screen(st, &mut h);
    h
}

#[cfg(test)]
#[path = "typed_test.rs"]
mod tests;
