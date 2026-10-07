//! The race start (`race_init::race_start`) against the game's own code: for every capture in
//! `work/e5298b24/race-init/` (`tools/race_init_capture.py`: the machine at the entry of
//! `race_start_from_table_a`), the typed `Setup` read from the pre-state gives the `World` that `World::load` reads
//! from the function oracle's result (`tools/race_init_oracle.py`, `NAME_oracle.*`), and the display memory (VRAM,
//! OAM, I/O) equals it byte for byte. With the recorded seed timing the rand index, racer slots and music request
//! are the emulator's (`NAME_post.*`). Missing captures follow `nfsgba_testkit`'s rule (`NFSGBA_REQUIRE_DATA`).

use std::{fs, path::PathBuf};

use nfsgba_game::{
    Machine, race_init,
    race_setup::{Display, Setup, load_pre},
    world::World,
};
use nfsgba_sim::data::GameData;

fn dir() -> Option<PathBuf> {
    nfsgba_testkit::fixture("race-init")
}

/// The second set (`race-init2/`, `NFSGBA_MGBA_SESSION=race-init2 tools/record.py race-init NAME`): conditions the first
/// never started: career events with the event AI skill set (a sprint at skill 65, the boss event at 4), a reversed
/// circuit, and the bumper camera.
const CAPTURES2: [&str; 4] = ["bumper", "career-boss", "career-late", "reverse"];

fn dir2() -> Option<PathBuf> {
    nfsgba_testkit::fixture("race-init2")
}

/// Every recorded race start (each with its seed timing, `NAME_seed.txt`).
const CAPTURES: [&str; 14] = [
    "career",
    "circuit",
    "circuitb",
    "elimination",
    "golf",
    "hunter",
    "hunterb",
    "ref",
    "refb",
    "rx7",
    "sprint",
    "sprintb",
    "wingman",
    "wingmanb",
];

fn diff_runs(a: &[u8], b: &[u8], base: u32) -> Vec<(u32, u32)> {
    let mut runs: Vec<(u32, u32)> = Vec::new();
    for i in (0..a.len()).filter(|&i| a[i] != b[i]) {
        let at = base + i as u32;
        match runs.last_mut() {
            Some(r) if at <= r.1 + 16 => r.1 = at,
            _ => runs.push((at, at)),
        }
    }
    runs
}

#[test]
fn race_start_matches_the_game() {
    race_start_matches(dir(), &CAPTURES);
}

#[test]
fn race_start_matches_the_game_in_more_conditions() {
    race_start_matches(dir2(), &CAPTURES2);
}

