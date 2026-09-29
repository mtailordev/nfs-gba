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
}

impl Gba {
    /// An mGBA dump (`<prefix>.{wram,iwram,io,palette,vram,oam}.bin`, `tools/mgba_ctl.py dump`).
    pub fn from_dump(rom: Vec<u8>, prefix: &std::path::Path) -> std::io::Result<Gba> {
        let read = |d: &str| std::fs::read(format!("{}.{d}.bin", prefix.display()));
        Ok(Gba {
            rom,
            ewram: read("wram")?,
            iwram: read("iwram")?,
            io: read("io")?,
            pal: read("palette")?,
            vram: read("vram")?,
            oam: read("oam")?,
            calls: Vec::new(),
            returns: Default::default(),
        })
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

/// `rand_table` (`0x0815FCFC`): the next of the 256 numbers at `0x7C03F0`.
pub fn rand_table(g: &mut Gba) -> u32 {
    let i = (g.u32(RAND_INDEX) + 1) & 0xFF;
    g.set_u32(RAND_INDEX, i);
    g.u16(0x087C_03F0 + 2 * i) as u32
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
        15 => {
            g.unported(0x0812_B320, &[]);
        }
        40 => g.set_u32(FADE, -0x10i32 as u32),
        _ => {}
    }
    if let Some(k) = enter_kind(screen) {
        g.unported(k.handlers()[0], &[]);
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
        g.unported(k.handlers()[2], &[full]);
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
            g.unported(k.handlers()[3], &[]);
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
        result = g.unported(k.handlers()[1], &[]);
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
            g.unported(0x0814_39C0, &[0]);
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
                g.unported(0x0816_0D18, &[a, b, 0x200, 0x20]);
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
                g.unported(0x0812_BB5C, &[0xB]);
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
        let t = crate::div(0x639C, g.i32(0x0300_5934)).clamp(10, 100);
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
    use crate::data_dir;

    /// Oracle cases saved by `tools/ui_menu_oracle.py <name>`.
    fn cases(name: &str) -> Option<Vec<serde_json::Value>> {
        let path = data_dir().join(format!("work/e5298b24/menus/{name}.jsonl"));
        let text = std::fs::read_to_string(&path)
            .map_err(|e| eprintln!("skipping: no oracle cases {} ({e})", path.display()))
            .ok()?;
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
        let Some(rom) = crate::paint::tests::rom() else { return };
        let Some(cases) = cases("toplevel") else { return };
        let mut snaps = std::collections::HashMap::new();
        for (n, c) in cases.iter().enumerate() {
            let snap = c["snap"].as_str().unwrap();
            let base = snaps
                .entry(snap.to_owned())
                .or_insert_with(|| Gba::from_dump(rom.clone(), &data_dir().join("work/e5298b24").join(snap)).unwrap());
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
                _ => panic!("{f}"),
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
            let calls: Vec<(u32, Vec<u32>)> = c["calls"]
                .as_array()
                .unwrap()
                .iter()
                .map(|k| {
                    let args = k[1].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u32);
                    (k[0].as_u64().unwrap() as u32, args.collect())
                })
                .collect();
            let got = changed(&pre, &g);
            let diff: Vec<_> = got.iter().filter(|(a, b)| want.get(a) != Some(b)).take(8).collect();
            let missing: Vec<_> = want.iter().filter(|(a, b)| got.get(a) != Some(b)).take(8).collect();
            assert!(
                diff.is_empty() && missing.is_empty(),
                "case {n} {f} {snap}: extra {diff:x?} missing {missing:x?}"
            );
            assert_eq!(g.calls, calls, "case {n} {f} {snap}: calls");
            if let Some(r0) = r0 {
                assert_eq!(r0 as u64, c["r0"].as_u64().unwrap(), "case {n} {f} {snap}: result");
            }
        }
        eprintln!("top level: {} oracle cases match", cases.len());
    }

    /// The four screen tables are the ROM's jump tables: each entry's stub calls (Thumb `bl`) its kind's handler
    /// before its first unconditional branch, or no handler.
    #[test]
    fn screen_tables_match_the_rom_jump_tables() {
        let Some(rom) = crate::paint::tests::rom() else { return };
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
        std::env::set_current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")).unwrap();
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
