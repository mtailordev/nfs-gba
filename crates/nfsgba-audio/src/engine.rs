//! The LS_Play player: per-frame update, sequencer (rows and ticks), effects, voices and the game's sound API.
//! Each function names the game function it reproduces; field comments give the offset in the game's
//! structures (engine work area, module state at engine `+0x5C`, channels of 0x98 bytes at module state
//! `+0x54`), see `docs/formats/audio.md`.

use crate::Rom;
use crate::format::{MODULE_TABLE, SAMPLE_BANK, SFX_SLOTS, SFX_TABLE, Stream};
use crate::mixer::{self, Voice};

/// Samples per frame for each rate index (`u16`).
pub const SAMPLES_PER_FRAME: u32 = 0x087C_03A2;
/// Mixing rate in Hz for each rate index (`u32`); the hardware rate is 2^24 / (0x10000 - timer reload).
pub const RATE_HZ: u32 = 0x087C_03B4;
/// Timer 0 reload for each rate index (`u16`).
pub const TIMER_RELOAD: u32 = 0x087C_0390;
/// For each rate index, a table of per-note steps (linear pitch mode only).
pub const NOTE_STEP_TABLES: u32 = 0x087F_5BA4;
/// Note -> period table of the linear pitch mode (engine `+0x2C`).
pub const LINEAR_PERIODS: u32 = 0x0815_3BB4;
/// 768 `u16` frequencies of one octave (period mode).
pub const FREQ_TABLE: u32 = 0x087B_FD60;
/// 32-byte half sine for vibrato.
pub const VIBRATO_SINE: u32 = 0x087B_FD40;
/// Where the game places the mixer code and the two mix buffers (IWRAM, allocated from 0x03005A00).
pub const MIX_BUFFERS: [u32; 2] = [0x0300_5DEC, 0x0300_5E9C];

/// BIOS `Div` (SWI 6): signed, truncating. The game never divides by zero here (a zero step or tick rate
/// needs data Carbon does not have); the real BIOS then hangs, so this returns what mGBA's HLE BIOS does.
fn div(n: i32, d: i32) -> i32 {
    if d == 0 {
        return if n < 0 { -1 } else { 1 };
    }
    n.wrapping_div(d)
}

/// Linear mode: step from a period value (`0x6C3E1D / (2 * period)`, then scaled to the mixing rate).
fn linear_step(mix_rate: i32, period: i32) -> u32 {
    let d = if period.wrapping_mul(2) == 0 {
        1
    } else {
        period.wrapping_mul(2)
    };
    div(div(0x006C_3E1D, d) << 12, mix_rate) as u32
}

/// A mixer channel (0x98 bytes). Channels `0..music` belong to the module, the next `sfx` to sound effects.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Channel {
    /// `+0x00` sample address; moves to the loop start at the first wrap. 0 = channel never used.
    pub sample: u32,
    /// `+0x04` position and `+0x08` step, both 20.12 fixed point bytes.
    pub pos: u32,
    pub step: u32,
    /// `+0x0C` end (20.12); becomes the loop length after the first wrap, 0 = stopped.
    pub end: u32,
    /// `+0x10` loop start in bytes (cleared after the first wrap) and `+0x14` loop length (20.12).
    pub loop_start: u32,
    pub loop_len: u32,
    /// `+0x18` volume (0..64 for music) and `+0x1C` the sound effect's set volume.
    pub volume: i32,
    pub sfx_volume: i32,
    /// `+0x20..+0x2C`: set by `play_sfx` for flagged effects (another sample format, mode-0 mixer only).
    pub sfx_state: [u32; 4],
    /// `+0x3C` relative note, `+0x40` sample index (or sound id), `+0x44` fine tune.
    pub rel_note: i16,
    pub instrument: u16,
    pub finetune: i16,
    /// `+0x46` note; the period after a note starts in period mode; 0xFFFF for sound effects.
    pub period: u16,
    /// `+0x48` running effect (-1 none).
    pub effect: i32,
    /// `+0x4C`, `+0x4E` volume slide up and down.
    pub slide_up: u16,
    pub slide_down: u16,
    /// `+0x50`, `+0x52` arpeggio offsets (stored, never applied by this player).
    pub arpeggio: [u16; 2],
    /// `+0x54` vibrato offset (linear mode), `+0x56` depth, `+0x58` speed, `+0x5A` position.
    pub vib_offset: i16,
    pub vib_depth: u16,
    pub vib_speed: u16,
    pub vib_pos: u16,
    /// `+0x5E` pitch offset (linear mode slides), `+0x60` portamento speed, `+0x62` slide per tick,
    /// `+0x64` portamento target note.
    pub pitch_offset: i16,
    pub porta_speed: i16,
    pub slide: i16,
    pub porta_note: i16,
    /// `+0x78` pattern-loop row and `+0x7A` count (E6x).
    pub loop_row: u16,
    pub loop_count: u16,
    /// `+0x7C` sound-effect format flag.
    pub sfx_flag: u16,
    /// `+0x80` period slide, `+0x84` vibrato period offset, `+0x88` last slide parameter.
    pub period_slide: i32,
    pub vib_period: i32,
    pub slide_param: u32,
    /// `+0x8C` volume column of the current row (volume + 1).
    pub volume_column: u32,
    /// `+0x94` tone-portamento target period (period mode).
    pub porta_target: i32,
}

/// The pattern decoder of one music channel (0x58 bytes at module state `+0x514`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    /// `+0x00` note and instrument, `+0x20` volume column, `+0x30` effect and parameter.
    pub note: Stream,
    pub volume: Stream,
    pub effect: Stream,
    /// `+0x50..+0x55`: the row as decoded: note (9 bits over two bytes), instrument, volume, effect, param.
    pub cell: [u8; 6],
}

