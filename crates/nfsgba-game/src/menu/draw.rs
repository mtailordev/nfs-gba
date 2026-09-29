//! The menus' drawing primitives on plain 240×160 8bpp pages, one copy: the pixel work is in
//! [`nfsgba_formats::ui`] (fonts, blitters, `fill_rect8`); this module is the game's menu-level wrappers around it
//! (`text_menu`, `text_box`, `menu_blit_material`, the button prompts, the message box) and the [`Screen`] a typed
//! menu owns: the two pages, the page flip, palette and OAM.
//!
//! Checked against the game's own code by `tools/oracle/menus.py` (the primitives are no longer stubbed there) and
//! `tools/oracle/draw.py` (`fill_rect8` on a real mGBA frame).

use nfsgba_formats::ui::{self, Align, Font};
use nfsgba_sim::state::MenuState;

use super::text::number_text;

pub const WIDTH: usize = 240;
pub const HEIGHT: usize = 160;

/// The message box rectangle (`0x7E8690`: x0, y0, x1, y1).
const MESSAGE_BOX_RECT: usize = 0x7E_8690;

/// The text table: 977 strings per language, the first row language-independent.
const TEXT_TABLE: usize = 0x7E_86A0;

/// What the primitives read besides the page: the ROM's tables, the language (`0x03005600`) and the page pitch.
#[derive(Clone, Copy)]
pub struct Ctx<'a> {
    pub rom: &'a [u8],
    pub language: u32,
    pub pitch: i32,
}

