//! Machine states recorded by `tools/mgba_game_trace.lua` at every entry of `main_frame` and packed by
//! `tools/game_trace.py` (`NAME.base.bin`, `NAME.delta`, `NAME.csv` in `work/e5298b24/<session>/`).

use std::{fs, io, path::Path};

use crate::{Machine, Timing};

/// Bytes of one machine state: EWRAM, IWRAM, palette, VRAM, OAM.
pub const STATE: usize = 0x4_0000 + 0x8000 + 0x400 + 0x1_8000 + 0x400;

/// A recorded run: `states[k]` is the machine at the entry of game frame `k` (one more state than frames).
pub struct Trace {
    pub states: Vec<Vec<u8>>,
    pub timing: Vec<Timing>,
    /// Per frame, the effect-sprite list (at `*0x03000058`) as `FUN_08161f38` found it: what the matrix-slot code
    /// left, for replays that stand in for that code.
    pub effects: Vec<Vec<u8>>,
}

/// The entity whose driver word (`+0x8C`) is `driver` in a trace state.
fn entity_of_driver(s: &[u8], driver: u32) -> usize {
    let iw = |a: usize| u32::from_le_bytes(s[0x4_0000 + a..0x4_0000 + a + 4].try_into().unwrap());
    let (base, count) = (iw(0xFC), (iw(0x1B8) & 0xFFFF) + (iw(0x1B8) >> 16));
    (0..count as usize)
        .find(|&i| {
            let at = (base & 0x3_FFFF) as usize + 0xA4 * i + 0x8C;
            u32::from_le_bytes(s[at..at + 4].try_into().unwrap()) == driver
        })
        .expect("a lane timer's driver is an entity's")
}

impl Trace {
    /// `dir/NAME.*`, or `None` when the files are absent.
    pub fn load(dir: &Path, name: &str) -> io::Result<Trace> {
        let mut s = fs::read(dir.join(format!("{name}.base.bin")))?;
        let delta = fs::read(dir.join(format!("{name}.delta")))?;
        let csv = fs::read_to_string(dir.join(format!("{name}.csv")))?;
        let word = |b: &[u8], at: usize| u32::from_le_bytes(b[at..at + 4].try_into().unwrap()) as usize;
        let mut states = vec![s.clone()];
        let mut at = 0;
        while at < delta.len() {
            let runs = word(&delta, at);
            at += 4;
            for _ in 0..runs {
                let (o, n) = (word(&delta, at), word(&delta, at + 4));
                s[o..o + n].copy_from_slice(&delta[at + 8..at + 8 + n]);
                at += 8 + n;
            }
            states.push(s.clone());
        }
        // Columns: video_frame, keys, the VBlank counter at main_frame / update_entities / hud_update, timer 3,
        // the counter at each sound call during the entity update (`;`-separated), at route_gap, at hud_timer.
        let rows: Vec<Vec<&str>> = csv.lines().skip(1).map(|l| l.split(',').collect()).collect();
        let num = |f: &str| f.parse::<u32>().ok();
        let timing = rows
            .windows(2)
            .enumerate()
            .map(|(k, w)| {
                let (r, start) = (&w[0], num(w[0][2]).unwrap());
                let at = |f: &str| num(f).map(|v| v - start);
                Timing {
                    timer3: num(r[5]).unwrap_or(0) as u16, // (menu frames after a hand-over record no marks)
                    entities: at(r[3]).unwrap_or(0),
                    // `entry-return` pairs (older traces: the entry only).
                    sounds: r[6]
                        .split(';')
                        .filter_map(|s| {
                            let (e, ret) = s.split_once('-').unwrap_or((s, s));
                            Some((at(e)?, at(ret)?))
                        })
                        .collect(),
                    gap: at(r[7]),
                    gap_reads: r.get(11).unwrap_or(&"").split(';').filter_map(at).collect(),
                    hud: at(r[4]).unwrap_or(0),
                    timer: at(r[8]),
                    end: num(w[1][2]).unwrap() - start,
                    start: r.get(14).and_then(|f| at(f)),
                    seed: r.get(12).and_then(|f| at(f)),
                    music: r.get(13).and_then(|f| at(f)),
                    pause: r.get(15).and_then(|f| at(f)),
                    exit: r.get(16).and_then(|f| at(f)),
                    handover: r.get(17).and_then(|f| at(f)),
                    marker: r.get(18).and_then(|f| at(f)),
                    laps: r
                        .get(19)
                        .unwrap_or(&"")
                        .split(';')
                        .filter_map(|p| p.split_once(':'))
                        .map(|(d, n)| (u32::from_str_radix(d, 16).unwrap(), num(n).unwrap() - start))
                        .map(|(d, n)| (entity_of_driver(&states[k], d), n))
                        .collect(),
                    lanes: r
                        .get(10)
                        .unwrap_or(&"")
                        .split(';')
                        .filter_map(|p| p.split_once(':'))
                        .map(|(d, n)| (u32::from_str_radix(d, 16).unwrap(), num(n).unwrap() - start))
                        .map(|(d, n)| (entity_of_driver(&states[k], d), n))
                        .collect(),
                }
            })
            .collect();
        let effects = rows
            .iter()
            .map(|r| {
                let h = r.get(9).copied().unwrap_or("");
                (0..h.len() / 2)
                    .map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).unwrap())
                    .collect()
            })
            .collect();
        Ok(Trace {
            states,
            timing,
            effects,
        })
    }

    pub fn machine(&self, rom: &[u8], k: usize) -> Machine {
        Machine::from_state(rom.to_vec(), &self.states[k])
    }

    /// The keys `main_frame` sampled at the end of frame `k` (`FUN_0812b040`): what the next state's held-keys
    /// word `0x030064C4` holds.
    pub fn keys(&self, k: usize) -> u16 {
        let s = &self.states[k + 1];
        let at = 0x4_0000 + 0x64C4;
        u16::from_le_bytes([s[at], s[at + 1]]) & 0x3FF
    }
}
