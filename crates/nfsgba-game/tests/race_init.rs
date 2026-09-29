//! The race start (`race_init::race_start`) against the game's own code: for every capture in
//! `work/e5298b24/race-init/` (`tools/race_init_capture.py`: the machine at the entry of
//! `race_start_from_table_a`), the port must give the state the game's code gives in the function oracle
//! (`tools/race_init_oracle.py`, `NAME_oracle.*`), byte for byte in EWRAM, IWRAM, I/O, palette, VRAM and OAM.
//! Skipped when the captures are absent.

use std::{fs, path::PathBuf};

use nfsgba_game::{Machine, race_init};

fn dir() -> Option<PathBuf> {
    let d = nfsgba_formats::data_dir().join("work/e5298b24/race-init");
    d.exists().then_some(d)
}

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
    let Some(dir) = dir() else {
        eprintln!("race-init captures not found; skipped");
        return;
    };
    let rom = nfsgba_formats::canonical_rom().expect("ROM vault");
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
    assert!(!names.is_empty(), "no NAME_oracle files: run tools/race_init_oracle.py");
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
        if report.is_empty() {
            eprintln!("{name}: race start exact");
        } else {
            eprintln!("{name}: differs\n{}", report.join("\n"));
            bad.push(name.clone());
        }
    }
    assert!(bad.is_empty(), "race start differs for {bad:?}");
}
