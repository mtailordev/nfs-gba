//! The List screens (0–6, 9, 27–30, 35, 36, 45, 46) on typed state: pages `0x7E544C` by `list_slot` (0x14 bytes:
//! heading, prompts `+2`/`+4`, background `+6`/`+8`, item count `+10`, no-profile flag `+0xC`, items `+0x10`: 8 bytes
//! each, `+0` text, `+2`/`+4` pictures, `+6` action). Cursor per slot at profile `+0x350 + slot`.
//! The scene setup and the drawing primitives stay behind [`Host`] (FIDELITY U7).

use nfsgba_sim::state::MenuState;

use super::event::{career_event_to_globals, hint_due};
use super::flow::{self, CARBON_PLAY_SOUND, Host, message_box_open, peek_back_i, rom_u16, rom_u32};
use super::text::{number_text, thousands};
use super::{INTRO_PAGE_SETUP, MENU_BLIT_MATERIAL, TEXT_BOX, TEXT_MENU, WORLD};

const LIST_PAGES: u32 = 0x087E_544C;
const MENU_BLIT_MATERIAL_ALT: u32 = 0x0813_6E60; // (world, material, x, y)
const UPGRADES_CHANGED: u32 = 0x0813_02C4; // (performance page?)

/// An i16 ROM read, sign-extended as the game passes it.
fn s16(h: &impl Host, a: u32) -> u32 {
    rom_u16(h.rom(), a) as i16 as i32 as u32
}

fn slot(st: &MenuState) -> i32 {
    flow::list_slot(st.g.screen, st.profile.u_12)
}

fn page(st: &MenuState) -> u32 {
    let s = slot(st);
    if s < 0 { 0 } else { LIST_PAGES + 0x14 * s as u32 }
}

fn cursor(st: &MenuState, slot: i32) -> i8 {
    st.profile.cursors.get(slot as usize).map_or(0, |&c| c as i8)
}

fn set_cursor(st: &mut MenuState, slot: i32, v: u8) {
    if let Some(c) = st.profile.cursors.get_mut(slot as usize) {
        *c = v;
    }
}

/// `garage_copy_car_record` (`FUN_0812D564`): the record of the garage car (17 bytes) to the working copy.
fn garage_copy_car_record(st: &mut MenuState) {
    // ponytail: the game reads the record through a pointer (`0x0300539C`, the profile's records); a car index past
    // the 15 records is out of range and reads as zeros here.
    let r = st.profile.car_records.get(st.g.player_car as usize).copied();
    st.g.garage_car = r.unwrap_or([0; 17]);
}

fn garage_select_car(st: &mut MenuState, h: &mut impl Host) {
    garage_copy_car_record(st);
    h.car_atlas(st);
    h.car_palette(st);
}

/// `list_enter` (`0x0812FE38`): the page's background; car select (9) starts on the career car (or Quick Play's,
/// or car 0 without a career profile) and loads it; the crew (0x2D) on the wingman (the first open one of the
/// first, third, … in career); the main menu empties the back stack.
pub fn enter(st: &mut MenuState, h: &mut impl Host) -> u32 {
    let slot = slot(st);
    let page = page(st);
    let first = st.g.screen_entered == 0 && st.g.screen != 5;
    let (m, p) = (s16(h, page + 6), s16(h, page + 8));
    h.scene_setup_ab(st, first, m, p, 0xFFFF);
    if st.g.screen == 9 {
        let car = if st.g.career == 0 {
            st.profile.car as i32
        } else if st.profile.u_12 == 0 {
            0
        } else {
            st.profile.career_car as i32
        };
        st.g.player_car = car as u32;
        set_cursor(st, slot, car as u8);
        garage_select_car(st, h);
    }
    if st.g.screen == 0x2D {
        if st.g.career == 1 {
            if st.profile.wingman == 0 {
                st.profile.wingman = 1;
            }
            for _ in 0..5 {
                let v = st.profile.wingman as i32;
                if v < 0xB && st.profile.is_locked(v + 0x129) == 0 {
                    st.profile.wingman = (v + 2) as u32;
                }
            }
        }
        set_cursor(st, slot, st.profile.wingman as u8);
    }
    if st.g.screen == 0 {
        st.g.back_top = -1;
    }
    1
}

