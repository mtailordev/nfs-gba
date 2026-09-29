//! The settings screens (Setup: 0x10 options, 0xA Quick Play, 0xF career race; records at `0x7E6260`,
//! `career::setup_screens`) and the career race's opponents (`career_opponents`) on typed state. Cursor per screen
//! at profile `+0x368 + index`; arrow delays at `+0x374 + 2·item`. The screen edits its settings through the
//! profile's copies (`MenuProfile::settings`). Scene setup and drawing stay behind [`Host`] (FIDELITY U7).

use nfsgba_sim::state::{MenuGlobals, MenuState};

use super::event::hint_due;
use super::flow::{self, CARBON_PLAY_SOUND, Host, message_box_open, rom_u16, rom_u32};
use super::{
    INTRO_PAGE_SETUP, MENU_BLIT_MATERIAL, MENU_BLIT_MATERIAL_ALT, SAVE_WRITE_PROFILE, TEXT_BOX, TEXT_MENU, WORLD,
};

const SETUP_SCREENS: u32 = 0x087E_6260;

/// Indices in `MenuProfile::settings` (race settings first: reverse, laps, difficulty, opponents, traffic).
const LAPS: usize = 1;
const OPPONENTS: usize = 3;
const U_580C: usize = 5;
const OPTIONS: std::ops::Range<usize> = 6..10; // u_53e4, units, HUD, u_5798
const MUSIC: usize = 10;
const SOUND: usize = 11;
const LANGUAGE: usize = 12;
const U_0050: usize = 13;

