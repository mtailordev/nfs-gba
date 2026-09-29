//! The game's top level and its menus (`docs/formats/ui.md`, "Menus"): palette fades, the game-state step, the
//! menu dispatcher and the screens. Every function follows the game's code and is checked against it through
//! the function oracle (`tools/ui_menu_oracle.py`).

/// One fade-in step of `n` BGR555 colours from `pal[first..]` towards `target[..]` (read from its start), as
/// `palette_fade_in_step` (`0x0815E530`, a buffer), `FUN_0815E2F0` (BG palette RAM) and `FUN_08161838` (OBJ palette
/// RAM) do it: each channel below its target gains `step`, then is capped at the target. The OBJ variant reads
/// `target` from `first` as well, so its callers pass `&target[first..]`.
pub fn fade_in_step(pal: &mut [u16], target: &[u16], first: usize, n: usize, step: u32) {
    for (c, &t) in pal[first..first + n].iter_mut().zip(target) {
        let ch = |v: u16, s: u32| (v as u32 >> s) & 0x1F;
        let (r, g, b) = [0, 5, 10]
            .map(|s| {
                let (mut c, t) = (ch(*c, s), ch(t, s));
                if c < t {
                    c += step;
                }
                c.min(t)
            })
            .into();
        *c = (b << 10 | g << 5 | r) as u16;
    }
}

/// One fade-out step of `n` colours from `pal[first..]`, as `palette_fade_out_step` (`0x0815E4D4`, a buffer),
/// `FUN_0815E290` (BG palette RAM) and `FUN_081617D4` (OBJ palette RAM): each non-zero channel loses `step`, floored
/// at 0. Bit 15 is dropped, as the game rebuilds the colour from the three channels.
pub fn fade_out_step(pal: &mut [u16], first: usize, n: usize, step: u32) {
    for c in &mut pal[first..first + n] {
        let [r, g, b] = [0, 5, 10].map(|s| ((*c as u32 >> s) & 0x1F).saturating_sub(step));
        *c = (b << 10 | g << 5 | r) as u16;
    }
}

// The game's globals this module reads and writes (docs/engine/address-map.md).
pub const WORLD: u32 = 0x0300_00C0;
pub const GAME_STATE: u32 = 0x0300_5808; // 0 boot, 1 menus, 4 race start, 5 race
pub const SCREEN: u32 = 0x0300_5944; // menu screen 0..=0x30; 0x80/0x81: leave for a race
pub const SCREEN_CHANGED: u32 = 0x0300_5948; // set by `mark_screen_changed`; the next draw is a full one
pub const EXIT_SCREEN: u32 = 0x0300_594C; // whose exit handler runs once the menus are left (-1: none)
pub const SCREEN_ENTERED: u32 = 0x0300_5938;
pub const MENU_EXIT: u32 = 0x0300_5780; // 7: leave the menus for the race once the fade has finished
pub const FADE: u32 = 0x0300_5630; // palette fade: > 0 fading in, < 0 fading out, by 2 per frame
pub const BACK_TOP: u32 = 0x0300_593C; // i8: top of the back stack (profile + 0x344)
pub const BACK_TOP_SAVED: u32 = 0x0300_5940;
pub const MESSAGE_BOX: u32 = 0x0300_59F0; // i32: open message box (< 0: none); 2 also takes B
pub const MESSAGE_RESULT: u32 = 0x0300_59F4;
pub const KEYS: u32 = 0x0300_64C0; // u16: keys pressed this frame
pub const PROFILE: u32 = 0x0300_56EC; // pointer to the profile (0x02000808)
pub const ROUTE: u32 = 0x0300_5388;
pub const REVERSE: u32 = 0x0300_5610;
pub const CAREER: u32 = 0x0300_00A0;
pub const PLAYER_CAR: u32 = 0x0300_5718;
pub const RAND_INDEX: u32 = 0x0300_64C8;
pub const FRAME_COUNTER: u32 = 0x0300_5628;
pub const LEVEL_DESC: u32 = 0x0300_5620;
pub const GRADIENT_BUFFER: u32 = 0x0300_53B8; // pointer
pub const SECOND_PALETTE: u32 = 0x0300_577C; // pointer to the second base palette buffer
pub const PALETTE_DIRTY: u32 = 0x0300_563C;

/// The game's address space as the menu code uses it: ROM, EWRAM, IWRAM, IO, palette RAM, VRAM and OAM, laid
/// out as on the GBA so every step compares byte for byte with the reference build.
///
/// Calls to game functions that are not ported yet go through [`Gba::unported`]: they are logged with their
/// arguments and return `returns[address]` (0 if absent), exactly as the oracle's stubs do in the tests.
#[derive(Clone)]
pub struct Gba {
    pub rom: Vec<u8>,
    pub ewram: Vec<u8>,
    pub iwram: Vec<u8>,
    pub io: Vec<u8>,
    pub pal: Vec<u8>,
    pub vram: Vec<u8>,
    pub oam: Vec<u8>,
    pub calls: Vec<(u32, Vec<u32>)>,
    pub returns: std::collections::HashMap<u32, u32>,
    /// C strings the game builds on its stack (numbers, times) and passes to unported text primitives by pointer:
    /// the port passes `STACK_TEXT + index` instead, and the tests compare the contents.
    pub texts: Vec<Vec<u8>>,
    /// The `texts` a drawing primitive read: they are not arguments of a logged call.
    pub consumed: std::collections::HashSet<usize>,
    /// The garage screens (Kind18) run typed when the flow reaches them (the garage oracle set); the top-level sets
    /// keep them stubbed.
    pub garage_typed: bool,
    /// `map_draw` runs typed (the map oracle set).
    pub map_typed: bool,
}

