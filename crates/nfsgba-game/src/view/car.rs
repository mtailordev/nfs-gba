//! The car step's place in the game loop. The adapters between the RAM image and the typed car state live in
//! `nfsgba_sim::ram` (the opponents' AI in the same crate still calls the car step's helpers on the RAM image, so
//! the adapters cannot sit in this crate); this is the call `Game::frame` makes: unlink the entity from its sector
//! list, load the racers, the globals, the wall pieces and the racing line, run `nfsgba_sim::car::handler`
//! (pausing for the traffic spawn), store, link.

use nfsgba_sim::Sim;

/// `FUN_0814bd4c` for the entity at `e`.
pub fn step(sim: &mut Sim, e: u32) -> nfsgba_sim::Result<()> {
    nfsgba_sim::ram::car_handler(sim, e)
}
