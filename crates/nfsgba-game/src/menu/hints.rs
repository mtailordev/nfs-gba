//! The hint and story pages (Kind38: 0x26 hints, 0x27 the wingman's introduction, 0x2B mode hints; 0x28 save,
//! 0x29 wait, 0x2A clear the frame buffers) on typed state. Page records (0xC bytes: `+0` enter script, `+4` draw
//! script, `+8` entries of 10 bytes: `+0`/`+2` background, `+4` line, `+6` text, `+8` action). The frame buffer
//! fills and the world's palette and page pointers stay behind [`Host`] (FIDELITY U7).

use nfsgba_sim::state::MenuState;

use super::flow::{self, CARBON_PLAY_SOUND, Host, rom_u16, rom_u32};
use super::{
    CARBON_PLAY_MUSIC, CARBON_STOP_SOUND, FILL_RECT, INTRO_PAGE_SETUP, MENU_BLIT_MATERIAL, MENU_BUTTON_PROMPTS,
    SAVE_WRITE_PROFILE, SND_STOP_MUSIC, TEXT_BOX, TEXT_MENU, VBLANK_INTR_WAIT, WORLD,
};

fn s16(h: &impl Host, a: u32) -> u32 {
    rom_u16(h.rom(), a) as i16 as i32 as u32
}

fn hint_record(st: &MenuState) -> u32 {
    match st.g.screen {
        0x26 => 0x087E_8570 + 0xC * st.profile.hints_a as u32,
        0x27 => 0x087E_8534,
        _ => 0x087E_8540_u32.wrapping_add(st.g.race_mode.wrapping_mul(0xC)),
    }
}

/// The entry of the record the hint flag (`+0x1FA`) points at.
fn entry(st: &MenuState, h: &impl Host, rec: u32) -> u32 {
    rom_u32(h.rom(), rec + 8) + 10 * st.profile.hint_flag as u32
}

/// Word `i` of a page script.
fn w(h: &impl Host, script: u32, i: u32) -> u32 {
    rom_u16(h.rom(), script + 2 * i) as u32
}

/// `FUN_0813609C`: no music (`0x0300003C` −1).
fn stop_music(st: &mut MenuState, h: &mut impl Host) {
    st.g.music_id = u32::MAX;
    h.call(SND_STOP_MUSIC, &[]);
}

/// `kind38_enter` (`0x08134DF0`): the entry's background (hints: entry `+0x1FA`) and the record's enter script.
pub fn enter(st: &mut MenuState, h: &mut impl Host) -> u32 {
    let rec = hint_record(st);
    let mut entry = rom_u32(h.rom(), rec + 8);
    if st.g.screen == 0x26 {
        entry += 10 * st.profile.hint_flag as u32;
    }
    let first = st.g.screen_entered == 0 && st.g.screen != 5;
    let (m, p) = (s16(h, entry), s16(h, entry + 2));
    h.scene_setup_ab(st, first, m, p, 0xFFFF);
    page_script(st, h, rom_u32(h.rom(), rec));
    1
}

