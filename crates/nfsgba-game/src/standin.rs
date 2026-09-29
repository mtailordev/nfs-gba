//! Stand-ins for game code that is not ported yet, so that a race can be driven live ([`crate::Checkpoint`]).
//! NOT 1:1: each names the code it stands for; replays use the reference build's state instead
//! (`tests/replay.rs`), and `tests/replay.rs` also measures how often these agree with it.

use nfsgba_sim::Mem;

use crate::view::{WORLD, entity_count};

/// A racer's matrix slot as `build_entity_matrix` (`0x0814da68`) builds it for an entity with flag bit 5: the
/// physics rotation (driver `+0x128`, 2.12) times the camera rotation (2.14, row-major), and the camera-space
/// position. `None` beyond depth 0xDAC without flag bit 1 (the game then leaves the slot as it was).
pub fn slot_matrix(m: &Mem, e: u32) -> Option<[i32; 12]> {
    let cam: [i32; 12] = std::array::from_fn(|k| m.i32(m.u32(WORLD + 0x54) + 4 * k as u32));
    let x = (m.i32(e + 0x0C) >> 8).wrapping_add(cam[9]);
    let z = (m.i32(e + 0x14) >> 8).wrapping_add(cam[11]);
    let depth = x.wrapping_mul(cam[2]).wrapping_add(z.wrapping_mul(cam[8])) >> 14;
    if m.u16(e + 0x0A) & 2 == 0 && depth > 0xDAC {
        return None;
    }
    let d = m.u32(e + 0x8C);
    let phys: [i32; 9] = std::array::from_fn(|k| m.i32(d + 0x128 + 4 * k as u32) << 2);
    let mut out = [0; 12];
    for r in 0..3 {
        for c in 0..3 {
            out[3 * r + c] = (0..3).fold(0i32, |s, k| {
                s.wrapping_add(phys[3 * r + k].wrapping_mul(cam[3 * k + c]))
            }) >> 14;
        }
    }
    out[9] = x.wrapping_mul(cam[0]).wrapping_add(z.wrapping_mul(cam[6])) >> 14;
    out[10] = (m.i32(e + 0x10) >> 8).wrapping_add(cam[10]);
    out[11] = depth;
    Some(out)
}

/// NOT 1:1 (R25), standing in for `build_player_matrices` (`0x0814bc30`) and `assign_entity_slot`
/// (`0x0814eba0`) with `FUN_08144c98`: the player takes the next two slots (body and spoiler share one matrix);
/// every other entity drawn last frame (`+0x0A` bit 2) with a model takes one, entities without a driver struct
/// none (their matrix comes from `+0x30`/`+0x32` rotations, not stood in). The player's real matrix comes from its
/// wheel contacts (`FUN_0814e050`), which also spawns sparks and effect sprites; none are made here.
pub fn slots(m: &mut Mem) {
    let (ents, table) = (m.u32(WORLD + 0x3C), m.u32(WORLD + 0xFC));
    let player = m.u32(0x0300_0060);
    let next = |m: &mut Mem| {
        let s = m.u32(0x0300_5394);
        m.set_u32(0x0300_5394, s + 1);
        if s + 1 > 0x3F { 0xFF } else { s as u8 }
    };
    let e = ents + 0xA4 * player;
    let slot = next(m);
    m.set_u8(e + 0x88, slot);
    if m.i16(e + 0x64) != 0 {
        next(m);
    }
    if slot != 0xFF
        && let Some(mat) = slot_matrix(m, e)
    {
        for k in [0, 1] {
            for (j, v) in mat.iter().enumerate() {
                m.set_i32(table + 0x30 * (slot as u32 + k) + 4 * j as u32, *v);
            }
        }
    }
    for i in (0..entity_count(m)).filter(|&i| i != player) {
        let e = ents + 0xA4 * i;
        if m.u16(e + 8) & 3 != 3 {
            continue;
        }
        let drawn = m.i16(e + 0x36) != 0 && m.u16(e + 0x0A) & 4 != 0 && m.u32(e + 0x8C) != 0;
        let slot = if drawn { next(m) } else { 0xFF };
        m.set_u8(e + 0x88, slot);
        if slot != 0xFF
            && let Some(mat) = slot_matrix(m, e)
        {
            for (j, v) in mat.iter().enumerate() {
                m.set_i32(table + 0x30 * slot as u32 + 4 * j as u32, *v);
            }
        }
        m.set_u16(e + 0x0A, m.u16(e + 0x0A) & 0xFFFB);
    }
}
