//! The car step against traces of the reference build (`tools/trace_race.py`, `docs/engine/physics.md`).
//!
//! A trace has, for every call of the car handler on the player's entity, the full RAM at its entry
//! (`<name>.ramdelta` over the first step's dump) and the player's entity and physics struct (`<name>.csv`).
//! `tools/trace_oracle.py` adds each step's RAM writes and sound calls as the game's own code makes them
//! (`<name>.oracle.txt`). Skipped when the traces are absent.

use nfsgba_sim::sound::Command;
use nfsgba_sim::world::W_ENTITIES;
use nfsgba_sim::{Mem, Sim, car};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const SCENARIOS: [&str; 12] = [
    "accel",
    "brake",
    "steer",
    "wall",
    "drive",
    "reverse",
    "handbrake",
    "long",
    "start",
    "hunter",
    "tipped",
    "stuck",
];
const EWRAM: usize = 0x4_0000;
/// Bits other game code maintains between car steps, as (offset, mask): the entity's sector-list link
/// (+0x02), draw-list link (+0x04), flags +0x0A bit 2, view depth (+0x28) and byte +0x88.
const ENTITY_EXTERNAL: [(usize, u8); 10] = [
    (0x02, 0xFF),
    (0x03, 0xFF),
    (0x04, 0xFF),
    (0x05, 0xFF),
    (0x0A, 0x04),
    (0x28, 0xFF),
    (0x29, 0xFF),
    (0x2A, 0xFF),
    (0x2B, 0xFF),
    (0x88, 0xFF),
];
/// The physics struct's race position (+0xA8), set by the ranking.
const PHYSICS_EXTERNAL: [(usize, u8); 4] = [(0xA8, 0xFF), (0xA9, 0xFF), (0xAA, 0xFF), (0xAB, 0xFF)];

struct Trace {
    rom: Vec<u8>,
    /// EWRAM then IWRAM at the first step.
    first: Vec<u8>,
    /// Per step: byte runs (offset into `first`, bytes) that differ from the first step.
    deltas: Vec<Vec<(usize, Vec<u8>)>>,
    /// Per step: the player's entity and physics struct (empty before the car's init).
    cars: Vec<(Vec<u8>, Vec<u8>)>,
}

impl Trace {
    /// The reference build's RAM at the entry of step `i`.
    fn state(&self, i: usize) -> Mem {
        let mut ram = self.first.clone();
        for (off, bytes) in &self.deltas[i] {
            ram[*off..*off + bytes.len()].copy_from_slice(bytes);
        }
        let iwram = ram.split_off(EWRAM);
        Mem::new(self.rom.clone(), ram, iwram)
    }
}

fn trace_dir() -> Option<PathBuf> {
    let data = nfsgba_formats::data_dir();
    let manifest: serde_json::Value = serde_json::from_slice(&fs::read(data.join("vault/manifest.json")).ok()?).ok()?;
    let sha8 = &manifest["canonical_target"].as_str()?[..8];
    let dir = data.join("work").join(sha8).join("vehicle-physics");
    dir.join("accel.ramdelta").exists().then_some(dir)
}

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn u32_at(b: &[u8], at: usize) -> usize {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap()) as usize
}

fn load(dir: &Path, name: &str) -> Option<Trace> {
    let csv = fs::read_to_string(dir.join(format!("{name}.csv"))).ok()?;
    let mut lines = csv.lines();
    let header: Vec<&str> = lines.next().unwrap().split(',').collect();
    let cars = lines
        .map(|l| {
            let f: HashMap<&str, &str> = header.iter().copied().zip(l.split(',')).collect();
            (hex(f["entity"]), hex(f["physics"]))
        })
        .collect();
    let mut first = fs::read(dir.join(format!("{name}.wram.bin"))).unwrap();
    first.extend(fs::read(dir.join(format!("{name}.iwram.bin"))).unwrap());
    let raw = fs::read(dir.join(format!("{name}.ramdelta"))).unwrap();
    assert_eq!(&raw[..4], b"RAMD");
    let mut at = 8;
    let deltas = (0..u32_at(&raw, 4))
        .map(|_| {
            let runs = u32_at(&raw, at);
            at += 4;
            (0..runs)
                .map(|_| {
                    let (off, n) = (u32_at(&raw, at), u32_at(&raw, at + 4));
                    at += 8 + n;
                    (off, raw[at - n..at].to_vec())
                })
                .collect()
        })
        .collect();
    let rom = nfsgba_formats::canonical_rom().expect("canonical ROM");
    Some(Trace {
        rom,
        first,
        deltas,
        cars,
    })
}

fn diff(label: &str, got: &[u8], want: &[u8], external: &[(usize, u8)]) -> Vec<String> {
    let mask = |k: usize| external.iter().find(|e| e.0 == k).map_or(0, |e| e.1);
    (0..got.len())
        .step_by(4)
        .filter(|&o| (o..o + 4).any(|k| (got[k] ^ want[k]) & !mask(k) != 0))
        .map(|o| {
            let w = |b: &[u8]| i32::from_le_bytes(b[o..o + 4].try_into().unwrap());
            format!("{label}+{o:#05x}: {} want {}", w(got), w(want))
        })
        .collect()
}

fn check(sim: &Sim, e: u32, want: &(Vec<u8>, Vec<u8>)) -> Vec<String> {
    let p = sim.mem.u32(e + 0x8C);
    let mut bad = diff("entity", sim.mem.bytes(e, 0xA4), &want.0, &ENTITY_EXTERNAL);
    bad.extend(diff("physics", sim.mem.bytes(p, 0x4FC), &want.1, &PHYSICS_EXTERNAL));
    bad
}