/// `FUN_081348E8` (script): runs the section of a page script whose id is the current entry (hints: profile
/// `+0x1FA`; 0x27: 0; else the race mode). A script is u16 sections `[id, commands…, 0xFFFF]`; commands:
/// 0xFFF5 stop sound; 0xFFF6 wait and play a sound; 0xFFF7 play a sound; 0xFFF8 stop a sound once the wait is over;
/// 0xFFF9 stop the music; 0xFFFA play music; 0xFFFB a flash (palette entries 0xC0.. and a filled rectangle); 0xFFFC
/// a picture on the map grid (profile `+0x254`); 0xFFFD a portrait and its 64-colour palette.
fn page_script(st: &mut MenuState, h: &mut impl Host, script: u32) {
    let second = st.g.second_palette;
    let mut src = h.world_palette();
    let sel = match st.g.screen {
        0x26 => st.profile.hint_flag as u32,
        0x27 => 0,
        _ => st.g.race_mode,
    };
    let (mut i, mut at) = (1u32, 0u32); // `at`: the section header being looked at
    while (w(h, script, at) as i32) < sel as i32 {
        i += 1;
        let mut end = at + 2;
        if w(h, script, at + 1) != 0xFFFF {
            let mut k = i;
            loop {
                let v = w(h, script, k);
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
    if w(h, script, i - 1) != sel {
        return;
    }
    let mut c = w(h, script, i);
    while c != 0xFFFF {
        match c.wrapping_sub(0xFFF5) {
            0 => {
                let id = w(h, script, i + 1);
                h.call(CARBON_STOP_SOUND, &[id]);
                i += 2;
            }
            1 | 2 => {
                if c == 0xFFF6 {
                    st.g.page_wait = w(h, script, i + 1).wrapping_add(st.g.flash) as i32;
                    i += 1;
                }
                let id = w(h, script, i + 1);
                h.call(CARBON_PLAY_SOUND, &[id, 1]);
                i += 2;
            }
            3 => {
                if st.g.page_wait != 0 && st.g.page_wait < st.g.flash as i32 {
                    st.g.page_wait = 0;
                    let id = w(h, script, i + 1);
                    h.call(CARBON_STOP_SOUND, &[id]);
                }
                i += 2;
            }
            4 => {
                stop_music(st, h);
                h.call(VBLANK_INTR_WAIT, &[]);
                i += 1;
            }
            5 => {
                let id = w(h, script, i + 1);
                h.call(CARBON_PLAY_MUSIC, &[id]);
                h.call(VBLANK_INTR_WAIT, &[]);
                h.call(VBLANK_INTR_WAIT, &[]);
                i += 2;
            }
            6 => {
                h.copy_mem(second, src, 0x180, 0x10);
                h.copy_mem(second + 0x180, 0x087E_6ED4, 0x18, 0x10);
                st.g.palette_dirty = 1;
                let (x, y) = (w(h, script, i + 2), w(h, script, i + 3));
                let rect = [x, y, x + 0x22, y + 0x16]
                    .iter()
                    .flat_map(|v| v.to_le_bytes())
                    .collect();
                let rect = h.text_arg(rect);
                let page = h.page_buffer();
                let colour = w(h, script, i + 1).wrapping_mul(0x0101_0101);
                h.call(FILL_RECT, &[rect, page, 0xF0, colour]);
                i += 4;
            }
            7 => {
                let grid = st.profile.map_grid;
                let x = (grid as u32 & 3) * 0x28 + w(h, script, i + 2);
                let y = ((grid as i16 as i32 >> 2) * 0x1E) as u32 + w(h, script, i + 3);
                let m = w(h, script, i + 1);
                h.call(MENU_BLIT_MATERIAL, &[WORLD, m, x, y]);
                i += 4;
            }
            8 => {
                h.copy_mem(second, src, 0x180, 0x10);
                let mut m = w(h, script, i + 1);
                if st.g.screen == 0x27 {
                    m = rom_u16(
                        h.rom(),
                        0x087E_78A0_u32.wrapping_add(st.profile.wingman.wrapping_mul(2)),
                    ) as u32;
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
                h.copy_mem(second + 0x180, src, 0x80, 0x10);
                st.g.palette_dirty = 1;
                h.call(MENU_BLIT_MATERIAL, &[WORLD, m, 0xA8, 0xC]);
                h.call(MENU_BLIT_MATERIAL, &[WORLD, 0xDB, 0xA0, 2]);
                i += 2;
            }
            // NOT 1:1 (N1): an unknown command makes the game loop forever.
            _ => panic!("page script {script:#x}: unknown command {c:#x}"),
        }
        c = w(h, script, i);
    }
}

/// `FUN_08134CC0` (tutorial): sets up one of the three tutorial races (two-player career mode 2, the Cobalt with
/// fixed upgrades, route 1 or 3), picks its opponents and counts the hint.
fn tutorial_race(st: &mut MenuState, h: &mut impl Host, which: u32) {
    st.g.career = 2;
    st.g.skill = 0x50;
    st.g.difficulty = 1;
    st.g.traffic = 0;
    st.g.reverse = 0;
    st.profile.career_car = 5;
    // Car 5's record: 0x55.. of the records.
    let rec = &mut st.profile.car_records[5];
    rec[7..].fill(0x7F);
    rec[..5].fill(2);
    rec[6] = 9;
    if let Some(b) = st.g.slot_bytes.get_mut(st.g.race_player as usize) {
        *b = st.g.start_slot as u8;
    }
    let setup = match which {
        0 => Some((1, 0, 1, 1)),
        1 => Some((1, 1, 2, 2)),
        2 => Some((3, 2, 2, 3)),
        _ => None,
    };
    if let Some((route, wingman, opponents, laps)) = setup {
        st.g.race_mode = 0;
        st.g.route = route;
        st.profile.wingman = wingman;
        st.g.opponents = opponents;
        st.g.laps = laps;
    }
    super::setup::career_opponents(st, h);
    st.profile.hints_b = st.profile.hints_b.wrapping_add(1);
}

/// `kind38_update` (`0x08134EB8`): the save/wait/clear transitions, the map grid (profile `+0x254`) on the zone-3
/// hint, A runs the entry's action (0x40 next page, 0x41 finish the hints: next zone, save, screen 3; 0x42 back;
/// 0x81 a tutorial race or a mode's first race; else a screen, 9 also resetting the cars and cash), B the page
/// before or back out.
pub fn update(st: &mut MenuState, h: &mut impl Host) -> u32 {
    let rec = hint_record(st);
    let entry = entry(st, h, rec);
    match st.g.screen {
        0x2A => {
            if st.g.fade == 0 {
                h.clear_frame_buffers();
                st.g.screen = 0x26;
                st.g.fade = 0x10;
                enter(st, h);
            }
            return 1;
        }
        0x29 => {
            if st.g.page_wait < st.g.flash as i32 {
                st.g.screen = 0x2A;
                st.g.fade = -0x10;
            }
            return 1;
        }
        0x28 => {
            if st.g.fade == 0 {
                if st.profile.hints_b != 0 {
                    st.profile.hints_a = st.profile.hints_b.wrapping_add(st.profile.hints_a);
                    st.profile.hints_b = 0;
                    h.call(SAVE_WRITE_PROFILE, &[st.g.save_buffer]);
                }
                stop_music(st, h);
                st.g.screen = 0x29;
                st.g.page_wait = st.g.flash.wrapping_add(0x78) as i32;
                if st.profile.hints_a == 0 {
                    h.scene_setup_ab(st, false, 5, 5, 0xFFFF);
                } else {
                    h.scene_setup_ab(st, false, 0xE2, 8, 0xFFFF);
                }
            }
            return 1;
        }
        _ => {}
    }
    if st.profile.hints_a == 2 && st.profile.hint_flag == 3 {
        let keys = st.g.keys;
        let grid = &mut st.profile.map_grid;
        if keys & 0x20 != 0 {
            if *grid == 0 {
                *grid = 0xC;
            }
            *grid = grid.wrapping_sub(1);
        }
        if keys & 0x10 != 0 {
            let v = grid.wrapping_add(1);
            *grid = if v as i16 > 0xB { 0 } else { v };
        }
        if keys & 0x40 != 0 {
            let old = *grid;
            let v = old.wrapping_sub(4);
            *grid = if (v as i16) < 0 { old.wrapping_add(8) } else { v };
        }
        if keys & 0x80 != 0 {
            let old = *grid;
            let v = old.wrapping_add(4);
            *grid = if v as i16 > 0xB { old.wrapping_sub(8) } else { v };
        }
        if keys & 0xF0 != 0 {
            h.call(CARBON_PLAY_SOUND, &[4, 1]);
            st.g.screen_changed = 1;
        }
    }
    if st.g.keys == 1 {
        h.call(CARBON_PLAY_SOUND, &[2, 1]);
        let action = rom_u16(h.rom(), entry + 8) as i16 as i32;
        match action {
            0x41 => {
                let p = &mut st.profile;
                p.hints_a = p.hints_a.wrapping_add(1).wrapping_add(p.hints_b);
                p.hints_b = 0;
                if p.zone_step != 0 {
                    if p.zone < 5 {
                        p.zone += 1;
                    }
                    p.zone_step = 0;
                }
                h.call(SAVE_WRITE_PROFILE, &[st.g.save_buffer]);
                h.call(CARBON_PLAY_MUSIC, &[0]);
                st.profile.back[0] = 0;
                st.g.screen = 3;
                st.g.back_top = 0;
                flow::enter_screen(st, h);
            }
            0x40 => {
                st.profile.hint_flag = st.profile.hint_flag.wrapping_add(1);
                flow::enter_screen(st, h);
            }
            0x42 => flow::menu_back(st, h),
            0x81 => {
                if st.g.screen == 0x26 {
                    tutorial_race(st, h, st.profile.hints_a as u32);
                } else {
                    st.g.back_top = 1;
                    let bit = 1u32.checked_shl(st.g.race_mode & 0xFF).unwrap_or(0);
                    st.g.mode_bits |= bit;
                }
                flow::goto_screen(st, h, action);
            }
            _ => {
                h.call(CARBON_PLAY_MUSIC, &[0]);
                if action as i16 == 9 {
                    for rec in st.profile.car_records.iter_mut() {
                        rec[7..].fill(0);
                        rec[..5].fill(0);
                    }
                    st.profile.cash = 1000;
                }
                st.profile.hints_b = st.profile.hints_b.wrapping_add(1);
                st.g.back_top = st.g.back_top.wrapping_sub(1);
                st.g.screen = 0xD;
                flow::goto_screen(st, h, action);
            }
        }
        st.g.screen_changed = 1;
    }
    if st.g.keys == 2 {
        if st.profile.hint_flag == 0 {
            h.call(CARBON_STOP_SOUND, &[3]);
            h.call(VBLANK_INTR_WAIT, &[]);
            h.call(CARBON_PLAY_SOUND, &[3, 1]);
            for _ in 0..8 {
                h.call(VBLANK_INTR_WAIT, &[]);
            }
            h.call(CARBON_PLAY_MUSIC, &[0]);
            st.g.keys = 0;
            st.profile.repeats[5] = 3;
            flow::menu_back(st, h);
        } else {
            h.call(CARBON_PLAY_SOUND, &[3, 1]);
            st.profile.hint_flag = st.profile.hint_flag.wrapping_sub(1);
            flow::enter_screen(st, h);
        }
        st.g.screen_changed = 1;
    }
    1
}

/// `kind38_draw` (`0x08135340`): 0x29 clears the page (or keeps the menu picture before the first hint) and says
/// "hint n"; the others run the record's draw script, the entry's text (0x27: the wingman's) and the prompts.
pub fn draw(st: &mut MenuState, h: &mut impl Host) -> u32 {
    let rec = hint_record(st);
    let entry = entry(st, h, rec);
    if st.g.screen == 0x29 {
        if st.profile.hints_a == 0 {
            h.call(INTRO_PAGE_SETUP, &[st.g.unpack_buffer]);
        } else {
            h.fill_page();
        }
        let key = st.profile.hints_a as u32 + 0x1D9;
        h.call(TEXT_MENU, &[0xE, key, 0x78, 0x46, 1, 0]);
        return 0;
    }
    h.call(INTRO_PAGE_SETUP, &[st.g.unpack_buffer]);
    page_script(st, h, rom_u32(h.rom(), rec + 4));
    let text = rom_u16(h.rom(), entry + 6) as i16 as i32;
    if text != -1 {
        if st.g.screen == 0x27 {
            let key = (text as u32).wrapping_add(st.profile.wingman);
            h.call(TEXT_BOX, &[0xE, key, 0x51, 4, 0xA6, 0x10, 8]);
        } else {
            let y = (rom_u16(h.rom(), entry + 4) as i16 as i32 * -0xB + 0x8E) as u32;
            h.call(TEXT_BOX, &[0xE, text as u32, 0x78, y, 0xF0, 0x10, 8]);
        }
    }
    h.call(MENU_BUTTON_PROMPTS, &[0x8D, 0x92, u32::MAX]);
    0
}
