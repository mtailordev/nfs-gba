//! The menu state: the game-state and screen globals and the profile fields the menu flow reads and writes
//! (names follow `docs/formats/ui.md`, "Menus"). Screen-local scratch is not declared here.

use crate::layout::{Field, Ptr};
use crate::{Mem, layout};

layout! {
    /// The globals of the top level and the menu dispatcher (IWRAM).
    pub struct MenuGlobals: 0 {
        /// The race outcome (5: quit to the menu) and the player's entity slot.
        0x0300_0048 race_outcome: u32,
        /// Bit per race mode: hints allowed.
        0x0300_0070 mode_bits: u32,
        /// The race setup the career event screen writes (AI skill, difficulty 0..2, laps, traffic, mode).
        0x0300_00BC skill: u32,
        0x0300_5604 traffic: u32,
        0x0300_5608 difficulty: u32,
        0x0300_56E0 race_mode: u32,
        0x0300_56E4 laps: u32,
        /// Per-racer start slot bytes (indexed by the player's slot) and the value written.
        0x0300_538C slot_bytes: [u8; 12],
        0x0300_53BC start_slot: u32,
        0x0300_57F0 unpack_buffer: u32,
        /// The interface language (0 English … 4).
        0x0300_5600 language: u32,
        /// The IWRAM divide routine's remainder.
        0x0300_6480 div_remainder: u32,
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
        /// The race results and their ranked copy (the standings).
        0x0300_5650 results: RaceResults,
        0x0300_5730 ranked: RaceResults,
        /// Opponents in the race (the results have this + 1 slots).
        0x0300_5784 opponents: u32,
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
        /// The map screens' state: view x/y (8.8), the cursor and "moved".
        0x0300_6230 map_x: i32,
        0x0300_6234 map_y: i32,
        0x0300_6238 map_cursor: i8,
        0x0300_6239 map_moved: u8,
        0x0300_64C0 keys: u16,
        /// The options screen's globals: units, sound flag, music and sound volumes (x8), and scratch the
        /// settings copy (`MenuProfile::settings`).
        0x0300_0040 units: u32,
        0x0300_0050 u_0050: u32,
        0x0300_53E4 u_53e4: u32,
        0x0300_5798 u_5798: u32,
        0x0300_578C music_volume: u32,
        0x0300_53A4 sound_volume: u32,
        0x0300_5994 u_5994: u32,
        /// The settings differ from the profile's copy (set by A on a settings screen).
        0x0300_5998 settings_changed: u32,
        /// Frame counter of the blinking cursors (bit 4) and PRESS START.
        0x0300_53B4 flash: u32,
        /// Keys held (L and R turn the garage car).
        0x0300_64C4 keys_held: u16,
        /// The garage car record's working copy (17 bytes) and its turn angle.
        0x0300_5700 garage_car: [u8; 17],
        0x0300_5F9C garage_angle: u32,
        /// EEPROM save buffer (passed to the save routines).
        0x0300_57F4 save_buffer: u32,
        /// Race setup scratch a free race sets (`+0x580C` 0, `+0x562C` 1..3, `+0x6118` 0..19).
        0x0300_580C u_580c: u32,
        0x0300_562C u_562c: u32,
        0x0300_6118 u_6118: u32,
        /// The garage's "upgrades changed" answer, kept for the save question.
        0x0300_5954 upgrades_changed: u32,
        /// The open message box's text key and argument.
        0x0300_59F8 message_text: u32,
        0x0300_59EC message_arg: u32,
        0x0300_64C8 rand_index: u32,
    }

    /// The race results, 4 slots (`results`; the ranked copy has the same layout).
    pub struct RaceResults: 0x40 {
        0x00 head: [u8; 4],
        /// Entity id per slot.
        0x04 ids: [u8; 4],
        /// 8: knocked out.
        0x08 knocked: [u8; 4],
        /// The position (ranking key 2).
        0x0C position: [u8; 4],
        0x10 best_lap: [u32; 4],
        /// Finish time (ranking key 1).
        0x20 finish: [u32; 4],
        /// Hunter life (ranking key 4).
        0x30 life: [u32; 4],
    }

    /// The profile fields the menu flow touches (the struct lives in EWRAM, `MenuGlobals::profile`).
    pub struct MenuProfile: 0x4C8 {
        /// The car in Quick Play (`+0x11`) and career (`+0x10`).
        0x10 career_car: i8,
        0x11 car: i8,
        0x0C cash: i32,
        0x12 u_12: u16,
        /// The 15 cars' 17-byte records.
        0xF9 car_records: [[u8; 17]; 15],
        /// Career hints seen (`+0x1F8`, `+0x1F9`) and the hint flag (`+0x1FA`).
        0x1F8 hints_a: u8,
        0x1F9 hints_b: u8,
        0x1FA hint_flag: u8,
        /// The career district (zone) selected on the map.
        0x1FB zone: u8,
        /// The selected event slot in the zone.
        0x1FC event_slot: u8,
        /// 2 bits per career event: 1 won, 2 second, 3 not done.
        /// The wingman (`+0x200`, 0 none) and its side (`+0x204`).
        0x200 wingman: u32,
        0x204 wingman_side: u8,
        0x205 events: [u8; 19],
        /// Record time (frames) per track: 12 circuits, then 18 sprints.
        0x218 records: [u16; 30],
        /// The event cursor per zone.
        /// The garage upgrade pages' selection (`+0x364`, `+0x365`).
        0x364 upgrade_a: u8,
        0x365 upgrade_b: u8,
        /// The settings screens' cursor per screen and their arrow delays (2 per item).
        0x368 setup_cursors: [u8; 6],
        0x374 setup_delays: [i8; 16],
        0x388 event_cursors: [u8; 6],
        /// Race end: a new record was set (`+0x3B4`), the payout (`+0x3B8`).
        0x3B4 record_flag: u32,
        0x3B8 payout: u32,
        0x3BC settings_head: u32,
        /// The settings screens' copy of the race and option settings (`setting_variable` 2..=0x10): reverse,
        /// laps, difficulty, opponents, traffic, `u_580c`, `u_53e4`, units, HUD, `u_5798`, music/8, sound/8,
        /// language, `u_0050`.
        0x3C0 settings: [u32; 15],
        /// A new track record (`+0x4A8`) and the unlock message keys (`+0x4AA`, 0-terminated).
        0x494 u_494: u16,
        0x4A8 new_record: u16,
        0x4AA unlock_messages: [u16; 14],
        /// `+0x256`: a career zone step is due; `+0x258`: the upgrades save question is open.
        0x256 zone_step: u16,
        0x258 upgrades_saving: u16,
        0x2EE music: i8,
        0x2F6 u_2f6: u16,
        0x2F8 u_2f8: u32,
        0x32C last_player: u32,
        /// Key-repeat delays: left, right, up, down, A, B, L, R (`menu_frame`).
        0x338 u_338: u32,
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

impl RaceResults {
    /// The table as the game's byte image (what `career::rank_results` sorts).
    pub fn to_bytes(&self) -> [u8; 0x40] {
        let mut b = [0; 0x40];
        b[..4].copy_from_slice(&self.head);
        b[4..8].copy_from_slice(&self.ids);
        b[8..12].copy_from_slice(&self.knocked);
        b[12..16].copy_from_slice(&self.position);
        for (at, words) in [(0x10, &self.best_lap), (0x20, &self.finish), (0x30, &self.life)] {
            for (k, w) in words.iter().enumerate() {
                b[at + 4 * k..][..4].copy_from_slice(&w.to_le_bytes());
            }
        }
        b
    }

    pub fn from_bytes(b: &[u8; 0x40]) -> Self {
        let words = |at: usize| std::array::from_fn(|k| u32::from_le_bytes(b[at + 4 * k..][..4].try_into().unwrap()));
        RaceResults {
            head: b[..4].try_into().unwrap(),
            ids: b[4..8].try_into().unwrap(),
            knocked: b[8..12].try_into().unwrap(),
            position: b[12..16].try_into().unwrap(),
            best_lap: words(0x10),
            finish: words(0x20),
            life: words(0x30),
        }
    }
}

impl MenuProfile {
    /// `unlock_is_locked` (`0x0812D784`): 1 when bit `id` of the unlock bits is clear.
    pub fn is_locked(&self, id: i32) -> u32 {
        (self.unlock_byte((id >> 3) as usize) as i32 >> (id & 7) & 1 == 0) as u32
    }

    /// Byte `i` of the unlock bits (0 past the declared 48).
    pub fn unlock_byte(&self, i: usize) -> u8 {
        let byte = self
            .unlocks
            .get(i)
            .or_else(|| self.unlocks_more.get(i.wrapping_sub(32)));
        byte.copied().unwrap_or(0)
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
