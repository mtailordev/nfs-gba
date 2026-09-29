//! The car step against traces of the reference build (`tools/trace_race.py`, `docs/engine/physics.md`).
//!
//! Each trace row is the player's entity and physics struct at the entry of the game's car handler, plus the
//! globals the step reads; the first step also has a full memory dump. Skipped when the traces are absent.

use nfsgba_sim::sound::Command;
use nfsgba_sim::world::{DT, INPUT, RACE_PHASE, W_ENTITIES};
use nfsgba_sim::{Mem, Sim, car};
use std::fs;
use std::path::{Path, PathBuf};

const SCENARIOS: [&str; 8] = [
    "accel",
    "brake",
    "steer",
    "wall",
    "drive",
    "reverse",
    "handbrake",
    "long",
];
/// Entity bytes other game code maintains between car steps: the sector-list link (+0x02), the renderer's
/// view depth (+0x28) and byte +0x88.
const EXTERNAL: [usize; 7] = [0x02, 0x03, 0x28, 0x29, 0x2A, 0x2B, 0x88];

struct Row {
    dt: u32,
    phase: u32,
    input: u16,
    flag610c: u32,
    entity: Vec<u8>,
    physics: Vec<u8>,
}

fn trace_dir() -> Option<PathBuf> {
    let data = nfsgba_formats::data_dir();
    let manifest: serde_json::Value = serde_json::from_slice(&fs::read(data.join("vault/manifest.json")).ok()?).ok()?;
    let sha8 = &manifest["canonical_target"].as_str()?[..8];
    let dir = data.join("work").join(sha8).join("vehicle-physics");
    dir.join("accel.csv").exists().then_some(dir)
}

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn load(dir: &Path, name: &str) -> (Mem, Vec<Row>) {
    let rom = nfsgba_formats::canonical_rom().expect("canonical ROM");
    let ewram = fs::read(dir.join(format!("{name}.wram.bin"))).unwrap();
    let iwram = fs::read(dir.join(format!("{name}.iwram.bin"))).unwrap();
    let csv = fs::read_to_string(dir.join(format!("{name}.csv"))).unwrap();
    let rows = csv
        .lines()
        .skip(1)
        .map(|l| {
            let f: Vec<&str> = l.split(',').collect();
            Row {
                dt: f[2].parse().unwrap(),
                phase: f[3].parse().unwrap(),
                input: f[4].parse().unwrap(),
                flag610c: f[5].parse().unwrap(),
                entity: hex(f[7]),
                physics: hex(f[8]),
            }
        })
        .collect();
    (Mem::new(rom, ewram, iwram), rows)
}

fn set_inputs(mem: &mut Mem, row: &Row) {
    mem.set_u32(DT, row.dt);
    mem.set_u16(INPUT, row.input);
}

fn diff(label: &str, got: &[u8], want: &[u8], skip: &[usize]) -> Vec<String> {
    (0..got.len())
        .step_by(4)
        .filter(|&o| (o..o + 4).any(|k| !skip.contains(&k) && got[k] != want[k]))
        .map(|o| {
            let w = |b: &[u8]| i32::from_le_bytes(b[o..o + 4].try_into().unwrap());
            format!("{label}+{o:#05x}: {} want {}", w(got), w(want))
        })
        .collect()
}

fn check(sim: &Sim, e: u32, p: u32, want: &Row) -> Vec<String> {
    let mut bad = diff("entity", sim.mem.bytes(e, 0xA4), &want.entity, &EXTERNAL);
    bad.extend(diff("physics", sim.mem.bytes(p, 0x4FC), &want.physics, &[]));
    bad
}

/// A step's RAM writes (address, new byte) and sound commands.
type Effects = (Vec<(u32, u8)>, Vec<String>);

/// What one step wrote to RAM (address, new byte) and the sound commands it issued, in the format of
/// `tools/trace_oracle.py` (`<name>.oracle.txt`).
fn effects(before: &Mem, after: &Sim) -> Effects {
    let mut writes = Vec::new();
    for (base, a, b) in [
        (0x0200_0000u32, &before.ewram, &after.mem.ewram),
        (0x0300_0000, &before.iwram, &after.mem.iwram),
    ] {
        writes.extend((0..a.len()).filter(|&k| a[k] != b[k]).map(|k| (base + k as u32, b[k])));
    }
    let sounds = after
        .sounds
        .iter()
        .map(|c| match *c {
            Command::Play(e) => format!("play({e},1)"),
            Command::Stop(e) => format!("stop({e})"),
            Command::Pitch(e, p) => format!("pitch({e},{p})"),
            Command::Start {
                sample,
                pitch,
                channel,
                volume,
            } => format!("start({sample},{pitch},{channel},{volume})"),
        })
        .collect();
    (writes, sounds)
}