impl Ctx<'_> {
    /// Font id 0xC..=0xF (the others draw nothing). Id 0xF takes the glyphs of material 14, as the text functions do.
    pub fn font(&self, id: u32) -> Option<Font> {
        let f = id.checked_sub(0xC).filter(|&f| f < 4)? as usize;
        Some(ui::font(self.rom, f, ui::FONT_MATERIALS[f]))
    }

    /// String `key` of the text table in the current language (a language above 4 reads the first row).
    pub fn table_text(&self, key: u32) -> Vec<u8> {
        // The text functions' jump table: language 0..4 read rows 1, 2, 4, 3, 5 (the row order is not the language's).
        let row = [1, 2, 4, 3, 5].get(self.language as usize).copied().unwrap_or(0);
        let at = TEXT_TABLE + 4 * (977 * row + key as usize);
        let s = u32::from_le_bytes(self.rom[at..at + 4].try_into().unwrap()) as usize & 0x1FF_FFFF;
        let n = self.rom[s..].iter().position(|&b| b == 0).unwrap();
        self.rom[s..s + n].to_vec()
    }

    /// The text of a `text_menu`-style argument: a key up to 0xFFFF, else a pointer `pointer` reads.
    pub fn text(&self, arg: u32, pointer: impl FnOnce(u32) -> Vec<u8>) -> Vec<u8> {
        if arg > 0xFFFF {
            pointer(arg)
        } else {
            self.table_text(arg)
        }
    }

    /// `text_menu` (`0x08141578`): `text` in font `font` at (`x`, `y`) of `page`, `align` 1 centre, −1 right, else left, colour offset `colour`. Returns the text's width; draws nothing (and returns 0) for a `y`
    /// above 160 or a font that does not exist.
    #[allow(clippy::too_many_arguments)] // mirrors the game's argument list
    pub fn text_menu(&self, page: &mut [u8], font: u32, text: &[u8], x: i32, y: u32, align: i32, colour: i16) -> u32 {
        let Some(f) = self.font(font).filter(|_| y < 0xA1) else {
            return 0;
        };
        let align = match align {
            1 => Align::Centre,
            -1 => Align::Right,
            _ => Align::Left,
        };
        f.draw(self.rom, page, self.pitch as usize, x, y as i32, text, colour, align);
        f.measure(self.rom, text) as u32
    }

    /// `text_menu_wrapped_colour` (`0x08141B40`): word-wrapped text from (`x`, `y`), at most `width` pixels and
    /// `lines` lines, left aligned.
    #[allow(clippy::too_many_arguments)]
    pub fn text_wrapped(
        &self,
        page: &mut [u8],
        font: u32,
        text: &[u8],
        x: i32,
        y: i32,
        width: i32,
        lines: i32,
        colour: i16,
    ) {
        if let Some(f) = self.font(font) {
            f.draw_wrapped(self.rom, page, self.pitch as usize, x, y, text, width, lines, colour);
        }
    }

    /// `text_box` (`0x08141C88`; `text_menu_7`, `0x081419C0`, is this on both pages): word-wrapped lines centred
    /// on `x`.
    #[allow(clippy::too_many_arguments)]
    pub fn text_box(
        &self,
        page: &mut [u8],
        font: u32,
        text: &[u8],
        x: i32,
        y: i32,
        width: i32,
        lines: i32,
        colour: i16,
    ) {
        if let Some(f) = self.font(font) {
            f.draw_centred(self.rom, page, self.pitch as usize, x, y, text, width, lines, colour);
        }
    }

    /// `menu_blit_material` (`0x08136D74`) or, with `alt`, `menu_blit_material_alt` (`0x08136E60`): menu material
    /// `material` at (`x`, `y`). Returns the material's texel offset when it is packed (the game unpacks it into a
    /// heap buffer first).
    pub fn blit_material(&self, page: &mut [u8], material: u32, x: i32, y: i32, alt: bool) -> Option<usize> {
        let m = ui::materials(self.rom, ui::MENU_MATERIALS)[material as usize];
        let pixels = ui::pixels_8bpp(self.rom, ui::MENU_TEXELS, &m);
        let pitch = self.pitch as usize;
        if alt {
            ui::blit_halves(page, pitch, x, y, &pixels, m.width, m.height);
        } else {
            ui::blit(page, pitch, x, y, &pixels, m.width, m.height, 0);
        }
        (m.kind & 0x40 != 0).then_some(m.offset)
    }

    /// `menu_button_prompts` (`0x0812BD60`): the prompt bar, `left` and `right` text keys (−1 for none) and `key` for
    /// the bottom-left prompt. Returns the blits' unpack offsets, in order.
    pub fn button_prompts(&self, page: &mut [u8], st: &MenuState, left: u32, right: u32, key: u32) -> Vec<usize> {
        let mut unpacks = Vec::new();
        if st.g.message_box >= 0 {
            return unpacks;
        }
        let text = |page: &mut [u8], arg: u32, x: i32, y: u32, align: i32| {
            self.text_menu(page, 0xE, &self.table_text(arg), x, y, align, 0)
        };
        let held = |i: usize| st.profile.repeats[i] > 0;
        if left != u32::MAX {
            let w = text(page, left, 0xEE, 0x93, -1) as i32;
            unpacks.extend(self.blit_material(page, if held(4) { 0x9D } else { 0x9C }, 0xDD - w, 0x8F, false));
        }
        if key != u32::MAX {
            let (y_icon, y_text) = if right == u32::MAX { (0x91, 0x93) } else { (0x84, 0x87) };
            unpacks.extend(self.blit_material(page, if held(6) { 0xA1 } else { 0xA0 }, 1, y_icon, false));
            text(page, key, 0x12, y_text, 0);
        }
        if right != u32::MAX && st.g.back_top >= 0 {
            unpacks.extend(self.blit_material(page, if held(5) { 0x9F } else { 0x9E }, 1, 0x8F, false));
            text(page, right, 0x12, 0x93, 0);
        }
        unpacks
    }

    /// `message_box_draw` (`0x0813550C`): the open message box (`message_box` 1: OK, 2: yes/no) with its text
    /// (`message_text`, a key or a pointer `pointer` reads) and number (`message_arg`, −1 for none). Returns the
    /// blits' unpack offsets; the number's division leaves its remainder in `st.g.div_remainder`.
    pub fn message_box(&self, page: &mut [u8], st: &mut MenuState, pointer: impl FnOnce(u32) -> Vec<u8>) -> Vec<usize> {
        let r = |i: usize| i32::from_le_bytes(self.rom[MESSAGE_BOX_RECT + 4 * i..][..4].try_into().unwrap());
        let (x0, y0, x1, y1) = (r(0), r(1), r(2), r(3));
        let (cx, icons) = ((x0 + x1) >> 1, y1 - 0x10);
        let mut unpacks: Vec<usize> = self.blit_material(page, 6, x0, y0, false).into_iter().collect();
        let text = self.text(st.g.message_text, pointer);
        self.text_box(page, 0xD, &text, cx, y0 + 0x1C, x1 - x0, 3, 8);
        if st.g.message_arg != u32::MAX {
            let n = number_text(&mut st.g.div_remainder, st.g.message_arg as i32);
            self.text_box(page, 0xC, &n, cx, y0 + 0x30, x1 - x0, 3, 8);
        }
        let label = |page: &mut [u8], key: u32, x: i32, align: i32| {
            self.text_menu(page, 0xD, &self.table_text(key), x, (y1 - 0xD) as u32, align, 0);
        };
        match st.g.message_box {
            1 => {
                unpacks.extend(self.blit_material(page, 0x9C, cx - 0x14, icons, false));
                label(page, 0x174, cx, 0);
            }
            2 => {
                unpacks.extend(self.blit_material(page, 0x9E, x0 + 2, icons, false));
                label(page, 0x16F, x0 + 0x14, 0);
                unpacks.extend(self.blit_material(page, 0x9C, x1 - 0x12, icons, false));
                label(page, 0x3C2, x1 - 0x14, -1);
            }
            _ => {}
        }
        unpacks
    }
}