/// `list_update` (`0x081303E8`). Left/right move the cursor (toggle on 2-item pages); on car select the car
/// changes and its model turns with the shoulder buttons. A runs the item's action: the crew pages pick a wingman
/// (open ones only), 3 (career) and 0xD may first show a hint, 0x1F–0x22 open the garage's upgrade pages, other
/// screens below 0x85 (RACE TYPE sets up a free race, QUICK PLAY's random race picks mode and track), and 0x85..
/// the car purchase, the profile, the options, the hints-done step, a career race's setup and the upgrades save.
/// Then a confirmed message box: resume the race (5), a new profile (2), buy the car (9), save the upgrades (0x8B).
pub fn update(st: &mut MenuState, h: &mut impl Host) -> u32 {
    let slot = slot(st);
    let page = page(st);
    let items = rom_u32(h.rom(), page + 0x10);
    let count = rom_u16(h.rom(), page + 10) as i16 as i32;
    for (bit, step) in [(0x20u16, -1i32), (0x10, 1)] {
        if st.g.keys & bit == 0 {
            continue;
        }
        if count < 3 {
            set_cursor(st, slot, cursor(st, slot) as u8 ^ 1);
            h.call(CARBON_PLAY_SOUND, &[4, 1]);
        } else {
            set_cursor(st, slot, (cursor(st, slot) as i32 + step) as u8);
            if step < 0 && cursor(st, slot) < 0 {
                set_cursor(st, slot, (count as u8).wrapping_sub(1));
            }
            if step > 0 && count <= cursor(st, slot) as i32 {
                set_cursor(st, slot, 0);
            }
            h.call(CARBON_PLAY_SOUND, &[4, 1]);
            if st.g.screen == 9 {
                st.g.player_car = cursor(st, slot) as i32 as u32;
                garage_select_car(st, h);
            }
        }
        st.g.screen_changed = 1;
    }
    if st.g.screen == 9 {
        for (bit, d) in [(0x40u16, 0x400u32), (0x80, 0xFFFF_FC00)] {
            if st.g.keys_held & bit != 0 {
                st.profile.u_2f6 = 0x80;
                st.profile.u_2f8 = st.g.garage_angle.wrapping_add(d);
            }
        }
    }
    let cur = cursor(st, slot) as i32;
    let item = items.wrapping_add((cur * 8) as u32);
    let action = rom_u16(h.rom(), item + 6) as i16 as i32;
    if st.g.keys == 1 {
        if st.g.screen == 0x2D {
            if st.profile.is_locked(cur + 0x127) == 0 {
                h.call(CARBON_PLAY_SOUND, &[2, 1]);
                pick_wingman(st, slot);
                st.g.opponents = if st.profile.wingman == 0 { 3 } else { 2 };
                if st.g.race_mode == 1 {
                    st.g.laps = st.g.opponents;
                }
                let top = peek_back_i(st, st.g.back_top);
                if top == 0xD || top == 0x26 {
                    flow::goto_screen(st, h, action);
                } else if top.wrapping_sub(7) < 2 {
                    flow::goto_screen(st, h, 0xF);
                } else {
                    flow::menu_back(st, h);
                }
                super::setup::career_opponents(st, h);
                return 1;
            }
            h.call(CARBON_PLAY_SOUND, &[0x27, 1]);
            return 1;
        }
        if action == 0x27 {
            if st.profile.is_locked(cur + 0x128) == 0 {
                h.call(CARBON_PLAY_SOUND, &[2, 1]);
                pick_wingman(st, slot);
                st.profile.hint_flag = 0;
                flow::goto_screen(st, h, action);
                return 1;
            }
            h.call(CARBON_PLAY_SOUND, &[0x27, 1]);
            return 1;
        }
        h.call(CARBON_PLAY_SOUND, &[2, 1]);
        let hint = (action == 3 && {
            st.g.career = 1;
            hint_due(st, h.rom(), 3, 0) != 0
        }) || (action == 0xD && hint_due(st, h.rom(), 0xD, 0) != 0);
        if hint {
            flow::goto_screen(st, h, 0x28);
            st.g.screen_changed = 1;
            return 1;
        }
        if action == -1 {
        } else if action < 0x85 {
            let upgrade = match action {
                0x1F => Some((false, -1, 0x13)),
                0x20 => Some((true, 0, 0x14)),
                0x21 => Some((true, 3, 0x14)),
                0x22 => Some((true, 2, 0x14)),
                _ => None,
            };
            if let Some((b, add, next)) = upgrade {
                st.profile.u_338 = 0;
                let v = (cursor(st, slot) as i32 + add) as u8;
                if b {
                    st.profile.upgrade_b = v;
                } else {
                    st.profile.upgrade_a = v;
                }
                flow::goto_screen(st, h, next);
                return 1;
            }
            match st.g.screen {
                6 => st.g.back_top = st.g.back_top.wrapping_sub(1),
                1 => free_race(st, h, slot),
                0x1C if cursor(st, slot) == 0 => random_quick_race(st, h),
                _ => {}
            }
            flow::goto_screen(st, h, action);
        } else if list_action(st, h, action) {
            return 1;
        }
    }
    confirmed_message(st, h, action);
    1
}

