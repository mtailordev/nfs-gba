//! Fly-through viewer for Carbon's city (portal/sector world) and vehicle model bank, read from the user's ROM:
//! textured walls, floors and ceilings, the 12 ROM skies, and a showroom of all 15 cars in every paint variant.
//! Run from the repo root: `cargo run --release -p nfsgba-viewer`.
//!
//! Colour works like the GBA's: textures are palette indices, drawn through one 256-colour palette that the game's
//! light tint rewrites every frame (`docs/engine/viewer-rendering.md`).
//!
//! Controls (Bevy `FreeCamera`): hold right mouse to look (M toggles), WASD move, Q/E down/up, Shift run,
//! scroll wheel changes speed; K switches environment (palette and sky); R moves to the next race route (grid and
//! chase camera; the light then follows the player's car). `NFSGBA_ROUTE=<n>` starts behind route n's grid;
//! `NFSGBA_ENV=<n>` picks the environment; `NFSGBA_CAM=x,y,z,tx,ty,tz` sets the start eye and target (metres);
//! `NFSGBA_SHOT=<file.png>` saves one frame and quits, for checking renders without anyone at the screen.

use std::collections::BTreeMap;

use bevy::{
    asset::{RenderAssetUsages, embedded_asset},
    camera_controller::free_camera::{FreeCamera, FreeCameraPlugin, FreeCameraState},
    core_pipeline::tonemapping::{DebandDither, Tonemapping},
    image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor},
    mesh::MeshVertexBufferLayoutRef,
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    reflect::TypePath,
    render::{
        render_resource::{
            AsBindGroup, Extent3d, PrimitiveTopology, RenderPipelineDescriptor, SpecializedMeshPipelineError,
            TextureDimension, TextureFormat,
        },
        view::screenshot::{Screenshot, save_to_disk},
    },
    shader::ShaderRef,
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
    /// Fan-triangulate a convex polygon (sector floors, wall quads, model faces) in one colour.
    fn fan(&mut self, pts: &[(Vec3, Vec2)], color: Color) {
        let c = color.to_linear().to_f32_array();
        for k in 1..pts.len().saturating_sub(1) {
            for (p, uv) in [pts[0], pts[k], pts[k + 1]] {
                self.pos.push(p.into());
                self.uv.push(uv.into());
                self.col.push(c);
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

/// GBA-style indexed colour (`indexed.wgsl`): a texture of 8-bit palette indices drawn through a 256-colour
/// palette texture, texel by texel with wrapped integer coordinates. Index 0 is not drawn.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
struct Indexed {
    #[texture(0, sample_type = "u_int")]
    indices: Handle<Image>,
    #[texture(1)]
    palette: Handle<Image>,
}

impl Material for Indexed {
    fn fragment_shader() -> ShaderRef {
        "embedded://nfsgba_viewer/indexed.wgsl".into()
    }

    /// Both faces: the winding of the original data is not normalised.
    fn specialize(
        _: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _: &MeshVertexBufferLayoutRef,
        _: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

fn texture_2d(width: usize, height: usize, data: Vec<u8>, format: TextureFormat) -> Image {
    let size = Extent3d {
        width: width as u32,
        height: height as u32,
        depth_or_array_layers: 1,
    };
    Image::new(size, TextureDimension::D2, data, format, RenderAssetUsages::default())
}

/// Palette indices as an `R8Uint` texture (read with `textureLoad`, so no sampler applies).
fn index_image(t: &rom::Texture) -> Image {
    texture_2d(t.width, t.height, t.pixels.clone(), TextureFormat::R8Uint)
}

/// A 256×1 palette texture; `Tint` rewrites its contents in place.
fn palette_image() -> Image {
    texture_2d(256, 1, vec![0; 256 * 4], TextureFormat::Rgba8UnormSrgb)
}

/// A car atlas as palette indices: the game loads atlas pixel `i` into palette slot `192 + (i ^ 16)`
/// (`docs/formats/vehicle-models.md`).
fn car_indices(atlas: &rom::Texture) -> Image {
    index_image(&rom::Texture {
        pixels: atlas.pixels.iter().map(|&i| (i ^ 16).wrapping_add(192)).collect(),
        ..atlas.clone()
    })
}

/// Palette slots 192..=223 (raw BGR555, slot order) from a 32-colour car palette indexed by atlas pixel, such as
/// `rom::car_palette` or a paint preset. The RGBA8 channels are `c << 3 | c >> 2`, so `>> 3` recovers `c`.
fn car_slots(by_pixel: &[[u8; 4]]) -> [u16; 32] {
    std::array::from_fn(|j| {
        let [r, g, b, _] = by_pixel[j ^ 16].map(|c| u16::from(c >> 3));
        r | g << 5 | b << 10
    })
}

/// RGBA texture for the sky gradient (not a palette lookup: the game writes those colours per scanline).
fn rgba_image(t: &rom::Texture, palette: &[[u8; 4]]) -> Image {
    let rgba = t.pixels.iter().flat_map(|&i| palette[i as usize]).collect();
    let mut img = texture_2d(t.width, t.height, rgba, TextureFormat::Rgba8UnormSrgb);
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::nearest()
    });
    img
}

fn main() {
    let mut app = App::new();
    app.insert_resource(ClearColor(Color::srgb(0.45, 0.62, 0.85)))
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
        .add_plugins((FreeCameraPlugin, MaterialPlugin::<Indexed>::default()))
        .add_systems(Startup, setup)
        .add_systems(Update, (shot, sky, race, tint.after(sky).after(race)));
    embedded_asset!(app, "indexed.wgsl");
    app.run();
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut indexed: ResMut<Assets<Indexed>>,
    mut images: ResMut<Assets<Image>>,
) {
    let data = rom::canonical_rom().expect("no ROM vault found: run `python tools/vault.py` first (see README)");
    let textures = rom::city_textures(&data);
    let sectors = rom::city(&data);
    let envs = rom::environments(&data);
    // Environment 11 is the reference race's (`0x0300006C`); NFSGBA_ENV picks another. K cycles them.
    let env = std::env::var("NFSGBA_ENV")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(11)
        % envs.len();
    // One palette for the whole city (and the skyline), as on the GBA; `tint` fills it every frame.
    let city_palette = images.add(palette_image());

    // City geometry grouped by material, one mesh each. Material 0 is "not drawn" (the scene code skips
    // floor/ceiling passes for it; assumed the same for the 91 walls that use it).
    let mut by_material: BTreeMap<u16, Tris> = BTreeMap::new();
    let floor_uv = |w: &rom::Wall| Vec2::new(w.floor_uv[0] as f32, w.floor_uv[1] as f32) / 16384.0;
    for sector in &sectors {
        let w = &sector.walls;
        for (material, height) in [(sector.floor, 1), (sector.ceiling, 0)] {
            if material != 0 {
                let pts: Vec<_> = w
                    .iter()
                    .map(|w| {
                        let y = if height == 1 { w.bottom[0] } else { w.top[0] };
                        (world(w.x as f32, y, w.z as f32), floor_uv(w))
                    })
                    .collect();
                by_material.entry(material).or_default().fan(&pts, Color::WHITE);
            }
        }
        for (k, a) in w.iter().enumerate().filter(|(_, a)| a.link < 0 && a.material != 0) {
            let b = &w[(k + 1) % w.len()];
            let (ax, az, bx, bz) = (a.x as f32, a.z as f32, b.x as f32, b.z as f32);
            let t = &textures[a.material as usize];
            let uv = a.uv(t.width, t.height).map(Vec2::from);
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
        let material = indexed.add(Indexed {
            indices: images.add(index_image(&textures[m as usize])),
            palette: city_palette.clone(),
        });
        commands.spawn((Mesh3d(meshes.add(tris.mesh())), MeshMaterial3d(material)));
    }

    // Every car has its own palette: the city's with slots 192..=223 holding its colours, tinted alike.
    // NOT 1:1 (R3): the game has one palette, so other cars use other slots (160..=191 in the race dump).
    let mut spawn_car = |commands: &mut Commands, tris: Tris, atlas: &rom::Texture, slots, transform| {
        let palette = images.add(palette_image());
        let material = indexed.add(Indexed {
            indices: images.add(car_indices(atlas)),
            palette: palette.clone(),
        });
        commands
            .spawn((
                Mesh3d(meshes.add(tris.mesh())),
                MeshMaterial3d(material),
                transform,
                CarPalette { slots, image: palette },
            ))
            .id()
    };

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
            let slots = car_slots(&paints[(3 * c + v) % 19]);
            spawn_car(&mut commands, tris, atlas, slots, Transform::from_translation(at));
        }
        info!("showroom row {c}: {} ({} paint variants)", car.name, car.paint_variants);
    }
    info!("showroom at {showroom:.1} m (rows of cars along +z, paint variants along +x)");

    // Race: one route's racing line and four cars on its start grid (the player's Chevy Cobalt SS first, as in
    // the reference race). R cycles the 44 routes; NFSGBA_ROUTE picks the first one and starts behind the grid.
    let floors = sectors
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
        active: start_route.is_some(),
    };
    let cars = rom::cars(&data);
    // Paint: the reference race's red for the player, then blue, black and silver (BGR555 channel scale).
    let colours = [[30, 0, 1], [4, 10, 28], [3, 3, 4], [22, 22, 23]];
    for (slot, c) in [2, 3, 7, 11].into_iter().enumerate() {
        let atlas = &vehicle_textures[cars[c].first_material];
        let tris = model_tris(&models[cars[c].models[0]], Some(atlas), Color::WHITE);
        let lift = on_ground(&tris);
        let slots = car_slots(&rom::car_palette(&data, colours[slot], 0));
        let car = spawn_car(&mut commands, tris, atlas, slots, race.grid(slot, lift));
        commands.entity(car).insert(GridSlot { slot, lift });
    }

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
    let chase = (std::env::var("NFSGBA_CAM").is_err() && start_route.is_some()).then(|| race.chase());
    commands.insert_resource(race);

    // Sky: the gradient on a far ring and the skyline panorama on a band at the horizon, both centred on the
    // camera every frame. Each environment has its own sky and palette; K cycles them.
    let mut skies = Environments {
        current: env,
        ..default()
    };
    for e in &envs {
        let gradient = rom::Texture {
            width: 1,
            height: 64,
            pixels: (0..64).collect(),
        };
        skies.gradient.push(materials.add(StandardMaterial {
            base_color_texture: Some(images.add(rgba_image(&gradient, &e.sky.gradient))),
            unlit: true,
            double_sided: true,
            cull_mode: None,
            ..default()
        }));
        // The skyline is 8bpp through the city palette (colour 0 = sky), so it shares the city's palette.
        skies.skyline.push(indexed.add(Indexed {
            indices: images.add(index_image(&e.sky.skyline)),
            palette: city_palette.clone(),
        }));
        let [r, g, b, _] = e.sky.gradient[0];
        skies.top.push(Color::srgb_u8(r, g, b));
        skies.palettes.push(rom::city_palette_raw(&data, e.palette));
    }
    // ponytail: the skyline repeats 4 times around the horizon and spans ~10° of height; take the real scroll
    // factor from the sky renderer.
    commands.spawn((
        Mesh3d(meshes.add(ring(16000.0, 0.0, 10000.0, 1.0).mesh())),
        MeshMaterial3d(skies.gradient[env].clone()),
        SkyRing,
    ));
    commands.spawn((
        Mesh3d(meshes.add(ring(14000.0, 0.0, 2480.0, 4.0).mesh())),
        MeshMaterial3d(skies.skyline[env].clone()),
        SkyRing,
    ));
    commands.insert_resource(ClearColor(skies.top[env]));
    commands.insert_resource(Tint {
        raw: skies.palettes[env].clone(),
        rom: data,
        sectors,
        m: None,
        sector: None,
        city: city_palette,
        dirty: true,
    });
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
        // Palette colours reach the screen unchanged: no tonemapping, dithering or edge blending.
        Tonemapping::None,
        DebandDither::Disabled,
        Msaa::Off,
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
    /// Race mode (NFSGBA_ROUTE or R): the light follows the player's car instead of the camera.
    active: bool,
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
        race.active = true;
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

/// A car's palette: the city palette with slots 192..=223 replaced by `slots` (raw BGR555, slot order), tinted
/// like the rest. Whatever decides a car's paint writes `slots`; `tint` rebuilds the palette.
#[derive(Component)]
struct CarPalette {
    slots: [u16; 32],
    image: Handle<Image>,
}

/// The in-race light tint (`FUN_0813a514`): every frame the whole palette is tinted by the light interpolated at
/// the observer's position in its sector (`rom::sector_light`, `rom::tint_palette`); when no light is found the
/// palette stays as it was. The observer is the camera, or the player's car in race mode.
#[derive(Resource)]
struct Tint {
    rom: Vec<u8>,
    sectors: Vec<rom::Sector>,
    /// The current environment's city palette, untinted.
    raw: Vec<u16>,
    /// Current multipliers; `None` until a sector first gives a light (the palette is then as loaded).
    m: Option<[i32; 3]>,
    sector: Option<usize>,
    /// The shared city (and skyline) palette.
    city: Handle<Image>,
    /// The raw palette changed (K): rebuild every palette with the current multipliers.
    dirty: bool,
}

impl Tint {
    /// RGBA8 bytes of the tinted palette, with a car's colours in slots 192..=223 when given.
    fn palette(&self, car: Option<&[u16; 32]>) -> Vec<u8> {
        let mut raw = self.raw.clone();
        if let Some(slots) = car {
            raw[192..224].copy_from_slice(slots);
        }
        let tinted = match self.m {
            Some(m) => rom::tint_palette(&raw, m),
            None => raw,
        };
        rom::palette_rgba(&tinted).as_flattened().to_vec()
    }
}

/// Whether the sector's outline (x, z) contains the point: even-odd rule, exact in integers.
fn contains(s: &rom::Sector, px: i32, pz: i32) -> bool {
    let w = &s.walls;
    let mut inside = false;
    for (k, a) in w.iter().enumerate() {
        let b = &w[(k + 1) % w.len()];
        if (a.z > pz) != (b.z > pz) {
            let (dx, dz) = (i64::from(b.x) - i64::from(a.x), i64::from(b.z) - i64::from(a.z));
            let left = (i64::from(px) - i64::from(a.x)) * dz < dx * (i64::from(pz) - i64::from(a.z));
            if left == (dz > 0) {
                inside = !inside;
            }
        }
    }
    inside
}

/// Find the observer's sector, re-tint when the light changes, and rewrite the palette textures.
fn tint(
    mut tint: ResMut<Tint>,
    race: Res<Race>,
    camera: Single<&Transform, With<Camera3d>>,
    cars: Query<Ref<CarPalette>>,
    mut images: ResMut<Assets<Image>>,
) {
    // City units, as the game's `position >> 8`.
    let (px, pz, y) = if race.active {
        let [x, _, z] = race.route().grid[0];
        (x, z, race.grid(0, 0.0).translation.y)
    } else {
        let t = camera.translation;
        ((t.x / SCALE).floor() as i32, (-t.z / SCALE).floor() as i32, t.y)
    };
    // NOT 1:1: the game follows the player's sector through the portals it crosses (0x03005614); we look it
    // up by position, keeping the current sector while it still contains the point, else taking the containing
    // sector whose floor is nearest the observer's height (sectors can overlap on different levels).
    let sector = tint.sector.filter(|&s| contains(&tint.sectors[s], px, pz)).or_else(|| {
        let near = |s: &usize| (y - race.floors[*s]).abs();
        (0..tint.sectors.len())
            .filter(|&s| contains(&tint.sectors[s], px, pz))
            .min_by(|a, b| near(a).total_cmp(&near(b)))
    });
    tint.sector = sector;
    let m = sector
        .and_then(|s| rom::sector_light(&tint.rom, &tint.sectors[s], px, pz))
        .or(tint.m);
    let changed = tint.dirty || m != tint.m;
    if changed {
        debug!("observer ({px}, {pz}) in sector {sector:?}: palette multipliers {m:?}");
        tint.m = m;
        tint.dirty = false;
        if let Some(mut image) = images.get_mut(&tint.city) {
            image.data = Some(tint.palette(None));
        }
    }
    for car in &cars {
        if (changed || car.is_changed())
            && let Some(mut image) = images.get_mut(&car.image)
        {
            image.data = Some(tint.palette(Some(&car.slots)));
        }
    }
}

/// The 12 environments: sky materials and untinted city palettes.
#[derive(Resource, Default)]
struct Environments {
    gradient: Vec<Handle<StandardMaterial>>,
    skyline: Vec<Handle<Indexed>>,
    top: Vec<Color>,
    palettes: Vec<Vec<u16>>,
    current: usize,
}

#[derive(Component)]
struct SkyRing;

/// A sky ring's transform and material: the gradient is RGBA, the skyline indexed.
type SkyRingParts = (
    &'static mut Transform,
    Option<&'static mut MeshMaterial3d<StandardMaterial>>,
    Option<&'static mut MeshMaterial3d<Indexed>>,
);

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

/// Keep the sky centred on the camera; K switches to the next environment (sky and palette).
fn sky(
    keys: Res<ButtonInput<KeyCode>>,
    mut skies: ResMut<Environments>,
    mut tint: ResMut<Tint>,
    mut clear: ResMut<ClearColor>,
    camera: Single<&Transform, (With<Camera3d>, Without<SkyRing>)>,
    mut rings: Query<SkyRingParts, With<SkyRing>>,
) {
    let switch = keys.just_pressed(KeyCode::KeyK) && !skies.top.is_empty();
    if switch {
        skies.current = (skies.current + 1) % skies.top.len();
        clear.0 = skies.top[skies.current];
        tint.raw = skies.palettes[skies.current].clone();
        tint.dirty = true;
        info!("environment {}", skies.current);
    }
    for (mut transform, gradient, skyline) in &mut rings {
        transform.translation = camera.translation;
        if switch {
            if let Some(mut m) = gradient {
                m.0 = skies.gradient[skies.current].clone();
            }
            if let Some(mut m) = skyline {
                m.0 = skies.skyline[skies.current].clone();
            }
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
