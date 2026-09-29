//! The race results screens (Career kind: 0xB the record and unlock messages, 0xC the standings) on typed state.
//! Results at `MenuGlobals::results`, the ranked copy the screens show at `ranked` (4 slots).

use nfsgba_sim::state::{MenuState, RaceResults};

use super::flow::{self, CARBON_PLAY_SOUND, Host, rom_u16, rom_u32};
use super::text::{frames_to_centiseconds, number_text, thousands, time_text};
use super::{
    INTRO_PAGE_SETUP, MENU_BLIT_MATERIAL, MENU_BLIT_MATERIAL_ALT, MENU_BUTTON_PROMPTS, TEXT_BOX, TEXT_MENU, WORLD,
};

const RESULT_PAGES: u32 = 0x087E_510C;
const CAREER_RACE_PAYOUT: u32 = 0x0812_EFE8; // (): `career::race_payout` models it; not wired to state here
/// Track slot per route (`0x7E49C4`).
const ROUTE_SLOTS: u32 = 0x087E_49C4;

/// `rank_results` (`0x0812E8E4`, key, descending) on the ranked results: [`nfsgba_formats::career::rank_results`]
/// over `opponents + 1` slots.
pub fn rank_results(st: &mut MenuState, key: u32, descending: bool) {
    let mut t = st.g.ranked.to_bytes();
    nfsgba_formats::career::rank_results(&mut t, st.g.opponents, key, descending);
    st.g.ranked = RaceResults::from_bytes(&t);
}

fn result_page(st: &MenuState) -> u32 {
    match st.g.screen {
        0xB => RESULT_PAGES,
        0xC => RESULT_PAGES + 0x18,
        _ => 0,
    }
}

/// `career_zone_enter` (`0x0812F204`). On 0xB (after a race): clears the record flags, zeroes the ranked slots past
/// the racers, copies the results (`FUN_0812EB70`), ranks an elimination by time then knock-out, clears the payout,
/// and outside race phases 6–8 checks the record time (`FUN_0812F180`, not in two-player career) and pays out; with
/// no new record it goes straight to the standings (0xC). Then the page's background.
pub fn enter(st: &mut MenuState, h: &mut impl Host) -> u32 {
    let mut page = result_page(st);
    if st.g.screen == 0xB {
        st.profile.record_flag = 0;
        st.profile.new_record = 0;
        st.profile.unlock_messages[0] = 0;
        for i in (st.g.opponents as i32 + 1).max(0)..4 {
            let i = i as usize;
            st.g.ranked.best_lap[i] = 0;
            st.g.ranked.finish[i] = 0;
            st.g.ranked.life[i] = 0;
            st.g.ranked.knocked[i] = 0;
        }
        // FUN_0812EB70: the results into the ranked table.
        st.g.ranked = st.g.results.clone();
        if st.g.race_mode == 1 {
            rank_results(st, 1, false);
            st.g.ranked.knocked[1..].fill(8);
            rank_results(st, 2, false);
        }
        st.profile.payout = 0;
        if st.g.race_outcome.wrapping_sub(6) > 2 {
            if (st.g.career as i32) < 2 {
                record_time(st, h.rom());
            }
            h.call(CAREER_RACE_PAYOUT, &[]);
        }
        if st.profile.record_flag == 0 {
            flow::poke_back(st, st.g.back_top, 0xC);
            st.g.screen = 0xC;
            page = RESULT_PAGES + 0x18;
        }
    }
    let (m, pal) = (
        rom_u16(h.rom(), page + 8) as i16 as i32 as u32,
        rom_u16(h.rom(), page + 10) as i16 as i32 as u32,
    );
    h.scene_setup(st, m, pal, 0xFFFF);
    1
}

/// `FUN_0812F180`: a new track record (profile per track, u16 frames) when the player's best lap beats it in a
/// finished race of a known mode; sets the record flags.
fn record_time(st: &mut MenuState, rom: &[u8]) {
    let mut t = rom_u32(rom, ROUTE_SLOTS.wrapping_add(st.g.route.wrapping_mul(4))) as i32;
    if t > 0xB {
        t -= 0xC;
    }
    let old = st.profile.records.get(t as usize).map_or(0, |&r| r as i32);
    let mode = st.g.race_mode as i32;
    if (0..4).contains(&mode) {
        rank_results(st, 2, false);
        if st.g.race_outcome != 8 && st.g.ranked.knocked[0] != 8 && (st.g.ranked.best_lap[0] as i32) < old {
            if let Some(r) = st.profile.records.get_mut(t as usize) {
                *r = st.g.ranked.best_lap[0] as u16;
            }
            st.profile.record_flag = 1;
            st.profile.new_record = 1;
        }
    }
}

