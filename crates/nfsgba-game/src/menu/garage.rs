//! The garage on typed state: the unlock and purchase rules (`0x0812C24C` … `0x0812D960`), the car stats, the "new"
//! marks of the list items, and the Kind18 screens (0x12 the career profile, 0x13 the part shop, 0x14 the car
//! upgrade pages). The car (`garage_load_car_atlas`, `_palette`, `garage_draw_car`) is [`super::car`] behind [`Host`].

use nfsgba_formats::unlock::{group as leader_group, id_adjust, table_index};
use nfsgba_sim::state::{MenuProfile, MenuState};

use super::event::{career_style_rating, event_status};
use super::flow::{self, CARBON_PLAY_SOUND, Host, message_box_open, rom_u16, rom_u32};
use super::text::{number_text, thousands};
use super::{INTRO_PAGE_SETUP, MENU_BLIT_MATERIAL, TEXT_MENU, WORLD};

const PAGES: u32 = 0x087E_6EA4; // Kind18 screen headers, 0x10 bytes: 0x13, 0x14, 0x12
const PART_BASE: u32 = 0x0879_7C68; // first unlock id of each part group (ids below 0x79), u16
const PART_BASE_B: u32 = 0x0879_7C7C; // the same for the groups above (ids from 0x79), u16
const PART_MASK: u32 = 0x0879_7C8A; // the 2-bit level masks, u16 at byte offset 0, 2, 4, 6
const SLOT_BYTES: u32 = 0x087F_0626;
const CAR_TABLE: u32 = 0x087F_0BD8; // 0x58 bytes per car: +0x52.. base stats
const STAT_WEIGHTS: u32 = 0x087E_46EC; // 10 parts x 4 signed bytes
const ITEMS: u32 = 0x0879_7CE8; // the list screens' new-mark ids, u16
const PRICES: u32 = 0x087E_503C;
const GROUP_PRICES: u32 = 0x087E_505A;
const UNLOCK_TABLE: u32 = 0x087E_4DE4;

fn s16(rom: &[u8], a: u32) -> i32 {
    rom_u16(rom, a) as i16 as i32
}

/// Byte `off` of car `car`'s 17-byte record (the game indexes flat; out of range reads 0).
fn rec(p: &MenuProfile, car: i32, off: i32) -> u8 {
    usize::try_from(car * 17 + off)
        .ok()
        .and_then(|i| p.car_records.as_flattened().get(i).copied())
        .unwrap_or(0)
}

fn rec_mut(p: &mut MenuProfile, car: i32, off: i32) -> Option<&mut u8> {
    usize::try_from(car * 17 + off)
        .ok()
        .and_then(|i| p.car_records.as_flattened_mut().get_mut(i))
}

/// Byte `i` of the unlock-bit-like area at profile `+0xF5` (the four bytes, then the car records).
fn f5(p: &MenuProfile, i: usize) -> u8 {
    p.field_f5
        .get(i)
        .or_else(|| p.car_records.as_flattened().get(i.wrapping_sub(4)))
        .copied()
        .unwrap_or(0)
}

fn f5_mut(p: &mut MenuProfile, i: usize) -> Option<&mut u8> {
    if i < 4 {
        p.field_f5.get_mut(i)
    } else {
        p.car_records.as_flattened_mut().get_mut(i - 4)
    }
}

/// `FUN_0812C24C` (`car_group`): the part group of an unlock id (−1 past the last).
#[allow(clippy::match_overlapping_arm)] // open ranges: the first arm that fits wins, as in the game
pub fn group(id: i32) -> i32 {
    const LOW: [i32; 9] = [0xD, 0x1A, 0x27, 0x34, 0x41, 0x4E, 0x5B, 0x68, 0x71];
    if id < 0x79 {
        return LOW.iter().position(|&l| id < l).map_or(9, |k| k as i32);
    }
    match id {
        ..0x89 => 0,
        ..0x90 => 1,
        ..0xA0 => 2,
        ..0xBF => 3,
        ..0xE0 => 4,
        ..0xF4 => 5,
        ..0x108 => 6,
        ..0x117 => 7,
        ..0x11D => 8,
        ..0x122 => 9,
        ..0x127 => 10,
        ..0x134 => 11,
        ..0x135 => 12,
        _ => -1,
    }
}

/// The 2-bit level slot of a part id below 0x79: `id − base` with the game's fix-ups.
fn slot(rom: &[u8], id: i32) -> u32 {
    let mut u = (id as u32).wrapping_sub(rom_u16(rom, PART_BASE + 2 * group(id) as u32) as u32);
    if id == 0x70 || u == 0xC {
        u = u.wrapping_add(1);
    }
    if id == 0x78 {
        u = u.wrapping_add(2);
    }
    u
}

/// `FUN_0812D69C`: the career car has exactly this part level installed.
pub fn installed(st: &MenuState, rom: &[u8], id: i32) -> bool {
    let (p, car, g) = (&st.profile, st.profile.career_car as i32, group(id));
    if id < 0x79 {
        let u = slot(rom, id);
        u & 3 == (rec(p, car, 7 + g) as u32) >> (((u & 0xFC) as i32) >> 1) & 3
    } else {
        let base = rom_u16(rom, PART_BASE_B.wrapping_add((g as u32).wrapping_mul(2))) as u32;
        u32::from(rec(p, car, g)) == (id as u32).wrapping_sub(base)
    }
}

