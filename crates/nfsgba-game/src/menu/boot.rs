//! The boot path on typed state: `game_main`'s profile setup (`0x0812A950`), then `main_frame` until the main menu.
//! [`BootHost`] is a host with typed saves and no drawing: every drawing call, sound call and hardware access is
//! dropped (FIDELITY U7), so it runs the menu logic of the language, health, logo, title and name screens and the
//! main menu on its own. A power-on run in headless mGBA (`tools/boot_trace.py`) checks the screen numbers, the
//! profile and the language frame by frame.

use nfsgba_sim::state::MenuState;

use super::Kind;
use super::flow::{self, Host};
use super::save;

/// The typed part of `game_main`: a new profile, then the EEPROM's slot 0 probed (`save_probe_profile`): a valid
/// save marks the profile as present and sets the language it was saved with. The first screen is the language
/// menu (0x19).
pub fn init(st: &mut MenuState, rom: &[u8], eeprom: &[u8]) {
    *st = MenuState::default();
    st.g.back_top = -1;
    st.g.message_box = -1;
    st.profile.u_494 = 0;
    save::profile_reset(st, rom);
    st.g.save_buffer = 0;
    st.profile.profile_exists = 0;
    if save::probe(st, eeprom, 0) != 0 {
        st.profile.profile_exists = 1;
        st.g.language = u32::from(st.profile.saved_language);
    }
    st.g.screen = 0x19;
}

/// A host for the menus without a screen: typed saves in `eeprom` (the `.sav` image), everything else dropped.
pub struct BootHost {
    pub rom: Vec<u8>,
    pub eeprom: Vec<u8>,
    colours: Vec<u16>,
}

impl BootHost {
    pub fn new(rom: Vec<u8>, eeprom: Vec<u8>) -> Self {
        BootHost {
            rom,
            eeprom,
            colours: vec![0; 256],
        }
    }

    /// One `main_frame`, with the frame's keys and tick counter (the vblank IRQ's and the pad's inputs).
    pub fn frame(&mut self, st: &mut MenuState, keys: u16, ticks: i32) {
        st.g.keys = keys;
        st.g.ticks = ticks;
        flow::main_frame(st, self);
    }
}

