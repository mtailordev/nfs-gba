//! The map screens (Kind7: 7 Quick Play circuits, 8 Quick Play sprints, 0xE the career district map, 0x11 the career
//! map) on typed state. Map state: view x/y (8.8), the cursor and "moved" (`MenuGlobals::map_*`).

use nfsgba_sim::state::MenuState;

use super::flow::{self, CARBON_PLAY_SOUND, Host, rom_u16, rom_u32};
use super::{MENU_BLIT_MATERIAL, TEXT_MENU, WORLD};

const MAP_DRAW: u32 = 0x0814_35C4; // (): scrolls the view towards the cursor and draws the map and its markers
/// Page records of the map screens (`0x7E50A4`, 0x14 bytes: `+2`/`+4` button prompts).
const MAP_PAGES: u32 = 0x087E_50A4;

const ZONE_TABLE: u32 = 0x087F_53BC; // pointers to the zone colour sets, by set number
const ZONE_SETS: u32 = 0x087F_5374; // set number per (map grid, zone row): 6 bytes per grid
const TINTS: u32 = 0x087F_45C4; // 16 colours per marked zone (0x20 bytes)

/// `map_zone_palettes` (`0x08143284`): the map's colours in the second base palette. Colours 0x40.. come from the
/// world palette; rows 0xA0.. hold the six zones' colours of the profile's map grid (a district that is still locked
/// keeps its grey on the career maps); the cursor's zone blinks (halved channels while the flash counter has bit 5),
/// and the marked zone's tint set is copied into row `0x40 + 16 * zone`.
pub fn zone_palettes(st: &mut MenuState, h: &mut impl Host) {
    let cursor = st.g.map_cursor as i32;
    let screen = st.g.screen;
    let world = h.world_palette();
    for k in 0..0xC0 {
        let c = h.peek16(world.wrapping_add(0x80 + 2 * k));
        h.set_second_colour(0x40 + k, c);
    }
    let grid = st.profile.map_grid as i16 as i32;
    for r in 0..6u32 {
        let set = h.rom()[((ZONE_SETS.wrapping_add((grid * 6) as u32) + r) & 0x1FF_FFFF) as usize] as u32;
        let ptr = rom_u32(h.rom(), ZONE_TABLE + 4 * set);
        copy_colours(h, 0xA0 + 16 * r, ptr + 0x20 * r);
    }
    if screen == 0x11 || screen == 0xE {
        let ptr = rom_u32(h.rom(), ZONE_TABLE.wrapping_add((grid * 4) as u32));
        for r in (0..5u32).filter(|&r| st.profile.is_locked(r as i32 + 0x118) == 0) {
            copy_colours(h, 0xA0 + 16 * r, ptr + 0x20 * r);
        }
        if st.profile.is_locked(0x134) == 0 {
            copy_colours(h, 0xA0 + 16 * 5, ptr + 0x20 * 5);
        }
    }
    if screen != 0x11 {
        let zone = if screen == 8 {
            nfsgba_fixed::div(cursor, 3)
        } else {
            cursor >> 1
        };
        if (0..6).contains(&zone) && st.g.flash & 0x20 != 0 {
            for k in 0..16 {
                let i = 0xA0 + 16 * zone as u32 + k;
                let c = h.second_colour(i) as u32;
                let half = |shift: u32| ((c >> shift & 0x1F) >> 1).min(0x1F);
                h.set_second_colour(i, ((c >> 11).min(0x1F) << 10 | half(5) << 5 | half(0)) as u16);
            }
        }
    }
    // The marked zone's tint set: by grid cell (sprints, career) or by pair (circuits).
    let cell = (
        nfsgba_fixed::div(cursor, 3),
        nfsgba_fixed::div(cursor, 3) * 5 + cursor % 3 + 2,
    );
    let pair = (cursor >> 1, (cursor >> 1) * 5 + (cursor & 1));
    let tint = match screen {
        8 => Some(cell),
        7 => Some(pair),
        0x11 if st.profile.map_mode == 3 => Some(pair),
        0x11 => Some(cell),
        _ => None,
    };
    if let Some((zone, set)) = tint {
        copy_colours(
            h,
            0x40u32.wrapping_add((zone * 16) as u32),
            TINTS.wrapping_add((set * 0x20) as u32),
        );
    }
    st.g.palette_dirty = 1;
}

/// 16 colours from ROM `src` to the second palette from colour `dst`.
fn copy_colours(h: &mut impl Host, dst: u32, src: u32) {
    for k in 0..16 {
        let c = rom_u16(h.rom(), src + 2 * k);
        h.set_second_colour(dst.wrapping_add(k), c);
    }
}

/// `kind7_enter` (`0x0812E80C`): background 0xDA, menu palette 7; the map state (`FUN_0814397C`).
pub fn enter(st: &mut MenuState, h: &mut impl Host) -> u32 {
    h.scene_setup(st, 0xDA, 7, 0xFFFF);
    // FUN_0814397C: the cursor starts on the district of the profile (×2) on screen 0xE, else 0; view (1, 0.75).
    st.g.map_cursor = if st.g.screen == 0xE {
        st.profile.zone.wrapping_shl(1) as i8
    } else {
        0
    };
    st.g.map_moved = 1;
    st.g.map_x = 0x100;
    st.g.map_y = 0xC0;
    h.zone_palettes(st);
    1
}

/// `FUN_081439C0` (cursor): moves the map cursor and marks it moved.
pub fn select(st: &mut MenuState, cursor: u8) {
    st.g.map_cursor = cursor as i8;
    st.g.map_moved = 1;
}