/// The module state (0x7E0 bytes at engine `+0x5C`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Player {
    /// `+0x00` module address, `+0x04` linear pitch mode, `+0x08` rows per pattern.
    pub module: u32,
    pub linear: u32,
    pub rows: u32,
    /// `+0x0C`: makes `B` end the pattern instead of jumping (never set in Carbon).
    pub break_pending: u32,
    /// `+0x14` speed (ticks per row), `+0x18` tempo lock (never set), `+0x3C` tempo.
    pub speed: u32,
    pub tempo_lock: u16,
    pub tempo: u32,
    /// `+0x1C` ticks per second: `tempo * 50 / 125` (whole).
    pub tick_hz: i32,
    /// `+0x20`/`+0x24` samples to the next row and per row; `+0x2C`/`+0x30` the same for ticks. The two run
    /// independently: a row is `speed * rate / tick_hz` samples, not `speed` ticks.
    pub row_countdown: i32,
    pub row_samples: i32,
    pub tick_countdown: i32,
    pub tick_samples: i32,
    /// `+0x38` tick within the row, `+0x3A` arpeggio tick.
    pub tick: u16,
    pub arp_tick: u16,
    /// `+0x40` row, `+0x44` order position, `+0x48` times the song wrapped.
    pub row: u32,
    pub order: u32,
    pub loops: u32,
    /// `+0x4C` pattern break done this row.
    pub break_done: u8,
    /// `+0x50` pattern data (module `+0x650`).
    pub patterns: u32,
    pub channels: [Channel; 8],
    pub tracks: [Track; 8],
    /// `+0x7D4` music channels, `+0x7D8` sound-effect channels, `+0x7DC` both.
    pub music_channels: i32,
    pub sfx_channels: i32,
    pub total_channels: i32,
}

/// The engine work area (0x160C bytes; engine pointer at 0x03006370).
#[derive(Clone, Debug, PartialEq)]
pub struct Engine {
    /// The two mix buffers (`+0x00`, `+0x04`: IWRAM addresses) and their contents (signed 8-bit samples).
    pub buffer_addr: [u32; 2],
    pub buffers: [Vec<u8>; 2],
    /// `+0x08` buffer the DMA plays this frame, `+0x0C` the one it played last frame.
    pub current: u32,
    pub previous: u32,
    /// `+0x10` set to -1001 when a frame starts without a buffer swap.
    pub underrun: i32,
    /// `+0x20` active and `+0x24` requested rate index.
    pub rate: u32,
    pub requested_rate: u32,
    /// `+0x28` note step table (linear mode) and `+0x2C` note period table.
    pub note_steps: u32,
    pub periods: u32,
    /// `+0x34` running.
    pub running: u32,
    /// `+0x44`: nonzero skips mixing (never set in Carbon).
    pub hold: u32,
    /// `+0x48` mixing rate in Hz.
    pub mix_rate: i32,
    /// `+0x50` music fade-in, 0..64 (+3 per frame).
    pub fade: i32,
    pub player: Player,
    /// `+0x101C` sample bank and `+0x1020` the address of each of its 256 samples.
    pub bank: u32,
    pub sample_addr: Vec<u32>,
    /// `+0x1420` sound-effect table entries, `+0x1424` their data, `+0x1438` how many.
    pub sfx_table: u32,
    pub sfx_data: u32,
    pub sfx_count: u32,
    /// `+0x1428` master, `+0x142C` music and `+0x1430` sound-effect volume (0..64).
    pub master_volume: i32,
    pub music_volume: i32,
    pub sfx_volume: i32,
    /// `+0x143C` music playing, `+0x1440` loop the music, `+0x1444` most sound-effect channels.
    pub playing: u16,
    pub loop_music: u32,
    pub max_sfx: i32,
    /// `+0x144C` buffer allocation and `+0x1450` samples mixed per frame.
    pub buffer_len: u32,
    pub frame_len: u32,
    /// `+0x1474` mixer mode (1 in Carbon; the mode-0 mixer is not rewritten).
    pub mode: u32,
    /// `+0x1478` module to start at the next update.
    pub music_request: u32,
    /// `+0x1484`, `+0x1488`: where and how much the last mixer call wrote.
    pub last_out: u32,
    pub last_count: i32,
    /// 0x03006378: mixer driver calls.
    pub mix_calls: u32,
}

impl Engine {
    /// The engine after the game's sound start-up (`FUN_08135df8` -> `FUN_08151f78(config, 0, 4)`): rate index
    /// 0 (10512 Hz, 176 samples a frame), four sound-effect channels, mixer mode 1, master volume 64, music and
    /// sound volumes from the options (`option * 4`, at most 63), music looping.
    pub fn new(rom: Rom, music_option: u32, sfx_option: u32) -> Engine {
        let rate = 0;
        let frame_len = rom.u16(SAMPLES_PER_FRAME + 2 * rate) as u32;
        let mut e = Engine {
            buffer_addr: MIX_BUFFERS,
            buffers: [vec![0; frame_len as usize], vec![0; frame_len as usize]],
            current: 0,
            previous: 1,
            underrun: 0,
            rate,
            requested_rate: rate,
            note_steps: rom.u32(NOTE_STEP_TABLES + 4 * rate),
            periods: 0,
            running: 1,
            hold: 0,
            mix_rate: rom.u32(RATE_HZ + 4 * rate) as i32,
            fade: 0,
            player: Player {
                sfx_channels: 4,
                total_channels: 4,
                ..Player::default()
            },
            bank: 0,
            sample_addr: vec![0; 256],
            sfx_table: SFX_TABLE + 4,
            sfx_data: 0,
            sfx_count: rom.u32(SFX_TABLE),
            master_volume: 0x40,
            music_volume: (music_option * 4).min(0x3F) as i32,
            sfx_volume: (sfx_option * 4).min(0x3F) as i32,
            playing: 0,
            loop_music: 1,
            max_sfx: 4,
            buffer_len: frame_len,
            frame_len,
            mode: 1,
            music_request: 0,
            last_out: 0,
            last_count: 0,
            mix_calls: 0,
        };
        e.sfx_data = e.sfx_table + 24 * e.sfx_count;
        e.set_bank(rom, SAMPLE_BANK);
        e
    }

