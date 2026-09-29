//! Fly-through viewer for Carbon's city (portal/sector world) and vehicle model bank, read from the user's ROM:
//! textured walls, floors and ceilings, the race sky, the reference race's four cars on a route's grid, and a
//! showroom of all 15 cars in every paint variant. Run from the repo root: `cargo run --release -p nfsgba-viewer`.
//!
//! Colour works like the GBA's: textures are palette indices, drawn through one 256-colour palette that holds the
//! city and the race cars and that the game's light tint rewrites every frame; behind the world the window shows
//! the GBA's 240×160 sky layer (backdrop gradient per line, skyline panorama). See
//! `docs/engine/viewer-rendering.md`.
//!
//! Controls (Bevy `FreeCamera`): hold right mouse to look (M toggles), WASD move, Q/E down/up, Shift run,
//! scroll wheel changes speed; K switches environment (palette and sky); R moves to the next race route (grid and
//! chase camera; the light then follows the player's car). `NFSGBA_ROUTE=<n>` starts behind route n's grid;
//! `NFSGBA_ENV=<n>` picks the environment; `NFSGBA_CAM=x,y,z,tx,ty,tz` sets the start eye and target (metres);
//! `NFSGBA_SHOT=<file.png>` saves one frame and quits, for checking renders without anyone at the screen.

use std::{collections::BTreeMap, f64::consts::TAU};

use bevy::{
    asset::{RenderAssetUsages, embedded_asset},
    camera_controller::free_camera::{FreeCamera, FreeCameraPlugin, FreeCameraState},
    core_pipeline::tonemapping::{DebandDither, Tonemapping},
    light::NotShadowCaster,
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
    window::WindowResolution,
};
use nfsgba_formats::{self as rom, paint, sky};

/// Raw units to metres. The engine draws cars and city in one unit (vehicle matrices are pure rotations,
/// translations are city positions), and all 15 car models measure ~48 units per real-world metre, so the
/// city comes out exaggerated: streets ~40 m wide, blocks ~50 m tall.
const SCALE: f32 = 1.0 / 48.0;

/// The race projection (view struct `0x03000080`): focal length 150 on the 240×160 screen.
const FOCAL: f32 = 150.0;

/// The skyline's clip rows (view rect `0x030053D0` `+4`/`+0xC` in the race).
const SKY_CLIP: [i32; 2] = [0, 159];

