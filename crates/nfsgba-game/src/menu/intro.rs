//! The boot and intro screens (Intro: 0x15 credits, 0x16 name keyboard, 0x17 title, 0x18 public service
//! announcement, 0x19 language, 0x1A EA logo, 0x25, 0x2F/0x30 health and safety, 0x47, 0x48) on typed state. The
//! second palette's colours and the scene setup stay behind [`Host`] (FIDELITY U7).

use nfsgba_sim::state::MenuState;

use super::flow::{self, CARBON_PLAY_SOUND, Host, rom_u16, rom_u32};
use super::{
    FLASH_BLINK, INTRO_PAGE_SETUP, MENU_BLIT_MATERIAL, MENU_BUTTON_PROMPTS, SAVE_WRITE_PROFILE, TEXT_MENU,
    TEXT_MENU_WRAPPED, VBLANK_INTR_WAIT, WORLD,
};

/// The name buffer's address (the game passes it to the text primitive as a pointer).
const NAME: u32 = 0x0300_5970;
const MENU_BLIT_MATERIAL_ALT: u32 = 0x0813_6E60; // (world, material, x, y)
const CARBON_PLAY_MUSIC: u32 = 0x0813_6054;
const SAVE_LOAD_PROFILE: u32 = 0x0814_9D84;

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

fn s16(h: &impl Host, a: u32) -> u32 {
    rom_u16(h.rom(), a) as i16 as i32 as u32
}

/// `intro_enter` (`0x081315A0`): the screen's background (menu scene setup `b` after a screen was entered, else
/// `a`), its deadline (tick counter + 0x5A on 0x2F, 0x1E0 on 0x18, else 0xF0) and per-screen state.
pub fn enter(st: &mut MenuState, h: &mut impl Host) -> u32 {
    let screen = st.g.screen;
    let page = intro_page(screen).expect("intro_enter on a screen without an intro page");
    if screen != 0x30 {
        let (a, b) = (s16(h, page + 6), s16(h, page + 8));
        h.scene_setup(st, a, b, 0xFFFF);
    }
    if st.g.screen == 0x2F {
        // Health and safety: colours 0..4 from 0x7E5E48, the language's text image.
        h.copy_mem(st.g.second_palette, 0x087E_5E48, 10, 0x10);
        let image = match st.g.language {
            0 => Some(7),
            1 => Some(8),
            2 => Some(0xA),
            3 => Some(9),
            4 => Some(0xB),
            _ => None,
        };
        if let Some(n) = image {
            h.call(0x0813_644C, &[WORLD, n]);
        }
        st.profile.deadline = st.g.ticks.wrapping_add(0x5A);
    } else {
        st.profile.deadline = st.g.ticks.wrapping_add(0xF0);
    }
    match st.g.screen {
        0x15 => {
            st.g.u_5984 = 0;
            st.g.u_5980 = 0;
            st.g.credits = 0x0879_9882;
        }
        0x16 => {
            st.g.keyboard_column = 0;
            st.g.keyboard_row = 0;
            st.g.name_len = 0;
            h.call(0x0813_56DC, &[]); // profile_reset
            st.g.name = [0; 9];
            // Start from the profile's current name.
            // ponytail: the game reads on until a 0 byte; a full 9-byte name stops at 9 here.
            while let Some(&b) = st.profile.name.get(st.g.name_len as usize)
                && b != 0
            {
                st.g.name[st.g.name_len as usize] = b;
                st.g.name_len += 1;
            }
            if st.profile.u_494 != 2 {
                st.profile.stats = [0; 6];
            }
        }
        0x17 => {
            h.call(CARBON_PLAY_MUSIC, &[0]); // the title music
        }
        0x18 => {
            st.profile.deadline = st.g.ticks.wrapping_add(0x1E0);
        }
        _ => {}
    }
    1
}

fn blit(h: &mut impl Host, m: u32, x: u32, y: u32) {
    h.call(MENU_BLIT_MATERIAL, &[WORLD, m, x, y]);
}

