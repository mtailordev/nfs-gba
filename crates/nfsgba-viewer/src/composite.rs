//! The last pass (G2, `composite.wgsl`): the 3D scene is rendered into an image of the window's size by the scene
//! camera, and a second, 2D camera draws one full-window quad that mixes that image with the HUD layer as the GBA
//! blends semi-transparent sprites (in 5-bit gamma space, which fixed-function blending on the sRGB target cannot do).
//! The UI (the menus, the banner) is drawn by the second camera, over the result.
//!
//! The scene image can be smaller than the window ([`RenderScale`]): the quad then scales it up. With `auto` the
//! scale follows the display rate: down when frames come late for a while, back up slowly when they are on time, so
//! a weak GPU keeps the game at its own speed (the game steps on the wall clock either way, `play::play`).

use bevy::{
    camera::{RenderTarget, ScalingMode},
    core_pipeline::tonemapping::{DebandDither, Tonemapping},
    prelude::*,
    reflect::TypePath,
    render::render_resource::{AsBindGroup, TextureFormat},
    shader::ShaderRef,
    sprite_render::{Material2d, Material2dPlugin},
};

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct Composite {
    #[texture(0)]
    scene: Handle<Image>,
    #[texture(1)]
    hud: Handle<Image>,
    /// `x`: EVA, `y`: EVB (`BLDALPHA`, each at most 16); `z`: the scene image's size over the window's, in 1/4096.
    #[uniform(2)]
    pub blend: UVec4,
}

impl Material2d for Composite {
    fn fragment_shader() -> ShaderRef {
        "embedded://nfsgba_viewer/composite.wgsl".into()
    }
}

/// The composite pass's material and cameras.
#[derive(Resource)]
pub struct Final {
    pub material: Handle<Composite>,
    pub scene: Handle<Image>,
    /// The camera that draws the window (and the UI).
    pub camera: Entity,
}

/// The scene image's size as a share of the window's (each side), and whether it follows the display rate.
#[derive(Resource, Debug, Clone, Copy)]
pub struct RenderScale {
    pub scale: f32,
    pub auto: bool,
    /// The scene is on the screen (not under the menus): only then are display frames measured.
    pub measuring: bool,
    /// Seconds of display frames measured since the last change, and how many of them came late.
    window: f32,
    frames: u32,
    late: u32,
    /// No step up before this many seconds after a step down (a step up that comes late again would flicker).
    hold: f32,
}

impl RenderScale {
    pub const fn fixed(scale: f32) -> RenderScale {
        RenderScale {
            scale,
            auto: false,
            measuring: true,
            window: 0.0,
            frames: 0,
            late: 0,
            hold: 0.0,
        }
    }

    /// Adaptive, from the full window size.
    pub const fn auto() -> RenderScale {
        RenderScale {
            auto: true,
            ..RenderScale::fixed(1.0)
        }
    }
}

impl Default for RenderScale {
    fn default() -> Self {
        RenderScale::auto()
    }
}

/// The smallest scene: the GBA's own 240 lines' worth (below that the original-resolution frame is the better view).
const MIN_LINES: f32 = 240.0;
/// A display frame later than this is late (a 50 Hz display's frame and a little).
const LATE: f32 = 1.0 / 45.0;

pub fn plugin(app: &mut App) {
    app.add_plugins(Material2dPlugin::<Composite>::default())
        .init_resource::<RenderScale>()
        .add_systems(PostUpdate, (adapt_scale, resize_scene).chain());
}

impl RenderScale {
    /// One display frame of `dt` seconds on a target `height` pixels high. Every 2 s of display frames: more than a
    /// fifth late, the scale steps down (×0.8, not below the GBA's 240 lines' worth) and no step up comes for 30 s;
    /// none late and the hold over, up (×1.1, at most 1). A frame longer than 0.25 s (a stall: loading, a window drag) is not counted. Returns
    /// whether the scale changed.
    pub fn observe(&mut self, dt: f32, height: f32) -> bool {
        self.hold = (self.hold - dt).max(0.0);
        if !self.auto || !self.measuring || dt <= 0.0 || dt > 0.25 {
            return false;
        }
        self.window += dt;
        self.frames += 1;
        self.late += u32::from(dt > LATE);
        if self.window < 2.0 {
            return false;
        }
        let least = (MIN_LINES / height).min(1.0);
        let (late, frames, before) = (self.late, self.frames, self.scale);
        if late * 5 > frames {
            // Late: down a step (at the floor, still late: no step up for a while either).
            self.scale = (self.scale * 0.8).max(least);
            self.hold = 30.0;
        } else if late == 0 && self.hold == 0.0 && self.scale < 1.0 {
            self.scale = (self.scale * 1.1).min(1.0);
            self.hold = 10.0;
        }
        (self.window, self.frames, self.late) = (0.0, 0, 0);
        self.scale != before
    }
}