/// `FUN_0812C5C4` (`unlock_owned`): the value the game returns (non-zero when owned).
pub fn owned(st: &MenuState, rom: &[u8], id: u32) -> u32 {
    let p = &st.profile;
    if id.wrapping_sub(0x108) < 15 {
        return u32::from(p.u_12) & (1 << (id - 0x108) & 0xFFFF);
    }
    let idx = table_index(rom, id);
    if idx == -1 {
        let g = group(id as i32);
        let special = matches!(g, 4..=6) || matches!(id, 0xC | 0x19 | 0x26 | 0x33 | 0x40 | 0x4D | 0x5A | 0x67 | 0x70);
        if !special && id != 0x78 {
            return 1;
        }
        return u32::from(installed(st, rom, id as i32));
    }
    let idx = idx as u32;
    let bit = if id < 0xA0 {
        let byte = p
            .car_extra
            .as_flattened()
            .get(st.g.player_car as usize * 15 + (idx >> 3) as usize)
            .copied()
            .unwrap_or(0);
        u32::from(byte) >> (idx & 7)
    } else {
        let adj = id_adjust(rom, id, st.g.player_car).wrapping_sub(0xA0);
        u32::from(f5(p, (adj >> 3) as usize)) >> (adj & 7)
    };
    bit & 1
}

/// `FUN_0812C75C`: marks an unlock id owned.
fn mark_owned(st: &mut MenuState, rom: &[u8], id: u32) {
    if id.wrapping_sub(0x108) < 15 {
        st.profile.u_12 |= 1 << (id - 0x108);
        return;
    }
    let idx = table_index(rom, id);
    if idx == -1 {
        return;
    }
    let idx = idx as u32;
    if id < 0xA0 {
        let i = st.g.player_car as usize * 15 + (idx >> 3) as usize;
        if let Some(b) = st.profile.car_extra.as_flattened_mut().get_mut(i) {
            *b |= 1 << (idx & 7);
        }
    } else {
        let adj = id_adjust(rom, id, st.g.player_car).wrapping_sub(0xA0);
        if let Some(b) = f5_mut(&mut st.profile, (adj >> 3) as usize) {
            *b |= 1 << (adj & 7);
        }
    }
}

/// `unlock_price` (`0x0812D960`).
pub fn price(st: &MenuState, rom: &[u8], id: u32) -> u32 {
    if id.wrapping_sub(0x108) < 15 {
        return s16(rom, PRICES + 2 * (id - 0x108)) as u32;
    }
    let id = id_adjust(rom, id, st.g.player_car);
    let idx = table_index(rom, id);
    if idx != -1 {
        return u32::from(rom_u16(rom, UNLOCK_TABLE + 4 * idx as u32 + 2));
    }
    let lead = leader_group(id);
    if lead != 0 {
        return s16(rom, GROUP_PRICES + 2 * lead) as u32;
    }
    let g = group(id as i32);
    if id > 0x78 && matches!(g, 4..=6) && !installed(st, rom, id as i32) {
        return match g {
            6 => 10,
            5 => 0x32,
            _ => 0x1E,
        };
    }
    0
}

/// `FUN_0812C81C` (`unlock_state`): 1 owned, 2 locked, 3 too expensive, else 0.
pub fn state(st: &MenuState, rom: &[u8], id: u32) -> u32 {
    let adj = id_adjust(rom, id, st.g.player_car);
    if st.profile.is_locked(adj as i32) != 0 {
        2
    } else if owned(st, rom, id) != 0 {
        1
    } else if (st.profile.cash as u32).wrapping_sub(price(st, rom, id)) as i32 >= 0 {
        0
    } else {
        3
    }
}

/// `FUN_0812C8A8` (`buy_unlock`): pays and installs.
pub fn buy(st: &mut MenuState, rom: &[u8], id: u32) {
    let car = st.profile.career_car as i32;
    if owned(st, rom, id) == 0 {
        mark_owned(st, rom, id);
        st.profile.cash = st.profile.cash.wrapping_sub(price(st, rom, id) as i32);
    }
    if id.wrapping_sub(0x108) < 15 {
        st.profile.career_car = (id as u8).wrapping_sub(8) as i8;
        st.g.player_car = st.profile.career_car as i32 as u32;
        return;
    }
    let g = group(id as i32);
    if id < 0x79 {
        let u = slot(rom, id as i32);
        let shift = ((u & 0xFC) as i32) >> 1;
        let mask = rom_u16(rom, PART_MASK + shift as u32) as u8;
        if let Some(b) = rec_mut(&mut st.profile, car, 7 + g) {
            *b = *b & mask | ((u & 3) << shift) as u8;
        }
    } else if let Some(b) = rec_mut(&mut st.profile, car, g) {
        *b = (id as u8).wrapping_sub(rom[(PART_BASE_B + 2 * g as u32) as usize & 0x1FF_FFFF]);
    }
}