/// A step's RAM writes (address, new byte) and sound commands.
type Effects = (Vec<(u32, u8)>, Vec<String>);

/// The oracle's record of a step: its effects, and whether other code changed the car before the next step
/// (a traffic car's collision response), so that the next traced state is not this step's result.
struct Expected {
    effects: Effects,
    external: bool,
}

/// What one step wrote to RAM and the sound commands it issued, in the format of `<name>.oracle.txt`.
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

fn oracle(dir: &Path, name: &str) -> Vec<Expected> {
    let text = fs::read_to_string(dir.join(format!("{name}.oracle.txt"))).expect("run tools/trace_oracle.py");
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
            Expected {
                effects: (writes, f[2].split_whitespace().map(str::to_owned).collect()),
                external: f.get(3) == Some(&"external"),
            }
        })
        .collect()
}

/// From the reference build's full RAM at each step's entry, the step gives the next traced car state, and
/// the same RAM writes and sound commands as the game's own code (`tools/trace_oracle.py`).
#[test]
fn each_step_matches_the_trace() {
    let Some(dir) = trace_dir() else {
        eprintln!("traces not found; skipped");
        return;
    };
    for name in SCENARIOS {
        let Some(trace) = load(&dir, name) else {
            eprintln!("{name}: not recorded; skipped");
            continue;
        };
        let expected = oracle(&dir, name);
        let mut failures = Vec::new();
        for (i, want) in expected.iter().enumerate().take(trace.cars.len() - 1) {
            let (want_writes, want_sounds) = &want.effects;
            let mut sim = Sim::new(trace.state(i));
            let e = sim.mem.u32(W_ENTITIES);
            let before = sim.mem.clone();
            if let Err(err) = car::handler(&mut sim, e) {
                failures.push(format!("{name} step {i}: {err}"));
                continue;
            }
            let mut bad = if want.external {
                Vec::new()
            } else {
                check(&sim, e, &trace.cars[i + 1])
            };
            let (writes, sounds) = effects(&before, &sim);
            let extra: Vec<_> = writes.iter().filter(|w| !want_writes.contains(w)).take(8).collect();
            let missing: Vec<_> = want_writes.iter().filter(|w| !writes.contains(w)).take(8).collect();
            if !extra.is_empty() || !missing.is_empty() {
                bad.push(format!("RAM writes differ: extra {extra:x?}, missing {missing:x?}"));
            }
            if sounds != **want_sounds {
                bad.push(format!("sounds {sounds:?} want {want_sounds:?}"));
            }
            if !bad.is_empty() {
                failures.push(format!("{name} step {i}: {}", bad.join(", ")));
            }
        }
        let steps = trace.cars.len() - 1;
        assert!(
            failures.is_empty(),
            "{} of {steps} steps differ:\n{}",
            failures.len(),
            failures.join("\n")
        );
        eprintln!("{name}: {steps} steps exact, with RAM writes and sounds");
    }
}

/// The player's car evolves on its own: each step runs on the reference RAM except the player's entity
/// and physics struct, which carry over from the port's previous step (the bits other code maintains come
/// from the reference). Every traced car state is reproduced.
#[test]
fn replay_matches_the_trace() {
    let Some(dir) = trace_dir() else {
        eprintln!("traces not found; skipped");
        return;
    };
    for name in SCENARIOS {
        let Some(trace) = load(&dir, name) else {
            continue;
        };
        let expected = oracle(&dir, name);
        let mut own: Option<(Vec<u8>, Vec<u8>)> = None;
        let mut sim = Sim::new(trace.state(0));
        let e = sim.mem.u32(W_ENTITIES);
        let start = sim.mem.vec3(e + 0xC);
        for (i, want) in expected.iter().enumerate().take(trace.cars.len() - 1) {
            let mut mem = trace.state(i);
            if let Some((entity, physics)) = &own {
                let merged: Vec<u8> = (0..0xA4)
                    .map(|k| {
                        let m = ENTITY_EXTERNAL.iter().find(|x| x.0 == k).map_or(0, |x| x.1);
                        entity[k] & !m | mem.u8(e + k as u32) & m
                    })
                    .collect();
                mem.set_bytes(e, &merged);
                let p = mem.u32(e + 0x8C);
                let race_position = mem.bytes(p + 0xA8, 4).to_vec();
                mem.set_bytes(p, physics);
                mem.set_bytes(p + 0xA8, &race_position);
            }
            sim = Sim::new(mem);
            car::handler(&mut sim, e).unwrap_or_else(|err| panic!("{name} step {i}: {err}"));
            if want.external {
                // Other code changed the car before the next step: carry on from the game's state.
                own = Some(trace.cars[i + 1].clone());
                continue;
            }
            let bad = check(&sim, e, &trace.cars[i + 1]);
            assert!(bad.is_empty(), "{name} step {i}: {}", bad.join(", "));
            let p = sim.mem.u32(e + 0x8C);
            own = Some((sim.mem.bytes(e, 0xA4).to_vec(), sim.mem.bytes(p, 0x4FC).to_vec()));
        }
        let end = sim.mem.vec3(e + 0xC);
        assert!(start != end, "{name}: the car should have moved");
        eprintln!(
            "{name}: {} steps exact, from {start:?} to {end:?} (8.8)",
            trace.cars.len() - 1
        );
    }
}