/// Where [`Gba::text_arg`] strings "live": pointer arguments from here up index `Gba::texts`.
pub const STACK_TEXT: u32 = 0x0300_7400;

impl Gba {
    /// An mGBA dump (`<prefix>.{wram,iwram,io,palette,vram,oam}.bin`, `tools/mgba_ctl.py dump`).
    pub fn from_dump(rom: Vec<u8>, prefix: &std::path::Path) -> std::io::Result<Gba> {
        let d = nfsgba_formats::Dump::load(prefix)?.require(&["io", "palette", "vram", "oam"])?;
        Ok(Gba {
            rom,
            ewram: d.ewram,
            iwram: d.iwram,
            io: d.io,
            pal: d.palette,
            vram: d.vram,
            oam: d.oam,
            calls: Vec::new(),
            returns: Default::default(),
            texts: Vec::new(),
            consumed: Default::default(),
            garage_typed: false,
            map_typed: false,
        })
    }

    /// A stack string argument (see `texts`).
    pub fn text_arg(&mut self, s: Vec<u8>) -> u32 {
        self.texts.push(s);
        STACK_TEXT + self.texts.len() as u32 - 1
    }

    fn at(&self, addr: u32) -> (&[u8], usize) {
        match addr >> 24 {
            0x02 => (&self.ewram, (addr & 0x3_FFFF) as usize),
            0x03 => (&self.iwram, (addr & 0x7FFF) as usize),
            0x04 => (&self.io, (addr & 0x3FF) as usize),
            0x05 => (&self.pal, (addr & 0x3FF) as usize),
            0x06 => (&self.vram, vram_offset(addr)),
            0x07 => (&self.oam, (addr & 0x3FF) as usize),
            0x08 | 0x09 => (&self.rom, (addr & 0x1FF_FFFF) as usize),
            _ => panic!("read from unmapped address {addr:#010x}"),
        }
    }

    /// Writes below EWRAM are ignored, as on the GBA.
    fn at_mut(&mut self, addr: u32) -> Option<(&mut [u8], usize)> {
        Some(match addr >> 24 {
            0x00 | 0x01 => return None,
            0x02 => (&mut self.ewram, (addr & 0x3_FFFF) as usize),
            0x03 => (&mut self.iwram, (addr & 0x7FFF) as usize),
            0x04 => (&mut self.io, (addr & 0x3FF) as usize),
            0x05 => (&mut self.pal, (addr & 0x3FF) as usize),
            0x06 => (&mut self.vram, vram_offset(addr)),
            0x07 => (&mut self.oam, (addr & 0x3FF) as usize),
            _ => panic!("write to non-RAM address {addr:#010x}"),
        })
    }

    pub fn u8(&self, addr: u32) -> u8 {
        let (m, o) = self.at(addr);
        m[o]
    }
    pub fn i8(&self, addr: u32) -> i8 {
        self.u8(addr) as i8
    }
    pub fn u16(&self, addr: u32) -> u16 {
        let (m, o) = self.at(addr);
        u16::from_le_bytes([m[o], m[o + 1]])
    }
    pub fn u32(&self, addr: u32) -> u32 {
        let (m, o) = self.at(addr);
        u32::from_le_bytes(m[o..o + 4].try_into().unwrap())
    }
    pub fn i32(&self, addr: u32) -> i32 {
        self.u32(addr) as i32
    }
    pub fn set_u8(&mut self, addr: u32, v: u8) {
        if let Some((m, o)) = self.at_mut(addr) {
            m[o] = v;
        }
    }
    pub fn set_u16(&mut self, addr: u32, v: u16) {
        if let Some((m, o)) = self.at_mut(addr) {
            m[o..o + 2].copy_from_slice(&v.to_le_bytes());
        }
    }
    pub fn set_u32(&mut self, addr: u32, v: u32) {
        if let Some((m, o)) = self.at_mut(addr) {
            m[o..o + 4].copy_from_slice(&v.to_le_bytes());
        }
    }
    pub fn u16s(&self, addr: u32, n: usize) -> Vec<u16> {
        (0..n as u32).map(|i| self.u16(addr + 2 * i)).collect()
    }
    pub fn set_u16s(&mut self, addr: u32, v: &[u16]) {
        for (i, &c) in v.iter().enumerate() {
            self.set_u16(addr + 2 * i as u32, c);
        }
    }

    /// A call to a game function that is not ported yet: logged, answered from `returns`.
    pub fn unported(&mut self, f: u32, args: &[u32]) -> u32 {
        self.calls.push((f, args.to_vec()));
        self.returns.get(&f).copied().unwrap_or(0)
    }
}

