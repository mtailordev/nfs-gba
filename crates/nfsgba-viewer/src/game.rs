//! The race camera's projection and the renderer's runtime tables. The race itself (camera, racers, matrix slots)
//! is `nfsgba_game`'s: every viewer mode reads it through `nfsgba_game::{Game, view, race_init}`
//! (`docs/engine/viewer-rendering.md`).

use bevy::{
    camera::{CameraProjection, SubCameraView},
    math::Vec3A,
    prelude::*,
};
use nfsgba_formats::{self as rom, render};

/// The race projection's focal length (view struct `0x03000080 +0x1C`); `camera_update` eases it back to 150 while
/// the speed effect is off.
pub const FOCAL: i32 = 150;
/// The near plane (view `+0x10`), in city units.
pub const NEAR: i32 = 64;
/// The chase view.
pub const CHASE: usize = 2;
/// The renderer's runtime tables in a race as every captured race has them (20 dumps, routes 18 and 23): each
/// moving wall piece (world `+0x18`, one per wall naming one at `+0x2A`) with no offsets and flags 1, so those 122
/// walls are open (not drawn, not blocking the camera); no material animation or scroll.
/// NOT 1:1 (R21): the tables' writers are not decoded, so other races may set them otherwise.
pub fn race_runtime(sectors: &[rom::Sector]) -> render::Runtime {
    let pieces = sectors
        .iter()
        .flat_map(|s| &s.walls)
        .filter(|w| w.piece != 0xFFFF)
        .map(|w| w.piece as usize + 1)
        .max()
        .unwrap_or(0);
    render::Runtime {
        pieces: vec![
            render::Piece {
                flags: 1,
                ..Default::default()
            };
            pieces
        ],
        ..Default::default()
    }
}

/// The game's projection (`render::View`): `sx = 120 + focal·x/(d + 1)`, `sy = 79 + focal·y/(d + 1)` on the
/// 240×160 screen, which the window shows whole; depth is reversed and infinite as Bevy expects, and geometry
/// nearer than the near plane (64 units) is clipped.
/// NOT 1:1 (R11): the game divides through the reciprocal table in integers and rounds down.
#[derive(Debug, Clone)]
pub struct GbaProjection {
    pub focal: f32,
    /// Near plane, in metres.
    pub near: f32,
    /// One city unit in metres (the `+ 1` of `d + 1`).
    pub unit: f32,
}

impl GbaProjection {
    /// View-space (x right, y up, −z ahead) extent of the screen at distance `d`: left, right, bottom, top.
    fn extent(&self, d: f32) -> [f32; 4] {
        [-120.0, 120.0, -81.0, 79.0].map(|e| e * (d + self.unit) / self.focal)
    }
}

impl CameraProjection for GbaProjection {
    fn get_clip_from_view(&self) -> Mat4 {
        // w = d + 1 unit; NDC x = focal·x/(120·w); NDC y = focal·y/(80·w) + 1/80, which puts the optical axis on
        // the top edge of screen row 79 (the game's `cy`) and column 120. Reversed depth near/w with the constant
        // at `near + unit`, so points clip exactly where `d < near`.
        Mat4::from_cols(
            Vec4::new(self.focal / 120.0, 0.0, 0.0, 0.0),
            Vec4::new(0.0, self.focal / 80.0, 0.0, 0.0),
            Vec4::new(0.0, -1.0 / 80.0, 0.0, -1.0),
            Vec4::new(0.0, self.unit / 80.0, self.near + self.unit, self.unit),
        )
    }

    fn get_clip_from_view_for_sub(&self, _: &SubCameraView) -> Mat4 {
        self.get_clip_from_view()
    }

    fn update(&mut self, _: f32, _: f32) {}

    fn far(&self) -> f32 {
        1.0e6
    }

    fn get_frustum_corners(&self, z_near: f32, z_far: f32) -> [Vec3A; 8] {
        let corners = |z: f32| {
            let [l, r, b, t] = self.extent(z.abs());
            [
                Vec3A::new(r, b, z),
                Vec3A::new(r, t, z),
                Vec3A::new(l, t, z),
                Vec3A::new(l, b, z),
            ]
        };
        let (n, f) = (corners(z_near), corners(z_far));
        [n[0], n[1], n[2], n[3], f[0], f[1], f[2], f[3]]
    }
}

/// The eye and view direction of a render frame in the viewer's world (see `world` in `main.rs`): the eye is
/// minus the matrix translation, the view direction its depth row `(m2, m8)`.
pub fn frame_transform(frame: &render::Frame, world: impl Fn(f32, f32, f32) -> Vec3) -> Transform {
    let m = &frame.camera;
    let eye = world(-m[9] as f32, -m[10] as f32, -m[11] as f32);
    let ahead = world(m[2] as f32, 0.0, m[8] as f32).normalize();
    Transform::from_translation(eye).looking_to(ahead, Vec3::Y)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The projection puts view-space points where `render::View` puts them on the 240×160 screen.
    #[test]
    fn projection_matches_the_game() {
        // In city units (unit = 1): the game's own numbers.
        let p = GbaProjection {
            focal: 150.0,
            near: 64.0,
            unit: 1.0,
        };
        let m = p.get_clip_from_view();
        for (x, y, d) in [(0.0, 0.0, 400.0), (-323.0, 0.0, 64.0), (100.0, -50.0, 700.0)] {
            // Game camera space has y down; view space has y up and looks along −z.
            let clip = m * Vec4::new(x, -y, -d, 1.0);
            let (sx, sy) = ((clip.x / clip.w + 1.0) * 120.0, (1.0 - clip.y / clip.w) * 80.0);
            assert!((sx - (120.0 + 150.0 * x / (d + 1.0))).abs() < 1e-3, "{x} {y} {d}: {sx}");
            assert!((sy - (79.0 + 150.0 * y / (d + 1.0))).abs() < 1e-3, "{x} {y} {d}: {sy}");
            // The near plane: depth 1 exactly at d = 64.
            assert!(d != 64.0 || (clip.z / clip.w - 1.0).abs() < 1e-6);
        }
    }
}
