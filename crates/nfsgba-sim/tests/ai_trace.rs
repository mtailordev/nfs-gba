//! The opponents' and traffic cars' steps against the game's own code (`docs/engine/ai.md`).
//!
//! The player-car traces (`tools/trace_race.py`) hold the full RAM at the start of every game frame's entity
//! loop. `tools/trace_ai_oracle.py` runs that loop on each frame's RAM in unicorn and records, per handler call,
//! every RAM byte it wrote and its stubbed calls (`<name>.ai-oracle.txt`, session `ai-traffic`). Here each frame
//! replays the loop: the other handlers' calls apply the game's writes, and every opponent (0x29) and traffic
//! (0x36) call runs the port, which must write exactly the same bytes and make the same calls. Skipped when the
//! traces are absent.

use nfsgba_sim::sound::Command;
use nfsgba_sim::{Mem, Sim, ai, traffic_ai};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const SCENARIOS: [&str; 9] = [
    "start",
    "accel",
    "brake",
    "steer",
    "wall",
    "drive",
    "reverse",
    "handbrake",
    "long",
];
const EWRAM: usize = 0x4_0000;
/// The IWRAM stack, which the oracle ignores.
const STACK: std::ops::Range<u32> = 0x0300_7A00..0x0300_7E80;

struct Trace {
    first: Vec<u8>,
    deltas: Vec<Vec<(usize, Vec<u8>)>>,
}

impl Trace {
    fn state(&self, rom: &[u8], i: usize) -> Mem {
        let mut ram = self.first.clone();
        for (off, bytes) in &self.deltas[i] {
            ram[*off..*off + bytes.len()].copy_from_slice(bytes);
        }
        let iwram = ram.split_off(EWRAM);
        Mem::new(rom.to_vec(), ram, iwram)
    }
}

/// One handler call of the entity loop, as the oracle recorded it.
struct Call {
    step: usize,
    entity: u32,
    handler: u32,
    writes: Vec<(u32, u8)>,
    calls: Vec<String>,
}

fn work(session: &str) -> Option<PathBuf> {
    let data = nfsgba_formats::data_dir();
    let manifest: serde_json::Value = serde_json::from_slice(&fs::read(data.join("vault/manifest.json")).ok()?).ok()?;
    let sha8 = &manifest["canonical_target"].as_str()?[..8];
    Some(data.join("work").join(sha8).join(session))
}

fn u32_at(b: &[u8], at: usize) -> usize {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap()) as usize
}

fn load(dir: &Path, name: &str) -> Option<Trace> {
    let raw = fs::read(dir.join(format!("{name}.ramdelta"))).ok()?;
    let mut first = fs::read(dir.join(format!("{name}.wram.bin"))).ok()?;
    first.extend(fs::read(dir.join(format!("{name}.iwram.bin"))).ok()?);
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
    Some(Trace { first, deltas })
}

fn oracle(dir: &Path, name: &str) -> Option<Vec<Call>> {
    let text = fs::read_to_string(dir.join(format!("{name}.ai-oracle.txt"))).ok()?;
    Some(
        text.lines()
            .map(|l| {
                let f: Vec<&str> = l.split(';').collect();
                Call {
                    step: f[0].parse().unwrap(),
                    entity: f[1].parse().unwrap(),
                    handler: u32::from_str_radix(f[2].trim_start_matches("0x"), 16).unwrap(),
                    writes: f[3]
                        .split_whitespace()
                        .map(|w| {
                            let (a, v) = w.split_once('=').unwrap();
                            (u32::from_str_radix(a, 16).unwrap(), u8::from_str_radix(v, 16).unwrap())
                        })
                        .collect(),
                    calls: f[4].split_whitespace().map(str::to_owned).collect(),
                }
            })
            .collect(),
    )
}

fn writes(before: &Mem, after: &Mem) -> BTreeSet<(u32, u8)> {
    let mut out = BTreeSet::new();
    for (base, a, b) in [
        (0x0200_0000u32, &before.ewram, &after.ewram),
        (0x0300_0000, &before.iwram, &after.iwram),
    ] {
        out.extend(
            (0..a.len())
                .filter(|&k| a[k] != b[k])
                .map(|k| (base + k as u32, b[k]))
                .filter(|(a, _)| !STACK.contains(a)),
        );
    }
    out
}