/// The setup record of the screen: 0x10 → 0, 0xA → 1, 0xF → 2..5 by race mode; −1 for anything else.
fn setup_index(st: &MenuState) -> i32 {
    match st.g.screen {
        0xF => match st.g.race_mode {
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

/// A setting's variable (`setting_variable`, `0x08132790`): 0 is the race mode, 1 the wingman, 2..=0x10 the
/// profile's setting copies; others none (reads 0, writes are dropped).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Var {
    RaceMode,
    Wingman,
    Copy(usize),
    None,
}

fn variable(id: u32) -> Var {
    match id {
        0 => Var::RaceMode,
        1 => Var::Wingman,
        2..=0x10 => Var::Copy(id as usize - 2),
        _ => Var::None,
    }
}

fn get(st: &MenuState, v: Var) -> i32 {
    match v {
        Var::RaceMode => st.g.race_mode as i32,
        Var::Wingman => st.profile.wingman as i32,
        Var::Copy(i) => st.profile.settings[i] as i32,
        Var::None => 0,
    }
}

fn set(st: &mut MenuState, v: Var, x: i32) {
    match v {
        Var::RaceMode => st.g.race_mode = x as u32,
        Var::Wingman => st.profile.wingman = x as u32,
        Var::Copy(i) => st.profile.settings[i] = x as u32,
        Var::None => {}
    }
}

/// The five race globals the screens edit through the copies `0..5`.
fn race_globals(g: &mut MenuGlobals) -> [&mut u32; 5] {
    [
        &mut g.reverse,
        &mut g.laps,
        &mut g.difficulty,
        &mut g.opponents,
        &mut g.traffic,
    ]
}

/// The copies to the globals (the race settings and `u_580c`, the sound flag).
fn copies_to_race(st: &mut MenuState) {
    let copies = st.profile.settings;
    for (i, v) in race_globals(&mut st.g).into_iter().enumerate() {
        *v = copies[i];
    }
    st.g.u_580c = 0;
}

/// `setup_screen_enter` (`0x081328F4`): the page's background, at most 2 opponents with a wingman, and the
/// settings copied into the profile (`+0x3BC…+0x3F4`; music and sound volumes / 8).
pub fn enter(st: &mut MenuState, h: &mut impl Host) -> u32 {
    let page = setup_page(setup_index(st));
    let (m, p) = (s16(h, page + 6), s16(h, page + 8));
    h.scene_setup(st, m, p, 0xFFFF);
    if st.profile.wingman != 0 && st.g.opponents > 2 {
        st.g.opponents = 2;
    }
    st.profile.settings_head = 0;
    let g = &mut st.g;
    let mut copies = st.profile.settings;
    for (i, v) in race_globals(g).into_iter().enumerate() {
        copies[i] = *v;
    }
    copies[U_580C] = g.u_580c;
    copies[U_0050] = g.u_0050;
    copies[6] = g.u_53e4;
    copies[7] = g.units;
    copies[8] = g.hud_on;
    copies[9] = g.u_5798;
    copies[MUSIC] = g.music_volume >> 3;
    copies[SOUND] = g.sound_volume >> 3;
    copies[LANGUAGE] = g.language;
    st.profile.settings = copies;
    st.g.u_5994 = 0;
    1
}

fn s16(h: &impl Host, a: u32) -> u32 {
    rom_u16(h.rom(), a) as i16 as i32 as u32
}

fn cursor(st: &MenuState, index: i32) -> i8 {
    st.profile.setup_cursors.get(index as usize).map_or(0, |&c| c as i8)
}

/// Whether the option settings differ from the profile's copies (what the save question asks).
fn options_changed(st: &MenuState) -> bool {
    let (g, c) = (&st.g, &st.profile.settings);
    [g.u_53e4, g.units, g.hud_on, g.u_5798] != c[OPTIONS]
        || g.music_volume >> 3 != c[MUSIC]
        || g.sound_volume >> 3 != c[SOUND]
        || g.language != c[LANGUAGE]
        || g.u_0050 != c[U_0050]
}

/// `setup_screen_update` (`0x08132AB8`). Outside 0xA the profile copies drive the race globals; left/right step
/// the item's value within its range (at most 2 laps/opponents with a wingman; laps and opponents move together in
/// an elimination), up/down move the cursor, SELECT on 0xF opens the career menu (9). A runs the item's action:
/// a screen (Quick Play 0x81 may first show a hint, 0x2B), back when nothing changed, else the save question
/// (0x87). A "yes" on the options (0x10) writes the settings back and saves the profile.
pub fn update(st: &mut MenuState, h: &mut impl Host) -> u32 {
    let index = setup_index(st);
    let screens = nfsgba_formats::career::setup_screens(h.rom());
    let screen = screens.get(index as usize);
    let count = screen.map_or(0, |s| s.items.len() as i32);
    let item_at = |st: &MenuState| screen.map(|s| s.items[cursor(st, index) as usize].clone());
    let item = item_at(st).expect("setup item");
    let (min, mut max) = (item.min as i32, item.max as i32);
    let var = variable(item.setting);
    if st.g.screen != 0xA {
        copies_to_race(st);
        st.g.u_0050 = st.profile.settings[U_0050];
        if st.g.screen == 0xF && st.g.keys & 0x200 != 0 {
            h.call(CARBON_PLAY_SOUND, &[4, 1]);
            flow::goto_screen(st, h, 9);
            st.g.screen_changed = 1;
        }
        if ((st.g.race_mode == 1 && var == Var::Copy(LAPS)) || var == Var::Copy(OPPONENTS)) && st.profile.wingman != 0 {
            max = 2;
        }
        // The arrow delay of the cursor's item: left or right.
        let delay = |st: &mut MenuState, side: usize| {
            let i = 2 * (cursor(st, index) as i32) as usize + side;
            if let Some(d) = st.profile.setup_delays.get_mut(i) {
                *d = 3;
            }
        };
        if st.g.keys & 0x20 != 0 {
            if get(st, var) == min {
                set(st, var, max + 1);
            }
            set(st, var, get(st, var) - 1);
            delay(st, 0);
            h.call(CARBON_PLAY_SOUND, &[4, 1]);
            st.g.screen_changed = 1;
        }
        if st.g.keys & 0x10 != 0 {
            let v = get(st, var) + 1;
            set(st, var, v);
            if max < v {
                set(st, var, min);
            }
            delay(st, 1);
            h.call(CARBON_PLAY_SOUND, &[4, 1]);
            st.g.screen_changed = 1;
        }
        if st.g.race_mode == 1 {
            if var == Var::Copy(LAPS) {
                st.profile.settings[OPPONENTS] = get(st, var) as u32;
            }
            if var == Var::Copy(OPPONENTS) {
                st.profile.settings[LAPS] = get(st, var) as u32;
            }
            st.g.laps = st.profile.settings[LAPS];
            st.g.opponents = st.profile.settings[OPPONENTS];
        }
        if st.g.keys & 0x40 != 0 {
            let c = cursor(st, index);
            if c > 0 {
                st.profile.setup_cursors[index as usize] = (c as u8).wrapping_sub(1);
                h.call(CARBON_PLAY_SOUND, &[1, 1]);
                st.g.screen_changed = 1;
            }
        }
        if st.g.keys & 0x80 != 0 {
            let c = cursor(st, index);
            if (c as i32) < count - 1 {
                st.profile.setup_cursors[index as usize] = (c as u8).wrapping_add(1);
                h.call(CARBON_PLAY_SOUND, &[1, 1]);
                st.g.screen_changed = 1;
            }
        }
    }
    for d in st.profile.setup_delays.iter_mut().filter(|d| **d > 0) {
        *d -= 1;
    }
    let action = item_at(st).expect("setup item").action as u16 as i16 as i32;
    if st.g.keys == 1 && action != -1 {
        st.g.settings_changed = (options_changed(st)) as u32;
        if action < 0x85 {
            if action == 0x81 {
                if st.g.career == 1 && hint_due(st, h.rom(), 10, 0) != 0 {
                    h.call(CARBON_PLAY_SOUND, &[2, 1]);
                    flow::goto_screen(st, h, 0x2B);
                    st.g.screen_changed = 1;
                    return 1;
                }
                st.g.back_top = 1;
            }
            flow::goto_screen(st, h, action);
            st.g.screen_changed = 1;
        } else if st.g.settings_changed == 0 {
            flow::menu_back(st, h);
        } else if action == 0x87 {
            message_box_open(st, 2, 0x1D8, u32::MAX);
        }
        h.call(CARBON_PLAY_SOUND, &[2, 1]);
    }
    if st.g.message_result > 0 {
        if st.g.screen == 0x10 {
            if st.g.settings_changed != 0 {
                copies_to_race(st);
                let c = st.profile.settings;
                let g = &mut st.g;
                (g.u_53e4, g.units, g.hud_on, g.u_5798) = (c[6], c[7], c[8], c[9]);
                g.music_volume = c[MUSIC] << 3;
                g.sound_volume = c[SOUND] << 3;
                g.language = c[LANGUAGE];
                g.u_0050 = c[U_0050];
                h.call(SAVE_WRITE_PROFILE, &[st.g.save_buffer]);
            }
            flow::menu_back(st, h);
        }
        st.g.message_result = 0;
    }
    1
}

fn text(h: &mut impl Host, font: u32, key: u32, x: u32, y: u32, a: u32, c: u32) {
    h.call(TEXT_MENU, &[font, key, x, y, a, c]);
}

fn alt(h: &mut impl Host, m: u32, x: u32, y: u32) {
    h.call(MENU_BLIT_MATERIAL_ALT, &[WORLD, m, x, y]);
}

/// `setup_screen_draw` (`0x08133074`): heading; on 0xA a summary (track or mode, car, wingman, blinking on bit 7
/// of the frame counter) and items from the fourth; every item's name and value text (`options[value]` in range,
/// else the text at `0x087E62C0`), the cursor highlight and, outside 0xA, the arrows; prompts (0xF adds 0xB5).
pub fn draw(st: &mut MenuState, h: &mut impl Host) -> u32 {
    let index = setup_index(st);
    let page = setup_page(index);
    let screens = nfsgba_formats::career::setup_screens(h.rom());
    let items = screens.get(index as usize).map(|s| s.items.clone()).unwrap_or_default();
    let count = rom_u16(h.rom(), page + 10) as i16 as i32;
    let slot = rom_u32(h.rom(), 0x087E_49C4_u32.wrapping_add(st.g.route.wrapping_mul(4)));
    st.g.player_car = st.profile.career_car as i32 as u32;
    h.call(INTRO_PAGE_SETUP, &[st.g.unpack_buffer]);
    let heading = s16(h, page);
    text(h, 0xC, heading, 0xEC, 2, u32::MAX, 0);
    let (first, top, pitch) = if st.g.screen == 0xA {
        for (m, x, y) in [
            (0xD2, 2, 0xF),
            (0xD5, 0x52, 0xF),
            (0xD2, 2, 0x1F),
            (0xD5, 0x52, 0x1F),
            (0xD2, 8, 0x2F),
            (0xD4, 0x78, 0x2F),
        ] {
            alt(h, m, x, y);
        }
        if st.g.flash & 0x80 == 0 {
            text(h, 0xE, 0x2E3, 3, 0x14, 0, 0);
            let track = rom_u16(h.rom(), 0x087E_4A70_u32.wrapping_add(slot.wrapping_mul(4))) as u32;
            text(h, 0xE, track, 0x55, 0x14, 0, 0);
        } else {
            text(h, 0xE, 0x131, 3, 0x14, 0, 0);
            let mode = rom_u32(h.rom(), 0x0879_9B6C + 4 * st.g.race_mode);
            text(h, 0xE, mode, 0x55, 0x14, 0, 0);
        }
        text(h, 0xE, items[1].text_key, 3, 0x24, 0, 0);
        let car = if st.g.career == 0 {
            st.profile.car as i32
        } else {
            st.g.player_car as i32
        };
        let name = s16(h, 0x087E_517C_u32.wrapping_add((car * 8) as u32));
        text(h, 0xE, name, 0x55, 0x24, 0, 0);
        text(h, 0xE, 0x3A5, 0xC, 0x34, 0, 0);
        let w = st.profile.wingman;
        let (key, width) = if w == 0 {
            (0x3A6, 0x60)
        } else if st.g.flash & 0x80 == 0 {
            (if w & 1 == 0 { 0x113 } else { 0x90 }, 0x70)
        } else {
            (w + 0x3A6, 0x60)
        };
        h.call(TEXT_BOX, &[0xE, key, 0xB0, 0x34, width, 2, 0]);
        (3, 0x13, 0x10)
    } else {
        (0, 0x18, 0x11)
    };
    let cursor = cursor(st, index) as i32;
    for i in first..count {
        let it = &items[i as usize];
        let on = i == cursor;
        let y = pitch * i + top;
        if on {
            alt(h, 0xD9, 8, (y - 4) as u32);
            alt(h, 0xD7, 0x78, (y - 4) as u32);
        } else {
            alt(h, 0xD2, 8, (y - 4) as u32);
            alt(h, 0xD4, 0x78, (y - 4) as u32);
        }
        let colour = if on { 8 } else { 0 };
        text(h, 0xE, it.text_key, 0xC, (y + 1) as u32, 0, colour);
        let var = variable(it.setting);
        let key = if var == Var::None {
            it.options[0]
        } else {
            let v = get(st, var);
            if (it.min as i32..=it.max as i32).contains(&v) {
                it.options[v as usize]
            } else {
                0x087E_62C0
            }
        };
        h.call(TEXT_BOX, &[0xE, key, 0xB0, (y + 1) as u32, 0x70, 2, colour]);
        if st.g.screen != 0xA && st.g.message_box < 0 {
            let lit = |off: usize| {
                st.profile
                    .setup_delays
                    .get(2 * i as usize + off)
                    .is_some_and(|&d| d >= 1)
            };
            let m = if lit(0) { 0xAE } else { 0xAD };
            h.call(MENU_BLIT_MATERIAL, &[WORLD, m, 0x76, (y - 4) as u32]);
            let m = if lit(1) { 0xB0 } else { 0xAF };
            h.call(MENU_BLIT_MATERIAL, &[WORLD, m, 0xDA, (y - 4) as u32]);
        }
    }
    let (l, r) = (s16(h, page + 2), s16(h, page + 4));
    let third = if st.g.screen == 0xF { 0xB5 } else { u32::MAX };
    h.button_prompts(st, &[l, r, third]);
    0
}

/// `setup_screen_15_prepare` (`FUN_0812B320`, entering screen 15): the career race's opponents, ids in the results
/// slots `0x03005655..=57`: three different random picks (`rand & 7`) of the zone's eight (`0x40 + 8·zone`, zone 5:
/// `0x60`), the first replaced by the zone's boss (`0x10 + 2·zone`, `+1`) on its boss events (and event 0x41), and
/// the wingman (`0x20 +` wingman) in the last opponent slot. In a two-player career before the first hint the
/// first opponent is fixed (0x2D).
pub fn career_opponents(st: &mut MenuState, h: &mut impl Host) {
    let zone = st.profile.zone as u32;
    let event = st.profile.event_slot as i32 + (zone * 12) as i32;
    if st.g.career == 2 && st.profile.hints_a == 0 {
        st.g.results.ids[1] = 0x2D;
        return;
    }
    let a = flow::rand_table(st, h) as u8 & 7;
    let b = loop {
        let v = flow::rand_table(st, h) as u8 & 7;
        if v != a {
            break v;
        }
    };
    let c = loop {
        let v = flow::rand_table(st, h) as u8 & 7;
        if v != b && v != a {
            break v;
        }
    };
    let z = st.profile.zone as i8;
    let base = if z == 5 {
        0x60
    } else {
        (z as u8).wrapping_mul(8).wrapping_add(0x40)
    };
    let twice = (zone * 2) as u8;
    let mut first = a.wrapping_add(base);
    if rom_u16(h.rom(), 0x087E_4714 + zone * 4) as i16 as i32 == event {
        first = twice.wrapping_add(0x10);
    }
    if rom_u16(h.rom(), 0x087E_4714 + (zone * 2 + 1) * 2) as i16 as i32 == event {
        first = twice.wrapping_add(0x11);
    }
    if event == 0x41 {
        first = twice.wrapping_add(0x11);
    }
    let ids = &mut st.g.results.ids;
    ids[1] = first;
    ids[2] = b.wrapping_add(base);
    ids[3] = c.wrapping_add(base);
    let w = st.profile.wingman;
    if w != 0 {
        // Slot `opponents + 1` of the results table (a byte image; past the ids it is the next rows).
        let mut bytes = st.g.results.to_bytes();
        if let Some(b) = bytes.get_mut(st.g.opponents as usize + 5) {
            *b = (w as u8).wrapping_add(0x20);
        }
        st.g.results = nfsgba_sim::state::RaceResults::from_bytes(&bytes);
    }
}