/// A crew page's A: the wingman (profile `+0x200`) and its side (`+0x204`).
fn pick_wingman(st: &mut MenuState, slot: i32) {
    let c = cursor(st, slot);
    st.profile.wingman = c as i32 as u32;
    st.profile.wingman_side = (c as u8).wrapping_add(1) & 1;
}

/// RACE TYPE (screen 1): a free race of the chosen mode with the Quick Play car, 2 opponents, 2 laps (1 in a
/// sprint), traffic, and random opponent looks.
fn free_race(st: &mut MenuState, h: &mut impl Host, slot: i32) {
    st.g.career = 0;
    st.g.race_mode = cursor(st, slot) as i32 as u32;
    st.g.player_car = st.profile.career_car as i32 as u32;
    if let Some(b) = st.g.slot_bytes.get_mut(st.g.race_player as usize) {
        *b = st.g.start_slot as u8;
    }
    st.g.u_580c = 0;
    st.g.difficulty = 1;
    st.g.opponents = 2;
    st.g.traffic = 1;
    st.g.laps = 2;
    st.g.u_562c = flow::rand_table(st, h) % 3 + 1;
    st.g.u_6118 = flow::rand_table(st, h) % 0x14;
    if st.g.race_mode == 3 {
        st.g.laps = 1;
    }
}

/// QUICK PLAY's random race (screen 0x1C, first item): a mode from `0x797D0A` (4 bytes) and a random track of it
/// (sprints `0x18 + rand % 18`; circuits `rand % 24`, reverse above 11), then `quick_race_random`.
fn random_quick_race(st: &mut MenuState, h: &mut impl Host) {
    let modes: [u8; 4] = std::array::from_fn(|i| h.rom()[0x79_7D0A + i]);
    st.g.career = 0;
    st.g.reverse = 0;
    let r = flow::rand_table(st, h);
    let mode = modes[(r & 3) as usize] as i8;
    st.g.race_mode = mode as i32 as u32;
    let track = if mode == 3 {
        (flow::rand_table(st, h) % 0x12) as u8 + 0x18
    } else {
        let t = (flow::rand_table(st, h) % 0x18) as u8;
        if t as i8 > 0xB {
            st.g.reverse = 1;
        }
        t
    };
    st.g.route = rom_u16(h.rom(), 0x087E_4A72_u32.wrapping_add(((track as i8 as i32) * 4) as u32)) as u32;
    h.quick_race_random(st);
}

