//! Fly-through viewer for Carbon's city (portal/sector world) and vehicle model bank, read from the user's ROM:
//! textured walls, floors and ceilings, the 12 ROM skies, and a showroom of all 15 cars in every paint variant.
//! Run from the repo root: `cargo run --release -p nfsgba-viewer`.
//!
//! Controls (Bevy `FreeCamera`): hold right mouse to look (M toggles), WASD move, Q/E down/up, Shift run,
//! scroll wheel changes speed; K switches sky; R moves to the next race route (grid and chase camera).
//! `NFSGBA_ROUTE=<n>` starts behind route n's grid; `NFSGBA_CAM=x,y,z,tx,ty,tz` sets the start eye and target
//! (metres); `NFSGBA_SHOT=<file.png>` saves one frame and quits, for checking renders without anyone at the screen.

use std::collections::BTreeMap;

use bevy::{
    asset::RenderAssetUsages,
    camera_controller::free_camera::{FreeCamera, FreeCameraPlugin, FreeCameraState},
    image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor},
    prelude::*,
    render::{
        render_resource::{Extent3d, PrimitiveTopology, TextureDimension, TextureFormat},
        view::screenshot::{Screenshot, save_to_disk},
    },
};
use nfsgba_formats as rom;

/// Raw units to metres. The engine draws cars and city in one unit (vehicle matrices are pure rotations,
/// translations are city positions), and all 15 car models measure ~48 units per real-world metre, so the
/// city comes out exaggerated: streets ~40 m wide, blocks ~50 m tall.
const SCALE: f32 = 1.0 / 48.0;

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
                (world(x, y, z), uv)
            })
            .collect();
        t.fan(&pts, color);
    }
    t
}

