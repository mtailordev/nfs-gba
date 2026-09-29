//! Offline renderer: the engine's output stream (what the Direct Sound FIFOs play) as 8-bit mono WAV.
//!
//!     nfsgba-audio-render music ID SECONDS OUT.wav     # music 0..=4 (Carbon's play_music ids)
//!     nfsgba-audio-render sfx ID OUT.wav               # sound 1..=39 through Carbon's play_sound, until it ends
//!     nfsgba-audio-render samples DIR                  # every bank sample and sound effect, raw
//!
//! Engine output is written at 10512 Hz; the hardware plays 2^24 / 1596 = 10512.04 Hz (176 samples a frame).
//! Keep the output out of git (it is derived from the ROM): put it under `$NFSGBA_DATA/out/`.

use nfsgba_audio::engine::period_freq;
use nfsgba_audio::format::{self, SAMPLE_BANK, SFX_TABLE};
use nfsgba_audio::{Engine, Rom};
use std::{env, fs, io, path::Path};

/// 8-bit WAV from signed samples.
fn wav(path: &Path, rate: u32, samples: &[u8]) -> io::Result<()> {
    let mut out = Vec::with_capacity(44 + samples.len());
    for part in [
        b"RIFF".as_slice(),
        &(36 + samples.len() as u32).to_le_bytes(),
        b"WAVEfmt ",
        &16u32.to_le_bytes(),
    ] {
        out.extend_from_slice(part);
    }
    for v in [1u16, 1] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for v in [rate, rate] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for v in [1u16, 8] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(samples.len() as u32).to_le_bytes());
    out.extend(samples.iter().map(|s| s ^ 0x80));
    fs::write(path, out)
}

fn main() -> io::Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    let rom = nfsgba_formats::canonical_rom()?;
    let rom = Rom(&rom);
    let num = |i: usize| {
        args.get(i)
            .and_then(|s| s.parse::<u32>().ok())
            .expect("see the usage in the source")
    };
    match args.first().map(String::as_str) {
        Some("music") => {
            let mut e = Engine::new(rom, 16, 16);
            e.carbon_play_music(rom, num(1));
            let frames = (num(2) as f64 * 16_777_216.0 / 280_896.0) as usize;
            let pcm: Vec<u8> = (0..frames).flat_map(|_| e.vblank(rom)).collect();
            wav(Path::new(&args[3]), 10512, &pcm)
        }
        Some("sfx") => {
            let mut e = Engine::new(rom, 16, 16);
            let id = num(1);
            let slot = e.carbon_play_sound(rom, id, 16);
            let mut pcm = Vec::new();
            // Two frames of latency, then until the sound ends or 10 seconds (looping sounds).
            while pcm.len() < 2 * 176
                || (e.sfx_playing(slot)
                    && e.player.channels[(e.player.music_channels + slot) as usize].volume > 0
                    && pcm.len() < 105_120)
            {
                pcm.extend(e.vblank(rom));
            }
            wav(Path::new(&args[2]), 10512, &pcm)
        }
        Some("samples") => {
            let dir = Path::new(&args[1]);
            fs::create_dir_all(dir)?;
            for (i, s) in format::sample_bank(rom, SAMPLE_BANK).iter().enumerate() {
                if let Some(s) = s {
                    // The rate Carbon's modules play them at: note 49 through the period formula.
                    let n = ((s.rel_note as i32 - 1 + 49 - 1) as u32) << 16 >> 10;
                    let p = 0x1DC0u32
                        .wrapping_sub(n)
                        .wrapping_sub(((s.finetune as i32) >> 1) as u32)
                        & 0xFFFF;
                    wav(
                        &dir.join(format!("sample{i:03}.wav")),
                        period_freq(rom, p as i32),
                        rom.bytes(s.addr, s.len),
                    )?;
                }
            }
            for (i, s) in format::sfx_table(rom, SFX_TABLE).iter().enumerate() {
                wav(&dir.join(format!("sfx{i:02}.wav")), s.rate, rom.bytes(s.addr, s.len))?;
            }
            Ok(())
        }
        _ => {
            eprintln!("usage: nfsgba-audio-render music ID SECONDS OUT.wav | sfx ID OUT.wav | samples DIR");
            Ok(())
        }
    }
}