    /// `FUN_08151ec4`: sample addresses of a bank.
    fn set_bank(&mut self, rom: Rom, bank: u32) {
        let mut data = 0;
        for i in 0..256 {
            let len = rom.u32(bank + 16 * i);
            self.sample_addr[i as usize] = if len == 0 { 0 } else { bank + data + 0x1000 };
            data += len;
        }
        self.bank = bank;
    }

    /// One VBlank's worth of sound, as the game's VBlank handler does it (`0x0812AC14`): restart the DMA on
    /// the current buffer (`FUN_08151928`), mix the other one (`FUN_08151b10`), swap (`FUN_08151c48`).
    /// Returns the samples the hardware plays during this frame.
    pub fn vblank(&mut self, rom: Rom) -> Vec<u8> {
        self.start_dma();
        let playing = self.buffers[self.current as usize][..self.frame_len as usize].to_vec();
        self.update(rom);
        self.swap();
        playing
    }

    /// `FUN_08151928`: the DMA restarts on buffer `current` (hardware, not modelled); an unswapped buffer is
    /// flagged.
    pub fn start_dma(&mut self) {
        if self.previous == self.current {
            self.underrun = -1001;
        }
    }

    /// `FUN_08151c48`: after mixing, the next frame plays the buffer just mixed.
    pub fn swap(&mut self) {
        if self.running != 0 {
            self.previous = self.current;
            self.current ^= 1;
        }
    }

    /// `FUN_08151b10`: per-frame update; mixes the next frame into the buffer the DMA is not playing.
    pub fn update(&mut self, rom: Rom) {
        if self.running == 0 {
            return;
        }
        self.apply_rate(rom);
        self.start_requested_music(rom);
        // FUN_081517e8 switches to and from a second module state for jingles; nothing in Carbon requests
        // one (FUN_081522b8 and FUN_08152310 have no callers), so it never does anything.
        let back = (self.current ^ 1) as usize;
        if self.playing == 0 {
            self.buffers[back][..self.frame_len as usize].fill(0);
        }
        // FUN_08151aa8 (a test tone replacing the mix) needs engine +0x14, which nothing sets.
        if self.hold == 0 && self.mix_rate != 0 {
            self.mix(rom, back, 0, self.player.total_channels);
        }
    }

    /// `FUN_081519d0`: switch the mixing rate when requested (never in Carbon), then fade the music in.
    fn apply_rate(&mut self, rom: Rom) {
        if self.rate != self.requested_rate {
            let r = self.requested_rate;
            self.rate = r;
            self.mix_rate = rom.u32(RATE_HZ + 4 * r) as i32;
            self.note_steps = rom.u32(NOTE_STEP_TABLES + 4 * r);
            self.frame_len = rom.u16(SAMPLES_PER_FRAME + 2 * r) as u32;
            let p = &mut self.player;
            p.tick_hz = div(((p.tempo as i32).wrapping_mul(50)) << 16, 125) >> 16;
            p.row_samples = div((p.speed as i32).wrapping_mul(self.mix_rate), p.tick_hz);
            for k in 0..8 {
                self.update_step(rom, k);
            }
            let n = self.buffer_len as usize;
            self.buffers.iter_mut().for_each(|b| b[..n].fill(0));
        }
        if self.fade != 0x40 {
            self.fade += 3;
            if self.fade > 0x40 {
                self.fade = 0x40;
            }
        }
    }

    /// `FUN_0815176c`: start the module requested with [`Engine::play_module`].
    fn start_requested_music(&mut self, rom: Rom) {
        let module = self.music_request;
        if module != 0 {
            self.music_request = 0;
            self.player.loops = 0;
            self.stop_music();
            if self.load_module(rom, module) {
                self.start_module(rom, 0);
            }
        }
    }

    /// `FUN_08151e04`: take a module's header into the module state; false if it is not GBAMOD30.
    fn load_module(&mut self, rom: Rom, module: u32) -> bool {
        let p = &mut self.player;
        p.speed = rom.u32(module + 0x148);
        p.tempo = rom.u32(module + 0x144);
        if rom.bytes(module, 8) != crate::format::MAGIC {
            return false;
        }
        let channels = rom.u32(module + 0x28) as i32;
        p.music_channels = channels.min(8);
        p.sfx_channels = (8 - channels).min(self.max_sfx);
        p.total_channels = p.music_channels + p.sfx_channels;
        p.rows = rom.u32(module + 0x13C);
        p.linear = rom.u32(module + 0x140);
        p.module = module;
        p.patterns = module + 0x650;
        true
    }

    /// `FUN_081521b4`: start the loaded module at an order position.
    fn start_module(&mut self, rom: Rom, order: i32) {
        if self.mix_rate == 0 || order >= rom.u32(self.player.module + 0x34) as i32 {
            return;
        }
        self.fade = 0x40;
        let rate = self.mix_rate;
        let p = &mut self.player;
        p.tick_hz = div(((p.tempo as i32).wrapping_mul(50)) << 16, 125) >> 16;
        p.row_samples = div((p.speed as i32).wrapping_mul(rate), p.tick_hz);
        self.periods = LINEAR_PERIODS;
        p.order = order as u32;
        p.row = 0;
        p.row_countdown = 0;
        p.tick_samples = div(rate, p.tick_hz);
        p.tick_countdown = 0;
        for ch in p.channels.iter_mut() {
            (ch.volume, ch.end, ch.loop_len) = (0, 0, 0);
        }
        p.break_done = 0;
        p.tick = 0;
        let n = self.frame_len as usize;
        self.buffers.iter_mut().for_each(|b| b[..n].fill(0));
        p.break_pending = 0;
        p.tempo_lock = 0;
        self.playing = 1;
        p.total_channels = p.music_channels + p.sfx_channels;
    }