/// A List item's action from 0x85 (`list_update`'s switch); true when it ends the update (a hint is due).
fn list_action(st: &mut MenuState, h: &mut impl Host, action: i32) -> bool {
    let slot = slot(st);
    match action - 0x85 {
        0 => {
            let state = {
                let id = st.g.player_car.wrapping_add(0x108);
                h.unlock_state(st, id)
            } as i32;
            if st.g.career == 0 {
                if state == 2 {
                    message_box_open(st, 1, 0xB4, u32::MAX);
                } else {
                    st.profile.car = st.g.player_car as i8;
                    flow::menu_back(st, h);
                }
            } else if state == 1 {
                st.profile.career_car = st.g.player_car as i8;
                if peek_back_i(st, st.g.back_top) == 0xF {
                    flow::menu_back(st, h);
                } else {
                    flow::goto_screen(st, h, 4);
                }
                h.save_write(st);
            } else if state == 0 {
                message_box_open(st, 2, 0x2BF, u32::MAX);
            } else if state == 2 {
                message_box_open(st, 1, 0xB4, u32::MAX);
            } else if state == 3 {
                message_box_open(st, 1, 0x172, u32::MAX);
            }
        }
        2 => {
            if cursor(st, slot) == 0 {
                st.profile.u_494 = 1;
                message_box_open(st, 2, 0x179, u32::MAX);
            } else {
                st.profile.u_494 = 2;
                message_box_open(st, 2, 0x121, u32::MAX);
            }
        }
        3 => {
            message_box_open(st, 2, 0x19F, u32::MAX);
        }
        4 => {
            let c = st.profile.hints_b;
            if c != 0 {
                st.profile.hints_a = c.wrapping_add(st.profile.hints_a);
                st.profile.hints_b = 0;
            }
            if hint_due(st, h.rom(), 6, 0) != 0 {
                flow::goto_screen(st, h, 0x28);
                st.g.screen_changed = 1;
                return true;
            }
            if st.profile.zone_step != 0 {
                if st.profile.zone < 5 {
                    st.profile.zone += 1;
                }
                st.profile.zone_step = 0;
            }
            h.save_write(st);
            st.profile.back[0] = 0;
            st.g.screen = if st.g.career == 0 { 0x1C } else { 3 };
            st.g.back_top = 0;
        }
        5 => {
            st.g.ranked.knocked = [0; 4];
            if st.g.career == 1 {
                career_event_to_globals(st, h.rom());
            }
            flow::goto_screen(st, h, 10);
            st.g.screen_changed = 1;
        }
        6 if st.profile.upgrades_saving == 0 => {
            message_box_open(st, 2, 0x91, u32::MAX);
        }
        _ => {}
    }
    false
}