/// 8bpp texture through the palette to RGBA, repeating and unfiltered like the original.
fn image(t: &rom::Texture, palette: &[[u8; 4]]) -> Image {
    let size = Extent3d {
        width: t.width as u32,
        height: t.height as u32,
        depth_or_array_layers: 1,
    };
    let rgba = t.pixels.iter().flat_map(|&i| palette[i as usize]).collect();
    let mut img = Image::new(
        size,
        TextureDimension::D2,
        rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
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
        .insert_resource(GlobalAmbientLight {
            brightness: 500.0,
            ..default()
        })
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "NFS Carbon GBA city viewer (unofficial)".into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(FreeCameraPlugin)
        .add_systems(Startup, setup)
        .add_systems(Update, (shot, sky, race))
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

    // City geometry grouped by material, one mesh and one textured material each. Material 0 is "not drawn"
    // (the scene code skips floor/ceiling passes for it; assumed the same for the 91 walls that use it).
    let mut by_material: BTreeMap<u16, Tris> = BTreeMap::new();
    let floor_uv = |w: &rom::Wall| Vec2::new(w.floor_uv[0] as f32, w.floor_uv[1] as f32) / 16384.0;
    for sector in rom::city(&data) {
        let w = &sector.walls;
        for (material, height) in [(sector.floor, 1), (sector.ceiling, 0)] {
            if material != 0 {
                let pts: Vec<(Vec3, Vec2)> = w
                    .iter()
                    .map(|w| {
                        (
                            world(w.x as f32, if height == 1 { w.bottom[0] } else { w.top[0] }, w.z as f32),
                            floor_uv(w),
                        )
                    })
                    .collect();
                by_material.entry(material).or_default().fan(&pts, Color::WHITE);
            }
        }
        for (k, a) in w.iter().enumerate().filter(|(_, a)| a.link < 0 && a.material != 0) {
            let b = &w[(k + 1) % w.len()];
            let (ax, az, bx, bz) = (a.x as f32, a.z as f32, b.x as f32, b.z as f32);
            let uv = a.uv(textures[a.material as usize].width).map(Vec2::from);
            let quad = [
                (world(ax, a.top[0], az), uv[0]),
                (world(bx, a.top[1], bz), uv[1]),
                (world(bx, a.bottom[1], bz), uv[2]),
                (world(ax, a.bottom[0], az), uv[3]),
            ];
            by_material.entry(a.material).or_default().fan(&quad, Color::WHITE);
        }
    }
    let (min, max) = by_material
        .values()
        .flat_map(|t| &t.pos)
        .fold((Vec3::MAX, Vec3::MIN), |(lo, hi), p| {
            (lo.min(Vec3::from(*p)), hi.max(Vec3::from(*p)))
        });
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
    let showroom = Vec3::new(center.x - 12.0, 0.0, max.z + 80.0);
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
            commands.spawn((
                Mesh3d(meshes.add(tris.mesh())),
                MeshMaterial3d(material),
                Transform::from_translation(at),
            ));
        }
        info!("showroom row {c}: {} ({} paint variants)", car.name, car.paint_variants);
    }
    info!("showroom at {showroom:.1} m (rows of cars along +z, paint variants along +x)");
    // Everything else in the bank (lower-detail car models, spoilers, traffic, markers) untextured, 12 per row.
    let flat = materials.add(StandardMaterial {
        double_sided: true,
        cull_mode: None,
        perceptual_roughness: 1.0,
        ..default()
    });
    let others = showroom + Vec3::new(30.0, 0.0, 0.0);
    for (n, (i, m)) in models
        .iter()
        .enumerate()
        .filter(|(i, _)| !car_models.contains(i))
        .enumerate()
    {
        let tris = model_tris(m, None, Color::hsl((i as f32 * 57.0) % 360.0, 0.6, 0.5));
        let at = others + Vec3::new((n % 12) as f32 * 6.0, on_ground(&tris), (n / 12) as f32 * 7.0);
        commands.spawn((
            Mesh3d(meshes.add(tris.mesh())),
            MeshMaterial3d(flat.clone()),
            Transform::from_translation(at),
        ));
    }

    // Race: one route's racing line and four cars on its start grid (the player's Chevy Cobalt SS first, as in
    // the reference race). R cycles the 44 routes; NFSGBA_ROUTE picks the first one and starts behind the grid.
    let floors = rom::city(&data)
        .iter()
        .map(|s| {
            s.walls
                .iter()
                .map(|w| world(0.0_f32, w.bottom[0], 0.0_f32).y)
                .sum::<f32>()
                / s.walls.len().max(1) as f32
        })
        .collect();
    let start_route: Option<usize> = std::env::var("NFSGBA_ROUTE").ok().and_then(|s| s.parse().ok());
    let race = Race {
        routes: rom::routes(&data),
        floors,
        current: start_route.unwrap_or(23),
    };
    let cars = rom::cars(&data);
    // Paint: the reference race's red for the player, then blue, black and silver (BGR555 channel scale).
    let colours = [[30, 0, 1], [4, 10, 28], [3, 3, 4], [22, 22, 23]];
    for (slot, c) in [2, 3, 7, 11].into_iter().enumerate() {
        let atlas = &vehicle_textures[cars[c].first_material];
        let tris = model_tris(&models[cars[c].models[0]], Some(atlas), Color::WHITE);
        let lift = on_ground(&tris);
        let material = materials.add(StandardMaterial {
            base_color_texture: Some(images.add(image(atlas, &rom::car_palette(&data, colours[slot], 0)))),
            unlit: true,
            double_sided: true,
            cull_mode: None,
            ..default()
        });
        commands.spawn((
            Mesh3d(meshes.add(tris.mesh())),
            MeshMaterial3d(material),
            race.grid(slot, lift),
            GridSlot { slot, lift },
        ));
    }
    let chase = (std::env::var("NFSGBA_CAM").is_err() && start_route.is_some()).then(|| race.chase());
    commands.insert_resource(race);

    // Sky: the gradient on a far ring and the skyline panorama on a band at the horizon, both centred on the
    // camera every frame. K cycles through the 12 ROM skies (which sky belongs to which district is not decoded).
    let mut skies = Skies::default();
    for sky in rom::skies(&data) {
        let gradient = rom::Texture {
            width: 1,
            height: 64,
            pixels: (0..64).collect(),
        };
        let mut unlit = |image: Image, alpha_mode| StandardMaterial {
            base_color_texture: Some(images.add(image)),
            alpha_mode,
            unlit: true,
            double_sided: true,
            cull_mode: None,
            ..default()
        };
        skies
            .gradient
            .push(materials.add(unlit(image(&gradient, &sky.gradient), AlphaMode::Opaque)));
        skies
            .skyline
            .push(materials.add(unlit(image(&sky.skyline, &palette), AlphaMode::Mask(0.5))));
        let [r, g, b, _] = sky.gradient[0];
        skies.top.push(Color::srgb_u8(r, g, b));
    }
    // ponytail: the skyline repeats 4 times around the horizon and spans ~10° of height; take the real scroll
    // factor from the sky renderer.
    commands.spawn((
        Mesh3d(meshes.add(ring(16000.0, 0.0, 10000.0, 1.0).mesh())),
        MeshMaterial3d(skies.gradient[0].clone()),
        SkyRing::Gradient,
    ));
    commands.spawn((
        Mesh3d(meshes.add(ring(14000.0, 0.0, 2480.0, 4.0).mesh())),
        MeshMaterial3d(skies.skyline[0].clone()),
        SkyRing::Skyline,
    ));
    commands.insert_resource(ClearColor(skies.top[0]));
    commands.insert_resource(skies);

    commands.spawn((
        DirectionalLight {
            illuminance: 6000.0,
            ..default()
        },
        Transform::default().looking_to(Vec3::new(-0.4, -1.0, -0.3), Vec3::Y),
    ));
    let (eye, target) = std::env::var("NFSGBA_CAM")
        .ok()
        .and_then(|s| {
            let v: Vec<f32> = s.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            (v.len() == 6).then(|| (Vec3::new(v[0], v[1], v[2]), Vec3::new(v[3], v[4], v[5])))
        })
        .unwrap_or((Vec3::new(center.x, max.y + 480.0, max.z + 640.0), center));
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            far: 24000.0,
            ..default()
        }),
        FreeCamera {
            walk_speed: 120.0,
            run_speed: 600.0,
            ..default()
        },
        chase.unwrap_or_else(|| Transform::from_translation(eye).looking_at(target, Vec3::Y)),
    ));
    info!(
        "city bounds {min:.0} .. {max:.0} m; controls: right mouse look, WASD/QE move, Shift run, wheel speed, K sky"
    );
}

