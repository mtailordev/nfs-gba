//! Reads the cases `tools/oracle/prove.py` saved from the function oracle (the game's own code run in unicorn)
//! and checks the Rust ports give identical results. Exit code 1 on any mismatch.

use std::{fs, process::ExitCode};

use nfsgba_formats as rom;
use serde_json::Value;

fn cases(name: &str) -> Vec<Value> {
    let path = rom::data_dir().join(format!("work/e5298b24/harness/oracle/{name}.jsonl"));
    let text =
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e} (run tools/oracle/prove.py)", path.display()));
    text.lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

fn int(v: &Value) -> i64 {
    v.as_i64().unwrap()
}

/// `nfsgba_formats`'s private `div` (lib.rs), copied verbatim: libgcc `__divsi3` as the game calls it.
fn div(a: i32, b: i32) -> i32 {
    if b == 0 { 0 } else { a.wrapping_div(b) }
}

fn report(name: &str, total: usize, bad: usize) -> bool {
    println!("{name}: {total} cases, {bad} mismatches");
    bad == 0
}

fn main() -> ExitCode {
    let data = rom::canonical_rom().expect("no ROM vault");
    let mut ok = true;

    let c = cases("divsi3");
    let bad = c
        .iter()
        .filter(|c| div(int(&c["a"]) as i32, int(&c["b"]) as i32) as i64 != int(&c["q"]))
        .count();
    ok &= report("__divsi3 vs div", c.len(), bad);

    let c = cases("sin_q14");
    let bad = c
        .iter()
        .filter(|c| rom::paint::sin_q14(&data, int(&c["angle"]) as i32) as i64 != int(&c["sin"]))
        .count();
    ok &= report("sin_q14 vs paint::sin_q14", c.len(), bad);

    // apply_sector_light_to_palette: palette RAM after the call. The Rust side tints the base buffer
    // (*0x030055F0) where sector_light finds light, and leaves the snapshot's palette RAM otherwise.
    let dir = rom::data_dir().join("work/e5298b24/mgba");
    let (iwram, wram, pal) = (
        fs::read(dir.join("race.iwram.bin")).unwrap(),
        fs::read(dir.join("race.wram.bin")).unwrap(),
        fs::read(dir.join("race.palette.bin")).unwrap(),
    );
    let u16s = |b: &[u8]| {
        b.chunks(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect::<Vec<u16>>()
    };
    let at = u32::from_le_bytes(iwram[0x55F0..0x55F4].try_into().unwrap()) as usize - 0x0200_0000;
    let (base, ram) = (u16s(&wram[at..at + 512]), u16s(&pal[..512]));
    let city = rom::city(&data);
    let c = cases("sector_light");
    let bad = c
        .iter()
        .filter(|c| {
            let s = &city[int(&c["sector"]) as usize];
            let (x, z) = (int(&c["x"]) as i32 >> 8, int(&c["z"]) as i32 >> 8);
            let expect = match rom::sector_light(&data, s, x, z) {
                Some(m) => {
                    let t = rom::tint_palette(&base, m);
                    (0..256)
                        .map(|i| {
                            if (1..=143).contains(&i) || (149..=255).contains(&i) {
                                t[i]
                            } else {
                                ram[i]
                            }
                        })
                        .collect()
                }
                None => ram.clone(),
            };
            let got = u16s(
                &(0..512)
                    .map(|i| u8::from_str_radix(&c["palette"].as_str().unwrap()[2 * i..2 * i + 2], 16).unwrap())
                    .collect::<Vec<u8>>(),
            );
            got != expect
        })
        .count();
    ok &= report(
        "apply_sector_light_to_palette vs sector_light + tint_palette",
        c.len(),
        bad,
    );

    if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}