/// The reference race's racers (`docs/formats/car-paint.md`), which the viewer takes as given: the game draws
/// them from the RNG (`pick_opponent_cars`) and the save. Car ids (`0x0300611C`), paints (`0x03005FEC`), the
/// player's car record (paint 11, glass 0) and the opponents' materials (entity `+0x48`).
const RACE_CARS: [i8; 4] = [2, 9, 10, 11];
const RACE_PAINTS: [i8; 4] = [11, 11, 11, 5];
const RACE_RECORD: [u8; 0x11] = [0, 0, 0, 0, 0, 0, 11, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
const OPPONENT_MATERIALS: [usize; 3] = [140, 141, 142];

/// Raw space (x right, y down, z forward) to Bevy (y up, -z forward): a 180° turn about x, no mirroring.
fn world(x: impl Into<f32>, y: impl Into<f32>, z: impl Into<f32>) -> Vec3 {
    Vec3::new(x.into(), -y.into(), -z.into()) * SCALE
}

/// A game angle (0x4000 per turn; entity headings and the camera yaw) as a direction on the ground: raw
/// `(sin, 0, cos)`, so 0 faces +z and 0x1000 faces +x (checked against every route's racing line).
fn direction(angle: i32) -> Vec3 {
    let a = f64::from(angle & 0x3FFF) * TAU / 16384.0;
    world(a.sin() as f32, 0.0_f32, a.cos() as f32).normalize()
}

/// The camera yaw (`0x03000214`, the convention of `direction`) of a view direction.
fn game_yaw(forward: Vec3) -> i32 {
    (f64::from(forward.x).atan2(-f64::from(forward.z)) * 16384.0 / TAU).round() as i32
}

/// The horizon shift (`0x030056B8`, screen rows): how far below the screen centre the eye-level horizon falls,
/// `focal · tan(pitch)`, clamped to ±32 as `camera_update` does. The game's cameras never pitch (0 in the chase
/// view; the bumper view shifts the projection centre instead); the viewer's free camera does.
fn horizon(forward: Vec3) -> i32 {
    (FOCAL * forward.y / forward.xz().length()).round().clamp(-32.0, 32.0) as i32
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

/// Index 0 shows the backdrop instead of being skipped (`indexed.wgsl`).
const OPAQUE: u32 = 1;
/// `indices` is the 240×160 GBA screen, read at each pixel's screen position (the sky layer).
const SCREEN: u32 = 2;

/// GBA-style indexed colour (`indexed.wgsl`): a texture of 8-bit palette indices drawn through a 256-colour
/// palette texture, texel by texel with wrapped integer coordinates; index 0 per `mode`.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
struct Indexed {
    #[texture(0, sample_type = "u_int")]
    indices: Handle<Image>,
    #[texture(1)]
    palette: Handle<Image>,
    /// Palette entry 0 per GBA screen line (160×1).
    #[texture(2)]
    backdrop: Handle<Image>,
    /// `x`: `OPAQUE | SCREEN` flags.
    #[uniform(3)]
    mode: UVec4,
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

/// An RGBA8 colour strip (`width`×1); the palette (256) and the backdrop lines (160). Systems rewrite its bytes.
fn colour_image(width: usize) -> Image {
    texture_2d(width, 1, vec![0; width * 4], TextureFormat::Rgba8UnormSrgb)
}

fn rgba(palette: &[u16]) -> Vec<u8> {
    rom::palette_rgba(palette).as_flattened().to_vec()
}

/// A car atlas moved into the car slots as `unpack_player_atlas` does for the player (`paint::remap_atlas`, body
/// 0xD0, trim 0xC0: pixel `i` becomes slot `192 + (i ^ 16)`).
fn player_atlas(atlas: &rom::Texture) -> Image {
    let mut pixels = atlas.pixels.clone();
    paint::remap_atlas(&mut pixels, 0xD0, 0xC0);
    index_image(&rom::Texture {
        pixels,
        ..atlas.clone()
    })
}

/// The race's base palette: the city palette with the reference race's car ramps (`load_car_palettes`).
fn race_base(data: &[u8], city: &[u16]) -> Vec<u16> {
    let mut base = city.to_vec();
    paint::load_car_palettes(data, &mut base, RACE_CARS, RACE_PAINTS, &RACE_RECORD, true);
    base
}

/// A showroom car's palette, as the garage builds it (`load_car_palettes(0)` with the edited record, paint `paint`
/// in slots 208..=223; `shade_car_paint` at turntable angle 0 for the glass).
fn garage_base(data: &[u8], city: &[u16], car: usize, paint: i8) -> Vec<u16> {
    let mut base = city.to_vec();
    let mut record = [0; 0x11];
    record[6] = paint as u8;
    paint::load_car_palettes(data, &mut base, [car as i8, 0, 0, 0], [paint, 0, 0, 0], &record, false);
    [base[192], base[208]] = paint::glass_shades(data, record[5], 0);
    base
}

/// Each route's grid headings: template entity `+0x2C >> 8` (0x4000 per turn). The route table `0x7F2798`
/// (0x14 bytes per route) points at the route's 0xA4-byte template entities at `+0x00`.
fn grid_headings(data: &[u8], routes: usize) -> Vec<[i32; 4]> {
    let word = |at: usize| i32::from_le_bytes(data[at..at + 4].try_into().unwrap());
    (0..routes)
        .map(|r| {
            let entities = (word(0x7F_2798 + 0x14 * r) as u32 - rom::ROM_BASE) as usize;
            std::array::from_fn(|e| word(entities + 0xA4 * e + 0x2C) >> 8)
        })
        .collect()
}

fn main() {
    let mut app = App::new();
    app.insert_resource(GlobalAmbientLight {
        brightness: 500.0,
        ..default()
    })
    .add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "NFS Carbon GBA city viewer (unofficial)".into(),
            // Four times the GBA screen: every GBA pixel is 4×4 window pixels.
            resolution: WindowResolution::new(960, 640),
            ..default()
        }),
        ..default()
    }))
    .add_plugins((FreeCameraPlugin, MaterialPlugin::<Indexed>::default()))
    .add_systems(Startup, setup)
    .add_systems(Update, (shot, race, sky.after(race), tint.after(sky)));
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
    // One palette for the city, the race cars and the skyline, as on the GBA; `tint` fills it every frame.
    // The backdrop (palette entry 0 per screen line) and the sky layer's screen are filled by `sky`.
    let palette = images.add(colour_image(256));
    let backdrop = images.add(colour_image(sky::SCREEN_H));
    let screen = images.add(texture_2d(
        sky::SCREEN_W,
        sky::SCREEN_H,
        vec![0; sky::SCREEN_W * sky::SCREEN_H],
        TextureFormat::R8Uint,
    ));
    let mut new_material = |indices, palette: &Handle<Image>, mode| {
        indexed.add(Indexed {
            indices,
            palette: palette.clone(),
            backdrop: backdrop.clone(),
            mode: UVec4::new(mode, 0, 0, 0),
        })
    };

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
        // `raster_wall_columns` picks the transparent drawer when a texture's first stored texel is 0 and the
        // opaque one otherwise; floors and ceilings are always opaque and hold no index 0.
        let t = &textures[m as usize];
        let mode = if t.pixels[0] != 0 { OPAQUE } else { 0 };
        let material = new_material(images.add(index_image(t)), &palette, mode);
        commands.spawn((Mesh3d(meshes.add(tris.mesh())), MeshMaterial3d(material)));
    }

    // Showroom in front of the city: one row per car, its paint variants side by side, each car in the garage's
    // palette with paint number = car row. It is a display only (the garage shows one car at a time), so every
    // car has a palette of its own; `tint` keeps them lit like the city.
    let models = rom::models(&data);
    let vehicle_textures = rom::vehicle_textures(&data);
    let on_ground = |t: &Tris| -t.pos.iter().map(|p| p[1]).fold(f32::MAX, f32::min);
    let showroom = Vec3::new(center.x - 12.0, 0.0, max.z + 80.0);
    let mut car_models = std::collections::HashSet::new();
    let cars = rom::cars(&data);
    for (c, car) in cars.iter().enumerate() {
        car_models.extend(car.models);
        for v in 0..car.paint_variants {
            let atlas = &vehicle_textures[car.first_material + v];
            let tris = model_tris(&models[car.models[0]], Some(atlas), Color::WHITE);
            let at = showroom + Vec3::new(v as f32 * 6.0, on_ground(&tris), c as f32 * 7.0);
            let image = images.add(colour_image(256));
            let material = new_material(images.add(player_atlas(atlas)), &image, 0);
            commands.spawn((
                Mesh3d(meshes.add(tris.mesh())),
                MeshMaterial3d(material),
                Transform::from_translation(at),
                CarPalette {
                    car: c,
                    paint: c as i8,
                    image,
                },
            ));
        }
        info!("showroom row {c}: {} ({} paint variants)", car.name, car.paint_variants);
    }
    info!("showroom at {showroom:.1} m (rows of cars along +z, paint variants along +x)");

    // Race: one route's racing line and the reference race's four cars on its start grid, in the race palette.
    // R cycles the 44 routes; NFSGBA_ROUTE picks the first one and starts behind the grid.
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
    let routes = rom::routes(&data);
    let mut race = Race {
        headings: grid_headings(&data, routes.len()),
        routes,
        floors,
        current: start_route.unwrap_or(23),
        active: start_route.is_some(),
        lift: 0.0,
    };
    for (slot, &car) in RACE_CARS.iter().enumerate() {
        let car = &cars[car as usize];
        // The player's atlas is its material remapped into the car slots; the opponents' materials already hold
        // final slots (their body ramps at 160, 176 or 208).
        let (atlas, indices) = match slot {
            0 => {
                let atlas = &vehicle_textures[car.first_material + RACE_RECORD[3] as usize];
                (atlas, player_atlas(atlas))
            }
            _ => {
                let atlas = &vehicle_textures[OPPONENT_MATERIALS[slot - 1]];
                (atlas, index_image(atlas))
            }
        };
        // Close up the race draws model entity `+0x36 - 1` = car table `+0x14` (`draw_sector_entities`; all four
        // reference entities), our `models[1]`.
        // NOT 1:1 (R10/R12): beyond depth 0x1FF the next model is drawn; the player also draws model 12 (entity
        // `+0x64`) with a second matrix; decals and overlays are not on the atlas (R13).
        let tris = model_tris(&models[car.models[1]], Some(atlas), Color::WHITE);
        let lift = on_ground(&tris);
        if slot == 0 {
            race.lift = lift;
        }
        commands.spawn((
            Mesh3d(meshes.add(tris.mesh())),
            MeshMaterial3d(new_material(images.add(indices), &palette, 0)),
            race.grid(slot, lift),
            GridSlot { slot, lift },
        ));
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

    // Sky: the GBA screen as it is before the world is drawn (skyline rows, index 0 elsewhere), on a quad that
    // follows the camera behind everything. `sky` redraws it and the backdrop lines when the view changes.
    let sky_layer = new_material(screen.clone(), &palette, OPAQUE | SCREEN);
    let city_palettes: Vec<Vec<u16>> = envs.iter().map(|e| rom::city_palette_raw(&data, e.palette)).collect();
    let descs: Vec<_> = (0..envs.len()).map(|e| sky::sky_desc(&data, e)).collect();
    commands.insert_resource(Skies {
        buffers: descs.iter().map(|d| sky::gradient_buffer(&data, d)).collect(),
        descs,
        palettes: city_palettes.clone(),
        current: env,
        screen: vec![0; sky::SCREEN_W * sky::SCREEN_H],
        screen_image: screen,
        backdrop,
        drawn: None,
    });
    commands.insert_resource(Tint {
        raw: city_palettes[env].clone(),
        rom: data,
        sectors,
        m: None,
        sector: None,
        palette,
        written: Vec::new(),
        dirty: true,
    });

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
        // The race's vertical field of view: 160 lines at focal length 150.
        // NOT 1:1 (R11): the game projects through a reciprocal table with the centre at (120, 79), in integers.
        Projection::Perspective(PerspectiveProjection {
            fov: 2.0 * (sky::SCREEN_H as f32 / 2.0 / FOCAL).atan(),
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
        children![(
            Mesh3d(meshes.add(Rectangle::new(1.0e6, 1.0e6))),
            MeshMaterial3d(sky_layer),
            Transform::from_xyz(0.0, 0.0, -20000.0),
            NotShadowCaster,
        )],
    ));
    info!(
        "city bounds {min:.0} .. {max:.0} m; controls: right mouse look, WASD/QE move, Shift run, wheel speed, K sky"
    );
}