fn unlock_bit(p: &MenuProfile, b: i32) -> bool {
    p.unlock_byte((b >> 3) as usize) >> (b & 7) & 1 != 0
}

/// `FUN_0812C984`: the id of a part the player can now unlock ("new"), else 0. Case 5 draws a random number.
#[allow(clippy::match_overlapping_arm)] // open ranges: the first arm that fits wins, as in the game
pub fn new_part(st: &mut MenuState, h: &mut impl Host, id: i32) -> u32 {
    let car = st.profile.career_car as i32;
    let g = group(id);
    let rom_slot = |i: u32| h.rom()[(SLOT_BYTES + i) as usize & 0x1FF_FFFF] as i32;
    let mut best = 0u32;
    if id < 0x79 {
        let (levels, base, limit, tail): (u32, i32, fn(u32) -> bool, i32) = match g {
            8 => (2, 0xF, |lv| lv < 3, 8),
            0..=7 => (3, 7 + g, |lv| lv < 3, 0xC),
            9 => (2, 0x10, |_| true, 7),
            _ => return 0,
        };
        for i in 0..levels {
            let lv = u32::from(rec(&st.profile, car, base)) >> (2 * i) & 3;
            let ok = if g == 9 { (lv as i32) < 3 - i as i32 } else { limit(lv) };
            if ok {
                let b = id + lv as i32 + 1 + 4 * i as i32;
                if unlock_bit(&st.profile, b) {
                    best = b as u32;
                }
            }
        }
        if rec(&st.profile, car, base) >> if g == 8 || g == 9 { 4 } else { 6 } != 0 {
            return best;
        }
        let t = id + tail;
        return if unlock_bit(&st.profile, t) { t as u32 } else { best };
    }
    let v = i32::from(rec(&st.profile, car, g)) + id;
    let bit = |off: usize, n: u32| st.profile.unlock_byte(off - 0x42D) >> n & 1 != 0;
    match g {
        0 => {
            let k = match v {
                0x79 => 0,
                ..0x80 => 1,
                ..0x85 => 2,
                ..0x88 => 3,
                _ => 4,
            };
            if k == 0 && bit(0x43C, 2) {
                best = 0x7A;
            }
            if k == 1 && bit(0x43D, 0) {
                best = 0x80;
            }
            if k == 2 && bit(0x43D, 5) {
                best = 0x85;
            }
            if k == 3 && bit(0x43E, 0) {
                best = 0x88;
            }
        }
        1 => {
            let k = (v - id + 1) >> 1;
            for (n, off, sh, r) in [(0, 0x43E, 2, 0x8A), (1, 0x43E, 4, 0x8C), (2, 0x43E, 6, 0x8E)] {
                if k == n && bit(off, sh) {
                    best = r;
                }
            }
        }
        2 => {
            let k = match v {
                0x90 => 0,
                ..=0x93 => 1,
                ..=0x96 => 2,
                ..=0x99 => 3,
                ..=0x9B => 4,
                ..0x9E => 5,
                _ => 6,
            };
            if k == 0 && bit(0x43F, 1) {
                best = 0x91;
            }
            if k == 1 && bit(0x43F, 4) {
                best = 0x94;
            }
            if k == 2 && bit(0x43F, 7) {
                best = 0x97;
            }
            if k == 3 && bit(0x440, 2) {
                best = 0x9A;
            }
            if k == 4 && bit(0x440, 4) {
                best = 0x9C;
            }
            if k == 5 && bit(0x440, 6) {
                best = 0x9E;
            }
        }
        3 => {
            if v - id + 4 - rom_slot(car as u32) < 3 {
                best = (v + 1) as u32;
                let mut u = best;
                if (v as u32).wrapping_sub(0xA0) < 3 {
                    let sum: i32 = (0..st.g.player_car).map(rom_slot).sum();
                    u = u.wrapping_add(sum as u32).wrapping_sub(st.g.player_car);
                }
                if !unlock_bit(&st.profile, u as i32) {
                    best = 0;
                }
            }
        }
        4 => {
            let k = match v {
                0xBF => 0,
                ..=199 => 1,
                ..=0xCF => 2,
                ..0xD8 => 3,
                _ => 4,
            };
            if k == 0 && bit(0x445, 0) {
                best = 0xC0;
            }
            if k == 1 && bit(0x446, 0) {
                best = 200;
            }
            if k == 2 && bit(0x447, 0) {
                best = 0xD0;
            }
            if k == 3 && bit(0x448, 0) {
                best = 0xD8;
            }
        }
        5 if v < 0xE1 => best = flow::rand_table(st, h) % 0x13 + 0xE1,
        _ => {}
    }
    best
}

/// `FUN_081300E0` (screen, item): whether a list item carries a "new" mark. Every id of the item is asked (each
/// may draw a random number).
pub fn list_item_new(st: &mut MenuState, h: &mut impl Host, screen: u32, item: u32) -> u32 {
    let (first, n) = match (screen, item) {
        (4, 0) | (0x1D, 0) => (0, 10),
        (4, _) => (10, 7),
        (0x1D, _) => (item - 1, 1),
        (0x1E, 0) => (10, 7),
        (0x1E, 1) => (10, 3),
        (0x1E, 2) => (13, 2),
        (0x1E, 3 | 4) => (item + 12, 1),
        (0x23, _) => (item + 10, 1),
        (0x24, _) => (item + 13, 1),
        _ => return 0,
    };
    let mut any = 0;
    for k in 0..n {
        let id = rom_u16(h.rom(), ITEMS + 2 * (first + k)) as i32;
        if new_part(st, h, id) != 0 {
            any = 1;
        }
    }
    any
}

