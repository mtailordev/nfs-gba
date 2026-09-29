//! The suspension step (`FUN_0814de40`) against the game's own code. No race reaches it (0x0300610C is always 1 at
//! a car step), so `tools/trace_suspension.py` runs it in the function oracle on the reference race's RAM with
//! random inputs (`vehicle-physics/suspension.jsonl`). Each case must give the same return value, points, sector
//! outputs and RAM writes. Skipped when the cases are absent.

use nfsgba_sim::{Mem, contact};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;

/// The points' scratch address in the oracle (they are a slice here).
const PTS: u64 = 0x0203_F000;

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn changed(before: &Mem, after: &Mem) -> BTreeSet<(u32, u8)> {
    let mut out = BTreeSet::new();
    for (base, a, b) in [
        (0x0200_0000u32, &before.ewram, &after.ewram),
        (0x0300_0000, &before.iwram, &after.iwram),
    ] {
        out.extend((0..a.len()).filter(|&k| a[k] != b[k]).map(|k| (base + k as u32, b[k])));
    }
    out
}

#[test]
fn suspension_matches_the_oracle() {
    let data = nfsgba_formats::data_dir();
    let dir = data.join("work/e5298b24");
    let Ok(cases) = fs::read_to_string(dir.join("vehicle-physics/suspension.jsonl")) else {
        eprintln!("suspension cases not found; skipped");
        return;
    };
    let rom = nfsgba_formats::canonical_rom().expect("canonical ROM");
    let (wram, iwram) = (
        fs::read(dir.join("mgba/race.wram.bin")).unwrap(),
        fs::read(dir.join("mgba/race.iwram.bin")).unwrap(),
    );
    let (mut total, mut bad) = (0, Vec::new());
    for line in cases.lines() {
        let c: Value = serde_json::from_str(line).unwrap();
        let mut mem = Mem::new(rom.clone(), wram.clone(), iwram.clone());
        let mut pts = [[0i32; 3]; 4];
        for p in c["patch"].as_array().unwrap() {
            let (addr, bytes) = (p[0].as_u64().unwrap(), hex(p[1].as_str().unwrap()));
            if addr == PTS {
                for (k, w) in bytes.chunks(4).enumerate() {
                    pts[k / 3][k % 3] = i32::from_le_bytes(w.try_into().unwrap());
                }
            } else {
                mem.set_bytes(addr as u32, &bytes);
            }
        }
        let before = mem.clone();
        let mut sectors = [0u16; 4];
        let e = c["entity"].as_u64().unwrap() as u32;
        let hits = contact::suspension(&mut mem, e, &mut pts, &mut sectors, c["dt"].as_i64().unwrap() as i32);
        let want: BTreeSet<(u32, u8)> = c["writes"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|w| {
                let a = w[0].as_u64().unwrap() as u32;
                hex(w[1].as_str().unwrap())
                    .into_iter()
                    .enumerate()
                    .map(move |(k, v)| (a + k as u32, v))
            })
            .collect();
        let got = changed(&before, &mem);
        let want_pts: Vec<i32> = c["points"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_i64().unwrap() as i32)
            .collect();
        let want_sectors: Vec<u16> = c["sectors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap() as u16)
            .collect();
        let mut diffs = Vec::new();
        if hits as i64 != c["ret"].as_i64().unwrap() {
            diffs.push(format!("hits {hits} want {}", c["ret"]));
        }
        if pts.concat() != want_pts || sectors.to_vec() != want_sectors {
            diffs.push(format!("points {pts:?} {sectors:?} want {want_pts:?} {want_sectors:?}"));
        }
        if got != want {
            let extra: Vec<_> = got.difference(&want).take(6).collect();
            let missing: Vec<_> = want.difference(&got).take(6).collect();
            diffs.push(format!("writes: extra {extra:x?} missing {missing:x?}"));
        }
        if !diffs.is_empty() {
            bad.push(format!("case {total}: {}", diffs.join("; ")));
        }
        total += 1;
    }
    assert!(
        bad.is_empty(),
        "{} of {total} cases differ:\n{}",
        bad.len(),
        bad[..bad.len().min(10)].join("\n")
    );
    eprintln!("suspension: {total} oracle cases exact");
}