/// The end of `list_update`: a message box answered with A. `action` is the item's.
fn confirmed_message(st: &mut MenuState, h: &mut impl Host, action: i32) {
    if st.g.message_result <= 0 {
        return;
    }
    match st.g.screen {
        5 => {
            // Back to the paused race.
            st.g.game_state = 5;
            st.g.u_5398 = 0;
            st.g.menu_exit = 2;
            st.g.race_outcome = 5;
            st.g.back_top = st.g.back_top.wrapping_sub(1);
            if st.profile.hints_b != 0 {
                st.profile.hints_b -= 1;
            }
        }
        2 => flow::goto_screen(st, h, 0x16),
        9 => {
            let id = st.g.player_car.wrapping_add(0x108);
            if h.unlock_state(st, id) < 2 {
                if st.profile.u_12 == 0 {
                    h.buy_unlock(st, id);
                    st.profile.hints_a = st.profile.hints_b.wrapping_add(st.profile.hints_a);
                    st.profile.hints_b = 0;
                    h.save_write(st);
                    st.g.back_top = -1;
                    st.g.screen = 0;
                    flow::goto_screen(st, h, 3);
                } else {
                    h.buy_unlock(st, id);
                    h.save_write(st);
                }
            }
        }
        _ => {}
    }
    if action as i16 == 0x8B {
        if st.profile.upgrades_saving == 0 {
            st.profile.upgrades_saving = 1;
            let changed = h.call(UPGRADES_CHANGED, &[(st.g.screen != 0x1D) as u32]);
            st.g.upgrades_changed = changed;
            message_box_open(st, 1, 0x376, changed);
        } else {
            st.profile.upgrades_saving = 0;
            if st.g.upgrades_changed != 0 {
                h.save_write(st);
            }
        }
    }
    st.g.message_result = 0;
}

fn text(h: &mut impl Host, font: u32, key: u32, x: u32, y: u32, a: u32, c: u32) -> u32 {
    h.call(TEXT_MENU, &[font, key, x, y, a, c])
}

fn tbox(h: &mut impl Host, font: u32, key: u32, x: u32, y: u32, w: u32, c: u32) {
    h.call(TEXT_BOX, &[font, key, x, y, w, 1, c]);
}

fn alt(h: &mut impl Host, m: u32, x: u32, y: u32) {
    h.call(MENU_BLIT_MATERIAL_ALT, &[WORLD, m, x, y]);
}

fn blit(h: &mut impl Host, m: u32, x: u32, y: u32) {
    h.call(MENU_BLIT_MATERIAL, &[WORLD, m, x, y]);
}

/// A number as the stack string the game passes to the text primitives.
fn number_arg(st: &mut MenuState, h: &mut impl Host, n: i32, separators: bool) -> u32 {
    let mut s = number_text(&mut st.g.div_remainder, n);
    if separators {
        thousands(st.g.language, &mut s, n);
    }
    h.text_arg(s)
}

