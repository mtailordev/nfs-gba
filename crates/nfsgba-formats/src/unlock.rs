//! Small garage unlock-id helpers (`docs/engine/symbols.csv`), ported from the game and checked against the
//! function oracle (`tools/oracle/unlock.py`, `tests/unlock.rs`). Typed arguments, no RAM image.

/// The unlock table: pairs of `u16` (id, price), sorted by id.
const TABLE: usize = 0x7E_4DE4;
/// One byte per car index: how far ids `0xA1..=0xA3` shift.
const SLOT_BYTES: usize = 0x7F_0626;

/// `unlock_table_index` (`0x0812D7D4`): the row of `id` in the unlock table, or -1.
/// The game scans until a row's id exceeds `id` (ids at or above `0x10000` would run off the table).
pub fn table_index(rom: &[u8], id: u32) -> i32 {
    let id = id as i32;
    for i in 0.. {
        let row = u16::from_le_bytes([rom[TABLE + 4 * i], rom[TABLE + 4 * i + 1]]);
        if i32::from(row) == id {
            return i as i32;
        }
        if i32::from(row) > id {
            return -1;
        }
    }
    unreachable!()
}

/// `unlock_id_adjust` (`0x0812D7F4`): ids `0xA1..=0xA3` are shifted by the sum of the first `car` slot bytes
/// minus `car`; every other id is returned as is.
pub fn id_adjust(rom: &[u8], id: u32, car: u32) -> u32 {
    if id.wrapping_sub(0xA1) >= 3 {
        return id;
    }
    let sum: u32 = (0..car as usize).map(|i| u32::from(rom[SLOT_BYTES + i])).sum();
    id.wrapping_add(sum).wrapping_sub(car)
}

/// `unlock_group` (`0x0812D730`): 1..=10 for the ten group-leader ids, else 0.
pub fn group(id: u32) -> u32 {
    match id {
        0xC => 1,
        0x19 => 2,
        0x26 => 3,
        0x33 => 4,
        0x40 => 5,
        0x4D => 6,
        0x5A => 7,
        0x67 => 8,
        0x70 => 9,
        0x78 => 10,
        _ => 0,
    }
}