/// `career_zone_update` (`0x0812F364`): A on 0xB clears the record flag and, with no unlock message left, goes to
/// the standings (0xC); A on 0xC ranks by position and returns to the career menu (screen 6), dropping the back
/// stack to below the first 0xC.
pub fn update(st: &mut MenuState, h: &mut impl Host) -> u32 {
    if st.g.screen == 0xB {
        if st.g.keys == 1 {
            h.call(CARBON_PLAY_SOUND, &[2, 1]);
            if st.profile.new_record != 0 {
                st.profile.new_record = 0;
                st.profile.repeats[4] = 0;
                st.g.keys = 0;
                if st.profile.unlock_messages[0] != 0 {
                    return 1;
                }
            }
            st.g.back_top = st.g.back_top.wrapping_sub(1);
            flow::goto_screen(st, h, 0xC);
        }
    } else if st.g.keys == 1 {
        h.call(CARBON_PLAY_SOUND, &[2, 1]);
        rank_results(st, 2, false);
        if st.g.screen == 0xC {
            let top = st.g.back_top as i32;
            let mut i = 0;
            let mut found = false;
            if top > 0 {
                while flow::peek_back(st, i as usize) != 0xC {
                    i += 1;
                    if st.g.back_top as i32 <= i {
                        break;
                    }
                }
                found = (st.g.back_top as i32) > i;
            }
            if found {
                st.g.back_top = i as u8 as i8;
            }
            st.g.back_top = st.g.back_top.wrapping_sub(1);
            flow::goto_screen(st, h, 6);
            st.g.screen_changed = 1;
        }
    }
    1
}

/// A racer's name for the standings: the player's (profile), a boss's (`0x7E4954[id − 0x10]`) or an opponent's
/// (`id + 0xA4` above 0x3F).
fn racer_name(st: &MenuState, rom: &[u8], id: u8) -> u32 {
    match id {
        0 => st.g.profile.addr,
        0x40.. => id as u32 + 0xA4,
        _ => rom_u16(rom, 0x087E_4954_u32.wrapping_add(((id as i32 - 0x10) * 2) as u32)) as u32,
    }
}

fn text(h: &mut impl Host, font: u32, key: u32, x: u32, y: u32, a: u32, c: u32) {
    h.call(TEXT_MENU, &[font, key, x, y, a, c]);
}

fn alt(h: &mut impl Host, m: u32, x: u32, y: u32) {
    h.call(MENU_BLIT_MATERIAL_ALT, &[WORLD, m, x, y]);
}

fn prompts(h: &mut impl Host, page: u32) {
    let (l, r) = (
        rom_u16(h.rom(), page + 4) as i16 as i32 as u32,
        rom_u16(h.rom(), page + 6) as i16 as i32 as u32,
    );
    h.call(MENU_BUTTON_PROMPTS, &[l, r, u32::MAX]);
}

/// The text of a time in the standings.
fn time_arg(st: &mut MenuState, h: &mut impl Host, frames: u32) -> u32 {
    let s = time_text(
        &mut st.g.div_remainder,
        st.g.language,
        frames_to_centiseconds(frames as i32),
    );
    h.text_arg(s)
}

