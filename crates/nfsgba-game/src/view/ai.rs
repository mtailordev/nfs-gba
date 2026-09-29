//! The opponents' and the traffic's place in the game loop. The adapters between the RAM image and the typed car
//! world live in `nfsgba_sim::ram` (the sim's tests use them too); these are the calls `Game::update_entities`
//! makes for entity handlers 0x29 and 0x36: load every entity with its driver, run `nfsgba_sim::ai::handler` or
//! `traffic_ai::handler`, store; the heap blocks and sector lists are replayed by the adapter.

use nfsgba_sim::{Sim, ai::Effects};

/// `FUN_0814a2a0` for the entity at `e`: the opponents and the wingman. The effects call it reports is the
/// sprites of the car's lights and exhaust (`slots::opponent_effects`).
pub fn opponent(sim: &mut Sim, e: u32) -> nfsgba_sim::Result<Option<Effects>> {
    nfsgba_sim::ram::ai_handler(sim, e)
}

/// `FUN_081443fc` for the entity at `e`: a traffic car.
pub fn traffic(sim: &mut Sim, e: u32) -> nfsgba_sim::Result<()> {
    nfsgba_sim::ram::traffic_handler(sim, e)
}
