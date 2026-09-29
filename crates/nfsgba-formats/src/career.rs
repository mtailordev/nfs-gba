//! Career mode, race setup, race rules and the EEPROM save (`docs/formats/career.md`).
//!
//! Offsets are ROM offsets. RAM addresses in comments are the game's (IWRAM `0x03…`, EWRAM `0x02…`); the
//! profile struct is `*0x030056EC` (`0x02000808` in the reference run).

use super::{i16_at, ptr, u16_at, u32_at};
use std::io;

/// 66 event records of 8 bytes: zones 1–5 have 12 events each, zone 6 has 6 (`FUN_0812da08`).
pub const EVENT_TABLE: usize = 0x7E_4744;
pub const EVENT_COUNT: usize = 66;
/// Per zone, the two boss events (`u16` event index; zone 6 has none and holds 66, 66).
pub const BOSS_EVENTS: usize = 0x7E_4714;
/// 42 `(u16 text key, u16 route number)` pairs: circuits forward (0–11), reverse (12–23), sprints (24–41).
pub const TRACK_NAMES: usize = 0x7E_4A70;
/// `u32` per route number 0..=42: the slot in [`TRACK_NAMES`] (`FUN_0812b5f0`).
pub const ROUTE_TRACK_SLOT: usize = 0x7E_49C4;
/// 44 records of 0xC bytes per route number: environment and route-table index (`FUN_0812b5f0`).
pub const RACE_SLOTS: usize = 0x7F_2588;
/// Route table (`docs/formats/race-routes.md`): 44 records of 0x14 bytes.
pub const ROUTE_TABLE: usize = 0x7F_2798;
pub const ROUTE_COUNT: usize = 44;
/// Wingman name keys (14 × `u16`) and roles (13 × `(u16 role text key, u16 level)`).
pub const WINGMAN_NAMES: usize = 0x7E_4974;
pub const WINGMAN_ROLES: usize = 0x7E_4990;
pub const WINGMAN_COUNT: usize = 13;
/// Unlocks by career progress: `u16 key, u16 unlock ids…, 0xFFFF`, repeated; a key of `0xFFFF` ends it.
pub const PROGRESS_UNLOCKS: usize = 0x7E_4B18;
/// Six setup screens (0x10-byte headers): options, race info, then the circuit/elimination/hunter/sprint setups.
pub const SETUP_SCREENS: usize = 0x7E_6260;
/// Race mode names (`u16` text keys), indexed by [`RaceMode`].
pub const MODE_NAMES: usize = 0x7E_5070;
/// Per car, the stock value that the style rating's fourth field is measured against (`FUN_0812c30c`).
pub const STYLE_BASE: usize = 0x7F_0626;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RaceMode {
    Circuit,
    Elimination,
    Hunter,
    Sprint,
}

impl RaceMode {
    /// The value of `0x030056E0`.
    pub fn from_index(i: u8) -> Option<Self> {
        [Self::Circuit, Self::Elimination, Self::Hunter, Self::Sprint]
            .get(i as usize)
            .copied()
    }
}

/// One career event (`FUN_0812da08` copies it into the race globals).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Event {
    pub zone: u8,
    pub slot: u8,
    /// `+0`: AI skill 0..100 (`0x030000BC`); `skill / 35` is the difficulty 0..2 (`0x03005608`).
    pub skill: u8,
    /// `+1`: slot in [`TRACK_NAMES`] of the forward track (`reverse` adds 12).
    pub track: u8,
    /// `+2` (`0x030056E0`).
    pub mode: RaceMode,
    /// `+3` (`0x03005610`).
    pub reverse: bool,
    /// `+4` (`0x030056E4`).
    pub laps: u8,
    /// `+5`: 0 none, 1 light, 2 heavy (`0x03005604`).
    pub traffic: u8,
    /// `+6`: cash for the next win, see [`race_payout`].
    pub reward: i16,
}

impl Event {
    pub fn difficulty(&self) -> u8 {
        self.skill / 35
    }
    pub fn track_slot(&self) -> usize {
        self.track as usize + if self.reverse { 12 } else { 0 }
    }
}

pub fn events(rom: &[u8]) -> Vec<Event> {
    (0..EVENT_COUNT)
        .map(|i| {
            let r = &rom[EVENT_TABLE + 8 * i..];
            Event {
                zone: (i / 12) as u8,
                slot: (i % 12) as u8,
                skill: r[0],
                track: r[1],
                mode: RaceMode::from_index(r[2]).expect("event mode"),
                reverse: r[3] != 0,
                laps: r[4],
                traffic: r[5],
                reward: i16_at(r, 6),
            }
        })
        .collect()
}

/// `[zone][0..2]`: the event indices that are boss races.
pub fn boss_events(rom: &[u8]) -> [[u16; 2]; 6] {
    std::array::from_fn(|z| [0, 1].map(|k| u16_at(rom, BOSS_EVENTS + 4 * z + 2 * k)))
}

