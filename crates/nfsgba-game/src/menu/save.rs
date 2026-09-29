//! The profile and its save (`docs/formats/career.md`): `profile_reset` (`0x081356DC`), `save_probe_profile`
//! (`0x08149B94`), `save_load_profile` (`0x08149D84`), `save_write_profile` (`0x08149FD8`) on typed state.
//! The EEPROM is the `.sav` image (512 bytes per slot, [`eeprom_to_buffer`] order); the game only uses slot 0.

use nfsgba_formats::career::{SAVE_SIZE, SAVE_VERSION, Save, checksum, eeprom_to_buffer};
use nfsgba_sim::state::MenuState;

use super::flow::Host;

const CARBON_PLAY_MUSIC: u32 = 0x0813_6054;
const SOUND_REINIT: u32 = 0x0813_5EA4; // (): LS_Play init with the volume globals
const CARBON_SOUND_INIT: u32 = 0x0813_5DF8;
const NEW_PROFILE_CAR_BYTES: u32 = 0x087E_EA33; // car record byte 6 of each new-profile car

/// The bits `save_encode` reads, from the profile and the option globals.
pub fn to_save(st: &MenuState) -> Save {
    let (p, g) = (&st.profile, &st.g);
    let flag = |i: usize| (p.stats[i] & 1) as u8;
    Save {
        name: p.name[..8].try_into().unwrap(),
        cars: p.car_records,
        car_bits: p.u_12,
        car_extra: p.car_extra,
        cash: p.cash as u32,
        car: p.career_car as u8,
        zone: p.zone,
        slot: p.event_slot,
        wingman: p.wingman as u8,
        field_254: p.map_grid as u8,
        field_1f8: p.hints_a,
        field_f5: p.field_f5,
        events: p.events[..18].try_into().unwrap(),
        best_times: p.records,
        options: nfsgba_formats::career::Options {
            camera: g.u_53e4 as u8,
            units: g.units as u8,
            hud: g.hud_on as u8,
            transmission: g.u_5798 as u8,
            music: (g.music_volume >> 3 & 3) as u8,
            sfx: (g.sound_volume >> 3 & 3) as u8,
            language: g.language as u8,
            catch_up: g.u_0050 as u8,
            mode_flags: g.mode_bits as u8,
        },
        unlock_flags: flag(1) | flag(2) << 1 | flag(3) << 2 | flag(5) << 3 | flag(4) << 4 | flag(0) << 5,
    }
}

/// `rebuild_unlocks` (`0x08135958`): the 40 unlock bytes from the events, hint and flag fields.
pub fn rebuild_unlocks(st: &mut MenuState, rom: &[u8]) {
    let bits = to_save(st).unlocks(rom);
    st.profile.unlocks.copy_from_slice(&bits[..32]);
    st.profile.unlocks_more[..8].copy_from_slice(&bits[32..]);
}

/// `save_decode` (`0x08149820`): a 512-byte buffer into the profile and the option globals, then the unlocks.
pub fn decode(st: &mut MenuState, rom: &[u8], buf: &[u8; SAVE_SIZE]) {
    let s = Save::decode(buf);
    let (p, g) = (&mut st.profile, &mut st.g);
    p.name[..8].copy_from_slice(&s.name);
    p.name[8] = 0;
    p.car_records = s.cars;
    p.u_12 = s.car_bits;
    p.car_extra = s.car_extra;
    p.events[..18].copy_from_slice(&s.events);
    p.records = s.best_times;
    p.field_f5 = s.field_f5;
    p.career_car = s.car as i8;
    p.hints_a = s.field_1f8;
    p.cash = s.cash as i32;
    p.wingman = u32::from(s.wingman);
    p.map_grid = u16::from(s.field_254);
    p.zone = s.zone;
    p.event_slot = s.slot;
    let f = |bit: u8| u32::from(s.unlock_flags >> bit & 1);
    p.stats = [f(5), f(0), f(1), f(2), f(4), f(3)];
    let o = s.options;
    g.u_53e4 = u32::from(o.camera);
    g.units = u32::from(o.units);
    g.hud_on = u32::from(o.hud);
    g.u_5798 = u32::from(o.transmission);
    g.music_volume = u32::from(o.music) << 3;
    g.sound_volume = u32::from(o.sfx) << 3;
    g.u_0050 = u32::from(o.catch_up);
    g.mode_bits = u32::from(o.mode_flags);
    rebuild_unlocks(st, rom);
}

/// `save_encode` (`0x081492C0`) into a zeroed buffer (`save_write_profile` allocates with `heap_alloc_zeroed`).
pub fn encode(st: &MenuState) -> [u8; SAVE_SIZE] {
    to_save(st).encode(&[0; SAVE_SIZE])
}

