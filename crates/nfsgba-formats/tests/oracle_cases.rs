//! The Rust ports against the function oracle's saved cases (`tools/oracle/prove.py`: the game's own code run in
//! unicorn on generated inputs, `harness/oracle/*.jsonl`). Every case must give the game's result.

use nfsgba_formats as rom;
use nfsgba_testkit::read;
use serde_json::Value;

fn cases(name: &str) -> Option<Vec<Value>> {
    let text = nfsgba_testkit::read_to_string(&format!("harness/oracle/{name}.jsonl"))?;
    Some(text.lines().map(|l| serde_json::from_str(l).unwrap()).collect())
}

fn int(v: &Value) -> i64 {
    v.as_i64().unwrap()
}

fn u16s(b: &[u8]) -> Vec<u16> {
    b.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
}

#[test]
fn divsi3_matches_div() {
    let Some(c) = cases("divsi3") else { return };
    assert_eq!(c.len(), 20121, "cases");
    let bad: Vec<_> = c
        .iter()
        .filter(|c| rom::div(int(&c["a"]) as i32, int(&c["b"]) as i32) as i64 != int(&c["q"]))
        .take(5)
        .collect();
    assert!(bad.is_empty(), "__divsi3 vs div: {bad:?}");
}

#[test]
fn sin_q14_matches_the_game() {
    let (Some(data), Some(c)) = (nfsgba_testkit::rom(), cases("sin_q14")) else {
        return;
    };
    assert_eq!(c.len(), 71048, "cases");
    let bad: Vec<_> = c
        .iter()
        .filter(|c| rom::paint::sin_q14(&data, int(&c["angle"]) as i32) as i64 != int(&c["sin"]))
        .take(5)
        .collect();
    assert!(bad.is_empty(), "sin_q14 vs paint::sin_q14: {bad:?}");
}

/// `apply_sector_light_to_palette`: palette RAM after the call. The Rust side tints the base buffer
/// (`*0x030055F0`) where `sector_light` finds light, and leaves the snapshot's palette RAM otherwise.
#[test]
fn sector_light_matches_the_game() {
    let (Some(data), Some(c), Some(iwram), Some(wram), Some(pal)) = (
        nfsgba_testkit::rom(),
        cases("sector_light"),
        read("mgba/race.iwram.bin"),
        read("mgba/race.wram.bin"),
        read("mgba/race.palette.bin"),
    ) else {
        return;
    };
    assert_eq!(c.len(), 2000, "cases");
    let at = u32::from_le_bytes(iwram[0x55F0..0x55F4].try_into().unwrap()) as usize - 0x0200_0000;
    let (base, ram) = (u16s(&wram[at..at + 512]), u16s(&pal[..512]));
    let city = rom::city(&data);
    let bad = c
        .iter()
        .filter(|c| {
            let s = &city[int(&c["sector"]) as usize];
            let (x, z) = (int(&c["x"]) as i32 >> 8, int(&c["z"]) as i32 >> 8);
            let expect: Vec<u16> = match rom::sector_light(&data, s, x, z) {
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
            let hex = c["palette"].as_str().unwrap();
            let got: Vec<u8> = (0..512)
                .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap())
                .collect();
            u16s(&got) != expect
        })
        .count();
    assert_eq!(
        bad,
        0,
        "apply_sector_light_to_palette vs sector_light + tint_palette: {bad} of {} differ",
        c.len()
    );
}