/// `(text key, route number)` of a [`TRACK_NAMES`] slot.
pub fn track_name(rom: &[u8], slot: usize) -> (usize, usize) {
    (
        u16_at(rom, TRACK_NAMES + 4 * slot) as usize,
        u16_at(rom, TRACK_NAMES + 4 * slot + 2) as usize,
    )
}

/// The [`TRACK_NAMES`] slot of route number `route` (0..=42).
pub fn route_track_slot(rom: &[u8], route: usize) -> usize {
    u32_at(rom, ROUTE_TRACK_SLOT + 4 * route) as usize
}

/// What a route number loads: `+0` environment (level descriptor, `0x0300006C`), `+1` route-table index
/// (`0x03005720`). The other 10 bytes are not decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RaceSlot {
    pub environment: u8,
    pub route: u8,
    pub rest: [u8; 10],
}

pub fn race_slots(rom: &[u8]) -> Vec<RaceSlot> {
    (0..ROUTE_COUNT)
        .map(|i| {
            let r = &rom[RACE_SLOTS + 0xC * i..];
            RaceSlot {
                environment: r[0],
                route: r[1],
                rest: r[2..12].try_into().unwrap(),
            }
        })
        .collect()
}

/// A racing-line section: section 0 is the lap (its last waypoint repeats the first), the others are
/// branches. The table sits at route `+0x04`, right before the waypoints (`+0x08`), 8 bytes per section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Section {
    pub count: u16,
    /// `+2`, always 0.
    pub flags: u16,
    /// Index of the first waypoint in the route's waypoint array.
    pub first: u32,
}

/// Per route-table record, its sections (empty when the record has no racing line).
pub fn route_sections(rom: &[u8]) -> Vec<Vec<Section>> {
    (0..ROUTE_COUNT)
        .map(|i| ROUTE_TABLE + 0x14 * i)
        .map(|r| {
            if u32_at(rom, r + 8) == 0 {
                return Vec::new();
            }
            let (table, line) = (ptr(rom, r + 4), ptr(rom, r + 8));
            (0..(line - table) / 8)
                .map(|k| table + 8 * k)
                .map(|s| Section {
                    count: u16_at(rom, s),
                    flags: u16_at(rom, s + 2),
                    first: u32_at(rom, s + 4),
                })
                .collect()
        })
        .collect()
}

/// A selectable wingman (menu order; 0 is "none"). Unlock id `0x127 + index`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wingman {
    pub name_key: u16,
    /// `TEXT_ATTACKER`/`TEXT_DRAFTER` (`TEXT_NONE` for index 0).
    pub role_key: u16,
    /// Shown as icon `0x10B + level` (`FUN_08130d8c`).
    pub level: u16,
}

pub fn wingmen(rom: &[u8]) -> Vec<Wingman> {
    (0..WINGMAN_COUNT)
        .map(|i| Wingman {
            name_key: u16_at(rom, WINGMAN_NAMES + 2 * i),
            role_key: u16_at(rom, WINGMAN_ROLES + 4 * i),
            level: u16_at(rom, WINGMAN_ROLES + 4 * i + 2),
        })
        .collect()
}

/// Unlock ids granted when a zone's completed-event count reaches `key - zone * 12` (`FUN_08135958`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressUnlock {
    pub key: u16,
    pub ids: Vec<u16>,
}

pub fn progress_unlocks(rom: &[u8]) -> Vec<ProgressUnlock> {
    let mut at = PROGRESS_UNLOCKS;
    let mut next = || {
        at += 2;
        u16_at(rom, at - 2)
    };
    let mut out = Vec::new();
    while let key @ 0..=0xFFFE = next() {
        out.push(ProgressUnlock {
            key,
            ids: std::iter::from_fn(|| Some(next()).filter(|&v| v != 0xFFFF)).collect(),
        });
    }
    out
}

/// One line of a setup screen. The value lives in the variable named by `setting` (see `career.md`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupItem {
    pub text_key: u32,
    pub setting: u32,
    pub min: i16,
    pub max: i16,
    /// Text key per value `0..=max`.
    pub options: Vec<u32>,
    pub action: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupScreen {
    pub title_key: u16,
    pub button_key: u16,
    pub items: Vec<SetupItem>,
}

pub fn setup_screens(rom: &[u8]) -> Vec<SetupScreen> {
    (0..6)
        .map(|s| SETUP_SCREENS + 0x10 * s)
        .map(|h| SetupScreen {
            title_key: u16_at(rom, h),
            button_key: u16_at(rom, h + 2),
            items: (0..u16_at(rom, h + 10) as usize)
                .map(|i| ptr(rom, h + 12) + 0x18 * i)
                .map(|it| {
                    let max = i16_at(rom, it + 0x12);
                    SetupItem {
                        text_key: u32_at(rom, it),
                        setting: u32_at(rom, it + 8),
                        min: i16_at(rom, it + 0x10),
                        max,
                        options: (0..=max as usize)
                            .map(|v| u32_at(rom, ptr(rom, it + 0xC) + 4 * v))
                            .collect(),
                        action: u32_at(rom, it + 0x14),
                    }
                })
                .collect(),
        })
        .collect()
}