/// `kind7_update` (`0x0812E3D4`). Left/right move the cursor over 12 entries (18 on screen 8, and on 0x11 in mode
/// 2; step 2 on 0xE). A depends on the profile's map mode: 0 picks a track (screen 7: slot `0x7E472C[cursor]`, route
/// number `0x7E4A70`; 8: sprint `cursor + 0x18`) if its district (unlock `0x117 +` cursor/2 or /3) is open and goes to
/// screen 0x2D; 1 picks the district on 0xE; 2 sets mode 3; 3 goes back.
pub fn update(st: &mut MenuState, h: &mut impl Host) -> u32 {
    let step = if st.g.screen == 0xE {
        st.g.map_cursor &= !1;
        2
    } else {
        1
    };
    let s = st.g.screen;
    let count: i8 = if s == 8 || (s == 0x11 && st.profile.map_mode == 2) {
        18
    } else {
        12
    };
    if st.g.keys == 1 {
        let cursor = st.g.map_cursor as i32;
        match st.profile.map_mode {
            0 => {
                let district = if st.g.screen == 7 {
                    cursor >> 1
                } else {
                    nfsgba_fixed::div(cursor, 3) as i8 as i32
                };
                if st.profile.is_locked(district + 0x117) != 0 {
                    h.call(CARBON_PLAY_SOUND, &[0x27, 1]);
                    return 1; // the locked beep skips the left/right handling
                }
                let slot = if st.g.screen == 7 {
                    rom_u16(h.rom(), 0x087E_472C_u32.wrapping_add((cursor as u32).wrapping_mul(2))) as u32
                } else {
                    (cursor + 0x18) as u32
                };
                st.g.route = rom_u16(h.rom(), 0x087E_4A72_u32.wrapping_add(slot.wrapping_mul(4))) as u32;
                h.call(CARBON_PLAY_SOUND, &[2, 1]);
                flow::goto_screen(st, h, 0x2D);
            }
            1 => {
                if st.profile.is_locked((cursor >> 1) + 0x117) == 0 {
                    st.profile.zone = (cursor >> 1) as u8;
                    h.call(CARBON_PLAY_SOUND, &[2, 1]);
                    flow::menu_back(st, h);
                } else {
                    h.call(CARBON_PLAY_SOUND, &[0x27, 1]);
                }
            }
            2 => {
                st.profile.map_mode = 3;
                h.call(CARBON_PLAY_SOUND, &[2, 1]);
                select(st, 0);
            }
            3 => {
                h.call(CARBON_PLAY_SOUND, &[2, 1]);
                flow::menu_back(st, h);
            }
            _ => {}
        }
        st.g.screen_changed = 1;
    }
    if st.g.keys & 0x20 != 0 {
        h.call(CARBON_PLAY_SOUND, &[4, 1]);
        let v = (st.g.map_cursor as u8 as u32).wrapping_sub(step) as u8;
        select(st, if (v as i8) < 0 { (count - 1) as u8 } else { v });
    }
    if st.g.keys & 0x10 != 0 {
        h.call(CARBON_PLAY_SOUND, &[4, 1]);
        let v = (st.g.map_cursor as u8).wrapping_add(step as u8);
        select(st, if count <= v as i8 { 0 } else { v });
    }
    1
}

/// `kind7_draw` (`0x0812E5AC`): the map (`FUN_081435C4`), the heading (500; 0x1F5 on 0xE; 0x2E4 and a mode text on
/// 0x11), the arrows (lit while their key-repeat delay runs) and the prompts; A's prompt reads 0x15A over a
/// locked district.
pub fn draw(st: &mut MenuState, h: &mut impl Host) -> u32 {
    let early = st.g.map_cursor as i32;
    let page = MAP_PAGES
        + 0x14
            * match st.g.screen {
                8 => 1,
                0xE => 2,
                0x11 => 3,
                _ => 0,
            };
    h.call(MAP_DRAW, &[]);
    let mut left = rom_u16(h.rom(), page + 2) as i16 as i32 as u32;
    let neg1 = u32::MAX;
    let district = match st.g.screen {
        0xE => {
            h.call(TEXT_MENU, &[0xC, 0x1F5, 2, 2, 0, 0]);
            Some((early >> 1) + 0x117)
        }
        0x11 => {
            h.call(TEXT_MENU, &[0xC, 0x2E4, 2, 2, 0, 0]);
            let key = if st.profile.map_mode == 2 { 0x209 } else { 0xD3 };
            h.call(TEXT_MENU, &[0xC, key, 0xE6, 2, neg1, 0]);
            None
        }
        s @ (7 | 8) => {
            h.call(TEXT_MENU, &[0xC, 500, 2, 2, 0, 0]);
            let cursor = st.g.map_cursor as i32;
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
        && st.profile.is_locked(id) != 0
    {
        left = 0x15A;
    }
    if st.g.message_box < 0 {
        let m = if st.profile.repeats[0] >= 1 { 0xA8 } else { 0xA7 };
        h.call(MENU_BLIT_MATERIAL, &[WORLD, m, 1, 0x48]);
        let m = if st.profile.repeats[1] >= 1 { 0xAA } else { 0xA9 };
        h.call(MENU_BLIT_MATERIAL, &[WORLD, m, 0xDF, 0x48]);
        h.button_prompts(st, &[left, rom_u16(h.rom(), page + 4) as i16 as i32 as u32, neg1]);
    }
    0
}
