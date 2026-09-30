//! The last pass (G2, `composite.wgsl`): the 3D scene is rendered into an image of the window's size by the scene
//! camera, and a second, 2D camera draws one full-window quad that mixes that image with the HUD layer as the GBA
//! blends semi-transparent sprites (in 5-bit gamma space, which fixed-function blending on the sRGB target cannot do).
//! The UI (the menus, the banner) is drawn by the second camera, over the result.

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
    /// `x`: EVA, `y`: EVB (`BLDALPHA`, each at most 16).
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

pub fn plugin(app: &mut App) {
    app.add_plugins(Material2dPlugin::<Composite>::default())
        .add_systems(PostUpdate, resize_scene);
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
        blend: UVec4::new(16, 0, 0, 0),
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

/// The scene image follows the size of the target the composite camera draws to (a resized window).
fn resize_scene(
    fin: Option<Res<Final>>,
    cameras: Query<&Camera>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<Composite>>,
) {
    let Some(fin) = fin else { return };
    let Some(size) = cameras.get(fin.camera).ok().and_then(Camera::physical_target_size) else {
        return;
    };
    if size.x == 0 || size.y == 0 {
        return;
    }
    let Some(image) = images.get(&fin.scene) else { return };
    if image.size() == size {
        return;
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