/// Style rating of the career car (`FUN_0812c30c`); `record` is its 17-byte RAM record ([`Save::cars`]).
pub fn style_rating(rom: &[u8], car: usize, record: &[u8; 17]) -> i32 {
    let r = record.map(i32::from);
    let mut v = if r[5] != 0 { 105 } else { 100 };
    if r[0] != 0 {
        v += tier(r[0], &[7, 12, 15], &[8, 10, 14, 17]);
    }
    if r[1] != 0 {
        v += tier(r[1], &[3, 5], &[8, 13, 15]);
    }
    if r[2] != 0 {
        v += tier(r[2], &[7, 12], &[4, 5, 6]);
    }
    if r[3] != 0 {
        v += (r[3] + 4 - rom[STYLE_BASE + car] as i32) * 15;
    }
    v + match r[4] >> 3 {
        1 => 4,
        2 => 7,
        3 => 10,
        4 => 12,
        _ => 0,
    }
}

/// Reward percentage for a style rating (`FUN_0812efe8`, `FUN_0812dd80`).
pub fn reward_percent(rating: i32) -> i32 {
    tier(rating, &[0x65, 0x7E, 0x97, 0xB0], &[100, 110, 120, 130, 140])
}

/// `add[k]`, where `k` counts the `limits` that `v` has reached (the games' `if v < limit … else if …` chains).
fn tier(v: i32, limits: &[i32], add: &[i32]) -> i32 {
    add[limits.iter().take_while(|&&l| v >= l).count()]
}

/// Career race result (`FUN_0812efe8`): `place` is 1 (won), 2 (second) or 3 (anything else). Updates the event's
/// status (2 bits per event: 1 won, 2 second, 3 not done) and the cash, and returns the payout.
pub fn race_payout(save: &mut Save, events: &[Event], zone: usize, slot: usize, place: u8, percent: i32) -> i32 {
    let event = zone * 12 + slot;
    let old = save.event_status(event);
    let done = (0..if zone == 5 { 6 } else { 12 }).filter(|&e| matches!(save.event_status(zone * 12 + e), 1 | 2));
    let base = events[zone * 12 + done.count() - usize::from(old != 3)].reward as i32;
    if place < old {
        save.events[event >> 2] = save.events[event >> 2] & !(3 << ((event & 3) * 2)) | place << ((event & 3) * 2);
    }
    let mut pay = percent * base / 100;
    if place == 2 {
        pay >>= 1;
    }
    if matches!(old, 1 | 2) {
        pay >>= 1;
    }
    if place == 3 {
        pay = 0;
    }
    save.cash = save.cash.wrapping_add(pay as u32);
    pay
}

/// Race progress used for positions (`FUN_081400ec`): laps done times the lap length plus the distance into the
/// lap. `laps_left` counts down from `laps` (driver `+0xC5`); sprints (`0x0300608C` = 0) use the distance alone.
pub fn race_progress(lapped: bool, lap_length: i32, laps: i32, laps_left: i32, distance: i32) -> i32 {
    if lapped {
        lap_length * (laps - laps_left) + distance
    } else {
        distance
    }
}

/// Hunter life (driver `+0x4E8`), `0..=HUNTER_LIFE_MAX`. Tuning from `FUN_081412ec`.
pub const HUNTER_LIFE_MAX: i32 = 0x80000;
/// Life gained per frame by race position 1..4 (`0x030061B0 + 4 * position`).
pub const HUNTER_GAIN: [i32; 5] = [0, 200, 150, 100, 0];

/// One frame of hunter life (`FUN_08140f78`): driving backwards (driver `+0x4EC` above 27) drains 1000, a wall
/// (`+0x4EE` above 50) drains 100, otherwise life grows by position until someone finishes (`0x030061A4`).
pub fn hunter_life_tick(life: i32, backwards: i16, wall: i16, position: usize, finished: bool) -> i32 {
    if backwards > 27 {
        (life - 1000).max(0)
    } else if wall > 50 {
        (life - 100).max(0)
    } else if finished {
        life
    } else {
        (life + HUNTER_GAIN[position]).min(HUNTER_LIFE_MAX)
    }
}