/// VRAM is 96 KiB mirrored in 128 KiB blocks, the last 32 KiB repeating the OBJ area.
fn vram_offset(addr: u32) -> usize {
    let o = (addr & 0x1_FFFF) as usize;
    if o >= 0x1_8000 { o - 0x8000 } else { o }
}

mod adapt;
pub mod boot;
pub mod draw;
mod event;
pub mod flow;
mod garage;
mod hints;
mod intro;
mod list;
mod map;
mod results;
pub mod save;
pub mod scene;
mod setup;
pub mod text;
pub mod typed;
pub use adapt::*;

/// The eight kinds of menu screen. Each has an enter, update, draw and exit handler; the screens share them
/// through four jump tables (`enter_kind`, `update_kind`, `draw_kind`, `exit_kind`). Names of the kinds not yet
/// identified follow their first screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Lists and menus: 0–6, 9, 27–30, 35, 36, 45, 46.
    List,
    /// 7, 8, 14, 17.
    Kind7,
    /// Career zones: 11, 12.
    Career,
    /// A career event: 13.
    Event,
    /// Race setup (Quick Play settings): 10, 15, 16.
    Setup,
    /// 18, 19, 20.
    Kind18,
    /// Boot and intro screens: 21–26, 37, 47, 48.
    Intro,
    /// 38–43.
    Kind38,
}

impl Kind {
    /// Handler addresses: enter, update, draw, exit (not ported yet; `Gba::unported`).
    pub const fn handlers(self) -> [u32; 4] {
        match self {
            Kind::List => [0x0812_FE38, 0x0813_03E8, 0x0813_0D8C, 0x0813_14A4],
            Kind::Kind7 => [0x0812_E80C, 0x0812_E3D4, 0x0812_E5AC, 0x0812_E850],
            Kind::Career => [0x0812_F204, 0x0812_F364, 0x0812_F450, 0x0812_FC6C],
            Kind::Event => [0x0812_E308, 0x0812_DAFC, 0x0812_DD80, 0x0812_E380],
            Kind::Setup => [0x0813_28F4, 0x0813_2AB8, 0x0813_3074, 0x0813_36BC],
            Kind::Kind18 => [0x0813_3708, 0x0813_38E0, 0x0813_3F2C, 0x0813_48D8],
            Kind::Intro => [0x0813_15A0, 0x0813_18E4, 0x0813_1FE0, 0x0813_2780],
            Kind::Kind38 => [0x0813_4DF0, 0x0813_4EB8, 0x0813_5340, 0x0813_54FC],
        }
    }
}

/// The screen's kind in the update table (`0x0812B980`, 49 entries); `None`: no handler.
pub fn update_kind(screen: u32) -> Option<Kind> {
    Some(match screen {
        0..=6 | 9 | 27..=30 | 35 | 36 | 45 | 46 => Kind::List,
        7 | 8 | 14 | 17 => Kind::Kind7,
        11 | 12 => Kind::Career,
        13 => Kind::Event,
        10 | 15 | 16 => Kind::Setup,
        18..=20 => Kind::Kind18,
        21..=26 | 37 | 47 | 48 => Kind::Intro,
        38..=43 => Kind::Kind38,
        _ => return None,
    })
}

/// The draw table (`0x0812D370`): as `update_kind` except that 40 and 42 draw nothing.
pub fn draw_kind(screen: u32) -> Option<Kind> {
    update_kind(screen).filter(|_| !matches!(screen, 40 | 42))
}

/// The exit table (`0x0812B640`): as `update_kind` except that 40–42 have no exit handler.
pub fn exit_kind(screen: u32) -> Option<Kind> {
    update_kind(screen).filter(|_| !matches!(screen, 40..=42))
}

/// The enter table (`0x0812B454`, 48 entries: screen 48 has none): as `update_kind` except that 40–42 have no
/// handler. Screens 7, 8, 15 and 28 run extra code first and 40 starts a fade-out (`enter_screen`).
pub fn enter_kind(screen: u32) -> Option<Kind> {
    update_kind(screen).filter(|_| !matches!(screen, 40..=42 | 48))
}

// The screens are all typed (`list`, `setup`, `hints`, `intro`, `map`, `event`, `results` on `MenuState`); what stays on
// `Gba` is the scene setup, the palette and VRAM helpers below (menu drawing, FIDELITY U7), the garage screens'
// (Kind18) unported handlers and the oracle-test adapter (`adapt.rs`).

/// Runs a screen handler (`phase`: 0 enter, 1 update, 2 draw, 3 exit): the ported ones in Rust, the others as
/// `Gba::unported` calls.
pub fn run_handler(g: &mut Gba, kind: Kind, phase: usize, args: &[u32]) -> u32 {
    if flow::is_typed(kind, phase) {
        return typed(g, |st, h| flow::run_typed(st, h, kind, phase, args));
    }
    stubbed_handler(g, kind, phase, args)
}

