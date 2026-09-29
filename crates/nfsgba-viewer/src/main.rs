//! Fly-through viewer for Carbon's city (portal/sector world) and vehicle model bank, read from the user's ROM.
//! City walls and floors are textured with the ROM's own textures; vehicles are still flat-coloured.
//! Run from the repo root: `cargo run --release -p nfsgba-viewer`.
//!
//! Controls (Bevy `FreeCamera`): hold right mouse to look (M toggles), WASD move, Q/E down/up, Shift run,
//! scroll wheel changes speed.
//! `NFSGBA_CAM=x,y,z,tx,ty,tz` sets the start eye and target (metres); `NFSGBA_SHOT=<file.png>` saves one frame
//! and quits, for checking renders without anyone at the screen.

use std::collections::BTreeMap;

use bevy::{
    asset::RenderAssetUsages,
    camera_controller::free_camera::{FreeCamera, FreeCameraPlugin},
    image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor},
    prelude::*,
    render::{
        render_resource::{Extent3d, PrimitiveTopology, TextureDimension, TextureFormat},
        view::screenshot::{Screenshot, save_to_disk},
    },
};
use nfsgba_formats as rom;

/// Raw city units to metres. All 15 car models measure ~48 model units per real-world metre on every axis;
/// with the vehicle factor below that makes the city 192 units per metre (streets ~10 m, facades ~13 m).
const SCALE: f32 = 1.0 / 192.0;
// ponytail: ×4 (a 2-bit shift) is the most plausible engine factor for model units; confirm in the matrix
// setup that fills world +0xFC (FUN_0814e8b4 and friends).
const CAR_SCALE: f32 = 4.0;

/// Raw space (x right, y down, z forward) to Bevy (y up, -z forward): a 180° turn about x, no mirroring.
fn world(x: impl Into<f32>, y: impl Into<f32>, z: impl Into<f32>) -> Vec3 {
    Vec3::new(x.into(), -y.into(), -z.into()) * SCALE
}

/// Unindexed triangle soup; flat normals are computed at the end.
#[derive(Default)]
struct Tris {
    pos: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    col: Vec<[f32; 4]>,
}

impl Tris {
    /// Fan-triangulate a convex polygon (sector floors, wall quads, model faces).
    fn fan(&mut self, pts: &[(Vec3, Vec2)], color: Color) {
        let c = color.to_linear();
        for k in 1..pts.len().saturating_sub(1) {
            for (p, uv) in [pts[0], pts[k], pts[k + 1]] {
                self.pos.push(p.into());
                self.uv.push(uv.into());
                self.col.push([c.red, c.green, c.blue, 1.0]);
            }
        }
    }

    fn mesh(self) -> Mesh {
        let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uv)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.col);
        m.compute_flat_normals();
        m
    }
}

/// Model polygons fan-triangulated, in metres, with atlas UVs when the model is textured.
fn model_tris(m: &rom::Model, atlas: Option<&rom::Texture>, color: Color) -> Tris {
    let mut t = Tris::default();
    for p in &m.polys {
        let pts: Vec<(Vec3, Vec2)> = p
            .verts
            .iter()
            .zip(&p.uvs)
            .map(|(&k, &uv)| {
                let [x, y, z] = m.verts[k as usize];
                // UVs are 1.15 fixed point, 32768 = the whole texture (checked by overlaying them on the atlas).
                let uv = match atlas {
                    Some(_) if m.flags & 1 != 0 => Vec2::from(m.uvs[uv as usize].map(|c| c as f32 / 32768.0)),
                    _ => Vec2::ZERO,
                };
                (world(x, y, z) * CAR_SCALE, uv)
            })
            .collect();
        t.fan(&pts, color);
    }
    t
}

/// 8bpp texture through the palette to RGBA, repeating and unfiltered like the original.
fn image(t: &rom::Texture, palette: &[[u8; 4]]) -> Image {
    let size = Extent3d { width: t.width as u32, height: t.height as u32, depth_or_array_layers: 1 };
    let rgba = t.pixels.iter().flat_map(|&i| palette[i as usize]).collect();
    let mut img = Image::new(size, TextureDimension::D2, rgba, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default());
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::nearest()
    });
    img
}

