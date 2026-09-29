//! Fly-through viewer for Carbon's city (portal/sector world) and vehicle model bank, read from the user's ROM:
//! textured walls, floors and ceilings, the race sky, a race's four cars, and a showroom of all 15 cars in every
//! paint variant. Run from the repo root: `cargo run --release -p nfsgba-viewer`.
//!
//! Colour works like the GBA's: textures are palette indices, drawn through one 256-colour palette that holds the
//! city and the race cars and that the game's light tint rewrites every frame; behind the world the window shows
//! the GBA's 240×160 sky layer (backdrop gradient per line, skyline panorama). The window is the GBA screen
//! scaled up, projected as the game projects (`game::GbaProjection`). In a race the camera is the game's chase
//! camera and only what the game's portal pass lists is drawn (`render::visible_sectors`, clipped to each
//! entry's screen span); O swaps the GPU view for the game's own 240×160 frame (`render::draw_world`). See
//! `docs/engine/viewer-rendering.md`.
//!
//! Controls: R moves to the next race route (cars onto its grid, game camera behind the player); G switches
//! between the game camera and the free camera (Bevy `FreeCamera`: hold right mouse to look, M toggles, WASD move,
//! Q/E down/up, Shift run, scroll wheel speed); O toggles the original-resolution frame; K switches environment.
//! `NFSGBA_ROUTE=<n>` starts a Quick Play race on route n's grid; `NFSGBA_DUMP=<dir/name>` starts from a race dump
//! under `$NFSGBA_DATA/work/e5298b24/` (e.g. `mgba/race`, the reference race); `NFSGBA_ORIGINAL=1` starts in the
//! original-resolution frame; `NFSGBA_ENV=<n>` picks the environment; `NFSGBA_CAM=x,y,z,tx,ty,tz` sets the free
//! camera's start eye and target (metres); `NFSGBA_SHOT=<file.png>` saves one frame and quits.

mod game;

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
use game::{GbaProjection, RaceSetup};
use nfsgba_formats::{self as rom, atlas, paint, render, sky};

/// Raw units to metres. The engine draws cars and city in one unit (vehicle matrices are pure rotations,
/// translations are city positions), and all 15 car models measure ~48 units per real-world metre, so the
/// city comes out exaggerated: streets ~40 m wide, blocks ~50 m tall.
const SCALE: f32 = 1.0 / 48.0;

/// The skyline's clip rows (view rect `0x030053D0` `+4`/`+0xC` in the race).
const SKY_CLIP: [i32; 2] = [0, 159];

/// The Quick Play car (the reference race's Cobalt).
const PLAYER_CAR: i8 = 2;

/// Raw space (x right, y down, z forward) to Bevy (y up, -z forward): a 180° turn about x, no mirroring.
fn world(x: impl Into<f32>, y: impl Into<f32>, z: impl Into<f32>) -> Vec3 {
    Vec3::new(x.into(), -y.into(), -z.into()) * SCALE
}

/// An 8.8 entity position in the viewer's world.
fn world_fixed(p: [i32; 3]) -> Vec3 {
    world(p[0] as f32 / 256.0, p[1] as f32 / 256.0, p[2] as f32 / 256.0)
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
/// view; the bumper view shifts the projection centre instead); only the viewer's free camera does.
fn horizon(forward: Vec3) -> i32 {
    (game::FOCAL as f32 * forward.y / forward.xz().length()).round().clamp(-32.0, 32.0) as i32
}

/// Unindexed triangle soup; flat normals are computed at the end. `uv_b` marks city geometry for the portal
/// clip: (sector, 1 for walls / 0 for flats).
#[derive(Default)]
struct Tris {
    pos: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    uv_b: Vec<[f32; 2]>,
    col: Vec<[f32; 4]>,
}

impl Tris {
    /// Fan-triangulate a convex polygon (sector floors, wall quads, model faces) in one colour; counter-clockwise
    /// points face the viewer.
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

    /// A city polygon of `sector`, tagged for the portal clip.
    fn city(&mut self, pts: &[(Vec3, Vec2)], sector: usize, wall: bool) {
        let n = self.pos.len();
        self.fan(pts, Color::WHITE);
        self.uv_b.resize(self.pos.len(), [0.0; 2]);
        self.uv_b[n..].fill([sector as f32, wall as u8 as f32]);
    }

    fn mesh(self) -> Mesh {
        let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uv)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.col);
        if !self.uv_b.is_empty() {
            m.insert_attribute(Mesh::ATTRIBUTE_UV_1, self.uv_b);
        }
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
/// Back faces are not drawn (city geometry, whose facing is set from the game's wall rules).
const CULL: u32 = 4;
/// Transparent wall texture: a pixel pair of the GBA's 240 columns is drawn only if both texels are non-zero.
const PAIRS: u32 = 8;

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
    /// `x`: `OPAQUE | SCREEN | CULL | PAIRS` flags.
    #[uniform(3)]
    mode: UVec4,
    /// The game's visible-sector list (32×1 `Rgba32Sint`, see `portal_texels`).
    #[texture(4, sample_type = "s_int")]
    portals: Handle<Image>,
}

