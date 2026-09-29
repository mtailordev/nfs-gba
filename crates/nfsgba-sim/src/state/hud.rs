//! The HUD's state (`nfsgba_formats::{hud, ui}` are already typed): the IWRAM variables it reads and writes, the
//! sprite-screen objects it animates, the message slots and the shadow OAM.

use nfsgba_formats::{hud, ui};

use crate::layout;
use crate::layout::Ptr;

layout! {
    /// The HUD's variables (`hud::Globals` minus the needle scale, which the profile holds) and the sprite screen
    /// of the race HUD (world `+0xA4`: objects at `+0x14`, screen index at `+0x18`).
    pub struct HudVars: 0 {
        /// Speed units, 0 = mph.
        0x0300_0040 units: u32,
        /// Race state and its change flag.
        0x0300_0048 race_state: u32,
        0x0300_00AC race_state_changed: u32,
        /// The race HUD's sprite screen: objects and screen index.
        0x0300_0178 objects: Ptr<ui::Object>,
        0x0300_017C screen: u16,
        /// Non-zero: message 2 is shown.
        0x0300_5384 message_flag: u32,
        0x0300_5388 route: u32,
        0x0300_5600 language: u32,
        0x0300_5698 hud: u32,
        0x0300_56E0 mode: i32,
        0x0300_56E4 laps: u32,
        0x0300_5784 opponents: u32,
        0x0300_57EC ai_cars: u32,
        0x0300_57F8 player: u32,
        /// Race time in frames.
        0x0300_5800 frames: i32,
        0x0300_601C arrow: i32,
        0x0300_6104 wingman: u32,
        0x0300_615C split: i32,
        0x0300_6188 bar_max: i32,
        0x0300_61D4 portrait_blink: u32,
        0x0300_61DC portrait: i32,
        0x0300_61E4 bar: i32,
        /// The remainder of the HUD's last division (the speed's ones digit).
        0x0300_6480 digit: i32,
        /// The OBJ tile base.
        0x0300_64E0 tile_base: u16,
    }

    /// The six message slots.
    pub struct HudMessages: 0 {
        0x0300_6210 slots: hud::Messages,
    }

    /// A sprite-screen object (the array at the sprite screen's `+0x14`).
    impl ui::Object: 0x10 {
        0x00 flags: u16,
        0x02 scale: [i16; 2],
        0x06 frame: i16,
        0x08 loaded: i16,
        0x0A dy: i16,
        0x0C dx: i16,
        0x0E angle: u16,
    }
}

/// The shadow OAM (`0x030064F0`), copied to OAM each frame. Declared by hand: `layout!` derives `Default`, which
/// arrays of 128 do not have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowOam {
    pub entries: ui::Oam,
}

impl Default for ShadowOam {
    fn default() -> Self {
        ShadowOam { entries: [[0; 4]; 128] }
    }
}

impl layout::Field for ShadowOam {
    const SIZE: u32 = 0;
    fn load(m: &crate::Mem, at: u32) -> Self {
        ShadowOam {
            entries: <ui::Oam as layout::Field>::load(m, at + 0x0300_64F0),
        }
    }
    fn store(&self, m: &mut crate::Mem, at: u32) {
        <ui::Oam as layout::Field>::store(&self.entries, m, at + 0x0300_64F0);
    }
}

impl layout::Layout for ShadowOam {
    const FIELDS: &'static [(&'static str, u32, u32)] = &[("entries", 0x0300_64F0, 0x400)];
}
