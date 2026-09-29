//! Game frames against the reference build, with nothing stood in: from the traced machine state at the entry of
//! frame k, one `Game::frame` with that frame's keys and timing must give the traced state at the entry of frame
//! k + 1 (`tools/game_trace.py`, `docs/engine/game-loop.md`). Skipped when the traces are absent.

use std::ops::Range;

use nfsgba_formats as rom;
use nfsgba_game::{Game, trace::Trace, view::WORLD};
use nfsgba_sim::Mem;

const EW: usize = 0;
const IW: usize = 0x4_0000;
const PAL: usize = IW + 0x8000;
const VRAM: usize = PAL + 0x400;
const OAM: usize = VRAM + 0x1_8000;

fn addr(o: usize) -> u32 {
    match o {
        _ if o < IW => 0x0200_0000 + (o - EW) as u32,
        _ if o < PAL => 0x0300_0000 + (o - IW) as u32,
        _ if o < VRAM => 0x0500_0000 + (o - PAL) as u32,
        _ if o < OAM => 0x0600_0000 + (o - VRAM) as u32,
        _ => 0x0700_0000 + (o - OAM) as u32,
    }
}

/// Machine state not carried from one game frame to the next, or set by the display rather than the game:
/// (range of GBA addresses, why).
fn scratch(m: &Mem) -> Vec<(Range<u32>, &'static str)> {
    let w = |o: u32| m.u32(WORLD + o);
    vec![
        (
            0x0300_7800..0x0300_8000,
            "IWRAM stack (and the BIOS IRQ words at the top)",
        ),
        (
            0x0500_0000..0x0500_0002,
            "palette entry 0: the VCount IRQ's backdrop colour of the current line",
        ),
        (0x0300_56E8..0x0300_56EC, "the VCount IRQ's gradient read pointer"),
        (w(0x60)..w(0x60) + 64 * 16, "renderer: visible-sector list"),
        (w(0x64)..w(0x64) + 8 * 1113, "renderer: sector map"),
        (w(0x68)..w(0x68) + 0x1A00, "renderer: wall buffer"),
        (w(0x6C)..w(0x6C) + 0x780, "renderer: flat vertex buffer"),
        (
            WORLD + 0xE2..WORLD + 0xF6,
            "renderer: portal span, camera sector, counts",
        ),
        (0x0300_53D0..0x0300_53F0, "renderer: rasteriser rectangle"),
        (0x0300_53F0..0x0300_5600, "renderer: projected model vertices"),
        (0x0300_6390..0x0300_63D0, "renderer: clipped UVs"),
        (0x0300_6430..0x0300_6480, "renderer: clipped corners"),
        (
            0x0300_68F0..0x0300_6960,
            "renderer: rasteriser state, visited bitmap, draw list",
        ),
    ]
}

/// The recorded runs (`tools/game_trace.py`): `drive` (150 frames from the reference race), `live` (700 frames from
/// the start of a hard circuit with heavy traffic: opponents alongside, braking, a car-to-car contact at frame 387),
/// `trail` (700 frames of a sprint behind the opponents through heavy traffic) and `views` (600 frames from the
/// reference race: the bumper view, looking back in both views, the switch back behind the car, L and R held) and
/// `nitro` (300 frames from the reference race with nitro poked into the tank before recording: the camera's speed
/// effect and the nitro flames).
const TRACES: [(&str, &str); 5] = [
    ("game-loop", "drive"),
    ("live-race", "live"),
    ("live-race", "trail"),
    ("live-race", "views"),
    ("live-race", "nitro"),
];

/// Car code paths the physics-paths work owns (FIDELITY D9–D11): a frame that reaches one stops with `Unported`,
/// which the replays accept and report; every frame before it must be exact.
const EXPECTED_STOPS: [&str; 6] = [
    "FUN_08144fa4",
    "FUN_081484f0",
    "FUN_0814efa8",
    "FUN_0814de40",
    "FUN_0814dbbc",
    "0x0813DF98",
];

fn expected(e: &nfsgba_sim::Unported) -> bool {
    EXPECTED_STOPS.iter().any(|s| e.0.contains(s))
}

fn traces() -> Vec<(&'static str, Trace)> {
    TRACES
        .iter()
        .filter_map(|&(session, name)| {
            let dir = rom::data_dir().join("work/e5298b24").join(session);
            match Trace::load(&dir, name) {
                Ok(t) => Some((name, t)),
                Err(_) => {
                    eprintln!("skipping {name}: no trace in {}", dir.display());
                    None
                }
            }
        })
        .collect()
}

