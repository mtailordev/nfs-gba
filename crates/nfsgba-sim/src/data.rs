//! `GameData`: the ROM tables the typed code reads, parsed once with the `nfsgba-formats` parsers. Every BN7E
//! ROM offset the typed code uses is in [`bn7e`]. The maths tables (sine, atan, reciprocals) stay in the ROM image
//! that `nfsgba_fixed` reads; typed code that needs them takes the ROM bytes too.

use nfsgba_formats::{Sector, city};

/// The BN7E ROM offsets (file offsets; the GBA address is `0x08000000` more).
pub mod bn7e {
    /// `camera_dispatch`'s camera function per view (Thumb address; 0 none), 8 views.
    pub const CAMERA_FNS: usize = 0x7F_399C;
    /// Per camera view: x offset, height (8.8), distance; 6 words each (the game's views 6 and 7 read on into the
    /// next table).
    pub const VIEW_X: usize = 0x7F_39BC;
    pub const VIEW_HEIGHT: usize = 0x7F_39D4;
    pub const VIEW_DISTANCE: usize = 0x7F_39EC;
    /// The camera probe (0, 0, 72): the camera sector is the one 72 units ahead along the look direction.
    pub const CAMERA_PROBE: usize = 0x7B_FC68;
    /// `camera_update` (`0x08137cb0`) as the camera table holds it (Thumb bit set).
    pub const CAMERA_UPDATE: u32 = 0x0813_7CB1;
    pub const ROM_BASE: usize = 0x0800_0000;
    /// Handling records (0x158 bytes), one per car; 15 cars fit before `CORNERS`.
    pub const HANDLING: usize = 0x7F_1100;
    pub const HANDLING_COUNT: usize = 15;
    /// The eight body corners of a tipped-over car (3 words each).
    pub const CORNERS: usize = 0x7F_2528;
    pub const TIME_SCALE: usize = 0x7F_40D0;
    pub const LANES: usize = 0x7F_4120;
    /// Curves: count, x of the first and last point, pointer to the y words.
    pub const SLIP_DAMPING: usize = 0x7F_4164;
    pub const REAR_GRIP: usize = 0x7F_41A0;
    pub const WINGMAN_GRID: usize = 0x7F_42E4;
    pub const START_BYTES: usize = 0x7F_3050;
    pub const SIDE_SEGMENTS: usize = 0x7F_37D8;
    pub const BREAK_PARTNERS: usize = 0x7F_3CDA;
    /// Up to the rest vector.
    pub const BREAK_PARTNER_COUNT: usize = 0x7B;
    pub const REST: usize = 0x7F_3DD0;
    pub const BINDINGS: usize = 0x7F_5494;
    pub const AXLE_OFFSETS: usize = 0x7F_55FC;
    pub const SURFACE_GRIP: usize = 0x7F_5904;
    pub const UPGRADE_WEIGHTS: usize = 0x7F_5988;
}

/// What a camera view runs each frame (`camera_dispatch`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraFn {
    None,
    /// `camera_update`.
    Update,
    /// Another function (the Thumb address; view 7's `camera_look_at_player`).
    Other(u32),
}

/// A camera view's function and offsets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CameraView {
    pub function: CameraFn,
    pub x: i32,
    /// 8.8.
    pub height: i32,
    pub distance: i32,
}

/// A piecewise-linear curve: `ys.len() - 1` points (plus the ROM word after them, which the last segment reads) evenly spaced from `x0` to `x1` (`FUN_0813d1f0` inlines it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Curve {
    pub x0: i32,
    pub x1: i32,
    pub ys: Vec<i32>,
}

impl Curve {
    /// The curve at `x`, clamped to its first and last points.
    pub fn eval(&self, x: i32) -> i32 {
        let count = self.ys.len() as i32 - 1;
        let step = nfsgba_fixed::div(self.x1 - self.x0, count - 1);
        let k = nfsgba_fixed::div(x - self.x0, step);
        if k < 1 {
            self.ys[0]
        } else if k < count {
            let y = self.ys[k as usize];
            y + nfsgba_fixed::div((self.ys[k as usize + 1] - y).wrapping_mul(x - step * k), step)
        } else {
            self.ys[count as usize - 1]
        }
    }
}

/// The words of a handling record (`car.rs` and `init.rs` name the ones they use).
pub const HANDLING_WORDS: usize = 0x56;

/// A route's side sections: (section, its sectors).
pub type SideSegments = Vec<(u32, Vec<u32>)>;

/// The ROM tables the car step reads.
#[derive(Debug, Clone)]
pub struct CarTables {
    /// One handling record per car (0x158 bytes).
    pub handling: Vec<[i32; HANDLING_WORDS]>,
    /// Slip angle to yaw damping, and slip and yaw rate to rear grip.
    pub slip_damping: Curve,
    pub rear_grip: Curve,
    /// The lane offsets across the road.
    pub lanes: [i32; 4],
    /// Grip per floor surface (sector `+0x08`, capped at 7).
    pub surface_grip: [i32; 8],
    /// The eight body corners the tipped-over car rests on (car axes).
    pub corners: [[i32; 3]; 8],
    /// The zero vector (knocked-out cars' momenta, a parked car's motion).
    pub rest: [i32; 3],
    /// Per dynamic wall state: the state that breaks together with it (−1 none).
    pub break_partners: Vec<i16>,
    /// Car-to-car test: wheel-pair offsets along the car.
    pub axle_offsets: [i32; 2],
    /// Control bindings per binding set and action: (held mask, held value, pressed mask, pressed value).
    pub bindings: [[[u16; 4]; 9]; 2],
    /// Upgrade weights: 10 categories × 5 attributes.
    pub upgrade_weights: [[i32; 5]; 10],
    /// Grid values of the wingman's car (`setup_handling`), by wingman kind plus one (kind 0 reads the word before).
    pub wingman_grid: Vec<i32>,
    /// `race_start_setup`'s tables: word `0x087F40D0[level * 2 + u_5610]` and the byte table `0x087F3050[4 * n]`.
    pub time_scale: Vec<i32>,
    pub start_bytes: Vec<i8>,
    /// Per route: the side segments and the sectors that belong to them (`None`: the route has no list).
    pub side_segments: Vec<Option<SideSegments>>,
}

