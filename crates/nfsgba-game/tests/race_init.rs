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
    let (Some(dir), Some(rom)) = (dir(), nfsgba_testkit::rom()) else {
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
        names, CAPTURES,
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
