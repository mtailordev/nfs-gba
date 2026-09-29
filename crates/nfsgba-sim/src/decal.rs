//! The player's decal pixels onto the heap (`unpack_decal`, `FUN_0813bf58`): renderer-side work on the game's RAM
//! heap that the car step only triggers (`ram::car_handler`, after `init::car_init`). Stays on the RAM image.

use crate::heap;
use crate::mem::Mem;
use crate::state::WORLD;

/// Per car in the save data (`*0x0300539C`, 0x11 bytes): +2 decal, +7 10 upgrade bytes (4 x 2 bits).
pub const CAR_SAVE: u32 = 0x0300_539C;
/// Decal records (0x10 bytes per car x 15 decals): +4 vehicle material.
const DECALS: u32 = 0x087E_F816;
/// Per entity: the decal pixels on the heap.
const DECAL_BUFFERS: u32 = 0x0300_6094;

/// `unpack_decal` (`FUN_0813bf58`) without its last call: the player's decal pixels onto the heap.
/// Rendering, done by `nfsgba-game` (`slots.rs`), not here: the blit onto the car's texture atlas
/// (`draw_decal_on_atlas`, `FUN_0813bd90`) belongs to the renderer.
pub fn unpack_decal(m: &mut Mem, e: u32) {
    let car = m.u8(e + 0x89) as u32;
    let save = m.u32(CAR_SAVE) + car * 0x11;
    let rec = DECALS + (car * 0xF + m.u8(save + 2) as u32) * 0x10;
    let material = m.u32(WORLD + 0x24).wrapping_add((m.i16(rec + 4) as i32 * 0x24) as u32);
    let src = m.u32(WORLD + 4) + m.u32(material + 8);
    let size = m.u16(material + 0xE) as u32 * m.u16(material + 0xC) as u32;
    let slot = DECAL_BUFFERS + m.u16(e) as u32 * 4;
    if m.u32(slot) != 0 {
        heap::free(m, m.u32(slot));
        m.set_u32(slot, 0);
    }
    let buf = heap::alloc(m, size);
    m.set_u32(slot, buf);
    let ring = heap::alloc(m, 0x1011);
    lz77_ring_decode(m, src, buf, ring);
    heap::free(m, ring);
    // `remap_decal_pixels`: index 0x10 becomes transparent, the rest move up to palette 0xB0.
    for k in 0..size {
        let v = m.u8(buf + k);
        m.set_u8(buf + k, if v == 0x10 { 0 } else { v.wrapping_add(0xB0) });
    }
}

/// `lz77_ring_decode` (IWRAM 0x030042F4, ARM): LZ77 through a 0x1000-byte ring prefilled with 0xFF up to
/// 0xFED, writing position 0xFEE on. Reproduces the decoder's exits exactly (it may read on after the last
/// output byte when that byte came from a back-reference).
fn lz77_ring_decode(m: &mut Mem, mut src: u32, mut dst: u32, ring: u32) {
    let header = u32::from_le_bytes([m.u8(src), m.u8(src + 1), m.u8(src + 2), m.u8(src + 3)]);
    src += 4;
    let size = header >> 8;
    let mut left = size as i32;
    for k in 0..=0xFED {
        m.set_u8(ring + k, 0xFF);
    }
    let (mut ip, mut flags, mut count, mut produced) = (0xFEEu32, 7u32, 7, 0u32);
    loop {
        flags <<= 1;
        count += 1;
        if count == 8 {
            count = 0;
            flags = m.u8(src) as u32;
            src += 1;
        }
        if flags & 0x80 == 0 {
            let c = m.u8(src);
            src += 1;
            if produced < size {
                left -= 1;
                m.set_u8(dst, c);
                dst += 1;
                if left <= 0 {
                    return;
                }
            }
            m.set_u8(ring + ip, c);
            produced += 1;
            ip = (ip + 1) & 0xFFF;
            continue;
        }
        let (b1, b2) = (m.u8(src) as u32, m.u8(src + 1) as u32);
        src += 2;
        let len = (b1 >> 4) + 2;
        let disp = b2 | (b1 << 8) & 0xF00;
        let at = |ip: u32| ip.wrapping_sub(disp).wrapping_sub(1) & 0xFFF;
        let mut c = m.u8(ring + at(ip));
        if produced < size {
            left -= 1;
            m.set_u8(dst, c);
            dst += 1;
            if left <= 0 {
                continue;
            }
        }
        let mut n = 0;
        loop {
            produced += 1;
            n += 1;
            m.set_u8(ring + ip, c);
            ip = (ip + 1) & 0xFFF;
            if n > len {
                break;
            }
            c = m.u8(ring + at(ip));
            if produced >= size {
                continue;
            }
            left -= 1;
            m.set_u8(dst, c);
            dst += 1;
            if left <= 0 {
                break;
            }
        }
    }
}
