//! The race start (`race_init::race_start`) against the game's own code: for every capture in
//! `work/e5298b24/race-init/` (`tools/race_init_capture.py`: the machine at the entry of
//! `race_start_from_table_a`), the port must give the state the game's code gives in the function oracle
//! (`tools/race_init_oracle.py`, `NAME_oracle.*`), byte for byte in EWRAM, IWRAM, I/O, palette, VRAM and OAM.
//! Missing captures follow `nfsgba_testkit`'s rule (`NFSGBA_REQUIRE_DATA`).

use std::{fs, path::PathBuf};

use nfsgba_game::{Machine, race_init};

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

/// What the VBlank and VCount IRQs write while the race start runs (as `tools/race_init_oracle.py` lists them):
/// the sound engine's block, the counters, the mix buffers, the rand index (seeded from the tick counter the
/// IRQs advance; the seed timing itself is an input), the IRQ stack and the time-varying hardware registers.
fn irq_write(a: u32, engine: u32) -> bool {
    (engine..engine + 0x26AC).contains(&a)
        || [
            (0x0300_0044, 0x0300_0048),
            (0x0300_53B4, 0x0300_53B8),
            (0x0300_56E8, 0x0300_56EC),
            (0x0300_5724, 0x0300_572C),
            (0x0300_5DEC, 0x0300_5F4C),
            (0x0300_6378, 0x0300_6380),
            (0x0300_64C8, 0x0300_64CC),
            (0x0300_7B00, 0x0300_8000),
            (0x0400_0004, 0x0400_0008),
            (0x0400_00A0, 0x0400_00A8),
            (0x0400_00BC, 0x0400_00D4),
        ]
        .iter()
        .any(|&(lo, hi)| (lo..hi).contains(&a))
}

#[test]
fn race_start_matches_the_game() {
    let (Some(dir), Some(rom)) = (dir(), nfsgba_testkit::rom()) else {
        return;
    };
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
        let pre = dir.join(format!("{name}_pre"));
        let mut g = Machine::load_dump(rom.clone(), &pre).unwrap();
        let mut io: race_init::Io = fs::read(dir.join(format!("{name}_pre.io.bin"))).unwrap()[..0x400]
            .try_into()
            .unwrap();
        race_init::race_start(&mut g, &mut io, 0).unwrap();
        let want = |d: &str| fs::read(dir.join(format!("{name}_oracle.{d}.bin"))).unwrap();
        let mut report = Vec::new();
        for (d, ours, base) in [
            ("wram", &g.mem.ewram[..], 0x0200_0000),
            ("iwram", &g.mem.iwram[..], 0x0300_0000),
            ("io", &io[..], 0x0400_0000),
            ("palette", &g.palette[..], 0x0500_0000),
            ("vram", &g.vram[..], 0x0600_0000),
            ("oam", &g.oam[..], 0x0700_0000),
        ] {
            let runs = diff_runs(ours, &want(d), base);
            if !runs.is_empty() {
                let shown: Vec<String> = runs.iter().take(12).map(|(a, b)| format!("{a:#x}..={b:#x}")).collect();
                report.push(format!("  {d}: {} runs: {}", runs.len(), shown.join(" ")));
            }
        }
        // Against the emulator: with the recorded seed timing, every byte outside the IRQs' own writes.
        let seed = fs::read_to_string(dir.join(format!("{name}_seed.txt"))).expect("seed timing");
        {
            let mut g = Machine::load_dump(rom.clone(), &pre).unwrap();
            let mut io: race_init::Io = fs::read(dir.join(format!("{name}_pre.io.bin"))).unwrap()[..0x400]
                .try_into()
                .unwrap();
            race_init::race_start(&mut g, &mut io, seed.trim().parse().unwrap()).unwrap();
            let engine = g.mem.u32(0x0300_6370);
            let post = |d: &str| fs::read(dir.join(format!("{name}_post.{d}.bin"))).unwrap();
            for (d, ours, base) in [
                ("wram", &g.mem.ewram[..], 0x0200_0000u32),
                ("iwram", &g.mem.iwram[..], 0x0300_0000),
                ("io", &io[..], 0x0400_0000),
                ("palette", &g.palette[..], 0x0500_0000),
                ("vram", &g.vram[..], 0x0600_0000),
                ("oam", &g.oam[..], 0x0700_0000),
            ] {
                let want = post(d);
                let off: Vec<u32> = (0..ours.len())
                    .filter(|&i| ours[i] != want[i])
                    .map(|i| base + i as u32)
                    .filter(|&a| !irq_write(a, engine))
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