fn main() {
    App::new()
        .insert_resource(ClearColor(Color::srgb(0.45, 0.62, 0.85)))
        .insert_resource(GlobalAmbientLight { brightness: 500.0, ..default() })
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window { title: "NFS Carbon GBA city viewer (unofficial)".into(), ..default() }),
            ..default()
        }))
        .add_plugins(FreeCameraPlugin)
        .add_systems(Startup, setup)
        .add_systems(Update, shot)
        .run();
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let data = rom::canonical_rom().expect("no ROM vault found: run `python tools/vault.py` first (see README)");
    let palette = rom::city_palette(&data);
    let textures = rom::city_textures(&data);

    // City geometry grouped by material, one mesh and one textured material each.
    let mut by_material: BTreeMap<u16, Tris> = BTreeMap::new();
    for sector in rom::city(&data) {
        let w = &sector.walls;
        let floor: Vec<(Vec3, Vec2)> = w
            .iter()
            .map(|w| (world(w.x as f32, w.bottom[0], w.z as f32), Vec2::new(w.floor_uv[0] as f32, w.floor_uv[1] as f32) / 16384.0))
            .collect();
        by_material.entry(sector.floor).or_default().fan(&floor, Color::WHITE);
        for (k, a) in w.iter().enumerate().filter(|(_, a)| a.link < 0) {
            let b = &w[(k + 1) % w.len()];
            let (ax, az, bx, bz) = (a.x as f32, a.z as f32, b.x as f32, b.z as f32);
            // ponytail: square-texel guess (texture height = wall height); decode the real wall u/v (+0x28, +0x40, +0x42).
            let t = &textures[a.material as usize];
            let height = (a.bottom[0] as f32 - a.top[0] as f32).abs().max(1.0);
            let u = Vec2::new(bx - ax, bz - az).length() / height * t.height as f32 / t.width as f32;
            let quad = [
                (world(ax, a.top[0], az), Vec2::new(0.0, 0.0)),
                (world(bx, a.top[1], bz), Vec2::new(u, 0.0)),
                (world(bx, a.bottom[1], bz), Vec2::new(u, 1.0)),
                (world(ax, a.bottom[0], az), Vec2::new(0.0, 1.0)),
            ];
            by_material.entry(a.material).or_default().fan(&quad, Color::WHITE);
        }
    }
    let (min, max) = by_material
        .values()
        .flat_map(|t| &t.pos)
        .fold((Vec3::MAX, Vec3::MIN), |(lo, hi), p| (lo.min(Vec3::from(*p)), hi.max(Vec3::from(*p))));
    let center = (min + max) / 2.0;
    for (m, tris) in by_material {
        let material = materials.add(StandardMaterial {
            base_color_texture: Some(images.add(image(&textures[m as usize], &palette))),
            alpha_mode: AlphaMode::Mask(0.5),
            unlit: true, // the GBA renderer has no lighting
            double_sided: true,
            cull_mode: None, // winding of the original data is not normalised yet
            ..default()
        });
        commands.spawn((Mesh3d(meshes.add(tris.mesh())), MeshMaterial3d(material)));
    }

    // Showroom in front of the city: one row per car, its paint variants side by side, textured with the
    // car's atlas and a paint preset (the race generates the real paint ramp at runtime).
    let models = rom::models(&data);
    let vehicle_textures = rom::vehicle_textures(&data);
    let paints = rom::paint_palettes(&data);
    let on_ground = |t: &Tris| -t.pos.iter().map(|p| p[1]).fold(f32::MAX, f32::min);
    let showroom = Vec3::new(center.x - 12.0, 0.0, max.z + 20.0);
    let mut car_models = std::collections::HashSet::new();
    for (c, car) in rom::cars(&data).iter().enumerate() {
        car_models.extend(car.models);
        for v in 0..car.paint_variants {
            let atlas = &vehicle_textures[car.first_material + v];
            let tris = model_tris(&models[car.models[0]], Some(atlas), Color::WHITE);
            let at = showroom + Vec3::new(v as f32 * 6.0, on_ground(&tris), c as f32 * 7.0);
            let material = materials.add(StandardMaterial {
                base_color_texture: Some(images.add(image(atlas, &paints[(3 * c + v) % 19]))),
                unlit: true,
                double_sided: true,
                cull_mode: None,
                ..default()
            });
            commands.spawn((Mesh3d(meshes.add(tris.mesh())), MeshMaterial3d(material), Transform::from_translation(at)));
        }
        info!("showroom row {c}: {} ({} paint variants)", car.name, car.paint_variants);
    }
    info!("showroom at {showroom:.1} m (rows of cars along +z, paint variants along +x)");
    // Everything else in the bank (lower-detail car models, spoilers, traffic, markers) untextured, 12 per row.
    let flat = materials.add(StandardMaterial { double_sided: true, cull_mode: None, perceptual_roughness: 1.0, ..default() });
    let others = showroom + Vec3::new(30.0, 0.0, 0.0);
    for (n, (i, m)) in models.iter().enumerate().filter(|(i, _)| !car_models.contains(i)).enumerate() {
        let tris = model_tris(m, None, Color::hsl((i as f32 * 57.0) % 360.0, 0.6, 0.5));
        let at = others + Vec3::new((n % 12) as f32 * 6.0, on_ground(&tris), (n / 12) as f32 * 7.0);
        commands.spawn((Mesh3d(meshes.add(tris.mesh())), MeshMaterial3d(flat.clone()), Transform::from_translation(at)));
    }

    commands.spawn((DirectionalLight { illuminance: 6000.0, ..default() }, Transform::default().looking_to(Vec3::new(-0.4, -1.0, -0.3), Vec3::Y)));
    let (eye, target) = std::env::var("NFSGBA_CAM")
        .ok()
        .and_then(|s| {
            let v: Vec<f32> = s.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            (v.len() == 6).then(|| (Vec3::new(v[0], v[1], v[2]), Vec3::new(v[3], v[4], v[5])))
        })
        .unwrap_or((Vec3::new(center.x, max.y + 120.0, max.z + 160.0), center));
    commands.spawn((
        Camera3d::default(),
        FreeCamera { walk_speed: 30.0, run_speed: 150.0, ..default() },
        Transform::from_translation(eye).looking_at(target, Vec3::Y),
    ));
    info!(
        "city bounds {min:.0} .. {max:.0} m; controls: right mouse look, WASD/QE move, Shift run, wheel speed"
    );
}

/// With `NFSGBA_SHOT=<file.png>`: save a frame after 8 s (shader pipelines compile in the background first) and quit.
fn shot(mut commands: Commands, time: Res<Time>, mut taken: Local<bool>, mut exit: MessageWriter<AppExit>) {
    let Ok(path) = std::env::var("NFSGBA_SHOT") else { return };
    let t = time.elapsed_secs();
    if t > 8.0 && !*taken {
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
        *taken = true;
    }
    if t > 10.0 {
        exit.write(AppExit::Success);
    }
}
