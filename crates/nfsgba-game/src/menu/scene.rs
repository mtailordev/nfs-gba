//! The menu scene and its sprites on typed state (batch U7b): what `load_menu_descriptor`, `menu_scene_setup_a`/`_b`,
//! `sprite_screen_*`, `oam_reset`, `unpack_to_buffer`, `intro_page_setup`, `health_screen_image`, `menu_scene_free`
//! and `copy_palette_to_ram` leave in a [`Screen`] and in the scene's own data. The game's heap blocks (the sprite
//! objects, the unpack buffer, the effect list) are owned values here and its frees are drops; the scene's world
//! struct is not modelled beyond what the screen shows (`race_load_level`'s pointers and buffers are invisible in
//! a menu, U7).
//!
//! Checked against the game's code by `tools/oracle/scene.py` (`menus3/scene-*.jsonl`, replay test
//! `scenes_match_the_game`).

use nfsgba_formats::ui::{self, Object, SpriteBank};

use super::draw::Screen;

/// The menu level descriptor (`0x087F2FE8`), the menu materials, texels and palettes (ROM offsets).
pub const MENU_DESCRIPTOR: usize = 0x7F_2FE8;
pub const MENU_MATERIALS: usize = 0x34_5114;
pub const MENU_TEXELS: usize = 0x16_C244;
pub const MENU_PALETTES: usize = 0x33_EF14;

/// A sprite screen holds 0x37 objects (`sprite_screen_alloc`).
pub const OBJECTS: usize = 0x37;
/// Sprite screen number meaning "none" in `menu_scene_setup`.
pub const NO_SPRITES: u32 = 0xFFFF;

/// The effect list (`effect_list_init`, `0x0816200C`): `capacity` zeroed 0x14-byte entries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectList {
    pub capacity: u16,
    pub limit: u16,
    pub items: Vec<[u8; 0x14]>,
}

impl EffectList {
    pub fn new(capacity: u16, limit: u16) -> Self {
        EffectList {
            capacity,
            limit,
            items: vec![[0; 0x14]; capacity as usize],
        }
    }
}

/// One menu screen's scene: sprite tables and objects, the effect list, the background's unpacked pixels and the
/// palette sources.
#[derive(Clone, Default)]
pub struct Scene {
    pub bank: Option<SpriteBank>,
    /// The current sprite screen (`sprite_screen + 0x18`).
    pub sprite_screen: usize,
    pub objects: Vec<Object>,
    pub effects: Option<EffectList>,
    /// `*0x030057F0`: the last unpacked background material (`unpack_to_buffer`).
    pub buffer: Vec<u8>,
    /// World `+0x30` and `+0x34`: ROM offsets of the base and OBJ palette sources.
    pub base_palette: usize,
    pub obj_palette: usize,
    /// The two base palette buffers `set_base_palette` fills, and the flag it raises.
    pub palettes: [Vec<u16>; 2],
    pub palette_dirty: bool,
}

/// `oam_hide_range` (`0x08160FD0`): `n` entries from `first` off the screen (y 160, affine off).
fn hide_range(s: &mut Screen, first: usize, n: usize) {
    for a in &mut s.oam[first..first + n] {
        a[0] = a[0] & 0xFC00 | 0xA0;
    }
}

/// `oam_reset` (`0x0812B10C`): every OAM entry hidden, 64x64 at x 0x3C, tile 0 of an OBJ tile base of 0x200; OBJs
/// on, 1-D mapping. (The game copies the hidden shadow to the OAM first and then sets the fields in the shadow
/// only; the per-frame copy presents them.)
pub fn oam_reset(s: &mut Screen) {
    s.tile_base = 0x200;
    s.oam = [[0xA0, 0xC03C, 0x200, 0]; 128];
    s.dispcnt |= 0x1040;
}

/// `copy_palette_to_ram` (`0x0815DFD8`): 256 colours to BG palette RAM; -1 for a null source.
pub fn copy_palette_to_ram(s: &mut Screen, src: Option<&[u16]>) -> i32 {
    match src {
        Some(p) => {
            s.palette[..256].copy_from_slice(&p[..256]);
            0
        }
        None => -1,
    }
}

impl Scene {
    fn bank(&mut self, rom: &[u8]) -> &SpriteBank {
        self.bank.get_or_insert_with(|| ui::sprite_bank(rom, MENU_DESCRIPTOR))
    }

    /// `set_base_palette` (`0x0812B21C`): 256 colours into both base palette buffers, palette marked dirty.
    pub fn set_base_palette(&mut self, rom: &[u8], at: usize) {
        let p = ui::palette_at(rom, at);
        self.palettes = [p.clone(), p];
        self.palette_dirty = true;
    }

    /// `sprite_screen_alloc` (`0x08161EA0`): the objects hidden, the first `count` of the screen visible.
    pub fn sprite_screen_alloc(&mut self, rom: &[u8], s: &mut Screen) {
        let i = self.sprite_screen;
        let count = self.bank(rom).screens[i].count;
        hide_range(s, 0, OBJECTS);
        self.objects = (0..OBJECTS)
            .map(|i| Object {
                flags: (i < count) as u16,
                ..Object::default()
            })
            .collect();
    }

    /// `sprite_screen_select` (`0x08161EEC`).
    pub fn sprite_screen_select(&mut self, rom: &[u8], s: &mut Screen, index: usize) {
        self.sprite_screen = index;
        self.sprite_screen_alloc(rom, s);
    }

