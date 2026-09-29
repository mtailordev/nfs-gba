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

/// Runs a screen handler (`phase`: 0 enter, 1 update, 2 draw, 3 exit): the ported ones in Rust, the others as
/// `Gba::unported` calls.
pub fn run_handler(g: &mut Gba, kind: Kind, phase: usize, args: &[u32]) -> u32 {
    let full = args.first().copied().unwrap_or(0);
    match (kind, phase) {
        (Kind::Intro, 0) => intro_enter(g),
        (Kind::Intro, 1) => intro_update(g),
        (Kind::Intro, 2) => intro_draw(g),
        (Kind::Kind7, 0) => kind7_enter(g),
        (Kind::Kind7, 1) => kind7_update(g),
        (Kind::Kind7, 2) => kind7_draw(g, full),
        (Kind::Event, 0) => event_enter(g),
        (Kind::Event, 1) => event_update(g),
        (Kind::Event, 2) => event_draw(g, full),
        (Kind::Career, 0) => career_zone_enter(g),
        (Kind::Career, 1) => career_zone_update(g),
        (Kind::Career, 2) => career_zone_draw(g, full),
        (Kind::Setup, 0) => setup_enter(g),
        (Kind::Setup, 1) => setup_update(g),
        (Kind::Setup, 2) => setup_draw(g, full),
        (Kind::Kind38, 0) => kind38_enter(g),
        (Kind::Kind38, 1) => kind38_update(g),
        (Kind::Kind38, 2) => kind38_draw(g, full),
        (Kind::List, 0) => list_enter(g),
        (Kind::List, 1) => list_update(g),
        (Kind::List, 2) => list_draw(g, full),
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
const LANGUAGE_CURSOR: u32 = 0x0300_5960;
const CREDITS: u32 = 0x0300_5964; // pointer into the credits list
const NAME: u32 = 0x0300_5970; // 9 bytes: the profile name being typed
const NAME_LEN: u32 = 0x0300_598C;
const KEYBOARD_ROW: u32 = 0x0300_597C; // 0..=4; row 4 holds DEL (columns 0–2), SPACE (3–6), OK (7–9)
const KEYBOARD_COLUMN: u32 = 0x0300_5990; // 0..=9

/// `list_slot` (`0x0812FD04`): the List screen's cursor slot (profile `+0x350 + slot`), −1 for other screens.
pub fn list_slot(g: &Gba) -> i32 {
    match g.u32(SCREEN) {
        s @ 0..=2 => s as i32,
        3 if g.u16(g.u32(PROFILE) + 0x12) != 0 => 3,
        3 => 0xF,
        s @ 4..=6 => s as i32,
        27 => 7,
        29 => 8,
        30 => 9,
        35 => 0xA,
        36 => 0xB,
        9 => 0xC,
        28 => 0xD,
        45 => 0xE,
        46 => 0x10,
        _ => -1,
    }
}

/// `goto_screen` (`0x0812BB5C`). Screens up to 0x7F are pushed: the old screen onto the back stack, and List
/// screens (but 9 and 28) clear their cursor slot, then `enter_screen`. Above: the keys are swallowed; 0x81 (Quick
/// Play) records the exit screen; 0x82 resumes a paused race.
pub fn goto_screen(g: &mut Gba, s: i32) {
    if s <= 0x7F {
        let top = g.u8(BACK_TOP).wrapping_add(1);
        g.set_u8(BACK_TOP, top);
        let old = g.u32(SCREEN) as u8;
        g.set_u8(
            g.u32(PROFILE).wrapping_add(0x344).wrapping_add(top as i8 as i32 as u32),
            old,
        );
        g.set_u32(SCREEN, s as u32);
        if matches!(s, 0..=6 | 27 | 29 | 30 | 35 | 36 | 45 | 46) {
            let slot = list_slot(g);
            g.set_u8(g.u32(PROFILE).wrapping_add(0x350).wrapping_add(slot as u32), 0);
        }
        enter_screen(g);
        return;
    }
    g.set_u16(KEYS, 0);
    if s == 0x82 {
        // Back to the race from the pause menu: 15 frames, then the race palettes and music.
        for _ in 0..15 {
            g.unported(VBLANK_INTR_WAIT, &[]);
        }
        fill_bg_palette(g, 0, 0, 0x100);
        g.set_u32(GAME_STATE, 5);
        g.set_u32(0x0300_5398, 0);
        g.set_u32(FADE, 0x10);
        g.set_u8(BACK_TOP, g.u8(BACK_TOP).wrapping_sub(1));
        g.unported(0x0813_72E4, &[WORLD]); // race_menu_palette_setup
        if g.u32(0x0300_5698) != 0 {
            g.unported(0x0814_3010, &[1]); // hud_toggle
        }
        g.unported(0x0813_9E10, &[WORLD]);
        let music = (g.i8(g.u32(PROFILE) + 0x2EE) as i32 + 1) as u32;
        g.unported(0x0813_6054, &[music]); // carbon_play_music
        if g.u32(ROUTE) != 0 {
            let (a, b) = (g.u32(0x0300_55F0), g.u32(SECOND_PALETTE));
            copy_mem(g, a, b, 0x200, 0x20);
            g.unported(0x0813_A514, &[WORLD]);
        }
        return;
    }
    if s == 0x81 {
        g.set_u32(EXIT_SCREEN, g.u32(SCREEN));
    }
    g.set_u32(SCREEN, s as u32);
}

/// The intro page record (`0x7E5DA8`, 0x14 bytes) of an intro screen: `+6`/`+8` the background passed to the
/// menu scene setup, `+0x10` the item list (`+8`: the next screen).
fn intro_page(screen: u32) -> Option<u32> {
    let p = match screen {
        0x15..=0x1A => screen - 0x15,
        0x25 => 6,
        0x2F | 0x30 => 7,
        _ => return None,
    };
    Some(0x087E_5DA8 + 0x14 * p)
}

/// `intro_enter` (`0x081315A0`, `menu_screen_setup` in earlier notes): the screen's background (menu scene setup
/// `b` after a screen was entered, else `a`), its deadline (tick counter + 0x5A on 0x2F, 0x1E0 on 0x18, else 0xF0)
/// and per-screen state.
pub fn intro_enter(g: &mut Gba) -> u32 {
    let screen = g.u32(SCREEN);
    let page = intro_page(screen).expect("intro_enter on a screen without an intro page");
    if screen != 0x30 {
        let (a, b) = (
            g.u16(page + 6) as i16 as i32 as u32,
            g.u16(page + 8) as i16 as i32 as u32,
        );
        menu_scene_setup(g, a, b, 0xFFFF);
    }
    let deadline = g.u32(PROFILE) + 0x3B0;
    if g.u32(SCREEN) == 0x2F {
        // Health and safety: colours 0..4 from 0x7E5E48, the language's text image.
        let second = g.u32(SECOND_PALETTE);
        for i in 0..5 {
            let c = g.u16(0x087E_5E48 + 2 * i);
            g.set_u16(second + 2 * i, c);
        }
        let image = match g.u32(LANGUAGE) {
            0 => Some(7),
            1 => Some(8),
            2 => Some(0xA),
            3 => Some(9),
            4 => Some(0xB),
            _ => None,
        };
        if let Some(n) = image {
            g.unported(0x0813_644C, &[WORLD, n]);
        }
        g.set_u32(deadline, g.u32(TICKS).wrapping_add(0x5A));
    } else {
        g.set_u32(deadline, g.u32(TICKS).wrapping_add(0xF0));
    }
    match g.u32(SCREEN) {
        0x15 => {
            g.set_u32(0x0300_5984, 0);
            g.set_u32(0x0300_5980, 0);
            g.set_u32(CREDITS, 0x0879_9882);
        }
        0x16 => {
            g.set_u32(KEYBOARD_COLUMN, 0);
            g.set_u32(KEYBOARD_ROW, 0);
            g.set_u32(NAME_LEN, 0);
            g.unported(0x0813_56DC, &[]); // profile_reset
            for i in 0..9 {
                g.set_u8(NAME + i, 0);
            }
            // Start from the profile's current name.
            while g.u8(g.u32(PROFILE) + g.u32(NAME_LEN)) != 0 {
                let len = g.u32(NAME_LEN);
                g.set_u8(NAME + len, g.u8(g.u32(PROFILE) + len));
                g.set_u32(NAME_LEN, len + 1);
            }
            let profile = g.u32(PROFILE);
            if g.u16(profile + 0x494) != 2 {
                for off in [0x478, 0x480, 0x484, 0x47C, 0x488, 0x48C] {
                    g.set_u32(profile + off, 0);
                }
            }
        }
        0x17 => {
            g.unported(0x0813_6054, &[0]); // carbon_play_music: the title music
        }
        0x18 => {
            g.set_u32(deadline, g.u32(TICKS).wrapping_add(0x1E0));
        }
        _ => {}
    }
    1
}

// Drawing primitives the draw handlers call; not ported onto `Gba` yet (their pixel output is exact in `ui.rs`).
pub const MENU_BLIT_MATERIAL: u32 = 0x0813_6D74; // (world, material, x, y)
pub const TEXT_MENU: u32 = 0x0814_1578; // (font, text key or pointer, x, y, colour/alignment, …)
pub const TEXT_MENU_WRAPPED: u32 = 0x0814_1B40; // (font, key, x, y, width, lines, colour)
pub const MENU_BUTTON_PROMPTS: u32 = 0x0812_BD60; // (left key, right key, -1)
const FLASH: u32 = 0x0300_53B4; // frame counter; bit 4 blinks the cursors and PRESS START

/// `intro_draw` (`0x08131FE0`): the intro screens' page (`FUN_081364C4`), heading, content and button prompts.
/// The health and safety screens (0x2F, 0x30) only set up the page.
pub fn intro_draw(g: &mut Gba) -> u32 {
    let screen = g.u32(SCREEN);
    let page = intro_page(screen).expect("intro_draw on a screen without an intro page");
    let count = g.u16(page + 0xA) as i16 as i32;
    let items = g.u32(page + 0x10);
    let buffer = g.u32(0x0300_57F0);
    g.unported(0x0813_64C4, &[buffer]);
    if screen.wrapping_sub(0x2F) <= 1 {
        return 0;
    }
    let neg1 = u32::MAX;
    if screen == 0x16 {
        let key = if g.u16(g.u32(PROFILE) + 0x494) == 2 {
            0x122
        } else {
            0x10C
        };
        g.unported(TEXT_MENU, &[0xC, key, 0xEC, 2, neg1, 0]);
    } else {
        let title = g.u16(page) as i16 as i32;
        if title != -1 {
            g.unported(TEXT_MENU, &[0xC, title as u32, 0xEC, 2, neg1, 0]);
        }
    }
    let lang = g.u32(LANGUAGE);
    let blit = |g: &mut Gba, m: u32, x: u32, y: u32| {
        g.unported(MENU_BLIT_MATERIAL, &[WORLD, m, x, y]);
    };
    match screen {
        0x15 => credits_draw(g),
        0x16 => {
            g.unported(TEXT_MENU, &[0xD, 0x164, 0x4A, 0x14, neg1, 8]);
            g.unported(TEXT_MENU, &[0xD, NAME, 0x78, 0x14, 1, 0]);
            let letters = match lang {
                1 => Some(0xDE),
                2 => Some(0xE0),
                3 => Some(0xDF),
                4 => Some(0xE1),
                _ => None,
            };
            if let Some(m) = letters {
                g.unported(0x0813_6E60, &[WORLD, m, 0x58, 0x77]);
            }
            let (row, col) = (g.i32(KEYBOARD_ROW), g.i32(KEYBOARD_COLUMN));
            if row == 4 {
                let b = if col > 6 {
                    2
                } else if col > 2 {
                    1
                } else {
                    0
                };
                blit(g, 0xB4, (67 * b + 0x11) as u32, (row * 20 + 0x22) as u32);
            } else {
                blit(g, 0xB3, (col * 20 + 0x14) as u32, (row * 20 + 0x24) as u32);
            }
        }
        0x17 => {
            blit(g, 0xCB, 0x30, 0x8A);
            if lang <= 4 {
                blit(g, 0xC4 + lang, 0, 0x96); // logo line per language
            }
            blit(g, if lang == 4 { 0xC9 } else { 0xCA }, 0x60, 4);
            let deadline = g.i32(g.u32(PROFILE) + 0x3B0);
            if g.i32(TICKS) > deadline && g.u32(FLASH) & 0x10 != 0 && lang <= 4 {
                blit(g, 0xBF + lang, 0xAA, 0x50); // PRESS START, blinking
            }
        }
        0x18 => {
            g.unported(TEXT_MENU, &[0xD, 0x19A, 0x78, 6, 1, 0]);
            g.unported(TEXT_MENU, &[0xD, 0x19B, 0x78, 0x10, 1, 0]);
            g.unported(TEXT_MENU_WRAPPED, &[0xE, 0x199, 8, 0x1E, 0xE0, 0xF, 8]);
        }
        0x19 => {
            // The cursor on the selected language, blinking: (material, x, y) per item from the page's list + 2.
            for i in 0..count.max(0) as u32 {
                let item = items + 2 + 10 * i;
                if g.u32(FLASH) & 0x10 != 0 && g.u32(LANGUAGE_CURSOR) == i {
                    let [m, x, y] = [0, 2, 4].map(|o| g.u16(item + o) as i16 as i32 as u32);
                    blit(g, m, x, y);
                }
            }
        }
        0x1A => {
            let logo = match lang {
                1 => Some(0xBA),
                2 => Some(0xBB),
                4 => Some(0xBD),
                _ => None,
            };
            if let Some(m) = logo {
                blit(g, m, 0, 0x91);
            }
        }
        _ => {}
    }
    let (left, right) = (
        g.u16(page + 2) as i16 as i32 as u32,
        g.u16(page + 4) as i16 as i32 as u32,
    );
    g.unported(MENU_BUTTON_PROMPTS, &[left, right, neg1]);
    0
}

/// The credits page (screen 0x15): the current entry's lines (u16 count, then (flags, text key) pairs), centred in
/// 0x90 rows. Flags: bits 0–1 colour (× 8), 0x2000 small font and 4 rows more, 0x1000 6 more, 0x800 12 more; on the
/// first line 0x8000 starts at row 0x28 and 0x4000 at row 0. Some keys sit lower in French, Spanish and Italian.
fn credits_draw(g: &mut Gba) {
    let p = g.u32(CREDITS);
    let n = g.u16(p) as u32;
    let mut height = 12 * n as i32;
    for k in 0..n {
        let f = g.u16(p + 2 + 4 * k);
        height += [(0x2000, 4), (0x1000, 6), (0x800, 0xC)]
            .iter()
            .filter(|(b, _)| f & b != 0)
            .map(|(_, h)| h)
            .sum::<i32>();
    }
    let mut y = (0x90 - height) >> 1;
    let mut k = 0;
    while k < g.u16(g.u32(CREDITS)) as u32 {
        let entry = g.u32(CREDITS) + 4 * k;
        let flags = g.u16(entry + 2);
        let colour = ((flags & 3) << 3) as u32;
        let font = if flags & 0x2000 != 0 { 0xC } else { 0xD };
        if flags & 0x8000 != 0 && k == 0 {
            y = 0x28;
        }
        if g.u16(g.u32(CREDITS) + 2) & 0x4000 != 0 && k == 0 {
            y = 0;
        }
        let key = g.u16(entry + 4);
        match g.u32(LANGUAGE) {
            1 if key == 0x1B || key == 0x1D => y += 0xE,
            4 if key == 0x1F => y += 0xE,
            3 if key == 0x19A => y += 4,
            _ => {}
        }
        g.unported(TEXT_MENU_WRAPPED, &[font, key as u32, 2, y as u32, 0xEE, 0xF, colour]);
        let flags = g.u16(g.u32(CREDITS) + 4 * k + 2);
        y += [(0x2000, 4), (0x1000, 6), (0x800, 0xC)]
            .iter()
            .filter(|(b, _)| flags & b != 0)
            .map(|(_, h)| h)
            .sum::<i32>();
        y += 0xC;
        k += 1;
    }
}

/// `intro_update` (`0x081318E4`): the boot and intro screens. Timed screens move on once the tick counter passes
/// profile `+0x3B0`.
pub fn intro_update(g: &mut Gba) -> u32 {
    let screen = g.u32(SCREEN);
    let deadline = g.u32(PROFILE) + 0x3B0;
    let expired = |g: &Gba| g.i32(TICKS) > g.i32(deadline);
    match screen {
        // Credits: each deadline advances to the next entry (u16 count, count words); the end presses B.
        0x15 => {
            if expired(g) {
                let p = g.u32(CREDITS);
                g.set_u32(CREDITS, p.wrapping_add(4 * g.u16(p) as u32 + 2));
                g.set_u32(deadline, g.u32(TICKS).wrapping_add(0xB4));
            }
            if g.u16(g.u32(CREDITS)) == 0 {
                g.set_u16(KEYS, 2);
            }
        }
        0x16 => name_entry(g),
        // Title: START once the deadline has passed loads the profile, or asks for a name.
        0x17 => {
            if g.u16(KEYS) & 8 != 0 && expired(g) {
                g.unported(CARBON_PLAY_SOUND, &[2, 1]);
                g.set_u8(BACK_TOP, 0xFF);
                let profile = g.u32(PROFILE);
                g.set_u16(profile + 0x494, 0);
                if g.u16(profile + 0x490) != 0 {
                    g.unported(0x0814_19C0, &[0xC, 0x159, 0x78, 0x32, 0xDC, 2, 0]);
                    let save = g.u32(SAVE_BUFFER);
                    g.unported(0x0814_9D84, &[save]); // save_load_profile
                    let lang = g.u32(LANGUAGE);
                    if lang != g.u16(g.u32(PROFILE) + 0x4E8) as u32 {
                        g.set_u32(UNITS, (lang != 0) as u32);
                    }
                    goto_screen(g, 0);
                } else {
                    goto_screen(g, 0x16);
                    mark_screen_changed(g);
                }
            }
        }
        // Public service announcement, EA logo: the page's next screen (item list `+8`) at the deadline.
        0x18 | 0x1A => {
            if expired(g) {
                g.set_u8(BACK_TOP, 0xFF);
                let items = g.u32(0x087E_5DA8 + 0x14 * (screen - 0x15) + 0x10);
                goto_screen(g, g.u16(items + 8) as i16 as i32);
            }
        }
        0x19 => language_select(g),
        // Health and safety, first part: after its deadline, the blinking part (0x30) for 0xDB6 ticks.
        0x2F => {
            if expired(g) {
                g.set_u32(deadline, g.u32(TICKS).wrapping_add(0xDB6));
                g.set_u32(SCREEN, 0x30);
                let second = g.u32(SECOND_PALETTE);
                g.set_u16(second + 8, 0);
                g.set_u32(0x0300_0000, 0);
            }
        }
        // Health and safety, blinking: any key or the deadline goes to the EA logo; else colour 4 steps by ±0x421
        // between 0 and 0x7FFF (direction at 0x03000000) and the frame waits an extra VBlank.
        0x30 => {
            if g.u16(KEYS) & 0x3FF != 0 || expired(g) {
                g.unported(CARBON_PLAY_SOUND, &[2, 1]);
                goto_screen(g, 0x1A);
                mark_screen_changed(g);
            } else {
                let second = g.u32(SECOND_PALETTE);
                let mut c = g.u16(second + 8) as u32;
                if g.u32(0x0300_0000) != 0 {
                    if c == 0 {
                        c = 0x421;
                        g.set_u32(0x0300_0000, 0);
                    } else {
                        c = c.wrapping_sub(0x421);
                    }
                } else if c == 0x7FFF {
                    c -= 0x421;
                    g.set_u32(0x0300_0000, 1);
                } else {
                    c = c.wrapping_add(0x421);
                }
                g.set_u16(g.u32(SECOND_PALETTE) + 8, c as u16);
                g.set_u32(PALETTE_DIRTY, 1);
                g.unported(VBLANK_INTR_WAIT, &[]);
            }
        }
        _ => {}
    }
    1
}

/// `FUN_081318B0`: the typed name counts when its 9 bytes OR to neither 0 nor 0x20; plays sound 2 either way.
fn name_is_valid(g: &mut Gba) -> bool {
    let or = (0..9).fold(0, |a, i| a | g.u8(NAME + i));
    g.unported(CARBON_PLAY_SOUND, &[2, 1]);
    or != 0 && or != 0x20
}

/// Screen 0x16: the profile name keyboard, 4 rows of 10 characters and a row of DEL / SPACE / OK. B deletes,
/// START is OK; OK saves the profile.
fn name_entry(g: &mut Gba) {
    let keys = g.u16(KEYS);
    let set = |g: &mut Gba, a: u32, v: i32| g.set_u32(a, v as u32);
    if keys & 0x40 != 0 {
        let r = g.i32(KEYBOARD_ROW) - 1;
        set(g, KEYBOARD_ROW, if r < 0 { 4 } else { r });
        g.unported(CARBON_PLAY_SOUND, &[4, 1]);
    }
    if g.u16(KEYS) & 0x80 != 0 {
        let r = g.i32(KEYBOARD_ROW) + 1;
        set(g, KEYBOARD_ROW, if r > 4 { 0 } else { r });
        g.unported(CARBON_PLAY_SOUND, &[4, 1]);
    }
    if g.u16(KEYS) & 0x20 != 0 {
        if g.i32(KEYBOARD_ROW) == 4 {
            let c = g.i32(KEYBOARD_COLUMN);
            set(
                g,
                KEYBOARD_COLUMN,
                if c > 6 {
                    4
                } else if c <= 2 {
                    8
                } else {
                    1
                },
            );
        }
        let c = g.i32(KEYBOARD_COLUMN) - 1;
        set(g, KEYBOARD_COLUMN, if c < 0 { 9 } else { c });
        g.unported(CARBON_PLAY_SOUND, &[4, 1]);
    }
    if g.u16(KEYS) & 0x10 != 0 {
        if g.i32(KEYBOARD_ROW) == 4 {
            let c = g.i32(KEYBOARD_COLUMN);
            set(
                g,
                KEYBOARD_COLUMN,
                if c > 6 {
                    -1
                } else if c > 2 {
                    6
                } else {
                    2
                },
            );
        }
        let c = g.i32(KEYBOARD_COLUMN) + 1;
        set(g, KEYBOARD_COLUMN, if c > 9 { 0 } else { c });
        g.unported(CARBON_PLAY_SOUND, &[4, 1]);
    }
    if g.u16(KEYS) & 0xB == 0 {
        return;
    }
    let (row, col) = (g.i32(KEYBOARD_ROW), g.i32(KEYBOARD_COLUMN));
    let mut key = col + row * 10;
    if row == 4 {
        key = if col > 6 {
            2
        } else if col > 2 {
            1
        } else {
            0
        } + g.i32(KEYBOARD_ROW) * 10;
    }
    let keys = g.u16(KEYS);
    if keys == 8 {
        key = 0x2A;
    }
    if keys == 2 {
        key = 0x28;
    }
    match key {
        0x28 => {
            let len = g.i32(NAME_LEN);
            if len == 0 {
                g.unported(CARBON_PLAY_SOUND, &[0x27, 1]);
            } else {
                set(g, NAME_LEN, len - 1);
                g.set_u8(NAME + (len - 1) as u32, 0);
                g.unported(CARBON_PLAY_SOUND, &[2, 1]);
            }
        }
        0x2A => {
            if !name_is_valid(g) {
                g.unported(CARBON_PLAY_SOUND, &[0x27, 1]);
                return;
            }
            g.unported(CARBON_PLAY_SOUND, &[2, 1]);
            for i in 0..9 {
                let b = g.u8(NAME + i);
                g.set_u8(g.u32(PROFILE) + i, b);
            }
            let save = g.u32(SAVE_BUFFER);
            if g.unported(0x0814_9FD8, &[save]) != 0 {
                goto_screen(g, 0); // save_write_profile failed
            } else if g.u16(g.u32(PROFILE) + 0x494) == 2 {
                menu_back(g);
            }
            mark_screen_changed(g);
        }
        _ => {
            let len = g.i32(NAME_LEN);
            if len > 7 {
                return;
            }
            let ch = match key {
                0..=8 => key + 0x31, // 1..9
                9 => 0x30,           // 0
                10..=0x23 => key + 0x37,
                0x24 => b'.' as i32,
                0x25 => b',' as i32,
                0x26 => b'!' as i32,
                0x27 => b':' as i32,
                _ => b' ' as i32,
            };
            g.set_u8(NAME + len as u32, ch as u8);
            set(g, NAME_LEN, len + 1);
            g.unported(CARBON_PLAY_SOUND, &[2, 1]);
            if len + 1 == 8 && name_is_valid(g) {
                set(g, KEYBOARD_COLUMN, 8);
                set(g, KEYBOARD_ROW, 4);
            }
        }
    }
}

/// Screen 0x19: the five languages (`0x7E5D10`, cursor `0x03005960`); A picks one and goes on to the health
/// and safety screen (0x2F).
fn language_select(g: &mut Gba) {
    let pick = |g: &mut Gba| {
        let lang = g.u32(0x087E_5D10 + 4 * g.u32(LANGUAGE_CURSOR));
        g.set_u32(LANGUAGE, lang);
    };
    if g.u16(KEYS) == 1 {
        pick(g);
        g.unported(CARBON_PLAY_SOUND, &[2, 1]);
        goto_screen(g, 0x2F);
        g.set_u8(BACK_TOP, 0xFF);
        mark_screen_changed(g);
        return;
    }
    let set = |g: &mut Gba, v: i32| g.set_u32(LANGUAGE_CURSOR, v as u32);
    if g.u16(KEYS) & 0x20 != 0 {
        let c = g.i32(LANGUAGE_CURSOR);
        set(g, if c == 0 { 4 } else { c - 1 });
    }
    if g.u16(KEYS) & 0x10 != 0 {
        let c = g.i32(LANGUAGE_CURSOR);
        set(g, if c == 4 { 0 } else { c + 1 });
    }
    if g.u16(KEYS) & 0x40 != 0 {
        let c = g.i32(LANGUAGE_CURSOR);
        set(
            g,
            if c > 2 {
                c - 3
            } else if c == 0 {
                3
            } else {
                4
            },
        );
    }
    if g.u16(KEYS) & 0x80 != 0 {
        let c = g.i32(LANGUAGE_CURSOR);
        set(
            g,
            if c + 3 == 5 {
                4
            } else if c + 3 <= 4 {
                c + 3
            } else {
                c - 3
            },
        );
    }
    if g.u16(KEYS) & 0xF0 != 0 {
        g.unported(CARBON_PLAY_SOUND, &[4, 1]);
        mark_screen_changed(g);
    }
    pick(g);
}

// The map screens (Kind7: 7 Quick Play circuits, 8 Quick Play sprints, 0xE the career district map, 0x11 the
// career map). Map state `0x03006230`: `+0`/`+4` view x/y (8.8), `+8` cursor (i8), `+9` moved.
const MAP: u32 = 0x0300_6230;
const MAP_PALETTES: u32 = 0x0814_3284; // (): the map's zone colours into the second base palette
const MAP_DRAW: u32 = 0x0814_35C4; // (): scrolls the view towards the cursor and draws the map and its markers
/// Page records of the map screens (`0x7E50A4`, 0x14 bytes: `+2`/`+4` button prompts).
const MAP_PAGES: u32 = 0x087E_50A4;

/// `kind7_enter` (`0x0812E80C`): background 0xDA, menu palette 7; the map state (`FUN_0814397C`).
pub fn kind7_enter(g: &mut Gba) -> u32 {
    menu_scene_setup(g, 0xDA, 7, 0xFFFF);
    // FUN_0814397C: the cursor starts on the district of profile +0x1FB (×2) on screen 0xE, else 0; view (1, 0.75).
    let cursor = if g.u32(SCREEN) == 0xE {
        (g.u8(g.u32(PROFILE) + 0x1FB) as i8 as u8).wrapping_shl(1)
    } else {
        0
    };
    g.set_u8(MAP + 8, cursor);
    g.set_u8(MAP + 9, 1);
    g.set_u32(MAP, 0x100);
    g.set_u32(MAP + 4, 0xC0);
    g.unported(MAP_PALETTES, &[]);
    1
}

/// `FUN_081439C0` (cursor): moves the map cursor and marks it moved.
fn map_select(g: &mut Gba, cursor: u8) {
    g.set_u8(MAP + 8, cursor);
    g.set_u8(MAP + 9, 1);
}

/// `kind7_update` (`0x0812E3D4`). Left/right move the cursor over 12 entries (18 on screen 8, and on 0x11 in mode
/// 2; step 2 on 0xE). A depends on profile `+0x404`: 0 picks a track (screen 7: slot `0x7E472C[cursor]`, route
/// number `0x7E4A70`; 8: sprint `cursor + 0x18`) if its district (unlock `0x117 +` cursor/2 or /3) is open and goes to
/// screen 0x2D; 1 picks the district on 0xE (profile `+0x1FB`); 2 sets mode 3; 3 goes back.
pub fn kind7_update(g: &mut Gba) -> u32 {
    let step = if g.u32(SCREEN) == 0xE {
        g.set_u8(MAP + 8, g.u8(MAP + 8) & 0xFE);
        2
    } else {
        1
    };
    let profile = g.u32(PROFILE);
    let s = g.u32(SCREEN);
    let count: i8 = if s == 8 || (s == 0x11 && g.u8(profile + 0x404) == 2) {
        18
    } else {
        12
    };
    if g.u16(KEYS) == 1 {
        let cursor = g.i8(MAP + 8) as i32;
        match g.u8(profile + 0x404) {
            0 => {
                let open = if g.u32(SCREEN) == 7 {
                    unlock_is_locked(g, (cursor >> 1) + 0x117) == 0
                } else {
                    unlock_is_locked(g, nfsgba_fixed::div(cursor, 3) as i8 as i32 + 0x117) == 0
                };
                if !open {
                    g.unported(CARBON_PLAY_SOUND, &[0x27, 1]);
                    return 1; // the locked beep skips the left/right handling
                }
                let slot = if g.u32(SCREEN) == 7 {
                    g.u16(0x087E_472C_u32.wrapping_add((cursor as u32).wrapping_mul(2))) as u32
                } else {
                    (cursor + 0x18) as u32
                };
                g.set_u32(ROUTE, g.u16(0x087E_4A72_u32.wrapping_add(slot.wrapping_mul(4))) as u32);
                g.unported(CARBON_PLAY_SOUND, &[2, 1]);
                goto_screen(g, 0x2D);
            }
            1 => {
                if unlock_is_locked(g, (cursor >> 1) + 0x117) == 0 {
                    g.set_u8(g.u32(PROFILE) + 0x1FB, (cursor >> 1) as u8);
                    g.unported(CARBON_PLAY_SOUND, &[2, 1]);
                    menu_back(g);
                } else {
                    g.unported(CARBON_PLAY_SOUND, &[0x27, 1]);
                }
            }
            2 => {
                g.set_u8(g.u32(PROFILE) + 0x404, 3);
                g.unported(CARBON_PLAY_SOUND, &[2, 1]);
                map_select(g, 0);
            }
            3 => {
                g.unported(CARBON_PLAY_SOUND, &[2, 1]);
                menu_back(g);
            }
            _ => {}
        }
        mark_screen_changed(g);
    }
    if g.u16(KEYS) & 0x20 != 0 {
        g.unported(CARBON_PLAY_SOUND, &[4, 1]);
        let v = (g.u8(MAP + 8) as u32).wrapping_sub(step) as u8;
        g.set_u8(MAP + 8, if (v as i8) < 0 { (count - 1) as u8 } else { v });
        map_select(g, g.u8(MAP + 8));
    }
    if g.u16(KEYS) & 0x10 != 0 {
        g.unported(CARBON_PLAY_SOUND, &[4, 1]);
        let v = g.u8(MAP + 8).wrapping_add(step as u8);
        g.set_u8(MAP + 8, if count <= v as i8 { 0 } else { v });
        map_select(g, g.u8(MAP + 8));
    }
    1
}

/// `kind7_draw` (`0x0812E5AC`): the map (`FUN_081435C4`), the heading (500; 0x1F5 on 0xE; 0x2E4 and a mode text on
/// 0x11), the arrows (lit while their key-repeat delay runs) and the prompts; A's prompt reads 0x15A over a
/// locked district.
pub fn kind7_draw(g: &mut Gba, _full: u32) -> u32 {
    let early = g.i8(MAP + 8) as i32;
    let page = MAP_PAGES
        + 0x14
            * match g.u32(SCREEN) {
                8 => 1,
                0xE => 2,
                0x11 => 3,
                _ => 0,
            };
    g.unported(MAP_DRAW, &[]);
    let mut left = g.u16(page + 2) as i16 as i32 as u32;
    let neg1 = u32::MAX;
    let district = match g.u32(SCREEN) {
        0xE => {
            g.unported(TEXT_MENU, &[0xC, 0x1F5, 2, 2, 0, 0]);
            Some((early >> 1) + 0x117)
        }
        0x11 => {
            g.unported(TEXT_MENU, &[0xC, 0x2E4, 2, 2, 0, 0]);
            let key = if g.u8(g.u32(PROFILE) + 0x404) == 2 { 0x209 } else { 0xD3 };
            g.unported(TEXT_MENU, &[0xC, key, 0xE6, 2, neg1, 0]);
            None
        }
        s @ (7 | 8) => {
            g.unported(TEXT_MENU, &[0xC, 500, 2, 2, 0, 0]);
            let cursor = g.i8(MAP + 8) as i32;
            Some(
                if s == 7 {
                    cursor >> 1
                } else {
                    nfsgba_fixed::div(cursor, 3) as i8 as i32
                } + 0x117,
            )
        }
        _ => None,
    };
    if let Some(id) = district
        && unlock_is_locked(g, id) != 0
    {
        left = 0x15A;
    }
    if g.i32(MESSAGE_BOX) < 0 {
        let profile = g.u32(PROFILE);
        let lit = |g: &Gba, off: u32| g.i8(profile + off) >= 1;
        let m = if lit(g, 0x33C) { 0xA8 } else { 0xA7 };
        g.unported(MENU_BLIT_MATERIAL, &[WORLD, m, 1, 0x48]);
        let m = if lit(g, 0x33D) { 0xAA } else { 0xA9 };
        g.unported(MENU_BLIT_MATERIAL, &[WORLD, m, 0xDF, 0x48]);
        g.unported(MENU_BUTTON_PROMPTS, &[left, g.u16(page + 4) as i16 as i32 as u32, neg1]);
    }
    0
}

// Helpers the career screens share.

const TEXT_BOX: u32 = 0x0814_1C88; // (font, text key or pointer, x, y, width, lines, colour)
const INTRO_PAGE_SETUP: u32 = 0x0813_64C4; // (unpack buffer): clears the page
const DIVIDEND_REMAINDER: u32 = 0x0300_6480;

/// The IWRAM divide routine (`call_via_r3(n, d, 0x03006480, *0x03006494)`, `nfsgba_fixed::iwram_divmod`): the quotient,
/// with the remainder stored at `0x03006480`.
fn iwram_div(g: &mut Gba, n: i32, d: i32) -> i32 {
    let (q, r) = nfsgba_fixed::iwram_divmod(n, d);
    g.set_u32(DIVIDEND_REMAINDER, r as u32);
    q
}

/// `FUN_081633CC` (buffer, n): `n` in decimal, a leading '-' when negative, digits from the millions down with
/// leading zeros dropped (a quotient above 9 prints as the character after '9').
fn number_text(g: &mut Gba, n: i32) -> Vec<u8> {
    let mut s = Vec::new();
    let mut n = n;
    if n < 0 {
        s.push(b'-');
        n = n.wrapping_neg();
    }
    if n < 10 {
        s.push(n as u8 + b'0');
        return s;
    }
    let (mut d, mut leading) = (1_000_000, true);
    for _ in 0..7 {
        let q = iwram_div(g, n, d);
        if q != 0 || !leading {
            leading = false;
            s.push((q as u8).wrapping_add(b'0'));
        }
        n = g.i32(DIVIDEND_REMAINDER);
        d = nfsgba_fixed::div(d, 10);
    }
    s
}

/// `FUN_0812D62C` (text, n): a thousands separator for `n` above 999 in French (a space) and German and Italian
/// (a dot), inserted `digits − 3` from the left of the text `number_text` made.
fn thousands(g: &Gba, s: &mut Vec<u8>, n: i32) {
    let lang = g.u32(LANGUAGE);
    if lang == 0 || lang == 4 || n <= 999 {
        return;
    }
    let sep = if lang == 1 { b' ' } else { b'.' };
    // NOT 1:1 (N1): above 999,999 the game uses a stale register as the position.
    let pos = match n {
        1_000..=9_999 => 1,
        10_000..=99_999 => 2,
        100_000..=999_999 => 3,
        _ => return,
    };
    s.resize(s.len().max(pos + 5), 0);
    let mut i = pos + 4;
    while pos < i {
        s[i] = s[i - 1];
        i -= 1;
    }
    s[i] = sep;
    s.truncate(s.iter().position(|&b| b == 0).unwrap_or(s.len()));
}

/// `FUN_0812FC00` (text, centiseconds): `FUN_08162FC0`'s "mm:ss:cc" (|t| capped at 0x57E3F), then the last colon
/// becomes a dot in French, German and Spanish, a comma in Italian.
fn time_text(g: &mut Gba, cs: i32) -> Vec<u8> {
    let n = cs.unsigned_abs().min(0x57E3F) as i32;
    let secs = iwram_div(g, n, 100);
    let hundredths = g.i32(DIVIDEND_REMAINDER);
    let mins = iwram_div(g, secs, 0x3C);
    let secs = g.i32(DIVIDEND_REMAINDER);
    let mut s = Vec::new();
    for (i, v) in [mins, secs, hundredths].into_iter().enumerate() {
        let tens = iwram_div(g, v, 10);
        s.push((tens as u8).wrapping_add(b'0'));
        s.push((g.i32(DIVIDEND_REMAINDER) as u8).wrapping_add(b'0'));
        if i < 2 {
            s.push(b':');
        }
    }
    match g.u32(LANGUAGE) {
        3 => s[5] = b',',
        1 | 2 | 4 => s[5] = b'.',
        _ => {}
    }
    s
}

/// `frames_to_centiseconds` (`0x08142F74`).
fn frames_to_centiseconds(frames: i32) -> i32 {
    nfsgba_fixed::div(frames.wrapping_mul(100), 0x3C)
}

/// `event_status` (`0x08135D4C`): the 2-bit status of career event `n` (profile `+0x205`; 1 won, 2 second, 3 not
/// done).
fn event_status(g: &Gba, n: i32) -> u32 {
    let (mem, o) = g.at(g.u32(PROFILE).wrapping_add(0x205));
    nfsgba_formats::career::event_status(&mem[o..], n as usize) as u32
}

/// `zone_ladder_index` (`0x0812FC34`): zone·12 plus the zone's events with status 1 or 2 (6 events in zone 5).
fn zone_ladder_index(g: &Gba, zone: i32) -> i32 {
    let n = if zone == 5 { 6 } else { 12 };
    zone * 12
        + (0..n)
            .filter(|&e| matches!(event_status(g, zone * 12 + e), 1 | 2))
            .count() as i32
}

/// `style_rating` (`0x0812C30C`) of the career car (profile `+0x10`, its 17-byte record at `*0x0300539C`):
/// [`nfsgba_formats::career::style_rating`].
fn career_style_rating(g: &Gba) -> i32 {
    let car = g.i8(g.u32(PROFILE) + 0x10) as i32;
    let at = g.u32(0x0300_539C).wrapping_add((car * 0x11) as u32);
    let record: [u8; 17] = std::array::from_fn(|i| g.u8(at + i as u32));
    nfsgba_formats::career::style_rating(&g.rom[..], car as usize, &record)
}

const ZONE: u32 = 0x1FB; // profile: the career zone 0..=5
const EVENT_CURSOR: u32 = 0x388; // profile + zone: the event cursor per zone

/// `career_event_to_globals` (`0x0812DA08`): the selected event ([`nfsgba_formats::career::events`]) into the race
/// globals, and its slot into profile `+0x1FC`.
fn career_event_to_globals(g: &mut Gba) {
    let profile = g.u32(PROFILE);
    let zone = g.u8(profile + ZONE) as i32;
    let slot = g.i8(profile + EVENT_CURSOR + zone as u32) as i32;
    let e = nfsgba_formats::career::events(&g.rom[..])[(zone * 12 + slot) as usize];
    g.set_u32(0x0300_00BC, e.skill as u32);
    g.set_u32(0x0300_5608, e.difficulty() as u32);
    g.set_u32(0x0300_56E4, e.laps as u32);
    g.set_u32(0x0300_5604, e.traffic as u32);
    g.set_u32(0x0300_56E0, e.mode as u32);
    g.set_u32(REVERSE, e.reverse as u32);
    g.set_u32(ROUTE, g.u16(0x087E_4A72 + 4 * e.track_slot() as u32) as u32);
    g.set_u32(PLAYER_CAR, g.i8(profile + 0x10) as i32 as u32);
    let at = 0x0300_538Cu32.wrapping_add(g.u32(0x0300_0060));
    g.set_u8(at, g.u32(0x0300_53BC) as u8);
    let p = g.u32(PROFILE);
    g.set_u8(p + 0x1FC, g.u8(p + EVENT_CURSOR + g.u8(p + ZONE) as u32));
}

/// `FUN_0812CF48` (screen, event): 1 when a career hint is due before going to `screen` (the hint screen 0x28):
/// the hints seen so far (profile `+0x1F8` + `+0x1F9`) pick the next one, per zone, by the screen it comes before
/// and conditions on the event, the car record bits (`+0x450…+0x453`) and the race mode. Clears `+0x1FA`.
fn hint_due(g: &mut Gba, screen: i32, event: i32) -> u32 {
    let p = g.u32(PROFILE);
    let n = g.u8(p + 0x1F8) as i32 + g.u8(p + 0x1F9) as i32;
    g.set_u8(p + 0x1FA, 0);
    if g.u32(CAREER) == 0 {
        return 0;
    }
    let z = g.u8(p + ZONE);
    let bit = |g: &Gba, off: u32, b: u32| (g.u8(p + off) as u32 >> b) & 1 != 0;
    let boss = |g: &Gba, off: u32, k: u32| {
        g.u8(p + 0x1FC) as u32 == (g.u16(0x087E_4714 + off) as i16 as i32 as u32).wrapping_sub(k)
    };
    let is = |k: i32, s: i32| n == k && screen == s;
    let mode = g.u32(0x0300_56E0) & 0xFF;
    let fine = (screen != 10 || g.u32(0x0300_0070).checked_shr(mode).unwrap_or(0) & 1 != 0)
        && (z != 0
            || !(is(0, 3)
                || is(1, 0xD)
                || is(2, 0xD)
                || is(3, 6)
                || (g.u16(p + 0x12) == 0 && screen == 0xD)
                || (is(4, 6) && bit(g, 0x450, 5))))
        && (z != 1
            || !(is(5, 0x2D)
                || is(6, 6)
                || (is(7, 0x2D) && event_status(g, event) == 3)
                || (is(8, 0x2D) && boss(g, 4, 0xC))
                || (is(9, 6) && bit(g, 0x450, 1))))
        && (z != 2 || !(is(10, 0xD) || (is(0xB, 0x2D) && boss(g, 10, 0x18)) || (is(0xC, 6) && bit(g, 0x450, 2))))
        && (z != 3
            || !(is(0xD, 0x2D)
                || is(0xE, 6)
                || is(0xF, 0x2D)
                || (is(0x10, 0x2D) && boss(g, 0xC, 0x24))
                || (is(0x11, 0x2D) && boss(g, 0xE, 0x24))
                || (is(0x12, 6) && bit(g, 0x450, 3))))
        && (z != 4
            || !((is(0x13, 6) && bit(g, 0x451, 1))
                || (is(0x14, 0x2D) && boss(g, 0x12, 0x30))
                || (is(0x15, 6) && bit(g, 0x450, 4))))
        && (z != 5 || !(is(0x16, 0xD) || (is(0x17, 6) && bit(g, 0x453, 4))));
    (!fine) as u32
}

// The career event screen (Event: screen 13), page `0x7E5090`.
const EVENT_PAGE: u32 = 0x087E_5090;

/// `career_event_enter` (`0x0812E308`): the page's background (`+6`, palette `+8`); career mode on.
pub fn event_enter(g: &mut Gba) -> u32 {
    let page = if g.u32(SCREEN) == 0xD { EVENT_PAGE } else { 0 };
    let (m, p) = (
        g.u16(page + 6) as i16 as i32 as u32,
        g.u16(page + 8) as i16 as i32 as u32,
    );
    menu_scene_setup(g, m, p, 0xFFFF);
    g.set_u32(CAREER, 1);
    0 // returns nothing (r0 is the last call's)
}

/// `career_event_update` (`0x0812DAFC`): the cursor over the zone's 12 events (6 in zone 5) in rows of 3;
/// SELECT opens the district map (0xE, profile `+0x404` 1); A on an open event (boss races need their unlock,
/// events past 0x3C their predecessor won) sets the race up, then shows a due hint (0x28) or goes on.
pub fn event_update(g: &mut Gba) -> u32 {
    let page = if g.u32(SCREEN) == 0xD { EVENT_PAGE } else { 0 };
    let items = g.u32(page + 0x10);
    let profile = g.u32(PROFILE);
    let count: i32 = if g.u8(profile + ZONE) == 5 { 6 } else { 12 };
    if g.u16(KEYS) & 0x200 != 0 {
        g.unported(CARBON_PLAY_SOUND, &[4, 1]);
        goto_screen(g, 0xE);
        g.set_u8(g.u32(PROFILE) + 0x404, 1);
        mark_screen_changed(g);
    }
    let cursor = |g: &Gba| g.u32(PROFILE) + EVENT_CURSOR + g.u8(g.u32(PROFILE) + ZONE) as u32;
    let add = |g: &mut Gba, v: i32| {
        let c = cursor(g);
        g.set_u8(c, (g.i8(c) as i32 + v) as u8);
    };
    if g.u16(KEYS) & 0x20 != 0 {
        let c = cursor(g);
        if g.u8(c) == 0 {
            g.set_u8(c, count as u8);
        }
        add(g, -1);
    }
    if g.u16(KEYS) & 0x10 != 0 {
        add(g, 1);
        let c = cursor(g);
        if count <= g.i8(c) as i32 {
            g.set_u8(c, 0);
        }
    }
    if g.u16(KEYS) & 0x40 != 0 {
        add(g, -3);
        if g.i8(cursor(g)) < 0 {
            add(g, count);
        }
    }
    if g.u16(KEYS) & 0x80 != 0 {
        add(g, 3);
        if count - 1 < g.i8(cursor(g)) as i32 {
            add(g, -count);
        }
    }
    if g.u16(KEYS) & 0xF0 != 0 {
        g.unported(CARBON_PLAY_SOUND, &[4, 1]);
        mark_screen_changed(g);
    }
    if g.u16(KEYS) != 1 {
        return 1;
    }
    let next = g.u16(items + 6) as i16 as i32;
    if next != -1 {
        let p = g.u32(PROFILE);
        let zone = g.u8(p + ZONE) as u32;
        let event = (zone * 12) as i32 + g.i8(p + EVENT_CURSOR + zone) as i32;
        let locked = (g.u16(0x087E_4714 + zone * 4) as i16 as i32 == event
            && unlock_is_locked(g, zone as i32 + 0x122) != 0)
            || (g.u16(0x087E_4714 + (zone * 2 + 1) * 2) as i16 as i32 == event
                && unlock_is_locked(g, g.u8(g.u32(PROFILE) + ZONE) as i32 + 0x11D) != 0)
            || (0x3C < event && event_status(g, event - 1) != 1);
        if locked {
            g.unported(CARBON_PLAY_SOUND, &[0x27, 1]);
            return 1;
        }
        for i in 0..4 {
            g.set_u8(0x0300_565B - i, 0);
        }
        career_event_to_globals(g);
        if hint_due(g, 0x2D, event) != 0 {
            g.unported(CARBON_PLAY_SOUND, &[2, 1]);
            goto_screen(g, 0x28);
            mark_screen_changed(g);
            return 1;
        }
        goto_screen(g, next);
        mark_screen_changed(g);
    }
    g.unported(CARBON_PLAY_SOUND, &[2, 1]);
    1
}

/// `career_event_screen` (`0x0812DD80`): the zone's event grid (4 rows of 3; 2 in zone 5) with the cursor, the
/// boss races' lock state, the mode icons, won and second marks, then the selected event's track, mode, reward
/// (the next win's, halved once won, times the car's style percentage) and record time.
pub fn event_draw(g: &mut Gba, _full: u32) -> u32 {
    let profile = g.u32(PROFILE);
    let page = if g.u32(SCREEN) == 0xD { EVENT_PAGE } else { 0 };
    let zone = g.u8(g.u32(PROFILE) + ZONE) as i32;
    let cursor = g.u8(g.u32(PROFILE) + EVENT_CURSOR + zone as u32) as i32;
    let event = zone * 12 + cursor;
    let blit = |g: &mut Gba, m: u32, x: i32, y: i32| {
        g.unported(MENU_BLIT_MATERIAL, &[WORLD, m, x as u32, y as u32]);
    };
    g.unported(INTRO_PAGE_SETUP, &[g.u32(0x0300_57F0)]);
    let neg1 = u32::MAX;
    g.unported(TEXT_MENU, &[0xC, g.u16(page) as i16 as i32 as u32, 0xEC, 2, neg1, 0]);
    let z = g.u8(g.u32(PROFILE) + ZONE) as u32;
    g.unported(TEXT_MENU, &[0xC, z + 0x3C3, 2, 2, 0, 0]);
    let (col, row);
    if g.u8(g.u32(PROFILE) + ZONE) == 5 {
        for r in 0..2 {
            let y = r * 0x1C;
            for c in 0..3 {
                let (x9, x11, x7) = (8 + 0x28 * c, 0x10 + 0x28 * c, 7 + 0x28 * c);
                let e = g.u8(g.u32(PROFILE) + ZONE) as i32 * 12 + r * 3 + c;
                if cursor == r * 3 + c {
                    blit(g, 0x90, x9, y + 0x36);
                } else {
                    blit(g, 0x8F, x9, y + 0x36);
                    blit(g, 0x99, x7, y + 0x35);
                }
                if e != 0x3C && event_status(g, e - 1) != 1 {
                    blit(g, 0xCD, x11, y + 0x38);
                }
                if event_status(g, e) == 1 {
                    blit(g, 0xAB, x11, y + 0x39);
                }
            }
        }
        (col, row) = (cursor as u32 % 3, ((cursor as u32 / 3) & 0xFF) + 1);
    } else {
        for r in 0..4 {
            let y = r * 0x1C;
            for c in 0..3 {
                let (x9, x11, x7) = (8 + 0x28 * c, 0x10 + 0x28 * c, 7 + 0x28 * c);
                let zone = g.u8(g.u32(PROFILE) + ZONE) as u32;
                let e = (zone * 12) as i32 + r * 3 + c;
                let boss = [
                    (g.u16(0x087E_4714 + zone * 4) as i16 as i32, 0x122),
                    (g.u16(0x087E_4714 + (zone * 2 + 1) * 2) as i16 as i32, 0x11D),
                ]
                .into_iter()
                .find(|&(b, _)| b == e);
                if let Some((_, unlock)) = boss {
                    if cursor == r * 3 + c {
                        blit(g, 0x90, x9, y + 0x18);
                    } else {
                        blit(g, 0x8F, x9, y + 0x18);
                        blit(g, 0x99, x7, y + 0x17);
                    }
                    let id = g.u8(g.u32(PROFILE) + ZONE) as i32 + unlock;
                    if unlock_is_locked(g, id) == 0 {
                        if event_status(g, e) == 1 {
                            blit(g, 0xAB, x11, y + 0x1B);
                        }
                    } else {
                        blit(g, 0xCD, x11, y + 0x1A);
                    }
                } else {
                    let mode = g.i8(0x087E_4744 + 8 * e as u32 + 2) as i32;
                    let icon = |g: &Gba, k: i32| g.u16(0x087E_5078_u32.wrapping_add((k * 2) as u32)) as u32;
                    if cursor == r * 3 + c {
                        blit(g, icon(g, mode + 4), x9, y + 0x18);
                    } else {
                        blit(g, icon(g, mode), x9, y + 0x18);
                        blit(g, 0x99, x7, y + 0x17);
                    }
                    let st = event_status(g, e);
                    if st == 1 {
                        blit(g, 0xAB, x11, y + 0x1B);
                    }
                    if st == 2 {
                        blit(g, 0xAC, x11, y + 0x1B);
                    }
                }
            }
        }
        (col, row) = (cursor as u32 % 3, (cursor as u32 / 3) & 0xFF);
    }
    blit(g, 0x9B, ((col & 0xFF) * 0x28 + 3) as i32, (row * 0x1C + 0x12) as i32);
    g.unported(TEXT_MENU, &[0xD, 0x2F1, 0xB4, 0x16, 1, 8]);
    let rec = 0x087E_4744 + 8 * event as u32;
    let track = g.i8(rec + 1) as i32;
    let name = g.u16(0x087E_4A70_u32.wrapping_add((track * 4) as u32)) as u32;
    g.unported(TEXT_BOX, &[0xD, name, 0xB4, 0x22, 0x70, 2, 0]);
    g.unported(TEXT_MENU, &[0xD, 0x131, 0xB4, 0x3E, 1, 8]);
    let mode = g.i8(rec + 2) as i32;
    let mode_name = g.u16(0x087E_5070_u32.wrapping_add((mode * 2) as u32)) as u32;
    g.unported(TEXT_MENU, &[0xD, mode_name, 0xB4, 0x4A, 1, 0]);
    g.unported(TEXT_MENU, &[0xD, 0x196, 0xB4, 0x5A, 1, 8]);
    let ladder = zone_ladder_index(g, g.u8(profile + ZONE) as i32);
    let reward = if event_status(g, event) == 3 {
        g.u16(0x087E_4744_u32.wrapping_add((ladder * 8 + 6) as u32)) as i16 as i32
    } else {
        (g.u16(0x087E_4744_u32.wrapping_add(((ladder - 1) * 8 + 6) as u32)) as i16 as i32) >> 1
    };
    let pct = nfsgba_formats::career::reward_percent(career_style_rating(g));
    let cash = nfsgba_fixed::div(pct * reward, 100);
    let mut s = number_text(g, cash);
    thousands(g, &mut s, cash);
    let arg = g.text_arg(s);
    g.unported(TEXT_MENU, &[0xE, arg, 0xB4, 0x66, 1, 0]);
    g.unported(TEXT_MENU, &[0xD, 0x1B2, 0xB4, 0x76, 1, 8]);
    let t = if track > 0xB { track - 0xC } else { track };
    let record = g.u16(profile.wrapping_add((0x218 + t * 2) as u32)) as i32;
    let s = time_text(g, frames_to_centiseconds(record));
    let arg = g.text_arg(s);
    g.unported(TEXT_MENU, &[0xE, arg, 0xB4, 0x82, 1, 0]);
    let (l, r) = (
        g.u16(page + 2) as i16 as i32 as u32,
        g.u16(page + 4) as i16 as i32 as u32,
    );
    g.unported(MENU_BUTTON_PROMPTS, &[l, r, 0x1F5]);
    0
}

// The race results (Career kind: 0xB the record and unlocks page, 0xC the standings). Results at `0x03005650`,
// ranked copy at `0x03005730` (0x40 bytes: `+4` entity ids, `+8` knocked out (8), `+0x10` best laps, `+0x20`
// finish times, `+0x30` hunter life).
const RESULTS: u32 = 0x0300_5650;
const RANKED: u32 = 0x0300_5730;
const OPPONENTS: u32 = 0x0300_5784;
const RACE_MODE: u32 = 0x0300_56E0;
const RESULT_PAGES: u32 = 0x087E_510C;
const CAREER_RACE_PAYOUT: u32 = 0x0812_EFE8; // (): `career::race_payout` models it; not wired to RAM here
const MENU_BLIT_MATERIAL_ALT: u32 = 0x0813_6E60; // (world, material, x, y)

/// `rank_results` (`0x0812E8E4`, key, descending) on the ranked results: [`nfsgba_formats::career::rank_results`] over
/// `*0x03005784 + 1` slots.
fn rank_results(g: &mut Gba, key: u32, descending: bool) {
    let mut t: [u8; 0x40] = std::array::from_fn(|i| g.u8(RANKED + i as u32));
    nfsgba_formats::career::rank_results(&mut t, g.u32(OPPONENTS), key, descending);
    for (i, &b) in t.iter().enumerate() {
        g.set_u8(RANKED + i as u32, b);
    }
}

fn result_page(g: &Gba) -> u32 {
    match g.u32(SCREEN) {
        0xB => RESULT_PAGES,
        0xC => RESULT_PAGES + 0x18,
        _ => 0,
    }
}

/// `career_zone_enter` (`0x0812F204`). On 0xB (after a race): clears the record flags, zeroes the ranked slots past
/// the racers, copies the results (`FUN_0812EB70`), ranks an elimination by time then knock-out, clears the payout,
/// and outside race phases 6–8 checks the record time (`FUN_0812F180`, not in two-player career) and pays out; with
/// no new record it goes straight to the standings (0xC). Then the page's background.
pub fn career_zone_enter(g: &mut Gba) -> u32 {
    let mut page = result_page(g);
    if g.u32(SCREEN) == 0xB {
        let p = g.u32(PROFILE);
        g.set_u32(p + 0x3B4, 0);
        g.set_u16(p + 0x4A8, 0);
        g.set_u16(p + 0x4AA, 0);
        for i in g.i32(OPPONENTS) + 1..4 {
            let i = i as u32;
            g.set_u32(RANKED + 0x10 + 4 * i, 0);
            g.set_u32(RANKED + 0x20 + 4 * i, 0);
            g.set_u32(RANKED + 0x30 + 4 * i, 0);
            g.set_u8(RANKED + 8 + i, 0);
        }
        // FUN_0812EB70: the results into the ranked table.
        for i in 0..0x40 {
            g.set_u8(RANKED + i, g.u8(RESULTS + i));
        }
        if g.u32(RACE_MODE) == 1 {
            rank_results(g, 1, false);
            for i in 9..0xC {
                g.set_u8(RANKED + i, 8);
            }
            rank_results(g, 2, false);
        }
        g.set_u32(g.u32(PROFILE) + 0x3B8, 0);
        if g.u32(0x0300_0048).wrapping_sub(6) > 2 {
            if g.i32(CAREER) < 2 {
                record_time(g);
            }
            g.unported(CAREER_RACE_PAYOUT, &[]);
        }
        if g.u32(g.u32(PROFILE) + 0x3B4) == 0 {
            let at = g
                .u32(PROFILE)
                .wrapping_add(0x344)
                .wrapping_add(g.i8(BACK_TOP) as i32 as u32);
            g.set_u8(at, 0xC);
            g.set_u32(SCREEN, 0xC);
            page = RESULT_PAGES + 0x18;
        }
    }
    let (m, pal) = (
        g.u16(page + 8) as i16 as i32 as u32,
        g.u16(page + 10) as i16 as i32 as u32,
    );
    menu_scene_setup(g, m, pal, 0xFFFF);
    1
}

/// `FUN_0812F180`: a new track record (profile `+0x218` per track, u16 frames) when the player's best lap beats
/// it in a finished race of a known mode; sets the record flags `+0x3B4` and `+0x4A8`.
fn record_time(g: &mut Gba) {
    let p = g.u32(PROFILE);
    let mut t = g.u32(0x087E_49C4_u32.wrapping_add(g.u32(ROUTE).wrapping_mul(4))) as i32;
    if t > 0xB {
        t -= 0xC;
    }
    let at = p.wrapping_add((0x218 + 2 * t) as u32);
    let old = g.u16(at) as i32;
    let mode = g.i32(RACE_MODE);
    if (0..4).contains(&mode) {
        rank_results(g, 2, false);
        if g.u32(0x0300_0048) != 8 && g.u8(RANKED + 8) != 8 && g.i32(RANKED + 0x10) < old {
            g.set_u16(at, g.u32(RANKED + 0x10) as u16);
            let p = g.u32(PROFILE);
            g.set_u32(p + 0x3B4, 1);
            g.set_u16(p + 0x4A8, 1);
        }
    }
}

/// `career_zone_update` (`0x0812F364`): A on 0xB clears the record flag and, with no unlock message left, goes to
/// the standings (0xC); A on 0xC ranks by position and returns to the career menu (screen 6), dropping the back
/// stack to below the first 0xC.
pub fn career_zone_update(g: &mut Gba) -> u32 {
    if g.u32(SCREEN) == 0xB {
        if g.u16(KEYS) == 1 {
            g.unported(CARBON_PLAY_SOUND, &[2, 1]);
            let p = g.u32(PROFILE);
            if g.u16(p + 0x4A8) != 0 {
                g.set_u16(p + 0x4A8, 0);
                g.set_u8(p + 0x340, 0);
                g.set_u16(KEYS, 0);
                if g.u16(g.u32(PROFILE) + 0x4AA) != 0 {
                    return 1;
                }
            }
            g.set_u8(BACK_TOP, g.u8(BACK_TOP).wrapping_sub(1));
            goto_screen(g, 0xC);
        }
    } else if g.u16(KEYS) == 1 {
        g.unported(CARBON_PLAY_SOUND, &[2, 1]);
        rank_results(g, 2, false);
        if g.u32(SCREEN) == 0xC {
            let top = g.i8(BACK_TOP) as i32;
            let mut i = 0;
            let mut found = false;
            if top > 0 {
                while g.u8(g.u32(PROFILE).wrapping_add(0x344 + i as u32)) != 0xC {
                    i += 1;
                    if g.i8(BACK_TOP) as i32 <= i {
                        break;
                    }
                }
                found = (g.i8(BACK_TOP) as i32) > i;
            }
            if found {
                g.set_u8(BACK_TOP, i as u8);
            }
            g.set_u8(BACK_TOP, g.u8(BACK_TOP).wrapping_sub(1));
            goto_screen(g, 6);
            mark_screen_changed(g);
        }
    }
    1
}

/// A racer's name for the standings: the player's (profile), a boss's (`0x7E4954[id − 0x10]`) or an opponent's
/// (`id + 0xA4` above 0x3F).
fn racer_name(g: &Gba, id: u8) -> u32 {
    match id {
        0 => g.u32(PROFILE),
        0x40.. => id as u32 + 0xA4,
        _ => g.u16(0x087E_4954_u32.wrapping_add(((id as i32 - 0x10) * 2) as u32)) as u32,
    }
}

/// `career_zone_draw` (`0x0812F450`). 0xB: the new record (track, best lap) or the unlock messages
/// (`FUN_0812EC68`). 0xC: the standings per race mode (hunter: life; circuit and elimination: finish time and best
/// lap, dashes when knocked out; sprint: finish time), the payout, the prompts.
pub fn career_zone_draw(g: &mut Gba, _full: u32) -> u32 {
    let page = result_page(g);
    g.unported(INTRO_PAGE_SETUP, &[g.u32(0x0300_57F0)]);
    let prompts = |g: &mut Gba| {
        let (l, r) = (
            g.u16(page + 4) as i16 as i32 as u32,
            g.u16(page + 6) as i16 as i32 as u32,
        );
        g.unported(MENU_BUTTON_PROMPTS, &[l, r, u32::MAX]);
    };
    let text = |g: &mut Gba, font: u32, key: u32, x: u32, y: u32, a: u32, c: u32| {
        g.unported(TEXT_MENU, &[font, key, x, y, a, c]);
    };
    let alt = |g: &mut Gba, m: u32, x: u32, y: u32| {
        g.unported(MENU_BLIT_MATERIAL_ALT, &[WORLD, m, x, y]);
    };
    if g.u32(SCREEN) == 0xB {
        let p = g.u32(PROFILE);
        if g.u16(p + 0x4A8) == 0 {
            if g.u16(p + 0x4AA) == 0 {
                return 0;
            }
            unlock_messages(g);
            prompts(g);
            return 0;
        }
        let slot = g.u32(0x087E_49C4_u32.wrapping_add(g.u32(ROUTE).wrapping_mul(4)));
        rank_results(g, 2, false);
        g.unported(MENU_BLIT_MATERIAL, &[WORLD, 6, 0x20, 0x1F]);
        text(g, 0xD, 0x165, 0x78, 0x3B, 1, 8);
        let name = g.u16(0x087E_4A70_u32.wrapping_add(slot.wrapping_mul(4))) as u32;
        text(g, 0xD, name, 0x78, 0x48, 1, 8);
        let s = time_text(g, frames_to_centiseconds(g.i32(RANKED + 0x10)));
        let arg = g.text_arg(s);
        text(g, 0xD, arg, 0x78, 0x55, 1, 0);
        g.unported(MENU_BLIT_MATERIAL, &[WORLD, 0x9C, 100, 0x61]);
        text(g, 0xD, 0xD7, 0x78, 100, 0, 0);
        return 0;
    }
    let heading = g.u32(page);
    if heading != u32::MAX {
        text(g, 0xC, heading, 0xEC, 2, u32::MAX, 0);
    }
    match g.i32(RACE_MODE) {
        2 => rank_results(g, 4, true),
        0 | 1 | 3 => rank_results(g, 1, false),
        _ => {}
    }
    match g.i32(RACE_MODE) {
        2 => {
            alt(g, 0xD6, 0x88, 0x17);
            text(g, 0xE, 0x157, 0xA8, 0x1B, 1, 0);
            let mut k = 0;
            while k <= g.u32(OPPONENTS) {
                let y = 0x10 * k;
                alt(g, 0xD2, 0x28, y + 0x27);
                alt(g, 0xD3, 0x88, y + 0x27);
                let name = racer_name(g, g.u8(RANKED + 4 + k));
                text(g, 0xE, name, 0x30, y + 0x2B, 0, 8);
                let v = (g.u32(RANKED + 0x30 + 4 * k).wrapping_mul(100) >> 0x13) as i32;
                let mut s = number_text(g, v);
                thousands(g, &mut s, v);
                let arg = g.text_arg(s);
                text(g, 0xE, arg, 0xA8, y + 0x2B, 1, 8);
                k += 1;
            }
        }
        0 | 1 => {
            alt(g, 0xD6, 0x68, 0x17);
            alt(g, 0xD6, 0xA8, 0x17);
            text(g, 0xE, 0x2CB, 0x88, 0x1B, 1, 0);
            text(g, 0xE, 0x94, 200, 0x1B, 1, 0);
            let mut k = 0;
            while k <= g.u32(OPPONENTS) {
                let y = 0x10 * k;
                alt(g, 0xD2, 8, y + 0x27);
                alt(g, 0xD3, 0x68, y + 0x27);
                alt(g, 0xD3, 0xA8, y + 0x27);
                let name = racer_name(g, g.u8(RANKED + 4 + k));
                text(g, 0xE, name, 0x10, y + 0x2B, 0, 8);
                if g.i8(RANKED + 8 + k) == 8 {
                    text(g, 0xE, 0x155, 0x88, y + 0x2B, 1, 8);
                    let mut s = b"--:--:--".to_vec();
                    match g.u32(LANGUAGE) {
                        3 => s[5] = b',',
                        1 | 2 | 4 => s[5] = b'.',
                        _ => {}
                    }
                    let arg = g.text_arg(s);
                    text(g, 0xE, arg, 200, y + 0x2B, 1, 8);
                } else {
                    let s = time_text(g, frames_to_centiseconds(g.i32(RANKED + 0x20 + 4 * k)));
                    let arg = g.text_arg(s);
                    text(g, 0xE, arg, 0x88, y + 0x2B, 1, 8);
                    let s = time_text(g, frames_to_centiseconds(g.i32(RANKED + 0x10 + 4 * k)));
                    let arg = g.text_arg(s);
                    text(g, 0xE, arg, 200, y + 0x2B, 1, 8);
                }
                k += 1;
            }
        }
        3 => {
            alt(g, 0xD6, 0x88, 0x17);
            text(g, 0xE, 0x2CB, 0xA8, 0x1B, 1, 0);
            let mut k = 0;
            while k <= g.u32(OPPONENTS) {
                let y = 0x10 * k;
                alt(g, 0xD2, 0x28, y + 0x27);
                alt(g, 0xD3, 0x88, y + 0x27);
                let name = racer_name(g, g.u8(RANKED + 4 + k));
                text(g, 0xE, name, 0x30, y + 0x2B, 0, 8);
                if g.i8(RANKED + 8 + k) == 8 {
                    text(g, 0xE, 0x155, 0xA8, y + 0x2B, 1, 8);
                } else {
                    let s = time_text(g, frames_to_centiseconds(g.i32(RANKED + 0x20 + 4 * k)));
                    let arg = g.text_arg(s);
                    text(g, 0xE, arg, 0xA8, y + 0x2B, 1, 8);
                }
                k += 1;
            }
        }
        _ => {}
    }
    let p = g.u32(PROFILE);
    if g.u32(p + 0x3B8) != 0 {
        alt(g, 0xD2, 0x28, 0x70);
        alt(g, 0xD3, 0x88, 0x70);
        text(g, 0xE, 0x196, 0x2C, 0x74, 0, 8);
        let v = g.i32(p + 0x3B8);
        let mut s = number_text(g, v);
        thousands(g, &mut s, v);
        let arg = g.text_arg(s);
        text(g, 0xE, arg, 0xA8, 0x74, 1, 8);
    }
    rank_results(g, 2, false);
    prompts(g);
    0
}

/// `FUN_0812EC68`: the unlock messages after a race (heading 0xD6, then each text key in profile `+0x4AA…`, 0x31A
/// skipped, headings of new cars and districts 2 rows lower and highlighted).
fn unlock_messages(g: &mut Gba) {
    g.unported(TEXT_BOX, &[0xE, 0xD6, 0x78, 2, 0xF0, 2, 8]);
    let mut y = 0xD;
    if g.u16(g.u32(PROFILE) + 0x4AA) == 0 {
        return;
    }
    let mut k = 2;
    loop {
        let key = g.u16(g.u32(PROFILE) + 0x4A8 + k) as u32;
        k += 2;
        if key != 0x31A {
            if matches!(key, 0x194 | 0x39A | 0xC6 | 0x3CF | 0xA3 | 0x3C1) {
                y += 2;
                g.unported(TEXT_MENU, &[0xE, key, 0x78, y, 1, 8]);
            } else {
                g.unported(TEXT_MENU, &[0xE, key, 0x78, y, 1, 0]);
            }
        }
        y += 0xB;
        if g.u16(g.u32(PROFILE) + 0x4A8 + k) == 0 {
            break;
        }
    }
}

// The settings screens (Setup: 0x10 options, 0xA Quick Play, 0xF career race; records at `0x7E6260`,
// `career::setup_screens`). Cursor per screen at profile `+0x368 + index`; arrow delays at `+0x374 + 2·item`.
const SETUP_SCREENS: u32 = 0x087E_6260;
const SETTINGS_CHANGED: u32 = 0x0300_5998;
const SAVE_WRITE_PROFILE: u32 = 0x0814_9FD8;

/// The setup record of the screen: 0x10 → 0, 0xA → 1, 0xF → 2..5 by race mode; −1 for anything else.
fn setup_index(g: &Gba) -> i32 {
    match g.u32(SCREEN) {
        0xF => match g.u32(RACE_MODE) {
            0 => 2,
            1 => 3,
            2 => 4,
            3 => 5,
            _ => -1,
        },
        0xA => 1,
        0x10 => 0,
        _ => -1,
    }
}

fn setup_page(index: i32) -> u32 {
    if index < 0 {
        0
    } else {
        SETUP_SCREENS + 0x10 * index as u32
    }
}

/// `setting_variable` (`0x08132790`): setting 0 is the race mode, 1 the wingman (profile `+0x200`), 2..=0x10 the
/// profile's setting copies `+0x3C0 + 4·(id − 2)`; others none (0).
fn setting_variable(g: &Gba, id: u32) -> u32 {
    match id {
        0 => RACE_MODE,
        1 => g.u32(PROFILE) + 0x200,
        2..=0x10 => g.u32(PROFILE) + 0x3C0 + 4 * (id - 2),
        _ => 0,
    }
}

/// The setting globals a settings screen edits through its profile copies (`+0x3C0…`).
const SETTING_GLOBALS: [(u32, u32); 5] = [
    (REVERSE, 0x3C0),
    (0x0300_56E4, 0x3C4), // laps
    (0x0300_5608, 0x3C8), // difficulty
    (OPPONENTS, 0x3CC),
    (0x0300_5604, 0x3D0), // traffic
];

/// `setup_screen_enter` (`0x081328F4`): the page's background, at most 2 opponents with a wingman, and the
/// settings copied into the profile (`+0x3BC…+0x3F4`; music and sound volumes / 8).
pub fn setup_enter(g: &mut Gba) -> u32 {
    let page = setup_page(setup_index(g));
    let (m, p) = (
        g.u16(page + 6) as i16 as i32 as u32,
        g.u16(page + 8) as i16 as i32 as u32,
    );
    menu_scene_setup(g, m, p, 0xFFFF);
    if g.u32(g.u32(PROFILE) + 0x200) != 0 && g.u32(OPPONENTS) > 2 {
        g.set_u32(OPPONENTS, 2);
    }
    let p = g.u32(PROFILE);
    g.set_u32(p + 0x3BC, 0);
    for (global, off) in SETTING_GLOBALS {
        g.set_u32(p + off, g.u32(global));
    }
    for (off, global) in [
        (0x3D4, 0x0300_580C),
        (0x3F4, 0x0300_0050),
        (0x3D8, 0x0300_53E4),
        (0x3DC, UNITS),
        (0x3E0, 0x0300_5698),
        (0x3E4, 0x0300_5798),
    ] {
        g.set_u32(p + off, g.u32(global));
    }
    g.set_u32(p + 0x3E8, g.u32(0x0300_578C) >> 3);
    g.set_u32(p + 0x3EC, g.u32(0x0300_53A4) >> 3);
    g.set_u32(p + 0x3F0, g.u32(LANGUAGE));
    g.set_u32(0x0300_5994, 0);
    1
}

/// `FUN_08135640` (type, text, arg): opens a message box when none is open (and swallows the keys); 1 if opened.
fn message_box_open(g: &mut Gba, kind: u32, text: u32, arg: u32) -> u32 {
    let open = g.i32(MESSAGE_BOX) < 0;
    if open {
        g.set_u32(MESSAGE_BOX, kind);
        g.set_u32(0x0300_59F8, text);
        g.set_u32(0x0300_59EC, arg);
        g.set_u16(KEYS, 0);
    }
    open as u32
}

/// `setup_screen_update` (`0x08132AB8`). Outside 0xA the profile copies drive the race globals; left/right step
/// the item's value within its range (at most 2 laps/opponents with a wingman; laps and opponents move together in
/// an elimination), up/down move the cursor, SELECT on 0xF opens the career menu (9). A runs the item's action:
/// a screen (Quick Play 0x81 may first show a hint, 0x2B), back when nothing changed, else the save question
/// (0x87). A "yes" on the options (0x10) writes the settings back and saves the profile.
pub fn setup_update(g: &mut Gba) -> u32 {
    let index = setup_index(g);
    let screen = nfsgba_formats::career::setup_screens(&g.rom[..])
        .get(index as usize)
        .cloned();
    let cursor_at = |g: &Gba| g.u32(PROFILE).wrapping_add(0x368).wrapping_add(index as u32);
    let item_of = |g: &Gba| screen.as_ref().map(|s| s.items[g.i8(cursor_at(g)) as usize].clone());
    let count = screen.as_ref().map_or(0, |s| s.items.len() as i32);
    let item = item_of(g).expect("setup item");
    let (min, mut max) = (item.min as i32, item.max as i32);
    let var = setting_variable(g, item.setting);
    if g.u32(SCREEN) != 0xA {
        let p = g.u32(PROFILE);
        for (global, off) in SETTING_GLOBALS {
            g.set_u32(global, g.u32(p + off));
        }
        g.set_u32(0x0300_580C, 0);
        g.set_u32(0x0300_0050, g.u32(p + 0x3F4));
        if g.u32(SCREEN) == 0xF && g.u16(KEYS) & 0x200 != 0 {
            g.unported(CARBON_PLAY_SOUND, &[4, 1]);
            goto_screen(g, 9);
            mark_screen_changed(g);
        }
        let p = g.u32(PROFILE);
        if ((g.u32(RACE_MODE) == 1 && var == p + 0x3C4) || var == p + 0x3CC) && g.u32(p + 0x200) != 0 {
            max = 2;
        }
        let delay = |g: &Gba, side: u32| g.u32(PROFILE) + 0x374 + side + 2 * g.i8(cursor_at(g)) as i32 as u32;
        if g.u16(KEYS) & 0x20 != 0 {
            if g.i32(var) == min {
                g.set_u32(var, (max + 1) as u32);
            }
            g.set_u32(var, (g.i32(var) - 1) as u32);
            g.set_u8(delay(g, 0), 3);
            g.unported(CARBON_PLAY_SOUND, &[4, 1]);
            mark_screen_changed(g);
        }
        if g.u16(KEYS) & 0x10 != 0 {
            let v = g.i32(var) + 1;
            g.set_u32(var, v as u32);
            if max < v {
                g.set_u32(var, min as u32);
            }
            g.set_u8(delay(g, 1), 3);
            g.unported(CARBON_PLAY_SOUND, &[4, 1]);
            mark_screen_changed(g);
        }
        if g.u32(RACE_MODE) == 1 {
            let p = g.u32(PROFILE);
            if var == p + 0x3C4 {
                g.set_u32(p + 0x3CC, g.u32(var));
            }
            if var == p + 0x3CC {
                g.set_u32(p + 0x3C4, g.u32(var));
            }
            g.set_u32(0x0300_56E4, g.u32(p + 0x3C4));
            g.set_u32(OPPONENTS, g.u32(p + 0x3CC));
        }
        if g.u16(KEYS) & 0x40 != 0 {
            let c = cursor_at(g);
            if g.i8(c) > 0 {
                g.set_u8(c, g.u8(c).wrapping_sub(1));
                g.unported(CARBON_PLAY_SOUND, &[1, 1]);
                mark_screen_changed(g);
            }
        }
        if g.u16(KEYS) & 0x80 != 0 {
            let c = cursor_at(g);
            if (g.i8(c) as i32) < count - 1 {
                g.set_u8(c, g.u8(c).wrapping_add(1));
                g.unported(CARBON_PLAY_SOUND, &[1, 1]);
                mark_screen_changed(g);
            }
        }
    }
    for i in 0..0x10 {
        let c = g.u32(PROFILE) + 0x374 + i;
        if g.i8(c) > 0 {
            g.set_u8(c, g.u8(c) - 1);
        }
    }
    let action = item_of(g).expect("setup item").action as u16 as i16 as i32;
    if g.u16(KEYS) == 1 && action != -1 {
        let p = g.u32(PROFILE);
        let changed = [
            (0x0300_53E4, 0x3D8),
            (UNITS, 0x3DC),
            (0x0300_5698, 0x3E0),
            (0x0300_5798, 0x3E4),
        ]
        .iter()
        .any(|&(global, off)| g.u32(global) != g.u32(p + off))
            || g.u32(0x0300_578C) >> 3 != g.u32(p + 0x3E8)
            || g.u32(0x0300_53A4) >> 3 != g.u32(p + 0x3EC)
            || g.u32(LANGUAGE) != g.u32(p + 0x3F0)
            || g.u32(0x0300_0050) != g.u32(p + 0x3F4);
        g.set_u32(SETTINGS_CHANGED, changed as u32);
        if action < 0x85 {
            if action == 0x81 {
                if g.u32(CAREER) == 1 && hint_due(g, 10, 0) != 0 {
                    g.unported(CARBON_PLAY_SOUND, &[2, 1]);
                    goto_screen(g, 0x2B);
                    mark_screen_changed(g);
                    return 1;
                }
                g.set_u8(BACK_TOP, 1);
            }
            goto_screen(g, action);
            mark_screen_changed(g);
        } else if g.u32(SETTINGS_CHANGED) == 0 {
            menu_back(g);
        } else if action == 0x87 {
            message_box_open(g, 2, 0x1D8, u32::MAX);
        }
        g.unported(CARBON_PLAY_SOUND, &[2, 1]);
    }
    if g.i32(MESSAGE_RESULT) > 0 {
        if g.u32(SCREEN) == 0x10 {
            if g.u32(SETTINGS_CHANGED) != 0 {
                let p = g.u32(PROFILE);
                for (global, off) in SETTING_GLOBALS {
                    g.set_u32(global, g.u32(p + off));
                }
                g.set_u32(0x0300_580C, 0);
                for (global, off) in [
                    (0x0300_53E4, 0x3D8),
                    (UNITS, 0x3DC),
                    (0x0300_5698, 0x3E0),
                    (0x0300_5798, 0x3E4),
                ] {
                    g.set_u32(global, g.u32(p + off));
                }
                g.set_u32(0x0300_578C, g.u32(p + 0x3E8) << 3);
                g.set_u32(0x0300_53A4, g.u32(p + 0x3EC) << 3);
                g.set_u32(LANGUAGE, g.u32(p + 0x3F0));
                g.set_u32(0x0300_0050, g.u32(p + 0x3F4));
                let save = g.u32(SAVE_BUFFER);
                g.unported(SAVE_WRITE_PROFILE, &[save]);
            }
            menu_back(g);
        }
        g.set_u32(MESSAGE_RESULT, 0);
    }
    1
}

/// `setup_screen_draw` (`0x08133074`): heading; on 0xA a summary (track or mode, car, wingman, blinking on bit 7
/// of the frame counter) and items from the fourth; every item's name and value text (`options[value]` in range,
/// else the text at `0x087E62C0`), the cursor highlight and, outside 0xA, the arrows; prompts (0xF adds 0xB5).
pub fn setup_draw(g: &mut Gba, _full: u32) -> u32 {
    let index = setup_index(g);
    let page = setup_page(index);
    let screens = nfsgba_formats::career::setup_screens(&g.rom[..]);
    let items = screens.get(index as usize).map(|s| s.items.clone()).unwrap_or_default();
    let count = g.u16(page + 10) as i16 as i32;
    let slot = g.u32(0x087E_49C4_u32.wrapping_add(g.u32(ROUTE).wrapping_mul(4)));
    let car = g.i8(g.u32(PROFILE) + 0x10) as i32 as u32;
    g.set_u32(PLAYER_CAR, car);
    g.unported(INTRO_PAGE_SETUP, &[g.u32(0x0300_57F0)]);
    let text = |g: &mut Gba, font: u32, key: u32, x: u32, y: u32, a: u32, c: u32| {
        g.unported(TEXT_MENU, &[font, key, x, y, a, c]);
    };
    let alt = |g: &mut Gba, m: u32, x: u32, y: u32| {
        g.unported(MENU_BLIT_MATERIAL_ALT, &[WORLD, m, x, y]);
    };
    text(g, 0xC, g.u16(page) as i16 as i32 as u32, 0xEC, 2, u32::MAX, 0);
    let (first, top, pitch) = if g.u32(SCREEN) == 0xA {
        for (m, x, y) in [
            (0xD2, 2, 0xF),
            (0xD5, 0x52, 0xF),
            (0xD2, 2, 0x1F),
            (0xD5, 0x52, 0x1F),
            (0xD2, 8, 0x2F),
            (0xD4, 0x78, 0x2F),
        ] {
            alt(g, m, x, y);
        }
        if g.u32(FLASH) & 0x80 == 0 {
            text(g, 0xE, 0x2E3, 3, 0x14, 0, 0);
            text(
                g,
                0xE,
                g.u16(0x087E_4A70_u32.wrapping_add(slot.wrapping_mul(4))) as u32,
                0x55,
                0x14,
                0,
                0,
            );
        } else {
            text(g, 0xE, 0x131, 3, 0x14, 0, 0);
            text(g, 0xE, g.u32(0x0879_9B6C + 4 * g.u32(RACE_MODE)), 0x55, 0x14, 0, 0);
        }
        text(g, 0xE, items[1].text_key, 3, 0x24, 0, 0);
        let car = if g.u32(CAREER) == 0 {
            g.i8(g.u32(PROFILE) + 0x11) as i32
        } else {
            g.i32(PLAYER_CAR)
        };
        text(
            g,
            0xE,
            g.u16(0x087E_517C_u32.wrapping_add((car * 8) as u32)) as i16 as i32 as u32,
            0x55,
            0x24,
            0,
            0,
        );
        text(g, 0xE, 0x3A5, 0xC, 0x34, 0, 0);
        let w = g.u32(g.u32(PROFILE) + 0x200);
        let (key, width) = if w == 0 {
            (0x3A6, 0x60)
        } else if g.u32(FLASH) & 0x80 == 0 {
            (if w & 1 == 0 { 0x113 } else { 0x90 }, 0x70)
        } else {
            (w + 0x3A6, 0x60)
        };
        g.unported(TEXT_BOX, &[0xE, key, 0xB0, 0x34, width, 2, 0]);
        (3, 0x13, 0x10)
    } else {
        (0, 0x18, 0x11)
    };
    let cursor_at = g.u32(PROFILE).wrapping_add(0x368).wrapping_add(index as u32);
    for i in first..count {
        let it = &items[i as usize];
        let on = |g: &Gba| i == g.i8(cursor_at) as i32;
        let y = pitch * i + top;
        if on(g) {
            alt(g, 0xD9, 8, (y - 4) as u32);
            alt(g, 0xD7, 0x78, (y - 4) as u32);
        } else {
            alt(g, 0xD2, 8, (y - 4) as u32);
            alt(g, 0xD4, 0x78, (y - 4) as u32);
        }
        let colour = if on(g) { 8 } else { 0 };
        text(g, 0xE, it.text_key, 0xC, (y + 1) as u32, 0, colour);
        let var = setting_variable(g, it.setting);
        let key = if var == 0 {
            it.options[0]
        } else {
            let v = g.i32(var);
            if (it.min as i32..=it.max as i32).contains(&v) {
                it.options[v as usize]
            } else {
                0x087E_62C0
            }
        };
        let colour = if on(g) { 8 } else { 0 };
        g.unported(TEXT_BOX, &[0xE, key, 0xB0, (y + 1) as u32, 0x70, 2, colour]);
        if g.u32(SCREEN) != 0xA && g.i32(MESSAGE_BOX) < 0 {
            let p = g.u32(PROFILE);
            let lit = |g: &Gba, off: u32| g.i8(p + off + 2 * i as u32) >= 1;
            let m = if lit(g, 0x374) { 0xAE } else { 0xAD };
            g.unported(MENU_BLIT_MATERIAL, &[WORLD, m, 0x76, (y - 4) as u32]);
            let m = if lit(g, 0x375) { 0xB0 } else { 0xAF };
            g.unported(MENU_BLIT_MATERIAL, &[WORLD, m, 0xDA, (y - 4) as u32]);
        }
    }
    let (l, r) = (
        g.u16(page + 2) as i16 as i32 as u32,
        g.u16(page + 4) as i16 as i32 as u32,
    );
    let third = if g.u32(SCREEN) == 0xF { 0xB5 } else { u32::MAX };
    g.unported(MENU_BUTTON_PROMPTS, &[l, r, third]);
    0
}

/// `setup_screen_15_prepare` (`FUN_0812B320`, entering screen 15): the career race's opponents, ids in the results
/// slots `0x03005655..=57`: three different random picks (`rand & 7`) of the zone's eight (`0x40 + 8·zone`, zone 5:
/// `0x60`), the first replaced by the zone's boss (`0x10 + 2·zone`, `+1`) on its boss events (and event 0x41), and
/// the wingman (`0x20 +` wingman) in the last opponent slot. In a two-player career before the first hint the
/// first opponent is fixed (0x2D).
fn career_opponents(g: &mut Gba) {
    let p = g.u32(PROFILE);
    let zone = g.u8(p + ZONE) as u32;
    let event = g.u8(p + 0x1FC) as i32 + (zone * 12) as i32;
    if g.u32(CAREER) == 2 && g.u8(p + 0x1F8) == 0 {
        g.set_u8(RESULTS + 5, 0x2D);
        return;
    }
    let a = rand_table(g) as u8 & 7;
    let b = loop {
        let v = rand_table(g) as u8 & 7;
        if v != a {
            break v;
        }
    };
    let c = loop {
        let v = rand_table(g) as u8 & 7;
        if v != b && v != a {
            break v;
        }
    };
    let p = g.u32(PROFILE);
    let z = g.i8(p + ZONE);
    let base = if z == 5 {
        0x60
    } else {
        (z as u8).wrapping_mul(8).wrapping_add(0x40)
    };
    let twice = (zone * 2) as u8;
    let mut first = a.wrapping_add(base);
    if g.u16(0x087E_4714 + zone * 4) as i16 as i32 == event {
        first = twice.wrapping_add(0x10);
    }
    if g.u16(0x087E_4714 + (zone * 2 + 1) * 2) as i16 as i32 == event {
        first = twice.wrapping_add(0x11);
    }
    if event == 0x41 {
        first = twice.wrapping_add(0x11);
    }
    g.set_u8(RESULTS + 5, first);
    g.set_u8(RESULTS + 6, b.wrapping_add(base));
    g.set_u8(RESULTS + 7, c.wrapping_add(base));
    let w = g.u32(p + 0x200);
    if w != 0 {
        g.set_u8(
            RESULTS.wrapping_add(g.u32(OPPONENTS)).wrapping_add(5),
            (w as u8).wrapping_add(0x20),
        );
    }
}

// The hint and story pages (Kind38: 0x26 hints, 0x27 the wingman's introduction, 0x2B mode hints; 0x28 save,
// 0x29 wait, 0x2A clear the frame buffers). Page records (0xC bytes: `+0` enter script, `+4` draw script, `+8`
// entries of 10 bytes: `+0`/`+2` background, `+4` line, `+6` text, `+8` action).
const PAGE_WAIT: u32 = 0x0300_59E8; // frame counter value the wait page (0x29) and script sounds wait for
const CARBON_STOP_SOUND: u32 = 0x0813_6028; // (id)
const CARBON_PLAY_MUSIC: u32 = 0x0813_6054; // (id)
const SND_STOP_MUSIC: u32 = 0x0815_240C; // ()
const FILL_RECT: u32 = 0x0816_4BEC; // (rect on the stack: x0, y0, x1, y1; page, pitch, colour × 0x01010101)

fn hint_record(g: &Gba) -> u32 {
    match g.u32(SCREEN) {
        0x26 => 0x087E_8570 + 0xC * g.u8(g.u32(PROFILE) + 0x1F8) as u32,
        0x27 => 0x087E_8534,
        _ => 0x087E_8540_u32.wrapping_add(g.u32(RACE_MODE).wrapping_mul(0xC)),
    }
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

/// `FUN_0813609C`: no music (`0x0300003C` −1).
fn stop_music(g: &mut Gba) {
    g.set_u32(0x0300_003C, u32::MAX);
    g.unported(SND_STOP_MUSIC, &[]);
}

/// `kind38_enter` (`0x08134DF0`): the entry's background (hints: entry `+0x1FA`) and the record's enter script.
pub fn kind38_enter(g: &mut Gba) -> u32 {
    let rec = hint_record(g);
    let mut entry = g.u32(rec + 8);
    if g.u32(SCREEN) == 0x26 {
        entry += 10 * g.u8(g.u32(PROFILE) + 0x1FA) as u32;
    }
    let first = g.u32(SCREEN_ENTERED) == 0 && g.u32(SCREEN) != 5;
    let (m, p) = (g.u16(entry) as i16 as i32 as u32, g.u16(entry + 2) as i16 as i32 as u32);
    menu_scene_setup_ab(g, first, m, p, 0xFFFF);
    page_script(g, g.u32(rec));
    1
}

/// `FUN_081348E8` (script): runs the section of a page script whose id is the current entry (hints: profile
/// `+0x1FA`; 0x27: 0; else the race mode). A script is u16 sections `[id, commands…, 0xFFFF]`; commands:
/// 0xFFF5 stop sound; 0xFFF6 wait and play a sound; 0xFFF7 play a sound; 0xFFF8 stop a sound once the wait is over;
/// 0xFFF9 stop the music; 0xFFFA play music; 0xFFFB a flash (palette entries 0xC0.. and a filled rectangle); 0xFFFC
/// a picture on the map grid (profile `+0x254`); 0xFFFD a portrait and its 64-colour palette.
fn page_script(g: &mut Gba, script: u32) {
    let w = |g: &Gba, i: u32| g.u16(script + 2 * i) as u32;
    let profile = g.u32(PROFILE);
    let second = g.u32(SECOND_PALETTE);
    let mut src = g.u32(WORLD + 0x30);
    let sel = match g.u32(SCREEN) {
        0x26 => g.u8(profile + 0x1FA) as u32,
        0x27 => 0,
        _ => g.u32(RACE_MODE),
    };
    let (mut i, mut at) = (1u32, 0u32); // `at`: the section header being looked at
    while (w(g, at) as i32) < sel as i32 {
        i += 1;
        let mut end = at + 2;
        if w(g, at + 1) != 0xFFFF {
            let mut k = i;
            loop {
                let v = w(g, k);
                k += 1;
                end += 1;
                i += 1;
                if v == 0xFFFF {
                    break;
                }
            }
        }
        i += 1;
        at = end;
    }
    if w(g, i - 1) != sel {
        return;
    }
    let mut c = w(g, i);
    while c != 0xFFFF {
        match c.wrapping_sub(0xFFF5) {
            0 => {
                g.unported(CARBON_STOP_SOUND, &[w(g, i + 1)]);
                i += 2;
            }
            1 | 2 => {
                if c == 0xFFF6 {
                    g.set_u32(PAGE_WAIT, w(g, i + 1).wrapping_add(g.u32(FLASH)));
                    i += 1;
                }
                g.unported(CARBON_PLAY_SOUND, &[w(g, i + 1), 1]);
                i += 2;
            }
            3 => {
                if g.u32(PAGE_WAIT) != 0 && g.i32(PAGE_WAIT) < g.i32(FLASH) {
                    g.set_u32(PAGE_WAIT, 0);
                    g.unported(CARBON_STOP_SOUND, &[w(g, i + 1)]);
                }
                i += 2;
            }
            4 => {
                stop_music(g);
                g.unported(VBLANK_INTR_WAIT, &[]);
                i += 1;
            }
            5 => {
                g.unported(CARBON_PLAY_MUSIC, &[w(g, i + 1)]);
                g.unported(VBLANK_INTR_WAIT, &[]);
                g.unported(VBLANK_INTR_WAIT, &[]);
                i += 2;
            }
            6 => {
                copy_mem(g, second, src, 0x180, 0x10);
                copy_mem(g, second + 0x180, 0x087E_6ED4, 0x18, 0x10);
                g.set_u32(PALETTE_DIRTY, 1);
                let (x, y) = (w(g, i + 2), w(g, i + 3));
                let rect = [x, y, x + 0x22, y + 0x16]
                    .iter()
                    .flat_map(|v| v.to_le_bytes())
                    .collect();
                let rect = g.text_arg(rect);
                let page = g.u32(g.u32(WORLD + 0x50));
                g.unported(FILL_RECT, &[rect, page, 0xF0, w(g, i + 1).wrapping_mul(0x0101_0101)]);
                i += 4;
            }
            7 => {
                let grid = g.u16(profile + 0x254);
                let x = (grid as u32 & 3) * 0x28 + w(g, i + 2);
                let y = ((grid as i16 as i32 >> 2) * 0x1E) as u32 + w(g, i + 3);
                g.unported(MENU_BLIT_MATERIAL, &[WORLD, w(g, i + 1), x, y]);
                i += 4;
            }
            8 => {
                copy_mem(g, second, src, 0x180, 0x10);
                let mut m = w(g, i + 1);
                if g.u32(SCREEN) == 0x27 {
                    m = g.u16(0x087E_78A0_u32.wrapping_add(g.u32(g.u32(PROFILE) + 0x200).wrapping_mul(2))) as u32;
                }
                // Portrait palettes: 0x40..=0x4C in order from 0x7E6EEC, then an irregular order.
                if let Some(p) = match m {
                    0x40..=0x4C => Some(0x087E_6EEC + 0x80 * (m - 0x40)),
                    0x4D => Some(0x087E_776C),
                    0x4E => Some(0x087E_77EC),
                    0x4F => Some(0x087E_75EC),
                    0x50 => Some(0x087E_76EC),
                    0x51 => Some(0x087E_766C),
                    0x52 => Some(0x087E_756C),
                    _ => None,
                } {
                    src = p;
                }
                copy_mem(g, second + 0x180, src, 0x80, 0x10);
                g.set_u32(PALETTE_DIRTY, 1);
                g.unported(MENU_BLIT_MATERIAL, &[WORLD, m, 0xA8, 0xC]);
                g.unported(MENU_BLIT_MATERIAL, &[WORLD, 0xDB, 0xA0, 2]);
                i += 2;
            }
            // NOT 1:1 (N1): an unknown command makes the game loop forever.
            _ => panic!("page script {script:#x}: unknown command {c:#x}"),
        }
        c = w(g, i);
    }
}

/// `FUN_08134CC0` (tutorial): sets up one of the three tutorial races (two-player career mode 2, the Cobalt with
/// fixed upgrades, route 1 or 3), picks its opponents and counts the hint.
fn tutorial_race(g: &mut Gba, which: u32) {
    g.set_u32(CAREER, 2);
    g.set_u32(0x0300_00BC, 0x50);
    g.set_u32(0x0300_5608, 1);
    g.set_u32(0x0300_5604, 0);
    g.set_u32(REVERSE, 0);
    g.set_u8(g.u32(PROFILE) + 0x10, 5);
    let records = g.u32(0x0300_539C);
    for i in 0..10 {
        g.set_u8(records + 0x5C + i, 0x7F);
    }
    for i in 0..5 {
        g.set_u8(g.u32(0x0300_539C) + 0x55 + i, 2);
    }
    g.set_u8(records + 0x5B, 9);
    let at = 0x0300_538Cu32.wrapping_add(g.u32(0x0300_0060));
    g.set_u8(at, g.u32(0x0300_53BC) as u8);
    let setup = match which {
        0 => Some((1, 0, 1, 1)),
        1 => Some((1, 1, 2, 2)),
        2 => Some((3, 2, 2, 3)),
        _ => None,
    };
    if let Some((route, wingman, opponents, laps)) = setup {
        g.set_u32(RACE_MODE, 0);
        g.set_u32(ROUTE, route);
        g.set_u32(g.u32(PROFILE) + 0x200, wingman);
        g.set_u32(OPPONENTS, opponents);
        g.set_u32(0x0300_56E4, laps);
    }
    career_opponents(g);
    let c = g.u32(PROFILE) + 0x1F9;
    g.set_u8(c, g.u8(c).wrapping_add(1));
}

/// `kind38_update` (`0x08134EB8`): the save/wait/clear transitions, the map grid (profile `+0x254`) on the zone-3
/// hint, A runs the entry's action (0x40 next page, 0x41 finish the hints: next zone, save, screen 3; 0x42 back;
/// 0x81 a tutorial race or a mode's first race; else a screen, 9 also resetting the cars and cash), B the page
/// before or back out.
pub fn kind38_update(g: &mut Gba) -> u32 {
    let p = g.u32(PROFILE);
    let rec = hint_record(g);
    let entry = g.u32(rec + 8) + 10 * g.u8(g.u32(PROFILE) + 0x1FA) as u32;
    match g.u32(SCREEN) {
        0x2A => {
            if g.u32(FADE) == 0 {
                let n = screen_bytes(g);
                fill32(g, g.u32(0x0300_641C), 0x0101_0101, n);
                let n = screen_bytes(g);
                fill32(g, g.u32(0x0300_6420), 0x0101_0101, n);
                g.set_u32(SCREEN, 0x26);
                g.set_u32(FADE, 0x10);
                kind38_enter(g);
            }
            return 1;
        }
        0x29 => {
            if g.i32(PAGE_WAIT) < g.i32(FLASH) {
                g.set_u32(SCREEN, 0x2A);
                g.set_u32(FADE, -0x10i32 as u32);
            }
            return 1;
        }
        0x28 => {
            if g.u32(FADE) == 0 {
                if g.u8(p + 0x1F9) != 0 {
                    g.set_u8(p + 0x1F8, g.u8(p + 0x1F9).wrapping_add(g.u8(p + 0x1F8)));
                    g.set_u8(g.u32(PROFILE) + 0x1F9, 0);
                    let save = g.u32(SAVE_BUFFER);
                    g.unported(SAVE_WRITE_PROFILE, &[save]);
                }
                stop_music(g);
                g.set_u32(SCREEN, 0x29);
                g.set_u32(PAGE_WAIT, g.u32(FLASH).wrapping_add(0x78));
                if g.u8(g.u32(PROFILE) + 0x1F8) == 0 {
                    menu_scene_setup_ab(g, false, 5, 5, 0xFFFF);
                } else {
                    menu_scene_setup_ab(g, false, 0xE2, 8, 0xFFFF);
                }
            }
            return 1;
        }
        _ => {}
    }
    if g.u32(p + 0x1F8) & 0x00FF_00FF == 0x0003_0002 {
        let grid = p + 0x254;
        if g.u16(KEYS) & 0x20 != 0 {
            if g.u16(grid) == 0 {
                g.set_u16(grid, 0xC);
            }
            g.set_u16(grid, g.u16(grid).wrapping_sub(1));
        }
        if g.u16(KEYS) & 0x10 != 0 {
            let v = g.u16(grid).wrapping_add(1);
            g.set_u16(grid, if v as i16 > 0xB { 0 } else { v });
        }
        if g.u16(KEYS) & 0x40 != 0 {
            let old = g.u16(grid);
            let v = old.wrapping_sub(4);
            g.set_u16(grid, if (v as i16) < 0 { old.wrapping_add(8) } else { v });
        }
        if g.u16(KEYS) & 0x80 != 0 {
            let old = g.u16(grid);
            let v = old.wrapping_add(4);
            g.set_u16(grid, if v as i16 > 0xB { old.wrapping_sub(8) } else { v });
        }
        if g.u16(KEYS) & 0xF0 != 0 {
            g.unported(CARBON_PLAY_SOUND, &[4, 1]);
            mark_screen_changed(g);
        }
    }
    if g.u16(KEYS) == 1 {
        g.unported(CARBON_PLAY_SOUND, &[2, 1]);
        let action = g.u16(entry + 8) as i16 as i32;
        let other = |g: &mut Gba| {
            g.unported(CARBON_PLAY_MUSIC, &[0]);
            if g.u16(entry + 8) as i16 == 9 {
                for car in 0..15 {
                    let rec = g.u32(0x0300_539C) + car * 0x11;
                    for k in 0..10 {
                        g.set_u8(rec + 7 + k, 0);
                    }
                    for k in 0..5 {
                        g.set_u8(g.u32(0x0300_539C) + car * 0x11 + k, 0);
                    }
                }
                g.set_u32(p + 0xC, 1000);
            }
            g.set_u8(p + 0x1F9, g.u8(p + 0x1F9).wrapping_add(1));
            g.set_u8(BACK_TOP, g.u8(BACK_TOP).wrapping_sub(1));
            g.set_u32(SCREEN, 0xD);
            goto_screen(g, g.u16(entry + 8) as i16 as i32);
        };
        match action {
            0x41 => {
                let q = g.u32(PROFILE);
                g.set_u8(q + 0x1F8, g.u8(q + 0x1F8).wrapping_add(1).wrapping_add(g.u8(q + 0x1F9)));
                g.set_u8(g.u32(PROFILE) + 0x1F9, 0);
                if g.u16(g.u32(PROFILE) + 0x256) != 0 {
                    let z = g.u32(PROFILE) + ZONE;
                    if g.u8(z) < 5 {
                        g.set_u8(z, g.u8(z) + 1);
                    }
                    g.set_u16(g.u32(PROFILE) + 0x256, 0);
                }
                let save = g.u32(SAVE_BUFFER);
                g.unported(SAVE_WRITE_PROFILE, &[save]);
                g.unported(CARBON_PLAY_MUSIC, &[0]);
                g.set_u8(g.u32(PROFILE) + 0x344, 0);
                g.set_u32(SCREEN, 3);
                g.set_u8(BACK_TOP, 0);
                enter_screen(g);
            }
            0x40 => {
                g.set_u8(p + 0x1FA, g.u8(p + 0x1FA).wrapping_add(1));
                enter_screen(g);
            }
            0x42 => menu_back(g),
            0x81 => {
                if g.u32(SCREEN) == 0x26 {
                    tutorial_race(g, g.u8(p + 0x1F8) as u32);
                } else {
                    g.set_u8(BACK_TOP, 1);
                    let bit = 1u32.checked_shl(g.u32(RACE_MODE) & 0xFF).unwrap_or(0);
                    g.set_u32(0x0300_0070, g.u32(0x0300_0070) | bit);
                }
                goto_screen(g, g.u16(entry + 8) as i16 as i32);
            }
            _ => other(g),
        }
        mark_screen_changed(g);
    }
    if g.u16(KEYS) == 2 {
        let c = p + 0x1FA;
        if g.u8(c) == 0 {
            g.unported(CARBON_STOP_SOUND, &[3]);
            g.unported(VBLANK_INTR_WAIT, &[]);
            g.unported(CARBON_PLAY_SOUND, &[3, 1]);
            for _ in 0..8 {
                g.unported(VBLANK_INTR_WAIT, &[]);
            }
            g.unported(CARBON_PLAY_MUSIC, &[0]);
            g.set_u16(KEYS, 0);
            g.set_u8(g.u32(PROFILE) + 0x341, 3);
            menu_back(g);
        } else {
            g.unported(CARBON_PLAY_SOUND, &[3, 1]);
            g.set_u8(c, g.u8(c).wrapping_sub(1));
            enter_screen(g);
        }
        mark_screen_changed(g);
    }
    1
}

/// `kind38_draw` (`0x08135340`): 0x29 clears the page (or keeps the menu picture before the first hint) and says
/// "hint n"; the others run the record's draw script, the entry's text (0x27: the wingman's) and the prompts.
pub fn kind38_draw(g: &mut Gba, _full: u32) -> u32 {
    let rec = hint_record(g);
    let entry = g.u32(rec + 8) + 10 * g.u8(g.u32(PROFILE) + 0x1FA) as u32;
    if g.u32(SCREEN) == 0x29 {
        if g.u8(g.u32(PROFILE) + 0x1F8) == 0 {
            g.unported(INTRO_PAGE_SETUP, &[g.u32(0x0300_57F0)]);
        } else {
            let (page, n) = (g.u32(g.u32(WORLD + 0x50)), screen_bytes(g));
            fill32(g, page, 0x0101_0101, n);
        }
        let key = g.u8(g.u32(PROFILE) + 0x1F8) as u32 + 0x1D9;
        g.unported(TEXT_MENU, &[0xE, key, 0x78, 0x46, 1, 0]);
        return 0;
    }
    g.unported(INTRO_PAGE_SETUP, &[g.u32(0x0300_57F0)]);
    page_script(g, g.u32(rec + 4));
    let text = g.u16(entry + 6) as i16 as i32;
    if text != -1 {
        if g.u32(SCREEN) == 0x27 {
            let key = (text as u32).wrapping_add(g.u32(g.u32(PROFILE) + 0x200));
            g.unported(TEXT_BOX, &[0xE, key, 0x51, 4, 0xA6, 0x10, 8]);
        } else {
            let y = (g.u16(entry + 4) as i16 as i32 * -0xB + 0x8E) as u32;
            g.unported(TEXT_BOX, &[0xE, text as u32, 0x78, y, 0xF0, 0x10, 8]);
        }
    }
    g.unported(MENU_BUTTON_PROMPTS, &[0x8D, 0x92, u32::MAX]);
    0
}

// The List screens (0–6, 9, 27–30, 35, 36, 45, 46): pages `0x7E544C` by `list_slot` (0x14 bytes: heading,
// prompts `+2`/`+4`, background `+6`/`+8`, item count `+10`, no-profile flag `+0xC`, items `+0x10`: 8 bytes each,
// `+0` text, `+2`/`+4` pictures, `+6` action). Cursor per slot at profile `+0x350 + slot`.
const LIST_PAGES: u32 = 0x087E_544C;
const GARAGE_LOAD_CAR_ATLAS: u32 = 0x0812_BEEC; // ()
const GARAGE_LOAD_CAR_PALETTE: u32 = 0x0812_BF48; // ()
const GARAGE_DRAW_CAR: u32 = 0x0812_BFA4; // (x, y, angle)
const QUICK_RACE_RANDOM: u32 = 0x0812_FFB0; // ()
const UNLOCK_STATE: u32 = 0x0812_C81C; // (unlock id): 1 owned, 2 locked, 0/3 not affordable (FUN_0812C81C)
const BUY_UNLOCK: u32 = 0x0812_C8A8; // (unlock id)
const UPGRADES_CHANGED: u32 = 0x0813_02C4; // (performance page?)
const UNLOCK_OWNED: u32 = 0x0812_C5C4; // (unlock id)
const LIST_ITEM_NEW: u32 = 0x0813_00E0; // (screen, item): a "new" mark
const CAR_STATS_DRAW: u32 = 0x0813_3D30; // (car, x, y, 0)
const UNLOCK_PRICE: u32 = 0x0812_D960; // (unlock id)

/// An i16 read, sign-extended as the game passes it.
fn s16(g: &Gba, a: u32) -> u32 {
    g.u16(a) as i16 as i32 as u32
}

fn list_page(g: &Gba) -> u32 {
    let slot = list_slot(g);
    if slot < 0 { 0 } else { LIST_PAGES + 0x14 * slot as u32 }
}

/// `garage_copy_car_record` (`FUN_0812D564`): the car record of `0x03005718` (17 bytes at `*0x0300539C`): bytes
/// 7..=16 to `0x03005707`, 0..=6 to `0x03005700` (the garage's working copy).
fn garage_copy_car_record(g: &mut Gba) {
    let at = g.u32(0x0300_539C).wrapping_add(g.u32(PLAYER_CAR).wrapping_mul(0x11));
    for i in 0..10 {
        g.set_u8(0x0300_5707 + i, g.u8(at + 7 + i));
    }
    for i in 0..7 {
        g.set_u8(
            0x0300_5700 + i,
            g.u8(g.u32(0x0300_539C).wrapping_add(g.u32(PLAYER_CAR).wrapping_mul(0x11)) + i),
        );
    }
}

fn garage_select_car(g: &mut Gba) {
    garage_copy_car_record(g);
    g.unported(GARAGE_LOAD_CAR_ATLAS, &[]);
    g.unported(GARAGE_LOAD_CAR_PALETTE, &[]);
}

/// `list_enter` (`0x0812FE38`): the page's background; car select (9) starts on the career car (or Quick Play's,
/// or car 0 without a career profile) and loads it; the crew (0x2D) on the wingman (the first open one of the
/// first, third, … in career); the main menu empties the back stack.
pub fn list_enter(g: &mut Gba) -> u32 {
    let slot = list_slot(g);
    let page = list_page(g);
    let first = g.u32(SCREEN_ENTERED) == 0 && g.u32(SCREEN) != 5;
    let (m, p) = (
        g.u16(page + 6) as i16 as i32 as u32,
        g.u16(page + 8) as i16 as i32 as u32,
    );
    menu_scene_setup_ab(g, first, m, p, 0xFFFF);
    let cursor_at = |g: &Gba| g.u32(PROFILE).wrapping_add(0x350).wrapping_add(slot as u32);
    if g.u32(SCREEN) == 9 {
        let profile = g.u32(PROFILE);
        let car = if g.u32(CAREER) == 0 {
            g.i8(profile + 0x11) as i32
        } else if g.u16(profile + 0x12) == 0 {
            0
        } else {
            g.i8(profile + 0x10) as i32
        };
        g.set_u32(PLAYER_CAR, car as u32);
        g.set_u8(cursor_at(g), car as u8);
        garage_select_car(g);
    }
    if g.u32(SCREEN) == 0x2D {
        let w = g.u32(PROFILE) + 0x200;
        if g.u32(CAREER) == 1 {
            if g.u32(w) == 0 {
                g.set_u32(w, 1);
            }
            for _ in 0..5 {
                let v = g.i32(g.u32(PROFILE) + 0x200);
                if v < 0xB && unlock_is_locked(g, v + 0x129) == 0 {
                    let w = g.u32(PROFILE) + 0x200;
                    g.set_u32(w, (g.i32(w) + 2) as u32);
                }
            }
        }
        g.set_u8(cursor_at(g), g.u32(g.u32(PROFILE) + 0x200) as u8);
    }
    if g.u32(SCREEN) == 0 {
        g.set_u8(BACK_TOP, 0xFF);
    }
    1
}

/// `list_update` (`0x081303E8`). Left/right move the cursor (toggle on 2-item pages); on car select the car
/// changes and its model turns with the shoulder buttons. A runs the item's action: the crew pages pick a wingman
/// (open ones only), 3 (career) and 0xD may first show a hint, 0x1F–0x22 open the garage's upgrade pages, other
/// screens below 0x85 (RACE TYPE sets up a free race, QUICK PLAY's random race picks mode and track), and 0x85..
/// the car purchase, the profile, the options, the hints-done step, a career race's setup and the upgrades save.
/// Then a confirmed message box: resume the race (5), a new profile (2), buy the car (9), save the upgrades (0x8B).
pub fn list_update(g: &mut Gba) -> u32 {
    let slot = list_slot(g);
    let page = list_page(g);
    let items = g.u32(page + 0x10);
    let count = g.u16(page + 10) as i16 as i32;
    let cursor_at = |g: &Gba| g.u32(PROFILE).wrapping_add(0x350).wrapping_add(slot as u32);
    for (bit, step) in [(0x20u16, -1i32), (0x10, 1)] {
        if g.u16(KEYS) & bit == 0 {
            continue;
        }
        let c = cursor_at(g);
        if count < 3 {
            g.set_u8(c, g.u8(c) ^ 1);
            g.unported(CARBON_PLAY_SOUND, &[4, 1]);
        } else {
            g.set_u8(c, (g.i8(c) as i32 + step) as u8);
            let c = cursor_at(g);
            if step < 0 && g.i8(c) < 0 {
                g.set_u8(c, (count as u8).wrapping_sub(1));
            }
            if step > 0 && count <= g.i8(c) as i32 {
                g.set_u8(c, 0);
            }
            g.unported(CARBON_PLAY_SOUND, &[4, 1]);
            if g.u32(SCREEN) == 9 {
                g.set_u32(PLAYER_CAR, g.i8(cursor_at(g)) as i32 as u32);
                garage_select_car(g);
            }
        }
        mark_screen_changed(g);
    }
    if g.u32(SCREEN) == 9 {
        let turn = |g: &mut Gba, d: u32| {
            let p = g.u32(PROFILE);
            g.set_u16(p + 0x2F6, 0x80);
            g.set_u32(p + 0x2F8, g.u32(0x0300_5F9C).wrapping_add(d));
        };
        if g.u16(0x0300_64C4) & 0x40 != 0 {
            turn(g, 0x400);
        }
        if g.u16(0x0300_64C4) & 0x80 != 0 {
            turn(g, 0xFFFF_FC00);
        }
    }
    let cursor = g.i8(cursor_at(g)) as i32;
    let item = items.wrapping_add((cursor * 8) as u32);
    let action = |g: &Gba| g.u16(item + 6) as i16 as i32;
    if g.u16(KEYS) == 1 {
        if g.u32(SCREEN) == 0x2D {
            if unlock_is_locked(g, cursor + 0x127) == 0 {
                g.unported(CARBON_PLAY_SOUND, &[2, 1]);
                pick_wingman(g, slot);
                let opponents = if g.u32(g.u32(PROFILE) + 0x200) == 0 { 3 } else { 2 };
                g.set_u32(OPPONENTS, opponents);
                if g.u32(RACE_MODE) == 1 {
                    g.set_u32(0x0300_56E4, g.u32(OPPONENTS));
                }
                let top = g.u8(g
                    .u32(PROFILE)
                    .wrapping_add(0x344)
                    .wrapping_add(g.i8(BACK_TOP) as i32 as u32));
                if top == 0xD || top == 0x26 {
                    goto_screen(g, action(g));
                } else if top.wrapping_sub(7) < 2 {
                    goto_screen(g, 0xF);
                } else {
                    menu_back(g);
                }
                career_opponents(g);
                return 1;
            }
            g.unported(CARBON_PLAY_SOUND, &[0x27, 1]);
            return 1;
        }
        if action(g) == 0x27 {
            if unlock_is_locked(g, cursor + 0x128) == 0 {
                g.unported(CARBON_PLAY_SOUND, &[2, 1]);
                pick_wingman(g, slot);
                g.set_u8(g.u32(PROFILE) + 0x1FA, 0);
                goto_screen(g, action(g));
                return 1;
            }
            g.unported(CARBON_PLAY_SOUND, &[0x27, 1]);
            return 1;
        }
        g.unported(CARBON_PLAY_SOUND, &[2, 1]);
        let hint = (action(g) == 3 && {
            g.set_u32(CAREER, 1);
            hint_due(g, 3, 0) != 0
        }) || (action(g) == 0xD && hint_due(g, 0xD, 0) != 0);
        if hint {
            goto_screen(g, 0x28);
            mark_screen_changed(g);
            return 1;
        }
        let a = action(g);
        if a == -1 {
        } else if a < 0x85 {
            let upgrade = match a {
                0x1F => Some((0x364, -1, 0x13)),
                0x20 => Some((0x365, 0, 0x14)),
                0x21 => Some((0x365, 3, 0x14)),
                0x22 => Some((0x365, 2, 0x14)),
                _ => None,
            };
            if let Some((field, add, next)) = upgrade {
                let p = g.u32(PROFILE);
                g.set_u32(p + 0x338, 0);
                g.set_u8(p + field, (g.i8(p + 0x350 + slot as u32) as i32 + add) as u8);
                goto_screen(g, next);
                return 1;
            }
            match g.u32(SCREEN) {
                6 => g.set_u8(BACK_TOP, g.u8(BACK_TOP).wrapping_sub(1)),
                1 => free_race(g, slot),
                0x1C if g.u8(cursor_at(g)) == 0 => random_quick_race(g),
                _ => {}
            }
            goto_screen(g, action(g));
        } else {
            if list_action(g, a) {
                return 1;
            }
        }
    }
    confirmed_message(g, item);
    1
}

/// A crew page's A: the wingman (profile `+0x200`) and its side (`+0x204`).
fn pick_wingman(g: &mut Gba, slot: i32) {
    let p = g.u32(PROFILE);
    let c = g.i8(p + 0x350 + slot as u32);
    g.set_u32(p + 0x200, c as i32 as u32);
    g.set_u8(p + 0x204, (c as u8).wrapping_add(1) & 1);
}

/// RACE TYPE (screen 1): a free race of the chosen mode with the Quick Play car, 2 opponents, 2 laps (1 in a
/// sprint), traffic, and random opponent looks.
fn free_race(g: &mut Gba, slot: i32) {
    g.set_u32(CAREER, 0);
    let p = g.u32(PROFILE);
    g.set_u32(RACE_MODE, g.i8(p + 0x350 + slot as u32) as i32 as u32);
    g.set_u32(PLAYER_CAR, g.i8(p + 0x10) as i32 as u32);
    let at = 0x0300_538Cu32.wrapping_add(g.u32(0x0300_0060));
    g.set_u8(at, g.u32(0x0300_53BC) as u8);
    g.set_u32(0x0300_580C, 0);
    g.set_u32(0x0300_5608, 1);
    g.set_u32(OPPONENTS, 2);
    g.set_u32(0x0300_5604, 1);
    g.set_u32(0x0300_56E4, 2);
    let r = rand_table(g);
    g.set_u32(0x0300_562C, r % 3 + 1);
    let r = rand_table(g);
    g.set_u32(0x0300_6118, r % 0x14);
    if g.u32(RACE_MODE) == 3 {
        g.set_u32(0x0300_56E4, 1);
    }
}

/// QUICK PLAY's random race (screen 0x1C, first item): a mode from `0x797D0A` (4 bytes) and a random track of it
/// (sprints `0x18 + rand % 18`; circuits `rand % 24`, reverse above 11), then `quick_race_random`.
fn random_quick_race(g: &mut Gba) {
    let modes: [u8; 4] = std::array::from_fn(|i| g.u8(0x0879_7D0A + i as u32));
    g.set_u32(CAREER, 0);
    g.set_u32(REVERSE, 0);
    let r = rand_table(g);
    let mode = modes[(r & 3) as usize] as i8;
    g.set_u32(RACE_MODE, mode as i32 as u32);
    let track = if mode == 3 {
        (rand_table(g) % 0x12) as u8 + 0x18
    } else {
        let t = (rand_table(g) % 0x18) as u8;
        if t as i8 > 0xB {
            g.set_u32(REVERSE, 1);
        }
        t
    };
    g.set_u32(
        ROUTE,
        g.u16(0x087E_4A72_u32.wrapping_add(((track as i8 as i32) * 4) as u32)) as u32,
    );
    g.unported(QUICK_RACE_RANDOM, &[]);
}

/// A List item's action from 0x85 (`list_update`'s switch); true when it ends the update (a hint is due).
fn list_action(g: &mut Gba, action: i32) -> bool {
    let slot = list_slot(g);
    let p = g.u32(PROFILE);
    match action - 0x85 {
        0 => {
            let state = g.unported(UNLOCK_STATE, &[g.u32(PLAYER_CAR).wrapping_add(0x108)]) as i32;
            if g.u32(CAREER) == 0 {
                if state == 2 {
                    message_box_open(g, 1, 0xB4, u32::MAX);
                } else {
                    g.set_u8(g.u32(PROFILE) + 0x11, g.u32(PLAYER_CAR) as u8);
                    menu_back(g);
                }
            } else if state == 1 {
                g.set_u8(g.u32(PROFILE) + 0x10, g.u32(PLAYER_CAR) as u8);
                let top = g.u8(g
                    .u32(PROFILE)
                    .wrapping_add(0x344)
                    .wrapping_add(g.i8(BACK_TOP) as i32 as u32));
                if top == 0xF {
                    menu_back(g);
                } else {
                    goto_screen(g, 4);
                }
                let save = g.u32(SAVE_BUFFER);
                g.unported(SAVE_WRITE_PROFILE, &[save]);
            } else if state == 0 {
                message_box_open(g, 2, 0x2BF, u32::MAX);
            } else if state == 2 {
                message_box_open(g, 1, 0xB4, u32::MAX);
            } else if state == 3 {
                message_box_open(g, 1, 0x172, u32::MAX);
            }
        }
        2 => {
            if g.u8(p + 0x350 + slot as u32) == 0 {
                g.set_u16(p + 0x494, 1);
                message_box_open(g, 2, 0x179, u32::MAX);
            } else {
                g.set_u16(p + 0x494, 2);
                message_box_open(g, 2, 0x121, u32::MAX);
            }
        }
        3 => {
            message_box_open(g, 2, 0x19F, u32::MAX);
        }
        4 => {
            let c = g.u8(p + 0x1F9);
            if c != 0 {
                g.set_u8(p + 0x1F8, c.wrapping_add(g.u8(p + 0x1F8)));
                g.set_u8(g.u32(PROFILE) + 0x1F9, 0);
            }
            if hint_due(g, 6, 0) != 0 {
                goto_screen(g, 0x28);
                mark_screen_changed(g);
                return true;
            }
            let q = g.u32(PROFILE);
            if g.u16(q + 0x256) != 0 {
                if g.u8(q + ZONE) < 5 {
                    g.set_u8(q + ZONE, g.u8(q + ZONE) + 1);
                }
                g.set_u16(q + 0x256, 0);
            }
            let save = g.u32(SAVE_BUFFER);
            g.unported(SAVE_WRITE_PROFILE, &[save]);
            g.set_u8(g.u32(PROFILE) + 0x344, 0);
            g.set_u32(SCREEN, if g.u32(CAREER) == 0 { 0x1C } else { 3 });
            g.set_u8(BACK_TOP, 0);
        }
        5 => {
            for i in 0..4 {
                g.set_u8(RANKED + 0xB - i, 0);
            }
            if g.u32(CAREER) == 1 {
                career_event_to_globals(g);
            }
            goto_screen(g, 10);
            mark_screen_changed(g);
        }
        6 if g.u16(p + 600) == 0 => {
            message_box_open(g, 2, 0x91, u32::MAX);
        }
        _ => {}
    }
    false
}

/// The end of `list_update`: a message box answered with A.
fn confirmed_message(g: &mut Gba, item: u32) {
    if g.i32(MESSAGE_RESULT) <= 0 {
        return;
    }
    match g.u32(SCREEN) {
        5 => {
            // Back to the paused race.
            g.set_u32(GAME_STATE, 5);
            g.set_u32(0x0300_5398, 0);
            g.set_u32(MENU_EXIT, 2);
            g.set_u32(0x0300_0048, 5);
            g.set_u8(BACK_TOP, g.u8(BACK_TOP).wrapping_sub(1));
            let c = g.u32(PROFILE) + 0x1F9;
            if g.u8(c) != 0 {
                g.set_u8(c, g.u8(c) - 1);
            }
        }
        2 => goto_screen(g, 0x16),
        9 => {
            let id = g.u32(PLAYER_CAR).wrapping_add(0x108);
            if g.unported(UNLOCK_STATE, &[id]) < 2 {
                let p = g.u32(PROFILE);
                if g.u16(p + 0x12) == 0 {
                    g.unported(BUY_UNLOCK, &[g.u32(PLAYER_CAR).wrapping_add(0x108)]);
                    let p = g.u32(PROFILE);
                    g.set_u8(p + 0x1F8, g.u8(p + 0x1F9).wrapping_add(g.u8(p + 0x1F8)));
                    g.set_u8(g.u32(PROFILE) + 0x1F9, 0);
                    let save = g.u32(SAVE_BUFFER);
                    g.unported(SAVE_WRITE_PROFILE, &[save]);
                    g.set_u8(BACK_TOP, 0xFF);
                    g.set_u32(SCREEN, 0);
                    goto_screen(g, 3);
                } else {
                    g.unported(BUY_UNLOCK, &[g.u32(PLAYER_CAR).wrapping_add(0x108)]);
                    let save = g.u32(SAVE_BUFFER);
                    g.unported(SAVE_WRITE_PROFILE, &[save]);
                }
            }
        }
        _ => {}
    }
    if g.u16(item + 6) as i16 == 0x8B {
        let at = g.u32(PROFILE) + 600;
        if g.u16(at) == 0 {
            g.set_u16(at, 1);
            let changed = g.unported(UPGRADES_CHANGED, &[(g.u32(SCREEN) != 0x1D) as u32]);
            g.set_u32(0x0300_5954, changed);
            message_box_open(g, 1, 0x376, changed);
        } else {
            g.set_u16(at, 0);
            if g.u32(0x0300_5954) != 0 {
                let save = g.u32(SAVE_BUFFER);
                g.unported(SAVE_WRITE_PROFILE, &[save]);
            }
        }
    }
    g.set_u32(MESSAGE_RESULT, 0);
}

/// `list_draw` (`0x08130D8C`): the heading; on car select the turning car, its name and stats; else three items
/// (previous, current, next) with their pictures, the crew's lock state, portrait and role, "new" marks; the arrows,
/// the player's name and cash, and the prompts (car select: buy or pick, with the price).
pub fn list_draw(g: &mut Gba, _full: u32) -> u32 {
    let slot = list_slot(g);
    let page = list_page(g);
    let count = g.u16(page + 10) as i16 as i32;
    let profile = g.u32(PROFILE);
    let second = g.u32(SECOND_PALETTE);
    let crew = (g.u32(SCREEN) == 0x2E) as i32;
    g.unported(INTRO_PAGE_SETUP, &[g.u32(0x0300_57F0)]);
    let text = |g: &mut Gba, font: u32, key: u32, x: u32, y: u32, a: u32, c: u32| {
        g.unported(TEXT_MENU, &[font, key, x, y, a, c])
    };
    let tbox = |g: &mut Gba, font: u32, key: u32, x: u32, y: u32, w: u32, c: u32| {
        g.unported(TEXT_BOX, &[font, key, x, y, w, 1, c]);
    };
    let alt = |g: &mut Gba, m: u32, x: u32, y: u32| {
        g.unported(MENU_BLIT_MATERIAL_ALT, &[WORLD, m, x, y]);
    };
    let blit = |g: &mut Gba, m: u32, x: u32, y: u32| {
        g.unported(MENU_BLIT_MATERIAL, &[WORLD, m, x, y]);
    };
    text(g, 0xC, s16(g, page), 0xEC, 2, u32::MAX, 0);
    let cur = g.i8(g.u32(PROFILE).wrapping_add(0x350).wrapping_add(slot as u32));
    let (prev, next) = if count < 3 {
        (cur.wrapping_sub(1), cur.wrapping_add(1))
    } else {
        let p = if cur == 0 { count as i8 } else { cur };
        (p.wrapping_sub(1), ((cur as i32 + 1) % count) as i8) // __modsi3
    };
    let shown = [prev, next, cur];
    let items = g.u32(page + 0x10);
    let (x2, y2) = (s16(g, 0x0879_7CD8 + 4), s16(g, 0x0879_7CE0 + 4));
    let arrow_y;
    if g.u32(SCREEN) == 9 {
        arrow_y = 0x5A;
        g.unported(GARAGE_DRAW_CAR, &[0x78, 0x3C, 0xFA]);
        let name = s16(g, items.wrapping_add((cur as i32 * 8) as u32));
        g.unported(TEXT_BOX, &[0xC, name, 0x78, 0x74, 200, 1, 8]);
        g.unported(CAR_STATS_DRAW, &[cur as i32 as u32, 0x78, 0x54, 0]);
    } else {
        arrow_y = 0x18;
        blit(g, 0xDC, x2.wrapping_sub(9), y2.wrapping_sub(10));
        for (k, &v) in shown.iter().enumerate() {
            let v = v as i32;
            if v < 0 || v >= count {
                continue;
            }
            let item = items.wrapping_add((v * 8) as u32);
            let on_crew = g.u32(SCREEN).wrapping_sub(0x2D) < 2;
            if k == 2 {
                if on_crew {
                    if unlock_is_locked(g, cur as i32 + crew + 0x127) != 0 {
                        alt(g, 0x3E, x2, y2);
                        tbox(g, 0xC, 0x15A, 0x78, 0x6E, 0xB0, 8);
                        let s = number_text(g, cur as i32);
                        let s = g.text_arg(s);
                        text(g, 0xC, s, x2.wrapping_add(0x40), y2, u32::MAX, 0);
                        continue;
                    }
                    let who = (cur as i32 + crew) as u32;
                    let pal = g.u32(0x087E_786C_u32.wrapping_add(who.wrapping_mul(4)));
                    copy_mem(g, second + 0x180, pal, 0x80, 0x10);
                    g.set_u32(PALETTE_DIRTY, 1);
                    alt(g, s16(g, item + 4), x2, y2);
                    if who != 0 {
                        let role = g.u16(0x087E_4990 + who * 4) as u32;
                        tbox(g, 0xE, role, x2.wrapping_add(0x20), y2.wrapping_add(2), 0xB0, 8);
                        let level = g.u16(0x087E_4990 + (who * 2 + 1) * 2) as u32;
                        blit(g, level + 0x10B, x2.wrapping_add(2), y2.wrapping_add(0x34));
                    }
                    tbox(g, 0xC, s16(g, item), 0x78, 0x6E, 0xB0, 8);
                } else {
                    alt(g, s16(g, item + 4), x2, y2);
                    tbox(g, 0xC, s16(g, item), 0x78, 0x6E, 0xB0, 8);
                    if g.unported(LIST_ITEM_NEW, &[g.u32(SCREEN), cur as i32 as u32]) != 0 {
                        alt(g, 0xB1, x2.wrapping_add(0x2C), y2);
                    }
                }
            } else {
                let (x, y) = (s16(g, 0x0879_7CD8 + 2 * k as u32), s16(g, 0x0879_7CE0 + 2 * k as u32));
                blit(g, 0xDD, x.wrapping_sub(1), y.wrapping_sub(1));
                if on_crew {
                    if unlock_is_locked(g, v + crew + 0x127) == 0 {
                        alt(g, s16(g, item + 2), x, y);
                    } else {
                        alt(g, 0x81, x, y);
                        let s = number_text(g, v);
                        let s = g.text_arg(s);
                        text(g, 0xC, s, x.wrapping_add(0x30), y, u32::MAX, 0);
                    }
                } else {
                    alt(g, s16(g, item + 2), x, y);
                    if g.unported(LIST_ITEM_NEW, &[g.u32(SCREEN), v as u32]) != 0 {
                        alt(g, 0xB2, x.wrapping_add(0x20), y);
                    }
                }
            }
        }
    }
    if g.i32(MESSAGE_BOX) < 0 {
        let y = s16(g, 0x0879_7CE0).wrapping_add(arrow_y);
        let m = if g.i8(g.u32(PROFILE) + 0x33C) < 1 { 0xA7 } else { 0xA8 };
        blit(g, m, 3, y);
        let m = if g.i8(g.u32(PROFILE) + 0x33D) < 1 { 0xA9 } else { 0xAA };
        blit(g, m, 0xDD, y);
    }
    let cash_text = |g: &mut Gba| {
        let cash = g.i32(profile + 0xC);
        let mut s = number_text(g, cash);
        thousands(g, &mut s, cash);
        g.text_arg(s)
    };
    if g.u16(page + 0xC) == 0 {
        text(g, 0xE, 0x197, 0xE6, 0x7C, u32::MAX, 0);
        text(g, 0xE, profile, 0xE6, 0x86, u32::MAX, 8);
        if g.u32(SCREEN) != 0x1C && g.u32(SCREEN) != 1 {
            let s = cash_text(g);
            text(g, 0xE, 0x196, 0x30, 0x7C, u32::MAX, 0);
            text(g, 0xE, s, 0x30, 0x86, u32::MAX, 8);
        }
    }
    let mut left = s16(g, page + 2);
    let mut right = s16(g, page + 4);
    let screen = g.u32(SCREEN);
    let cursor = g.i8(g.u32(PROFILE).wrapping_add(0x350).wrapping_add(slot as u32)) as i32;
    let state = if screen == 9 {
        if g.u32(CAREER) != 0 {
            if g.u16(g.u32(PROFILE) + 0x12) == 0 {
                right = u32::MAX;
            }
            if g.unported(UNLOCK_OWNED, &[(cursor + 0x108) as u32]) == 0 {
                let state = g.unported(UNLOCK_STATE, &[(cursor + 0x108) as u32]);
                left = if state == 2 { 0x15A } else { 0x19C };
                let w = text(g, 0xE, 0xE3, 0x15, 0x85, 0, 0);
                let price = g.unported(UNLOCK_PRICE, &[(cursor + 0x108) as u32]) as i32;
                let mut s = number_text(g, price);
                let price = g.unported(UNLOCK_PRICE, &[(cursor + 0x108) as u32]) as i32;
                thousands(g, &mut s, price);
                let s = g.text_arg(s);
                text(g, 0xE, s, w.wrapping_add(0x1D), 0x85, 0, 8);
            }
            let s = cash_text(g);
            let w = text(g, 0xE, s, 0xDA, 0x85, u32::MAX, 8);
            text(g, 0xE, 0x196, 0xD2u32.wrapping_sub(w), 0x85, u32::MAX, 0);
            None
        } else {
            left = 0x1F2;
            Some(g.unported(UNLOCK_STATE, &[(cursor + 0x108) as u32]))
        }
    } else if (0x2D..=0x2E).contains(&screen) {
        Some(g.unported(UNLOCK_STATE, &[(cursor + crew + 0x127) as u32]))
    } else {
        None
    };
    if state == Some(2) {
        left = 0x15A;
    }
    g.unported(MENU_BUTTON_PROMPTS, &[left, right, u32::MAX]);
    0
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

/// `enter_screen` (`0x0812B430`): runs the new screen's enter handler, then draws it in full.
pub fn enter_screen(g: &mut Gba) {
    mark_screen_changed(g);
    let screen = g.u32(SCREEN);
    let profile = g.u32(PROFILE);
    match screen {
        28 => {
            // A random car among the unlocked ones (unlock ids 0x108..=0x116) into profile + 0x11.
            let unlocked = (0..=14).filter(|i| unlock_is_locked(g, 0x108 + i) == 0).count() as u32;
            let r = rand_table(g);
            g.set_u8(profile + 0x11, r.checked_rem(unlocked).unwrap_or(0) as u8); // __umodsi3: x % 0 = 0
        }
        7 => g.set_u8(profile + 0x404, 0),
        8 => {
            g.set_u32(REVERSE, 0);
            g.set_u8(profile + 0x404, 0);
        }
        15 => career_opponents(g),
        40 => g.set_u32(FADE, -0x10i32 as u32),
        _ => {}
    }
    if let Some(k) = enter_kind(screen) {
        run_handler(g, k, 0, &[]);
    }
    if g.u32(SCREEN) != 5 {
        g.set_u32(SCREEN_ENTERED, 1);
    }
    let profile = g.u32(PROFILE);
    g.set_u16(profile + 0x2F6, 0);
    g.set_u32(profile + 0x2F8, 0);
    g.set_u32(EXIT_SCREEN, u32::MAX);
    draw_screen(g, 1);
}

/// `draw_screen` (`0x0812D334`): the screen's draw handler (`full` = 1 draws everything; forced by a screen
/// change), then the open message box. Some kinds draw a random number first, so the time spent in menus moves
/// the random sequence that later picks the opponents.
pub fn draw_screen(g: &mut Gba, full: u32) {
    let screen = g.i32(SCREEN);
    if screen > 0x7F {
        return;
    }
    let mut full = full;
    if g.u32(SCREEN_CHANGED) != 0 {
        full = 1;
        g.set_u32(SCREEN_CHANGED, 0);
    }
    if let Some(k) = draw_kind(screen as u32) {
        if matches!(k, Kind::List | Kind::Setup | Kind::Intro | Kind::Kind38) {
            rand_table(g);
        }
        run_handler(g, k, 2, &[full]);
    }
    if g.i32(MESSAGE_BOX) >= 0 {
        g.unported(0x0813_550C, &[full]);
    }
}

/// `menu_back` (`0x0812D49C`): pops the back stack (profile + 0x344) into the screen and enters it.
pub fn menu_back(g: &mut Gba) {
    let top = g.i8(BACK_TOP);
    if top >= 0 {
        g.set_u8(BACK_TOP, (top as u8).wrapping_sub(1));
        let screen = g.u8(g.u32(PROFILE).wrapping_add(0x344).wrapping_add(top as i32 as u32));
        g.set_u32(SCREEN, screen as u32);
        enter_screen(g);
    }
}

/// `message_box_input` (`0x08135690`): 1 on A, −1 on B when the box (`MESSAGE_BOX` 2) takes B, else 0.
pub fn message_box_input(g: &mut Gba) -> i32 {
    g.set_u32(MESSAGE_RESULT, 0);
    if g.u16(KEYS) == 1 {
        g.set_u32(MESSAGE_RESULT, 1);
        g.unported(CARBON_PLAY_SOUND, &[2, 1]);
    }
    if g.i32(MESSAGE_BOX) == 2 && g.u16(KEYS) == 2 {
        g.set_u32(MESSAGE_RESULT, u32::MAX);
        g.unported(CARBON_PLAY_SOUND, &[3, 1]);
    }
    g.i32(MESSAGE_RESULT)
}

/// `message_box_close` (`0x08135678`): closes the box and swallows the keys.
pub fn message_box_close(g: &mut Gba) {
    g.set_u32(MESSAGE_BOX, u32::MAX);
    g.set_u16(KEYS, 0);
}

pub const CARBON_PLAY_SOUND: u32 = 0x0813_5FDC;

/// `menu_frame` (`0x0812B5F0`, called `race_setup_route` in earlier notes): one frame of the menus. Returns 0 when
/// the menus hand over to the race (after the exit handler of `EXIT_SCREEN`), else 1 or the update handler's
/// result.
pub fn menu_frame(g: &mut Gba) -> u32 {
    let exit = g.u32(MENU_EXIT);
    if exit != 0 {
        if g.u32(FADE) != 0 || exit != 7 {
            return 1;
        }
        let sub = g.i32(EXIT_SCREEN);
        if sub < 0 {
            return 0;
        }
        if let Some(k) = exit_kind(sub as u32) {
            run_handler(g, k, 3, &[]);
        }
        g.set_u32(SCREEN_ENTERED, 0);
        return 0;
    }
    let screen = g.i32(SCREEN);
    if screen > 0x7F {
        leave_for_race(g, screen);
    }
    if g.i32(MESSAGE_BOX) >= 0 {
        if message_box_input(g) != 0 {
            message_box_close(g);
        }
        tick_repeats(g);
        return 1;
    }
    let profile = g.u32(PROFILE);
    let keys = g.u16(KEYS);
    // Key-repeat delays: 3 frames on each newly pressed key (profile + 0x33C..0x343).
    for (bit, off) in [
        (0x20, 0x33C),
        (0x10, 0x33D),
        (0x40, 0x33E),
        (0x80, 0x33F),
        (1, 0x340),
        (2, 0x341),
        (0x200, 0x342),
    ]
    .into_iter()
    .chain([(0x100, 0x343)])
    {
        if keys & bit != 0 {
            g.set_u8(profile + off, 3);
        }
    }
    let mut result = 1;
    if let Some(k) = update_kind(g.u32(SCREEN)) {
        result = run_handler(g, k, 1, &[]);
    }
    // B goes back, except on these screens (read again: the update may have changed it).
    let screen = g.u32(SCREEN);
    let blocked = screen.wrapping_sub(0xB) <= 1
        || matches!(
            screen,
            6 | 5 | 0x17 | 0x2F | 0x30 | 0x18 | 0x19 | 0x16 | 0x26 | 0x27 | 0x2A
        )
        || (screen == 9 && g.u16(g.u32(PROFILE) + 0x12) == 0 && g.u32(CAREER) != 0)
        || g.u32(SCREEN) == 0x1A;
    if !blocked && g.u16(KEYS) == 2 {
        if g.i8(BACK_TOP) >= 0 {
            g.unported(CARBON_PLAY_SOUND, &[3, 1]);
        }
        let flag = g.u32(PROFILE) + 0x404;
        if g.u8(flag) == 3 && g.u32(SCREEN) == 0x11 {
            g.set_u8(flag, 2);
            map_select(g, 0);
        } else {
            menu_back(g);
        }
        mark_screen_changed(g);
    }
    tick_repeats(g);
    result
}

/// The end of `menu_frame`: each key-repeat delay above 0 counts down.
fn tick_repeats(g: &mut Gba) {
    for i in 0..8 {
        let a = g.u32(PROFILE) + 0x33C + i;
        let d = g.i8(a);
        if d > 0 {
            g.set_u8(a, (d - 1) as u8);
        }
    }
}

/// The part of `menu_frame` for screens above 0x7F: the race is chosen. 0x81 (Quick Play) also takes the track
/// slot of `ROUTE` (`0x7E49C4`), fixes its direction, sets the player's car, the route number (`0x7E4A70`), clears
/// the race slots and picks the environment and route index (`0x7F2588`). Then the menus are left (`MENU_EXIT` 7)
/// with a fade-out.
fn leave_for_race(g: &mut Gba, screen: i32) {
    let route = g.u32(ROUTE);
    let mut slot = g.u32(0x087E_49C4 + 4 * route) as i32;
    if screen == 0x81 {
        if g.u32(REVERSE) != 0 {
            if slot <= 0xB {
                slot += 0xC;
            }
            if slot > 0x17 {
                g.set_u32(REVERSE, 0); // `MENU_EXIT`, which is 0 on this path
            }
        } else if ((slot - 0xC) as u32) <= 0xB {
            slot -= 0xC;
        }
        let profile = g.u32(PROFILE);
        let car = g.i8(profile + if g.u32(CAREER) != 0 { 0x10 } else { 0x11 }) as i32;
        g.set_u32(PLAYER_CAR, car as u32);
        let number = g.u16(0x087E_4A70 + 2 + 4 * slot as u32);
        g.set_u32(ROUTE, number as u32);
        g.set_u8(0x0300_611C, car as u8);
        g.set_u8(BACK_TOP_SAVED, g.u8(BACK_TOP));
        for i in 0..4 {
            g.set_u8(0x0300_5658 + i, 0);
            g.set_u32(0x0300_5660 + 4 * i, 0);
            g.set_u32(0x0300_5670 + 4 * i, 0);
            g.set_u32(0x0300_5680 + 4 * i, 0);
        }
        let rec = 0x087F_2588 + 12 * g.u32(ROUTE);
        g.set_u32(0x0300_006C, g.u8(rec) as u32);
        g.set_u32(0x0300_5720, g.u8(rec + 1) as u32);
    }
    g.set_u32(MENU_EXIT, 7);
    g.set_u32(FADE, -0x10i32 as u32);
}

/// `game_state_step` (`0x0812ACEC`): boot → menus → race start → race → menus.
pub fn game_state_step(g: &mut Gba) {
    match g.u32(GAME_STATE) {
        0 => {
            g.set_u32(MENU_EXIT, 0);
            if g.u32(FADE) != 0 {
                return;
            }
            enter_screen(g);
            g.set_u32(GAME_STATE, 1);
            let t = g.unported(0x0816_0E74, &[]);
            g.unported(0x0815_E9E8, &[0x0879_7C54, t]);
        }
        1 => match menu_frame(g) {
            0 => g.set_u32(GAME_STATE, 4),
            1 if g.u32(FADE) == 0 => draw_screen(g, 0),
            _ => {}
        },
        4 => {
            g.set_u32(MENU_EXIT, 0);
            g.set_u32(GAME_STATE, 5);
            g.unported(0x0813_9E34, &[WORLD]); // race_start_from_table_a
            g.set_u32(FADE, 0x10);
            g.set_u32(0x0300_57E0, 0x10);
            g.set_u32(FRAME_COUNTER, 0);
            g.set_u32(0x0300_56F0, 0);
            let t = g.unported(0x0816_0E74, &[]);
            g.unported(0x0815_E9E8, &[0x0879_7C5C, t]);
            if g.u32(ROUTE) != 0 {
                let (a, b) = (g.u32(0x0300_55F0), g.u32(SECOND_PALETTE));
                copy_mem(g, a, b, 0x200, 0x20);
                g.unported(0x0813_A514, &[WORLD]); // apply_sector_light_to_palette
            }
            game_state_step(g);
        }
        5 => {
            if g.unported(0x0813_A954, &[WORLD]) != 0 {
                return; // race_frame_update: still racing
            }
            g.unported(0x0813_5F38, &[]);
            let player = g.u32(0x0300_0060);
            g.set_u32(g.u32(PROFILE) + 0x32C, player);
            if g.u32(0x0300_0048) != 5 {
                g.unported(0x0812_EAAC, &[WORLD]);
            }
            g.unported(0x0813_96C4, &[WORLD]);
            g.set_u8(BACK_TOP, g.u8(BACK_TOP_SAVED));
            if g.u32(0x0300_0048) == 5 {
                menu_back(g);
            } else {
                goto_screen(g, 0xB);
            }
            g.set_u32(GAME_STATE, 1);
            g.unported(0x0813_6054, &[0]); // carbon_play_music
        }
        _ => {}
    }
}

/// `main_frame` (`0x0812AE64`). Timer 3's count for the last frame (`FUN_0816223C(3)`, hardware) sets the frame
/// time `0x03005640` = 25,500 / ticks within 10..=100 (a count of 0 reads as 0x200), or 15 when `0x03005624` is 2.
/// Then the game state steps, the frame counter counts, the race palette is tinted, and the palette fade moves 4
/// per channel: the sky gradient buffer (towards the level's gradient), BG palette RAM (towards the second base
/// palette) and OBJ palette RAM (towards world `+0x34`), by 2 on the counter.
pub fn main_frame(g: &mut Gba) {
    g.unported(0x0816_2228, &[3]);
    let ticks = g.unported(0x0816_223C, &[3]);
    g.set_u32(0x0300_5934, ticks);
    if g.u32(0x0300_5624) == 2 {
        g.set_u32(0x0300_5640, 0xF);
    } else {
        if ticks == 0 {
            g.set_u32(0x0300_5934, 0x200);
        }
        let t = nfsgba_fixed::div(0x639C, g.i32(0x0300_5934)).clamp(10, 100);
        g.set_u32(0x0300_5640, t as u32);
    }
    g.unported(0x0816_21F0, &[3, 3, 0, 0]);
    g.unported(0x0812_B084, &[]);
    game_state_step(g);
    g.unported(0x0816_1F38, &[0x0300_0058]);
    g.unported(0x0816_102C, &[]);
    g.set_u32(FRAME_COUNTER, g.u32(FRAME_COUNTER).wrapping_add(1));
    if g.u32(GAME_STATE) == 5 {
        g.unported(0x0813_A514, &[WORLD]);
    }
    let fade = g.i32(FADE);
    if fade != 0 {
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
    } else if g.u32(GAME_STATE) != 5 && g.u32(PALETTE_DIRTY) != 0 {
        let second = g.u32(SECOND_PALETTE);
        g.unported(0x0815_DFD8, &[second]); // copy_palette_to_ram
        g.set_u32(PALETTE_DIRTY, 0);
    }
    g.unported(0x0812_B040, &[]);
    g.unported(0x0814_2090, &[]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use nfsgba_testkit::{dump, rom};

    /// Oracle cases saved by `tools/ui_menu_oracle.py <name>`.
    fn cases(name: &str) -> Option<Vec<serde_json::Value>> {
        let text = nfsgba_testkit::read_to_string(&format!("menus2/{name}.jsonl"))?;
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
                            match g.texts.get(i) {
                                Some(t) if *t == bytes(&v[1]) => STACK_TEXT + i as u32,
                                _ => u32::MAX,
                            }
                        }
                    });
                    (k[0].as_u64().unwrap() as u32, args.collect::<Vec<_>>())
                })
                .collect();
            let got = changed(&pre, &g);
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
