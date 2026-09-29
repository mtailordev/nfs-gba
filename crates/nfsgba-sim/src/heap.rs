//! The game's EWRAM heap (`heap_alloc` `FUN_08160af8`, `heap_free` `FUN_08160de4`).
//!
//! `*0x030064CC` points at 256 block nodes of 8 bytes: u16 own index, next node, start and end (in words from
//! `*0x030064D0`, the data area). Node 0 anchors a list sorted by address; a node whose end is 0 is free.
//! Allocation takes the first gap after a block that fits, leaving one spare word after the previous block.

use crate::mem::Mem;

const NODES: u32 = 0x0300_64CC;
const DATA: u32 = 0x0300_64D0;

fn node(m: &Mem, i: u32) -> u32 {
    m.u32(NODES) + i * 8
}

/// `FUN_08160af8`: `size` bytes (at least 2 words); 0 when full.
pub fn alloc(m: &mut Mem, size: u32) -> u32 {
    let words = (size.wrapping_add(3) >> 2).max(2);
    let Some(i) = (0..0x100).find(|&i| m.u16(node(m, i) + 6) == 0) else {
        return 0;
    };
    let new = node(m, i);
    let mut cur = node(m, 0);
    loop {
        let next = node(m, m.u16(cur + 2) as u32);
        let gap = (m.u16(next + 4) as u32).wrapping_sub(m.u16(cur + 6) as u32);
        if words <= gap {
            let c = node(m, m.u16(cur) as u32);
            m.set_u16(new, i as u16);
            m.set_u16(new + 2, m.u16(c + 2));
            let start = m.u16(c + 6).wrapping_add(1);
            m.set_u16(new + 4, start);
            m.set_u16(new + 6, start.wrapping_add(words as u16));
            m.set_u16(c + 2, i as u16);
            return m.u32(DATA) + m.u16(new + 4) as u32 * 4;
        }
        cur = next;
        if m.u16(cur + 2) == 0xFFFF {
            return 0;
        }
    }
}

/// `FUN_08160b94`: allocate and clear. The game clears `size / 4` words, then `size % 4` bytes at the start
/// again (not the tail).
pub fn alloc_zeroed(m: &mut Mem, size: u32) -> u32 {
    let p = alloc(m, size);
    if p != 0 {
        for k in 0..size >> 2 {
            m.set_u32(p + 4 * k, 0);
        }
        for k in 0..size & 3 {
            m.set_u8(p + k, 0);
        }
    }
    p
}

/// `FUN_08160de4`: release the block starting at `addr` (searches at most 0x400 nodes).
pub fn free(m: &mut Mem, addr: u32) {
    let start = addr.wrapping_sub(m.u32(DATA)) >> 2;
    let mut prev = node(m, 0);
    let mut cur = node(m, m.u16(prev + 2) as u32);
    if start == 0 {
        return;
    }
    for _ in 0..0x400 {
        if m.u16(cur + 4) as u32 == start {
            m.set_u16(prev + 2, m.u16(cur + 2));
            let own = node(m, m.u16(cur) as u32);
            m.set_u16(own + 6, 0);
            return;
        }
        prev = cur;
        cur = node(m, m.u16(cur + 2) as u32);
    }
}
