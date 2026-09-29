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

#[derive(Debug, Clone)]
pub struct GameData {
    /// The city: sectors with their walls, by sector index.
    pub city: Vec<Sector>,
    pub views: [CameraView; 8],
    pub camera_probe: [i32; 3],
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