#[derive(Resource)]
struct Race {
    routes: Vec<rom::Route>,
    /// Grid headings per route (template entity `+0x2C >> 8`).
    headings: Vec<[i32; 4]>,
    /// Mean floor height (m) of every sector.
    floors: Vec<f32>,
    current: usize,
    /// Race mode (NFSGBA_ROUTE or R): the light follows the player's car instead of the camera.
    active: bool,
    /// The player's model lift (the chase camera sits relative to the model's origin, the entity position).
    lift: f32,
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

    /// The heading of grid slot `slot` (0x4000 per turn), which `shade_car_paint` also reads for the player.
    fn heading(&self, slot: usize) -> i32 {
        self.headings[self.current][slot]
    }

    /// A car on grid slot `slot`, standing on the start sector's floor, facing its template heading.
    fn grid(&self, slot: usize, lift: f32) -> Transform {
        let [x, _, z] = self.route().grid[slot];
        let floor = self.route().waypoints.first().map_or(0.0, |w| self.floors[w.sector]);
        let at = world(x as f32, 0.0_f32, z as f32).with_y(floor + lift);
        Transform::from_translation(at).looking_to(direction(self.heading(slot)), Vec3::Y)
    }

    /// The race's chase camera: level (the game never pitches it), yaw = the player's heading, and the car at
    /// (0, 134, 343) in camera space (the reference race's vehicle matrix, world `+0xFC`).
    /// NOT 1:1 (R11): the game's camera trails the car (yaw 0xFFD against heading 0x1000 at the reference start).
    fn chase(&self) -> Transform {
        let car = self.grid(0, self.lift).translation;
        let ahead = direction(self.heading(0));
        let eye = car - ahead * 343.0 * SCALE + Vec3::Y * 134.0 * SCALE;
        Transform::from_translation(eye).looking_to(ahead, Vec3::Y)
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

/// A showroom car's own palette: the garage palette for `car` in paint `paint`, tinted like the city.
#[derive(Component)]
struct CarPalette {
    car: usize,
    paint: i8,
    image: Handle<Image>,
}

/// The race palette every frame, in the game's order (`docs/formats/car-paint.md`, "Frame timing"):
/// `shade_car_paint` writes the glass shades for the player's heading into the base palette, then the light tint
/// (`FUN_0813a514`) derives palette RAM from it, tinted by the light interpolated at the observer's position in
/// its sector (`rom::sector_light`); the next frame's shade restores the raw glass (`paint::race_palette`).
/// When no light is found the palette stays as it was. The observer is the camera, or the player's car in race
/// mode.
#[derive(Resource)]
struct Tint {
    rom: Vec<u8>,
    sectors: Vec<rom::Sector>,
    /// The current environment's city palette, untinted.
    raw: Vec<u16>,
    /// Current multipliers; `None` until a sector first gives a light (the palette is then as loaded).
    m: Option<[i32; 3]>,
    sector: Option<usize>,
    /// The shared palette (city, race cars, skyline) and what was last written to it.
    palette: Handle<Image>,
    written: Vec<u16>,
    /// The raw palette changed (K): rebuild the showroom palettes.
    dirty: bool,
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

/// Find the observer's sector and light, then rewrite the palette textures that changed.
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
    // NOT 1:1 (R15): the game follows the player's sector through the portals it crosses (0x03005614); we look
    // it up by position, keeping the current sector while it still contains the point, else taking the
    // containing sector whose floor is nearest the observer's height (sectors can overlap on different levels).
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
    }
    let ram = |base: Vec<u16>| match m {
        Some(m) => paint::race_palette(&base, m),
        None => base,
    };

    // NOT 1:1 (R17): for 1–7 scanlines per game frame the game shows the tinted glass instead.
    let mut base = race_base(&tint.rom, &tint.raw);
    [base[192], base[208]] = paint::glass_shades(&tint.rom, RACE_RECORD[5], race.heading(0));
    let palette = ram(base);
    if palette != tint.written
        && let Some(mut image) = images.get_mut(&tint.palette)
    {
        image.data = Some(rgba(&palette));
        tint.written = palette;
    }
    for car in &cars {
        if (changed || car.is_changed())
            && let Some(mut image) = images.get_mut(&car.image)
        {
            image.data = Some(rgba(&ram(garage_base(&tint.rom, &tint.raw, car.car, car.paint))));
        }
    }
}

/// The 12 environments' city palettes and skies, and the sky layer: the GBA screen before the world is drawn.
#[derive(Resource)]
struct Skies {
    palettes: Vec<Vec<u16>>,
    descs: Vec<sky::SkyDesc>,
    /// Gradient buffers (`0x0200120C`, after the race fade-in).
    buffers: Vec<Vec<u16>>,
    current: usize,
    /// 240×160 indices: `draw_skyline`'s rows on a cleared screen.
    screen: Vec<u8>,
    screen_image: Handle<Image>,
    /// Palette entry 0 per screen line (160×1 RGBA).
    backdrop: Handle<Image>,
    /// Environment and camera of the last drawn sky.
    drawn: Option<(usize, sky::SkyCamera)>,
}

/// K switches to the next environment (sky and palette). Redraws the sky layer when the environment or the
/// camera's yaw or horizon changes: the skyline (`sky::draw_skyline`) and the backdrop colour of every screen line
/// (`gradient_buffer[backdrop_entry(gradient_start, y)]`, palette entry 0, which the light tint leaves alone).
///
/// NOT 1:1 (R6): the game clears only whole 32-byte blocks above the skyline and never clears below it, so a few
/// bytes keep the page's previous frame (none in the chase view); the viewer starts from a cleared screen.
fn sky(
    keys: Res<ButtonInput<KeyCode>>,
    mut skies: ResMut<Skies>,
    mut tint: ResMut<Tint>,
    camera: Single<&Transform, With<Camera3d>>,
    mut images: ResMut<Assets<Image>>,
) {
    if keys.just_pressed(KeyCode::KeyK) {
        skies.current = (skies.current + 1) % skies.palettes.len();
        tint.raw = skies.palettes[skies.current].clone();
        tint.dirty = true;
        info!("environment {}", skies.current);
    }
    let forward = camera.forward().as_vec3();
    let cam = sky::SkyCamera {
        yaw: game_yaw(forward),
        horizon: horizon(forward),
        view: 2,
        ..default()
    };
    if skies.drawn == Some((skies.current, cam)) {
        return;
    }
    let Skies {
        descs,
        buffers,
        current,
        screen,
        ..
    } = &mut *skies;
    let desc = &descs[*current];
    screen.fill(0);
    sky::draw_skyline(&tint.rom, desc, &cam, SKY_CLIP, screen);
    let start = sky::gradient_start(desc, &cam);
    let lines: Vec<u8> = (0..sky::SCREEN_H)
        .flat_map(|y| rom::bgr555(buffers[*current][sky::backdrop_entry(start, y)]))
        .collect();
    if let Some(mut image) = images.get_mut(&skies.screen_image) {
        image.data = Some(skies.screen.clone());
    }
    if let Some(mut image) = images.get_mut(&skies.backdrop) {
        image.data = Some(lines);
    }
    skies.drawn = Some((skies.current, cam));
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The camera conversions invert `direction` and follow the game's sign conventions.
    #[test]
    fn camera_angles() {
        for angle in [0, 0x1000, 0x0FFD, 0x2000, 0x3000, 0x0452, 0x1C1A] {
            assert_eq!(game_yaw(direction(angle)) & 0x3FFF, angle);
        }
        let level = direction(0x1000);
        assert_eq!(horizon(level), 0);
        // Looking up, the horizon falls below the screen centre (positive shift), and the shift is clamped.
        assert_eq!(horizon((level + Vec3::Y * 0.1).normalize()), 15);
        assert_eq!(horizon(Vec3::NEG_Y), -32);
    }
}