/// A hunter hit (`FUN_0814101c`): the victim loses `impulse * 0x440 >> 8`, the attacker gains three quarters of
/// that while nobody has finished and the victim is a racer (entity id ≤ opponents). Returns (attacker, victim).
pub fn hunter_hit(attacker: i32, victim: i32, impulse: i32, finished: bool, victim_is_racer: bool) -> (i32, i32) {
    let damage = (impulse * 0x440) >> 8;
    let attacker = if !finished && victim_is_racer {
        (attacker + ((damage * 3) >> 2)).min(HUNTER_LIFE_MAX)
    } else {
        attacker
    };
    (attacker, (victim - damage).max(0))
}

/// Elimination (`FUN_0813f098`): when the car in `position` finishes a lap (after `laps_left` was decremented)
/// and `position == opponents - laps_done + 1`, the car in `position + 1` is knocked out.
pub fn eliminated_position(position: i32, opponents: i32, laps: i32, laps_left: i32) -> Option<i32> {
    (position == opponents - (laps - laps_left) + 1).then_some(position + 1)
}

pub const SAVE_SIZE: usize = 512;
pub const SAVE_VERSION: u16 = 9;
pub const CHECKSUM_BIAS: u16 = 0xBADD;

/// The EEPROM image as mGBA stores it (`.sav`, bits in wire order) to the game's buffer: each 64-bit block
/// arrives most significant bit first and `FUN_08151194` stores it as four `u16` from the top, so every
/// 8-byte block is byte-reversed.
pub fn eeprom_to_buffer(sav: &[u8]) -> [u8; SAVE_SIZE] {
    let mut out = [0; SAVE_SIZE];
    for (o, s) in out.chunks_mut(8).zip(sav.chunks(8)) {
        o.copy_from_slice(s);
        o.reverse();
    }
    out
}

/// Sum of all 512 bytes with the checksum field zeroed, plus `0xBADD` (`FUN_081492c0`, `FUN_08149d84`).
pub fn checksum(buf: &[u8; SAVE_SIZE]) -> u16 {
    let sum = buf.iter().map(|&b| b as u32).sum::<u32>() - buf[0x100] as u32 - buf[0x101] as u32;
    (sum as u16).wrapping_add(CHECKSUM_BIAS)
}

/// Menu options, as the game's globals hold them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// 0 chase, 1 bumper (`0x030053E4`).
    pub camera: u8,
    /// 0 MPH, 1 KM/H (`0x03000040`).
    pub units: u8,
    /// 0 off, 1 on (`0x03005698`).
    pub hud: u8,
    /// 0 manual, 1 automatic (`0x03005798`).
    pub transmission: u8,
    /// 0 off, 1 low, 2 high; the globals hold `value << 3` (`0x0300578C`, `0x030053A4`).
    pub music: u8,
    pub sfx: u8,
    /// 0 En, 1 Fr, 2 De, 3 It, 4 Es (`0x03005600`; read at boot by `FUN_08149b94`).
    pub language: u8,
    /// 0 off, 1 on (`0x03000050`).
    pub catch_up: u8,
    /// One bit per [`RaceMode`] (`0x03000070`, set by `FUN_08134eb8`, tested by `FUN_0812cf48`).
    pub mode_flags: u8,
}

/// The profile, decoded exactly as `FUN_08149820` loads it (field comments: save offset → RAM).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Save {
    /// `0xB4` → profile `+0x00` (8 bytes, NUL-padded).
    pub name: [u8; 8],
    /// `0x00`, 12 bit-packed bytes per car → the 17-byte records at `*0x0300539C` (profile `+0xF9`).
    pub cars: [[u8; 17]; 15],
    /// Bit 7 of each car's 12th byte → profile `+0x12`.
    pub car_bits: u16,
    /// `0x11C`, 15 bytes per car → profile `+0x14`.
    pub car_extra: [[u8; 15]; 15],
    /// `0xC0` `u16` plus bit 0 of `0xC2` → profile `+0x0C`.
    pub cash: u32,
    /// `u16 0xBC` bits 7–10 → profile `+0x10` (career car).
    pub car: u8,
    /// `0xBC` bits 0–2 → profile `+0x1FB` (selected zone).
    pub zone: u8,
    /// `0xBC` bits 3–6 → profile `+0x1FC` (selected event in the zone).
    pub slot: u8,
    /// `u16 0xBE` bits 7–10 → profile `+0x200` (wingman, [`wingmen`] index).
    pub wingman: u8,
    /// `0xBF` bits 3–6 → profile `+0x254` (unknown).
    pub field_254: u8,
    /// `0x11A` → profile `+0x1F8` (unknown; 1 and 2 unlock wingmen 1 and 2 in `FUN_08135958`).
    pub field_1f8: u8,
    /// `0x104` → profile `+0xF5` (unknown).
    pub field_f5: [u8; 4],
    /// `0x108` → profile `+0x205`: 2 bits per event, see [`Save::event_status`].
    pub events: [u8; 18],
    /// `0xC4` → profile `+0x218`: per track (12 circuits, then 18 sprints), the record time to beat.
    pub best_times: [u16; 30],
    pub options: Options,
    /// `0x11B` bits 0–5 → profile `+0x47C, +0x480, +0x484, +0x48C, +0x488, +0x478` (unlock-everything flags).
    pub unlock_flags: u8,
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

