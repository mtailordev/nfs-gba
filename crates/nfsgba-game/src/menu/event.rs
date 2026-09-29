//! The career event screen (Event: screen 13, page `0x7E5090`) on typed state, with the career helpers it shares
//! (`event_status`, `zone_ladder_index`, `style_rating`, `career_event_to_globals`, `hint_due`).

use nfsgba_sim::state::{MenuProfile, MenuState};

use super::flow::{self, CARBON_PLAY_SOUND, Host, rom_u16};
use super::text::{frames_to_centiseconds, number_text, thousands, time_text};
use super::{INTRO_PAGE_SETUP, MENU_BLIT_MATERIAL, MENU_BUTTON_PROMPTS, TEXT_BOX, TEXT_MENU, WORLD};

const EVENT_PAGE: u32 = 0x087E_5090;
/// Per zone: two boss-event numbers (u16 pairs) the hints and locks test.
const BOSS_EVENTS: u32 = 0x087E_4714;
/// The event table: 8 bytes per event (`+1` track, `+2` mode, `+6` reward).
const EVENT_TABLE: u32 = 0x087E_4744;

fn r8(h: &impl Host, a: u32) -> u8 {
    h.rom()[(a & 0x1FF_FFFF) as usize]
}
fn r16(h: &impl Host, a: u32) -> u16 {
    rom_u16(h.rom(), a)
}

/// `event_status` (`0x08135D4C`): the 2-bit status of career event `n` (1 won, 2 second, 3 not done).
pub fn event_status(p: &MenuProfile, n: i32) -> u32 {
    p.events.get((n as usize) >> 2).map_or(0, |b| {
        nfsgba_formats::career::event_status(&[*b], (n as usize) & 3) as u32
    })
}

/// `zone_ladder_index` (`0x0812FC34`): zone·12 plus the zone's events with status 1 or 2 (6 events in zone 5).
pub fn zone_ladder_index(p: &MenuProfile, zone: i32) -> i32 {
    let n = if zone == 5 { 6 } else { 12 };
    zone * 12
        + (0..n)
            .filter(|&e| matches!(event_status(p, zone * 12 + e), 1 | 2))
            .count() as i32
}

/// `style_rating` (`0x0812C30C`) of the career car ([`nfsgba_formats::career::style_rating`]).
pub fn career_style_rating(rom: &[u8], p: &MenuProfile) -> i32 {
    let car = p.career_car as i32;
    let record = p.car_records.get(car as usize).copied().unwrap_or([0; 17]);
    nfsgba_formats::career::style_rating(rom, car as usize, &record)
}

fn cursor(st: &MenuState) -> i8 {
    st.profile
        .event_cursors
        .get(st.profile.zone as usize)
        .map_or(0, |&c| c as i8)
}

fn add_cursor(st: &mut MenuState, v: i32) {
    let new = (cursor(st) as i32 + v) as u8;
    set_cursor(st, new);
}

fn set_cursor(st: &mut MenuState, v: u8) {
    if let Some(c) = st.profile.event_cursors.get_mut(st.profile.zone as usize) {
        *c = v;
    }
}

/// `career_event_to_globals` (`0x0812DA08`): the selected event ([`nfsgba_formats::career::events`]) into the race
/// globals, and its slot into the profile.
pub fn career_event_to_globals(st: &mut MenuState, rom: &[u8]) {
    let zone = st.profile.zone as i32;
    let slot = cursor(st) as i32;
    let e = nfsgba_formats::career::events(rom)[(zone * 12 + slot) as usize];
    let g = &mut st.g;
    g.skill = e.skill as u32;
    g.difficulty = e.difficulty() as u32;
    g.laps = e.laps as u32;
    g.traffic = e.traffic as u32;
    g.race_mode = e.mode as u32;
    g.reverse = e.reverse as u32;
    g.route = rom_u16(rom, 0x087E_4A72 + 4 * e.track_slot() as u32) as u32;
    g.player_car = st.profile.career_car as i32 as u32;
    if let Some(b) = g.slot_bytes.get_mut(g.race_player as usize) {
        *b = g.start_slot as u8;
    }
    st.profile.event_slot = cursor(st) as u8;
}

