//! The game's RAM layout of the engine, field by field: load engine state from the reference build and write
//! it back (the trace tests compare every field the rewrite keeps). Offsets are documented on the fields.

use crate::engine::{Channel, Engine, Player};
use crate::format::Stream;

/// Size of the engine work area.
pub const ENGINE_SIZE: usize = 0x160C;

trait Io {
    fn u32(&mut self, off: usize, v: &mut u32);
    fn u16(&mut self, off: usize, v: &mut u16);
    fn u8(&mut self, off: usize, v: &mut u8);
    fn i32(&mut self, off: usize, v: &mut i32) {
        let mut u = *v as u32;
        self.u32(off, &mut u);
        *v = u as i32;
    }
    fn i16(&mut self, off: usize, v: &mut i16) {
        let mut u = *v as u16;
        self.u16(off, &mut u);
        *v = u as i16;
    }
}

struct Load<'a>(&'a [u8]);
impl Io for Load<'_> {
    fn u32(&mut self, o: usize, v: &mut u32) {
        *v = u32::from_le_bytes(self.0[o..o + 4].try_into().unwrap());
    }
    fn u16(&mut self, o: usize, v: &mut u16) {
        *v = u16::from_le_bytes([self.0[o], self.0[o + 1]]);
    }
    fn u8(&mut self, o: usize, v: &mut u8) {
        *v = self.0[o];
    }
}