    /// `sprite_screen_update` (`0x08161C24`): the objects into the shadow OAM and the tiles they need into OBJ
    /// VRAM.
    pub fn sprite_screen_update(&mut self, rom: &[u8], s: &mut Screen, init: bool) {
        let bank = self.bank(rom).clone();
        let uploads = ui::update_sprites(
            rom,
            &bank,
            self.sprite_screen,
            &mut self.objects,
            &mut s.oam,
            init,
            s.tile_base,
        );
        for u in uploads {
            // NOT 1:1 (N1): a tile below the OBJ base (0x200) would land in the BG pages; the port drops it.
            if let Some(at) = (32 * u.tile)
                .checked_sub(0x4000)
                .filter(|&at| at + u.len <= s.obj_tiles.len())
            {
                s.obj_tiles[at..at + u.len].copy_from_slice(&rom[u.src..u.src + u.len]);
            }
        }
    }

    /// `hud_reset` on a menu (HUD mode 0): the screen's objects visible and semi-transparent, scale, frame and
    /// rotation cleared.
    fn hud_reset(&mut self, rom: &[u8]) {
        let i = self.sprite_screen;
        let count = self.bank(rom).screens[i].count;
        for o in self.objects.iter_mut().take(count) {
            *o = Object {
                flags: o.flags | 3,
                scale: [0, 0],
                frame: 0,
                angle: 0,
                ..*o
            };
        }
    }

    /// `load_menu_descriptor` (`0x08139C8C`): blend registers, both pages cleared, OAM reset, the descriptor's
    /// palettes, sprite screen 0 drawn once, the effect list. (The copies of ROM code to IWRAM, the world pointers
    /// and buffers of `race_load_level` and the HUD's message state have no effect on a menu screen.)
    pub fn load_menu_descriptor(&mut self, rom: &[u8], s: &mut Screen) {
        let u32_at =
            |o: usize| u32::from_le_bytes(rom[MENU_DESCRIPTOR + o..][..4].try_into().unwrap()) as usize & 0x1FF_FFFF;
        let u16_at = |o: usize| u16::from_le_bytes(rom[MENU_DESCRIPTOR + o..][..2].try_into().unwrap()) as usize;
        s.bldcnt = 0x3F3F;
        s.bldalpha = 0x0C0F;
        s.pages.iter_mut().for_each(|p| p.fill(0));
        oam_reset(s);
        self.base_palette = u32_at(0);
        self.obj_palette = u32_at(4) + 2 * u16_at(0x58);
        self.set_base_palette(rom, self.base_palette + 2 * u16_at(0x5A));
        self.sprite_screen = 0;
        self.sprite_screen_alloc(rom, s);
        self.sprite_screen_update(rom, s, true);
        self.hud_reset(rom);
        self.effects = Some(EffectList::new(0x20, 0x7F));
        s.dispstat &= !0x20;
    }

    /// `unpack_to_buffer` (`0x08163D30`) of menu material `material` when it is packed.
    fn unpack_material(&mut self, rom: &[u8], material: usize) {
        let m = ui::materials(rom, MENU_MATERIALS)[material];
        if m.kind & 0x40 != 0 {
            self.buffer = ui::unpack(rom, MENU_TEXELS + m.offset);
        }
    }

    /// `menu_scene_setup_a` (`first`, `0x081370D4`) or `_b` (`0x081371A4`): the screen's background material
    /// (unpacked into `buffer` when packed), its menu palette (`palette`) and OBJ palette 1 (scaled to black), and
    /// sprite screen `sprite` (`NO_SPRITES`: none). `_a` first blacks BG palette RAM and loads the descriptor;
    /// the caller clears `MENU_EXIT` and waits the VBlank.
    pub fn setup(&mut self, rom: &[u8], s: &mut Screen, first: bool, material: u32, palette: u32, sprite: u32) {
        if first {
            s.palette[..256].fill(0);
            self.load_menu_descriptor(rom, s);
        }
        self.unpack_material(rom, material as usize);
        self.base_palette = MENU_PALETTES + palette.wrapping_mul(0x200) as usize;
        self.obj_palette = MENU_PALETTES + 0x200;
        s.palette[256..].fill(0);
        self.set_base_palette(rom, self.base_palette);
        if sprite != NO_SPRITES {
            self.sprite_screen_select(rom, s, sprite as usize);
            self.sprite_screen_update(rom, s, true);
        }
    }

    /// `health_screen_image` (`0x0813644C`): the language's health-and-safety material into the buffer.
    pub fn health_screen_image(&mut self, rom: &[u8], material: u32) {
        self.unpack_material(rom, material as usize);
    }

    /// `intro_page_setup` (`0x081364C4`): the unpacked background onto the drawing page (a buffer shorter than the
    /// page leaves the rest as it is; the game copies what follows in its heap).
    pub fn intro_page_setup(&self, page: &mut [u8]) {
        let n = page.len().min(self.buffer.len());
        page[..n].copy_from_slice(&self.buffer[..n]);
    }

    /// `menu_scene_free` (`0x08139B7C`): the sprites hidden, every block dropped, the blend-interrupt flag and
    /// timer 3 off.
    pub fn free(&mut self, s: &mut Screen) {
        hide_range(s, 0, OBJECTS);
        self.objects = Vec::new();
        self.effects = None;
        self.buffer = Vec::new();
        s.dispstat &= !0x20;
        s.timer3 = 0;
    }
}

#[cfg(test)]
#[path = "scene_test.rs"]
mod tests;