impl Save {
    /// Decodes a game-order buffer (see [`eeprom_to_buffer`]). Requires the checksum (`FUN_08149d84`, load) and
    /// version 9 (`FUN_08149b94`, the boot check that marks the profile as present).
    pub fn parse(b: &[u8; SAVE_SIZE]) -> io::Result<Save> {
        let stored = u16_at(b, 0x100);
        if checksum(b) != stored {
            return Err(invalid("save checksum mismatch"));
        }
        if u16_at(b, 0x102) != SAVE_VERSION {
            return Err(invalid("save version is not 9"));
        }
        let mut car_bits = 0;
        let cars = std::array::from_fn(|i| {
            let p = &b[12 * i..12 * i + 12];
            car_bits |= u16::from(p[11] >> 7) << i;
            let mut r = [0u8; 17];
            r[7] = p[0] & 0x7F;
            r[8] = (p[1] & 0x3F) << 1 | p[0] >> 7;
            r[9] = (p[2] & 0x1F) << 2 | p[1] >> 6;
            r[10] = (p[3] & 0xF) << 3 | p[2] >> 5;
            r[0] = p[3] >> 4;
            r[11] = p[4] & 0x7F;
            r[12] = (p[5] & 0x3F) << 1 | p[4] >> 7;
            r[13] = (p[6] & 0x1F) << 2 | p[5] >> 6;
            r[14] = (p[7] & 0xF) << 3 | p[6] >> 5;
            r[2] = p[7] >> 4;
            r[15] = p[8] & 0x1F;
            r[16] = (p[9] & 3) << 3 | p[8] >> 5;
            r[6] = (p[9] & 0x7F) >> 2;
            r[5] = (p[10] & 0xF) << 1 | p[9] >> 7;
            r[1] = (p[10] & 0x7F) >> 4;
            r[3] = (p[11] & 1) << 1 | p[10] >> 7;
            r[4] = (p[11] & 0x7F) >> 1;
            r
        });
        let (bc, be) = (u16_at(b, 0xBC), u16_at(b, 0xBE));
        Ok(Save {
            name: b[0xB4..0xBC].try_into().unwrap(),
            cars,
            car_bits,
            car_extra: std::array::from_fn(|i| b[0x11C + 15 * i..0x11C + 15 * i + 15].try_into().unwrap()),
            cash: u32::from(b[0xC2] & 1) << 16 | u32::from(u16_at(b, 0xC0)),
            car: ((bc & 0x7FF) >> 7) as u8,
            zone: (bc & 7) as u8,
            slot: ((bc & 0x7F) >> 3) as u8,
            wingman: ((be & 0x7FF) >> 7) as u8,
            field_254: (b[0xBF] & 0x7F) >> 3,
            field_1f8: b[0x11A],
            field_f5: b[0x104..0x108].try_into().unwrap(),
            events: b[0x108..0x11A].try_into().unwrap(),
            best_times: std::array::from_fn(|i| u16_at(b, 0xC4 + 2 * i)),
            options: Options {
                camera: b[0xBD] >> 7,
                units: b[0xBE] & 1,
                hud: (b[0xBE] >> 1) & 1,
                transmission: (b[0xBE] >> 2) & 1,
                music: (b[0xBD] >> 3) & 3,
                sfx: (b[0xBD] & 0x7F) >> 5,
                language: (b[0xBE] & 0x7F) >> 4,
                catch_up: (b[0xBE] & 0xF) >> 3,
                mode_flags: b[0xC3] >> 4,
            },
            unlock_flags: b[0x11B] & 0x3F,
        })
    }

    /// 1 won, 2 second place, 3 not done (a new profile has every event at 3) (`FUN_08135d4c`).
    pub fn event_status(&self, event: usize) -> u8 {
        self.events[event >> 2] >> ((event & 3) * 2) & 3
    }

