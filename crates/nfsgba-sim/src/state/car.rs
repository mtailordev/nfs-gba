//! The car's typed state: the rigid body, wheels and physics struct, the car step's globals and the profile's
//! stats. Offsets and names follow `docs/engine/physics.md`.

use super::EntityRef;
use crate::layout;

layout! {
    /// The car's rigid body (physics struct `+0xC8`; `body.rs`).
    pub struct RigidBody: 0xB4 {
        0x00 mass: i32,
        0x04 inv_mass: i32,
        /// 8.8.
        0x08 pos: [i32; 3],
        /// x, y, z, w; 1.0 = 0x1000.
        0x20 quat: [i32; 4],
        0x30 momentum: [i32; 3],
        0x48 ang_momentum: [i32; 3],
        0x54 vel: [i32; 3],
        /// 3×3 row-major, 20.12; row 2 is forward.
        0x60 rot: [i32; 9],
        0x90 ang_vel: [i32; 3],
        0x9C quat_rate: [i32; 4],
        0xAC inertia: i32,
        0xB0 inv_inertia: i32,
    }

    /// A wheel (physics struct `+0x18C + 0x94·i`; 0 and 1 front).
    pub struct Wheel: 0x94 {
        0x0C contact: [i32; 3],
        /// In world axes, then in car axes.
        0x18 world_pos: [i32; 3],
        0x48 car_pos: [i32; 3],
        0x64 spin: i32,
        0x68 spring: i32,
        0x6C damping: i32,
        0x70 drive: i32,
        0x74 brake: i32,
        0x78 ride_height: i32,
        0x7C slip: i32,
        0x80 grip: i32,
        0x84 base_grip: i32,
        0x88 u_88: i32,
        0x8C u_8c: i32,
        /// The sector under the wheel.
        0x90 sector: u16,
        0x92 u_92: u16,
    }

    /// The car physics struct (0x4FC bytes, heap, entity `+0x8C`): the driver of a player, opponent or wingman car.
    pub struct Car: 0x4FC {
        /// The heading the chase camera follows (14-bit), and its sign word.
        0x000 heading: i32,
        0x004 heading_sign: i32,
        /// Suspension per point: vertical speed.
        0x008 point_speed: [i32; 4],
        0x018 u_018: i32,
        0x01C u_01c: i32,
        /// ±0x80000.
        0x020 steering: i32,
        /// 0..0x9999.
        0x024 throttle: i32,
        0x028 brake: i32,
        0x02C u_02c: i32,
        /// Inertia terms from handling `+0x00/+0x08/+0x0C/+0x10`.
        0x034 inertia: [i32; 2],
        0x03C revs: i32,
        /// 0 reverse, 1 neutral, 2..7 first..sixth.
        0x040 gear: i32,
        /// |`forward_speed`|.
        0x044 speed: i32,
        /// Suspension: points on the floor, spring position, spring rate and height per point.
        0x048 points_on_floor: i32,
        0x04C spring: [i32; 4],
        0x05C spring_rate: [i32; 4],
        0x06C point_height: [i32; 4],
        /// Wheel angle (`>> 8`; the rim redraw rotates by it). physics.md calls it the odometer.
        0x090 wheel_angle: i32,
        /// AI: target heading (14-bit), curve term.
        0x094 target_heading: i32,
        0x098 curve: i32,
        0x09C gearbox_pause: i32,
        /// Forward speed × 32.
        0x0A0 forward_speed: i32,
        /// Race position (1-based) and race progress.
        0x0A8 position: i32,
        0x0AC progress: i32,
        0x0B0 u_0b0: i32,
        /// Best lap time and lap start time (race time).
        0x0B4 best_lap: i32,
        0x0B8 lap_start: i32,
        0x0BC u_0bc: u32,
        /// Nearest lane (`0x087F4120`).
        0x0C0 lane: u16,
        0x0C2 u_0c2: u16,
        0x0C5 laps_left: i8,
        0x0C6 laps: u8,
        0x0C8 body: RigidBody,
        /// Grid/skill value.
        0x188 grid: i32,
        0x18C wheels: [Wheel; 4],
        /// Sum of the wheel spins.
        0x3DC wheel_spin: i32,
        /// Upgrade levels (the last is 1 in career mode 2).
        0x3E0 upgrades: [i32; 10],
        /// Gear ratios × final drive: reverse, neutral, first..sixth.
        0x408 gear_ratios: [i32; 8],
        /// Torque multiplier (0x8000 once the timer runs out), its timer, 1 << lane.
        0x428 torque_multiplier: i32,
        0x42C torque_timer: i32,
        0x430 lane_bit: u32,
        0x434 u_434: u16,
        0x436 u_436: u16,
        /// Centre-of-mass offset (y forced to −0x1800).
        0x438 centre_of_mass: [i32; 3],
        /// Launch/limiter counter.
        0x444 launch: i32,
        /// Contact flags: 0x10 wall, 2/4 side, 1 blocks the parked stop, 8 kept.
        0x448 contact: u32,
        0x44C upshift_rpm: i32,
        0x450 lower_rpm: i32,
        0x454 max_rpm: i32,
        0x458 engine_braking: i32,
        /// Shift rpm (torque peak).
        0x45C shift_rpm: i32,
        /// Neutral revs torque multiplier (0x1000..0x1800).
        0x460 neutral_torque: i32,
        /// Torque curve (18 words from handling `+0x6C`; 10 used).
        0x464 torque_curve: [i32; 18],
        0x4AE prev_control: u16,
        /// AI: steps without progress (0x32 resyncs the segment, 0x96 puts the car back).
        0x4B0 stuck: i32,
        /// Bit 0: hard wall hit.
        0x4B4 hard_hit: u32,
        0x4B8 u_4b8: i32,
        0x4BC torque_scale: i32,
        /// Last touched wall with flag 0x4000.
        0x4C0 wall_4000: u32,
        0x4C4 u_4c4: u32,
        /// Nitro: tank, drain and torque factors, on.
        0x4C8 nitro_tank: i32,
        0x4CC nitro_drain: u16,
        0x4CE nitro_torque: u16,
        0x4D0 u_4d0: u8,
        0x4D1 nitro_on: u8,
        0x4D2 u_4d2: u16,
        /// AI: lane-change timer.
        0x4D4 lane_timer: u16,
        /// AI: preferred lane; set to 2 when the car changes section.
        0x4D6 u_4d6: u16,
        /// 1 before the start line, 2 after waypoints 1..9, 8 out of the ranking, 0x10 no yaw damping.
        0x4D8 route_flags: u16,
        /// AI: lanes 0..4 blocked ahead (traffic in lanes 1 and 3, racers close ahead).
        0x4DA blocked: [u16; 5],
        0x4E4 tipped: i16,
        0x4E6 airborne: i16,
        0x4E8 hunter_life: i32,
        0x4EC wrong_way: i16,
        0x4EE stationary: i16,
        0x4F0 u_4f0: u16,
        0x4F2 hunter_countdown: u16,
        /// AI: the entity followed while `u_4f0` runs.
        0x4F4 follow: EntityRef,
        /// AI: boost timer.
        0x4F8 boost_timer: i16,
    }

    /// A racing-line section (world `+0x40`, 8 bytes): `nfsgba_formats::career::Section`.
    pub struct SectionRec: 8 {
        0x00 count: u16,
        0x02 flags: u16,
        0x04 first: u32,
    }

    /// A racing-line waypoint (world `+0x44`, 0x18 bytes): `LinePoint` plus the heading of its line and the
    /// sector it lies in.
    pub struct WaypointRec: 0x18 {
        0x00 x: i32,
        0x04 z: i32,
        0x0A heading: u16,
        0x0C link_section: u16,
        0x0E link_index: u16,
        0x10 distance: i32,
        0x14 sector: i32,
    }

    /// The profile's stats the car step keeps (`Profile` holds the rest).
    pub struct CarProfile: 0x498 {
        /// The AI's drive-force curve (`ai::speed_curve`): count, x range, the values (the game's pointer to them at
        /// `+0x278` always names `curve`).
        0x26C curve_count: i32,
        0x270 curve_x0: i32,
        0x274 curve_x1: i32,
        0x27C curve: [i32; 21],
        /// Distance driven (speed >> 12 per step), skids counted, top speed.
        0x2D0 distance: i32,
        0x2D8 skids: i32,
        0x2DC top_speed: i32,
        /// HUD flags: gear changed, accelerating.
        0x2E0 gear_changed: i32,
        0x2E8 accelerating: i32,
        /// The wall contact sound is playing.
        0x2EC scrape: u8,
        /// Set in phase 6: the race end resets the camera setting (`0x030053E4` = 1) and clears it.
        0x2F4 camera_reset: u8,
        0x2EF engine_sound: i8,
        /// Per entity: the skid sound is on.
        0x318 skid_sound: [u32; 8],
    }

    /// The world's query point (the sector search's scratch: `WorldHeader::query`, `query_sector`).
    pub struct Query: 0 {
        0x0300_0180 pos: [i32; 3],
        0x0300_01AA sector: u16,
    }

    /// The globals the car step reads and writes (IWRAM). Names follow `docs/engine/physics.md`.
    pub struct CarGlobals: 0 {
        0x0300_0030 u_0030: i32,
        /// 0 not started, 1 and 4 frozen, 3 race over, 9 replay.
        0x0300_0048 phase: i32,
        /// Catch-up on: the AI eases off ahead of the player and presses on behind.
        0x0300_0050 catch_up: i32,
        /// Entity index of the local player.
        0x0300_0060 player: u32,
        0x0300_006C level: i32,
        /// 0 quick race, 1 and 2 career.
        0x0300_00A0 career: i32,
        0x0300_00BC career_level: i32,
        /// The player has been going the wrong way for over 27 steps.
        0x0300_5384 wrong_way: u32,
        0x0300_5388 u_5388: u32,
        /// Sound effect volume option.
        0x0300_53A4 volume: u32,
        /// Race frames since the start (the AI waits 0x78 / 0xB4; picks the traffic model).
        0x0300_5628 race_frames: u32,
        /// Counts race steps (the AI takes wider lanes in the first 0x32).
        0x0300_56F0 steps: u32,
        /// Frame-time tick the wingman's and the knocked-away timers count down by.
        0x0300_5934 frame_ticks: i32,
        0x0300_6048 u_6048: u32,
        /// Wheel-spin scale added to the grid value.
        0x0300_6110 spin_bias: i32,
        0x0300_5604 u_5604: u32,
        0x0300_5608 difficulty: u32,
        0x0300_5610 u_5610: i32,
        /// 2 in link play.
        0x0300_5624 link: i32,
        0x0300_5630 fade: i32,
        /// Frame time.
        0x0300_5640 dt: i32,
        /// Result bytes: `+8` per car (8 knocked out), `+0x20` a finish time per car.
        0x0300_5650 results: [u8; 32],
        0x0300_5670 results_b: [u8; 32],
        /// Race mode (2 hunter, 3 sprint).
        0x0300_56E0 mode: u32,
        0x0300_56E4 laps: i32,
        0x0300_5720 route_index: u32,
        0x0300_5780 race_over: i32,
        /// Racers besides the player.
        0x0300_5784 opponents: u32,
        /// Automatic gearbox.
        0x0300_5798 automatic: i32,
        0x0300_57D0 over_index: u32,
        /// Per entity: `~KEYINPUT` bits for the player, the AI's output for the others.
        0x0300_57D8 input: [u16; 10],
        /// Cars below this index are racers.
        0x0300_57EC racers: u32,
        /// The camera's car.
        0x0300_57F8 focus: u32,
        /// Race frames.
        0x0300_5800 time: u32,
        0x0300_5FE0 u_5fe0: u32,
        0x0300_5FE4 u_5fe4: u32,
        /// The player's upgrade totals (the HUD).
        0x0300_6000 upgrade_totals: [i32; 5],
        /// Off-route warning: -1 left, 1 right.
        0x0300_601C off_route: i32,
        0x0300_6020 u_6020: u32,
        0x0300_6028 u_6028: i32,
        0x0300_6030 gravity: i32,
        /// The manual gearbox's state (L/R).
        0x0300_6074 shift_state: i16,
        0x0300_6076 u_6076: i16,
        0x0300_6078 u_6078: i32,
        0x0300_6080 u_6080: u32,
        0x0300_6084 u_6084: i32,
        0x0300_6088 u_6088: u32,
        /// Non-zero for a circuit (the lap wraps).
        0x0300_608C circuit: u32,
        0x0300_6090 u_6090: u32,
        /// The player's final drive.
        0x0300_60A4 final_drive: i32,
        0x0300_60A8 u_60a8: u32,
        0x0300_60AC u_60ac: u32,
        /// Route segments visited.
        0x0300_60C0 visited: [u32; 16],
        /// Wingman kind (1..=12).
        0x0300_6104 wingman: i32,
        /// 1 once a step has run (the suspension step is unreachable).
        0x0300_610C settled: i32,
        0x0300_614C u_614c: i32,
        0x0300_6150 nitro_free: i32,
        0x0300_6154 time_limit: i32,
        0x0300_6158 u_6158: i32,
        /// The time gap to the neighbouring place.
        0x0300_615C gap: i32,
        0x0300_6170 u_6170: i32,
        0x0300_6174 u_6174: i32,
        0x0300_6178 wingman_target: EntityRef,
        0x0300_6180 u_6180: i32,
        0x0300_6188 u_6188: u32,
        0x0300_618C u_618c: u32,
        0x0300_6190 u_6190: i32,
        0x0300_6194 u_6194: i32,
        0x0300_619C wingman_car: EntityRef,
        /// Someone finished.
        0x0300_61A4 finished: i32,
        /// Hunter races: the life a hit costs, per impulse.
        0x0300_61A0 hunter_damage: i32,
        0x0300_61D4 u_61d4: u32,
        0x0300_61D8 wingman_cooldown: i32,
        0x0300_61DC wingman_commands: i32,
        0x0300_61E4 u_61e4: u32,
        0x0300_61E8 wingman_running: u32,
        0x0300_61EC u_61ec: i32,
        0x0300_61F0 u_61f0: i32,
        0x0300_61F8 wingman_attacker: i32,
        0x0300_61FC u_61fc: u32,
        0x0300_6200 u_6200: i32,
        0x0300_6240 traffic_count: u8,
        /// Traffic: steps between speed-ups, the number of models.
        0x0300_624C speed_up_steps: u32,
        0x0300_6250 u_6250: u8,
        0x0300_625C traffic_models: u32,
        0x0300_6260 traffic_period: u32,
        0x0300_6264 traffic_timer: u8,
        /// The live traffic cars (entities, null = free), their speed limit and their stop factor.
        0x0300_6270 live: [EntityRef; 8],
        0x0300_6290 traffic_max_speed: u32,
        0x0300_6294 traffic_stop: i32,
        0x0300_6298 traffic_on: u8,
        /// Control binding set: 0 when the gearbox is automatic.
        0x0300_629C binding_set: u8,
        /// The IWRAM division's remainder scratch.
        0x0300_6480 div_rem: i32,
        /// `rand_table` index.
        0x0300_64C8 rand: u32,
        /// The route's per-section distance scale onto the lap (x256; `RacingLine::scales`).
        0x0300_6120 scales: [i32; 10],
    }
}

impl Car {
    /// The word at `offset` bytes into the struct's memory as the game lays it out (the game reads a few words
    /// before the torque curve with a negative rpm). Offsets outside the struct read as 0.
    pub fn word(&self, offset: i32) -> i32 {
        use crate::layout::Field;
        if !(0..Car::SIZE as i32 - 3).contains(&offset) {
            return 0;
        }
        let mut m = crate::Mem::new(Vec::new(), vec![0; 0x4_0000], vec![0; 0x8000]);
        self.store(&mut m, 0x0200_0000);
        m.i32(0x0200_0000 + offset as u32)
    }
}