/// `FUN_0812CF48` (screen, event): 1 when a career hint is due before going to `screen` (the hint screen 0x28):
/// the hints seen so far pick the next one, per zone, by the screen it comes before and conditions on the event,
/// the car record bits (unlock bytes `+0x450…+0x453`) and the race mode. Clears the hint flag.
pub fn hint_due(st: &mut MenuState, rom: &[u8], screen: i32, event: i32) -> u32 {
    let n = st.profile.hints_a as i32 + st.profile.hints_b as i32;
    st.profile.hint_flag = 0;
    if st.g.career == 0 {
        return 0;
    }
    let (p, z) = (&st.profile, st.profile.zone);
    let bit = |off: usize, b: u32| (p.unlock_byte(off + 35) as u32 >> b) & 1 != 0;
    let boss = |off: u32, k: u32| {
        p.event_slot as u32 == (rom_u16(rom, BOSS_EVENTS + off) as i16 as i32 as u32).wrapping_sub(k)
    };
    let is = |k: i32, s: i32| n == k && screen == s;
    let mode = st.g.race_mode & 0xFF;
    let fine = (screen != 10 || st.g.mode_bits.checked_shr(mode).unwrap_or(0) & 1 != 0)
        && (z != 0
            || !(is(0, 3)
                || is(1, 0xD)
                || is(2, 0xD)
                || is(3, 6)
                || (p.u_12 == 0 && screen == 0xD)
                || (is(4, 6) && bit(0, 5))))
        && (z != 1
            || !(is(5, 0x2D)
                || is(6, 6)
                || (is(7, 0x2D) && event_status(p, event) == 3)
                || (is(8, 0x2D) && boss(4, 0xC))
                || (is(9, 6) && bit(0, 1))))
        && (z != 2 || !(is(10, 0xD) || (is(0xB, 0x2D) && boss(10, 0x18)) || (is(0xC, 6) && bit(0, 2))))
        && (z != 3
            || !(is(0xD, 0x2D)
                || is(0xE, 6)
                || is(0xF, 0x2D)
                || (is(0x10, 0x2D) && boss(0xC, 0x24))
                || (is(0x11, 0x2D) && boss(0xE, 0x24))
                || (is(0x12, 6) && bit(0, 3))))
        && (z != 4
            || !((is(0x13, 6) && bit(1, 1)) || (is(0x14, 0x2D) && boss(0x12, 0x30)) || (is(0x15, 6) && bit(0, 4))))
        && (z != 5 || !(is(0x16, 0xD) || (is(0x17, 6) && bit(3, 4))));
    (!fine) as u32
}

fn page(st: &MenuState) -> u32 {
    if st.g.screen == 0xD { EVENT_PAGE } else { 0 }
}

/// `career_event_enter` (`0x0812E308`): the page's background (`+6`, palette `+8`); career mode on.
pub fn enter(st: &mut MenuState, h: &mut impl Host) -> u32 {
    let page = page(st);
    let (m, p) = (
        r16(h, page + 6) as i16 as i32 as u32,
        r16(h, page + 8) as i16 as i32 as u32,
    );
    h.scene_setup(st, m, p, 0xFFFF);
    st.g.career = 1;
    0 // returns nothing (r0 is the last call's)
}

