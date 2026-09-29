//! The HUD's frame (`nfsgba_formats::{hud, ui}` run on it; `World::hud_frame` builds it) and the effect sprites'
//! place in the shadow OAM.

use nfsgba_formats::{hud, ui};

use crate::world::World;

/// Objects of the race HUD's sprite screen.
pub const OBJECTS: u32 = 55;

/// What `hud_update` and `sprite_screen_update` read and write.
pub struct HudFrame {
    pub g: hud::Globals,
    pub racers: [hud::Racer; 4],
    pub objects: Vec<ui::Object>,
    pub messages: hud::Messages,
    pub oam: ui::Oam,
}

/// `FUN_08161f38` on the effect-sprite pool: draws its sprites into the shadow OAM.
pub fn draw_effect_sprites(rom: &[u8], w: &mut World) {
    let (first, tile_base) = (w.pool_first, w.hud.tile_base);
    crate::oam::draw_effect_sprites(rom, &mut w.pool, first, tile_base, &mut w.hud.oam);
}

/// The shadow OAM as the game copies it to OAM each frame (`FUN_0816102c`).
pub fn shadow_oam_bytes(w: &World) -> Vec<u8> {
    w.hud.oam.iter().flatten().flat_map(|v| v.to_le_bytes()).collect()
}