/// `list_draw` (`0x08130D8C`): the heading; on car select the turning car, its name and stats; else three items
/// (previous, current, next) with their pictures, the crew's lock state, portrait and role, "new" marks; the arrows,
/// the player's name and cash, and the prompts (car select: buy or pick, with the price).
pub fn draw(st: &mut MenuState, h: &mut impl Host) -> u32 {
    let slot = slot(st);
    let page = page(st);
    let count = rom_u16(h.rom(), page + 10) as i16 as i32;
    let name: Vec<u8> = st.profile.name.iter().copied().take_while(|&b| b != 0).collect();
    let profile = h.text_arg(name); // the game passes the profile's address: its name
    let second = st.g.second_palette;
    let crew = (st.g.screen == 0x2E) as i32;
    h.call(INTRO_PAGE_SETUP, &[st.g.unpack_buffer]);
    let heading = s16(h, page);
    text(h, 0xC, heading, 0xEC, 2, u32::MAX, 0);
    let cur = cursor(st, slot);
    let (prev, next) = if count < 3 {
        (cur.wrapping_sub(1), cur.wrapping_add(1))
    } else {
        let p = if cur == 0 { count as i8 } else { cur };
        (p.wrapping_sub(1), ((cur as i32 + 1) % count) as i8) // __modsi3
    };
    let shown = [prev, next, cur];
    let items = rom_u32(h.rom(), page + 0x10);
    let (x2, y2) = (s16(h, 0x0879_7CD8 + 4), s16(h, 0x0879_7CE0 + 4));
    let arrow_y;
    if st.g.screen == 9 {
        arrow_y = 0x5A;
        h.draw_car(st, 0x78, 0x3C, 0xFA);
        let name = s16(h, items.wrapping_add((cur as i32 * 8) as u32));
        h.call(TEXT_BOX, &[0xC, name, 0x78, 0x74, 200, 1, 8]);
        h.car_stats(st, cur as i32 as u32, 0x78, 0x54, 0);
    } else {
        arrow_y = 0x18;
        blit(h, 0xDC, x2.wrapping_sub(9), y2.wrapping_sub(10));
        for (k, &v) in shown.iter().enumerate() {
            let v = v as i32;
            if v < 0 || v >= count {
                continue;
            }
            let item = items.wrapping_add((v * 8) as u32);
            let on_crew = st.g.screen.wrapping_sub(0x2D) < 2;
            if k == 2 {
                if on_crew {
                    if st.profile.is_locked(cur as i32 + crew + 0x127) != 0 {
                        alt(h, 0x3E, x2, y2);
                        tbox(h, 0xC, 0x15A, 0x78, 0x6E, 0xB0, 8);
                        let s = number_arg(st, h, cur as i32, false);
                        text(h, 0xC, s, x2.wrapping_add(0x40), y2, u32::MAX, 0);
                        continue;
                    }
                    let who = (cur as i32 + crew) as u32;
                    let pal = rom_u32(h.rom(), 0x087E_786C_u32.wrapping_add(who.wrapping_mul(4)));
                    h.copy_mem(second + 0x180, pal, 0x80, 0x10);
                    st.g.palette_dirty = 1;
                    let pic = s16(h, item + 4);
                    alt(h, pic, x2, y2);
                    if who != 0 {
                        let role = rom_u16(h.rom(), 0x087E_4990 + who * 4) as u32;
                        tbox(h, 0xE, role, x2.wrapping_add(0x20), y2.wrapping_add(2), 0xB0, 8);
                        let level = rom_u16(h.rom(), 0x087E_4990 + (who * 2 + 1) * 2) as u32;
                        blit(h, level + 0x10B, x2.wrapping_add(2), y2.wrapping_add(0x34));
                    }
                    let t = s16(h, item);
                    tbox(h, 0xC, t, 0x78, 0x6E, 0xB0, 8);
                } else {
                    let pic = s16(h, item + 4);
                    alt(h, pic, x2, y2);
                    let t = s16(h, item);
                    tbox(h, 0xC, t, 0x78, 0x6E, 0xB0, 8);
                    let screen = st.g.screen;
                    if h.new_mark(st, screen, cur as i32 as u32) != 0 {
                        alt(h, 0xB1, x2.wrapping_add(0x2C), y2);
                    }
                }
            } else {
                let (x, y) = (s16(h, 0x0879_7CD8 + 2 * k as u32), s16(h, 0x0879_7CE0 + 2 * k as u32));
                blit(h, 0xDD, x.wrapping_sub(1), y.wrapping_sub(1));
                if on_crew {
                    if st.profile.is_locked(v + crew + 0x127) == 0 {
                        let pic = s16(h, item + 2);
                        alt(h, pic, x, y);
                    } else {
                        alt(h, 0x81, x, y);
                        let s = number_arg(st, h, v, false);
                        text(h, 0xC, s, x.wrapping_add(0x30), y, u32::MAX, 0);
                    }
                } else {
                    let pic = s16(h, item + 2);
                    alt(h, pic, x, y);
                    let screen = st.g.screen;
                    if h.new_mark(st, screen, v as u32) != 0 {
                        alt(h, 0xB2, x.wrapping_add(0x20), y);
                    }
                }
            }
        }
    }
    if st.g.message_box < 0 {
        let y = s16(h, 0x0879_7CE0).wrapping_add(arrow_y);
        blit(h, if st.profile.repeats[0] < 1 { 0xA7 } else { 0xA8 }, 3, y);
        blit(h, if st.profile.repeats[1] < 1 { 0xA9 } else { 0xAA }, 0xDD, y);
    }
    if rom_u16(h.rom(), page + 0xC) == 0 {
        text(h, 0xE, 0x197, 0xE6, 0x7C, u32::MAX, 0);
        text(h, 0xE, profile, 0xE6, 0x86, u32::MAX, 8);
        if st.g.screen != 0x1C && st.g.screen != 1 {
            let s = number_arg(st, h, st.profile.cash, true);
            text(h, 0xE, 0x196, 0x30, 0x7C, u32::MAX, 0);
            text(h, 0xE, s, 0x30, 0x86, u32::MAX, 8);
        }
    }
    let mut left = s16(h, page + 2);
    let mut right = s16(h, page + 4);
    let screen = st.g.screen;
    let cursor = cursor(st, slot) as i32;
    let state = if screen == 9 {
        if st.g.career != 0 {
            if st.profile.u_12 == 0 {
                right = u32::MAX;
            }
            let id = (cursor + 0x108) as u32;
            if h.unlock_owned(st, id) == 0 {
                let state = h.unlock_state(st, id);
                left = if state == 2 { 0x15A } else { 0x19C };
                let w = text(h, 0xE, 0xE3, 0x15, 0x85, 0, 0);
                let price = h.unlock_price(st, id) as i32;
                let mut s = number_text(&mut st.g.div_remainder, price);
                let price = h.unlock_price(st, id) as i32;
                thousands(st.g.language, &mut s, price);
                let s = h.text_arg(s);
                text(h, 0xE, s, w.wrapping_add(0x1D), 0x85, 0, 8);
            }
            let s = number_arg(st, h, st.profile.cash, true);
            let w = text(h, 0xE, s, 0xDA, 0x85, u32::MAX, 8);
            text(h, 0xE, 0x196, 0xD2u32.wrapping_sub(w), 0x85, u32::MAX, 0);
            None
        } else {
            left = 0x1F2;
            Some({
                let id = (cursor + 0x108) as u32;
                h.unlock_state(st, id)
            })
        }
    } else if (0x2D..=0x2E).contains(&screen) {
        Some({
            let id = (cursor + crew + 0x127) as u32;
            h.unlock_state(st, id)
        })
    } else {
        None
    };
    if state == Some(2) {
        left = 0x15A;
    }
    h.button_prompts(st, &[left, right, u32::MAX]);
    0
}