fn oracle(dir: &Path, name: &str) -> Option<Vec<Effects>> {
    let text = fs::read_to_string(dir.join(format!("{name}.oracle.txt"))).ok()?;
    Some(
        text.lines()
            .map(|l| {
                let f: Vec<&str> = l.split(';').collect();
                let writes = f[1]
                    .split_whitespace()
                    .map(|w| {
                        let (a, v) = w.split_once('=').unwrap();
                        (u32::from_str_radix(a, 16).unwrap(), u8::from_str_radix(v, 16).unwrap())
                    })
                    .collect();
                (writes, f[2].split_whitespace().map(str::to_owned).collect())
            })
            .collect(),
    )
}

/// Every step from the traced entry state gives the next traced state; where `tools/trace_oracle.py` has
/// recorded the game code's own RAM writes and sound calls for the step, those match too.
#[test]
fn each_step_matches_the_trace() {
    let Some(dir) = trace_dir() else {
        eprintln!("traces not found; skipped");
        return;
    };
    for name in SCENARIOS {
        let (mem, rows) = load(&dir, name);
        let expected = oracle(&dir, name);
        let mut sim = Sim::new(mem);
        let e = sim.mem.u32(W_ENTITIES);
        let p = sim.mem.u32(e + 0x8C);
        let mut failures = Vec::new();
        for (i, pair) in rows.windows(2).enumerate() {
            let (row, next) = (&pair[0], &pair[1]);
            sim.mem.set_bytes(e, &row.entity);
            sim.mem.set_bytes(p, &row.physics);
            set_inputs(&mut sim.mem, row);
            sim.mem.set_u32(RACE_PHASE, row.phase);
            sim.mem.set_u32(0x0300_610C, row.flag610c);
            sim.sounds.clear();
            let before = sim.mem.clone();
            if let Err(err) = car::handler(&mut sim, e) {
                failures.push(format!("{name} step {i}: {err}"));
                continue;
            }
            let mut bad = check(&sim, e, p, next);
            if let Some(expected) = &expected {
                let (writes, sounds) = effects(&before, &sim);
                let (want_writes, want_sounds) = &expected[i];
                let extra: Vec<_> = writes.iter().filter(|w| !want_writes.contains(w)).take(8).collect();
                let missing: Vec<_> = want_writes.iter().filter(|w| !writes.contains(w)).take(8).collect();
                if !extra.is_empty() || !missing.is_empty() {
                    bad.push(format!("RAM writes differ: extra {extra:x?}, missing {missing:x?}"));
                }
                if &sounds != want_sounds {
                    bad.push(format!("sounds {sounds:?} want {want_sounds:?}"));
                }
            }
            if !bad.is_empty() {
                failures.push(format!("{name} step {i}: {}", bad.join(", ")));
            }
        }
        assert!(
            failures.is_empty(),
            "{} of {} steps differ:\n{}",
            failures.len(),
            rows.len() - 1,
            failures.join("\n")
        );
        eprintln!(
            "{name}: {} steps exact (RAM writes and sounds checked: {})",
            rows.len() - 1,
            expected.is_some()
        );
    }
}

/// Replaying only the recorded inputs (keys and frame times) from the first traced state reproduces every
/// traced state.
#[test]
fn replay_matches_the_trace() {
    let Some(dir) = trace_dir() else {
        eprintln!("traces not found; skipped");
        return;
    };
    for name in SCENARIOS {
        let (mem, rows) = load(&dir, name);
        let mut sim = Sim::new(mem);
        let e = sim.mem.u32(W_ENTITIES);
        let p = sim.mem.u32(e + 0x8C);
        let start = sim.mem.vec3(e + 0xC);
        for (i, pair) in rows.windows(2).enumerate() {
            set_inputs(&mut sim.mem, &pair[0]);
            car::handler(&mut sim, e).unwrap_or_else(|err| panic!("{name} step {i}: {err}"));
            let bad = check(&sim, e, p, &pair[1]);
            assert!(
                bad.is_empty(),
                "{name} step {i} of {}: {}",
                rows.len() - 1,
                bad.join(", ")
            );
        }
        let end = sim.mem.vec3(e + 0xC);
        assert!(rows.len() > 50 && start != end, "{name}: the car should have moved");
        eprintln!(
            "{name}: {} steps exact, from {start:?} to {end:?} (8.8)",
            rows.len() - 1
        );
    }
}