fn race_start_matches(dir: Option<PathBuf>, captures: &[&str]) {
    let (Some(dir), Some(rom)) = (dir, nfsgba_testkit::rom()) else {
        return;
    };
    let data = GameData::parse(&rom);
    let mut names: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| {
            e.ok()?
                .file_name()
                .to_str()?
                .strip_suffix("_oracle.wram.bin")
                .map(String::from)
        })
        .collect();
    names.sort();
    assert_eq!(
        names, captures,
        "the recorded race starts (NAME_oracle files: tools/race_init_oracle.py)"
    );
    let mut bad = Vec::new();
    for name in &names {
        let (setup, mut display): (Setup, Display) = load_pre(rom.clone(), &dir.join(format!("{name}_pre"))).unwrap();
        let world = race_init::race_start(&rom, &data, &setup, 0, &mut display).unwrap();
        let want = |d: &str| fs::read(dir.join(format!("{name}_oracle.{d}.bin"))).unwrap();
        let mut report = Vec::new();
        // The typed world against the oracle's: every field but the arena, then the arena as the game reads it.
        let mut oracle = World::load(&Machine::load_dump(rom.clone(), &dir.join(format!("{name}_oracle"))).unwrap());
        // The stale entity pointers the start drops (the previous race's wingman, read against this race's array).
        (oracle.g.wingman_car, oracle.g.wingman_target) = Default::default();
        let fields = race_init::differing(&world, &oracle);
        if !fields.is_empty() {
            report.push(format!(
                "  the typed world differs from the oracle's: {}",
                fields.join(" ")
            ));
            if fields.contains(&"g") {
                let (a, b) = (format!("{:?}", world.g), format!("{:?}", oracle.g));
                for (x, y) in a.split(", ").zip(b.split(", ")).filter(|(x, y)| x != y) {
                    report.push(format!("    g: {x} vs {y}"));
                }
            }
        }
        if race_init::arena_view(&world) != race_init::arena_view(&oracle) {
            report.push("  the heap arena (atlases, node table) differs".to_string());
        }
        for (d, ours, base) in [
            ("io", &display.io[..], 0x0400_0000),
            ("palette", &display.palette[..], 0x0500_0000),
            ("vram", &display.vram[..], 0x0600_0000),
            ("oam", &display.oam[..], 0x0700_0000),
        ] {
            let runs = diff_runs(ours, &want(d), base);
            if !runs.is_empty() {
                let shown: Vec<String> = runs.iter().take(12).map(|(a, b)| format!("{a:#x}..={b:#x}")).collect();
                report.push(format!("  {d}: {} runs: {}", runs.len(), shown.join(" ")));
            }
        }
        // Against the emulator, with the recorded seed timing.
        let seed = fs::read_to_string(dir.join(format!("{name}_seed.txt"))).expect("seed timing");
        {
            let (setup, mut display) = load_pre(rom.clone(), &dir.join(format!("{name}_pre"))).unwrap();
            let world = race_init::race_start(&rom, &data, &setup, seed.trim().parse().unwrap(), &mut display).unwrap();
            let post = World::load(&Machine::load_dump(rom.clone(), &dir.join(format!("{name}_post"))).unwrap());
            if (world.g.rand, world.g.results) != (post.g.rand, post.g.results) {
                report.push("  vs mGBA: the rand index or the racer slots differ".to_string());
            }
            let post_bytes = |d: &str| fs::read(dir.join(format!("{name}_post.{d}.bin"))).unwrap();
            for (d, ours, base) in [
                ("vram", &display.vram[..], 0x0600_0000u32),
                ("oam", &display.oam[..], 0x0700_0000),
            ] {
                let want = post_bytes(d);
                let off: Vec<u32> = (0..ours.len())
                    .filter(|&i| ours[i] != want[i])
                    .map(|i| base + i as u32)
                    .collect();
                if !off.is_empty() {
                    report.push(format!("  vs mGBA {d}: {} bytes, first {:#x}", off.len(), off[0]));
                }
            }
        }
        if report.is_empty() {
            eprintln!("{name}: race start exact (and equal to mGBA outside the IRQ writes)");
        } else {
            eprintln!("{name}: differs\n{}", report.join("\n"));
            bad.push(name.clone());
        }
    }
    assert!(bad.is_empty(), "race start differs for {bad:?}");
}

/// The menus and the race share IWRAM in the game; the session hands the menus' choice to the race start through
/// `Setup::menus` and `session::apply_choice`. On every recorded race start (the menus' state as `game_state_step`
/// state 4 found it), every word the menus' globals and the race's input both declare is what the race start read
/// in the game. (The career level and the reverse flag were missing: career opponents raced at skill 0.)
#[test]
fn the_race_start_takes_every_shared_word_from_the_menus() {
    shared_words(dir(), &CAPTURES);
}

#[test]
fn the_race_start_takes_every_shared_word_from_the_menus_in_more_conditions() {
    shared_words(dir2(), &CAPTURES2);
}

