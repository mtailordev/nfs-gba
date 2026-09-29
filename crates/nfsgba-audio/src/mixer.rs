//! The mixing loop: ARM code the game LZ77-unpacks from 0x0815CFD4 to IWRAM 0x03005A00 (mode 1) and calls once
//! per segment with a list of voices (`FUN_08152660`). Reproduced to the bit, quirks included:
//!
//! - Output bytes are `sum(sample * volume) >> 8`, wrapped to 8 bits (no clipping).
//! - Whole words are mixed four samples at a time in two packed accumulators (samples 0 and 2 share one
//!   register, 1 and 3 the other), so a negative low half borrows from the high half's byte.
//! - The position is a 32-bit fraction plus an integer step; add-with-carry chains run through the flags,
//!   so the first voice of a 16-sample block starts with carry set (one extra 1/2^32 step).
//! - Up to three leading bytes before a word boundary, and the trailing `n & 3` bytes, are mixed one at a time;
//!   the trailing loop also moves every voice one extra byte per sample (`ldrsb [r3], #1`).
//! - A voice with volume 0 is skipped and does not move; volume 0xFF ends the list.

use crate::Rom;

/// One voice record as `FUN_08152660` builds it (five words).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Voice {
    /// Address of the next sample.
    pub ptr: u32,
    pub vol: u32,
    /// Position fraction (2^-32 units) and the step, split into fraction and whole bytes.
    pub frac: u32,
    pub frac_step: u32,
    pub step: u32,
}

fn adc(a: u32, b: u32, carry: u32) -> (u32, u32) {
    let s = a as u64 + b as u64 + carry as u64;
    (s as u32, (s >> 32) as u32)
}

/// Mixes `n` samples into `out[at..at + n]` (`out` starts word aligned, as both mix buffers do).
pub fn mix(rom: Rom, voices: &mut [Voice], out: &mut [u8], at: usize, n: usize) {
    let (mut o, mut n) = (at, n);
    if o & 3 != 0 {
        let head = (4 - (o & 3)).min(n);
        n -= head;
        for _ in 0..head {
            out[o] = single(rom, voices, 0);
            o += 1;
        }
    }
    if n == 0 {
        return;
    }
    let tail = n & 3;
    n -= tail;
    for len in std::iter::repeat_n(16, n / 16).chain((n % 16 != 0).then_some(n % 16)) {
        block(rom, voices, &mut out[o..o + len]);
        o += len;
    }
    for _ in 0..tail {
        out[o] = single(rom, voices, 1);
        o += 1;
    }
}

/// One output byte (`0x03005A30` loop; `extra` = 1 in the trailing loop at `0x03005CD8`).
fn single(rom: Rom, voices: &mut [Voice], extra: u32) -> u8 {
    let mut acc = 0u32;
    for v in voices.iter_mut() {
        match v.vol {
            0 => continue,
            0xFF => break,
            _ => {}
        }
        let s = rom.i8(v.ptr) as i32;
        let (frac, c) = adc(v.frac, v.frac_step, (v.vol > 0xFF) as u32);
        let (ptr, _) = adc(v.ptr.wrapping_add(extra), v.step, c);
        (v.frac, v.ptr) = (frac, ptr);
        if ptr != 0 {
            acc = acc.wrapping_add(s.wrapping_mul(v.vol as i32) as u32);
        }
    }
    (acc >> 8) as u8
}

/// A block of 4, 8, 12 or 16 samples (`0x03005AE4`; shorter blocks patch a branch into the unrolled loop).
fn block(rom: Rom, voices: &mut [Voice], out: &mut [u8]) {
    let mut acc = [0u32; 16];
    for (i, v) in voices.iter_mut().enumerate() {
        // The first record is only checked for 0 (and starts with carry set by that compare).
        let mut carry = match v.vol {
            0 => continue,
            _ if i == 0 => 1,
            0xFF => break,
            vol => (vol > 0xFF) as u32,
        };
        for (k, a) in acc[..out.len()].iter_mut().enumerate() {
            let prod = (rom.i8(v.ptr) as i32).wrapping_mul(v.vol as i32) as u32;
            // Samples 0 and 1 of each group are `mlane` (skipped at address 0), 2 and 3 plain `mul` + `add`.
            if k % 4 >= 2 || k == 0 || v.ptr != 0 {
                *a = a.wrapping_add(prod);
            }
            let (frac, c) = adc(v.frac, v.frac_step, carry);
            (v.ptr, carry) = adc(v.ptr, v.step, c);
            v.frac = frac;
        }
    }
    for (g, word) in out.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let a = acc[4 * g].wrapping_add(acc[4 * g + 2] << 16);
        let b = acc[4 * g + 1].wrapping_add(acc[4 * g + 3] << 16);
        *word = [(a >> 8) as u8, (b >> 8) as u8, (a >> 24) as u8, (b >> 24) as u8];
    }
}
