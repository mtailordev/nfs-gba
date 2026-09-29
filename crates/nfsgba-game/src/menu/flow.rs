//! The menus' top level on typed state ([`MenuState`]): `main_frame`'s order, `game_state_step`, the screen
//! dispatcher (`enter_screen`, `draw_screen`, `goto_screen`, `menu_back`), the message box and the key repeats.
//! Nothing here reads a GBA address: the screens' handlers, the drawing and the game functions not ported yet are
//! reached through [`Host`]. The oracle-case tests run it through the `Gba` adapters in `super`.

use nfsgba_sim::state::MenuState;

use super::{
    Kind, VBLANK_INTR_WAIT, draw_kind, enter_kind, event, exit_kind, hints, intro, list, map, results, setup,
    update_kind,
};

pub const CARBON_PLAY_SOUND: u32 = 0x0813_5FDC;
const CARBON_PLAY_MUSIC: u32 = 0x0813_6054;
const APPLY_SECTOR_LIGHT: u32 = 0x0813_A514;
const MESSAGE_BOX_DRAW: u32 = 0x0813_550C;

/// What the typed flow needs from outside it: the ROM, calls to game functions not ported yet (logged, answered by
/// the caller), the screens' handlers (still on the RAM image), and hardware (palette RAM).
pub trait Host {
    fn rom(&self) -> &[u8];
    /// A game function that is not ported: its address and arguments; returns its result.
    fn call(&mut self, function: u32, args: &[u32]) -> u32;
    /// A string the game builds on its stack and passes to a text primitive by pointer: the pointer it gets.
    fn text_arg(&mut self, s: Vec<u8>) -> u32;
    /// A kind's handler (`phase`: 0 enter, 1 update, 2 draw, 3 exit). It reads and writes `st`.
    fn handler(&mut self, st: &mut MenuState, kind: Kind, phase: usize, args: &[u32]) -> u32;
    /// `menu_scene_setup` (material, palette, sprite screen): the screen's background, palettes and sprite screen.
    fn scene_setup(&mut self, st: &mut MenuState, material: u32, palette: u32, sprite: u32);
    /// `menu_scene_setup_a` (`first`) or `_b`, chosen by the caller.
    fn scene_setup_ab(&mut self, st: &mut MenuState, first: bool, material: u32, palette: u32, sprite: u32);
    /// The world's current base palette pointer (`WORLD + 0x30`).
    fn world_palette(&self) -> u32;
    /// The drawing page's pointer (`**(WORLD + 0x50)`).
    fn page_buffer(&self) -> u32;
    /// Fills both frame buffers with colour 1 (`fill32` over the screen size).
    fn clear_frame_buffers(&mut self);
    /// Fills the drawing page with colour 1.
    fn fill_page(&mut self);
    /// A halfword of the game's memory (the credits pointer: ROM, or stale RAM before the screen is entered).
    fn peek16(&self, addr: u32) -> u16;
    /// Colour `i` of the second base palette buffer.
    fn second_colour(&self, i: u32) -> u16;
    fn set_second_colour(&mut self, i: u32, c: u16);
    /// Black BG palette RAM (`fill_bg_palette(0, 0, 0x100)`).
    fn black_bg_palette(&mut self);
    /// `copy_mem(dst, src, n, width)`.
    fn copy_mem(&mut self, dst: u32, src: u32, n: u32, width: u32);
    /// `main_frame`'s palette fade while `st.g.fade != 0`: the gradient buffer, BG and OBJ palette RAM one step
    /// towards their targets; moves `fade` by 2.
    fn fade_step(&mut self, st: &mut MenuState);
}

