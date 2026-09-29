//! The menu state: the game-state and screen globals and the profile fields the menu flow reads and writes
//! (names follow `docs/formats/ui.md`, "Menus"). Screen-local scratch is not declared here.

use crate::layout::{Field, Ptr};
use crate::{Mem, layout};

layout! {
    /// The globals of the top level and the menu dispatcher (IWRAM).
    pub struct MenuGlobals: 0 {
        /// The race outcome (5: quit to the menu) and the player's entity slot.
        0x0300_0048 race_outcome: u32,
        0x0300_0060 race_player: u32,
        /// Environment of the chosen route (first byte of the route record).
        0x0300_006C environment: u32,
        /// Career mode (non-zero) or Quick Play.
        0x0300_00A0 career: u32,
        0x0300_5388 route: u32,
        0x0300_5398 u_5398: u32,
        0x0300_55F0 race_palette: u32,
        0x0300_5610 reverse: u32,
        0x0300_5620 level_desc: u32,
        0x0300_5624 timing_mode: u32,
        0x0300_5628 frame_counter: u32,
        /// Palette fade: > 0 fading in, < 0 fading out, by 2 per frame.
        0x0300_5630 fade: i32,
        0x0300_563C palette_dirty: u32,
        0x0300_5640 frame_time: u32,
        0x0300_5658 u_5658: [u8; 4],
        0x0300_5660 u_5660: [u32; 4],
        0x0300_5670 u_5670: [u32; 4],
        0x0300_5680 u_5680: [u32; 4],
        0x0300_5698 hud_on: u32,
        0x0300_56EC profile: Ptr<MenuProfile>,
        0x0300_56F0 u_56f0: u32,
        0x0300_5718 player_car: u32,
        0x0300_5720 route_flag: u32,
        0x0300_577C second_palette: u32,
        /// 7: leave the menus for the race once the fade has finished.
        0x0300_5780 menu_exit: u32,
        0x0300_57E0 u_57e0: u32,
        /// 0 boot, 1 menus, 4 race start, 5 race.
        0x0300_5808 game_state: u32,
        0x0300_5934 timer_ticks: u32,
        0x0300_5938 screen_entered: u32,
        /// Top of the back stack (profile `back`), -1: empty.
        0x0300_593C back_top: i8,
        0x0300_5940 back_top_saved: u8,
        /// Menu screen 0..=0x30; 0x80/0x81: leave for a race; 0x82: resume.
        0x0300_5944 screen: u32,
        /// The next draw is a full one.
        0x0300_5948 screen_changed: u32,
        /// Whose exit handler runs once the menus are left (-1: none).
        0x0300_594C exit_screen: u32,
        /// Open message box (< 0: none; 2 also takes B).
        0x0300_59F0 message_box: i32,
        0x0300_59F4 message_result: i32,
        0x0300_611C race_car: u8,
        0x0300_64C0 keys: u16,
        0x0300_64C8 rand_index: u32,
    }

    /// The profile fields the menu flow touches (the struct lives in EWRAM, `MenuGlobals::profile`).
    pub struct MenuProfile: 0x460 {
        /// The car in Quick Play (`+0x11`) and career (`+0x10`).
        0x10 career_car: i8,
        0x11 car: i8,
        0x12 u_12: u16,
        0x2EE music: i8,
        0x2F6 u_2f6: u16,
        0x2F8 u_2f8: u32,
        0x32C last_player: u32,
        /// Key-repeat delays: left, right, up, down, A, B, L, R (`menu_frame`).
        0x33C repeats: [i8; 8],
        /// The screens to go back to.
        0x344 back: [u8; 12],
        /// The List screens' cursor slots (`list_slot`).
        0x350 cursors: [u8; 17],
        /// Map screens' mode.
        0x404 map_mode: u8,
        /// Unlock bits by id.
        0x42D unlocks: [u8; 32],
        0x44D unlocks_more: [u8; 16],
    }
}

impl MenuProfile {
    /// `unlock_is_locked` (`0x0812D784`): 1 when bit `id` of the unlock bits is clear.
    pub fn is_locked(&self, id: i32) -> u32 {
        let i = (id >> 3) as usize;
        let byte = self
            .unlocks
            .get(i)
            .or_else(|| self.unlocks_more.get(i.wrapping_sub(32)));
        (byte.copied().unwrap_or(0) as i32 >> (id & 7) & 1 == 0) as u32
    }
}

/// The menu state: what the typed dispatcher works on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MenuState {
    pub g: MenuGlobals,
    pub profile: MenuProfile,
}

impl MenuState {
    /// The profile is read where `g.profile` points.
    pub fn load(m: &Mem) -> Self {
        let g = MenuGlobals::load(m, 0);
        // ponytail: a profile pointer outside EWRAM (before the boot sets it) reads as a default profile.
        let profile = if g.profile.addr >> 24 == 2 {
            MenuProfile::load(m, g.profile.addr)
        } else {
            Default::default()
        };
        MenuState { g, profile }
    }

    pub fn store(&self, m: &mut Mem) {
        self.g.store(m, 0);
        if self.g.profile.addr >> 24 == 2 {
            self.profile.store(m, self.g.profile.addr);
        }
    }
}