/// `quick_race_random` (`0x0812FFB0`): the Quick Play race's random settings (traffic, difficulty, laps, the wingman
/// among the 13 unlocked, the opponents; a sprint has one lap, a hunter as many laps as opponents). The rand draws
/// come in the game's order.
pub fn quick_race_random(st: &mut MenuState, h: &mut impl Host) {
    st.g.player_car = st.profile.career_car as i32 as u32;
    if let Some(b) = st.g.slot_bytes.get_mut(st.g.race_player as usize) {
        *b = st.g.start_slot as u8;
    }
    fn roll(st: &mut MenuState, h: &mut impl Host, n: u32) -> u32 {
        flow::rand_table(st, h).checked_rem(n).unwrap_or(0) // __umodsi3
    }
    st.g.u_562c = roll(st, h, 3) + 1;
    st.g.difficulty = roll(st, h, 3);
    st.g.u_6118 = roll(st, h, 0x14);
    st.g.u_0050 = flow::rand_table(st, h) & 1;
    st.g.u_580c = 0;
    let unlocked = (0..13).filter(|i| st.profile.is_locked(0x127 + i) == 0).count() as u32;
    let wingman = roll(st, h, unlocked);
    st.profile.wingman = wingman;
    if wingman != 0 {
        st.profile.wingman_side = (wingman as u8).wrapping_add(1) & 1;
    }
    st.g.opponents = if wingman == 0 { 3 } else { 2 };
    st.g.laps = roll(st, h, 6) + 1;
    st.g.traffic = roll(st, h, 3);
    match st.g.race_mode {
        3 => st.g.laps = 1,
        1 => st.g.laps = st.g.opponents,
        _ => {}
    }
}
