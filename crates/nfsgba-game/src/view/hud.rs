//! The HUD's adapters: `nfsgba_formats::{hud, ui}` run on typed state; this loads it from the game's RAM and
//! stores it back (`state::hud`). Also the effect sprites' place in the shadow OAM.

use nfsgba_formats::{hud, ui};
use nfsgba_sim::{
    Mem,
    layout::Field,
    state::{HudMessages, HudVars, Race, ShadowOam, SpritePool, WORLD, WorldHeader},
};

/// Objects of the race HUD's sprite screen.
pub const OBJECTS: u32 = 55;

/// What `hud_update` and `sprite_screen_update` read and write.
pub struct HudFrame {
    /// The HUD variables, for the ones the frame keeps besides `g` (the message flag, the screen, the tile base,
    /// the division remainder).
    pub vars: HudVars,
    pub g: hud::Globals,
    pub racers: [hud::Racer; 4],
    pub objects: Vec<ui::Object>,
    pub messages: hud::Messages,
    pub oam: ui::Oam,
}

pub fn load(m: &Mem) -> HudFrame {
    let (w, race) = (WorldHeader::load(m, WORLD), Race::load(m, 0));
    let vars = HudVars::load(m, 0);
    let g = hud::Globals {
        hud: vars.hud,
        mode: vars.mode,
        language: vars.language,
        units: vars.units,
        frames: vars.frames,
        split: vars.split,
        opponents: vars.opponents,
        ai_cars: vars.ai_cars,
        laps: vars.laps,
        wingman: vars.wingman,
        portrait: vars.portrait,
        portrait_blink: vars.portrait_blink,
        bar: vars.bar,
        bar_max: vars.bar_max,
        arrow: vars.arrow,
        route: vars.route,
        needle_scale: race.profile.read(m).needle_scale,
        player: vars.player as usize,
        race_state: vars.race_state,
        race_state_changed: vars.race_state_changed,
    };
    let racers = std::array::from_fn(|i| {
        let e = w.entities.at(i as u32).read(m);
        hud::Racer {
            x: e.pos[0],
            z: e.pos[2],
            heading: e.heading,
            driver: (!e.driver.is_null()).then(|| {
                let c = e.driver.read(m);
                hud::Driver {
                    revs: c.revs,
                    gear: c.gear,
                    speed: c.speed,
                    position: c.position,
                    laps_left: c.laps_left,
                    rev_scale: c.max_rpm,
                    dial: c.nitro_tank,
                    flags: c.route_flags,
                    hunter_life: c.hunter_life,
                }
            }),
        }
    });
    HudFrame {
        objects: vars.objects.read_n(m, OBJECTS),
        messages: HudMessages::load(m, 0).slots,
        oam: ShadowOam::load(m, 0).entries,
        vars,
        g,
        racers,
    }
}

/// Stores what the HUD changes: the race state past the time limit, the portrait and bar animation (from `g`),
/// the objects, the messages, the shadow OAM and the frame's own variables.
pub fn store(m: &mut Mem, f: &HudFrame) {
    let mut vars = f.vars.clone();
    vars.race_state = f.g.race_state;
    vars.race_state_changed = f.g.race_state_changed;
    vars.portrait = f.g.portrait;
    vars.portrait_blink = f.g.portrait_blink;
    vars.bar = f.g.bar;
    vars.store(m, 0);
    for (k, o) in f.objects.iter().enumerate() {
        f.vars.objects.at(k as u32).write(m, o);
    }
    HudMessages { slots: f.messages }.store(m, 0);
    ShadowOam { entries: f.oam }.store(m, 0);
}

/// `FUN_08161f38` on the effect-sprite pool: draws its sprites into the shadow OAM.
pub fn draw_effect_sprites(rom: &[u8], m: &mut Mem) {
    let pool = SpritePool::load(m, 0);
    let mut sprites = pool.objects.read_n(m, pool.count.max(0) as u32);
    let mut oam = ShadowOam::load(m, 0);
    let tile_base = HudVars::load(m, 0).tile_base;
    crate::oam::draw_effect_sprites(rom, &mut sprites, pool.first, tile_base, &mut oam.entries);
    for (k, s) in sprites.iter().enumerate() {
        pool.objects.at(k as u32).write(m, s);
    }
    oam.store(m, 0);
}

/// The shadow OAM as the game copies it to OAM each frame (`FUN_0816102c`).
pub fn shadow_oam_bytes(m: &Mem) -> Vec<u8> {
    let oam = ShadowOam::load(m, 0).entries;
    oam.iter().flatten().flat_map(|v| v.to_le_bytes()).collect()
}