/// The addresses where the game's state differs from `want`, as runs of nearby bytes.
fn differences(g: &Game, want: &[u8]) -> Vec<(u32, u32)> {
    let got = g.machine().state();
    let skip = scratch(&g.sim.mem);
    let mut runs: Vec<(u32, u32)> = Vec::new();
    for o in (0..got.len()).filter(|&o| got[o] != want[o]) {
        let a = addr(o);
        if skip.iter().any(|(r, _)| r.contains(&a)) {
            continue;
        }
        match runs.last_mut() {
            Some((_, end)) if a <= *end + 8 => *end = a,
            _ => runs.push((a, a)),
        }
    }
    runs
}

fn report(name: &str, k: usize, g: &Game, want: &[u8], runs: &[(u32, u32)]) {
    let got = g.machine().state();
    let off = |a: u32| match a >> 24 {
        2 => EW + (a & 0x3_FFFF) as usize,
        3 => IW + (a & 0x7FFF) as usize,
        5 => PAL + (a & 0x3FF) as usize,
        6 => VRAM + (a & 0x1_FFFF) as usize,
        _ => OAM + (a & 0x3FF) as usize,
    };
    eprintln!("{name} frame {k}: {} runs differ:", runs.len());
    for (a, b) in runs.iter().take(8) {
        let (o, n) = (off(*a), ((b - a + 1) as usize).min(12));
        eprintln!(
            "  {a:#010x}..={b:#010x}: got {:02x?} want {:02x?}",
            &got[o..o + n],
            &want[o..o + n]
        );
    }
}

#[test]
fn frames_match_the_trace() {
    std::env::set_current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")).unwrap();
    let Ok(rom) = rom::canonical_rom() else {
        eprintln!("skipping: no ROM vault");
        return;
    };
    for (name, trace) in traces() {
        let (mut failed, mut stopped) = (0, Vec::new());
        for k in 0..trace.timing.len() {
            let want = &trace.states[k + 1];
            let mut g = Game::new(trace.machine(&rom, k));
            match g.frame(trace.keys(k), &trace.timing[k]) {
                Err(e) if expected(&e) => {
                    stopped.push(k);
                    continue;
                }
                Err(e) => panic!("{name} frame {k}: {e}"),
                Ok(()) => {}
            }
            let runs = differences(&g, want);
            if !runs.is_empty() {
                failed += 1;
                if failed <= 6 {
                    report(name, k, &g, want, &runs);
                }
            }
        }
        eprintln!(
            "{name}: {} of {} frames exact; {} stopped in D9–D11 code: {stopped:?}",
            trace.timing.len() - failed - stopped.len(),
            trace.timing.len(),
            stopped.len()
        );
        assert_eq!(failed, 0, "{name}: {failed} of {} frames differ", trace.timing.len());
    }
}

/// Each trace as one run: the game keeps its own state from the first traced state on (every car, the camera, the
/// matrix slots and effects, the sound engine, HUD, palette and frame buffers); only the keys and the frame timing
/// (T1) come from the trace. Every frame must match.
#[test]
fn free_run_matches_the_trace() {
    std::env::set_current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")).unwrap();
    let Ok(rom) = rom::canonical_rom() else {
        eprintln!("skipping: no ROM vault");
        return;
    };
    for (name, trace) in traces() {
        let mut g = Game::new(trace.machine(&rom, 0));
        let mut exact = 0;
        for k in 0..trace.timing.len() {
            let want = &trace.states[k + 1];
            match g.frame(trace.keys(k), &trace.timing[k]) {
                Err(e) if expected(&e) => {
                    eprintln!("{name}: the free run stops in frame {k}: {e}");
                    break;
                }
                Err(e) => panic!("{name} frame {k}: {e}"),
                Ok(()) => {}
            }
            let runs = differences(&g, want);
            if !runs.is_empty() {
                report(name, k, &g, want, &runs);
                panic!("{name}: the free run leaves the trace in frame {k}");
            }
            exact += 1;
        }
        eprintln!(
            "{name}: {exact} of {} frames free-running, all exact",
            trace.timing.len()
        );
    }
}

/// One frame of one trace with its differences (`NFSGBA_TRACE=live NFSGBA_FRAME=193 cargo test -p nfsgba-game
/// one_frame -- --ignored --nocapture`).
#[test]
#[ignore]
fn one_frame() {
    std::env::set_current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")).unwrap();
    let rom = rom::canonical_rom().unwrap();
    let name = std::env::var("NFSGBA_TRACE").unwrap();
    let k: usize = std::env::var("NFSGBA_FRAME").unwrap().parse().unwrap();
    let (_, trace) = traces().into_iter().find(|(n, _)| *n == name).unwrap();
    let mut g = Game::new(trace.machine(&rom, k));
    eprintln!("timing {:?}", trace.timing[k]);
    g.frame(trace.keys(k), &trace.timing[k]).unwrap();
    report(
        &name,
        k,
        &g,
        &trace.states[k + 1],
        &differences(&g, &trace.states[k + 1]),
    );
}