/// A handler that is not ported, or that the top-level oracle sets keep stubbed (Kind18: `tools/oracle/garage.py`
/// checks it on its own): one logged call.
pub fn stubbed_handler(g: &mut Gba, kind: Kind, phase: usize, args: &[u32]) -> u32 {
    match (kind, phase) {
        // Every kind's exit handler (`0x08132780`, `0x0812E850`, …) is only this call: the menu scene's teardown.
        (_, 3) => g.unported(SCENE_EXIT, &[WORLD]),
        _ => g.unported(kind.handlers()[phase], args),
    }
}

const SCENE_EXIT: u32 = 0x0813_72D8; // (world): FUN_08139B7C, the menu scene's teardown
const LOAD_MENU_DESCRIPTOR: u32 = 0x0813_9C8C; // (world, descriptor)
const UNPACK_TO_BUFFER: u32 = 0x0816_3D30; // (packed texels, destination): the ring decoder on the game's heap (U7)
const SPRITE_SCREEN_SELECT: u32 = 0x0816_1EEC; // (sprite screen, index)
const SPRITE_SCREEN_UPDATE: u32 = 0x0816_1C24; // (sprite screen, init)
const MENU_DESCRIPTOR: u32 = 0x087F_2FE8;
const MENU_MATERIALS: u32 = 0x0834_5114;
const MENU_TEXELS: u32 = 0x0816_C244;
const MENU_PALETTES: u32 = 0x0833_EF14;

/// `menu_scene_setup_a` (`0x081370D4`, a screen entered from outside the menus: `SCREEN_ENTERED` 0) and `_b`
/// (`0x081371A4`): the screen's background menu material (unpacked when packed), its menu palette
/// (`0x33EF14 + palette·0x200` → world `+0x30`, the base palettes), OBJ palette 1 scaled to black, and its sprite
/// screen (`0xFFFF`: none). `_a` first waits a VBlank, blacks out BG palette RAM, clears `MENU_EXIT` and loads
/// the menu descriptor. This is where each screen's palette comes from (FIDELITY U4).
pub fn menu_scene_setup(g: &mut Gba, material: u32, palette: u32, sprite: u32) {
    let first = g.u32(SCREEN_ENTERED) == 0;
    menu_scene_setup_ab(g, first, material, palette, sprite);
}

/// `menu_scene_setup_a` (`first`) or `_b`, chosen by the caller.
pub fn menu_scene_setup_ab(g: &mut Gba, first: bool, material: u32, palette: u32, sprite: u32) {
    if first {
        g.unported(VBLANK_INTR_WAIT, &[]);
        fill_bg_palette(g, 0, 0, 0x100);
        g.set_u32(MENU_EXIT, 0);
        g.unported(LOAD_MENU_DESCRIPTOR, &[WORLD, MENU_DESCRIPTOR]);
    }
    let m = MENU_MATERIALS.wrapping_add(material.wrapping_mul(0x24));
    if g.u16(m + 2) & 0x40 != 0 {
        let (src, dst) = (g.u32(m + 8).wrapping_add(MENU_TEXELS), g.u32(0x0300_57F0));
        g.unported(UNPACK_TO_BUFFER, &[src, dst]);
    }
    g.set_u32(WORLD + 0x30, palette.wrapping_mul(0x200).wrapping_add(MENU_PALETTES));
    g.set_u32(WORLD + 0x34, MENU_PALETTES + 0x200);
    g.set_u32(WORLD + 0x20, MENU_MATERIALS);
    g.set_u32(WORLD, MENU_TEXELS);
    scale_obj_palette(g, MENU_PALETTES + 0x200, [0, 0, 0], 0, 0x100);
    set_base_palette(g, g.u32(WORLD + 0x30));
    if sprite != 0xFFFF {
        g.unported(SPRITE_SCREEN_SELECT, &[WORLD + 0xA4, sprite]);
        g.unported(SPRITE_SCREEN_UPDATE, &[WORLD + 0xA4, 1]);
    }
}

/// `FUN_0815E04C` (start, colour, count): BG palette entries from `start` set to `colour`.
fn fill_bg_palette(g: &mut Gba, start: u32, colour: u16, count: u16) {
    let mut i = start;
    for _ in 0..count {
        g.set_u16(0x0500_0000 + 2 * (i & 0xFFFF), colour);
        i = (i & 0xFFFF) + 1;
    }
}

/// `FUN_08161754` (source, r, g, b, first, n): OBJ palette entries `first..first + n` from `source`, each channel
/// times its factor / 256, capped at 31.
fn scale_obj_palette(g: &mut Gba, src: u32, k: [u32; 3], first: u32, n: u32) {
    for i in first..first + n {
        let c = g.u16(src + 2 * i) as u32;
        let [r, gr, b] = [0, 5, 10].map(|s| ((((c >> s) & 0x1F) * k[s as usize / 5]) >> 8).min(0x1F));
        g.set_u16(0x0500_0200 + 2 * i, (b << 10 | gr << 5 | r) as u16);
    }
}

