//! The car step against traces of the reference build (`tools/trace_race.py`, `docs/engine/physics.md`).
//!
//! Each trace row is the player's entity and physics struct at the entry of the game's car handler, the other
//! racers, and the globals the step reads that other code writes (frame time, keys, race controller); the first
//! step also has a full memory dump. Skipped when the traces are absent.

use nfsgba_sim::sound::Command;
use nfsgba_sim::world::W_ENTITIES;
use nfsgba_sim::{Mem, Sim, car};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Scenario and the first step the port is checked from.
const SCENARIOS: [(&str, usize); 9] = [
    ("accel", 0),
    ("brake", 0),
    ("steer", 0),
    ("wall", 0),
    ("drive", 0),
    ("reverse", 0),
    ("handbrake", 0),
    ("long", 0),
    ("start", 0),
];
/// Traced globals: column, address, size (as `tools/mgba_remote.lua` logs them).
const GLOBALS: [(&str, u32, u32); 9] = [
    ("dt", 0x0300_5640, 4),
    ("phase", 0x0300_0048, 4),
    ("input", 0x0300_57D8, 2),
    ("flag610c", 0x0300_610C, 4),
    ("sector", 0x0300_5614, 4),
    ("racetime", 0x0300_5800, 4),
    ("armed", 0x0300_5780, 4),
    ("countdown", 0x0300_5630, 4),
    ("mode5624", 0x0300_5624, 4),
];
/// Entity bytes other game code maintains between car steps: the sector-list link (+0x02), the draw-list link
/// (+0x04), the renderer's view depth (+0x28) and byte +0x88.
const ENTITY_EXTERNAL: [usize; 9] = [0x02, 0x03, 0x04, 0x05, 0x28, 0x29, 0x2A, 0x2B, 0x88];
/// Physics bytes other code maintains: the race position (+0xA8, the ranking).
const PHYSICS_EXTERNAL: [usize; 4] = [0xA8, 0xA9, 0xAA, 0xAB];

struct Row {
    globals: HashMap<String, u32>,
    entity: Vec<u8>,
    physics: Vec<u8>,
    others: Vec<(Vec<u8>, Vec<u8>)>,
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

fn load(dir: &Path, name: &str) -> Option<(Mem, Vec<Row>)> {
    let csv = fs::read_to_string(dir.join(format!("{name}.csv"))).ok()?;
    let rom = nfsgba_formats::canonical_rom().expect("canonical ROM");
    let ewram = fs::read(dir.join(format!("{name}.wram.bin"))).unwrap();
    let iwram = fs::read(dir.join(format!("{name}.iwram.bin"))).unwrap();
    let mut lines = csv.lines();
    let header: Vec<&str> = lines.next().unwrap().split(',').collect();
    let rows = lines
        .map(|l| {
            let f: HashMap<&str, &str> = header.iter().copied().zip(l.split(',')).collect();
            Row {
                globals: GLOBALS
                    .iter()
                    .filter_map(|&(k, _, _)| Some((k.to_owned(), f.get(k)?.parse().unwrap())))
                    .collect(),
                entity: hex(f["entity"]),
                physics: hex(f["physics"]),
                others: f.get("others").filter(|s| !s.is_empty()).map_or(Vec::new(), |s| {
                    s.split('|')
                        .map(|c| {
                            let (e, p) = c.split_once(':').unwrap();
                            (hex(e), hex(p))
                        })
                        .collect()
                }),
            }
        })
        .collect();
    Some((Mem::new(rom, ewram, iwram), rows))
}

fn load_car(mem: &mut Mem, e: u32, entity: &[u8], physics: &[u8]) {
    mem.set_bytes(e, entity);
    if !physics.is_empty() {
        let p = mem.u32(e + 0x8C);
        mem.set_bytes(p, physics);
    }
}

/// What the rest of the game provides to the player's step: the traced globals and the other racers.
fn load_environment(mem: &mut Mem, e: u32, row: &Row) {
    for (k, a, size) in GLOBALS {
        if let Some(&v) = row.globals.get(k) {
            if size == 2 { mem.set_u16(a, v as u16) } else { mem.set_u32(a, v) }
        }
    }
    for (k, (entity, physics)) in row.others.iter().enumerate() {
        load_car(mem, e + 0xA4 * (k as u32 + 1), entity, physics);
    }
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

fn check(sim: &Sim, e: u32, want: &Row) -> Vec<String> {
    let p = sim.mem.u32(e + 0x8C);
    let mut bad = diff("entity", sim.mem.bytes(e, 0xA4), &want.entity, &ENTITY_EXTERNAL);
    bad.extend(diff("physics", sim.mem.bytes(p, 0x4FC), &want.physics, &PHYSICS_EXTERNAL));
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
    for (name, first) in SCENARIOS {
        let Some((mem, rows)) = load(&dir, name) else {
            eprintln!("{name}: not recorded; skipped");
            continue;
        };
        let expected = oracle(&dir, name);
        let mut sim = Sim::new(mem);
        let e = sim.mem.u32(W_ENTITIES);
        let mut failures = Vec::new();
        for (i, pair) in rows.windows(2).enumerate().skip(first) {
            let (row, next) = (&pair[0], &pair[1]);
            load_car(&mut sim.mem, e, &row.entity, &row.physics);
            load_environment(&mut sim.mem, e, row);
            sim.sounds.clear();
            let before = sim.mem.clone();
            if let Err(err) = car::handler(&mut sim, e) {
                failures.push(format!("{name} step {i}: {err}"));
                continue;
            }
            let mut bad = check(&sim, e, next);
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
        let checked = rows.len() - 1 - first;
        assert!(
            failures.is_empty(),
            "{} of {checked} steps differ:\n{}",
            failures.len(),
            failures.join("\n")
        );
        eprintln!(
            "{name}: {checked} steps exact (RAM writes and sounds checked: {})",
            expected.is_some()
        );
    }
}

/// From the first checked step, the port run on its own state (fed only the keys, frame times, race
/// controller globals and the other racers from the trace) reproduces every traced state.
#[test]
fn replay_matches_the_trace() {
    let Some(dir) = trace_dir() else {
        eprintln!("traces not found; skipped");
        return;
    };
    for (name, first) in SCENARIOS {
        let Some((mem, rows)) = load(&dir, name) else {
            continue;
        };
        let mut sim = Sim::new(mem);
        let e = sim.mem.u32(W_ENTITIES);
        load_car(&mut sim.mem, e, &rows[first].entity, &rows[first].physics);
        let start = sim.mem.vec3(e + 0xC);
        for (i, pair) in rows.windows(2).enumerate().skip(first) {
            load_environment(&mut sim.mem, e, &pair[0]);
            car::handler(&mut sim, e).unwrap_or_else(|err| panic!("{name} step {i}: {err}"));
            let bad = check(&sim, e, &pair[1]);
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
            rows.len() - 1 - first
        );
    }
}