impl Host for BootHost {
    fn rom(&self) -> &[u8] {
        &self.rom
    }
    fn call(&mut self, _function: u32, _args: &[u32]) -> u32 {
        0
    }
    fn text_arg(&mut self, _s: Vec<u8>) -> u32 {
        0
    }
    fn handler(&mut self, st: &mut MenuState, kind: Kind, phase: usize, args: &[u32]) -> u32 {
        if flow::is_typed(kind, phase) {
            flow::run_typed(st, self, kind, phase, args)
        } else {
            u32::from(phase == 2) // ponytail: the garage screens (Kind18) are not ported (U3)
        }
    }
    fn scene_setup(&mut self, st: &mut MenuState, _material: u32, _palette: u32, _sprite: u32) {
        self.scene_setup_ab(st, st.g.screen_entered == 0, 0, 0, 0);
    }
    fn scene_setup_ab(&mut self, st: &mut MenuState, first: bool, _material: u32, _palette: u32, _sprite: u32) {
        if first {
            st.g.menu_exit = 0;
        }
        st.g.palette_dirty = 1;
    }
    fn world_palette(&self) -> u32 {
        0
    }
    fn page_buffer(&self) -> u32 {
        0
    }
    fn clear_frame_buffers(&mut self) {}
    fn fill_page(&mut self) {}
    fn peek16(&self, addr: u32) -> u16 {
        match addr >> 24 {
            8 => flow::rom_u16(&self.rom, addr),
            _ => 0,
        }
    }
    fn second_colour(&self, i: u32) -> u16 {
        self.colours[i as usize & 0xFF]
    }
    fn set_second_colour(&mut self, i: u32, c: u16) {
        self.colours[i as usize & 0xFF] = c;
    }
    fn black_bg_palette(&mut self) {}
    fn copy_mem(&mut self, _dst: u32, _src: u32, _n: u32, _width: u32) {}
    fn fade_step(&mut self, st: &mut MenuState) {
        st.g.fade = if st.g.fade > 0 {
            (st.g.fade - 2).max(0)
        } else {
            (st.g.fade + 2).min(0)
        };
    }
    fn button_prompts(&mut self, _st: &MenuState, _args: &[u32; 3]) {}
    fn message_box_draw(&mut self, _st: &mut MenuState) {}
    fn profile_reset(&mut self, st: &mut MenuState) {
        save::profile_reset(st, &self.rom);
    }
    fn save_load(&mut self, st: &mut MenuState) -> u32 {
        let mut eeprom = std::mem::take(&mut self.eeprom);
        let r = save::load(st, self, &mut eeprom, st.g.save_buffer as usize);
        self.eeprom = eeprom;
        r
    }
    fn save_write(&mut self, st: &mut MenuState) -> u32 {
        let mut eeprom = std::mem::take(&mut self.eeprom);
        let r = save::write(st, self, &mut eeprom, st.g.save_buffer as usize);
        self.eeprom = eeprom;
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nfsgba_testkit::rom;
    use serde_json::Value;

    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len() / 2)
            .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
            .collect()
    }

    /// The typed state as `tools/boot_trace.py` reads it from the game's RAM: screen, state, back stack top,
    /// language, fade, exit, message box, units, profile exists, loaded, name.
    fn row_of(st: &MenuState) -> Vec<i64> {
        let (g, p) = (&st.g, &st.profile);
        let mut r = vec![
            g.screen as i64,
            g.game_state as i64,
            g.back_top as i64,
            g.language as i64,
        ];
        r.extend([g.fade as i64, g.menu_exit as i64, g.message_box as i64, g.units as i64]);
        r.extend([p.profile_exists as i64, p.loaded as i64]);
        r.extend(p.name.iter().map(|&b| b as i64));
        r
    }

    fn want(f: &Value) -> Vec<i64> {
        let n = |k: &str| f[k].as_i64().unwrap();
        let mut r = vec![
            n("screen"),
            n("state"),
            n("back_top"),
            n("language"),
            n("fade"),
            n("menu_exit"),
        ];
        r.extend([n("message_box"), n("units"), n("exists"), n("loaded")]);
        r.extend(unhex(f["name"].as_str().unwrap()).iter().map(|&b| b as i64));
        r
    }

    /// The first frame of `main_frame` (state 0 enters the language screen; the game sets up the profile and the sound before).
    const FIRST: usize = 17;

    /// A power-on run in headless mGBA (`tools/boot_trace.py`, `boot/NAME.json`), one row per video frame. The game
    /// runs `main_frame` back to back, several times per video frame when idle and once per several when a screen
    /// change draws; the row's counter change (less the vblank IRQ's own increment) says how many completed. The
    /// typed boot runs that many with the previous row's tick counter, and each key press of the run's script
    /// arrives as one edge in its first frame (the game's key variable holds the newest edge, so the trace's own
    /// column loses presses). A screen change takes the game several video frames (T1) while the port's frame is
    /// atomic, so the rows from 3 before to 12 after a screen change of either run are not compared; on every other
    /// row the screen, state, back stack, language, profile and units are equal, both runs visit the same
    /// screens, and the EEPROM at the end is the game's.
    fn boot_matches(name: &str) {
        let (Some(rom), Some(text)) = (rom(), nfsgba_testkit::read_to_string(&format!("boot/{name}.json"))) else {
            return;
        };
        let t: Value = serde_json::from_str(&text).unwrap();
        let frames = t["frames"].as_array().unwrap();
        let mut eeprom = unhex(t["start_eeprom"].as_str().unwrap());
        if eeprom.is_empty() {
            eeprom = vec![0xFF; 512]; // a cartridge without a save
        }
        let mut edges = std::collections::BTreeMap::<usize, u16>::new();
        for press in t["script"].as_array().unwrap() {
            let key = ["A", "B", "SELECT", "START", "RIGHT", "LEFT", "UP", "DOWN", "R", "L"]
                .iter()
                .position(|k| *k == press[2].as_str().unwrap())
                .unwrap();
            *edges.entry(press[0].as_u64().unwrap() as usize).or_default() |= 1 << key;
        }
        let mut st = MenuState::default();
        init(&mut st, &rom, &eeprom);
        let mut host = BootHost::new(rom, eeprom);
        let (mut ours, mut pending) = (vec![], 0);
        for f in FIRST..frames.len() {
            pending |= edges.get(&f).copied().unwrap_or(0);
            let (row, last) = (&frames[f], &frames[f - 1]);
            let bumped = row["irq_a"] == 0 && row["irq_b"] == 2;
            let done = (row["count"].as_i64().unwrap() - last["count"].as_i64().unwrap() - i64::from(bumped)).max(0);
            for _ in 0..done {
                host.frame(
                    &mut st,
                    std::mem::take(&mut pending),
                    last["ticks"].as_i64().unwrap() as i32,
                );
            }
            ours.push(row_of(&st));
        }
        let game: Vec<Vec<i64>> = frames[FIRST..].iter().map(want).collect();
        let mut transient = vec![false; game.len()];
        let changes = |run: &[Vec<i64>]| {
            (1..run.len())
                .filter(|&c| run[c][0] != run[c - 1][0])
                .collect::<Vec<_>>()
        };
        for (o, g) in changes(&ours).into_iter().zip(changes(&game)) {
            // The k-th change of each run: between the two (a save blocks the game for 70 frames) and around them.
            transient[o.min(g).saturating_sub(3)..(o.max(g) + 12).min(game.len())].fill(true);
        }
        for (i, (o, g)) in ours.iter().zip(&game).enumerate() {
            assert!(
                transient[i] || o == g,
                "{name} frame {}: ours {o:?}, game {g:?}",
                i + FIRST
            );
        }
        let screens = |run: &[Vec<i64>]| -> Vec<i64> {
            let mut v: Vec<i64> = run.iter().map(|r| r[0]).collect();
            v.dedup();
            v
        };
        assert_eq!(screens(&ours), screens(&game), "{name}: the screens visited");
        assert!(screens(&game).len() >= 8, "{name}: too few screens");
        // The EEPROM (game order): a new profile draws its 30 record times from the random sequence, which the menus'
        // frame count moves (T1); those bytes (0xC4..0x100) and the checksum (0x100) differ, the rest is equal.
        use nfsgba_formats::career::{checksum, eeprom_to_buffer};
        let (mine, theirs) = (
            eeprom_to_buffer(&host.eeprom),
            eeprom_to_buffer(&unhex(t["end_eeprom"].as_str().unwrap())),
        );
        let rest = |b: &[u8]| [&b[..0xC4], &b[0x102..]].concat();
        assert_eq!(rest(&mine), rest(&theirs), "{name}: the EEPROM at the end");
        assert_eq!(u16::from_le_bytes([mine[0x100], mine[0x101]]), checksum(&mine));
        if name == "fresh" {
            // A profile our code writes: `boot_trace.py ours boot/ours.sav` boots it in mGBA (checked by
            // `tools/test_boot_trace.py`): the name ZED and the options reach the menu.
            let mut edited = st.clone();
            edited.profile.name = *b"ZED\0\0\0\0\0\0";
            edited.profile.cash = 12345;
            edited.g.units = 1;
            edited.profile.events[..3].copy_from_slice(&[1, 2, 1]);
            let mut image = host.eeprom.clone();
            save::write(&mut edited, &mut host, &mut image, 0);
            let dir = nfsgba_testkit::work_dir().unwrap().join("boot");
            std::fs::write(dir.join("ours.sav"), image).unwrap();
        }
    }

    #[test]
    fn boot_matches_the_game() {
        for name in ["fresh", "existing"] {
            boot_matches(name);
        }
    }
}
