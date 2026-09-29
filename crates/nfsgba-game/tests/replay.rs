//! Game frames against the reference build, with nothing stood in: from the traced machine state at the entry of
//! frame k, one `Game::frame` with that frame's keys and timing must give the traced state at the entry of frame
//! k + 1 (`tools/game_trace.py`, `docs/engine/game-loop.md`). Missing traces follow `nfsgba_testkit`'s rule
//! (`NFSGBA_REQUIRE_DATA`). Every trace must run to its last frame: no stop is accepted.
//!
//! Compared is the game's state, not its bytes (the contract, `docs/DECISIONS.md`): the typed `World` against
//! `World::load` of the traced state (everything but the heap and the sky gradient's read pointer, which the
//! VCount IRQ advances within a frame), the racers' atlases in the heap, and the frame's outputs (palette RAM but
//! entry 0, the VCount IRQ's backdrop colour; VRAM; OAM).

use nfsgba_game::{Game, Machine, trace::Trace, world::World};

/// The recorded runs (`tools/game_trace.py`): `drive` (150 frames from the reference race), `live` (700 frames from
/// the start of a hard circuit with heavy traffic: opponents alongside, braking, a car-to-car contact at frame 387),
/// `trail` (700 frames of a sprint behind the opponents through heavy traffic) and `views` (600 frames from the
/// reference race: the bumper view, looking back in both views, the switch back behind the car, L and R held) and
/// `nitro` (300 frames from the reference race with nitro poked into the tank before recording: the camera's speed
/// effect and the nitro flames), `fadeout` and `fadein` (20 and 14 frames from the reference race with `main_frame`'s
/// palette fade counter poked: the palettes and the sky gradient fade to black, and in from black).
/// (session, trace, game frames): every frame of every trace must replay.
const TRACES: [(&str, &str, usize); 7] = [
    ("game-loop", "fadeout", 19),
    ("game-loop", "fadein", 13),
    ("game-loop", "drive", 149),
    ("live-race", "live", 699),
    ("live-race", "trail", 699),
    ("live-race", "views", 599),
    ("live-race", "nitro", 299),
];

fn traces() -> Vec<(&'static str, usize, Trace)> {
    TRACES
        .iter()
        .filter_map(|&(session, name, frames)| {
            nfsgba_testkit::fixture(&format!("{session}/{name}.base.bin"))?;
            let dir = nfsgba_testkit::fixture(session)?;
            let trace = Trace::load(&dir, name).unwrap_or_else(|e| panic!("{session}/{name}: {e}"));
            assert_eq!(trace.timing.len(), frames, "{name}: recorded game frames");
            Some((name, frames, trace))
        })
        .collect()
}

/// Where the game's state differs from `want`: the typed state (a line diff of its debug form, with the path of
/// each differing line) and the output bytes.
fn differences(g: &Game, want: &Machine) -> Vec<String> {
    let mut out = Vec::new();
    let (mut a, mut b) = (g.world.clone(), World::load(want));
    for i in 0..a.slots.len().min(b.slots.len()) {
        if a.atlas(i) != b.atlas(i) {
            out.push(format!("the atlas of entity {i}"));
        }
    }
    for w in [&mut a, &mut b] {
        (w.heap, w.gradient_start) = (Vec::new(), 0);
    }
    if a != b {
        let (x, y) = (format!("{a:#?}"), format!("{b:#?}"));
        let (x, y): (Vec<&str>, Vec<&str>) = (x.lines().collect(), y.lines().collect());
        let indent = |l: &str| l.len() - l.trim_start().len();
        for k in (0..x.len().max(y.len())).filter(|&k| x.get(k) != y.get(k)).take(12) {
            let (mut path, mut level) = (Vec::new(), x.get(k).map_or(0, |l| indent(l)));
            for l in x[..k.min(x.len())].iter().rev() {
                if indent(l) < level {
                    level = indent(l);
                    path.push(l.trim().trim_end_matches(['{', '[', '(', ' ']));
                }
            }
            path.reverse();
            out.push(format!(
                "{}: got {} want {}",
                path.join(" "),
                x.get(k).map_or("-", |l| l.trim()),
                y.get(k).map_or("-", |l| l.trim())
            ));
        }
    }
    for (name, got, want) in [
        ("palette", &g.palette[2..], &want.palette[2..]),
        ("vram", &g.vram[..], &want.vram[..]),
        ("oam", &g.oam[..], &want.oam[..]),
    ] {
        let off: Vec<usize> = (0..got.len()).filter(|&o| got[o] != want[o]).collect();
        if let Some(o) = off.first() {
            out.push(format!("{name}: {} bytes differ, first at {o:#x}", off.len()));
        }
    }
    out
}