    /// `FUN_08152660` in mode 1: mix `frame_len` samples of channels `first..` into buffer `buf`, running rows
    /// and ticks as their sample countdowns reach zero and cutting segments at every row, tick and sample end.
    fn mix(&mut self, rom: Rom, buf: usize, first: i32, count: i32) {
        self.mix_calls = self.mix_calls.wrapping_add(1);
        let (mut done, mut out) = (0i32, 0usize);
        while done < self.frame_len as i32 {
            let len = self.frame_len as i32;
            let mut n = if self.playing == 0 {
                len
            } else {
                if self.player.row_countdown == 0 {
                    self.row(rom);
                    self.player.row_countdown = self.player.row_samples;
                }
                if self.player.tick_countdown == 0 {
                    self.tick(rom);
                    self.player.tick_countdown = self.player.tick_samples;
                }
                let n = self.player.row_countdown.min(self.player.tick_countdown);
                if done + n > len { len - done } else { n }
            };
            let p = &mut self.player;
            if first < p.total_channels {
                let last = if first + count > p.total_channels {
                    p.music_channels - first
                } else {
                    count
                };
                let mut voices = Vec::with_capacity(8);
                for k in first..last {
                    let ch = &mut p.channels[k as usize];
                    if ch.sample == 0 {
                        continue;
                    }
                    let ptr = ch.sample.wrapping_add(ch.pos >> 12);
                    if ch.end < 2 {
                        ch.volume = 0;
                    }
                    voices.push(if ch.end == 0 || ch.volume == 0 {
                        Voice {
                            ptr,
                            ..Voice::default()
                        }
                    } else {
                        let v = if k < p.music_channels {
                            (((((ch.volume * self.music_volume) >> 6) * self.master_volume) >> 6) * self.fade) >> 6
                        } else {
                            (((ch.volume * self.sfx_volume) >> 6) * self.master_volume) >> 6
                        };
                        Voice {
                            ptr,
                            vol: v as u32,
                            frac: ch.pos << 20,
                            frac_step: ch.step << 20,
                            step: ch.step >> 12,
                        }
                    });
                    // Cut the segment where the sample ends (or loops).
                    if ch.end != 0 && ch.volume != 0 {
                        let to = ch.pos.wrapping_add((n as u32).wrapping_mul(ch.step));
                        if ch.end.wrapping_sub(0x1000) < to {
                            n -= div(to.wrapping_sub(ch.end) as i32, ch.step as i32);
                        }
                    }
                }
                if done + n > len {
                    n = len - done;
                }
                if self.playing != 0 {
                    n = n.min(p.row_countdown).min(p.tick_countdown);
                }
                if self.mode == 1 && !voices.is_empty() {
                    if n > 0 {
                        self.last_out = self.buffer_addr[buf].wrapping_add(out as u32);
                        self.last_count = n;
                        mixer::mix(rom, &mut voices, &mut self.buffers[buf], out, n as usize);
                    }
                    for ch in &mut p.channels[first.max(0) as usize..last.max(first) as usize] {
                        if ch.end != 0 || ch.volume != 0 {
                            let pos = ch.pos.wrapping_add(ch.step.wrapping_mul(n as u32));
                            ch.pos = pos;
                            if pos >= ch.end {
                                ch.sample = ch.sample.wrapping_add(ch.loop_start);
                                ch.pos = pos.wrapping_sub(ch.end);
                                ch.end = ch.loop_len;
                                ch.loop_start = 0;
                            }
                        }
                    }
                }
            }
            out = out.wrapping_add(n as usize);
            done += n;
            p.row_countdown -= n;
            p.tick_countdown -= n;
        }
    }

    /// `FUN_08152550`: read one row for every music channel, trigger it, advance the song.
    fn row(&mut self, rom: Rom) {
        if self.playing == 0 {
            return;
        }
        let p = &mut self.player;
        (p.tick, p.arp_tick, p.break_done) = (0, 0, 0);
        let row = p.row;
        let mut next = 0u32;
        let mut k = 0;
        while k < p.music_channels {
            next = Self::read_cell(rom, p, k as usize, next, row);
            k += 1;
        }
        p.row = p.row.wrapping_add(1);
        let mut k = 0;
        while k < self.player.music_channels {
            self.trigger(rom, k as usize);
            k += 1;
        }
        let p = &mut self.player;
        if p.row == p.rows {
            p.order = p.order.wrapping_add(1);
            if p.order >= rom.u32(p.module + 0x34) {
                p.order = 0;
                p.loops = p.loops.wrapping_add(1);
                if self.loop_music == 0 {
                    self.playing = 0;
                }
            }
            p.row = 0;
        }
    }

    /// `FUN_0815248c`: decode channel `k`'s cell for `row`. On row 0 the streams restart at the channel's block
    /// of the order's pattern (`prev` = the address after the previous channel's block, 0 for channel 0);
    /// returns the address after this channel's block (0 on other rows).
    fn read_cell(rom: Rom, p: &mut Player, k: usize, prev: u32, row: u32) -> u32 {
        let t = &mut p.tracks[k];
        if row != 0 {
            let a = t.note.read(rom, 2, None);
            let v = t.volume.read(rom, 1, None);
            let e = t.effect.read(rom, 2, None);
            t.cell = [
                (a >> 7) as u8,
                (a >> 15) as u8,
                (a & 0x7F) as u8,
                v as u8,
                (e >> 8) as u8,
                e as u8,
            ];
            return 0;
        }
        let block = if prev == 0 {
            let pattern = rom.u8(p.module.wrapping_add(0x14C).wrapping_add(p.order));
            p.patterns.wrapping_add(rom.u32(p.module + 0x24C + 4 * pattern as u32))
        } else {
            prev
        };
        let first = block + 0x14;
        let a = t.note.read(rom, 2, Some(first));
        let v = t.volume.read(rom, 1, Some(first.wrapping_add(rom.u32(block + 4))));
        let e = t.effect.read(rom, 2, Some(first.wrapping_add(rom.u32(block + 8))));
        t.cell = [
            (a >> 7) as u8,
            (a >> 15) as u8,
            (a & 0x7F) as u8,
            v as u8,
            (e >> 8) as u8,
            e as u8,
        ];
        first.wrapping_add(rom.u32(block + 0x10))
    }