/// `set_base_palette` (`0x0812B21C`): a 0x200-byte palette into both base palette buffers (`FUN_08160D18`, which
/// copies nothing when either pointer is null), then marks the palette dirty.
pub fn set_base_palette(g: &mut Gba, src: u32) {
    for dst in [g.u32(SECOND_PALETTE), g.u32(0x0300_55F0)] {
        copy_mem(g, dst, src, 0x200, 0x20);
    }
    g.set_u32(PALETTE_DIRTY, 1);
}

/// `FUN_08160D18` (dst, src, bytes, width): a forward copy in words (width 0x20), halfwords (0x10) or bytes (8);
/// nothing when a pointer is null or the size is below one unit.
pub fn copy_mem(g: &mut Gba, dst: u32, src: u32, n: u32, width: u32) {
    if dst == 0 || src == 0 {
        return;
    }
    match width {
        0x20 => (0..n >> 2).for_each(|i| g.set_u32(dst + 4 * i, g.u32(src + 4 * i))),
        0x10 => (0..n >> 1).for_each(|i| g.set_u16(dst + 2 * i, g.u16(src + 2 * i))),
        8 => (0..n).for_each(|i| g.set_u8(dst + i, g.u8(src + i))),
        _ => {}
    }
}

pub const TICKS: u32 = 0x0300_0044; // running tick counter: intro deadlines (profile + 0x3B0) compare with it
pub const LANGUAGE: u32 = 0x0300_5600;
pub const UNITS: u32 = 0x0300_0040;
pub const SAVE_BUFFER: u32 = 0x0300_57F4; // pointer passed to the EEPROM save routines
pub const VBLANK_INTR_WAIT: u32 = 0x0815_1454;

// Drawing primitives the draw handlers call; not ported onto `Gba` yet (their pixel output is exact in `ui.rs`).
pub const MENU_BLIT_MATERIAL: u32 = 0x0813_6D74; // (world, material, x, y)
pub const TEXT_MENU: u32 = 0x0814_1578; // (font, text key or pointer, x, y, colour/alignment, …)
pub const TEXT_MENU_WRAPPED: u32 = 0x0814_1B40; // (font, key, x, y, width, lines, colour)

const FLASH_BLINK: u32 = 0x10; // bit 4 of the frame counter (`flash`) blinks the cursors and PRESS START

// Helpers the career screens share.

const TEXT_BOX: u32 = 0x0814_1C88; // (font, text key or pointer, x, y, width, lines, colour)
const INTRO_PAGE_SETUP: u32 = 0x0813_64C4; // (unpack buffer): clears the page

const MENU_BLIT_MATERIAL_ALT: u32 = 0x0813_6E60; // (world, material, x, y)