/// `career_zone_draw` (`0x0812F450`). 0xB: the new record (track, best lap) or the unlock messages
/// (`FUN_0812EC68`). 0xC: the standings per race mode (hunter: life; circuit and elimination: finish time and best
/// lap, dashes when knocked out; sprint: finish time), the payout, the prompts.
pub fn draw(st: &mut MenuState, h: &mut impl Host) -> u32 {
    let page = result_page(st);
    h.call(INTRO_PAGE_SETUP, &[st.g.unpack_buffer]);
    if st.g.screen == 0xB {
        if st.profile.new_record == 0 {
            if st.profile.unlock_messages[0] == 0 {
                return 0;
            }
            unlock_messages(st, h);
            prompts(h, page);
            return 0;
        }
        let slot = rom_u32(h.rom(), ROUTE_SLOTS.wrapping_add(st.g.route.wrapping_mul(4)));
        rank_results(st, 2, false);
        h.call(MENU_BLIT_MATERIAL, &[WORLD, 6, 0x20, 0x1F]);
        text(h, 0xD, 0x165, 0x78, 0x3B, 1, 8);
        let name = rom_u16(h.rom(), 0x087E_4A70_u32.wrapping_add(slot.wrapping_mul(4))) as u32;
        text(h, 0xD, name, 0x78, 0x48, 1, 8);
        let arg = time_arg(st, h, st.g.ranked.best_lap[0]);
        text(h, 0xD, arg, 0x78, 0x55, 1, 0);
        h.call(MENU_BLIT_MATERIAL, &[WORLD, 0x9C, 100, 0x61]);
        text(h, 0xD, 0xD7, 0x78, 100, 0, 0);
        return 0;
    }
    let heading = rom_u32(h.rom(), page);
    if heading != u32::MAX {
        text(h, 0xC, heading, 0xEC, 2, u32::MAX, 0);
    }
    match st.g.race_mode as i32 {
        2 => rank_results(st, 4, true),
        0 | 1 | 3 => rank_results(st, 1, false),
        _ => {}
    }
    let racers = st.g.opponents as usize;
    let at = |a: &[u8; 4], k: usize| a.get(k).copied().unwrap_or(0);
    let word = |a: &[u32; 4], k: usize| a.get(k).copied().unwrap_or(0);
    match st.g.race_mode as i32 {
        2 => {
            alt(h, 0xD6, 0x88, 0x17);
            text(h, 0xE, 0x157, 0xA8, 0x1B, 1, 0);
            for k in 0..=racers {
                let y = 0x10 * k as u32;
                alt(h, 0xD2, 0x28, y + 0x27);
                alt(h, 0xD3, 0x88, y + 0x27);
                let name = racer_name(st, h.rom(), at(&st.g.ranked.ids, k));
                text(h, 0xE, name, 0x30, y + 0x2B, 0, 8);
                let v = (word(&st.g.ranked.life, k).wrapping_mul(100) >> 0x13) as i32;
                let mut s = number_text(&mut st.g.div_remainder, v);
                thousands(st.g.language, &mut s, v);
                let arg = h.text_arg(s);
                text(h, 0xE, arg, 0xA8, y + 0x2B, 1, 8);
            }
        }
        0 | 1 => {
            alt(h, 0xD6, 0x68, 0x17);
            alt(h, 0xD6, 0xA8, 0x17);
            text(h, 0xE, 0x2CB, 0x88, 0x1B, 1, 0);
            text(h, 0xE, 0x94, 200, 0x1B, 1, 0);
            for k in 0..=racers {
                let y = 0x10 * k as u32;
                alt(h, 0xD2, 8, y + 0x27);
                alt(h, 0xD3, 0x68, y + 0x27);
                alt(h, 0xD3, 0xA8, y + 0x27);
                let name = racer_name(st, h.rom(), at(&st.g.ranked.ids, k));
                text(h, 0xE, name, 0x10, y + 0x2B, 0, 8);
                if at(&st.g.ranked.knocked, k) as i8 == 8 {
                    text(h, 0xE, 0x155, 0x88, y + 0x2B, 1, 8);
                    let mut s = b"--:--:--".to_vec();
                    match st.g.language {
                        3 => s[5] = b',',
                        1 | 2 | 4 => s[5] = b'.',
                        _ => {}
                    }
                    let arg = h.text_arg(s);
                    text(h, 0xE, arg, 200, y + 0x2B, 1, 8);
                } else {
                    let arg = time_arg(st, h, word(&st.g.ranked.finish, k));
                    text(h, 0xE, arg, 0x88, y + 0x2B, 1, 8);
                    let arg = time_arg(st, h, word(&st.g.ranked.best_lap, k));
                    text(h, 0xE, arg, 200, y + 0x2B, 1, 8);
                }
            }
        }
        3 => {
            alt(h, 0xD6, 0x88, 0x17);
            text(h, 0xE, 0x2CB, 0xA8, 0x1B, 1, 0);
            for k in 0..=racers {
                let y = 0x10 * k as u32;
                alt(h, 0xD2, 0x28, y + 0x27);
                alt(h, 0xD3, 0x88, y + 0x27);
                let name = racer_name(st, h.rom(), at(&st.g.ranked.ids, k));
                text(h, 0xE, name, 0x30, y + 0x2B, 0, 8);
                if at(&st.g.ranked.knocked, k) as i8 == 8 {
                    text(h, 0xE, 0x155, 0xA8, y + 0x2B, 1, 8);
                } else {
                    let arg = time_arg(st, h, word(&st.g.ranked.finish, k));
                    text(h, 0xE, arg, 0xA8, y + 0x2B, 1, 8);
                }
            }
        }
        _ => {}
    }
    if st.profile.payout != 0 {
        alt(h, 0xD2, 0x28, 0x70);
        alt(h, 0xD3, 0x88, 0x70);
        text(h, 0xE, 0x196, 0x2C, 0x74, 0, 8);
        let v = st.profile.payout as i32;
        let mut s = number_text(&mut st.g.div_remainder, v);
        thousands(st.g.language, &mut s, v);
        let arg = h.text_arg(s);
        text(h, 0xE, arg, 0xA8, 0x74, 1, 8);
    }
    rank_results(st, 2, false);
    prompts(h, page);
    0
}

/// `FUN_0812EC68`: the unlock messages after a race (heading 0xD6, then each text key of the profile's list, 0x31A
/// skipped, headings of new cars and districts 2 rows lower and highlighted).
fn unlock_messages(st: &MenuState, h: &mut impl Host) {
    h.call(TEXT_BOX, &[0xE, 0xD6, 0x78, 2, 0xF0, 2, 8]);
    let mut y = 0xD;
    let msg = |i: usize| st.profile.unlock_messages.get(i).copied().unwrap_or(0);
    if msg(0) == 0 {
        return;
    }
    let mut i = 0;
    loop {
        let key = msg(i) as u32;
        i += 1;
        if key != 0x31A {
            if matches!(key, 0x194 | 0x39A | 0xC6 | 0x3CF | 0xA3 | 0x3C1) {
                y += 2;
                h.call(TEXT_MENU, &[0xE, key, 0x78, y, 1, 8]);
            } else {
                h.call(TEXT_MENU, &[0xE, key, 0x78, y, 1, 0]);
            }
        }
        y += 0xB;
        if msg(i) == 0 {
            break;
        }
    }
}