/// `garage_copy_car_record` (`FUN_0812D564`): the record of the garage car to the working copy.
pub fn copy_car_record(st: &mut MenuState) {
    st.g.garage_car = (0..17)
        .map(|i| rec(&st.profile, st.g.player_car as i32, i))
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
}

/// `FUN_0812D5BC`: the working copy's level of the part's group.
fn level_of(st: &MenuState, id: i32) -> u32 {
    let at = group(id) + if id < 0x79 { 7 } else { 0 };
    usize::try_from(at)
        .ok()
        .and_then(|i| st.g.garage_car.get(i))
        .copied()
        .unwrap_or(0) as u32
}

/// `FUN_0812D5E8`: the four 2-bit levels of the group's byte (0 for ids from 0x79), and their sum.
fn levels(st: &MenuState, id: i32) -> ([i32; 4], i32) {
    let v = if id < 0x79 {
        usize::try_from(group(id) + 7)
            .ok()
            .and_then(|i| st.g.garage_car.get(i))
            .copied()
            .unwrap_or(0)
    } else {
        0
    } as i32;
    let l = [v & 3, v >> 2 & 3, v >> 4 & 3, v >> 6];
    (l, l.iter().sum())
}

/// `FUN_0812D4E4` (id, reload): sets the working copy's level and reloads what depends on it.
fn set_level(st: &mut MenuState, h: &mut impl Host, id: i32, reload: bool) {
    let g = group(id);
    let rom = h.rom();
    if id < 0x79 {
        let v = (id as u8).wrapping_sub(rom[(PART_BASE + 2 * g as u32) as usize & 0x1FF_FFFF]);
        if let Some(b) = usize::try_from(g + 7).ok().and_then(|i| st.g.garage_car.get_mut(i)) {
            *b = v;
        }
    } else {
        let v = (id as u8).wrapping_sub(rom[(PART_BASE_B + 2 * g as u32) as usize & 0x1FF_FFFF]);
        if let Some(b) = usize::try_from(g).ok().and_then(|i| st.g.garage_car.get_mut(i)) {
            *b = v;
        }
        if g < 4 {
            h.car_atlas(st);
        }
        if g == 6 {
            h.car_palette(st);
        }
        if g == 5 {
            h.car_atlas(st);
        }
        if g == 4 {
            h.car_atlas(st);
            h.car_palette(st);
        }
    }
    if reload {
        h.car_atlas(st);
    }
}

/// `garage_load_car_palette` (`0x0812BF48`): the garage car and its paint (the working copy's `+6`, or the special
/// car's ramp from `0x7EEA24`) into palette slots 160.. of a base palette buffer ([`nfsgba_formats::paint`]); the
/// other two paint numbers are what the last race or garage left in `0x03005FEE`. The glass slots 192 and 208 are
/// `garage_draw_car`'s.
pub fn load_car_palette(st: &mut MenuState, rom: &[u8], base: &mut [u16]) {
    let car = st.g.player_car as u8;
    st.g.race_car = car;
    st.g.race_car_b = car.wrapping_add(15);
    let paint = st.g.garage_car[6];
    st.g.paints[0] = if paint < 0x14 {
        paint as i8
    } else {
        rom[0x7E_EA24 + st.g.player_car as usize] as i8
    };
    let cars = [car as i8, car.wrapping_add(15) as i8, 0, 0];
    nfsgba_formats::paint::load_car_palettes(rom, base, cars, st.g.paints, &st.g.garage_car, false);
    st.g.palette_dirty = 1;
}

/// `FUN_0812C3E0`: the working copy's rating (100 and the parts' shares).
fn rating(st: &MenuState, rom: &[u8]) -> i32 {
    let c = &st.g.garage_car;
    let mut r = if c[5] != 0 { 0x69 } else { 100 };
    r += match c[0] {
        0 => 0,
        1..=6 => 8,
        7..=11 => 10,
        12..=14 => 0xE,
        _ => 0x11,
    };
    r += match c[1] {
        0 => 0,
        1..=2 => 8,
        3..=4 => 0xD,
        _ => 0xF,
    };
    r += match c[2] {
        0 => 0,
        1..=6 => 4,
        7..=11 => 5,
        _ => 6,
    };
    if c[3] != 0 {
        let slot = rom[(SLOT_BYTES + st.profile.career_car as u32) as usize & 0x1FF_FFFF];
        r += (i32::from(c[3]) + 4 - i32::from(slot)) * 0xF;
    }
    r + match c[4] >> 3 {
        1 => 4,
        2 => 7,
        3 => 10,
        4 => 0xC,
        _ => 0,
    }
}