#[derive(Resource)]
struct Race {
    routes: Vec<rom::Route>,
    /// Mean floor height (m) of every sector.
    floors: Vec<f32>,
    current: usize,
}

#[derive(Component)]
struct GridSlot {
    slot: usize,
    /// Raises the model so its lowest point sits on the floor.
    lift: f32,
}

impl Race {
    fn route(&self) -> &rom::Route {
        &self.routes[self.current]
    }

    fn at(&self, w: &rom::Waypoint) -> Vec3 {
        world(w.x as f32, 0.0_f32, w.z as f32).with_y(self.floors[w.sector])
    }

    /// Driving direction at the start: first waypoint towards the second, on the ground plane.
    fn heading(&self) -> Vec3 {
        match &self.route().waypoints[..] {
            [a, b, ..] => ((self.at(b) - self.at(a)) * Vec3::new(1.0, 0.0, 1.0)).normalize_or(Vec3::X),
            _ => Vec3::X,
        }
    }

    /// A car on grid slot `slot`, standing on the start sector's floor and facing along the route.
    fn grid(&self, slot: usize, lift: f32) -> Transform {
        let [x, _, z] = self.route().grid[slot];
        let floor = self.route().waypoints.first().map_or(0.0, |w| self.floors[w.sector]);
        let at = world(x as f32, 0.0_f32, z as f32).with_y(floor + lift);
        Transform::from_translation(at).looking_to(self.heading(), Vec3::Y)
    }

