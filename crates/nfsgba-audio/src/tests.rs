//! Tests against the user's ROM and against engine traces recorded from the reference build with
//! `tools/audio_trace.py` (skipped, with a note, when either is missing).

use crate::format::{self, MODULE_COUNT, MODULE_TABLE, NO_NOTE, SAMPLE_BANK, SFX_TABLE};
use crate::ram::{self, ENGINE_SIZE};
use crate::{Engine, Rom};
use nfsgba_testkit::rom;

fn modules(rom: Rom) -> Vec<format::Module> {
    (0..MODULE_COUNT as u32)
        .map(|i| format::Module::parse(rom, rom.u32(MODULE_TABLE + 4 * i)).expect("GBAMOD30"))
        .collect()
}

#[test]
fn parses_all_modules() {
    let Some(rom) = rom() else { return };
    let rom = Rom(&rom);
    let mods = modules(rom);
    let addrs: Vec<u32> = mods.iter().map(|m| m.addr).collect();
    assert_eq!(addrs, [0x0804_EA14, 0x0804_F134, 0x0804_F8DC, 0x0805_0034, 0x0805_07FC]);
    // The only other "GBAMOD" in the ROM is the loader's own magic literal (FUN_08151e04's pool).
    let tags: Vec<usize> = rom
        .0
        .windows(6)
        .enumerate()
        .filter(|(_, w)| w == b"GBAMOD")
        .map(|(i, _)| i)
        .collect();
    let mut expect: Vec<usize> = addrs.iter().map(|a| (a - Rom::BASE) as usize).collect();
    expect.push(0x15_1E34);
    assert_eq!(tags, expect);

    let bank = format::sample_bank(rom, SAMPLE_BANK);
    for (m, (channels, tempo)) in mods.iter().zip([(2, 125), (1, 98), (1, 98), (1, 98), (1, 98)]) {
        assert_eq!(
            (m.channels, m.tempo, m.speed, m.rows, m.linear, m.order_count),
            (channels, tempo, 6, 64, 0, 255)
        );
        // Every song is its order list up to the first pattern that ends with B00 (jump to order 0).
        let mut looped = false;
        for &p in &m.orders {
            let rows = m.pattern(rom, p);
            assert_eq!(rows.len(), 64);
            for cell in rows.iter().flatten() {
                if cell.note != NO_NOTE {
                    assert!(bank[cell.instrument as usize - 1].is_some(), "note on an empty sample");
                }
                assert!(
                    [(0, 0), (0xB, 0)].contains(&(cell.effect, cell.param)),
                    "unexpected effect {cell:?}"
                );
            }
            if rows.iter().flatten().any(|c| c.effect == 0xB) {
                looped = true;
                break;
            }
        }
        assert!(looped, "module {:#x} never loops", m.addr);
    }
}

#[test]
fn sample_bank_and_sfx_table() {
    let Some(rom) = rom() else { return };
    let rom = Rom(&rom);
    let bank: Vec<_> = format::sample_bank(rom, SAMPLE_BANK).into_iter().flatten().collect();
    assert_eq!(bank.len(), 26);
    assert_eq!(bank[0].addr, 0x0805_1FD4);
    assert_eq!(bank[25].addr + bank[25].len, 0x0812_A84B);
    let sfx = format::sfx_table(rom, SFX_TABLE);
    assert_eq!(sfx.len(), 40);
    assert_eq!(
        (sfx[12].addr, sfx[12].len, sfx[12].loop_len, sfx[12].rate),
        (0x0800_3F2C, 0x4A57, 0x4A57, 10512)
    );
    assert!(sfx.iter().all(|s| s.flag == 0));
}

/// A `tools/mgba_audio_trace.lua` record: the engine on entry to (tag 0) or return from (tag 1) the game's
/// per-frame update, or a call into the engine's API between updates (tag 2).
enum Record {
    State {
        tag: u32,
        frame: u32,
        globals: Vec<u8>,
        eng: Vec<u8>,
        buffers: [Vec<u8>; 2],
    },
    Call {
        addr: u32,
        regs: [u32; 4],
    },
}

