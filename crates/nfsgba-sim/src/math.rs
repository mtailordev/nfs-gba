//! The game's integer maths, exactly: libgcc division, the ROM's sine and reciprocal tables, its atan2, square
//! root, and the 20.12 fixed-point vector, matrix and quaternion helpers. Every function names the game function
//! it reproduces. Arithmetic wraps like the ARM does.

use crate::mem::Mem;

/// Sine table: 0x2000 `i16` entries for a half turn (a full turn is 0x4000), 1.0 = 0x4000.
const SIN_TABLE: u32 = 0x087C_05F0;
/// Reciprocal table: entry k = 2^24 / (k + 1).
const RECIP_TABLE: u32 = 0x087C_45F0;

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

/// `__muldi3` (`FUN_0816a8b4`) on two sign-extended words: the low 64 bits of the product.
pub fn mul64(a: i32, b: i32) -> i64 {
    (a as i64).wrapping_mul(b as i64)
}

/// The 32 bits `(v >> shift)` of a 64-bit value, as the game assembles them from two words.
pub fn shr64(v: i64, shift: u32) -> i32 {
    (v >> shift) as i32
}

/// ARM shift by a register amount: `asr` saturates at 32.
pub fn asr(v: i32, n: u32) -> i32 {
    if n >= 32 { v >> 31 } else { v >> n }
}

/// `a * b >> 12`, 32-bit wrapping.
pub fn mul12(a: i32, b: i32) -> i32 {
    a.wrapping_mul(b) >> 12
}

/// `FUN_0815f948`: sine of a 14-bit angle, 1.0 = 0x4000.
pub fn sin(mem: &Mem, angle: i32) -> i32 {
    let a = (angle & 0x3FFF) as u32;
    if a > 0x1FFF {
        -(mem.i16(SIN_TABLE + (a - 0x2000) * 2) as i32)
    } else {
        mem.i16(SIN_TABLE + a * 2) as i32
    }
}

/// `FUN_0815f988`: cosine, `sin(angle + 0x1000)`.
pub fn cos(mem: &Mem, angle: i32) -> i32 {
    sin(mem, angle.wrapping_add(0x1000))
}

/// `FUN_0815f9cc`: the angle (0x4000 = full turn) of the direction (`x`, `z`), 0 along +z, positive towards +x.
pub fn atan2(x: i32, z: i32) -> i32 {
    let ax = x.wrapping_abs();
    let (r, base) = if z < 0 {
        let d = match ax.wrapping_sub(z) {
            0 => 1,
            d => d,
        };
        (div(z.wrapping_add(ax).wrapping_mul(0x8001), d), 0x12D9A)
    } else {
        let d = match z.wrapping_add(ax) {
            0 => 1,
            d => d,
        };
        (div(z.wrapping_sub(ax).wrapping_mul(0x8001), d), 0x6488)
    };
    let cube = (r.wrapping_mul(r.wrapping_mul(r) >> 15) >> 15).wrapping_mul(0x1920) >> 15;
    let a = (base + cube - (r.wrapping_mul(0x7DA9) >> 15)).wrapping_mul(0x1460) >> 16;
    if x < 0 { -a } else { a }
}

/// `FUN_0815fa54`: integer square root of an unsigned word, at least 1.
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

/// `FUN_08149178` (also inlined in `FUN_08147a6c` and `FUN_08147ec4`): about 2^24 / x through the reciprocal
/// table, keeping the table index at most 0x2000.
pub fn recip(mem: &Mem, x: i32) -> i32 {
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
        -asr(mem.i32(RECIP_TABLE + (a >> 1) as u32 * 4), shift + 1)
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
        asr(mem.i32(RECIP_TABLE + a as u32 * 4), shift + 1)
    }
}

/// Reciprocal table entry (`FUN_08147b18`, `FUN_0814ca84`).
pub fn recip_entry(mem: &Mem, k: i32) -> i32 {
    mem.i32(RECIP_TABLE.wrapping_add((k as u32).wrapping_mul(4)))
}

pub type V3 = [i32; 3];

/// `FUN_08149150`: 20.12 dot product.
pub fn dot(a: V3, b: V3) -> i32 {
    mul12(a[0], b[0])
        .wrapping_add(mul12(a[1], b[1]))
        .wrapping_add(mul12(a[2], b[2]))
}

/// `FUN_08149058`
pub fn add(a: V3, b: V3) -> V3 {
    [0, 1, 2].map(|k| a[k].wrapping_add(b[k]))
}

/// `FUN_08149038`
pub fn sub(a: V3, b: V3) -> V3 {
    [0, 1, 2].map(|k| a[k].wrapping_sub(b[k]))
}

