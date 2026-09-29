//! The game's integer maths, exactly and once: libgcc and the IWRAM division, the ROM's sine, reciprocal and
//! random tables, the two atan2s, the integer square root and the angle difference. Every function names the
//! game function it reproduces. Arithmetic wraps like the ARM does. Tables are read from the ROM (`rom` is the
//! cartridge image, file offsets = GBA address − `0x0800_0000`).

/// Sine table: 0x2000 `i16` entries for a half turn (a full turn is 0x4000), 1.0 = 0x4000.
pub const SIN_TABLE: usize = 0x7C_05F0;
/// Reciprocal table: 32,767 `i32`, entry k = 2^24 / (k + 1).
pub const RECIP_TABLE: usize = 0x7C_45F0;
/// Random table: 256 `u16`.
pub const RAND_TABLE: usize = 0x7C_03F0;

fn i16_at(rom: &[u8], at: usize) -> i16 {
    i16::from_le_bytes([rom[at], rom[at + 1]])
}

fn i32_at(rom: &[u8], at: usize) -> i32 {
    i32::from_le_bytes(rom[at..at + 4].try_into().unwrap())
}

/// `__divsi3` (`FUN_0816a708`): truncates toward zero; x / 0 = 0.
pub fn div(a: i32, b: i32) -> i32 {
    if b == 0 { 0 } else { a.wrapping_div(b) }
}

/// `__udivsi3` (`FUN_0816a93c`): x / 0 = 0.
pub fn udiv(a: u32, b: u32) -> u32 {
    a.checked_div(b).unwrap_or(0)
}

/// `__umodsi3` (`FUN_0816a9b4`): x % 0 = 0.
pub fn umod(a: u32, b: u32) -> u32 {
    a.checked_rem(b).unwrap_or(0)
}

/// The IWRAM division at `0x03000220` (ARM; ROM copy `0x08165134`, called through `*0x03006494`): returns
/// `(a / b, a − |a / b| · b)`; the game stores the remainder at `0x03006480`. Shift-and-subtract on the
/// magnitudes, the quotient signed by `a ^ b`. Reproduces the game's slips: the remainder uses the unsigned
/// quotient (a negative `a` gives an off remainder), and for a negative `b` it divides by `|a|`, not `|b|`.
pub fn iwram_divmod(a: i32, b: i32) -> (i32, i32) {
    let sign = a ^ b;
    let n = a.unsigned_abs();
    let d = if b < 0 { n.wrapping_neg() } else { b as u32 };
    let (mut n, mut d, mut bit) = (n, d, 1u32);
    while d < 0x8000_0000 && d < n {
        assert!(d != 0, "the game's IWRAM division never ends for a zero divisor");
        d <<= 1;
        bit <<= 1;
    }
    let mut q = 0u32;
    loop {
        if n >= d {
            n -= d;
            q = q.wrapping_add(bit);
        }
        bit >>= 1;
        if bit == 0 {
            break;
        }
        d >>= 1;
    }
    let rem = a.wrapping_sub((q as i32).wrapping_mul(b));
    (if sign < 0 { (q as i32).wrapping_neg() } else { q as i32 }, rem)
}

/// Entry `k` of the reciprocal table (2^24 / (k + 1)). The game indexes it without bounds checks.
pub fn recip(rom: &[u8], k: i32) -> i32 {
    i32_at(rom, (RECIP_TABLE as i64 + 4 * k as i64) as usize)
}

/// `recip_q24` (`FUN_08149178`, also inlined in `FUN_08147a6c` and `FUN_08147ec4`): about 2^24 / x through the
/// reciprocal table, keeping the table index at most 0x2000.
pub fn recip_q24(rom: &[u8], x: i32) -> i32 {
    let asr = |v: i32, n: u32| if n >= 32 { v >> 31 } else { v >> n };
    let mut shift = 0u32;
    if x < 0 {
        let mut a = x.wrapping_neg();
        if x != -0x2000 && a > 0x1FFF {
            loop {
                a >>= 1;
                shift += 1;
                if a <= 0x2000 {
                    break;
                }
            }
        }
        -asr(recip(rom, a >> 1), shift + 1)
    } else {
        let mut a = x;
        loop {
            let more = a > 0x2000;
            a >>= 1;
            if !more {
                break;
            }
            shift += 1;
        }
        asr(recip(rom, a), shift + 1)
    }
}

/// `recip_div_16` (IWRAM `0x03004D20`): `a · recip[b] >> 8` in 64 bits (`smull`), about `(a << 16) / (b + 1)`.
pub fn recip_div_16(rom: &[u8], a: i32, b: i32) -> i32 {
    ((a as i64).wrapping_mul(recip(rom, b) as i64) >> 8) as i32
}

/// `sin_q14` (`FUN_0815f948`): sine of a 14-bit angle (0x4000 per turn), result in ±0x4000.
pub fn sin_q14(rom: &[u8], angle: i32) -> i32 {
    let a = angle & 0x3FFF;
    let v = i16_at(rom, SIN_TABLE + 2 * (a & 0x1FFF) as usize) as i32;
    if a > 0x1FFF { -v } else { v }
}

