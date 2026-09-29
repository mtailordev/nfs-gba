//! The typed game state shared by the simulation and the game loop, each field at its place in the GBA RAM
//! ([`layout!`]; conventions in `docs/engine/typed-state.md`). Names follow `docs/engine/physics.md` and
//! `docs/engine/address-map.md`; an offset the ported code touches without a known meaning is `u_<offset>`.
//! Bytes no field declares are ones no ported code reads or writes.

use nfsgba_formats::render::Piece;

use crate::layout;
use crate::layout::Ptr;

/// The world struct (IWRAM).
pub const WORLD: u32 = 0x0300_00C0;
/// The race camera's matrix (the world's `+0x54` points here during a race).
pub const CAMERA_MATRIX: u32 = 0x0300_57A0;

layout! {
    /// The world struct (`WORLD`): the city's tables (walls and sectors are ROM addresses: `GameData` holds what is
    /// there), the entity array, the renderer's buffers, the sector search's query point, and the counts.
    pub struct WorldHeader: 0x100 {
        /// u16 per sector: the head of its entity list (`0xFFFF` none).
        0x0C sector_heads: Ptr<u16>,
        0x10 walls: u32,
        0x14 sectors: u32,
        0x18 pieces: Ptr<Piece>,
        0x1C sector_offsets: Ptr<SectorOffset>,
        0x3C entities: Ptr<Entity>,
        /// Racing-line section table and racing line.
        0x40 sections: u32,
        0x44 racing_line: u32,
        /// Per-material runtime entries (8 bytes: `+2` frame, `+4`/`+6` u/v scroll).
        0x48 materials: u32,
        0x50 view: Ptr<ViewPort>,
        0x54 camera_matrix: Ptr<[i32; 12]>,
        /// Screen rectangle: left, right, top, bottom.
        0x58 rect: [i16; 4],
        /// The visible-sector list; entry 0 is the root portal the camera sets.
        0x60 visible: Ptr<ListEntry>,
        0x64 sector_map: u32,
        0x68 wall_buffer: u32,
        0x6C flat_buffer: u32,
        /// Sector search scratch: the query point (x, y, z city units) and start sector (`+0xEA`).
        0xC0 query: [i32; 3],
        0xD8 material_count: u16,
        0xDA sector_count: u16,
        0xDC wall_count: u16,
        0xDE sector_offset_count: u16,
        0xE0 piece_count: u16,
        0xEA query_sector: u16,
        0xF0 u_f0: u16,
        /// The sky is visible this frame.
        0xF6 sky: u16,
        /// First entity and entity count for the update loop.
        0xF8 first_entity: u16,
        0xFA entity_count: u16,
        /// Matrix slots, 0x30 each, 64 slots.
        0xFC matrix_slots: u32,
    }

    /// A sector's offsets record (world `+0x1C`, by sector `+0x0A`): height offsets and the sloped floor plane.
    pub struct SectorOffset: 0x14 {
        0x04 ceiling: i16,
        0x06 floor: i16,
        /// Replaces sector `+0x12` (`0x40` hides the sector).
        0x08 flags: u16,
        /// The floor plane (a, b, c): height = `−(a·dx + c·dz) / b` from the sector's first wall corner.
        0x0C plane: [i16; 3],
    }

    /// An entity (0xA4 bytes, world `+0x3C`): a car, an opponent, traffic, an effect, a marker.
    pub struct Entity: 0xA4 {
        0x00 index: u16,
        /// Next entity in its sector's list (heads: world `+0x0C`).
        0x02 next: u16,
        /// Next entity in the sector's draw order (the renderer's sort).
        0x04 draw_next: u16,
        /// Bit 0 active; bit 1 updated by `update_entities`; bit 2 sorted and drawn.
        0x08 state: u16,
        /// Renderer flags (bit 2 drawn this frame; `nfsgba_formats::render::Entity`).
        0x0A flags: u16,
        /// Position, 8.8 city units (`-y` up).
        0x0C pos: [i32; 3],
        /// Traffic: direction x, (unknown), direction z (1.0 = 0x1000), speed.
        0x18 dir_x: i32,
        0x1C u_1c: i32,
        0x20 dir_z: i32,
        0x24 speed: i32,
        /// Sort key, camera space.
        0x28 key: i32,
        /// Heading << 8 (0x4000 per turn after `>> 8`).
        0x2C heading: i32,
        /// Angles of `build_entity_matrix`'s two rotations (traffic: pitch, shown heading).
        0x30 angles: [i16; 2],
        /// Far model (near = `model − 1`); a sector index with flag bit 4.
        0x36 model: i16,
        /// Traffic: heading wobble.
        0x38 wobble: i32,
        0x44 material_step: u16,
        0x46 material_offset: u16,
        0x48 material: u16,
        /// 0 init, 0x100 racing, 2 finished.
        0x4A race_state: u16,
        /// Handler index (table `0x087F38B8`).
        0x4E handler: u16,
        /// Traffic: speed-up counter, knocked-away timer.
        0x52 speed_up: u16,
        0x56 knocked: u16,
        /// Second model (the spoiler).
        0x64 extra_model: i16,
        0x70 u_70: u16,
        /// Route segment (0 main route).
        0x72 segment: u16,
        0x74 start_sector: u16,
        0x76 u_76: u16,
        0x78 sector: u16,
        /// Traffic type.
        0x7C traffic_type: u16,
        /// RAM address of the unpacked atlas (flag bit 3).
        0x84 atlas: u32,
        /// Matrix slot (`0xFF` not drawn) and car id.
        0x88 slot: u8,
        0x89 car: u8,
        /// The driver's physics struct; traffic uses this word as two u16 (`+0x8C`, `+0x8E`).
        0x8C driver: Ptr<Car>,
        /// Racing-line segment / route waypoint.
        0x90 waypoint: i16,
        0x94 u_94: u16,
        0x98 u_98: u16,
        /// Traffic: direction along the route (±1), waypoint, mode.
        0x9A direction: i16,
        0x9C traffic_waypoint: i16,
        0x9E traffic_mode: i16,
        0xA0 u_a0: u16,
    }

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
        0x4B0 u_4b0: i32,
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
        0x4D4 u_4d4: u16,
        0x4D6 u_4d6: u16,
        /// 1 before the start line, 2 after waypoints 1..9, 8 out of the ranking, 0x10 no yaw damping.
        0x4D8 route_flags: u16,
        0x4DA u_4da: u16,
        0x4DC u_4dc: i16,
        0x4DE u_4de: i16,
        0x4E0 u_4e0: i16,
        0x4E2 u_4e2: u16,
        0x4E4 tipped: i16,
        0x4E6 airborne: i16,
        0x4E8 hunter_life: i32,
        0x4EC wrong_way: i16,
        0x4EE stationary: i16,
        0x4F0 u_4f0: u16,
        0x4F2 u_4f2: u16,
        0x4F4 u_4f4: u32,
        0x4F8 u_4f8: u16,
    }

    /// The race globals (IWRAM).
    pub struct Race: 0 {
        /// The race phase (2 racing; `docs/engine/game-loop.md`).
        0x0300_0048 phase: u32,
        /// Entity index of the local player.
        0x0300_0060 player: u32,
        /// The player's entity, which the camera follows.
        0x0300_53AC player_entity: Ptr<Entity>,
        /// Entity index whose heading `shade_car_paint` uses and behind which the chase view resets.
        0x0300_57F8 focus: u32,
        0x0300_56EC profile: Ptr<Profile>,
        /// 5 in a race (`main_frame`'s state machine).
        0x0300_5808 game_state: u32,
        0x0300_5628 frame_count: u32,
        /// 2 in link play.
        0x0300_5624 link: u32,
        /// Frame time (25,500 / timer-3 ticks, clamped to 10..100).
        0x0300_5640 dt: u32,
        0x0300_5630 fade: i32,
        0x0300_5780 race_over: u32,
    }

    /// The keys (`~KEYINPUT`): held, and newly pressed this frame.
    pub struct Input: 0 {
        0x0300_64C0 pressed: u16,
        0x0300_64C4 held: u16,
    }

    /// The race camera (IWRAM globals).
    pub struct Camera: 0 {
        /// SELECT toggles it: 0 chase, 1 bumper.
        0x0300_53E4 setting: u32,
        /// Set while the bumper view is on; back in the chase view the camera resets behind the car.
        0x0300_5804 bumper_on: u32,
        /// The view: 0 bumper, 2 chase (index into the per-view tables, `GameData::views`).
        0x0300_55F8 view: u32,
        /// Chase orbit yaw.
        0x0300_5F94 orbit: i32,
        /// Look yaw (0x4000 per turn).
        0x0300_0214 look: i32,
        /// `−look & 0x3FFF`: the matrix's yaw.
        0x0300_5F9C matrix_yaw: i32,
        /// Height offset (8.8).
        0x0300_5FA4 height: i32,
        /// Position (8.8).
        0x0300_56A0 x: i32,
        0x0300_00A4 z: i32,
        0x0300_5614 sector: u16,
        /// The smoothed floor height at the camera (the height limit of flag-0x4000 walls in the wall push).
        0x0300_5778 floor: i32,
        /// Horizon shift in rows (±32).
        0x0300_56B8 horizon: i32,
        /// 0x82 above the car instead of 0x10, and the orbit does not follow.
        0x0300_6148 raised: u32,
        /// 3×3 rotation (2.14) then translation.
        0x0300_57A0 matrix: [i32; 12],
    }

    /// The screen globals: shake, projection centre offset, the rasteriser rectangle and the frame buffer.
    pub struct Screen: 0 {
        0x0300_5390 shake: [i16; 2],
        /// Extra projection-centre y offset (8.8).
        0x0300_53A0 centre_y: u32,
        /// Rasteriser rectangle: left, top, right, bottom.
        0x0300_53D0 rect: [i32; 4],
        /// Frame buffer: width, height, mode byte, the two pages.
        0x0300_6410 size: [i16; 2],
        0x0300_6418 mode: u8,
        0x0300_641C pages: [u32; 2],
    }

    /// The view struct (world `+0x50`, `0x03000080`).
    pub struct ViewPort: 0x20 {
        /// The page drawn into.
        0x00 page: u32,
        /// Projection centre.
        0x08 cx: i16,
        0x0A cy: i16,
        0x10 near: i32,
        0x1C focal: i32,
    }

    /// An entry of the visible-sector list (world `+0x60`): a sector and its portal rectangle.
    pub struct ListEntry: 0x10 {
        0x00 sector: u16,
        0x02 left: i16,
        0x04 right: i16,
        0x06 top: i16,
        0x08 bottom: i16,
        0x0A u_0a: [u16; 3],
    }

    /// The profile (EWRAM, saved to EEPROM), the fields the race code uses.
    pub struct Profile: 0x498 {
        /// Car-to-car contact this frame (set by the car steps).
        0x2E0 contact: [u32; 2],
        0x2EF engine_sound: i8,
        /// The camera sector has a ceiling: this frame, last frame.
        0x400 ceiling: [u8; 2],
        0x402 needle_scale: u8,
    }

    /// A moving wall piece (world `+0x18`, 0x20 bytes, by wall `+0x2A`).
    impl Piece: 0x20 {
        0x00 dx: i16,
        0x02 dz: i16,
        0x04 ceiling: i16,
        0x06 floor: i16,
        0x08 top: i16,
        0x0A bottom: i16,
        0x0C material: u16,
        0x0E flags: u16,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::assert_disjoint;

    #[test]
    fn fields_do_not_overlap() {
        assert_disjoint::<WorldHeader>("WorldHeader");
        assert_disjoint::<SectorOffset>("SectorOffset");
        assert_disjoint::<Entity>("Entity");
        assert_disjoint::<RigidBody>("RigidBody");
        assert_disjoint::<Wheel>("Wheel");
        assert_disjoint::<Car>("Car");
        assert_disjoint::<Race>("Race");
        assert_disjoint::<Input>("Input");
        assert_disjoint::<Camera>("Camera");
        assert_disjoint::<Screen>("Screen");
        assert_disjoint::<ViewPort>("ViewPort");
        assert_disjoint::<ListEntry>("ListEntry");
        assert_disjoint::<Profile>("Profile");
        assert_disjoint::<Piece>("Piece");
        assert!(<Camera as crate::layout::Layout>::FIELDS.contains(&("matrix", CAMERA_MATRIX, 48)));
    }
}