fn sounds(sim: &Sim) -> Vec<String> {
    sim.sounds
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
        .collect()
}

/// Runs the port for one call; `None` when the handler is not one of the port's.
fn run(sim: &mut Sim, handler: u32, e: u32) -> Option<Result<Vec<String>, String>> {
    let effects = match handler {
        0x29 => ai::handler(sim, e)
            .map(|fx| fx.map(|f| format!("effects({},{},{},{})", f.entity, f.heading, f.view, f.size))),
        0x36 => traffic_ai::handler(sim, e).map(|()| None),
        _ => return None,
    };
    Some(match effects {
        Ok(fx) => {
            let mut calls = sounds(sim);
            calls.extend(fx);
            Ok(calls)
        }
        Err(err) => Err(err.to_string()),
    })
}

/// Every opponent and traffic call of every traced frame writes exactly what the game's code writes.
#[test]
fn each_call_matches_the_game() {
    let (Some(traces), Some(oracles)) = (work("vehicle-physics"), work("ai-traffic")) else {
        eprintln!("traces not found; skipped");
        return;
    };
    let Ok(rom) = nfsgba_formats::canonical_rom() else {
        eprintln!("no ROM; skipped");
        return;
    };
    let (mut total, mut total_stopped) = (0, 0);
    for name in SCENARIOS {
        let (Some(trace), Some(calls)) = (load(&traces, name), oracle(&oracles, name)) else {
            eprintln!("{name}: not recorded; skipped");
            continue;
        };
        let (mut checked, mut stopped, mut failures) = (0, Vec::new(), Vec::new());
        let mut step = usize::MAX;
        let mut mem = trace.state(&rom, 0);
        for c in &calls {
            if c.step != step {
                step = c.step;
                mem = trace.state(&rom, step);
            }
            let e = mem.u32(0x0300_00FC) + c.entity * 0xA4;
            let mut sim = Sim::new(mem.clone());
            if let Some(result) = run(&mut sim, c.handler, e) {
                checked += 1;
                match result {
                    // Paths the port does not have yet stop instead of guessing (the 1:1 rule); only the ones
                    // listed in docs/engine/ai.md may stop.
                    Err(err) if EXPECTED_STOPS.iter().any(|s| err.contains(s)) => {
                        stopped.push(format!("{name} step {} entity {}: {err}", c.step, c.entity))
                    }
                    Err(err) => failures.push(format!("{name} step {} entity {}: {err}", c.step, c.entity)),
                    Ok(got_calls) => {
                        let got = writes(&mem, &sim.mem);
                        let want: BTreeSet<(u32, u8)> = c.writes.iter().copied().collect();
                        let extra: Vec<_> = got.difference(&want).take(6).collect();
                        let missing: Vec<_> = want.difference(&got).take(6).collect();
                        if !extra.is_empty() || !missing.is_empty() || got_calls != c.calls {
                            failures.push(format!(
                                "{name} step {} entity {} (handler {:#x}): extra {extra:x?} missing {missing:x?} \
                                 calls {got_calls:?} want {:?}",
                                c.step, c.entity, c.handler, c.calls
                            ));
                        }
                    }
                }
            }
            // Continue from the game's own result, so each call is checked from the exact game state.
            for &(a, v) in &c.writes {
                mem.set_u8(a, v);
            }
        }
        assert!(
            failures.is_empty(),
            "{} of {checked} calls differ:\n{}",
            failures.len(),
            failures.iter().take(20).cloned().collect::<Vec<_>>().join("\n")
        );
        eprintln!(
            "{name}: {} opponent and traffic calls exact, {} stopped",
            checked - stopped.len(),
            stopped.len()
        );
        for s in &stopped {
            eprintln!("  stopped: {s}");
        }
        total += checked - stopped.len();
        total_stopped += stopped.len();
    }
    eprintln!("{total} calls exact, {total_stopped} stopped at unported code");
}

/// Unported code the traces reach (owned by the car-physics port: FIDELITY D9 and D10).
const EXPECTED_STOPS: [&str; 2] = ["FUN_08144fa4", "FUN_081484f0"];
