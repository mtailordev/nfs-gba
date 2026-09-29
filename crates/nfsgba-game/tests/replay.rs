//! Game frames against the reference build: from the traced machine state at the entry of frame k, one
//! `Game::frame` with that frame's keys and timing must give the traced state at the entry of frame k + 1
//! (`tools/game_trace.py`, `docs/engine/game-loop.md`). Skipped when the trace is absent.

use std::ops::Range;

use nfsgba_formats as rom;
use nfsgba_game::{Checkpoint, Game, trace::Trace, view::WORLD};
use nfsgba_sim::Mem;

const EW: usize = 0;
const IW: usize = 0x4_0000;
const PAL: usize = IW + 0x8000;
const VRAM: usize = PAL + 0x400;
const OAM: usize = VRAM + 0x1_8000;

/// State offset of a GBA address (EWRAM, IWRAM, palette, VRAM, OAM).
fn off(a: u32) -> usize {
    match a >> 24 {
        2 => EW + (a & 0x3_FFFF) as usize,
        3 => IW + (a & 0x7FFF) as usize,
        5 => PAL + (a & 0x3FF) as usize,
        6 => VRAM + (a & 0x1_FFFF) as usize,
        7 => OAM + (a & 0x3FF) as usize,
        _ => panic!("{a:#x}"),
    }
}

fn addr(o: usize) -> u32 {
    match o {
        _ if o < IW => 0x0200_0000 + o as u32,
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

/// Copies `range` of GBA addresses from the reference state into the game.
fn take(g: &mut Game, next: &[u8], range: Range<u32>) {
    g.sim
        .mem
        .set_bytes(range.start, &next[off(range.start)..off(range.end)]);
}

/// Stand-ins for what is not ported yet, from the reference state after the frame (and, for the effect-sprite
/// list, from the trace's record of it as `FUN_08161f38` found it).
fn assist(next: &[u8], effects: &[u8], at: Checkpoint, g: &mut Game) -> bool {
    let m = &g.sim.mem;
    let ents = m.u32(WORLD + 0x3C);
    match at {
        Checkpoint::Entities => {
            // D4: the opponents' and traffic handlers: their entity, driver struct and control word.
            for (i, _) in g.skipped.clone() {
                let e = ents + 0xA4 * i;
                take(g, next, e..e + 0xA4);
                let d = g.sim.mem.u32(e + 0x8C);
                if d != 0 {
                    take(g, next, d..d + 0x500);
                }
                take(g, next, 0x0300_57D8 + 2 * i..0x0300_57DA + 2 * i);
            }
            // They move cars between sector lists: the heads (world +0x0C) and every entity's link (+0x02).
            if !g.skipped.is_empty() {
                take(g, next, 0x0300_64C8..0x0300_64CC); // the RNG index (rand_table)
                let heads = g.sim.mem.u32(WORLD + 0x0C);
                take(g, next, heads..heads + 2 * 1113);
                for i in 0..nfsgba_game::view::entity_count(&g.sim.mem) {
                    let e = ents + 0xA4 * i;
                    take(g, next, e + 2..e + 4);
                }
            }
        }
        Checkpoint::Camera => {
            // camera_update (chase view): orbit and look yaw, position, sector, focal, matrix, list entry 0.
            for r in [
                0x0300_5F94..0x0300_5FA8,
                0x0300_0210..0x0300_0218,
                0x0300_56A0..0x0300_56A4,
                0x0300_00A4..0x0300_00A8,
                0x0300_5614..0x0300_5618,
                0x0300_009C..0x0300_00A0,
                0x0300_57A0..0x0300_57D0,
                0x0300_5778..0x0300_577C,
                0x0300_56B8..0x0300_56BC,
                WORLD + 0xC0..WORLD + 0xCC, // the sector search point (find_camera_sector)
            ] {
                take(g, next, r);
            }
            let list = g.sim.mem.u32(WORLD + 0x60);
            take(g, next, list..list + 16);
        }
        Checkpoint::Slots => {
            // R25: matrix slots, entity slot bytes and flags, the slot counter, the effect-sprite list.
            let slots = g.sim.mem.u32(WORLD + 0xFC);
            take(g, next, slots..slots + 64 * 0x30);
            for i in 0..nfsgba_game::view::entity_count(&g.sim.mem) {
                let e = ents + 0xA4 * i;
                take(g, next, e + 0x88..e + 0x89);
                take(g, next, e + 0x0A..e + 0x0C);
            }
            take(g, next, 0x0300_5394..0x0300_5398);
            let fx = g.sim.mem.u32(0x0300_0058);
            g.sim.mem.set_bytes(fx, effects);
            // The player's contact effects spawn entities (FUN_0814c37c: sparks, handler 0x34) into free slots and
            // link them into sector lists; the rand index moves with them.
            let player = g.sim.mem.u32(0x0300_0060);
            for i in (0..nfsgba_game::view::entity_count(&g.sim.mem)).filter(|&i| i != player) {
                let e = ents + 0xA4 * i;
                take(g, next, e..e + 0xA4);
            }
            let heads = g.sim.mem.u32(WORLD + 0x0C);
            take(g, next, heads..heads + 2 * 1113);
            take(g, next, ents + 0xA4 * player + 2..ents + 0xA4 * player + 4);
            take(g, next, 0x0300_64C8..0x0300_64CC);
        }
    }
    true
}

#[test]
fn frames_match_the_trace() {
    std::env::set_current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")).unwrap();
    let Ok(rom) = rom::canonical_rom() else {
        eprintln!("skipping: no ROM vault");
        return;
    };
    let dir = rom::data_dir().join("work/e5298b24/game-loop");
    let Ok(trace) = Trace::load(&dir, "drive") else {
        eprintln!("skipping: no trace in {}", dir.display());
        return;
    };
    let mut failed = 0;
    for k in 0..trace.timing.len() {
        let next = &trace.states[k + 1];
        let mut g = Game::new(trace.machine(&rom, k));
        let effects = &trace.effects[k];
        let r = g.frame_with(trace.keys(k), &trace.timing[k], &mut |at, g| {
            assist(next, effects, at, g)
        });
        if let Err(e) = r {
            panic!("frame {k}: {e}");
        }
        let got = g.machine().state();
        let skip = scratch(&g.sim.mem);
        let differs: Vec<usize> = (0..got.len())
            .filter(|&o| got[o] != next[o] && !skip.iter().any(|(r, _)| r.contains(&addr(o))))
            .collect();
        if !differs.is_empty() {
            failed += 1;
            if failed <= 12 {
                let mut runs: Vec<(u32, u32)> = Vec::new();
                for &o in &differs {
                    let a = addr(o);
                    match runs.last_mut() {
                        Some((_, end)) if a <= *end + 8 => *end = a,
                        _ => runs.push((a, a)),
                    }
                }
                eprintln!("frame {k}: {} bytes differ in {} runs:", differs.len(), runs.len());
                for (a, b) in runs.iter().take(6) {
                    let (o, n) = (off(*a), (b - a + 1) as usize);
                    eprintln!(
                        "  {a:#010x}..={b:#010x}: got {:02x?} want {:02x?}",
                        &got[o..o + n.min(12)],
                        &next[o..o + n.min(12)]
                    );
                }
            }
        }
    }
    assert_eq!(failed, 0, "{failed} of {} frames differ", trace.timing.len());
}