    /// `FUN_08153654`: start channel `k`'s row: note, instrument, volume column, effect.
    fn trigger(&mut self, rom: Rom, k: usize) {
        let cell = self.player.tracks[k].cell;
        let mut note = (cell[0] as u32 | (cell[1] as u32) << 8) & 0x1FF;
        let mut ins = cell[2] as u32;
        let (eff, param) = (cell[4] as u32, cell[5] as u32);
        let linear = self.player.linear != 0;
        let ch = &mut self.player.channels[k];
        ch.volume_column = cell[3] as u32;
        if eff & 0xF == 3 {
            if note != 0x1FF {
                ch.porta_note = note as i16;
            }
            if ins != 0 {
                ch.volume = rom.u16(self.bank.wrapping_add(16 * ins).wrapping_sub(0x10) + 6) as i32;
            }
            (ins, note) = (0, 0x1FF);
        }
        ch.effect = -1;
        ch.vib_offset = 0;
        if note == 0x1FF {
            if ins != 0 {
                ch.instrument = (ins - 1) as u16;
                ch.sample = self.sample_addr[ch.instrument as usize];
                ch.volume = rom.u16(self.bank.wrapping_add(16 * ch.instrument as u32) + 6) as i32;
                ch.sfx_flag = 0;
            }
        } else {
            if ins != 0 {
                ch.instrument = (ins - 1) as u16;
                ch.volume = rom.u16(self.bank.wrapping_add(16 * ch.instrument as u32) + 6) as i32;
            }
            let h = self.bank.wrapping_add(16 * ch.instrument as u32);
            ch.sample = self.sample_addr[ch.instrument as usize];
            ch.end = 0;
            ch.loop_len = 0;
            let len = rom.u32(h);
            if len != 0 {
                ch.end = len << 12;
            }
            let loop_len = rom.u16(h + 10) as u32;
            if loop_len != 0 {
                ch.loop_len = loop_len << 12;
            }
            ch.loop_start = rom.u16(h + 8) as u32;
            ch.finetune = rom.u16(h + 4) as i16;
            ch.rel_note = rom.u16(h + 12) as i16;
            ch.period = note as u16;
            ch.pos = 0;
            ch.sfx_flag = 0;
            if eff != 3 {
                ch.period_slide = 0;
            }
            ch.vib_period = 0;
            ch.slide_param = 0;
            ch.vib_offset = 0;
            if linear {
                if ch.pitch_offset == 0 && ch.finetune == 0 {
                    ch.step = rom.u32(self.note_steps.wrapping_add(4 * note));
                } else {
                    let table = rom.u16(self.periods.wrapping_add(2 * (note as i32 + ch.finetune as i32) as u32));
                    ch.step = linear_step(self.mix_rate, ch.pitch_offset as i32 + table as i32);
                }
            } else {
                self.note_period(rom, k);
                self.player.channels[k].porta_target = 0;
            }
            let ch = &mut self.player.channels[k];
            if eff != 3 {
                ch.pitch_offset = 0;
            }
        }
        let tick = self.player.tick as i16 as i32;
        let ch = &mut self.player.channels[k];
        ch.arpeggio = [0, 0];
        match eff {
            0 => {
                ch.effect = 0;
                ch.arpeggio = [((param >> 4) << 3) as u16, ((param & 0xF) << 3) as u16];
            }
            1 | 2 => {
                ch.effect = eff as i32;
                if linear {
                    ch.slide = if eff == 1 { -(param as i32) } else { param as i32 } as i16;
                } else {
                    slide_period(ch, tick, param, eff == 1);
                    self.update_step(rom, k);
                }
            }
            3 => {
                ch.effect = 3;
                if param != 0 {
                    ch.porta_speed = param as i16;
                }
                if linear {
                    let (target, cur) = (ch.porta_note as i32, ch.period as i32);
                    ch.slide = match target.cmp(&cur) {
                        std::cmp::Ordering::Equal => 0,
                        std::cmp::Ordering::Less => ch.porta_speed as u16 as i16,
                        std::cmp::Ordering::Greater => (ch.porta_speed as u16 as i32).wrapping_neg() as i16,
                    };
                } else {
                    let n = ((ch.rel_note as i32 - 1 + ch.porta_note as i32 - 1) as u32) << 16 >> 10;
                    let p = 0x1DC0u32
                        .wrapping_sub(n)
                        .wrapping_sub(((ch.finetune as i32) >> 1) as u32);
                    ch.porta_target = (p & 0xFFFF) as i32;
                }
            }
            4 => {
                ch.effect = 4;
                let (hi, lo) = ((param >> 4) as u16, (param & 0xF) as u16);
                if hi != 0 {
                    ch.vib_speed = hi;
                }
                if lo != 0 {
                    ch.vib_depth = lo;
                }
                if hi != 0 || lo != 0 {
                    ch.vib_pos = 0;
                }
            }
            5 | 6 | 10 => {
                ch.effect = eff as i32;
                (ch.slide_up, ch.slide_down) = ((param >> 4) as u16, (param & 0xF) as u16);
            }
            11 => {
                let p = &mut self.player;
                if p.break_pending != 0 {
                    p.row = p.rows;
                    p.break_pending = 0;
                } else {
                    // A jingle's B returns to the main music here (FUN_0815236c); Carbon has no jingles.
                    p.order = param.wrapping_sub(1);
                    p.row = p.rows;
                }
            }
            12 => ch.volume = param as i32,
            13 => {
                let p = &mut self.player;
                if p.break_done == 0 {
                    p.row = param;
                    p.order = p.order.wrapping_add(1);
                    if p.order >= rom.u32(p.module + 0x34) {
                        p.order = 0;
                    }
                    p.break_done = 1;
                }
            }
            14 if param & 0xF0 == 0x60 => {
                if param & 0xF == 0 {
                    ch.loop_row = self.player.row.wrapping_sub(1) as u16;
                } else {
                    if ch.loop_count != 0 {
                        ch.loop_count -= 1;
                    } else {
                        ch.loop_count = (param & 0xF) as u16;
                    }
                    if ch.loop_count != 0 {
                        self.player.row = ch.loop_row as u32;
                    }
                }
            }
            15 => {
                let rate = self.mix_rate;
                let p = &mut self.player;
                if p.tempo_lock == 0 {
                    if param <= 0x1F {
                        p.speed = param;
                    } else {
                        p.tempo = param;
                    }
                    p.tick_hz = div(((p.tempo as i32).wrapping_mul(50)) << 16, 125) >> 16;
                    p.row_samples = div(rate.wrapping_mul(p.speed as i32), p.tick_hz);
                    p.tick_samples = div(rate, p.tick_hz);
                }
            }
            _ => {}
        }
        volume_column(&mut self.player.channels[k]);
    }