/// `career_event_update` (`0x0812DAFC`): the cursor over the zone's 12 events (6 in zone 5) in rows of 3;
/// SELECT opens the district map (0xE, map mode 1); A on an open event (boss races need their unlock, events past
/// 0x3C their predecessor won) sets the race up, then shows a due hint (0x28) or goes on.
pub fn update(st: &mut MenuState, h: &mut impl Host) -> u32 {
    let page = page(st);
    let items = flow::rom_u32(h.rom(), page + 0x10);
    let count: i32 = if st.profile.zone == 5 { 6 } else { 12 };
    if st.g.keys & 0x200 != 0 {
        h.call(CARBON_PLAY_SOUND, &[4, 1]);
        flow::goto_screen(st, h, 0xE);
        st.profile.map_mode = 1;
        st.g.screen_changed = 1;
    }
    if st.g.keys & 0x20 != 0 {
        if cursor(st) == 0 {
            set_cursor(st, count as u8);
        }
        add_cursor(st, -1);
    }
    if st.g.keys & 0x10 != 0 {
        add_cursor(st, 1);
        if count <= cursor(st) as i32 {
            set_cursor(st, 0);
        }
    }
    if st.g.keys & 0x40 != 0 {
        add_cursor(st, -3);
        if cursor(st) < 0 {
            add_cursor(st, count);
        }
    }
    if st.g.keys & 0x80 != 0 {
        add_cursor(st, 3);
        if count - 1 < cursor(st) as i32 {
            add_cursor(st, -count);
        }
    }
    if st.g.keys & 0xF0 != 0 {
        h.call(CARBON_PLAY_SOUND, &[4, 1]);
        st.g.screen_changed = 1;
    }
    if st.g.keys != 1 {
        return 1;
    }
    let next = r16(h, items + 6) as i16 as i32;
    if next != -1 {
        let zone = st.profile.zone as u32;
        let event = (zone * 12) as i32 + cursor(st) as i32;
        let locked = (r16(h, BOSS_EVENTS + zone * 4) as i16 as i32 == event
            && st.profile.is_locked(zone as i32 + 0x122) != 0)
            || (r16(h, BOSS_EVENTS + (zone * 2 + 1) * 2) as i16 as i32 == event
                && st.profile.is_locked(zone as i32 + 0x11D) != 0)
            || (0x3C < event && event_status(&st.profile, event - 1) != 1);
        if locked {
            h.call(CARBON_PLAY_SOUND, &[0x27, 1]);
            return 1;
        }
        st.g.u_5658 = [0; 4];
        career_event_to_globals(st, h.rom());
        if hint_due(st, h.rom(), 0x2D, event) != 0 {
            h.call(CARBON_PLAY_SOUND, &[2, 1]);
            flow::goto_screen(st, h, 0x28);
            st.g.screen_changed = 1;
            return 1;
        }
        flow::goto_screen(st, h, next);
        st.g.screen_changed = 1;
    }
    h.call(CARBON_PLAY_SOUND, &[2, 1]);
    1
}

fn blit(h: &mut impl Host, m: u32, x: i32, y: i32) {
    h.call(MENU_BLIT_MATERIAL, &[WORLD, m, x as u32, y as u32]);
}