/// `FUN_081490e0`: `s * v >> 12` per component.
pub fn scale(v: V3, s: i32) -> V3 {
    v.map(|c| mul12(s, c))
}

/// `FUN_0815fbd0`: cross product without scaling.
pub fn cross(a: V3, b: V3) -> V3 {
    [
        b[2].wrapping_mul(a[1]).wrapping_sub(a[2].wrapping_mul(b[1])),
        a[2].wrapping_mul(b[0]).wrapping_sub(a[0].wrapping_mul(b[2])),
        a[0].wrapping_mul(b[1]).wrapping_sub(a[1].wrapping_mul(b[0])),
    ]
}

/// `FUN_08149078`: `v` times the 3x3 matrix `m` (row-major, 20.12): `out[j] = sum_i v[i] * m[3i + j]`.
pub fn mat_mul(v: V3, m: &[i32; 9]) -> V3 {
    [0, 1, 2].map(|j| {
        mul12(m[j], v[0])
            .wrapping_add(mul12(v[1], m[3 + j]))
            .wrapping_add(mul12(v[2], m[6 + j]))
    })
}

/// `FUN_08147a6c`: normalise to length 0x1000 (through `isqrt` and `recip`); returns the length.
pub fn normalize(mem: &Mem, v: &mut V3) -> i32 {
    let sq = v[0]
        .wrapping_mul(v[0])
        .wrapping_add(v[1].wrapping_mul(v[1]))
        .wrapping_add(v[2].wrapping_mul(v[2]));
    let len = isqrt(sq as u32);
    if len != 0 {
        let r = recip(mem, len);
        *v = [mul12(v[0], r), mul12(v[1], r), mul12(r, v[2])];
    }
    len
}

/// `FUN_0815fdd0`: normalise to length 0x4000 by division.
pub fn normalize14(v: &mut V3) {
    let sq = v[0]
        .wrapping_mul(v[0])
        .wrapping_add(v[1].wrapping_mul(v[1]))
        .wrapping_add(v[2].wrapping_mul(v[2]));
    let len = isqrt(sq as u32);
    *v = v.map(|c| div(c << 14, len));
}

/// `FUN_0815fc38`: signed difference `b - a` of two angles in (-0x2000, 0x2000].
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

/// `FUN_08147618`: quaternion product `a * b` (x, y, z, w; 20.12).
pub fn quat_mul(a: [i32; 4], b: [i32; 4]) -> [i32; 4] {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        mul12(bx, aw) + mul12(bw, ax) + mul12(bz, ay) - mul12(by, az),
        mul12(by, aw) + mul12(bw, ay) + mul12(bx, az) - mul12(bz, ax),
        mul12(bz, aw) + mul12(bw, az) + mul12(by, ax) - mul12(bx, ay),
        mul12(bw, aw) - mul12(bx, ax) - mul12(by, ay) - mul12(az, bz),
    ]
}

/// `FUN_081476d8`: rotation matrix (row-major, 20.12) of a unit quaternion.
pub fn quat_matrix(q: [i32; 4]) -> [i32; 9] {
    let [x, y, z, w] = q;
    let (y2, z2) = (y * 2, z * 2);
    let xx = x * x * 2 >> 12;
    let xy = x * y2 >> 12;
    let xz = x * z2 >> 12;
    let yy = y * y2 >> 12;
    let yz = y * z2 >> 12;
    let zz = z2 * z >> 12;
    let wx = w * x * 2 >> 12;
    let wy = y2 * w >> 12;
    let wz = z2 * w >> 12;
    [
        0x1000 - (yy + zz),
        xy - wz,
        xz + wy,
        xy + wz,
        0x1000 - (xx + zz),
        yz - wx,
        xz - wy,
        yz + wx,
        0x1000 - (xx + yy),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_helpers_match_the_game() {
        assert_eq!((div(7, 0), div(-7, 2), div(i32::MIN, -1)), (0, -3, i32::MIN));
        assert_eq!((isqrt(0), isqrt(15), isqrt(16), isqrt(u32::MAX)), (1, 3, 4, 0xFFFF));
        assert_eq!(
            (
                angle_diff(0x3F00, 0x100),
                angle_diff(0x100, 0x3F00),
                angle_diff(0, 0x2000)
            ),
            (0x200, -0x200, 0)
        );
        // The polynomial is one unit off straight ahead.
        assert_eq!(
            (atan2(0, 0x1000), atan2(0x1000, 0), atan2(-0x1000, 0)),
            (-1, 0x1000, -0x1000)
        );
        // 2^30 * 8 overflows 32 bits; the 64-bit product keeps it.
        assert_eq!(shr64(mul64(0x4000_0000, 8), 15), 0x4_0000);
    }
}