/// `list_slot` (`0x0812FD04`): the List screen's cursor slot, −1 for other screens.
pub fn list_slot(screen: u32, career_flag: u16) -> i32 {
    match screen {
        s @ 0..=2 => s as i32,
        3 if career_flag != 0 => 3,
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

/// `rand_table` (`0x0815FCFC`): the next of the 256 numbers at `0x7C03F0`.
pub fn rand_table(st: &mut MenuState, h: &impl Host) -> u32 {
    nfsgba_fixed::rand_table(h.rom(), &mut st.g.rand_index)
}

/// `enter_screen` (`0x0812B430`): runs the new screen's enter handler, then draws it in full.
pub fn enter_screen(st: &mut MenuState, h: &mut impl Host) {
    st.g.screen_changed = 1;
    let screen = st.g.screen;
    match screen {
        28 => {
            // A random car among the unlocked ones (unlock ids 0x108..=0x116).
            let unlocked = (0..=14).filter(|i| st.profile.is_locked(0x108 + i) == 0).count() as u32;
            let r = rand_table(st, h);
            st.profile.car = r.checked_rem(unlocked).unwrap_or(0) as i8; // __umodsi3: x % 0 = 0
        }
        7 => st.profile.map_mode = 0,
        8 => {
            st.g.reverse = 0;
            st.profile.map_mode = 0;
        }
        15 => setup::career_opponents(st, h),
        40 => st.g.fade = -0x10,
        _ => {}
    }
    if let Some(k) = enter_kind(screen) {
        h.handler(st, k, 0, &[]);
    }
    if st.g.screen != 5 {
        st.g.screen_entered = 1;
    }
    st.profile.u_2f6 = 0;
    st.profile.u_2f8 = 0;
    st.g.exit_screen = u32::MAX;
    draw_screen(st, h, 1);
}

/// `draw_screen` (`0x0812D334`): the screen's draw handler (`full` = 1 draws everything; forced by a screen
/// change), then the open message box. Some kinds draw a random number first, so the time spent in menus moves
/// the random sequence that later picks the opponents.
pub fn draw_screen(st: &mut MenuState, h: &mut impl Host, full: u32) {
    if st.g.screen as i32 > 0x7F {
        return;
    }
    let mut full = full;
    if st.g.screen_changed != 0 {
        full = 1;
        st.g.screen_changed = 0;
    }
    if let Some(k) = draw_kind(st.g.screen) {
        if matches!(k, Kind::List | Kind::Setup | Kind::Intro | Kind::Kind38) {
            rand_table(st, h);
        }
        h.handler(st, k, 2, &[full]);
    }
    if st.g.message_box >= 0 {
        h.call(MESSAGE_BOX_DRAW, &[full]);
    }
}

/// Back stack slot `top` (profile `+0x344 + top`) written. Slot −1 is the last key-repeat byte (`+0x343`); past
/// the 12 slots the game writes into the List cursors.
pub fn poke_back(st: &mut MenuState, top: i8, v: u8) {
    let i = top as i32;
    if i == -1 {
        st.profile.repeats[7] = v as i8;
    } else if let Ok(i) = usize::try_from(i) {
        match i.checked_sub(st.profile.back.len()) {
            None => st.profile.back[i] = v,
            Some(j) => {
                if let Some(c) = st.profile.cursors.get_mut(j) {
                    *c = v;
                }
            }
        }
    }
}

/// The back stack byte at `top` as the game indexes it (an `i8`): -1 is the last key-repeat byte.
pub fn peek_back_i(st: &MenuState, top: i8) -> u8 {
    match top {
        -1 => st.profile.repeats[7] as u8,
        t if t >= 0 => peek_back(st, t as usize),
        _ => 0, // ponytail: further below the stack the game reads other profile bytes; the stack never goes there
    }
}

/// Back stack slot `i` (profile `+0x344 + i`) read; past the 12 slots it reads the List cursors.
pub fn peek_back(st: &MenuState, i: usize) -> u8 {
    match i.checked_sub(st.profile.back.len()) {
        None => st.profile.back[i],
        Some(j) => st.profile.cursors.get(j).copied().unwrap_or(0),
    }
}

/// `goto_screen` (`0x0812BB5C`). Screens up to 0x7F are pushed: the old screen onto the back stack, and List
/// screens (but 9 and 28) clear their cursor slot, then `enter_screen`. Above: the keys are swallowed; 0x81 (Quick
/// Play) records the exit screen; 0x82 resumes a paused race.
pub fn goto_screen(st: &mut MenuState, h: &mut impl Host, s: i32) {
    if s <= 0x7F {
        let top = (st.g.back_top as u8).wrapping_add(1);
        st.g.back_top = top as i8;
        poke_back(st, top as i8, st.g.screen as u8);
        st.g.screen = s as u32;
        if matches!(s, 0..=6 | 27 | 29 | 30 | 35 | 36 | 45 | 46) {
            let slot = list_slot(st.g.screen, st.profile.u_12);
            if let Some(c) = st.profile.cursors.get_mut(slot as usize) {
                *c = 0;
            }
        }
        enter_screen(st, h);
        return;
    }
    st.g.keys = 0;
    if s == 0x82 {
        // Back to the race from the pause menu: 15 frames, then the race palettes and music.
        for _ in 0..15 {
            h.call(VBLANK_INTR_WAIT, &[]);
        }
        h.black_bg_palette();
        st.g.game_state = 5;
        st.g.u_5398 = 0;
        st.g.fade = 0x10;
        st.g.back_top = st.g.back_top.wrapping_sub(1);
        h.call(0x0813_72E4, &[super::WORLD]); // race_menu_palette_setup
        if st.g.hud_on != 0 {
            h.call(0x0814_3010, &[1]); // hud_toggle
        }
        h.call(0x0813_9E10, &[super::WORLD]);
        h.call(CARBON_PLAY_MUSIC, &[(st.profile.music as i32 + 1) as u32]);
        if st.g.route != 0 {
            h.copy_mem(st.g.race_palette, st.g.second_palette, 0x200, 0x20);
            h.call(APPLY_SECTOR_LIGHT, &[super::WORLD]);
        }
        return;
    }
    if s == 0x81 {
        st.g.exit_screen = st.g.screen;
    }
    st.g.screen = s as u32;
}

/// `menu_back` (`0x0812D49C`): pops the back stack into the screen and enters it.
pub fn menu_back(st: &mut MenuState, h: &mut impl Host) {
    let top = st.g.back_top;
    if top >= 0 {
        st.g.back_top = top.wrapping_sub(1);
        st.g.screen = peek_back(st, top as usize) as u32;
        enter_screen(st, h);
    }
}

/// `message_box_input` (`0x08135690`): 1 on A, −1 on B when the box (`message_box` 2) takes B, else 0.
pub fn message_box_input(st: &mut MenuState, h: &mut impl Host) -> i32 {
    st.g.message_result = 0;
    if st.g.keys == 1 {
        st.g.message_result = 1;
        h.call(CARBON_PLAY_SOUND, &[2, 1]);
    }
    if st.g.message_box == 2 && st.g.keys == 2 {
        st.g.message_result = -1;
        h.call(CARBON_PLAY_SOUND, &[3, 1]);
    }
    st.g.message_result
}

/// `FUN_08135640` (type, text, arg): opens a message box when none is open (and swallows the keys); 1 if opened.
pub fn message_box_open(st: &mut MenuState, kind: u32, text: u32, arg: u32) -> u32 {
    let open = st.g.message_box < 0;
    if open {
        st.g.message_box = kind as i32;
        st.g.message_text = text;
        st.g.message_arg = arg;
        st.g.keys = 0;
    }
    open as u32
}

/// `message_box_close` (`0x08135678`): closes the box and swallows the keys.
pub fn message_box_close(st: &mut MenuState) {
    st.g.message_box = -1;
    st.g.keys = 0;
}

/// `menu_frame` (`0x0812B5F0`): one frame of the menus. Returns 0 when the menus hand over to the race (after the
/// exit handler of `exit_screen`), else 1 or the update handler's result.
pub fn menu_frame(st: &mut MenuState, h: &mut impl Host) -> u32 {
    let exit = st.g.menu_exit;
    if exit != 0 {
        if st.g.fade != 0 || exit != 7 {
            return 1;
        }
        let sub = st.g.exit_screen as i32;
        if sub < 0 {
            return 0;
        }
        if let Some(k) = exit_kind(sub as u32) {
            h.handler(st, k, 3, &[]);
        }
        st.g.screen_entered = 0;
        return 0;
    }
    if st.g.screen as i32 > 0x7F {
        leave_for_race(st, h.rom());
    }
    if st.g.message_box >= 0 {
        if message_box_input(st, h) != 0 {
            message_box_close(st);
        }
        tick_repeats(st);
        return 1;
    }
    // Key-repeat delays: 3 frames on each newly pressed key.
    for (bit, i) in [
        (0x20, 0),
        (0x10, 1),
        (0x40, 2),
        (0x80, 3),
        (1, 4),
        (2, 5),
        (0x200, 6),
        (0x100, 7),
    ] {
        if st.g.keys & bit != 0 {
            st.profile.repeats[i] = 3;
        }
    }
    let mut result = 1;
    if let Some(k) = update_kind(st.g.screen) {
        result = h.handler(st, k, 1, &[]);
    }
    // B goes back, except on these screens (read again: the update may have changed it).
    let screen = st.g.screen;
    let blocked = screen.wrapping_sub(0xB) <= 1
        || matches!(
            screen,
            6 | 5 | 0x17 | 0x2F | 0x30 | 0x18 | 0x19 | 0x16 | 0x26 | 0x27 | 0x2A | 0x1A
        )
        || (screen == 9 && st.profile.u_12 == 0 && st.g.career != 0);
    if !blocked && st.g.keys == 2 {
        if st.g.back_top >= 0 {
            h.call(CARBON_PLAY_SOUND, &[3, 1]);
        }
        if st.profile.map_mode == 3 && st.g.screen == 0x11 {
            st.profile.map_mode = 2;
            map::select(st, 0);
        } else {
            menu_back(st, h);
        }
        st.g.screen_changed = 1;
    }
    tick_repeats(st);
    result
}

/// The end of `menu_frame`: each key-repeat delay above 0 counts down.
fn tick_repeats(st: &mut MenuState) {
    for d in st.profile.repeats.iter_mut().filter(|d| **d > 0) {
        *d -= 1;
    }
}

pub fn rom_u16(rom: &[u8], addr: u32) -> u16 {
    let o = (addr & 0x1FF_FFFF) as usize;
    u16::from_le_bytes([rom[o], rom[o + 1]])
}

pub fn rom_u32(rom: &[u8], addr: u32) -> u32 {
    let o = (addr & 0x1FF_FFFF) as usize;
    u32::from_le_bytes(rom[o..o + 4].try_into().unwrap())
}

/// The part of `menu_frame` for screens above 0x7F: the race is chosen. 0x81 (Quick Play) also takes the track
/// slot of `route` (`0x7E49C4`), fixes its direction, sets the player's car, the route number (`0x7E4A70`), clears
/// the race slots and picks the environment and route index (`0x7F2588`). Then the menus are left (`menu_exit` 7)
/// with a fade-out.
fn leave_for_race(st: &mut MenuState, rom: &[u8]) {
    let g = &mut st.g;
    let mut slot = rom_u32(rom, 0x087E_49C4 + 4 * g.route) as i32;
    if g.screen == 0x81 {
        if g.reverse != 0 {
            if slot <= 0xB {
                slot += 0xC;
            }
            if slot > 0x17 {
                g.reverse = 0;
            }
        } else if ((slot - 0xC) as u32) <= 0xB {
            slot -= 0xC;
        }
        let car = (if g.career != 0 {
            st.profile.career_car
        } else {
            st.profile.car
        }) as i32;
        g.player_car = car as u32;
        let a = 0x087E_4A70 + 2 + 4 * slot as u32;
        let number = u16::from_le_bytes([rom[(a & 0x1FF_FFFF) as usize], rom[(a & 0x1FF_FFFF) as usize + 1]]);
        g.route = number as u32;
        g.race_car = car as u8;
        g.back_top_saved = g.back_top as u8;
        g.results.knocked = [0; 4];
        g.results.best_lap = [0; 4];
        g.results.finish = [0; 4];
        g.results.life = [0; 4];
        let rec = (0x087F_2588 + 12 * g.route) & 0x1FF_FFFF;
        g.environment = rom[rec as usize] as u32;
        g.route_flag = rom[rec as usize + 1] as u32;
    }
    g.menu_exit = 7;
    g.fade = -0x10;
}

/// `game_state_step` (`0x0812ACEC`): boot → menus → race start → race → menus.
pub fn game_state_step(st: &mut MenuState, h: &mut impl Host) {
    match st.g.game_state {
        0 => {
            st.g.menu_exit = 0;
            if st.g.fade != 0 {
                return;
            }
            enter_screen(st, h);
            st.g.game_state = 1;
            let t = h.call(0x0816_0E74, &[]);
            h.call(0x0815_E9E8, &[0x0879_7C54, t]);
        }
        1 => match menu_frame(st, h) {
            0 => st.g.game_state = 4,
            1 if st.g.fade == 0 => draw_screen(st, h, 0),
            _ => {}
        },
        4 => {
            st.g.menu_exit = 0;
            st.g.game_state = 5;
            h.call(0x0813_9E34, &[super::WORLD]); // race_start_from_table_a
            st.g.fade = 0x10;
            st.g.u_57e0 = 0x10;
            st.g.frame_counter = 0;
            st.g.u_56f0 = 0;
            let t = h.call(0x0816_0E74, &[]);
            h.call(0x0815_E9E8, &[0x0879_7C5C, t]);
            if st.g.route != 0 {
                h.copy_mem(st.g.race_palette, st.g.second_palette, 0x200, 0x20);
                h.call(APPLY_SECTOR_LIGHT, &[super::WORLD]); // apply_sector_light_to_palette
            }
            game_state_step(st, h);
        }
        5 => {
            if h.call(0x0813_A954, &[super::WORLD]) != 0 {
                return; // race_frame_update: still racing
            }
            h.call(0x0813_5F38, &[]);
            st.profile.last_player = st.g.race_player;
            if st.g.race_outcome != 5 {
                h.call(0x0812_EAAC, &[super::WORLD]);
            }
            h.call(0x0813_96C4, &[super::WORLD]);
            st.g.back_top = st.g.back_top_saved as i8;
            if st.g.race_outcome == 5 {
                menu_back(st, h);
            } else {
                goto_screen(st, h, 0xB);
            }
            st.g.game_state = 1;
            h.call(CARBON_PLAY_MUSIC, &[0]);
        }
        _ => {}
    }
}

/// `main_frame` (`0x0812AE64`). Timer 3's count for the last frame sets the frame time = 25,500 / ticks within
/// 10..=100 (a count of 0 reads as 0x200), or 15 when `timing_mode` is 2. Then the game state steps, the frame
/// counter counts, the race palette is tinted, and the palette fade moves ([`Host::fade_step`]).
pub fn main_frame(st: &mut MenuState, h: &mut impl Host) {
    h.call(0x0816_2228, &[3]);
    let ticks = h.call(0x0816_223C, &[3]);
    st.g.timer_ticks = ticks;
    if st.g.timing_mode == 2 {
        st.g.frame_time = 0xF;
    } else {
        if ticks == 0 {
            st.g.timer_ticks = 0x200;
        }
        st.g.frame_time = nfsgba_fixed::div(0x639C, st.g.timer_ticks as i32).clamp(10, 100) as u32;
    }
    h.call(0x0816_21F0, &[3, 3, 0, 0]);
    h.call(0x0812_B084, &[]);
    game_state_step(st, h);
    h.call(0x0816_1F38, &[0x0300_0058]);
    h.call(0x0816_102C, &[]);
    st.g.frame_counter = st.g.frame_counter.wrapping_add(1);
    if st.g.game_state == 5 {
        h.call(APPLY_SECTOR_LIGHT, &[super::WORLD]);
    }
    if st.g.fade != 0 {
        h.fade_step(st);
    } else if st.g.game_state != 5 && st.g.palette_dirty != 0 {
        h.call(0x0815_DFD8, &[st.g.second_palette]); // copy_palette_to_ram
        st.g.palette_dirty = 0;
    }
    h.call(0x0812_B040, &[]);
    h.call(0x0814_2090, &[]);
}

/// Whether a kind's handler runs on typed state ([`run_typed`]); the others still run on the RAM image.
pub fn is_typed(kind: Kind, phase: usize) -> bool {
    matches!(
        (kind, phase),
        (
            Kind::Kind7 | Kind::Event | Kind::Career | Kind::List | Kind::Setup | Kind::Kind38 | Kind::Intro,
            0..=2
        )
    )
}

/// A typed handler (see [`is_typed`]).
pub fn run_typed(st: &mut MenuState, h: &mut impl Host, kind: Kind, phase: usize, _args: &[u32]) -> u32 {
    match (kind, phase) {
        (Kind::Kind7, 0) => map::enter(st, h),
        (Kind::Kind7, 1) => map::update(st, h),
        (Kind::Kind7, 2) => map::draw(st, h),
        (Kind::Event, 0) => event::enter(st, h),
        (Kind::Event, 1) => event::update(st, h),
        (Kind::Event, 2) => event::draw(st, h),
        (Kind::Career, 0) => results::enter(st, h),
        (Kind::Career, 1) => results::update(st, h),
        (Kind::Career, 2) => results::draw(st, h),
        (Kind::Intro, 0) => intro::enter(st, h),
        (Kind::Intro, 1) => intro::update(st, h),
        (Kind::Intro, 2) => intro::draw(st, h),
        (Kind::Kind38, 0) => hints::enter(st, h),
        (Kind::Kind38, 1) => hints::update(st, h),
        (Kind::Kind38, 2) => hints::draw(st, h),
        (Kind::Setup, 0) => setup::enter(st, h),
        (Kind::Setup, 1) => setup::update(st, h),
        (Kind::Setup, 2) => setup::draw(st, h),
        (Kind::List, 0) => list::enter(st, h),
        (Kind::List, 1) => list::update(st, h),
        (Kind::List, 2) => list::draw(st, h),
        _ => unreachable!("{kind:?} phase {phase} is not typed"),
    }
}