#[derive(Debug, Clone)]
pub struct GameData {
    /// The city: sectors with their walls, by sector index.
    pub city: Vec<Sector>,
    pub views: [CameraView; 8],
    pub camera_probe: [i32; 3],
    pub car: CarTables,
    pub effects: crate::slot_data::EffectTables,
    pub ai: crate::ai_tables::AiTables,
}

impl GameData {
    pub fn parse(rom: &[u8]) -> GameData {
        let word = |o: usize| u32::from_le_bytes(rom[o..o + 4].try_into().unwrap());
        GameData {
            city: city(rom),
            views: std::array::from_fn(|v| CameraView {
                function: match word(bn7e::CAMERA_FNS + 4 * v) {
                    0 => CameraFn::None,
                    bn7e::CAMERA_UPDATE => CameraFn::Update,
                    f => CameraFn::Other(f),
                },
                x: word(bn7e::VIEW_X + 4 * v) as i32,
                height: word(bn7e::VIEW_HEIGHT + 4 * v) as i32,
                distance: word(bn7e::VIEW_DISTANCE + 4 * v) as i32,
            }),
            camera_probe: std::array::from_fn(|k| word(bn7e::CAMERA_PROBE + 4 * k) as i32),
            car: CarTables::parse(rom),
            effects: crate::slot_data::EffectTables::parse(rom),
            ai: crate::ai_tables::AiTables::parse(rom),
        }
    }
}

impl CarTables {
    fn parse(rom: &[u8]) -> CarTables {
        use bn7e::*;
        let word = |o: usize| u32::from_le_bytes(rom[o..o + 4].try_into().unwrap());
        let half = |o: usize| i16::from_le_bytes(rom[o..o + 2].try_into().unwrap());
        let words = |o: usize, n: usize| (0..n).map(|k| word(o + 4 * k) as i32).collect::<Vec<_>>();
        let curve = |o: usize| {
            let (count, ys) = (word(o) as usize, word(o + 0xC) as usize - ROM_BASE);
            Curve {
                x0: word(o + 4) as i32,
                x1: word(o + 8) as i32,
                ys: words(ys, count + 1),
            }
        };
        CarTables {
            handling: (0..HANDLING_COUNT)
                .map(|c| std::array::from_fn(|k| word(HANDLING + 0x158 * c + 4 * k) as i32))
                .collect(),
            slip_damping: curve(SLIP_DAMPING),
            rear_grip: curve(REAR_GRIP),
            lanes: std::array::from_fn(|k| word(LANES + 4 * k) as i32),
            surface_grip: std::array::from_fn(|k| word(SURFACE_GRIP + 4 * k) as i32),
            corners: std::array::from_fn(|c| std::array::from_fn(|k| word(CORNERS + 12 * c + 4 * k) as i32)),
            rest: std::array::from_fn(|k| word(REST + 4 * k) as i32),
            break_partners: (0..BREAK_PARTNER_COUNT).map(|k| half(BREAK_PARTNERS + 2 * k)).collect(),
            axle_offsets: std::array::from_fn(|k| word(AXLE_OFFSETS + 4 * k) as i32),
            bindings: std::array::from_fn(|s| {
                std::array::from_fn(|a| std::array::from_fn(|k| half(BINDINGS + 0x48 * s + 8 * a + 2 * k) as u16))
            }),
            upgrade_weights: std::array::from_fn(|c| {
                std::array::from_fn(|k| word(UPGRADE_WEIGHTS + 0x14 * c + 4 * k) as i32)
            }),
            wingman_grid: words(WINGMAN_GRID - 4, 13),
            time_scale: words(TIME_SCALE, 64),
            start_bytes: rom[START_BYTES..START_BYTES + 0x200].iter().map(|&b| b as i8).collect(),
            side_segments: (0..nfsgba_formats::career::ROUTE_COUNT)
                .map(|route| match word(SIDE_SEGMENTS + 4 * route) as usize {
                    0 => None,
                    list => {
                        let list = list - ROM_BASE;
                        let mut at = list + 4;
                        let mut entries = Vec::new();
                        for _ in 0..word(list) {
                            let segment = word(at);
                            at += 4;
                            let mut sectors = Vec::new();
                            while word(at) != u32::MAX {
                                sectors.push(word(at));
                                at += 4;
                            }
                            at += 4;
                            entries.push((segment, sectors));
                        }
                        Some(entries)
                    }
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_rom() {
        let Some(rom) = nfsgba_testkit::rom() else { return };
        let d = GameData::parse(&rom);
        assert_eq!(d.camera_probe, [0, 0, 72]);
        assert_eq!(d.views[2].function, CameraFn::Update);
        assert_eq!((d.views[2].height, d.views[2].distance), (-150 * 256, -300));
        // The typed floor lookup takes a sector's first wall: no sector is empty.
        assert!(d.city.iter().all(|s| !s.walls.is_empty()));
    }
}