/// What a typed menu screen presents: two 240×160 8bpp pages in the game's mode 4, the page flip (DISPCNT bit 4
/// selects the page shown; the game draws on the other), the BG and OBJ palettes and the shadow OAM. Nothing here
/// has the GBA's memory layout.
#[derive(Clone)]
pub struct Screen {
    pub pages: [Vec<u8>; 2],
    /// `DISPCNT`: bit 4 is the frame select.
    pub dispcnt: u16,
    /// BG palette, then OBJ palette (256 BGR555 colours each).
    pub palette: [u16; 512],
    /// Shadow OAM: 128 entries of attr0, attr1, attr2 and the affine parameter.
    pub oam: [[u16; 4]; 128],
}

impl Default for Screen {
    fn default() -> Self {
        Screen {
            pages: [vec![0; WIDTH * HEIGHT], vec![0; WIDTH * HEIGHT]],
            dispcnt: 0x0404,
            palette: [0; 512],
            oam: [[0; 4]; 128],
        }
    }
}

impl Screen {
    /// The page being shown.
    pub fn shown(&self) -> &[u8] {
        &self.pages[(self.dispcnt >> 4 & 1) as usize]
    }

    /// The page the game draws on (the one not shown).
    pub fn page(&mut self) -> &mut [u8] {
        &mut self.pages[(self.dispcnt >> 4 & 1) as usize ^ 1]
    }

    /// Shows the page just drawn.
    pub fn flip(&mut self) {
        self.dispcnt ^= 0x10;
    }

    /// `fill_rect8` on the drawing page (`x0, y0, x1, y1`; `colour` a palette index), as on BG VRAM.
    pub fn fill_rect(&mut self, rect: [i32; 4], colour: u8) {
        ui::fill_rect8(self.page(), WIDTH as i32, rect, colour as u32 * 0x0101_0101, true);
    }

    /// Draws with the game's primitives on the drawing page.
    pub fn draw<R>(&mut self, ctx: &Ctx, f: impl FnOnce(&Ctx, &mut [u8]) -> R) -> R {
        f(ctx, self.page())
    }
}