fn trace(name: &str) -> Option<Vec<Record>> {
    let data = nfsgba_testkit::read(&format!("audio/{name}.trace"))?;
    let mut out = Vec::new();
    let mut o = 0;
    while o < data.len() {
        let w = |k: usize| u32::from_le_bytes(data[o + 4 * k..o + 4 * k + 4].try_into().unwrap());
        if w(0) == 2 {
            out.push(Record::Call {
                addr: w(2),
                regs: [w(3), w(4), w(5), w(6)],
            });
            o += 28;
            continue;
        }
        let n = w(5) as usize;
        let e = o + 40 + ENGINE_SIZE;
        out.push(Record::State {
            tag: w(0),
            frame: w(1),
            globals: data[o + 24..o + 40].to_vec(),
            eng: data[o + 40..e].to_vec(),
            buffers: [data[e..e + n].to_vec(), data[e + n..e + 2 * n].to_vec()],
        });
        o = e + 2 * n;
    }
    Some(out)
}

/// Every field the rewrite keeps, and both mix buffers, against a recorded state.
fn compare(e: &Engine, globals: &[u8], eng: &[u8], buffers: &[Vec<u8>; 2]) -> Result<(), String> {
    if let Some(k) = (0..2).find(|&k| e.buffers[k] != buffers[k]) {
        return Err(format!("buffer {k} differs"));
    }
    let (mut mine, mut g) = (eng.to_vec(), globals.to_vec());
    ram::store(e, &mut mine, &mut g);
    let diff: Vec<String> = (0..ENGINE_SIZE)
        .filter(|&o| mine[o] != eng[o])
        .map(|o| format!("+{o:#x}"))
        .collect();
    if !diff.is_empty() || g != globals {
        return Err(format!("engine differs at {diff:?}"));
    }
    Ok(())
}

/// A call the game made into the engine (the functions the trace watches).
fn apply(e: &mut Engine, rom: Rom, addr: u32, r: [u32; 4]) {
    let i = |k: usize| r[k] as i32;
    match addr {
        0x0815_2E40 => _ = e.play_sfx(rom, r[0], i(1), i(2), i(3)),
        0x0815_2F44 => e.set_sfx(i(0), i(1), i(2)),
        0x0815_2F88 => e.stop_sfx(i(0)),
        0x0815_2FB8 => e.set_sfx_channel_volume(i(0), i(1)),
        0x0815_2FEC => e.set_sfx_rate(i(0), i(1)),
        0x0815_1758 => e.play_module(r[0]),
        0x0815_240C => e.stop_music(),
        0x0815_18D0 => e.set_master_volume(i(0)),
        0x0815_18E4 => e.set_music_volume(i(0)),
        0x0815_18F8 => e.set_sfx_volume(i(0)),
        0x0815_264C => e.set_loop(r[0]),
        _ => panic!("the game called {addr:#x}, which the replay does not support"),
    }
}

/// Frame by frame: load the engine as it was when the game called its per-frame update, run the rewrite's
/// update, and compare the mix buffers and every engine field with the game's after the call.
fn check_frames(name: &str, min_frames: usize) {
    let Some(rom) = rom() else { return };
    let Some(recs) = trace(name) else { return };
    let rom = Rom(&rom);
    let states: Vec<_> = recs.iter().filter(|r| matches!(r, Record::State { .. })).collect();
    let mut frames = 0;
    for pair in states.as_chunks::<2>().0 {
        let (
            Record::State {
                tag: 0,
                frame,
                globals,
                eng,
                buffers,
            },
            Record::State {
                tag: 1,
                frame: frame1,
                globals: g1,
                eng: e1,
                buffers: b1,
            },
        ) = (pair[0], pair[1])
        else {
            panic!("{name}: records out of order")
        };
        assert_eq!(frame, frame1);
        let mut e = ram::load(eng, globals, [&buffers[0], &buffers[1]]);
        e.update(rom);
        compare(&e, g1, e1, b1).unwrap_or_else(|err| panic!("{name} frame {frame}: {err}"));
        frames += 1;
    }
    eprintln!("{name}: {frames} frames exact");
    assert!(frames >= min_frames);
}