    /// The unlock bitfield (profile `+0x42D`, 40 bytes, bit `id` set = unlocked) that `FUN_08135958` rebuilds
    /// after loading: fixed starting unlocks, progress unlocks per zone, zone and boss unlocks
    /// (`FUN_0813589c`) and the `unlock_flags` ranges.
    pub fn unlocks(&self, rom: &[u8]) -> [u8; 40] {
        let mut bits = [0u8; 40];
        let mut set = |id: u16| bits[id as usize >> 3] |= 1 << (id & 7);
        const START: [u16; 40] = [
            0x108, 0x109, 0x10A, 0x72, 0x76, 0x117, 0x127, 0, 4, 8, 0xD, 0x11, 0x15, 0x1A, 0x1E, 0x22, 0x27, 0x2B,
            0x2F, 0x34, 0x38, 0x3C, 0x41, 0x45, 0x49, 0x4E, 0x52, 0x56, 0x5B, 0x5F, 0x63, 0x68, 0x6C, 0x71, 0x75, 0x79,
            0x89, 0x90, 0xA0, 0xBF,
        ];
        START.into_iter().chain(0xE0..=0x107).for_each(&mut set);
        match self.field_1f8 {
            0 => {}
            1 => set(0x128),
            _ => [0x128, 0x129].into_iter().for_each(&mut set),
        }
        let table = progress_unlocks(rom);
        for (zone, boss) in boss_events(rom).iter().enumerate() {
            let first = zone * 12;
            let mut key = first; // advances by one per counted event
            for i in first..first + if zone == 5 { 6 } else { 12 } {
                let counts = match self.event_status(i) {
                    1 => true,
                    2 => zone != 5 && !boss.contains(&(i as u16)),
                    _ => false,
                };
                if counts {
                    key += 1;
                    table
                        .iter()
                        .filter(|u| u.key as usize == key)
                        .flat_map(|u| &u.ids)
                        .for_each(|&id| set(id));
                }
            }
            if key - first > 6 {
                let z = zone as u16;
                if zone < 5 {
                    set(0x122 + z);
                    if self.event_status(boss[0] as usize) == 1 {
                        [0x11D + z, 0x10B + 2 * z].into_iter().for_each(&mut set);
                    }
                }
                if self.event_status(boss[1] as usize) == 1 && zone < 5 {
                    [0x10C + 2 * z, 0x118 + z, 0x12A + 2 * z, 0x12B + 2 * z]
                        .into_iter()
                        .for_each(&mut set);
                }
            }
        }
        let f = |bit: u8| self.unlock_flags >> bit & 1 != 0;
        if f(5) {
            (0x108..0x117).for_each(&mut set);
        }
        if f(4) {
            (0x127..0x135).for_each(&mut set);
        }
        if f(3) {
            (0x117..0x11D).for_each(&mut set);
        }
        if f(1) {
            (0x79..=0x107).for_each(&mut set);
        }
        if f(2) {
            (0..0x79).for_each(&mut set);
        }
        if f(0) {
            (0..6).flat_map(|i| [0x11D + i, 0x122 + i]).for_each(&mut set);
        }
        bits
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{canonical_rom, data_dir, text};

    fn rom() -> Option<Vec<u8>> {
        std::env::set_current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")).unwrap();
        canonical_rom()
            .map_err(|e| eprintln!("skipping: no ROM vault ({e})"))
            .ok()
    }

    /// Files from the reference run (`data/work/e5298b24/mgba/`); tests skip without them.
    fn reference(name: &str) -> Option<Vec<u8>> {
        std::fs::read(data_dir().join("work/e5298b24/mgba").join(name))
            .map_err(|e| eprintln!("skipping: no reference {name} ({e})"))
            .ok()
    }

    #[test]
    fn career_events_are_consistent() {
        let Some(rom) = rom() else { return };
        let events = events(&rom);
        let name = |e: &Event| text(&rom, track_name(&rom, e.track_slot()).0, Some(0));
        for e in &events {
            let sprint = e.mode == RaceMode::Sprint;
            assert_eq!(sprint, e.track >= 24);
            assert!(!sprint || (e.laps == 1 && !e.reverse));
            assert!(e.track < 12 || sprint);
            assert!(e.difficulty() <= 2 && e.traffic <= 2 && (1..=5).contains(&e.laps));
        }
        assert_eq!(
            (events[0].mode, name(&events[0]).as_str(), events[0].reward),
            (RaceMode::Circuit, "LONGPOINT", 100)
        );
        assert_eq!(
            (events[65].skill, events[65].difficulty(), name(&events[65]).as_str()),
            (100, 2, "SOUTH RUN")
        );
        assert_eq!(
            boss_events(&rom),
            [[7, 8], [19, 20], [31, 32], [43, 44], [55, 56], [66, 66]]
        );
        // Every track slot maps back to itself through the route number.
        for slot in 0..42 {
            assert_eq!(route_track_slot(&rom, track_name(&rom, slot).1), slot);
        }
        assert_eq!(text(&rom, u16_at(&rom, MODE_NAMES + 2 * 2) as usize, Some(0)), "HUNTER");
    }

    #[test]
    fn race_slots_and_sections_match_the_reference_race() {
        let Some(rom) = rom() else { return };
        let slots = race_slots(&rom);
        // Quick Play STORAGE RUN forward is route number 23: environment 11 and route 23 in the race's IWRAM.
        assert_eq!(track_name(&rom, route_track_slot(&rom, 23)).1, 23);
        assert_eq!((slots[23].environment, slots[23].route), (11, 23));
        if let Some(iw) = reference("race.iwram.bin") {
            assert_eq!((iw[0x6C], iw[0x5720], iw[0x5388]), (11, 23, 23));
        }
        assert!(
            slots[1..43]
                .iter()
                .enumerate()
                .all(|(i, s)| s.route as usize == i + 1 && s.environment < 12)
        );
        let sections = route_sections(&rom);
        assert!(sections[0].is_empty() && sections[1..].iter().all(|s| !s.is_empty() && s[0].first == 0));
        assert_eq!(
            sections[23].iter().map(|s| (s.count, s.first)).collect::<Vec<_>>(),
            [(36, 0), (8, 36)]
        );
        // The lap closes on its first waypoint; the lap length is the last main waypoint's distance.
        let line = ptr(&rom, ROUTE_TABLE + 0x14 * 23 + 8);
        assert_eq!(rom[line..line + 8], rom[line + 35 * 0x18..line + 35 * 0x18 + 8]);
        assert_eq!(u32_at(&rom, line + 35 * 0x18 + 0x10), 108_219);
    }

    #[test]
    fn setup_screens_and_the_reference_quick_play() {
        let Some(rom) = rom() else { return };
        let screens = setup_screens(&rom);
        let key = |k: u32| text(&rom, k as usize, Some(0));
        assert_eq!(key(screens[2].title_key.into()), "SETTING CIRCUIT ");
        let circuit: Vec<_> = screens[2]
            .items
            .iter()
            .map(|i| (key(i.text_key), i.setting, i.min, i.max))
            .collect();
        let want = [
            ("DIRECTION", 2, 0, 1),
            ("LAPS", 3, 1, 6),
            ("DIFFICULTY", 4, 0, 2),
            ("OPPONENTS", 5, 1, 3),
        ];
        for (got, want) in circuit.iter().zip(want) {
            assert_eq!((got.0.as_str(), got.1, got.2, got.3), want);
        }
        assert_eq!(screens[3].items[1].max, 3); // elimination: at most 3 laps
        assert_eq!(
            screens[0].items.iter().map(|i| key(i.options[1])).collect::<Vec<_>>()[..2],
            ["BUMPER", "KM/H"]
        );
        // The reference race (screenshot s11): forward, 3 laps, easy, 3 opponents, no traffic, catch-up off.
        let Some(iw) = reference("race.iwram.bin") else { return };
        let g = |a: usize| u32_at(&iw, a - 0x0300_0000);
        let got = [
            0x0300_56E0,
            0x0300_5610,
            0x0300_56E4,
            0x0300_5608,
            0x0300_5784,
            0x0300_5604,
            0x0300_0050,
        ]
        .map(g);
        assert_eq!(got, [0, 0, 3, 0, 3, 0, 0]);
    }

    #[test]
    fn wingmen_and_unlocks() {
        let Some(rom) = rom() else { return };
        let w = wingmen(&rom);
        let key = |k: u16| text(&rom, k as usize, Some(0));
        assert_eq!(
            (key(w[0].name_key).as_str(), key(w[0].role_key).as_str()),
            ("NONE", "NONE")
        );
        assert_eq!(
            (key(w[1].name_key).as_str(), key(w[1].role_key).as_str(), w[1].level),
            ("KITA", "ATTACKER", 0)
        );
        assert_eq!(
            (key(w[12].name_key).as_str(), key(w[12].role_key).as_str(), w[12].level),
            ("CLUTCH", "DRAFTER", 5)
        );
        let unlocks = progress_unlocks(&rom);
        assert!(unlocks.windows(2).all(|p| p[0].key <= p[1].key));
        assert_eq!((unlocks[0].key, &unlocks[0].ids[..]), (1, &[0x69, 0x6D][..]));
        assert_eq!(unlocks.last().map(|u| u.key), Some(66));
    }

    #[test]
    fn save_decodes_to_the_reference_ram() {
        let Some(sav) = reference("BN7E_v0_e5298b24.sav") else {
            return;
        };
        let buf = eeprom_to_buffer(&sav);
        let save = Save::parse(&buf).unwrap();
        assert_eq!(&save.name, b"A\0\0\0\0\0\0\0");
        assert!((0..EVENT_COUNT).all(|e| save.event_status(e) == 3));
        let (Some(iw), Some(ew)) = (reference("race.iwram.bin"), reference("race.wram.bin")) else {
            return;
        };
        let g32 = |a: usize| match a {
            0x0300_0000.. => u32_at(&iw, a - 0x0300_0000),
            _ => u32_at(&ew, a - 0x0200_0000),
        };
        let p = g32(0x0300_56EC) as usize - 0x0200_0000;
        let cars = g32(0x0300_539C) as usize - 0x0200_0000;
        assert_eq!(cars, p + 0xF9);
        assert_eq!(ew[p..p + 8], save.name);
        assert!((0..15).all(|i| ew[cars + 17 * i..cars + 17 * i + 17] == save.cars[i]));
        assert!((0..15).all(|i| ew[p + 0x14 + 15 * i..p + 0x14 + 15 * i + 15] == save.car_extra[i]));
        assert_eq!(u16_at(&ew, p + 0x12), save.car_bits);
        assert_eq!(u32_at(&ew, p + 0xC), save.cash);
        assert_eq!(
            [ew[p + 0x10], ew[p + 0x1FB], ew[p + 0x1FC], ew[p + 0x1F8]],
            [save.car, save.zone, save.slot, save.field_1f8]
        );
        assert_eq!(
            (u32_at(&ew, p + 0x200), i16_at(&ew, p + 0x254)),
            (save.wingman as u32, save.field_254 as i16)
        );
        assert_eq!(ew[p + 0xF5..p + 0xF9], save.field_f5);
        assert_eq!(ew[p + 0x205..p + 0x217], save.events);
        assert!((0..30).all(|i| u16_at(&ew, p + 0x218 + 2 * i) == save.best_times[i]));
        let o = save.options;
        let ram = [
            0x0300_53E4,
            0x0300_0040,
            0x0300_5698,
            0x0300_5798,
            0x0300_578C,
            0x0300_53A4,
            0x0300_5600,
        ];
        let want = [
            o.camera,
            o.units,
            o.hud,
            o.transmission,
            o.music << 3,
            o.sfx << 3,
            o.language,
        ];
        assert_eq!(ram.map(g32), want.map(u32::from));
        // Catch-up was saved at its new-profile default (on); the race setup turned it off without saving.
        assert_eq!((o.catch_up, g32(0x0300_0050)), (1, 0));
        assert_eq!(g32(0x0300_0070), u32::from(o.mode_flags));
        assert_eq!(
            save.cars.map(|c| c[6]),
            [5, 6, 11, 13, 1, 2, 11, 6, 14, 11, 6, 13, 0, 1, 19]
        );
        let Some(rom) = rom() else { return };
        assert_eq!(save.cars.map(|c| c[6]), rom[0x7E_EA33..0x7E_EA42]); // new-profile defaults (`FUN_081356dc`)
        assert_eq!(ew[p + 0x42D..p + 0x455], save.unlocks(&rom));
    }

    /// A valid save with every event not done.
    fn blank_save() -> Save {
        let mut b = [0u8; SAVE_SIZE];
        b[0x102] = 9;
        b[0x108..0x11A].fill(0xFF);
        let c = checksum(&b).to_le_bytes();
        b[0x100..0x102].copy_from_slice(&c);
        Save::parse(&b).unwrap()
    }

    #[test]
    fn progress_and_zone_unlocks() {
        let Some(rom) = rom() else { return };
        let mut save = blank_save();
        save.events[..2].copy_from_slice(&[0b01_01_01_01, 0b11_01_01_01]); // events 0..=6 won
        let bits = save.unlocks(&rom);
        let has = |id: u16| bits[id as usize >> 3] >> (id & 7) & 1 == 1;
        // Keys 1 and 7 of the progress table; seven wins open the zone's first boss race (0x122).
        assert!([0x69, 0x6D, 0xA4, 0x42, 0x122].into_iter().all(has));
        assert!(![0xA2, 0x11D, 0x10B].into_iter().any(has)); // key 8 and the boss-1 rewards are not reached
    }

    #[test]
    fn race_rules() {
        let mut save = blank_save();
        let events: Vec<Event> = (0..EVENT_COUNT)
            .map(|i| Event {
                zone: (i / 12) as u8,
                slot: (i % 12) as u8,
                skill: 0,
                track: 0,
                mode: RaceMode::Circuit,
                reverse: false,
                laps: 3,
                traffic: 0,
                reward: 100 + 25 * i as i16,
            })
            .collect();
        assert_eq!(race_payout(&mut save, &events, 0, 3, 1, 110), 110); // first win: next reward in line
        assert_eq!(save.event_status(3), 1);
        assert_eq!(race_payout(&mut save, &events, 0, 3, 1, 100), 50); // replay: half the last reward
        assert_eq!(race_payout(&mut save, &events, 0, 4, 2, 100), 62); // second place: half (125 / 2)
        assert_eq!(race_payout(&mut save, &events, 0, 5, 3, 100), 0);
        assert_eq!(save.cash, 222);
        assert_eq!(
            (reward_percent(100), reward_percent(101), reward_percent(176)),
            (100, 110, 140)
        );
        assert_eq!(race_progress(true, 108_219, 3, 2, 500), 108_719);
        assert_eq!(eliminated_position(3, 3, 3, 2), Some(4));
        assert_eq!(hunter_life_tick(HUNTER_LIFE_MAX - 10, 0, 0, 1, false), HUNTER_LIFE_MAX);
        assert_eq!(hunter_hit(0, 100, 0x100, false, true), (0x330, 0));
    }
}