/// `career_event_screen` (`0x0812DD80`): the zone's event grid (4 rows of 3; 2 in zone 5) with the cursor, the
/// boss races' lock state, the mode icons, won and second marks, then the selected event's track, mode, reward
/// (the next win's, halved once won, times the car's style percentage) and record time.
pub fn draw<H: Host>(st: &mut MenuState, h: &mut H) -> u32 {
    let page = page(st);
    let zone = st.profile.zone as i32;
    let cursor = cursor(st) as u8 as i32;
    let event = zone * 12 + cursor;
    h.call(INTRO_PAGE_SETUP, &[st.g.unpack_buffer]);
    let neg1 = u32::MAX;
    h.call(TEXT_MENU, &[0xC, r16(h, page) as i16 as i32 as u32, 0xEC, 2, neg1, 0]);
    h.call(TEXT_MENU, &[0xC, zone as u32 + 0x3C3, 2, 2, 0, 0]);
    let (col, row);
    if st.profile.zone == 5 {
        for r in 0..2 {
            let y = r * 0x1C;
            for c in 0..3 {
                let (x9, x11, x7) = (8 + 0x28 * c, 0x10 + 0x28 * c, 7 + 0x28 * c);
                let e = zone * 12 + r * 3 + c;
                if cursor == r * 3 + c {
                    blit(h, 0x90, x9, y + 0x36);
                } else {
                    blit(h, 0x8F, x9, y + 0x36);
                    blit(h, 0x99, x7, y + 0x35);
                }
                if e != 0x3C && event_status(&st.profile, e - 1) != 1 {
                    blit(h, 0xCD, x11, y + 0x38);
                }
                if event_status(&st.profile, e) == 1 {
                    blit(h, 0xAB, x11, y + 0x39);
                }
            }
        }
        (col, row) = (cursor as u32 % 3, ((cursor as u32 / 3) & 0xFF) + 1);
    } else {
        for r in 0..4 {
            let y = r * 0x1C;
            for c in 0..3 {
                let (x9, x11, x7) = (8 + 0x28 * c, 0x10 + 0x28 * c, 7 + 0x28 * c);
                let zone = st.profile.zone as u32;
                let e = (zone * 12) as i32 + r * 3 + c;
                let boss = [
                    (r16(h, BOSS_EVENTS + zone * 4) as i16 as i32, 0x122),
                    (r16(h, BOSS_EVENTS + (zone * 2 + 1) * 2) as i16 as i32, 0x11D),
                ]
                .into_iter()
                .find(|&(b, _)| b == e);
                if let Some((_, unlock)) = boss {
                    if cursor == r * 3 + c {
                        blit(h, 0x90, x9, y + 0x18);
                    } else {
                        blit(h, 0x8F, x9, y + 0x18);
                        blit(h, 0x99, x7, y + 0x17);
                    }
                    if st.profile.is_locked(zone as i32 + unlock) == 0 {
                        if event_status(&st.profile, e) == 1 {
                            blit(h, 0xAB, x11, y + 0x1B);
                        }
                    } else {
                        blit(h, 0xCD, x11, y + 0x1A);
                    }
                } else {
                    let mode = r8(h, EVENT_TABLE + 8 * e as u32 + 2) as i8 as i32;
                    let icon = |h: &H, k: i32| r16(h, 0x087E_5078_u32.wrapping_add((k * 2) as u32)) as u32;
                    if cursor == r * 3 + c {
                        let m = icon(h, mode + 4);
                        blit(h, m, x9, y + 0x18);
                    } else {
                        let m = icon(h, mode);
                        blit(h, m, x9, y + 0x18);
                        blit(h, 0x99, x7, y + 0x17);
                    }
                    let status = event_status(&st.profile, e);
                    if status == 1 {
                        blit(h, 0xAB, x11, y + 0x1B);
                    }
                    if status == 2 {
                        blit(h, 0xAC, x11, y + 0x1B);
                    }
                }
            }
        }
        (col, row) = (cursor as u32 % 3, (cursor as u32 / 3) & 0xFF);
    }
    blit(h, 0x9B, ((col & 0xFF) * 0x28 + 3) as i32, (row * 0x1C + 0x12) as i32);
    h.call(TEXT_MENU, &[0xD, 0x2F1, 0xB4, 0x16, 1, 8]);
    let rec = EVENT_TABLE + 8 * event as u32;
    let track = r8(h, rec + 1) as i8 as i32;
    let name = r16(h, 0x087E_4A70_u32.wrapping_add((track * 4) as u32)) as u32;
    h.call(TEXT_BOX, &[0xD, name, 0xB4, 0x22, 0x70, 2, 0]);
    h.call(TEXT_MENU, &[0xD, 0x131, 0xB4, 0x3E, 1, 8]);
    let mode = r8(h, rec + 2) as i8 as i32;
    let mode_name = r16(h, 0x087E_5070_u32.wrapping_add((mode * 2) as u32)) as u32;
    h.call(TEXT_MENU, &[0xD, mode_name, 0xB4, 0x4A, 1, 0]);
    h.call(TEXT_MENU, &[0xD, 0x196, 0xB4, 0x5A, 1, 8]);
    let ladder = zone_ladder_index(&st.profile, st.profile.zone as i32);
    let reward = if event_status(&st.profile, event) == 3 {
        r16(h, EVENT_TABLE.wrapping_add((ladder * 8 + 6) as u32)) as i16 as i32
    } else {
        (r16(h, EVENT_TABLE.wrapping_add(((ladder - 1) * 8 + 6) as u32)) as i16 as i32) >> 1
    };
    let pct = nfsgba_formats::career::reward_percent(career_style_rating(h.rom(), &st.profile));
    let cash = nfsgba_fixed::div(pct * reward, 100);
    let mut s = number_text(&mut st.g.div_remainder, cash);
    thousands(st.g.language, &mut s, cash);
    let arg = h.text_arg(s);
    h.call(TEXT_MENU, &[0xE, arg, 0xB4, 0x66, 1, 0]);
    h.call(TEXT_MENU, &[0xD, 0x1B2, 0xB4, 0x76, 1, 8]);
    let t = if track > 0xB { track - 0xC } else { track };
    let record = st.profile.records.get(t as usize).map_or(0, |&r| r as i32);
    let s = time_text(&mut st.g.div_remainder, st.g.language, frames_to_centiseconds(record));
    let arg = h.text_arg(s);
    h.call(TEXT_MENU, &[0xE, arg, 0xB4, 0x82, 1, 0]);
    let (l, r) = (
        r16(h, page + 2) as i16 as i32 as u32,
        r16(h, page + 4) as i16 as i32 as u32,
    );
    h.call(MENU_BUTTON_PROMPTS, &[l, r, 0x1F5]);
    0
}