/// `FUN_0812C494`: a car's base stats and the stats with its installed parts (each capped at 100).
fn stats(st: &MenuState, rom: &[u8], car: i32) -> ([i32; 3], [i32; 3]) {
    let at = CAR_TABLE + car as u32 * 0x58;
    let base: [i32; 3] = std::array::from_fn(|i| i32::from(rom[(at + 0x52 + i as u32) as usize & 0x1FF_FFFF]));
    let mut up = base;
    for j in 0..10u32 {
        let b = i32::from(rec(&st.profile, car, 7 + j as i32));
        let sum = (b & 3) + (b >> 2 & 3) + (b >> 4 & 3) + (b >> 6);
        for (k, u) in up.iter_mut().enumerate() {
            let w = i32::from(rom[(STAT_WEIGHTS + 4 * j + k as u32) as usize & 0x1FF_FFFF] as i8);
            *u += nfsgba_fixed::div(w * sum, 100);
        }
    }
    (base, up.map(|v| v.min(100)))
}

fn text(h: &mut impl Host, font: u32, key: u32, x: i32, y: i32, a: i32, c: i32) -> u32 {
    h.call(TEXT_MENU, &[font, key, x as u32, y as u32, a as u32, c as u32])
}

fn blit(h: &mut impl Host, m: u32, x: i32, y: i32) {
    h.call(MENU_BLIT_MATERIAL, &[WORLD, m, x as u32, y as u32]);
}

fn number_arg(st: &mut MenuState, h: &mut impl Host, n: i32, separators: bool) -> u32 {
    let mut s = number_text(&mut st.g.div_remainder, n);
    if separators {
        thousands(st.g.language, &mut s, n);
    }
    h.text_arg(s)
}

/// A number with " %" (a space first in French and Italian... language 1 and 3) as the game builds it.
fn percent_arg(st: &mut MenuState, h: &mut impl Host, n: i32) -> u32 {
    let mut s = number_text(&mut st.g.div_remainder, n);
    if matches!(st.g.language, 1 | 3) {
        s.push(b' ');
    }
    s.push(b'%');
    h.text_arg(s)
}

/// `FUN_08133D30` (car, x, y, rows): the stat bars (speed, accel, handling, and with `rows` the style rating).
pub fn car_stats_draw(st: &mut MenuState, h: &mut impl Host, car: i32, x: i32, y: i32, rows: u32) {
    let x8 = x - 8;
    text(h, 0xE, 0x8B, x8, y, -1, 0);
    text(h, 0xE, 0x2E2, x8, y + 10, -1, 0);
    text(h, 0xE, 0x141, x8, y + 0x14, -1, 0);
    let n = if rows == 0 {
        3
    } else {
        text(h, 0xE, 0x39A, x8, y + 0x1E, -1, 0);
        4
    };
    let (base, up) = stats(st, h.rom(), car);
    let style = career_style_rating(h.rom(), &st.profile) - 100;
    let (base, up) = ([base[0], base[1], base[2], style], [up[0], up[1], up[2], style]);
    for i in 0..n {
        for k in 0..10 {
            let m = if k < nfsgba_fixed::div(base[i], 10) {
                0xA5
            } else if k < nfsgba_fixed::div(up[i], 10) {
                0xA6
            } else {
                0xA4
            };
            blit(h, m, x + 8 * k, y + i as i32 * 10 + 2);
        }
    }
}

/// `FUN_08133E74`: a district's completion in percent (events won or second, 25/3 each; the 6-event district counts
/// double).
fn completion(st: &MenuState, zone: i32) -> i32 {
    let n = if zone == 5 { 6 } else { 12 };
    let won = (0..n)
        .filter(|&u| matches!(event_status(&st.profile, zone * 12 + u), 1 | 2))
        .count() as i32;
    nfsgba_fixed::div((if zone == 5 { won << 1 } else { won }) * 25, 3)
}

/// The header (`0x087E6EA4 + 0x10 * index`) of screen 0x13, 0x14, 0x12 and the page it points to, by the upgrade
/// selection at profile `+0x364 + index`.
fn page(st: &MenuState, rom: &[u8]) -> Option<(u32, u32, u32)> {
    let (index, sel) = match st.g.screen {
        0x13 => (0, st.profile.upgrade_a),
        0x14 => (1, st.profile.upgrade_b),
        0x12 => (2, st.profile.upgrade_c),
        _ => return None,
    };
    let header = PAGES + 0x10 * index;
    let page = rom_u32(rom, header + 0xC).wrapping_add((sel as i8 as i32 * 0x14) as u32);
    Some((index, header, page))
}