/// `cos_q14` (`FUN_0815f988`): `sin_q14(angle + 0x1000)`.
pub fn cos_q14(rom: &[u8], angle: i32) -> i32 {
    sin_q14(rom, angle.wrapping_add(0x1000))
}

/// The polynomial both atan2s share: `r` is the scaled ratio, `base` the octant's offset.
fn atan_poly(x: i32, r: i32, base: i32) -> i32 {
    let cube = (r.wrapping_mul(r.wrapping_mul(r) >> 15) >> 15).wrapping_mul(0x1920) >> 15;
    let a = base
        .wrapping_add(cube)
        .wrapping_sub(r.wrapping_mul(0x7DA9) >> 15)
        .wrapping_mul(0x1460)
        >> 16;
    if x < 0 { -a } else { a }
}

/// `atan2_q14` (`FUN_0815f9cc`): the angle (0x4000 per turn) of the direction (`x`, `z`), 0 along +z, positive
/// towards +x; the ratio by `__divsi3`.
pub fn atan2_q14(x: i32, z: i32) -> i32 {
    let ax = x.wrapping_abs();
    let nonzero = |d: i32| if d == 0 { 1 } else { d };
    let (r, base) = if z < 0 {
        (
            div(z.wrapping_add(ax).wrapping_mul(0x8001), nonzero(ax.wrapping_sub(z))),
            0x12D9A,
        )
    } else {
        (
            div(z.wrapping_sub(ax).wrapping_mul(0x8001), nonzero(z.wrapping_add(ax))),
            0x6488,
        )
    };
    atan_poly(x, r, base)
}

/// `atan2_fast` (IWRAM `0x03004470`, ARM): the same polynomial with the ratio through `recip_div_16`, divisor
/// clamped to 0x7FFE. Not exact at the axes: (344, 0) gives 0xFFD, the reference race's camera yaw.
pub fn atan2_fast(rom: &[u8], x: i32, z: i32) -> i32 {
    let ax = (x ^ (x >> 31)).wrapping_sub(x >> 31);
    let clamp = |d: i32| if d == 0 { 1 } else { d.min(0x7FFE) };
    let (r, base) = if z >= 0 {
        (
            recip_div_16(rom, z.wrapping_sub(ax), clamp(z.wrapping_add(ax))) >> 1,
            0x6488,
        )
    } else {
        (
            recip_div_16(rom, z.wrapping_add(ax), clamp(ax.wrapping_sub(z))) >> 1,
            0x12D9A,
        )
    };
    atan_poly(x, r, base)
}

/// `isqrt` (`FUN_0815fa54`): floor square root of an unsigned word in 16 two-bit steps; 0 gives 1.
pub fn isqrt(v: u32) -> i32 {
    let (mut rem, mut root, mut v) = (0u32, 0u32, v);
    for _ in 0..16 {
        rem = rem.wrapping_mul(4).wrapping_add(v >> 30);
        v <<= 2;
        let trial = root << 2 | 1;
        root <<= 1;
        if trial <= rem {
            rem -= trial;
            root += 1;
        }
    }
    root.max(1) as i32
}

/// `angle_diff` (`FUN_0815fc38`): `b − a` taken the short way round, in (−0x2000, 0x2000], 0x4000 per turn.
pub fn angle_diff(a: i32, b: i32) -> i32 {
    if (a <= b && b < a + 0x2000) || (b <= a && a < b + 0x2000) {
        b - a
    } else if a + 0x2000 < b {
        b - 0x4000 - a
    } else if a - 0x2000 < b {
        0
    } else {
        b - (a - 0x4000)
    }
}

/// `rand_table` (`FUN_0815fcfc`): `index = (index + 1) & 0xFF`, then `u16 0x7C03F0[index]`. The game keeps the
/// index at `0x030064C8`; `setup_race_cars` reseeds it with `*0x03000044 & 0xFF` (`FUN_0815fd1c`).
pub fn rand_table(rom: &[u8], index: &mut u32) -> u32 {
    *index = (*index + 1) & 0xFF;
    i16_at(rom, RAND_TABLE + 2 * *index as usize) as u16 as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iwram_division_keeps_its_quirks() {
        assert_eq!(iwram_divmod(100, 7), (14, 2));
        // The remainder uses the unsigned quotient: -100 - 14 * 7.
        assert_eq!(iwram_divmod(-100, 7), (-14, -198));
        assert_eq!(div(7, 0), 0);
        assert_eq!((udiv(7, 0), umod(7, 0)), (0, 0));
    }

    #[test]
    fn angles_and_roots() {
        assert_eq!(angle_diff(0x3F00, 0x0100), 0x200);
        assert_eq!(angle_diff(0x0100, 0x3F00), -0x200);
        assert_eq!((isqrt(0), isqrt(1), isqrt(99), isqrt(100)), (1, 1, 9, 10));
    }
}
