//! The traffic cars' typed state (`traffic_ai.rs`, `traffic.rs`). The opponents' AI keeps its state in the
//! [`Car`](super::Car) (physics struct) and [`CarGlobals`](super::CarGlobals); a traffic car has only its entity and
//! this 0x28-byte block (heap, entity `+0x8C`).

use crate::layout;

layout! {
    /// A traffic car's block: the point it drives to, the turn in progress, flags and lane.
    pub struct TrafficBlock: 0x28 {
        /// Target point (city units, x and z).
        0x00 target: [i32; 2],
        /// The turn: the direction it starts from, the change, the progress, the steps left.
        0x08 turn_from: [i32; 2],
        0x10 turn_by: [i32; 2],
        0x18 turn_t: i32,
        0x1C turn_steps: i32,
        /// Bit 0: the pitch has been set.
        0x20 flags: u32,
        /// 0 or 2: the lane offset index.
        0x24 lane: u32,
    }
}