/// `kind18_enter` (`0x08133708`).
pub fn enter(st: &mut MenuState, h: &mut impl Host) -> u32 {
    let Some((_, header, page)) = page(st, h.rom()) else {
        return 1;
    };
    let (items, rowdefs) = (rom_u32(h.rom(), page + 8), rom_u32(h.rom(), page + 0x10));
    st.profile.u_338 = 0;
    let (material, palette) = (s16(h.rom(), header + 4) as u32, s16(h.rom(), header + 6) as u32);
    h.scene_setup(st, material, palette, 0xFFFF);
    if st.g.screen == 0x13 {
        st.g.garage_row = 0;
        let mut item = items;
        for r in 0..s16(h.rom(), page + 0xC).max(0) as usize {
            if let Some(p) = st.g.garage_picks.get_mut(r) {
                *p = 0;
            }
            for j in 0..s16(h.rom(), rowdefs + 8 * r as u32 + 6).max(0) as usize {
                let (id, key) = (rom_u16(h.rom(), item + 6), rom_u16(h.rom(), item));
                for (o, v) in [(0, id), (1, key)] {
                    if let Some(c) = st.g.garage_items.get_mut(r * 8 + j * 2 + o) {
                        *c = v;
                    }
                }
                item += 10;
            }
        }
        copy_car_record(st);
    } else {
        st.g.player_car = st.profile.career_car as i32 as u32;
        copy_car_record(st);
        st.profile.u_338 = level_of(st, s16(h.rom(), items + 6));
        h.car_atlas(st);
        h.car_palette(st);
    }
    st.g.screen_changed = 1;
    1
}

/// The unlock id under the part shop's cursor (screen 0x13): the pick of the current row.
fn picked(st: &MenuState) -> i32 {
    let (row, pick) = (
        st.g.garage_row as i32,
        st.g.garage_picks.get(st.g.garage_row as usize).copied().unwrap_or(0),
    );
    usize::try_from(row * 8 + pick as i32 * 2)
        .ok()
        .and_then(|i| st.g.garage_items.get(i))
        .map_or(0, |&v| v as i16 as i32)
}

/// The unlock id the update acts on: the part shop's pick, else the current item's.
fn chosen(st: &MenuState, rom: &[u8], item: u32) -> i32 {
    if st.g.screen == 0x13 {
        picked(st)
    } else {
        s16(rom, item + 6)
    }
}

/// `kind18_update` (`0x081338E0`).
pub fn update(st: &mut MenuState, h: &mut impl Host) -> u32 {
    let Some((_, _, page)) = page(st, h.rom()) else {
        return 1;
    };
    let (title, count) = (s16(h.rom(), page), rom_u16(h.rom(), page + 4) as u32);
    let (items, rowdefs, rows) = (
        rom_u32(h.rom(), page + 8),
        rom_u32(h.rom(), page + 0x10),
        s16(h.rom(), page + 0xC),
    );
    let mut count = count;
    if items == 0 {
        return 1;
    }
    let keys = u32::from(st.g.keys);
    match st.g.screen {
        0x13 => {
            let row = st.g.garage_row as usize;
            let rowlen = s16(h.rom(), rowdefs + 8 * row as u32 + 6);
            if keys & 0x20 != 0
                && let Some(p) = st.g.garage_picks.get_mut(row)
            {
                if *p == 0 {
                    *p = rowlen as i8;
                }
                *p = p.wrapping_sub(1);
            }
            if keys & 0x10 != 0
                && let Some(p) = st.g.garage_picks.get_mut(row)
            {
                *p = p.wrapping_add(1);
                if rowlen <= i32::from(*p) {
                    *p = 0;
                }
            }
            if keys & 0x40 != 0 {
                if st.g.garage_row == 0 {
                    st.g.garage_row = rows as i8;
                }
                st.g.garage_row = st.g.garage_row.wrapping_sub(1);
            }
            if keys & 0x80 != 0 {
                st.g.garage_row = st.g.garage_row.wrapping_add(1);
                if rows <= i32::from(st.g.garage_row) {
                    st.g.garage_row = 0;
                }
            }
            if keys & 0xF0 != 0 {
                h.call(CARBON_PLAY_SOUND, &[4, 1]);
                st.g.screen_changed = 1;
            }
        }
        0x12 => st.profile.u_338 = 0,
        _ => {
            if title == 0x95 {
                count = u32::from(h.rom()[(SLOT_BYTES + st.g.player_car) as usize & 0x1FF_FFFF]);
            }
            if keys & 0x20 != 0 {
                st.profile.u_338 = st.profile.u_338.wrapping_sub(1);
                if (st.profile.u_338 as i32) < 0 {
                    st.profile.u_338 = count.wrapping_sub(1);
                }
                let id = s16(h.rom(), items + st.profile.u_338.wrapping_mul(10) + 6);
                set_level(st, h, id, false);
            }
            if keys & 0x10 != 0 {
                st.profile.u_338 = st.profile.u_338.wrapping_add(1);
                if count as i32 <= st.profile.u_338 as i32 {
                    st.profile.u_338 = 0;
                }
                let id = s16(h.rom(), items + st.profile.u_338.wrapping_mul(10) + 6);
                set_level(st, h, id, false);
            }
            let held = u32::from(st.g.keys_held);
            if held & 0x40 != 0 {
                st.profile.u_2f6 = 0x80;
                st.profile.u_2f8 = st.g.garage_angle.wrapping_add(0x400);
            }
            if held & 0x80 != 0 {
                st.profile.u_2f6 = 0x80;
                st.profile.u_2f8 = st.g.garage_angle.wrapping_sub(0x400);
            }
            if keys & 0xF0 != 0 {
                h.call(CARBON_PLAY_SOUND, &[4, 1]);
                st.g.screen_changed = 1;
            }
        }
    }
    let item = items.wrapping_add(st.profile.u_338.wrapping_mul(10));
    let target = s16(h.rom(), item + 8);
    if st.g.keys == 1 && target != -1 {
        if target < 0x85 {
            h.call(CARBON_PLAY_SOUND, &[2, 1]);
            if st.g.screen == 0x12 {
                st.profile.map_mode = 2;
            }
            flow::goto_screen(st, h, target);
        } else if target == 0x86 {
            let id = chosen(st, h.rom(), item);
            if !installed(st, h.rom(), id) {
                let (kind, key) = match state(st, h.rom(), id as u32) {
                    1 => {
                        h.call(CARBON_PLAY_SOUND, &[2, 1]);
                        (1, 0x376)
                    }
                    0 => {
                        h.call(CARBON_PLAY_SOUND, &[2, 1]);
                        (
                            2,
                            if (id as u32).wrapping_sub(0xF4) < 0x14 {
                                0x2BD
                            } else {
                                0x2BE
                            },
                        )
                    }
                    2 => {
                        h.call(CARBON_PLAY_SOUND, &[0x27, 1]);
                        (1, 0x192)
                    }
                    3 => {
                        h.call(CARBON_PLAY_SOUND, &[0x27, 1]);
                        (1, 0x172)
                    }
                    _ => (0, 0),
                };
                if kind != 0 {
                    message_box_open(st, kind, key, u32::MAX);
                }
            }
        }
    }
    if st.g.message_result > 0 {
        if matches!(st.g.screen, 0x13 | 0x14) {
            let id = chosen(st, h.rom(), item);
            if state(st, h.rom(), id as u32) < 2 {
                buy(st, h.rom(), id as u32);
                h.save_write(st);
            }
        }
        st.g.message_result = 0;
    }
    if st.g.keys == 2 {
        copy_car_record(st);
        h.car_atlas(st);
        h.car_palette(st);
        h.call(CARBON_PLAY_SOUND, &[3, 1]);
    }
    1
}