    /// `FUN_08153354`: one tick of the running effects on every music channel.
    fn tick(&mut self, rom: Rom) {
        if self.playing == 0 {
            return;
        }
        let linear = self.player.linear != 0;
        let mut k = 0;
        while k < self.player.music_channels {
            let tick = self.player.tick as i16 as i32;
            let ku = k as usize;
            let ch = &mut self.player.channels[ku];
            match ch.effect {
                0 => {
                    let t = self.player.arp_tick as i16;
                    if t != 0 && ch.arpeggio[0] as u32 + ch.arpeggio[1] as u32 != 0 {
                        if t == 4 {
                            self.player.arp_tick = 1;
                        }
                        let ch = &mut self.player.channels[ku];
                        ch.step = if linear {
                            let table = rom.u16(
                                self.periods
                                    .wrapping_add(2 * (ch.finetune as i32 + ch.period as i32) as u32),
                            );
                            linear_step(
                                self.mix_rate,
                                ch.pitch_offset as i32 + table as i32 + ch.vib_offset as i32,
                            )
                        } else {
                            // Indexes the note table with the period: the game's arpeggio is broken here.
                            rom.u32(self.note_steps.wrapping_add(4 * ch.period as u32))
                        };
                    }
                }
                1 | 2 => {
                    if linear {
                        ch.pitch_offset = ch.pitch_offset.wrapping_add(ch.slide);
                    } else {
                        slide_period(ch, tick, 0, ch.effect == 1);
                        self.update_step(rom, ku);
                    }
                }
                3 => self.tone_portamento(rom, ku),
                4 => self.vibrato(rom, ku),
                5 => {
                    self.tone_portamento(rom, ku);
                    self.volume_slide(ku);
                }
                6 => {
                    self.vibrato(rom, ku);
                    self.volume_slide(ku);
                }
                10 => self.volume_slide(ku),
                _ => {}
            }
            let ch = &self.player.channels[ku];
            if ch.arpeggio[0] as u32 + ch.arpeggio[1] as u32 == 0 && linear {
                let step = if ch.pitch_offset == 0 && ch.vib_offset == 0 && ch.finetune == 0 {
                    rom.u32(self.note_steps.wrapping_add(4 * ch.period as u32))
                } else {
                    let table = rom.u16(
                        self.periods
                            .wrapping_add(2 * (ch.finetune as i32 + ch.period as i32) as u32),
                    );
                    linear_step(
                        self.mix_rate,
                        ch.pitch_offset as i32 + table as i32 + ch.vib_offset as i32,
                    )
                };
                self.player.channels[ku].step = step;
            }
            volume_column(&mut self.player.channels[ku]);
            k += 1;
        }
        let p = &mut self.player;
        p.tick = p.tick.wrapping_add(1);
        p.arp_tick = p.arp_tick.wrapping_add(1);
    }

    /// `FUN_081530b0`: tone portamento, one tick.
    fn tone_portamento(&mut self, rom: Rom, k: usize) {
        let linear = self.player.linear != 0;
        let steps = self.note_steps;
        let ch = &mut self.player.channels[k];
        if ch.porta_target == 0 {
            return;
        }
        if linear {
            ch.pitch_offset = ch.pitch_offset.wrapping_add(ch.slide);
            let cur = rom
                .u32(steps.wrapping_add(4 * ch.period as u32))
                .wrapping_add(ch.pitch_offset as i32 as u32);
            let target = rom.u32(steps.wrapping_add(4 * ch.porta_note as i32 as u32));
            if if ch.slide > 0 { cur < target } else { cur > target } {
                return;
            }
            ch.period = ch.porta_note as u16;
            ch.slide = 0;
            ch.pitch_offset = 0;
            return;
        }
        let target = ch.porta_target;
        let diff = (ch.period as i32).wrapping_add(ch.period_slide).wrapping_sub(target);
        let speed = ch.porta_speed as i32 * 4;
        let arrived = if diff == 0 || speed > diff.wrapping_abs() {
            true
        } else if diff > 0 {
            ch.period_slide -= speed;
            ch.period as i32 + ch.period_slide <= target
        } else {
            ch.period_slide += speed;
            ch.period as i32 + ch.period_slide >= target
        };
        if arrived {
            ch.period_slide = target - ch.period as i32;
            ch.porta_target = 0;
        }
        self.update_step(rom, k);
    }