fn report(name: &str, k: usize, diffs: &[String]) {
    eprintln!("{name} frame {k}: {} differences:", diffs.len());
    for d in diffs {
        eprintln!("  {d}");
    }
}

#[test]
fn frames_match_the_trace() {
    let Some(rom) = nfsgba_testkit::rom() else { return };
    for (name, frames, trace) in traces() {
        let mut failed = 0;
        let mut checked = nfsgba_testkit::Expect::new(format!("{name} frames"), frames);
        for k in 0..frames {
            checked.tick();
            let want = trace.machine(&rom, k + 1);
            let mut g = Game::new(trace.machine(&rom, k));
            if let Err(e) = g.frame(trace.keys(k), &trace.timing[k]) {
                panic!("{name} frame {k}: {e}");
            }
            let diffs = differences(&g, &want);
            if !diffs.is_empty() {
                failed += 1;
                if failed <= 6 {
                    report(name, k, &diffs);
                }
            }
        }
        eprintln!("{name}: {} of {frames} frames exact", frames - failed);
        assert_eq!(failed, 0, "{name}: {failed} of {frames} frames differ");
    }
}

/// Each trace as one run: the game keeps its own state from the first traced state on (every car, the camera, the
/// matrix slots and effects, the sound engine, HUD, palette and frame buffers); only the keys and the frame timing
/// (T1) come from the trace. Every frame must match.
#[test]
fn free_run_matches_the_trace() {
    let Some(rom) = nfsgba_testkit::rom() else { return };
    for (name, frames, trace) in traces() {
        let mut g = Game::new(trace.machine(&rom, 0));
        let mut exact = 0;
        let mut checked = nfsgba_testkit::Expect::new(format!("{name} free run"), frames);
        for k in 0..frames {
            checked.tick();
            let want = trace.machine(&rom, k + 1);
            if let Err(e) = g.frame(trace.keys(k), &trace.timing[k]) {
                panic!("{name}: the free run stops in frame {k}: {e}");
            }
            let diffs = differences(&g, &want);
            if !diffs.is_empty() {
                report(name, k, &diffs);
                panic!("{name}: the free run leaves the trace in frame {k}");
            }
            exact += 1;
        }
        assert_eq!(exact, frames, "{name}: free-running frames");
        eprintln!("{name}: {exact} of {frames} frames free-running, all exact");
    }
}

/// One frame of one trace with its differences (`NFSGBA_TRACE=live NFSGBA_FRAME=193 cargo test -p nfsgba-game
/// one_frame -- --ignored --nocapture`).
#[test]
#[ignore]
fn one_frame() {
    let rom = nfsgba_testkit::rom().unwrap();
    let name = std::env::var("NFSGBA_TRACE").unwrap();
    let k: usize = std::env::var("NFSGBA_FRAME").unwrap().parse().unwrap();
    let (_, _, trace) = traces().into_iter().find(|(n, _, _)| *n == name).unwrap();
    let mut g = Game::new(trace.machine(&rom, k));
    eprintln!("timing {:?}", trace.timing[k]);
    g.frame(trace.keys(k), &trace.timing[k]).unwrap();
    report(&name, k, &differences(&g, &trace.machine(&rom, k + 1)));
}