/// The rewrite running on its own from the first recorded state, fed only the API calls the game made:
/// every later state and buffer of the trace must come out the same.
fn replay(name: &str, min_frames: usize) {
    let Some(rom) = rom() else { return };
    let Some(recs) = trace(name) else { return };
    let rom = Rom(&rom);
    let Some(Record::State {
        globals, eng, buffers, ..
    }) = recs.first()
    else {
        panic!("{name}: empty")
    };
    let mut e = ram::load(eng, globals, [&buffers[0], &buffers[1]]);
    let (mut frames, mut calls) = (0, std::collections::BTreeMap::new());
    for (i, r) in recs.iter().enumerate() {
        match r {
            Record::State {
                tag,
                frame,
                globals,
                eng,
                buffers,
            } => {
                if *tag == 0 && i > 0 {
                    e.start_dma();
                }
                compare(&e, globals, eng, buffers)
                    .unwrap_or_else(|err| panic!("{name} frame {frame} tag {tag}: {err}"));
                if *tag == 0 {
                    e.update(rom);
                } else {
                    e.swap();
                    frames += 1;
                }
            }
            Record::Call { addr, regs } => {
                apply(&mut e, rom, *addr, *regs);
                *calls.entry(*addr).or_insert(0) += 1;
            }
        }
    }
    let calls: Vec<String> = calls.iter().map(|(a, n)| format!("{a:#x}×{n}")).collect();
    eprintln!("{name}: replayed {frames} frames exactly; API calls {calls:?}");
    assert!(frames >= min_frames);
}

#[test]
fn race_frames() {
    check_frames("race", 5400);
}

#[test]
fn main_menu_frames() {
    check_frames("mainmenu", 1800);
}

#[test]
fn race_replay() {
    replay("race", 5400);
}

#[test]
fn race_drive_replay() {
    replay("race-drive", 3600);
}

#[test]
fn main_menu_replay() {
    replay("mainmenu-nav", 2400);
}

/// Pausing the race stops every sound and the music and starts module 0 from its first row.
#[test]
fn race_pause_replay() {
    replay("race-pause", 600);
}

/// `Engine::new` sets up what the game's start-up leaves in the fields that do not change afterwards.
#[test]
fn new_matches_the_game_setup() {
    let Some(rom) = rom() else { return };
    let Some(recs) = trace("race") else { return };
    let rom = Rom(&rom);
    let Some(Record::State {
        globals, eng, buffers, ..
    }) = recs.first()
    else {
        panic!()
    };
    let g = ram::load(eng, globals, [&buffers[0], &buffers[1]]);
    let e = Engine::new(
        rom,
        (g.music_volume as u32).div_ceil(4),
        (g.sfx_volume as u32).div_ceil(4),
    );
    let fixed = |e: &Engine| {
        (
            e.buffer_addr,
            e.rate,
            e.requested_rate,
            e.note_steps,
            e.running,
            e.hold,
            e.mix_rate,
            e.bank,
        )
    };
    assert_eq!(fixed(&e), fixed(&g));
    assert_eq!(e.sample_addr, g.sample_addr);
    let more = |e: &Engine| {
        let v = (e.master_volume, e.music_volume, e.sfx_volume, e.loop_music, e.max_sfx);
        (
            e.sfx_table,
            e.sfx_data,
            e.sfx_count,
            v,
            e.buffer_len,
            e.frame_len,
            e.mode,
        )
    };
    assert_eq!(more(&e), more(&g));
}

/// The rewrite on its own: a fresh engine starts the race music with the timing the race trace shows.
#[test]
fn plays_a_module_from_scratch() {
    let Some(rom) = rom() else { return };
    let rom = Rom(&rom);
    let mut e = Engine::new(rom, 16, 16);
    e.carbon_play_music(rom, 4);
    let mut heard = Vec::new();
    for _ in 0..60 {
        heard.extend(e.vblank(rom));
    }
    let p = &e.player;
    assert_eq!(
        (p.module, p.music_channels, p.sfx_channels, e.playing),
        (0x0805_07FC, 1, 4, 1)
    );
    assert_eq!((p.tick_hz, p.row_samples, p.tick_samples), (39, 1617, 269));
    assert_eq!(p.row, 60 * 176 / 1617 + 1);
    assert!(heard.iter().any(|&s| s != 0));
}