    /// `FUN_081531dc`: vibrato, one tick.
    fn vibrato(&mut self, rom: Rom, k: usize) {
        let ch = &mut self.player.channels[k];
        if self.player.linear != 0 {
            let s = rom.u8(VIBRATO_SINE.wrapping_add(ch.vib_pos as u32)) as u32;
            ch.vib_offset = ((ch.vib_depth as u32 * s) as i32 >> 7) as i16;
            ch.vib_pos = ch.vib_speed.wrapping_add(ch.vib_pos) & 0x3F;
        } else {
            let pos = ch.vib_pos as u32;
            let v = (rom.u8(VIBRATO_SINE + ((pos >> 2) & 0x1F)) as u32 * ch.vib_depth as u32) as i32 >> 5;
            ch.vib_period = if pos > 0x1F { -v } else { v };
            ch.vib_pos = ((((ch.vib_speed as u32 + 4) >> 1) + pos) & 0x3F) as u16;
            self.update_step(rom, k);
        }
    }

    /// `FUN_08153068`: volume slide (not on a row's first tick), clamped to 0..64.
    fn volume_slide(&mut self, k: usize) {
        let first_tick = self.player.tick as i16 == 0;
        let ch = &mut self.player.channels[k];
        if !first_tick {
            ch.volume = if ch.slide_up != 0 {
                ch.volume + ch.slide_up as i32
            } else {
                ch.volume - ch.slide_down as i32
            };
        }
        ch.volume = ch.volume.clamp(0, 0x40);
    }

    /// `FUN_081514a0`: turn channel `k`'s note into a period (kept in `period`) and set its step.
    fn note_period(&mut self, rom: Rom, k: usize) {
        let ch = &mut self.player.channels[k];
        let n = ((ch.rel_note as i32 - 1 + ch.period as i32 - 1) as u32) << 16 >> 10;
        let p = 0x1DC0u32
            .wrapping_sub(n)
            .wrapping_sub(((ch.finetune as i32) >> 1) as u32)
            & 0xFFFF;
        ch.period = p as u16;
        self.update_step(rom, k);
    }

    /// `FUN_08151500`: step from the period plus slide and vibrato offsets.
    fn update_step(&mut self, rom: Rom, k: usize) {
        let ch = &self.player.channels[k];
        let f = period_freq(
            rom,
            (ch.period as i32)
                .wrapping_add(ch.period_slide)
                .wrapping_add(ch.vib_period),
        );
        self.player.channels[k].step = div((f << 12) as i32, self.mix_rate) as u32;
    }

    // ---- API (the functions the game calls) ----

    /// `FUN_08151758`: start a module at the next update.
    pub fn play_module(&mut self, module: u32) {
        self.music_request = module;
    }

    /// `FUN_0815240c`: stop the music (silences the music channels).
    pub fn stop_music(&mut self) {
        self.playing = 0;
        let p = &mut self.player;
        for ch in p.channels.iter_mut().take(p.music_channels.max(0) as usize) {
            ch.volume = 0;
        }
    }

    /// `FUN_081516bc` (Carbon's sound shutdown, `0x08135f68`, first zeroes the three volumes and waits four
    /// frames): clear both buffers, stop the music and the engine; the DMA and timer stop (hardware).
    /// The game starts it again with a full re-initialisation (`0x08135ea4`), i.e. [`Engine::new`].
    pub fn shutdown(&mut self) {
        if self.running == 0 {
            return;
        }
        let n = self.frame_len as usize;
        self.buffers.iter_mut().for_each(|b| b[..n].fill(0));
        self.stop_music();
        self.running = 0;
    }

    /// `FUN_0815264c`: loop the music when it ends (else it stops).
    pub fn set_loop(&mut self, on: u32) {
        self.loop_music = on;
    }

    /// `FUN_081518d0`, `FUN_081518e4`, `FUN_081518f8`.
    pub fn set_master_volume(&mut self, v: i32) {
        self.master_volume = v;
    }
    pub fn set_music_volume(&mut self, v: i32) {
        self.music_volume = v;
    }
    pub fn set_sfx_volume(&mut self, v: i32) {
        self.sfx_volume = v;
    }

    fn sfx_channel(&mut self, slot: i32, limit: u32) -> Option<&mut Channel> {
        let k = slot.wrapping_add(self.player.music_channels) as u32;
        (k < limit).then(|| &mut self.player.channels[k as usize])
    }

    /// `FUN_08152e40`: play sound effect `id` on sound channel `slot` at `rate` Hz (0 = the effect's own)
    /// and `volume`. Returns `slot`, or -2001 for a bad id, -2002 for a bad slot.
    pub fn play_sfx(&mut self, rom: Rom, id: u32, rate: i32, slot: i32, volume: i32) -> i32 {
        if id >= self.sfx_count {
            return -2001;
        }
        if slot < 0 || slot >= self.player.sfx_channels {
            return -2002;
        }
        let e = self.sfx_table + 24 * id;
        let rate = if rate == 0 { rom.u32(e + 16) as i32 } else { rate };
        let (data, mix_rate) = (self.sfx_data, self.mix_rate);
        let ch = &mut self.player.channels[(self.player.music_channels + slot) as usize];
        ch.instrument = id as u16;
        ch.sample = data.wrapping_add(rom.u32(e));
        ch.end = rom.u32(e + 4) << 12;
        ch.loop_start = rom.u16(e + 12) as u32;
        ch.loop_len = (rom.u16(e + 14) as u32) << 12;
        ch.volume = volume;
        ch.sfx_volume = volume;
        ch.period = 0xFFFF;
        ch.pos = 0;
        ch.finetune = 0;
        ch.vib_offset = 0;
        ch.step = div(rate << 12, mix_rate) as u32;
        ch.sfx_flag = rom.u8(e + 20) as u16;
        if ch.sfx_flag != 0 {
            ch.end = (rom.u32(e + 4) * 2).wrapping_sub(0x40);
            ch.sfx_state = [0, 0, ch.sample, u32::MAX];
        }
        slot
    }