/// `profile_reset` (`0x081356DC`): a new profile's defaults; nothing when the name screen was opened from the
/// settings (`u_494` = 2).
pub fn profile_reset(st: &mut MenuState, rom: &[u8]) {
    if st.profile.u_494 == 2 {
        return;
    }
    let g = &mut st.g;
    let p = &mut st.profile;
    p.name[0] = 0;
    g.units = u32::from(g.language != 0);
    (g.u_53e4, g.hud_on, g.u_5798, g.mode_bits) = (0, 1, 1, 0);
    (g.music_volume, g.sound_volume, g.u_0050) = (0x10, 0x10, 1);
    (p.hints_b, p.zone_step, p.upgrades_saving) = (0, 0, 0);
    p.events[..18].fill(0xFF);
    p.car_records = [[0; 17]; 15];
    p.field_f5 = [0; 4];
    p.car_extra = [[0; 15]; 15];
    p.u_12 = 0;
    (p.hints_a, p.zone, p.event_slot, p.career_car, p.wingman, p.map_grid) = (0, 0, 0, 0, 0, 0);
    for t in p.records.iter_mut() {
        let r = nfsgba_fixed::rand_table(rom, &mut g.rand_index);
        *t = (r % 0xE10).wrapping_add(0x2A30) as u16;
    }
    p.cash = 0;
    rebuild_unlocks(st, rom);
    let base = (NEW_PROFILE_CAR_BYTES & 0x1FF_FFFF) as usize;
    for (i, c) in st.profile.car_records.iter_mut().enumerate() {
        c[6] = rom[base + i];
    }
}

/// The 512 bytes of `slot` in game order, if the image holds them.
fn slot_buffer(eeprom: &[u8], slot: usize) -> Option<[u8; SAVE_SIZE]> {
    let raw = eeprom.get(slot * SAVE_SIZE..(slot + 1) * SAVE_SIZE)?;
    Some(eeprom_to_buffer(raw))
}

/// A slot's buffer if its checksum matches (`save_load_profile`).
fn valid(eeprom: &[u8], slot: usize) -> Option<[u8; SAVE_SIZE]> {
    slot_buffer(eeprom, slot).filter(|b| checksum(b) == u16::from_le_bytes([b[0x100], b[0x101]]))
}

/// `save_probe_profile` (`0x08149B94`), the boot check: a slot with a good checksum and version 9 gives the
/// profile its saved name and language (`+0x4E8`).
pub fn probe(st: &mut MenuState, eeprom: &[u8], slot: usize) -> u32 {
    match valid(eeprom, slot).filter(|b| u16::from_le_bytes([b[0x102], b[0x103]]) == SAVE_VERSION) {
        Some(b) => {
            st.profile.slot_names[slot][..8].copy_from_slice(&b[0xB4..0xBC]);
            st.profile.slot_names[slot][8] = 0;
            st.profile.saved_language = u16::from((b[0xBE] & 0x7F) >> 4);
            1
        }
        None => {
            st.profile.slot_names[slot][0] = 0;
            0
        }
    }
}

/// `save_load_profile` (`0x08149D84`): decodes the slot (1), or, when its checksum is wrong, starts a new
/// profile and writes it (0). Both restart the sound (`FUN_08135EA4`) and the music.
pub fn load(st: &mut MenuState, h: &mut impl Host, eeprom: &mut Vec<u8>, slot: usize) -> u32 {
    let music = st.g.music_id;
    let ok = match valid(eeprom, slot) {
        Some(b) => {
            decode(st, h.rom(), &b);
            st.profile.loaded = 1;
            st.profile.slot_names[slot][..8].copy_from_slice(&b[0xB4..0xBC]);
            st.profile.slot_names[slot][8] = 0;
            true
        }
        None => {
            st.profile.slot_names[slot][0] = 0;
            st.profile.loaded = 0;
            st.profile.stats = [0; 6];
            profile_reset(st, h.rom());
            write(st, h, eeprom, slot);
            false
        }
    };
    h.call(SOUND_REINIT, &[]);
    h.call(CARBON_PLAY_MUSIC, &[music]);
    u32::from(ok)
}