fn adapt_scale(time: Res<Time<Real>>, mut s: ResMut<RenderScale>, fin: Option<Res<Final>>, cameras: Query<&Camera>) {
    let height = fin
        .and_then(|f| cameras.get(f.camera).ok().and_then(Camera::physical_target_size))
        .map_or(640.0, |size| size.y as f32);
    if s.observe(time.delta_secs(), height) {
        info!("render scale {:.2}", s.scale);
    }
}

/// Spawns the quad and its camera (rendering to `target`: the window, or an image in the tests) and returns the image
/// the scene camera must render into.
pub fn spawn(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<Composite>,
    images: &mut Assets<Image>,
    hud: Handle<Image>,
    target: Option<Handle<Image>>,
) -> (Handle<Image>, Entity) {
    let scene = images.add(Image::new_target_texture(960, 640, TextureFormat::Rgba8UnormSrgb, None));
    let material = materials.add(Composite {
        scene: scene.clone(),
        hud,
        blend: UVec4::new(16, 0, 4096, 0),
    });
    let mut camera = commands.spawn((
        Camera2d,
        Camera { order: 1, ..default() },
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::Fixed {
                width: 2.0,
                height: 2.0,
            },
            ..OrthographicProjection::default_2d()
        }),
        Tonemapping::None,
        DebandDither::Disabled,
        Msaa::Off,
    ));
    if let Some(t) = target {
        camera.insert(RenderTarget::Image(t.into()));
    }
    let camera = camera.id();
    commands.spawn((
        Mesh2d(meshes.add(Rectangle::new(2.0, 2.0))),
        bevy::sprite_render::MeshMaterial2d(material.clone()),
    ));
    commands.insert_resource(Final {
        material,
        scene: scene.clone(),
        camera,
    });
    (scene, camera)
}

/// The scene image follows the size of the target the composite camera draws to (a resized window), times the
/// render scale.
fn resize_scene(
    fin: Option<Res<Final>>,
    scale: Res<RenderScale>,
    cameras: Query<&Camera>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<Composite>>,
) {
    let Some(fin) = fin else { return };
    let Some(window) = cameras.get(fin.camera).ok().and_then(Camera::physical_target_size) else {
        return;
    };
    if window.x == 0 || window.y == 0 {
        return;
    }
    let size = (window.as_vec2() * scale.scale).round().as_uvec2().max(UVec2::ONE);
    let Some(image) = images.get(&fin.scene) else { return };
    if image.size() == size {
        return;
    }
    let ratio = (size.y as f32 / window.y as f32 * 4096.0).round() as u32;
    if let Some(mut m) = materials.get_mut(&fin.material) {
        m.blend.z = ratio;
    }
    if let Some(mut image) = images.get_mut(&fin.scene) {
        image.resize(bevy::render::render_resource::Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        });
    }
    // A new GPU texture: the material's bind group is made again.
    let _ = materials.get_mut(&fin.material);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(s: &mut RenderScale, fps: f32, secs: f32) {
        for _ in 0..(fps * secs) as u32 {
            s.observe(1.0 / fps, 1080.0);
        }
    }

    /// A display at 30 fps steps the scale down every 2 s to the GBA's 240 lines; at 60 fps it comes back to 1 after
    /// the holds, and a steady 60 fps never changes it.
    #[test]
    fn the_scale_follows_the_display_rate() {
        let mut s = RenderScale::auto();
        run(&mut s, 60.0, 20.0);
        assert_eq!(s.scale, 1.0, "on time: full size");
        run(&mut s, 30.0, 4.5);
        assert!(
            s.scale < 1.0 && s.scale >= 0.64 - 1e-6,
            "late: a step or two down, {}",
            s.scale
        );
        run(&mut s, 30.0, 40.0);
        assert!((s.scale - 240.0 / 1080.0).abs() < 1e-6, "floor: {}", s.scale);
        run(&mut s, 60.0, 27.0);
        assert!((s.scale - 240.0 / 1080.0).abs() < 1e-6, "held after late frames");
        run(&mut s, 60.0, 200.0);
        assert_eq!(s.scale, 1.0, "back up");
        // Stalls (a frame over 0.25 s) are not counted.
        run(&mut s, 2.0, 10.0);
        assert_eq!(s.scale, 1.0);
        // Under the menus (the scene not drawn) nothing is measured.
        s.measuring = false;
        run(&mut s, 10.0, 30.0);
        assert_eq!(s.scale, 1.0, "not measured under the menus");
        let mut fixed = RenderScale::fixed(0.5);
        run(&mut fixed, 10.0, 10.0);
        assert_eq!(fixed.scale, 0.5);
    }
}