fn shared_words(dir: Option<PathBuf>, captures: &[&str]) {
    use nfsgba_game::world::LoopGlobals;
    use nfsgba_sim::{
        layout::{Field, Layout},
        mem::Mem,
        state::{Camera, CarGlobals, MenuGlobals, MenuState},
    };
    let (Some(rom), Some(dir)) = (nfsgba_testkit::rom(), dir) else {
        return;
    };
    // Race-side words of the start's input, by address.
    // The race HUD's settings (`world::Hud`, not a RAM layout): units, language, the HUD option.
    const HUD: &[(&str, u32, u32)] = &[
        ("hud.units", 0x0300_0040, 4),
        ("hud.language", 0x0300_5600, 4),
        ("hud.enabled", 0x0300_5698, 4),
    ];
    let race_fields: Vec<(&str, u32, u32)> = [
        <CarGlobals as Layout>::FIELDS,
        <Camera as Layout>::FIELDS,
        <LoopGlobals as Layout>::FIELDS,
        HUD,
    ]
    .concat();
    let image = |s: &Setup| {
        let mut m = Mem::new(rom.clone(), vec![0; 0x4_0000], vec![0; 0x8000]);
        s.g.store(&mut m, 0);
        s.camera.store(&mut m, 0);
        for (v, (_, at, _)) in [s.hud.units, s.hud.language, s.hud.enabled].into_iter().zip(HUD) {
            m.set_u32(*at, v);
        }
        s.lp.store(&mut m, 0);
        m
    };
    // Words the race start writes before anything reads them (the clock counters the session passes or the start
    // restarts, the level descriptor, the phase, the music, the camera yaw; state 4's pause, fade, race-over and
    // game state), the frame time (100, T1) and the division scratch: the captures hold the menus' or an earlier
    // race's values.
    const SET_BY_THE_START: [&str; 16] = [
        "ticks",
        "vblanks",
        "race_frames",
        "frame_ticks",
        "steps",
        "input",
        "descriptor",
        "phase",
        "music_id",
        "matrix_yaw",
        "paused",
        "fade",
        "race_over",
        "game_state",
        "dt",
        "div_rem",
    ];
    let mut shared = 0;
    let mut wrong = Vec::new();
    // Each capture as recorded, then with every shared word set to a test pattern (the captures hold few of the values
    // a player can choose: skill 0 in the career one, no reversed route, the chase camera).
    for (name, pattern) in captures.iter().flat_map(|n| [(n, false), (n, true)]) {
        let mut machine = Machine::load_dump(rom.clone(), &dir.join(format!("{name}_pre"))).unwrap();
        if pattern {
            for (k, &(_, at, size)) in <MenuGlobals as Layout>::FIELDS.iter().enumerate() {
                let covered = race_fields
                    .iter()
                    .any(|&(f, r, n)| r <= at && at + size <= r + n && !SET_BY_THE_START.contains(&f));
                if covered {
                    let bytes: Vec<u8> = (0..size).map(|i| (k as u8).wrapping_mul(37) ^ (i as u8) | 1).collect();
                    machine.mem.set_bytes(at, &bytes);
                }
            }
        }
        let st = MenuState::load(&machine.mem);
        let want = Setup::load(&machine);
        let mut ours = Setup::menus(&rom, want.audio.clone(), 0, 0, &st.profile.car_records, None);
        nfsgba_game::session::apply_choice(&mut ours, &st);
        let (a, b) = (image(&ours), image(&want));
        for &(field, at, size) in <MenuGlobals as Layout>::FIELDS {
            let Some(&(race, ..)) = race_fields.iter().find(|&&(_, r, n)| r <= at && at + size <= r + n) else {
                continue;
            };
            if SET_BY_THE_START.contains(&race) {
                continue;
            }
            shared += 1;
            if a.bytes(at, size as usize) != b.bytes(at, size as usize) {
                wrong.push(
                    format!("{name}{}: {field}", if pattern { " (pattern)" } else { "" })
                        + &format!(
                            " / {race} at {at:#x}: ours {:02x?}, the game's {:02x?}",
                            a.bytes(at, size as usize),
                            b.bytes(at, size as usize)
                        ),
                );
            }
        }
    }
    assert!(shared > captures.len() * 20, "{shared} shared words compared");
    assert!(wrong.is_empty(), "{} words differ:\n{}", wrong.len(), wrong.join("\n"));
}