    /// `FUN_08152f44`: set a sound channel's volume and (if nonzero) rate.
    pub fn set_sfx(&mut self, slot: i32, rate: i32, volume: i32) {
        let mix_rate = self.mix_rate;
        if let Some(ch) = self.sfx_channel(slot, 8) {
            (ch.volume, ch.sfx_volume) = (volume, volume);
            if rate != 0 {
                ch.step = div(rate << 12, mix_rate) as u32;
            }
        }
    }

    /// `FUN_08152f88`: silence a sound channel.
    pub fn stop_sfx(&mut self, slot: i32) {
        let total = self.player.total_channels as u32;
        if let Some(ch) = self.sfx_channel(slot, total) {
            (ch.volume, ch.sfx_volume) = (0, 0);
        }
    }

    /// `FUN_08152fb8`: set a sound channel's volume.
    pub fn set_sfx_channel_volume(&mut self, slot: i32, volume: i32) {
        let total = self.player.total_channels as u32;
        if let Some(ch) = self.sfx_channel(slot, total) {
            (ch.volume, ch.sfx_volume) = (volume, volume);
        }
    }

    /// `FUN_08152fec`: set a sound channel's rate in Hz (0 is ignored).
    pub fn set_sfx_rate(&mut self, slot: i32, rate: i32) {
        let (total, mix_rate) = (self.player.total_channels as u32, self.mix_rate);
        if let Some(ch) = self.sfx_channel(slot, total).filter(|_| rate != 0) {
            ch.step = div(rate << 12, mix_rate) as u32;
        }
    }

    /// `FUN_08153034`: is a sound still playing (end above 2)?
    pub fn sfx_playing(&mut self, slot: i32) -> bool {
        self.sfx_channel(slot, 8).is_some_and(|ch| ch.end > 2)
    }
}

/// `FUN_08153344`: the volume column overrides the volume on every row and tick.
fn volume_column(ch: &mut Channel) {
    if ch.volume_column != 0 {
        ch.volume = ch.volume_column as i32 - 1;
    }
}

/// `FUN_081532e0` (`up`: period down) and `FUN_0815327c`: portamento in period mode. `param` 0 reuses the
/// last one; `Fx` and `Ex` are fine slides on the row's first tick, anything else slides on the other ticks.
fn slide_period(ch: &mut Channel, tick: i32, param: u32, up: bool) {
    let p = if param != 0 {
        ch.slide_param = param;
        param
    } else {
        ch.slide_param
    };
    let (hi, lo) = ((p >> 4) & 0xFF, p & 0xF);
    let delta = match hi {
        0xF if tick == 0 => lo * 4,
        0xE if tick == 0 => lo,
        0xE | 0xF => return,
        _ if tick != 0 => (p & 0xFFFF) * 4,
        _ => return,
    } as i32;
    ch.period_slide = if up {
        ch.period_slide.wrapping_sub(delta)
    } else {
        ch.period_slide.wrapping_add(delta)
    };
}

/// `FUN_0815145c`: frequency of a period: `FREQ_TABLE[(0x1E00 - p) % 0x300] * 4 >> (7 - (0x1E00 - p) / 0x300)`.
pub fn period_freq(rom: Rom, period: i32) -> u32 {
    let x = 0x1E00i32.wrapping_sub(period);
    let (octave, rem) = (div(x, 0x300), x.wrapping_rem(0x300));
    let v = (rom.u16(FREQ_TABLE.wrapping_add((rem * 2) as u32)) as u32) << 2;
    let shift = (7i32.wrapping_sub(octave) as u32) & 0xFF;
    if shift >= 32 { 0 } else { v >> shift }
}

/// Carbon's own sound functions (the game's side, 0x08135xxx), on top of the engine.
impl Engine {
    /// `0x08135fdc`: play sound `id` (1..=39) on its fixed slot (`SFX_SLOTS`) at the sound option's volume
    /// (`option * 4`; ids 0x20-0x21 use `option`, ids 0x0C-0x10 `option * 5`; at most 63). Returns what
    /// `play_sfx` returns, 0x7F for an id out of range.
    pub fn carbon_play_sound(&mut self, rom: Rom, id: u32, sfx_option: u32) -> i32 {
        if id.wrapping_sub(1) > 0x26 {
            return 0x7F;
        }
        let o = sfx_option.wrapping_mul(4);
        let v = if id.wrapping_sub(0x20) <= 1 {
            o >> 2
        } else if id.wrapping_sub(0xC) <= 4 {
            o + (o >> 2)
        } else {
            o
        };
        self.play_sfx(rom, id, 0, rom.u8(SFX_SLOTS + id) as i32, v.min(0x3F) as i32)
    }

    /// `0x08136028`: stop sound `id` (zero volume on its slot).
    pub fn carbon_stop_sound(&mut self, rom: Rom, id: u32) {
        if id.wrapping_sub(1) <= 0x26 {
            let slot = rom.u8(SFX_SLOTS + id) as i32;
            self.set_sfx(slot, 0, 0);
            self.stop_sfx(slot);
        }
    }

    /// `0x081360b4`: set sound `id`'s rate to `rate8 * 8` Hz.
    pub fn carbon_set_sound_rate(&mut self, rom: Rom, id: u32, rate8: i32) {
        if id.wrapping_sub(1) <= 0x26 {
            self.set_sfx_rate(rom.u8(SFX_SLOTS + id) as i32, rate8 << 3);
        }
    }

    /// `0x08135fb0`: set sound `id`'s volume.
    pub fn carbon_set_sound_volume(&mut self, rom: Rom, id: u32, volume: i32) {
        self.set_sfx_channel_volume(rom.u8(SFX_SLOTS.wrapping_add(id)) as i32, volume);
    }

    /// `0x08136054` without its "already playing" check (the caller keeps the current id at 0x0300003C):
    /// request music `id` (0..=4) from `MODULE_TABLE`.
    pub fn carbon_play_music(&mut self, rom: Rom, id: u32) {
        self.play_module(rom.u32(MODULE_TABLE + 4 * id));
    }
}