impl Material for Indexed {
    fn fragment_shader() -> ShaderRef {
        "embedded://nfsgba_viewer/indexed.wgsl".into()
    }

    /// Both faces reach the shader: the car models' winding is not normalised, and the city culls in the shader.
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

/// The portal texture: texel 0 holds the entry count (−1: no list, draw everything), texels 1.. the drawn entries
/// as (sector, left, right, top | bottom << 16).
fn portal_texels(entries: Option<&[render::Portal]>) -> Vec<u8> {
    let mut texels = vec![[0i32; 4]; 32];
    match entries {
        None => texels[0][0] = -1,
        Some(list) => {
            texels[0][0] = list.len() as i32;
            for (t, p) in texels[1..].iter_mut().zip(list) {
                *t = [
                    p.sector as i32,
                    p.left as i32,
                    p.right as i32,
                    p.top as i32 | (p.bottom as i32) << 16,
                ];
            }
        }
    }
    texels.iter().flatten().flat_map(|v| v.to_le_bytes()).collect()
}

/// A car atlas moved into the car slots as `unpack_player_atlas` does for the player (`paint::remap_atlas`, body
/// 0xD0, trim 0xC0: pixel `i` becomes slot `192 + (i ^ 16)`); the showroom's display.
fn remapped(atlas: &rom::Texture) -> Image {
    let mut pixels = atlas.pixels.clone();
    paint::remap_atlas(&mut pixels, 0xD0, 0xC0);
    index_image(&rom::Texture {
        pixels,
        ..atlas.clone()
    })
}

/// The race's base palette: the city palette with the racers' car ramps (`load_car_palettes`).
fn race_base(data: &[u8], city: &[u16], setup: &RaceSetup) -> Vec<u16> {
    let mut base = city.to_vec();
    paint::load_car_palettes(data, &mut base, setup.cars, setup.paints, &setup.record, true);
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

/// The floor height (8.8, `-y` up) at (x, z) in `sector`: the floor fan (wall 0 to each later edge) the viewer
/// draws, with the corner heights the flats use (`Wall::floor_y`).
/// NOT 1:1 (D2): the game's racers stand where the physics puts them.
fn floor_at(sectors: &[rom::Sector], sector: u16, x: i32, z: i32) -> i32 {
    let w = &sectors[sector as usize].walls;
    let p = |k: usize| Vec3::new(w[k].x as f32, w[k].floor_y as f32, w[k].z as f32);
    let q = Vec2::new(x as f32, z as f32);
    let inside = (1..w.len().saturating_sub(1)).find_map(|k| {
        let (a, b, c) = (p(0), p(k), p(k + 1));
        let area = |u: Vec3, v: Vec3, r: Vec2| (v.x - u.x) * (r.y - u.z) - (v.z - u.z) * (r.x - u.x);
        let total = area(a, b, c.xz());
        let (l0, l1) = (area(b, c, q) / total, area(c, a, q) / total);
        let l2 = 1.0 - l0 - l1;
        (l0 >= 0.0 && l1 >= 0.0 && l2 >= 0.0).then(|| l0 * a.y + l1 * b.y + l2 * c.y)
    });
    (inside.unwrap_or(p(0).y) * 256.0) as i32
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
    .add_systems(
        Update,
        (
            shot,
            (keys, game_camera, visibility, sky, tint).chain(),
            racing_line,
        ),
    );
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
    // The backdrop (palette entry 0 per screen line) and the sky layer's screen are filled by `sky`, the portal
    // list by `visibility`.
    let palette = images.add(colour_image(256));
    let backdrop = images.add(colour_image(sky::SCREEN_H));
    let screen = images.add(texture_2d(
        sky::SCREEN_W,
        sky::SCREEN_H,
        vec![0; sky::SCREEN_W * sky::SCREEN_H],
        TextureFormat::R8Uint,
    ));
    let portals = images.add(texture_2d(32, 1, portal_texels(None), TextureFormat::Rgba32Sint));
    let mut new_material = |indices, palette: &Handle<Image>, mode| {
        indexed.add(Indexed {
            indices,
            palette: palette.clone(),
            backdrop: backdrop.clone(),
            mode: UVec4::new(mode, 0, 0, 0),
            portals: portals.clone(),
        })
    };

    // City geometry grouped by material, one mesh each, every vertex tagged with its sector. Material 0 is never
    // drawn (`draw_sector_walls`; the flat passes skip it). Walls: those without flag bit 0 (open portals), solid
    // or portal alike (R8: a portal wall is a step, kerb or fence over its own top..bottom). A wall's front, where
    // its start lies left of its end on screen, is drawn unless the wall has flag 2; its back only with flag 2 or
    // 0x2000 (`setup_wall_spans`), and then as a deferred wall, which the 8-bit defer mask drops for countdown
    // index 8 and up (`draw_sector_walls`). Flats from each corner's floor/ceiling height (`+0x38`/`+0x3A`, R19),
    // both faces.
    let mut by_material: BTreeMap<u16, Tris> = BTreeMap::new();
    let floor_uv = |w: &rom::Wall| Vec2::new(w.floor_uv[0] as f32, w.floor_uv[1] as f32) / 16384.0;
    for (s, sector) in sectors.iter().enumerate() {
        let w = &sector.walls;
        for (material, floor) in [(sector.floor, true), (sector.ceiling, false)] {
            if material != 0 {
                let mut pts: Vec<_> = w
                    .iter()
                    .map(|w| {
                        let y = if floor { w.floor_y } else { w.ceiling_y };
                        (world(w.x as f32, y, w.z as f32), floor_uv(w))
                    })
                    .collect();
                let tris = by_material.entry(material).or_default();
                tris.city(&pts, s, false);
                pts.reverse();
                tris.city(&pts, s, false);
            }
        }
        for (k, a) in w.iter().enumerate().filter(|(_, a)| a.material != 0 && a.flags & 1 == 0) {
            let b = &w[(k + 1) % w.len()];
            let (ax, az, bx, bz) = (a.x as f32, a.z as f32, b.x as f32, b.z as f32);
            let t = &textures[a.material as usize];
            let uv = a.uv(t.width, t.height).map(Vec2::from);
            let front = [
                (world(ax, a.top[0], az), uv[0]),
                (world(ax, a.bottom[0], az), uv[3]),
                (world(bx, a.bottom[1], bz), uv[2]),
                (world(bx, a.top[1], bz), uv[1]),
            ];
            let tris = by_material.entry(a.material).or_default();
            if a.flags & 2 == 0 {
                tris.city(&front, s, true);
            }
            let countdown = w.len() - k;
            if a.flags & 0x2002 != 0 && countdown < 8 {
                let back: Vec<_> = front.iter().rev().copied().collect();
                tris.city(&back, s, true);
            }
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
        // `raster_wall_columns` picks the transparent drawer (pairs) when a texture's first stored texel is 0 and
        // the opaque one otherwise; floors and ceilings are always opaque and hold no index 0.
        let t = &textures[m as usize];
        let mode = CULL | if t.pixels[0] != 0 { OPAQUE } else { PAIRS };
        let material = new_material(images.add(index_image(t)), &palette, mode);
        commands.spawn((Mesh3d(meshes.add(tris.mesh())), MeshMaterial3d(material), CityMesh));
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
            let material = new_material(images.add(remapped(atlas)), &image, 0);
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

    // Race: the reference race from a dump (NFSGBA_DUMP), or a Quick Play race on a route's grid (NFSGBA_ROUTE;
    // R cycles the 44 routes), with the racers dealt as the game deals them.
    let floors = sectors
        .iter()
        .map(|s| {
            s.walls
                .iter()
                .map(|w| world(0.0_f32, w.floor_y, 0.0_f32).y)
                .sum::<f32>()
                / s.walls.len().max(1) as f32
        })
        .collect();
    let routes = rom::routes(&data);
    let dump = std::env::var("NFSGBA_DUMP").ok().map(|p| {
        let d = game::Dump::load(&p).unwrap_or_else(|e| panic!("NFSGBA_DUMP={p}: {e}"));
        (RaceSetup::from_dump(&d), d.route())
    });
    let start_route: Option<usize> = std::env::var("NFSGBA_ROUTE").ok().and_then(|s| s.parse().ok());
    let current = dump.as_ref().map(|d| d.1).or(start_route).unwrap_or(23);
    let setup = match &dump {
        Some((setup, _)) => setup.clone(),
        None => RaceSetup::grid(&data, &routes[current], PLAYER_CAR, game::REFERENCE_RAND, |s, x, z| {
            floor_at(&sectors, s, x, z)
        }),
    };
    let active = dump.is_some() || start_route.is_some();
    info!(
        "race on route {current}: cars {:?}, paints {:?}{}",
        setup.cars,
        setup.paints,
        if dump.is_some() { " (from the dump)" } else { "" }
    );
    // The player's texture is built as `unpack_player_atlas` builds it, with the rim at angle 0 (race start);
    // the opponents' are raw materials already in their palette slots (`look`).
    // NOT 1:1 (R13 in-race): the game redraws the rim rotated by the wheel angle most frames; the viewer has no
    // wheel spin. NOT 1:1 (R24): the rim blit can read heap bytes around the rim buffer; not at angle 0.
    let player_car = setup.cars[0] as usize;
    let (_, mut player_pixels) = atlas::player_atlas(&data, &vehicle_textures, player_car, 0, &setup.record);
    if let Some(rim) = atlas::rim(&data, &vehicle_textures, player_car, &setup.record) {
        let pixels = atlas::rim_pixels(&vehicle_textures, &rim);
        atlas::draw_rim(&data, &mut player_pixels, &rim, &pixels, 0, 0);
    }
    for slot in 0..4 {
        // Close up the race draws model entity `+0x36 − 1`: for the player the car table's close model (`+0x14`),
        // for the opponents their `0x7EEA44` entry's model − 1 (`draw_sector_entities`).
        // NOT 1:1 (R12): beyond depth 0x1FF the next model is drawn; the player also draws model entity `+0x64`
        // with a second matrix; the entity draw's clip span and LOD culls are not applied.
        let (model, atlas) = if slot == 0 {
            let first = cars[player_car].first_material;
            let t = &vehicle_textures[first];
            (
                cars[player_car].models[1],
                rom::Texture {
                    pixels: player_pixels.clone(),
                    ..t.clone()
                },
            )
        } else {
            let look = atlas::look(&data, setup.cars, slot, false, false);
            (
                look.model as usize - 1,
                vehicle_textures[look.material as usize].clone(),
            )
        };
        let tris = model_tris(&models[model], Some(&atlas), Color::WHITE);
        let lift = on_ground(&tris);
        commands.spawn((
            Mesh3d(meshes.add(tris.mesh())),
            MeshMaterial3d(new_material(images.add(index_image(&atlas)), &palette, 0)),
            Transform::default(),
            RaceCar { slot, lift },
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
    let free_cam = std::env::var("NFSGBA_CAM").is_ok();
    commands.insert_resource(Race {
        routes,
        floors,
        current,
        active,
        from_dump: dump.is_some(),
        game_camera: active && !free_cam,
        original: active && std::env::var("NFSGBA_ORIGINAL").is_ok(),
        frame: None,
        visible: None,
        setup,
        portals: portals.clone(),
    });

    // Sky: the GBA screen as it is before the world is drawn (skyline rows, index 0 elsewhere), on a quad that
    // follows the camera behind everything. `sky` redraws it and the backdrop lines when the view changes; in the
    // original-resolution frame the world is drawn into it as well.
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
        // The game's projection: focal 150 on the 240×160 screen, optical centre (120, 79), near plane 64.
        Projection::custom(GbaProjection {
            focal: game::FOCAL as f32,
            near: game::NEAR as f32 * SCALE,
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
        Transform::from_translation(eye).looking_at(target, Vec3::Y),
        children![(
            Mesh3d(meshes.add(Rectangle::new(1.0e6, 1.0e6))),
            MeshMaterial3d(sky_layer),
            Transform::from_xyz(0.0, 0.0, -20000.0),
            NotShadowCaster,
        )],
    ));
    info!(
        "city bounds {min:.0} .. {max:.0} m; keys: R next route, G game/free camera, O original frame, K sky; \
         free camera: right mouse look, WASD/QE move, Shift run, wheel speed"
    );
}

/// The city's meshes (hidden in the original-resolution frame).
#[derive(Component)]
struct CityMesh;

#[derive(Resource)]
struct Race {
    routes: Vec<rom::Route>,
    /// Mean floor height (m) of every sector (the racing line's height).
    floors: Vec<f32>,
    current: usize,
    /// A race is shown: its cars stand where the setup puts them and the light follows the player.
    active: bool,
    /// The racers stand where a dump had them (else on the route's grid).
    from_dump: bool,
    /// The camera is the game's chase camera (else the free camera).
    game_camera: bool,
    /// Show the game's own 240×160 world frame (`render::draw_world`) instead of the GPU view.
    original: bool,
    /// This frame's render frame and camera-sector entry, in game-camera mode.
    frame: Option<(render::Frame, render::Portal)>,
    /// The visible-sector list of `frame` (unfiltered, as `draw_world` takes it).
    visible: Option<render::Visibility>,
    setup: RaceSetup,
    /// The portal texture every city material reads.
    portals: Handle<Image>,
}

impl Race {
    fn route(&self) -> &rom::Route {
        &self.routes[self.current]
    }

    fn at(&self, w: &rom::Waypoint) -> Vec3 {
        world(w.x as f32, 0.0_f32, w.z as f32).with_y(self.floors[w.sector])
    }

    fn player(&self) -> &game::Racer {
        &self.setup.racers[0]
    }
}

/// A race car: racer `slot`'s model, whose lowest point is `lift` below its origin.
#[derive(Component)]
struct RaceCar {
    slot: usize,
    lift: f32,
}

/// R moves to the next route (Quick Play on its grid), G switches game and free camera, O the original frame.
fn keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut race: ResMut<Race>,
    tint: Res<Tint>,
    camera: Single<(&Transform, &mut FreeCameraState), With<Camera3d>>,
) {
    if keys.just_pressed(KeyCode::KeyR) {
        race.current = (race.current + 1) % race.routes.len();
        let setup = RaceSetup::grid(
            &tint.rom,
            race.route(),
            PLAYER_CAR,
            game::REFERENCE_RAND,
            |s, x, z| floor_at(&tint.sectors, s, x, z),
        );
        (race.setup, race.active, race.from_dump, race.game_camera) = (setup, true, false, true);
        let r = race.route();
        info!(
            "route {}: {} waypoints, {} units",
            race.current,
            r.waypoints.len(),
            r.waypoints.last().map_or(0, |w| w.distance)
        );
    }
    if keys.just_pressed(KeyCode::KeyG) && race.active {
        race.game_camera = !race.game_camera;
    }
    if keys.just_pressed(KeyCode::KeyO) && race.active {
        race.original = !race.original;
        race.game_camera |= race.original;
    }
    race.original &= race.game_camera;
    let (t, mut state) = camera.into_inner();
    if state.enabled == race.game_camera {
        // Hand over to the free camera from where the game camera was.
        (state.yaw, state.pitch, _) = t.rotation.to_euler(EulerRot::YXZ);
        state.velocity = Vec3::ZERO;
        state.enabled = !race.game_camera;
    }
}

/// The game camera: one `camera_update` (chase view) per frame for the player, the viewer camera set to its eye
/// and view direction, and the portal pass from its camera sector.
fn game_camera(mut race: ResMut<Race>, tint: Res<Tint>, mut camera: Single<&mut Transform, With<Camera3d>>) {
    if !race.game_camera {
        (race.frame, race.visible) = (None, None);
        return;
    }
    let player = *race.player();
    let (frame, root) = race.setup.chase.step(&tint.rom, &player);
    **camera = game::frame_transform(&frame, |x, y, z| world(x, y, z));
    race.visible = Some(render::visible_sectors(&tint.rom, &frame, root));
    race.frame = Some((frame, root));
}

/// What is drawn: in game-camera mode the city through the game's visible list (the entries whose sector
/// `draw_sector` draws: `transform_walls` finds no corner deeper than 0x5FFF), each clipped to its screen span in
/// the shader; the race cars on their frame positions, those with a matrix slot whose sector is listed.
/// NOT 1:1 (R10): hidden surfaces come from the depth buffer; the game overdraws in list order (painter's).
fn visibility(
    race: Res<Race>,
    tint: Res<Tint>,
    mut images: ResMut<Assets<Image>>,
    mut city: Query<&mut Visibility, (With<CityMesh>, Without<RaceCar>)>,
    mut cars: Query<(&RaceCar, &mut Transform, &mut Visibility), Without<CityMesh>>,
) {
    let drawn: Option<Vec<render::Portal>> = race.frame.as_ref().zip(race.visible.as_ref()).map(|((f, _), v)| {
        let rt = render::Runtime::default();
        v.portals
            .iter()
            .filter(|p| p.flags & 8 == 0 && render::transform_walls(&tint.rom, f, &rt, p.sector).is_some())
            .copied()
            .collect()
    });
    if let Some(mut image) = images.get_mut(&race.portals) {
        let texels = portal_texels(drawn.as_deref());
        if image.data.as_ref() != Some(&texels) {
            image.data = Some(texels);
        }
    }
    let shown = |v: bool| if v { Visibility::Inherited } else { Visibility::Hidden };
    for mut v in &mut city {
        v.set_if_neq(shown(!race.original));
    }
    for (car, mut t, mut v) in &mut cars {
        let r = &race.setup.racers[car.slot];
        let mut at = world_fixed(r.pos);
        if !race.from_dump {
            // NOT 1:1 (D2): on the grid the model's lowest point stands on the floor.
            at.y += car.lift;
        }
        let pose = Transform::from_translation(at).looking_to(direction(r.heading), Vec3::Y);
        t.set_if_neq(pose);
        let listed = drawn.as_ref().is_none_or(|d| d.iter().any(|p| p.sector == r.sector));
        v.set_if_neq(shown(race.active && !race.original && r.slot != 0xFF && listed));
    }
}

/// The current racing line.
fn racing_line(race: Res<Race>, mut gizmos: Gizmos) {
    if race.active && !race.original {
        let line: Vec<Vec3> = race.route().waypoints.iter().map(|w| race.at(w) + Vec3::Y).collect();
        gizmos.linestrip(line, Color::srgb(1.0, 0.2, 0.1));
    }
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
/// (`apply_sector_light_to_palette`) derives palette RAM from it, tinted by the light interpolated at the player's
/// position in the camera sector (`0x03005614`; `rom::sector_light`); the next frame's shade restores the raw
/// glass (`paint::race_palette`). When no light is found the palette stays as it was. Without a race the observer
/// is the free camera.
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
    let sector = if race.active {
        // The player's position in the camera's sector, as the game has it.
        Some(race.setup.chase.sector as usize)
    } else {
        let t = camera.translation;
        let (px, pz, y) = ((t.x / SCALE).floor() as i32, (-t.z / SCALE).floor() as i32, t.y);
        // NOT 1:1 (free camera only): the observer's sector by position, keeping the current one while it still
        // contains the point, else the containing sector whose floor is nearest the eye (sectors overlap).
        tint.sector.filter(|&s| contains(&tint.sectors[s], px, pz)).or_else(|| {
            let near = |s: &usize| (y - race.floors[*s]).abs();
            (0..tint.sectors.len())
                .filter(|&s| contains(&tint.sectors[s], px, pz))
                .min_by(|a, b| near(a).total_cmp(&near(b)))
        })
    };
    let (px, pz) = if race.active {
        (race.player().pos[0] >> 8, race.player().pos[2] >> 8)
    } else {
        let t = camera.translation;
        ((t.x / SCALE).floor() as i32, (-t.z / SCALE).floor() as i32)
    };
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
    let setup = &race.setup;
    let mut base = race_base(&tint.rom, &tint.raw, setup);
    [base[192], base[208]] = paint::glass_shades(&tint.rom, setup.record[5], setup.racers[0].heading);
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
    /// 240×160 indices: `draw_skyline`'s rows on a cleared screen (plus `draw_world` in the original frame).
    screen: Vec<u8>,
    screen_image: Handle<Image>,
    /// Palette entry 0 per screen line (160×1 RGBA).
    backdrop: Handle<Image>,
    /// Environment, camera and (original frame) render camera of the last drawn sky.
    drawn: Option<(usize, sky::SkyCamera, Option<([i32; 12], u16)>)>,
}

/// K switches to the next environment (sky and palette). Redraws the sky layer when the environment or the
/// camera changes: the skyline (`sky::draw_skyline`, only when the portal pass saw an open sky, world `+0xF6`)
/// and the backdrop colour of every screen line (`gradient_buffer[backdrop_entry(gradient_start, y)]`, palette
/// entry 0, which the light tint leaves alone). In the original frame `render::draw_world` then draws the world
/// over it, as the game does into its back buffer.
///
/// NOT 1:1 (R6): the game clears only whole 32-byte blocks above the skyline and never clears below it, so a few
/// bytes keep the page's previous frame (none in the chase view); the viewer starts from a cleared screen.
/// NOT 1:1 (R12, original frame): the cars (pass 1) are not drawn.
fn sky(
    keys: Res<ButtonInput<KeyCode>>,
    mut skies: ResMut<Skies>,
    mut tint: ResMut<Tint>,
    race: Res<Race>,
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
    let cam = match race.frame {
        // The chase camera looks along 0x03000214 and never pitches (horizon 0).
        Some(_) => sky::SkyCamera {
            yaw: race.setup.chase.look,
            view: game::CHASE as u32,
            ..default()
        },
        None => sky::SkyCamera {
            yaw: game_yaw(forward),
            horizon: horizon(forward),
            view: game::CHASE as u32,
            ..default()
        },
    };
    let world_frame = race.frame.filter(|_| race.original);
    let key = (skies.current, cam, world_frame.map(|(f, r)| (f.camera, r.sector)));
    if skies.drawn == Some(key) {
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
    if race.visible.as_ref().is_none_or(|v| v.sky) {
        sky::draw_skyline(&tint.rom, desc, &cam, SKY_CLIP, screen);
    }
    if let (Some((frame, _)), Some(visible)) = (world_frame, race.visible.as_ref()) {
        render::draw_world(
            &tint.rom,
            &frame,
            &render::Runtime::default(),
            &mut visible.clone(),
            screen,
        );
    }
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
    skies.drawn = Some(key);
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

    /// The game camera's transform looks where the render frame looks: every point projects to the same screen x.
    #[test]
    fn frame_transform_matches_the_render_camera() {
        let camera = [18, 0, 16383, 0, 16384, 0, -16383, 0, 18, -118039, -30, 64321];
        let frame = render::Frame {
            view: render::View {
                cx: 120,
                cy: 79,
                near: 64,
                focal: 150,
            },
            camera,
            rect: [0, 240, 0, 159],
        };
        let t = game::frame_transform(&frame, |x, y, z| world(x, y, z));
        let view = t.to_matrix().inverse();
        for (x, y, z) in [(118400, 164, -64320), (120000, -300, -65000), (119000, 0, -63000)] {
            let v = view.transform_point3(world(x as f32, y as f32, z as f32)) / SCALE;
            let (cx, depth) = frame.to_camera(x, z);
            assert!((v.x - cx as f32).abs() < 1.5 && (-v.z - depth as f32).abs() < 1.5, "{x} {z}: {v}");
            assert!((-v.y - (y - 30) as f32).abs() < 0.01, "height: {v}");
        }
    }
}