/// `intro_draw` (`0x08131FE0`): the intro screens' page (`FUN_081364C4`), heading, content and button prompts.
/// The health and safety screens (0x2F, 0x30) only set up the page.
pub fn draw(st: &mut MenuState, h: &mut impl Host) -> u32 {
    let screen = st.g.screen;
    let page = intro_page(screen).expect("intro_draw on a screen without an intro page");
    let count = rom_u16(h.rom(), page + 0xA) as i16 as i32;
    let items = rom_u32(h.rom(), page + 0x10);
    h.call(INTRO_PAGE_SETUP, &[st.g.unpack_buffer]);
    if screen.wrapping_sub(0x2F) <= 1 {
        return 0;
    }
    let neg1 = u32::MAX;
    if screen == 0x16 {
        let key = if st.profile.u_494 == 2 { 0x122 } else { 0x10C };
        h.call(TEXT_MENU, &[0xC, key, 0xEC, 2, neg1, 0]);
    } else {
        let title = rom_u16(h.rom(), page) as i16 as i32;
        if title != -1 {
            h.call(TEXT_MENU, &[0xC, title as u32, 0xEC, 2, neg1, 0]);
        }
    }
    let lang = st.g.language;
    match screen {
        0x15 => credits_draw(st, h),
        0x16 => {
            h.call(TEXT_MENU, &[0xD, 0x164, 0x4A, 0x14, neg1, 8]);
            h.call(TEXT_MENU, &[0xD, NAME, 0x78, 0x14, 1, 0]);
            let letters = match lang {
                1 => Some(0xDE),
                2 => Some(0xE0),
                3 => Some(0xDF),
                4 => Some(0xE1),
                _ => None,
            };
            if let Some(m) = letters {
                h.call(MENU_BLIT_MATERIAL_ALT, &[WORLD, m, 0x58, 0x77]);
            }
            let (row, col) = (st.g.keyboard_row, st.g.keyboard_column);
            if row == 4 {
                let b = if col > 6 {
                    2
                } else if col > 2 {
                    1
                } else {
                    0
                };
                blit(h, 0xB4, (67 * b + 0x11) as u32, (row * 20 + 0x22) as u32);
            } else {
                blit(h, 0xB3, (col * 20 + 0x14) as u32, (row * 20 + 0x24) as u32);
            }
        }
        0x17 => {
            blit(h, 0xCB, 0x30, 0x8A);
            if lang <= 4 {
                blit(h, 0xC4 + lang, 0, 0x96); // logo line per language
            }
            blit(h, if lang == 4 { 0xC9 } else { 0xCA }, 0x60, 4);
            if st.g.ticks > st.profile.deadline && st.g.flash & FLASH_BLINK != 0 && lang <= 4 {
                blit(h, 0xBF + lang, 0xAA, 0x50); // PRESS START, blinking
            }
        }
        0x18 => {
            h.call(TEXT_MENU, &[0xD, 0x19A, 0x78, 6, 1, 0]);
            h.call(TEXT_MENU, &[0xD, 0x19B, 0x78, 0x10, 1, 0]);
            h.call(TEXT_MENU_WRAPPED, &[0xE, 0x199, 8, 0x1E, 0xE0, 0xF, 8]);
        }
        0x19 => {
            // The cursor on the selected language, blinking: (material, x, y) per item from the page's list + 2.
            for i in 0..count.max(0) as u32 {
                let item = items + 2 + 10 * i;
                if st.g.flash & FLASH_BLINK != 0 && st.g.language_cursor == i {
                    let [m, x, y] = [0, 2, 4].map(|o| s16(h, item + o));
                    blit(h, m, x, y);
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
                blit(h, m, 0, 0x91);
            }
        }
        _ => {}
    }
    let (left, right) = (s16(h, page + 2), s16(h, page + 4));
    h.call(MENU_BUTTON_PROMPTS, &[left, right, neg1]);
    0
}

// The credits pointer lives in the ROM once the screen is entered, but stale RAM before: `Host::peek16` reads either.

/// The extra rows a credits line's flags add.
fn extra_rows(flags: u16) -> i32 {
    [(0x2000, 4), (0x1000, 6), (0x800, 0xC)]
        .iter()
        .filter(|(b, _)| flags & b != 0)
        .map(|(_, h)| h)
        .sum::<i32>()
}

/// The credits page (screen 0x15): the current entry's lines (u16 count, then (flags, text key) pairs), centred in
/// 0x90 rows. Flags: bits 0–1 colour (× 8), 0x2000 small font and 4 rows more, 0x1000 6 more, 0x800 12 more; on the
/// first line 0x8000 starts at row 0x28 and 0x4000 at row 0. Some keys sit lower in French, Spanish and Italian.
fn credits_draw(st: &mut MenuState, h: &mut impl Host) {
    let p = st.g.credits;
    let n = h.peek16(p) as u32;
    let mut height = 12 * n as i32;
    for k in 0..n {
        height += extra_rows(h.peek16(p + 2 + 4 * k));
    }
    let mut y = (0x90 - height) >> 1;
    for k in 0..n {
        let entry = p + 4 * k;
        let flags = h.peek16(entry + 2);
        let colour = ((flags & 3) << 3) as u32;
        let font = if flags & 0x2000 != 0 { 0xC } else { 0xD };
        if flags & 0x8000 != 0 && k == 0 {
            y = 0x28;
        }
        if h.peek16(p + 2) & 0x4000 != 0 && k == 0 {
            y = 0;
        }
        let key = h.peek16(entry + 4);
        match st.g.language {
            1 if key == 0x1B || key == 0x1D => y += 0xE,
            4 if key == 0x1F => y += 0xE,
            3 if key == 0x19A => y += 4,
            _ => {}
        }
        h.call(TEXT_MENU_WRAPPED, &[font, key as u32, 2, y as u32, 0xEE, 0xF, colour]);
        y += extra_rows(flags) + 0xC;
    }
}

/// `intro_update` (`0x081318E4`): the boot and intro screens. Timed screens move on once the tick counter passes
/// profile `+0x3B0`.
pub fn update(st: &mut MenuState, h: &mut impl Host) -> u32 {
    let screen = st.g.screen;
    let expired = |st: &MenuState| st.g.ticks > st.profile.deadline;
    match screen {
        // Credits: each deadline advances to the next entry (u16 count, count words); the end presses B.
        0x15 => {
            if expired(st) {
                let p = st.g.credits;
                st.g.credits = p.wrapping_add(4 * h.peek16(p) as u32 + 2);
                st.profile.deadline = st.g.ticks.wrapping_add(0xB4);
            }
            if h.peek16(st.g.credits) == 0 {
                st.g.keys = 2;
            }
        }
        0x16 => name_entry(st, h),
        // Title: START once the deadline has passed loads the profile, or asks for a name.
        0x17 => {
            if st.g.keys & 8 != 0 && expired(st) {
                h.call(CARBON_PLAY_SOUND, &[2, 1]);
                st.g.back_top = -1;
                st.profile.u_494 = 0;
                if st.profile.profile_exists != 0 {
                    h.call(0x0814_19C0, &[0xC, 0x159, 0x78, 0x32, 0xDC, 2, 0]);
                    h.call(SAVE_LOAD_PROFILE, &[st.g.save_buffer]);
                    let lang = st.g.language;
                    if lang != st.profile.saved_language as u32 {
                        st.g.units = (lang != 0) as u32;
                    }
                    flow::goto_screen(st, h, 0);
                } else {
                    flow::goto_screen(st, h, 0x16);
                    st.g.screen_changed = 1;
                }
            }
        }
        // Public service announcement, EA logo: the page's next screen (item list `+8`) at the deadline.
        0x18 | 0x1A => {
            if expired(st) {
                st.g.back_top = -1;
                let items = rom_u32(h.rom(), 0x087E_5DA8 + 0x14 * (screen - 0x15) + 0x10);
                let next = s16(h, items + 8) as i32;
                flow::goto_screen(st, h, next);
            }
        }
        0x19 => language_select(st, h),
        // Health and safety, first part: after its deadline, the blinking part (0x30) for 0xDB6 ticks.
        0x2F => {
            if expired(st) {
                st.profile.deadline = st.g.ticks.wrapping_add(0xDB6);
                st.g.screen = 0x30;
                h.set_second_colour(4, 0);
                st.g.blink_dir = 0;
            }
        }
        // Health and safety, blinking: any key or the deadline goes to the EA logo; else colour 4 steps by ±0x421
        // between 0 and 0x7FFF (direction in `blink_dir`) and the frame waits an extra VBlank.
        0x30 => {
            if st.g.keys & 0x3FF != 0 || expired(st) {
                h.call(CARBON_PLAY_SOUND, &[2, 1]);
                flow::goto_screen(st, h, 0x1A);
                st.g.screen_changed = 1;
            } else {
                let mut c = h.second_colour(4) as u32;
                if st.g.blink_dir != 0 {
                    if c == 0 {
                        c = 0x421;
                        st.g.blink_dir = 0;
                    } else {
                        c = c.wrapping_sub(0x421);
                    }
                } else if c == 0x7FFF {
                    c -= 0x421;
                    st.g.blink_dir = 1;
                } else {
                    c = c.wrapping_add(0x421);
                }
                h.set_second_colour(4, c as u16);
                st.g.palette_dirty = 1;
                h.call(VBLANK_INTR_WAIT, &[]);
            }
        }
        _ => {}
    }
    1
}

/// `FUN_081318B0`: the typed name counts when its 9 bytes OR to neither 0 nor 0x20; plays sound 2 either way.
fn name_is_valid(st: &MenuState, h: &mut impl Host) -> bool {
    let or = st.g.name.iter().fold(0, |a, b| a | b);
    h.call(CARBON_PLAY_SOUND, &[2, 1]);
    or != 0 && or != 0x20
}

/// Screen 0x16: the profile name keyboard, 4 rows of 10 characters and a row of DEL / SPACE / OK. B deletes,
/// START is OK; OK saves the profile.
fn name_entry(st: &mut MenuState, h: &mut impl Host) {
    let keys = st.g.keys;
    if keys & 0x40 != 0 {
        let r = st.g.keyboard_row - 1;
        st.g.keyboard_row = if r < 0 { 4 } else { r };
        h.call(CARBON_PLAY_SOUND, &[4, 1]);
    }
    if keys & 0x80 != 0 {
        let r = st.g.keyboard_row + 1;
        st.g.keyboard_row = if r > 4 { 0 } else { r };
        h.call(CARBON_PLAY_SOUND, &[4, 1]);
    }
    if keys & 0x20 != 0 {
        if st.g.keyboard_row == 4 {
            let c = st.g.keyboard_column;
            st.g.keyboard_column = if c > 6 {
                4
            } else if c <= 2 {
                8
            } else {
                1
            };
        }
        let c = st.g.keyboard_column - 1;
        st.g.keyboard_column = if c < 0 { 9 } else { c };
        h.call(CARBON_PLAY_SOUND, &[4, 1]);
    }
    if keys & 0x10 != 0 {
        if st.g.keyboard_row == 4 {
            let c = st.g.keyboard_column;
            st.g.keyboard_column = if c > 6 {
                -1
            } else if c > 2 {
                6
            } else {
                2
            };
        }
        let c = st.g.keyboard_column + 1;
        st.g.keyboard_column = if c > 9 { 0 } else { c };
        h.call(CARBON_PLAY_SOUND, &[4, 1]);
    }
    if keys & 0xB == 0 {
        return;
    }
    let (row, col) = (st.g.keyboard_row, st.g.keyboard_column);
    let mut key = col + row * 10;
    if row == 4 {
        key = if col > 6 {
            2
        } else if col > 2 {
            1
        } else {
            0
        } + row * 10;
    }
    if keys == 8 {
        key = 0x2A;
    }
    if keys == 2 {
        key = 0x28;
    }
    match key {
        0x28 => {
            let len = st.g.name_len;
            if len == 0 {
                h.call(CARBON_PLAY_SOUND, &[0x27, 1]);
            } else {
                st.g.name_len = len - 1;
                if let Some(b) = st.g.name.get_mut((len - 1) as usize) {
                    *b = 0;
                }
                h.call(CARBON_PLAY_SOUND, &[2, 1]);
            }
        }
        0x2A => {
            if !name_is_valid(st, h) {
                h.call(CARBON_PLAY_SOUND, &[0x27, 1]);
                return;
            }
            h.call(CARBON_PLAY_SOUND, &[2, 1]);
            st.profile.name = st.g.name;
            if h.call(SAVE_WRITE_PROFILE, &[st.g.save_buffer]) != 0 {
                flow::goto_screen(st, h, 0); // save_write_profile failed
            } else if st.profile.u_494 == 2 {
                flow::menu_back(st, h);
            }
            st.g.screen_changed = 1;
        }
        _ => {
            let len = st.g.name_len;
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
            if let Some(b) = st.g.name.get_mut(len as usize) {
                *b = ch as u8;
            }
            st.g.name_len = len + 1;
            h.call(CARBON_PLAY_SOUND, &[2, 1]);
            if len + 1 == 8 && name_is_valid(st, h) {
                st.g.keyboard_column = 8;
                st.g.keyboard_row = 4;
            }
        }
    }
}

/// Screen 0x19: the five languages (`0x7E5D10`, cursor `language_cursor`); A picks one and goes on to the health
/// and safety screen (0x2F).
fn language_select(st: &mut MenuState, h: &mut impl Host) {
    fn pick(st: &mut MenuState, h: &impl Host) {
        st.g.language = rom_u32(h.rom(), 0x087E_5D10 + 4 * st.g.language_cursor);
    }
    let keys = st.g.keys;
    if keys == 1 {
        pick(st, h);
        h.call(CARBON_PLAY_SOUND, &[2, 1]);
        flow::goto_screen(st, h, 0x2F);
        st.g.back_top = -1;
        st.g.screen_changed = 1;
        return;
    }
    // Each key moves the cursor from where the last one left it.
    let mut step = |bit: u16, f: &dyn Fn(i32) -> i32| {
        if keys & bit != 0 {
            st.g.language_cursor = f(st.g.language_cursor as i32) as u32;
        }
    };
    step(0x20, &|c| if c == 0 { 4 } else { c - 1 });
    step(0x10, &|c| if c == 4 { 0 } else { c + 1 });
    step(0x40, &|c| {
        if c > 2 {
            c - 3
        } else if c == 0 {
            3
        } else {
            4
        }
    });
    step(0x80, &|c| {
        if c + 3 == 5 {
            4
        } else if c + 3 <= 4 {
            c + 3
        } else {
            c - 3
        }
    });
    if keys & 0xF0 != 0 {
        h.call(CARBON_PLAY_SOUND, &[4, 1]);
        st.g.screen_changed = 1;
    }
    pick(st, h);
}