// The hint and story pages' constants (`hints.rs`).
const CARBON_STOP_SOUND: u32 = 0x0813_6028; // (id)
const CARBON_PLAY_MUSIC: u32 = 0x0813_6054; // (id)
const SND_STOP_MUSIC: u32 = 0x0815_240C; // ()
const FILL_RECT: u32 = 0x0816_4BEC; // (rect on the stack: x0, y0, x1, y1; page, pitch, colour × 0x01010101)

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::flow::Host;
    use nfsgba_testkit::{dump, rom};

    /// Oracle cases saved by `tools/ui_menu_oracle.py <name>`.
    fn cases(name: &str) -> Option<Vec<serde_json::Value>> {
        let text = nfsgba_testkit::read_to_string(&format!("menus3/{name}.jsonl"))?;
        Some(text.lines().map(|l| serde_json::from_str(l).unwrap()).collect())
    }

    fn words(v: &serde_json::Value) -> Vec<u16> {
        let b: Vec<u8> = (0..v.as_str().unwrap().len() / 2)
            .map(|i| u8::from_str_radix(&v.as_str().unwrap()[2 * i..2 * i + 2], 16).unwrap())
            .collect();
        b.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
    }

    fn bytes(v: &serde_json::Value) -> Vec<u8> {
        let s = v.as_str().unwrap();
        (0..s.len() / 2)
            .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
            .collect()
    }

    /// Every RAM byte that differs between two states, by GBA address.
    fn changed(a: &Gba, b: &Gba) -> std::collections::BTreeMap<u32, u8> {
        let regions = [
            (0x0200_0000, &a.ewram, &b.ewram),
            (0x0300_0000, &a.iwram, &b.iwram),
            (0x0400_0000, &a.io, &b.io),
            (0x0500_0000, &a.pal, &b.pal),
            (0x0600_0000, &a.vram, &b.vram),
            (0x0700_0000, &a.oam, &b.oam),
        ];
        let mut out = std::collections::BTreeMap::new();
        for (base, x, y) in regions {
            out.extend(
                x.iter()
                    .zip(y.iter())
                    .enumerate()
                    .filter(|(_, (p, q))| p != q)
                    .map(|(i, (_, &q))| (base + i as u32, q)),
            );
        }
        out
    }

    /// Oracle cases of the game's top level with its unported callees stubbed (`tools/ui_menu_oracle.py
    /// toplevel`): same RAM writes, same stub calls with the same arguments, same result.
    #[test]
    fn top_level_matches_the_game() {
        replay("toplevel");
    }

    /// The same for the boot and intro screens (`intro_update` ported): `tools/ui_menu_oracle.py intro`.
    #[test]
    fn intro_screens_match_the_game() {
        replay("intro");
    }

    /// Kind7, the map screens, handler by handler: `tools/ui_menu_oracle.py kind7`.
    #[test]
    fn map_screens_match_the_game() {
        replay("kind7");
    }

    /// The career event screen (13): `tools/ui_menu_oracle.py event`.
    #[test]
    fn career_event_screen_matches_the_game() {
        replay("event");
    }

    /// The race results (11, 12): `tools/ui_menu_oracle.py career`.
    #[test]
    fn race_results_match_the_game() {
        replay("career");
    }

    /// The settings screens (10, 15, 16): `tools/ui_menu_oracle.py setup`.
    #[test]
    fn settings_screens_match_the_game() {
        replay("setup");
    }

    /// The hint and story pages (38..=43): `tools/ui_menu_oracle.py kind38`.
    #[test]
    fn hint_pages_match_the_game() {
        replay("kind38");
    }

    /// The List screens (menus, car select, garage lists, crew): `tools/ui_menu_oracle.py list`.
    #[test]
    fn list_screens_match_the_game() {
        replay("lists");
    }

    /// The garage screens (Kind18), the unlock rules, the new marks and the stat bars, on generated menu states:
    /// `tools/oracle/cases.py garage`.
    #[test]
    fn garage_screens_match_the_game() {
        replay("garage");
    }

    /// `map_draw` and the map screens' draw handler: `tools/oracle/cases.py garage` (the `mapdraw` set).
    #[test]
    fn map_drawing_matches_the_game() {
        replay("mapdraw");
    }

    const KINDS: [Kind; 8] = [
        Kind::List,
        Kind::Kind7,
        Kind::Career,
        Kind::Event,
        Kind::Setup,
        Kind::Kind18,
        Kind::Intro,
        Kind::Kind38,
    ];

    fn replay(name: &str) {
        let Some(rom) = rom() else { return };
        let Some(cases) = cases(name) else { return };
        let mut snaps = std::collections::HashMap::new();
        for (n, c) in cases.iter().enumerate() {
            let snap = c["snap"].as_str().unwrap();
            let base = snaps.entry(snap.to_owned()).or_insert_with(|| {
                let prefix = dump(snap).unwrap_or_else(|| panic!("snapshot {snap} of case set {name}"));
                Gba::from_dump(rom.clone(), &prefix).unwrap()
            });
            let mut g = base.clone();
            g.garage_typed = name == "garage";
            g.map_typed = name == "mapdraw";
            for m in c["mem"].as_array().unwrap() {
                for (i, &b) in bytes(&m[1]).iter().enumerate() {
                    g.set_u8(m[0].as_u64().unwrap() as u32 + i as u32, b);
                }
            }
            for (a, v) in c["ret"].as_object().unwrap() {
                g.returns
                    .insert(u32::from_str_radix(&a[2..], 16).unwrap(), v.as_u64().unwrap() as u32);
            }
            let pre = g.clone();
            let f = c["fn"].as_str().unwrap();
            let r0 = match f {
                "0x812b5f0" => Some(menu_frame(&mut g)),
                "0x812acec" => {
                    game_state_step(&mut g);
                    None
                }
                "0x812ae64" => {
                    main_frame(&mut g);
                    None
                }
                "0x812d334" => {
                    draw_screen(&mut g, c["arg"].as_u64().unwrap() as u32);
                    None
                }
                "0x812bb5c" => {
                    goto_screen(&mut g, c["arg"].as_u64().unwrap() as i32);
                    None
                }
                "0x8164bec" | "0x8136d74" | "0x8136e60" | "0x8141578" | "0x81419c0" | "0x8141b40" | "0x8141c88" => {
                    let a: Vec<u32> = c["args"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|v| v.as_u64().unwrap() as u32)
                        .collect();
                    let r = g.draw_call(u32::from_str_radix(&f[2..], 16).unwrap(), &a).unwrap();
                    (f == "0x8141578").then_some(r) // the text width
                }
                "0x812bd60" => {
                    let a = &c["args"];
                    let k: [u32; 3] = std::array::from_fn(|i| a[i].as_u64().unwrap() as u32);
                    adapt::typed(&mut g, |st, h| flow::Host::button_prompts(h, st, &k));
                    None
                }
                "0x813550c" => {
                    adapt::typed(&mut g, |st, h| flow::Host::message_box_draw(h, st));
                    None
                }
                "0x812c81c" | "0x812c5c4" | "0x812d960" | "0x812c8a8" | "0x812c984" | "0x81300e0" | "0x8133d30"
                | "0x812d564" | "0x81435c4" => {
                    let a: Vec<u32> = c["args"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|v| v.as_u64().unwrap() as u32)
                        .collect();
                    adapt::typed(&mut g, |st, h| match f {
                        "0x812c81c" => Some(garage::state(st, h.rom(), a[0])),
                        "0x812c5c4" => Some(garage::owned(st, h.rom(), a[0])),
                        "0x812d960" => Some(garage::price(st, h.rom(), a[0])),
                        "0x812c8a8" => {
                            garage::buy(st, h.rom(), a[0]);
                            None
                        }
                        "0x812c984" => Some(garage::new_part(st, h, a[0] as i32)),
                        "0x81300e0" => Some(garage::list_item_new(st, h, a[0], a[1])),
                        "0x8133d30" => {
                            garage::car_stats_draw(st, h, a[0] as i32, a[1] as i32, a[2] as i32, a[3]);
                            None
                        }
                        "0x812d564" => {
                            garage::copy_car_record(st);
                            None
                        }
                        _ => {
                            map::draw_map(st, h);
                            None
                        }
                    })
                }
                _ => {
                    // A kind's handler called directly: enter and update return 1 or 0; draw and exit nothing.
                    let a = u32::from_str_radix(&f[2..], 16).unwrap();
                    let (kind, phase) = KINDS
                        .iter()
                        .find_map(|&k| k.handlers().iter().position(|&h| h == a).map(|p| (k, p)))
                        .unwrap_or_else(|| panic!("{f}"));
                    let r = run_handler(&mut g, kind, phase, &[c["arg"].as_u64().unwrap() as u32]);
                    (phase == 1).then_some(r) // only the update's result is used (`menu_frame`)
                }
            };
            let want: std::collections::BTreeMap<u32, u8> = c["writes"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|w| {
                    let a = w[0].as_u64().unwrap() as u32;
                    bytes(&w[1])
                        .into_iter()
                        .enumerate()
                        .map(move |(i, b)| (a + i as u32, b))
                })
                .collect();
            // Stub arguments: words, or ["s", hex] for a string the game built on its stack, which the port passes
            // as `STACK_TEXT + i` (`Gba::texts[i]`); compared by content.
            let mut next_text = 0; // the port's strings, in the order the game passed them
            let live: Vec<usize> = (0..g.texts.len()).filter(|i| !g.consumed.contains(i)).collect(); // not read by a drawing primitive
            let calls: Vec<(u32, Vec<u32>)> = c["calls"]
                .as_array()
                .unwrap()
                .iter()
                .map(|k| {
                    let args = k[1].as_array().unwrap().iter().map(|v| match v.as_u64() {
                        Some(w) => w as u32,
                        None => {
                            let i = next_text;
                            next_text += 1;
                            match live.get(i).map(|&j| (j, &g.texts[j])) {
                                Some((j, t)) if *t == bytes(&v[1]) => STACK_TEXT + j as u32,
                                _ => u32::MAX,
                            }
                        }
                    });
                    (k[0].as_u64().unwrap() as u32, args.collect::<Vec<_>>())
                })
                .collect();
            // NOT 1:1 (N1): a blit above the top row lands in the invisible gap below the previous page, which the
            // port drops; both sides are compared without the gaps between the mode 4 pages.
            let visible = |a: &u32| {
                ![
                    0x0600_9600..0x0600_A000,
                    0x0601_3600..0x0601_4000,
                    0x0201_F600..0x0202_0000,
                    0x0202_9600..0x0202_A000,
                ]
                .iter()
                .any(|r| r.contains(a))
            };
            let want: std::collections::BTreeMap<u32, u8> = want.into_iter().filter(|(a, _)| visible(a)).collect();
            let got: std::collections::BTreeMap<u32, u8> =
                changed(&pre, &g).into_iter().filter(|(a, _)| visible(a)).collect();
            let diff: Vec<_> = got.iter().filter(|(a, b)| want.get(a) != Some(b)).take(8).collect();
            let missing: Vec<_> = want.iter().filter(|(a, b)| got.get(a) != Some(b)).take(8).collect();
            assert!(
                diff.is_empty() && missing.is_empty(),
                "case {n} {f} {snap}: extra {diff:x?} missing {missing:x?}"
            );
            let texts: Vec<_> = g.texts.iter().map(|t| String::from_utf8_lossy(t)).collect();
            assert_eq!(g.calls, calls, "case {n} {f} {snap}: calls (texts {texts:?})");
            if let Some(r0) = r0 {
                assert_eq!(r0 as u64, c["r0"].as_u64().unwrap(), "case {n} {f} {snap}: result");
            }
        }
        eprintln!("{name}: {} oracle cases match", cases.len());
    }

    /// The drawing primitives against the game's own code on a page in EWRAM (`tools/oracle/draw.py`): every changed
    /// byte (the page, the number text's remainder), the `unpack_to_buffer` calls and `text_menu`'s width.
    #[test]
    fn drawing_matches_the_game() {
        for kind in [
            "fill", "blit", "alt", "text", "text7", "wrapped", "box", "prompts", "message",
        ] {
            replay(&format!("draw-{kind}"));
        }
    }

    /// `fill_rect8` on a real mGBA frame (`tools/oracle/fill_rect_hw.py`: the game's own routine on 40 rectangles of
    /// every start column and many widths, run headless on BG VRAM): the byte stores' halfword duplication and the
    /// forced alignment of unaligned word stores that the function oracle does not model.
    #[test]
    fn fill_rect8_matches_mgba() {
        let Some(text) = nfsgba_testkit::read_to_string("menus3/fill-rect-hw.json") else {
            return;
        };
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let (mut hw, mut plain) = (vec![0u8; 240 * 160], vec![0u8; 240 * 160]);
        for r in v["rects"].as_array().unwrap() {
            let n: Vec<i64> = r.as_array().unwrap().iter().map(|x| x.as_i64().unwrap()).collect();
            let rect = [n[0] as i32, n[1] as i32, n[2] as i32, n[3] as i32];
            nfsgba_formats::ui::fill_rect8(&mut hw, 240, rect, n[4] as u32, true);
            nfsgba_formats::ui::fill_rect8(&mut plain, 240, rect, n[4] as u32, false);
        }
        assert_eq!(hw, bytes(&v["page"]), "BG VRAM rules against the mGBA page");
        assert_ne!(hw, plain, "the check must exercise the duplication");
    }

    /// A typed screen draws on the hidden page and shows it on the flip.
    #[test]
    fn screen_draws_and_flips() {
        let Some(rom) = rom() else { return };
        let mut s = draw::Screen::default();
        let ctx = draw::Ctx {
            rom: &rom,
            language: 0,
            pitch: 240,
        };
        s.fill_rect([8, 8, 16, 12], 3);
        assert!(s.shown().iter().all(|&b| b == 0), "the shown page is untouched");
        let w = s.draw(&ctx, |c, p| c.text_menu(p, 0xE, b"AB", 20, 40, 0, 0));
        assert!(w > 0);
        s.flip();
        assert_eq!(s.shown()[8 * 240 + 8], 3);
        assert!(
            s.shown().iter().any(|&b| b != 0 && b != 3),
            "the text is on the page just drawn"
        );
        assert!(s.page().iter().all(|&b| b == 0));
    }

    /// The four screen tables are the ROM's jump tables: each entry's stub calls (Thumb `bl`) its kind's handler
    /// before its first unconditional branch, or no handler.
    #[test]
    fn screen_tables_match_the_rom_jump_tables() {
        let Some(rom) = rom() else { return };
        let hw = |a: u32| u16::from_le_bytes([rom[(a & 0x1FF_FFFF) as usize], rom[(a & 0x1FF_FFFF) as usize + 1]]);
        let handler_of = |stub: u32, phase: usize| -> Option<Kind> {
            let kinds = [
                Kind::List,
                Kind::Kind7,
                Kind::Career,
                Kind::Event,
                Kind::Setup,
                Kind::Kind18,
                Kind::Intro,
                Kind::Kind38,
            ];
            let mut pc = stub;
            for _ in 0..24 {
                let (h, l) = (hw(pc) as u32, hw(pc + 2) as u32);
                if h & 0xF800 == 0xE000 {
                    return None; // b: the no-handler exit
                }
                if h & 0xF800 == 0xF000 && l & 0xF800 == 0xF800 {
                    let hi = ((h & 0x7FF) << 21) as i32 >> 9;
                    let target = (pc as i32 + 4 + hi + ((l & 0x7FF) << 1) as i32) as u32;
                    if let Some(&k) = kinds.iter().find(|k| k.handlers()[phase] == target) {
                        return Some(k);
                    }
                    pc += 4;
                } else {
                    pc += 2;
                }
            }
            None
        };
        let entry = |table: u32, s: u32| {
            u32::from_le_bytes(rom[(table - 0x0800_0000 + 4 * s) as usize..][..4].try_into().unwrap())
        };
        for s in 0..=0x30u32 {
            let enter = (s <= 0x2F).then(|| handler_of(entry(0x0812_B454, s), 0)).flatten();
            assert_eq!(enter, enter_kind(s), "enter {s}");
            assert_eq!(handler_of(entry(0x0812_B980, s), 1), update_kind(s), "update {s}");
            assert_eq!(handler_of(entry(0x0812_D370, s), 2), draw_kind(s), "draw {s}");
            assert_eq!(handler_of(entry(0x0812_B640, s), 3), exit_kind(s), "exit {s}");
        }
    }

    #[test]
    fn fades_match_the_game() {
        let Some(cases) = cases("fades") else { return };
        for c in &cases {
            let n = |k: &str| c[k].as_u64().unwrap() as usize;
            let mut pal = words(&c["before"]);
            if c["kind"].as_str().unwrap().starts_with("in") {
                // The OBJ variant reads the target from `first` too; the others from its start.
                let skip = if c["kind"] == "in_obj" { n("first") } else { 0 };
                fade_in_step(
                    &mut pal,
                    &words(&c["target"])[skip..],
                    n("first"),
                    n("count"),
                    n("step") as u32,
                );
            } else {
                fade_out_step(&mut pal, n("first"), n("count"), n("step") as u32);
            }
            let want = words(&c["after"]);
            if let Some(i) = (0..256).find(|&i| pal[i] != want[i]) {
                let before = words(&c["before"])[i];
                panic!(
                    "{} first {} count {} step {}: [{i}] {before:#06x} -> ours {:#06x}, game {:#06x}",
                    c["kind"],
                    n("first"),
                    n("count"),
                    n("step"),
                    pal[i],
                    want[i]
                );
            }
        }
        eprintln!("fades: {} oracle cases match", cases.len());
    }
}