struct Store<'a>(&'a mut [u8]);
impl Io for Store<'_> {
    fn u32(&mut self, o: usize, v: &mut u32) {
        self.0[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn u16(&mut self, o: usize, v: &mut u16) {
        self.0[o..o + 2].copy_from_slice(&v.to_le_bytes());
    }
    fn u8(&mut self, o: usize, v: &mut u8) {
        self.0[o] = *v;
    }
}

fn stream(io: &mut dyn Io, b: usize, s: &mut Stream) {
    io.u32(b, &mut s.ptr);
    io.i32(b + 4, &mut s.count);
    io.u32(b + 8, &mut s.value);
}

fn channel(io: &mut dyn Io, b: usize, c: &mut Channel) {
    io.u32(b, &mut c.sample);
    io.u32(b + 4, &mut c.pos);
    io.u32(b + 8, &mut c.step);
    io.u32(b + 0xC, &mut c.end);
    io.u32(b + 0x10, &mut c.loop_start);
    io.u32(b + 0x14, &mut c.loop_len);
    io.i32(b + 0x18, &mut c.volume);
    io.i32(b + 0x1C, &mut c.sfx_volume);
    for (k, v) in c.sfx_state.iter_mut().enumerate() {
        io.u32(b + 0x20 + 4 * k, v);
    }
    io.i16(b + 0x3C, &mut c.rel_note);
    io.u16(b + 0x40, &mut c.instrument);
    io.i16(b + 0x44, &mut c.finetune);
    io.u16(b + 0x46, &mut c.period);
    io.i32(b + 0x48, &mut c.effect);
    io.u16(b + 0x4C, &mut c.slide_up);
    io.u16(b + 0x4E, &mut c.slide_down);
    io.u16(b + 0x50, &mut c.arpeggio[0]);
    io.u16(b + 0x52, &mut c.arpeggio[1]);
    io.i16(b + 0x54, &mut c.vib_offset);
    io.u16(b + 0x56, &mut c.vib_depth);
    io.u16(b + 0x58, &mut c.vib_speed);
    io.u16(b + 0x5A, &mut c.vib_pos);
    io.i16(b + 0x5E, &mut c.pitch_offset);
    io.i16(b + 0x60, &mut c.porta_speed);
    io.i16(b + 0x62, &mut c.slide);
    io.i16(b + 0x64, &mut c.porta_note);
    io.u16(b + 0x78, &mut c.loop_row);
    io.u16(b + 0x7A, &mut c.loop_count);
    io.u16(b + 0x7C, &mut c.sfx_flag);
    io.i32(b + 0x80, &mut c.period_slide);
    io.i32(b + 0x84, &mut c.vib_period);
    io.u32(b + 0x88, &mut c.slide_param);
    io.u32(b + 0x8C, &mut c.volume_column);
    io.i32(b + 0x94, &mut c.porta_target);
}

fn player(io: &mut dyn Io, b: usize, p: &mut Player) {
    io.u32(b, &mut p.module);
    io.u32(b + 4, &mut p.linear);
    io.u32(b + 8, &mut p.rows);
    io.u32(b + 0xC, &mut p.break_pending);
    io.u32(b + 0x14, &mut p.speed);
    io.u16(b + 0x18, &mut p.tempo_lock);
    io.i32(b + 0x1C, &mut p.tick_hz);
    io.i32(b + 0x20, &mut p.row_countdown);
    io.i32(b + 0x24, &mut p.row_samples);
    io.i32(b + 0x2C, &mut p.tick_countdown);
    io.i32(b + 0x30, &mut p.tick_samples);
    io.u16(b + 0x38, &mut p.tick);
    io.u16(b + 0x3A, &mut p.arp_tick);
    io.u32(b + 0x3C, &mut p.tempo);
    io.u32(b + 0x40, &mut p.row);
    io.u32(b + 0x44, &mut p.order);
    io.u32(b + 0x48, &mut p.loops);
    io.u8(b + 0x4C, &mut p.break_done);
    io.u32(b + 0x50, &mut p.patterns);
    for (k, c) in p.channels.iter_mut().enumerate() {
        channel(io, b + 0x54 + 0x98 * k, c);
    }
    for (k, t) in p.tracks.iter_mut().enumerate() {
        let tb = b + 0x514 + 0x58 * k;
        stream(io, tb, &mut t.note);
        stream(io, tb + 0x20, &mut t.volume);
        stream(io, tb + 0x30, &mut t.effect);
        for (i, v) in t.cell.iter_mut().enumerate() {
            io.u8(tb + 0x50 + i, v);
        }
    }
    io.i32(b + 0x7D4, &mut p.music_channels);
    io.i32(b + 0x7D8, &mut p.sfx_channels);
    io.i32(b + 0x7DC, &mut p.total_channels);
}

fn engine(io: &mut dyn Io, e: &mut Engine) {
    io.u32(0, &mut e.buffer_addr[0]);
    io.u32(4, &mut e.buffer_addr[1]);
    io.u32(8, &mut e.current);
    io.u32(0xC, &mut e.previous);
    io.i32(0x10, &mut e.underrun);
    io.u32(0x20, &mut e.rate);
    io.u32(0x24, &mut e.requested_rate);
    io.u32(0x28, &mut e.note_steps);
    io.u32(0x2C, &mut e.periods);
    io.u32(0x34, &mut e.running);
    io.u32(0x44, &mut e.hold);
    io.i32(0x48, &mut e.mix_rate);
    io.i32(0x50, &mut e.fade);
    player(io, 0x5C, &mut e.player);
    io.u32(0x101C, &mut e.bank);
    for (k, a) in e.sample_addr.iter_mut().enumerate() {
        io.u32(0x1020 + 4 * k, a);
    }
    io.u32(0x1420, &mut e.sfx_table);
    io.u32(0x1424, &mut e.sfx_data);
    io.i32(0x1428, &mut e.master_volume);
    io.i32(0x142C, &mut e.music_volume);
    io.i32(0x1430, &mut e.sfx_volume);
    io.u32(0x1438, &mut e.sfx_count);
    io.u16(0x143C, &mut e.playing);
    io.u32(0x1440, &mut e.loop_music);
    io.i32(0x1444, &mut e.max_sfx);
    io.u32(0x144C, &mut e.buffer_len);
    io.u32(0x1450, &mut e.frame_len);
    io.u32(0x1474, &mut e.mode);
    io.u32(0x1478, &mut e.music_request);
    io.u32(0x1484, &mut e.last_out);
    io.i32(0x1488, &mut e.last_count);
}

/// An engine from RAM: the 0x160C-byte work area, the 16 bytes of globals at 0x03006370 (engine pointer,
/// mixer-call counter at +8, current module state at +12) and both mix buffers. Panics if the second module
/// state (jingles, unused by Carbon) is the current one.
pub fn load(eng: &[u8], globals: &[u8], buffers: [&[u8]; 2]) -> Engine {
    let g = |o: usize| u32::from_le_bytes(globals[o..o + 4].try_into().unwrap());
    assert_eq!(g(12), g(0) + 0x5C, "the jingle module state is active");
    let mut e = Engine {
        buffer_addr: [0; 2],
        buffers: buffers.map(<[u8]>::to_vec),
        current: 0,
        previous: 0,
        underrun: 0,
        rate: 0,
        requested_rate: 0,
        note_steps: 0,
        periods: 0,
        running: 0,
        hold: 0,
        mix_rate: 0,
        fade: 0,
        player: Player::default(),
        bank: 0,
        sample_addr: vec![0; 256],
        sfx_table: 0,
        sfx_data: 0,
        sfx_count: 0,
        master_volume: 0,
        music_volume: 0,
        sfx_volume: 0,
        playing: 0,
        loop_music: 0,
        max_sfx: 0,
        buffer_len: 0,
        frame_len: 0,
        mode: 0,
        music_request: 0,
        last_out: 0,
        last_count: 0,
        mix_calls: g(8),
    };
    engine(&mut Load(eng), &mut e);
    e
}

/// Writes every field the rewrite keeps over `eng` (a copy of the work area) and the mixer-call counter over
/// `globals`; fields it does not keep are left as they are.
pub fn store(e: &Engine, eng: &mut [u8], globals: &mut [u8]) {
    engine(&mut Store(eng), &mut e.clone());
    globals[8..12].copy_from_slice(&e.mix_calls.to_le_bytes());
}