/// `save_write_profile` (`0x08149FD8`): encodes the profile into the slot; the profile then has a saved name and
/// exists. Returns 1 (a failed EEPROM write, which the image never has, would return 0).
pub fn write(st: &mut MenuState, h: &mut impl Host, eeprom: &mut Vec<u8>, slot: usize) -> u32 {
    let music = st.g.music_id;
    let buf = encode(st);
    if eeprom.len() < (slot + 1) * SAVE_SIZE {
        eeprom.resize((slot + 1) * SAVE_SIZE, 0xFF);
    }
    eeprom[slot * SAVE_SIZE..(slot + 1) * SAVE_SIZE].copy_from_slice(&eeprom_to_buffer(&buf));
    h.call(CARBON_SOUND_INIT, &[]);
    h.call(CARBON_PLAY_MUSIC, &[music]);
    st.profile.slot_names[slot][..8].copy_from_slice(&buf[0xB4..0xBC]);
    st.profile.slot_names[slot][8] = 0;
    // ponytail: the exists flag is one halfword at +0x490 (slot 0); the game never saves to another slot.
    st.profile.profile_exists = 1;
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::{Gba, adapt, boot::BootHost, map};
    use nfsgba_testkit::{dump, rom};

    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len() / 2)
            .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
            .collect()
    }

    const BUF: u32 = 0x0201_0000;

    /// Real saves (recorded `.sav`/EEPROM dumps of career and Quick Play sessions): bytes to typed profile to bytes
    /// is the identity, also through the slot reader and writer (what a save written by our code holds).
    #[test]
    fn real_saves_round_trip() {
        const SAVES: [&str; 18] = [
            "race-rules/rr-career.sav",
            "race-rules/rr-after1.sav",
            "ai-traffic/BN7E_v0_e5298b24.sav",
            "audio/BN7E_v0_e5298b24.sav",
            "car-atlas/BN7E_v0_e5298b24.sav",
            "car-paint/BN7E_v0_e5298b24.sav",
            "entity-draw/BN7E_v0_e5298b24.sav",
            "game-loop/BN7E_v0_e5298b24.sav",
            "harness/BN7E_v0_e5298b24.sav",
            "hud-logic/BN7E_v0_e5298b24.sav",
            "live-race/BN7E_v0_e5298b24.sav",
            "mgba/BN7E_v0_e5298b24.sav",
            "physics-paths/BN7E_v0_e5298b24.sav",
            "race-init/BN7E_v0_e5298b24.sav",
            "race-rules/BN7E_v0_e5298b24.sav",
            "sky/BN7E_v0_e5298b24.sav",
            "ui-2d/BN7E_v0_e5298b24.sav",
            "vehicle-physics/BN7E_v0_e5298b24.sav",
        ];
        let Some(rom) = rom() else { return };
        let mut good = 0;
        for name in SAVES {
            let Some(sav) = nfsgba_testkit::read(name) else {
                continue;
            };
            let Some(buf) = valid(&sav, 0).filter(|b| b[0x102] == SAVE_VERSION as u8) else {
                continue;
            };
            let mut st = MenuState::default();
            decode(&mut st, &rom, &buf);
            assert!(
                encode(&st) == buf,
                "{name}: the typed profile does not re-encode to the save"
            );
            let mut h = BootHost::new(rom.clone(), sav.clone());
            let mut image = sav.clone();
            assert_eq!(write(&mut st, &mut h, &mut image, 0), 1);
            assert_eq!(image[..SAVE_SIZE], sav[..SAVE_SIZE], "{name}");
            let mut loaded = MenuState::default();
            assert_eq!(load(&mut loaded, &mut h, &mut image, 0), 1);
            assert_eq!(loaded.profile.name, st.profile.name, "{name}");
            good += 1;
        }
        assert!(good >= 8, "only {good} real saves round-trip");
    }

    /// `tools/oracle/cases.py save`: the game's `save_decode`, `save_encode` and `profile_reset` on 1,800 generated
    /// buffers and profiles, and `map_zone_palettes` on 600 map states; the typed versions leave the same bytes in RAM.
    #[test]
    fn save_functions_match_the_game() {
        let (Some(rom), Some(text)) = (rom(), nfsgba_testkit::read_to_string("menus3/save.jsonl")) else {
            return;
        };
        let mut snaps = std::collections::HashMap::new();
        let mut n = 0;
        for line in text.lines() {
            let c: serde_json::Value = serde_json::from_str(line).unwrap();
            let snap = c["snap"].as_str().unwrap();
            let base = snaps
                .entry(snap.to_owned())
                .or_insert_with(|| Gba::from_dump(rom.clone(), &dump(snap).unwrap()).unwrap());
            let (mut ours, mut want) = (base.clone(), base.clone());
            let poke = |g: &mut Gba, at: u64, b: &[u8]| {
                b.iter()
                    .enumerate()
                    .for_each(|(i, &v)| g.set_u8(at as u32 + i as u32, v))
            };
            for m in c["mem"].as_array().unwrap() {
                let b = unhex(m[1].as_str().unwrap());
                poke(&mut ours, m[0].as_u64().unwrap(), &b);
                poke(&mut want, m[0].as_u64().unwrap(), &b);
            }
            for w in c["writes"].as_array().unwrap() {
                poke(&mut want, w[0].as_u64().unwrap(), &unhex(w[1].as_str().unwrap()));
            }
            let buf: [u8; SAVE_SIZE] = std::array::from_fn(|i| ours.u8(BUF + i as u32));
            let f = c["fn"].as_str().unwrap();
            let encoded = adapt::typed(&mut ours, |st, h| match f {
                "0x8149820" => {
                    decode(st, &rom, &buf);
                    None
                }
                "0x81492c0" => Some(to_save(st).encode(&buf)), // the game encodes into the buffer in place
                "0x81356dc" => {
                    profile_reset(st, &rom);
                    None
                }
                "0x8143284" => {
                    map::zone_palettes(st, h);
                    None
                }
                _ => panic!("{f}"),
            });
            if let Some(out) = encoded {
                out.iter()
                    .enumerate()
                    .for_each(|(i, &v)| ours.set_u8(BUF + i as u32, v));
            }
            assert!(
                ours.ewram == want.ewram && ours.iwram == want.iwram,
                "case {n}: {f}\n{line}"
            );
            n += 1;
        }
        assert_eq!(n, 2400);
    }
}