    /// The reference race's chase camera: 343 units behind the player's car and 134 above it.
    fn chase(&self) -> Transform {
        let car = self.grid(0, 0.0).translation;
        let eye = car - self.heading() * 343.0 * SCALE + Vec3::Y * 134.0 * SCALE;
        Transform::from_translation(eye).looking_at(car + self.heading() * 20.0, Vec3::Y)
    }
}

/// Draw the current racing line; R moves to the next route (cars onto its grid, camera behind them).
fn race(
    keys: Res<ButtonInput<KeyCode>>,
    mut race: ResMut<Race>,
    mut cars: Query<(&GridSlot, &mut Transform), Without<Camera3d>>,
    camera: Single<(&mut Transform, &mut FreeCameraState), With<Camera3d>>,
    mut gizmos: Gizmos,
) {
    if keys.just_pressed(KeyCode::KeyR) {
        race.current = (race.current + 1) % race.routes.len();
        for (g, mut t) in &mut cars {
            *t = race.grid(g.slot, g.lift);
        }
        let (mut t, mut state) = camera.into_inner();
        *t = race.chase();
        (state.yaw, state.pitch, _) = t.rotation.to_euler(EulerRot::YXZ);
        state.velocity = Vec3::ZERO;
        let r = race.route();
        let length = r.waypoints.last().map_or(0, |w| w.distance);
        info!(
            "route {}: {} waypoints, {length} units",
            race.current,
            r.waypoints.len()
        );
    }
    let line: Vec<Vec3> = race.route().waypoints.iter().map(|w| race.at(w) + Vec3::Y).collect();
    gizmos.linestrip(line, Color::srgb(1.0, 0.2, 0.1));
}

#[derive(Resource, Default)]
struct Skies {
    gradient: Vec<Handle<StandardMaterial>>,
    skyline: Vec<Handle<StandardMaterial>>,
    top: Vec<Color>,
    current: usize,
}

#[derive(Component)]
enum SkyRing {
    Gradient,
    Skyline,
}

/// Open cylinder around the origin from `y0` to `y1`, texture v from 1 (bottom) to 0 (top), u repeating `repeats` times.
fn ring(radius: f32, y0: f32, y1: f32, repeats: f32) -> Tris {
    const SEGMENTS: usize = 64;
    let mut t = Tris::default();
    for k in 0..SEGMENTS {
        let (a0, a1) = (k as f32 / SEGMENTS as f32, (k + 1) as f32 / SEGMENTS as f32);
        let at = |a: f32, y: f32| {
            Vec3::new(
                radius * (a * std::f32::consts::TAU).cos(),
                y,
                radius * (a * std::f32::consts::TAU).sin(),
            )
        };
        let quad = [
            (at(a0, y1), Vec2::new(a0 * repeats, 0.0)),
            (at(a1, y1), Vec2::new(a1 * repeats, 0.0)),
            (at(a1, y0), Vec2::new(a1 * repeats, 1.0)),
            (at(a0, y0), Vec2::new(a0 * repeats, 1.0)),
        ];
        t.fan(&quad, Color::WHITE);
    }
    t
}

/// Keep the sky centred on the camera; K switches to the next sky.
fn sky(
    keys: Res<ButtonInput<KeyCode>>,
    mut skies: ResMut<Skies>,
    mut clear: ResMut<ClearColor>,
    camera: Single<&Transform, (With<Camera3d>, Without<SkyRing>)>,
    mut rings: Query<(&mut Transform, &mut MeshMaterial3d<StandardMaterial>, &SkyRing)>,
) {
    let switch = keys.just_pressed(KeyCode::KeyK) && !skies.top.is_empty();
    if switch {
        skies.current = (skies.current + 1) % skies.top.len();
        clear.0 = skies.top[skies.current];
        info!("sky {}", skies.current);
    }
    for (mut transform, mut material, ring) in &mut rings {
        transform.translation = camera.translation;
        if switch {
            material.0 = match ring {
                SkyRing::Gradient => skies.gradient[skies.current].clone(),
                SkyRing::Skyline => skies.skyline[skies.current].clone(),
            };
        }
    }
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