/// `kind18_draw` (`0x08133F2C`).
pub fn draw(st: &mut MenuState, h: &mut impl Host) -> u32 {
    let Some((_, header, page)) = page(st, h.rom()) else {
        return 0;
    };
    let (mut items, rowdefs, rows) = (
        rom_u32(h.rom(), page + 8),
        rom_u32(h.rom(), page + 0x10),
        s16(h.rom(), page + 0xC),
    );
    h.call(INTRO_PAGE_SETUP, &[st.g.unpack_buffer]);
    text(h, 0xC, s16(h.rom(), page) as u32, 0xEC, 2, -1, 0);
    let mut percent_n: Option<i32> = None;
    match st.g.screen {
        0x13 => {
            let row = st.g.garage_row as i32;
            let last = rows - 1;
            for r in 0..rows {
                let rd = rowdefs + 8 * r as u32;
                let (x, y) = (s16(h.rom(), rd + 2), s16(h.rom(), rd + 4));
                let dim = if r != row { 8 } else { 0 };
                let mut only_locked = true;
                blit(h, if r == row { 0xD8 } else { 0xD1 }, x, y + 2);
                text(h, 0xD, s16(h.rom(), rd) as u32, x + 3, y + 5, 0, dim);
                let mut cx = 0x82;
                for j in 0..s16(h.rom(), rd + 6) {
                    let at = (r * 8 + j * 2) as usize;
                    let (id, key) = (st.g.garage_items[at] as i16 as i32, st.g.garage_items[at + 1]);
                    if st.profile.is_locked(id) == 0 {
                        if !installed(st, h.rom(), id) {
                            blit(h, 0xCE, x + cx, y + 6);
                        } else {
                            if dim != 0 {
                                text(h, 0xE, u32::from(key), 0x78, y + 0x10, -1, dim);
                                only_locked = false;
                            }
                            blit(h, 0xCF, x + cx, y + 6);
                        }
                    } else {
                        blit(h, 0xCD, x + cx, y + 6);
                    }
                    cx += 0x14;
                }
                if dim == 0 {
                    let pick = st.g.garage_picks.get(r as usize).copied().unwrap_or(0) as i32;
                    let at = (r * 8 + pick * 2) as usize;
                    let id = st.g.garage_items.get(at).map_or(0, |&v| v as i16 as i32);
                    if st.profile.is_locked(id) == 0 {
                        let key = st.g.garage_items.get(at + 1).copied().unwrap_or(0);
                        text(h, 0xE, u32::from(key), 0x78, y + 0x10, -1, dim);
                    } else {
                        text(h, 0xE, 0x15A, 0x78, y + 0x10, -1, dim);
                    }
                    only_locked = false;
                }
                if only_locked {
                    text(h, 0xE, 0x170, 0x78, y + 0x10, -1, dim);
                }
            }
            let rd = rowdefs + 8 * row as u32;
            let pick = st.g.garage_picks.get(row as usize).copied().unwrap_or(0) as i32;
            blit(
                h,
                0xD0,
                s16(h.rom(), rd + 2) + pick * 0x14 + 0x7F,
                s16(h.rom(), rd + 4) + 3,
            );
            if row != last {
                let y = s16(h.rom(), rd + 4) + 6;
                blit(h, if st.profile.repeats[0] < 1 { 0xAD } else { 0xAE }, 0x78, y);
                blit(h, if st.profile.repeats[1] < 1 { 0xAF } else { 0xB0 }, 0xDC, y);
            }
            copy_car_record(st);
            let (lv, sum) = levels(st, s16(h.rom(), items + 6));
            for k in sum..10 {
                blit(h, 0xA4, 0x7D + 7 * k, 0x82);
            }
            for k in 0..sum {
                blit(h, 0xA5, 0x7D + 7 * k, 0x82);
            }
            let mut n = sum;
            if st.profile.is_locked(picked(st)) == 0 {
                let mut d = pick - lv[row as usize & 3];
                if row == last {
                    d += 1;
                }
                let start = sum.max(0);
                if d < 0 {
                    let mut x = start * 7 + 0x7D;
                    for _ in 0..lv[row as usize & 3] - pick {
                        x -= 7;
                        blit(h, 0xA6, x, 0x82);
                    }
                } else {
                    for k in 0..d {
                        blit(h, 0xA6, start * 7 + 0x7D + 7 * k, 0x82);
                    }
                }
                n = (0..4).filter(|&i| i != row).map(|i| lv[i as usize]).sum::<i32>()
                    + if row == last { 1 } else { 0 }
                    + pick;
            }
            percent_n = Some(n * 10);
        }
        0x14 => {
            h.draw_car(st, 0x78, 0x3C, 0xFA);
            items = items.wrapping_add(st.profile.u_338.wrapping_mul(10));
            text(h, 0xC, s16(h.rom(), items) as u32, 0x78, 0x6E, 1, 8);
            if st.g.message_box < 0 {
                let y = s16(h.rom(), 0x0879_7CE0) + 0x54;
                blit(h, if st.profile.repeats[0] < 1 { 0xA7 } else { 0xA8 }, 3, y);
                blit(h, if st.profile.repeats[1] < 1 { 0xA9 } else { 0xAA }, 0xDD, y);
            }
            for k in 0..10 {
                blit(h, 0xA4, 0x7D + 7 * k, 0x82);
            }
            let rated = nfsgba_fixed::div(rating(st, h.rom()) - 100, 10);
            let styled = nfsgba_fixed::div(career_style_rating(h.rom(), &st.profile) - 100, 10);
            for k in 0..10 {
                let m = if styled < rated {
                    if k < styled {
                        0xA5
                    } else if k < rated {
                        0xA6
                    } else {
                        0xA4
                    }
                } else if k < rated {
                    0xA5
                } else if k < styled {
                    0xA6
                } else {
                    0xA4
                };
                blit(h, m, 0x7D + 7 * k, 0x82);
            }
            percent_n = Some(rated * 10);
        }
        _ => {
            h.draw_car(st, 0xB2, 0x3C, 0x172);
            car_stats_draw(st, h, st.profile.career_car as i32, 0x8C, 0x68, 1);
            let name: Vec<u8> = st.profile.name.iter().copied().take_while(|&b| b != 0).collect();
            let name = h.text_arg(name);
            text(h, 0xD, name, 0x6E, 6, -1, 8);
            for i in 0..6 {
                let y = 0x14 + 0xE * i;
                text(h, 0xD, (i + 0x3C9) as u32, 4, y, 0, 0);
                if st.profile.is_locked(i + 0x117) == 0 {
                    let s = percent_arg(st, h, completion(st, i));
                    text(h, 0xD, s, 0x6E, y, -1, 8);
                } else {
                    text(h, 0xD, 0x15A, 0x6E, y, -1, 8);
                }
            }
        }
    }
    if let Some(n) = percent_n {
        let s = percent_arg(st, h, n);
        text(h, 0xD, s, 200, 0x80, 0, 8);
    }
    let left = s16(h.rom(), page + 2);
    let right = s16(h.rom(), header + 2);
    if matches!(st.g.screen, 0x13 | 0x14) {
        let id = if st.g.screen == 0x13 {
            picked(st)
        } else {
            s16(h.rom(), items + 6)
        };
        let mut left = left;
        if owned(st, h.rom(), id as u32) == 0 {
            left = if state(st, h.rom(), id as u32) == 2 {
                0x15A
            } else {
                0x19C
            };
            let w = text(h, 0xE, 0xE3, 0xB, 0x7C, 0, 0);
            let cost = price(st, h.rom(), id as u32) as i32;
            let mut s = number_text(&mut st.g.div_remainder, cost);
            thousands(st.g.language, &mut s, cost);
            let s = h.text_arg(s);
            text(h, 0xE, s, w as i32 + 0xB, 0x86, -1, 8);
        }
        text(h, 0xE, 0x196, 0x6A, 0x7C, -1, 0);
        let s = number_arg(st, h, st.profile.cash, true);
        text(h, 0xE, s, 0x6A, 0x86, -1, 8);
        if installed(st, h.rom(), id) {
            left = -1;
        }
        h.button_prompts(st, &[left as u32, right as u32, u32::MAX]);
    } else {
        h.button_prompts(st, &[left as u32, right as u32, u32::MAX]);
    }
    0
}
