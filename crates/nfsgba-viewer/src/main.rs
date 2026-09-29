//! Fly-through viewer for Carbon's city (portal/sector world) and vehicle model bank, read from the user's ROM:
//! textured walls, floors and ceilings, the race sky, a race's four cars, and a showroom of all 15 cars in every
//! paint variant. Run from the repo root: `cargo run --release -p nfsgba-viewer`.
//!
//! Colour works like the GBA's: textures are palette indices, drawn through one 256-colour palette that holds the
//! city and the race cars and that the game's light tint rewrites every frame; behind the world the window shows
//! the GBA's 240×160 sky layer (backdrop gradient per line, skyline panorama). The window is the GBA screen
//! scaled up, projected as the game projects (`game::GbaProjection`). Every race is an `nfsgba_game::Game` (`play.rs`, a
//! route's race start, a dump, or play): the camera is the game's chase
//! camera and only what the game's portal pass lists is drawn (`render::visible_sectors`, clipped to each
//! entry's screen span); O swaps the GPU view for the game's own 240×160 frame (`render::draw_world`). See
//! `docs/engine/viewer-rendering.md`.
//!
//! Controls: R moves to the next race route (a new race start; K in a race: the next environment); G switches
//! between the game camera and the free camera (Bevy `FreeCamera`: hold right mouse to look, M toggles, WASD move,
//! Q/E down/up, Shift run, scroll wheel speed); O toggles the original-resolution frame; K switches environment.
//! `NFSGBA_ROUTE=<n>` starts a Quick Play race start on route n (paused, G1); `NFSGBA_DUMP=<dir/name>` starts from a race dump
//! under `$NFSGBA_DATA/work/e5298b24/` (e.g. `mgba/race`, the reference race); `NFSGBA_ORIGINAL=1` starts in the
//! original-resolution frame; `NFSGBA_ENV=<n>` picks the environment; `NFSGBA_CAM=x,y,z,tx,ty,tz` sets the free
//! camera's start eye and target (metres); `NFSGBA_SHOT=<file.png>` saves one frame and quits.
//! `NFSGBA_PLAY=1` with `NFSGBA_DUMP` runs the game itself from that machine state (`nfsgba_game::Game`, `play.rs`):
//! the keyboard drives the race, the HUD is the game's OAM, and O shows the GBA screen as the game composes it.
//! The dump must be taken at `main_frame`'s entry with palette, VRAM and OAM (e.g. a `tools/game_trace.py` state).

mod game;
mod play;

use std::{collections::BTreeMap, f64::consts::TAU};

use bevy::{
    asset::{RenderAssetUsages, embedded_asset},
    audio::AddAudioSource,
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
use game::GbaProjection;
use nfsgba_formats::{self as rom, atlas, paint, render, sky};
use nfsgba_game::view;

/// Raw units to metres. The engine draws cars and city in one unit (vehicle matrices are pure rotations,
/// translations are city positions), and all 15 car models measure ~48 units per real-world metre, so the
/// city comes out exaggerated: streets ~40 m wide, blocks ~50 m tall.
const SCALE: f32 = 1.0 / 48.0;

/// The skyline's clip rows (view rect `0x030053D0` `+4`/`+0xC` in the race).
const SKY_CLIP: [i32; 2] = [0, 159];

/// Raw space (x right, y down, z forward) to Bevy (y up, -z forward): a 180° turn about x, no mirroring.
fn world(x: impl Into<f32>, y: impl Into<f32>, z: impl Into<f32>) -> Vec3 {
    Vec3::new(x.into(), -y.into(), -z.into()) * SCALE
}

/// The camera yaw (`0x03000214`; 0x4000 per turn, 0 faces raw +z and 0x1000 faces +x) of a view direction.
fn game_yaw(forward: Vec3) -> i32 {
    (f64::from(forward.x).atan2(-f64::from(forward.z)) * 16384.0 / TAU).round() as i32
}

/// The horizon shift (`0x030056B8`, screen rows): how far below the screen centre the eye-level horizon falls,
/// `focal · tan(pitch)`, clamped to ±32 as `camera_update` does. The game's cameras never pitch (0 in the chase
/// view; the bumper view shifts the projection centre instead); only the viewer's free camera does.
fn horizon(forward: Vec3) -> i32 {
    (game::FOCAL as f32 * forward.y / forward.xz().length())
        .round()
        .clamp(-32.0, 32.0) as i32
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

    /// A city polygon of `sector`, tagged for the portal clip: `wall` is the wall's index in its sector, `None` for
    /// a floor or ceiling.
    fn city(&mut self, pts: &[(Vec3, Vec2)], sector: usize, wall: Option<usize>) {
        let n = self.pos.len();
        self.fan(pts, Color::WHITE);
        self.uv_b.resize(self.pos.len(), [0.0; 2]);
        self.uv_b[n..].fill([sector as f32, wall.map_or(0.0, |k| k as f32 + 1.0)]);
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
    /// The game's visible-sector list and wall masks (32×2 `Rgba32Sint`, see `portal_texels`).
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

/// The portal texture (32×2): row 0 texel 0 holds the entry count (−1: no list, draw everything), texels 1.. the
/// drawn entries as (sector, left, right, top | bottom << 16); row 1 under each entry the mask of the sector's walls
/// the game draws through it (bit k of the 128-bit mask: wall k).
fn portal_texels(entries: Option<&[(render::Portal, u128)]>) -> Vec<u8> {
    let mut texels = vec![[0i32; 4]; 64];
    match entries {
        None => texels[0][0] = -1,
        Some(list) => {
            texels[0][0] = list.len() as i32;
            for (k, (p, walls)) in list.iter().enumerate().take(31) {
                texels[1 + k] = [
                    p.sector as i32,
                    p.left as i32,
                    p.right as i32,
                    p.top as i32 | (p.bottom as i32) << 16,
                ];
                texels[33 + k] = std::array::from_fn(|i| (walls >> (32 * i)) as u32 as i32);
            }
        }
    }
    texels.iter().flatten().flat_map(|v| v.to_le_bytes()).collect()
}

/// The walls of `portal`'s sector that the game draws through that entry, as a mask (bit k: wall k): the checks of
/// `draw_sector_walls` (material 0, open portal, span flag 4, deferred walls beyond the 8-bit mask, rows outside
/// the entry) and the early returns of `raster_wall_columns` (fewer than 2 column pairs, before or after clipping
/// to the entry's pairs `left >> 1 .. right >> 1`), on the spans `render::setup_wall_spans` made for the entry.
/// A moving piece's flags replace the wall's (`render::Runtime::wall_flags`); its material offset is 0 in every race seen.
fn walls_drawn(rt: &render::Runtime, portal: &render::Portal, spans: &[render::WallSpan], walls: &[rom::Wall]) -> u128 {
    let (e6, e8) = (portal.top as u16 as i32, portal.bottom as u16 as i32);
    let (left, right) = ((portal.left as u16 >> 1) as i32, (portal.right as u16 >> 1) as i32);
    let mut mask = 0;
    for (i, (s, w)) in spans.iter().zip(walls).enumerate() {
        let k = walls.len() - i;
        let visible = s.flags & 4 == 0 && (s.flags & 2 == 0 || k < 8);
        let rows =
            (s.top[0] as i32) < e8 || (s.top[1] as i32) < e8 || e6 <= s.bottom[0] as i32 || e6 <= s.bottom[1] as i32;
        let columns = || {
            let (mut start, end) = (s.x[0] as i32 >> 1, (s.x[1] as i32 + 1) >> 1);
            let mut cols = end - start;
            if cols <= 1 {
                return false;
            }
            if start < left {
                if end < left {
                    return false;
                }
                (start, cols) = (left, end - left);
            }
            if right < end {
                if right < start {
                    return false;
                }
                cols = right - start;
            }
            cols > 1
        };
        if w.material != 0 && rt.wall_flags(w.piece, w.flags) & 1 == 0 && visible && rows && columns() {
            mask |= 1u128 << i;
        }
    }
    mask
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
    .add_audio_source::<play::GbaSound>()
    .add_systems(Startup, setup)
    .add_systems(PostStartup, play::start_sound)
    .add_systems(
        Update,
        (
            shot,
            (play::play, keys, game_camera, visibility, sky, tint).chain(),
            racing_line,
            play::hud_layer,
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
    let portals = images.add(texture_2d(32, 2, portal_texels(None), TextureFormat::Rgba32Sint));
    let most_walls = sectors.iter().map(|s| s.walls.len()).max().unwrap_or(0);
    assert!(
        most_walls <= 128,
        "a sector has {most_walls} walls; the portal texture's wall masks hold 128"
    );
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
    // index 8 and up (`draw_sector_walls`). A moving piece's flags replace its wall's (in races all 122 are open,
    // `game::race_runtime`). Flats from each corner's floor/ceiling height (`+0x38`/`+0x3A`, R19), both faces.
    let rt = game::race_runtime(&sectors);
    let mut by_material: BTreeMap<u16, Tris> = BTreeMap::new();
    let floor_uv = |w: &rom::Wall| Vec2::new(w.floor_uv[0] as f32, w.floor_uv[1] as f32) / 16384.0;
    let (mut solid, mut portal, mut backs, mut lost) = (0, 0, 0, 0);
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
                tris.city(&pts, s, None);
                pts.reverse();
                tris.city(&pts, s, None);
            }
        }
        for (k, a) in w.iter().enumerate() {
            let flags = rt.wall_flags(a.piece, a.flags);
            if a.material == 0 || flags & 1 != 0 {
                continue;
            }
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
            if a.link < 0 {
                solid += 1
            } else {
                portal += 1
            }
            if flags & 2 == 0 {
                tris.city(&front, s, Some(k));
            }
            let countdown = w.len() - k;
            if flags & 0x2002 != 0 {
                if countdown < 8 {
                    let back: Vec<_> = front.iter().rev().copied().collect();
                    tris.city(&back, s, Some(k));
                    backs += 1;
                } else {
                    lost += 1;
                }
            }
        }
    }
    info!(
        "walls drawn: {solid} solid, {portal} portal (steps, kerbs, fences); {backs} with a back side, {lost} backs lost to the 8-bit defer mask"
    );
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
    // The HUD layer: the game's OAM over the window (empty until the game draws sprites).
    let hud = images.add(play::hud_image());
    commands.spawn((
        ImageNode::new(hud.clone()),
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
    ));
    let dump = std::env::var("NFSGBA_DUMP").ok();
    let start_route: Option<u32> = std::env::var("NFSGBA_ROUTE").ok().and_then(|s| s.parse().ok());
    let running = std::env::var("NFSGBA_PLAY").is_ok();
    let race_game = match &dump {
        Some(prefix) => play::Play::load(data.clone(), prefix, hud.clone(), running)
            .unwrap_or_else(|e| panic!("NFSGBA_DUMP={prefix}: {e} (needs the dump's palette, vram and oam too)")),
        None => play::Play::grid(data.clone(), env as u32, start_route.unwrap_or(23), hud.clone())
            .unwrap_or_else(|e| panic!("race start: {e} (needs the race-init/circuit_pre capture)")),
    };
    let setup = view::RaceView::read(&race_game.game.world);
    let env = if dump.is_some() { setup.env % envs.len() } else { env };
    let current = setup.route % routes.len();
    let active = dump.is_some() || start_route.is_some();
    info!(
        "race on route {current}: cars {:?}, paints {:?}{}",
        setup.cars,
        setup.paints,
        match (&dump, running) {
            (Some(_), true) => " (from the dump, running)",
            (Some(_), false) => " (from the dump, paused)",
            _ => " (race start, paused)",
        }
    );
    // The player's texture as the game holds it in EWRAM (`unpack_player_atlas`, the rim); the opponents' are raw
    // materials already in their palette slots (`look`).
    let player_car = setup.cars[0] as usize;
    let player_first = &vehicle_textures[cars[player_car].first_material];
    let player_pixels = view::player_atlas(&race_game.game.world, player_first.pixels.len())
        .unwrap_or_else(|| player_first.pixels.clone());
    for (slot, racer) in setup.racers.iter().enumerate().filter(|(_, r)| r.model > 0) {
        let atlas = if slot == 0 {
            rom::Texture {
                pixels: player_pixels.clone(),
                ..player_first.clone()
            }
        } else {
            vehicle_textures[atlas::look(&data, setup.cars, slot, false, false).material as usize].clone()
        };
        let material = new_material(images.add(index_image(&atlas)), &palette, 0);
        // Every model the racer can show (near body `+0x36 − 1`, far body `+0x36`, spoiler `+0x64`); `visibility`
        // picks by depth as `draw_sector_entities` does.
        let mut parts = vec![racer.model as usize - 1, racer.model as usize];
        if racer.extra != 0 {
            parts.push(racer.extra.unsigned_abs() as usize);
        }
        for model in parts {
            let tris = model_tris(&models[model], Some(&atlas), Color::WHITE);
            commands.spawn((
                Mesh3d(meshes.add(tris.mesh())),
                MeshMaterial3d(material.clone()),
                Transform::default(),
                RaceCar { slot, model },
            ));
        }
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
    if running {
        info!("play mode: arrows, X = A, Z = B, A = L, S = R, Enter = START, Backspace = SELECT");
    }
    commands.insert_resource(race_game);
    let free_cam = std::env::var("NFSGBA_CAM").is_ok();
    commands.insert_resource(Race {
        routes,
        floors,
        current,
        active,
        hud,
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
        rt,
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
            unit: SCALE,
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
    /// A race is shown: its cars stand where the game puts them and the light follows the player.
    active: bool,
    /// The HUD layer's image (a new `play::Play` draws into it).
    hud: Handle<Image>,
    /// The camera is the game's chase camera (else the free camera).
    game_camera: bool,
    /// Show the game's own 240×160 world frame (`render::draw_world`) instead of the GPU view.
    original: bool,
    /// This frame's render frame and camera-sector entry, in game-camera mode.
    frame: Option<(render::Frame, render::Portal)>,
    /// The visible-sector list of `frame` (unfiltered, as `draw_world` takes it).
    visible: Option<render::Visibility>,
    /// The game's racers, cars and route (`play::play` reads them back every frame).
    setup: view::RaceView,
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

    fn player(&self) -> &view::Racer {
        &self.setup.racers[0]
    }
}

/// A race car part: racer `slot`'s model `model`.
#[derive(Component)]
struct RaceCar {
    slot: usize,
    model: usize,
}

/// A racer's pose from its vehicle matrix slot (camera space, `render::Scene::matrices`) and the camera it was built
/// for: model vertex `v` goes to camera space as `A·v + t` (rows `(m0 m3 m6)`, `(m1 m4 m7)`, `(m2 m5 m8)` over
/// 0x4000; x right, y down, depth ahead). With `F = diag(1, −1, −1)` both the mesh (`world`) and Bevy's view space
/// are `F`-flipped, so the mesh-to-view map is `F·A·F` plus `F·t`.
fn pose_from_matrix(camera: &Transform, m: &[i32; 12]) -> Transform {
    let f = [1.0, -1.0, -1.0];
    let linear = Mat3::from_cols_array(&std::array::from_fn(|i| {
        let (c, r) = (i / 3, i % 3);
        f[r] * m[r + 3 * c] as f32 / 16384.0 * f[c]
    }));
    let t = Vec3::new(m[9] as f32, -m[10] as f32, -m[11] as f32) * SCALE;
    Transform::from_matrix(camera.to_matrix() * Mat4::from_mat3_translation(linear, t))
}

/// R moves to the next route and K, in a race start, to the next environment (a new race start through the game,
/// `play::Play::grid`); G switches game and free camera, O the original frame.
fn keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut race: ResMut<Race>,
    tint: Res<Tint>,
    skies: Res<Skies>,
    play: Res<play::Play>,
    camera: Single<(&Transform, &mut FreeCameraState), With<Camera3d>>,
) {
    let (env, route) = (race.setup.env as u32, race.setup.route as u32);
    let routes = race.routes.len() as u32;
    let start = if keys.just_pressed(KeyCode::KeyR) {
        Some((env, (1..=routes).map(|k| (route + k) % routes).collect::<Vec<_>>()))
    } else if keys.just_pressed(KeyCode::KeyK) && race.active && play.grid.is_some() {
        Some(((env + 1) % skies.palettes.len() as u32, vec![route]))
    } else {
        None
    };
    // The first route (from the next one on) whose race start the game code runs.
    if let Some((env, routes)) = start {
        for route in routes {
            match play::Play::grid(tint.rom.clone(), env, route, race.hud.clone()) {
                Ok(p) => {
                    commands.insert_resource(p);
                    (race.active, race.game_camera) = (true, true);
                    let r = &race.routes[route as usize];
                    info!(
                        "route {route}, environment {env}: {} waypoints, {} units",
                        r.waypoints.len(),
                        r.waypoints.last().map_or(0, |w| w.distance)
                    );
                    break;
                }
                Err(e) => warn!("route {route}: {e}"),
            }
        }
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

/// The game camera: the camera the game left in RAM (`camera_update`, the chase view), the viewer camera set to its
/// eye and view direction, and the game's visible-sector list from its camera sector.
fn game_camera(
    mut race: ResMut<Race>,
    tint: Res<Tint>,
    play: Res<play::Play>,
    mut camera: Single<&mut Transform, With<Camera3d>>,
) {
    if !race.game_camera {
        (race.frame, race.visible) = (None, None);
        return;
    }
    let (frame, root, visible) = play::frame(&play, &tint.rom);
    **camera = game::frame_transform(&frame, world);
    (race.frame, race.visible) = (Some((frame, root)), Some(visible));
}

/// What is drawn: in game-camera mode the city through the game's visible list (the entries whose sector
/// `draw_sector` draws: `transform_walls` finds no corner deeper than 0x5FFF), each clipped to its screen span in
/// the shader; the racers whose sector is listed, each with the models `draw_sector_entities` picks at its camera
/// depth (`Racer::models_at`). The racers stand as the game's vehicle matrix slots put them (pitch and roll
/// included), seen from the game's camera whichever camera is shown.
/// NOT 1:1 (R10): hidden surfaces come from the depth buffer; the game overdraws in list order (painter's).
/// NOT 1:1 (R29): the cars are not clipped to their portal's span and the screen-row cull is not applied.
fn visibility(
    race: Res<Race>,
    tint: Res<Tint>,
    play: Res<play::Play>,
    mut logged: Local<Vec<(render::Portal, u128)>>,
    mut images: ResMut<Assets<Image>>,
    mut city: Query<&mut Visibility, (With<CityMesh>, Without<RaceCar>)>,
    mut cars: Query<(&RaceCar, &mut Transform, &mut Visibility), Without<CityMesh>>,
) {
    let drawn: Option<Vec<(render::Portal, u128)>> =
        race.frame.as_ref().zip(race.visible.as_ref()).map(|((f, _), v)| {
            let rt = &tint.rt;
            v.portals
                .iter()
                .filter(|p| p.flags & 8 == 0)
                .filter_map(|p| {
                    let (mut spans, _) = render::transform_walls(&tint.rom, f, rt, p.sector)?;
                    render::setup_wall_spans(&tint.rom, f, rt, p, &mut spans);
                    Some((*p, walls_drawn(rt, p, &spans, &tint.sectors[p.sector as usize].walls)))
                })
                .collect()
        });
    if let Some(d) = drawn.as_ref().filter(|d| **d != *logged) {
        let list: Vec<_> = d
            .iter()
            .map(|(p, w)| (p.sector, p.left, p.right, p.depth, w.count_ones()))
            .collect();
        info!("portal list (sector, left, right, depth, walls drawn): {list:?}");
        logged.clone_from(d);
    }
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
    let mem = &play.game.world;
    let game_frame = view::frame(mem);
    let eye = game::frame_transform(&game_frame, world);
    for (car, mut t, mut v) in &mut cars {
        let r = &race.setup.racers[car.slot];
        if r.slot != 0xFF {
            t.set_if_neq(pose_from_matrix(&eye, &view::matrix(mem, r.slot)));
        }
        // The entity's camera depth as `draw_sector_entities` computes it.
        let depth = {
            let m = &game_frame.camera;
            let (x, z) = (m[9].wrapping_add(r.pos[0] >> 8), m[11].wrapping_add(r.pos[2] >> 8));
            m[2].wrapping_mul(x).wrapping_add(m[8].wrapping_mul(z)) >> 14
        };
        let listed = drawn
            .as_ref()
            .is_none_or(|d| d.iter().any(|(p, _)| p.sector == r.sector));
        let drawn_model = r.models_at(depth).contains(&car.model);
        v.set_if_neq(shown(race.active && !race.original && listed && drawn_model));
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
    /// The renderer's runtime tables as races have them (`game::race_runtime`).
    rt: render::Runtime,
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
    play: Res<play::Play>,
) {
    // Play mode: palette RAM as the game frame left it (tint, glass shades, cars).
    if !play.paused {
        let pal = &play.game.palette;
        let palette: Vec<u16> = (0..256)
            .map(|i| u16::from_le_bytes([pal[2 * i], pal[2 * i + 1]]))
            .collect();
        if palette != tint.written
            && let Some(mut image) = images.get_mut(&tint.palette)
        {
            image.data = Some(rgba(&palette));
            tint.written = palette;
        }
        return;
    }
    let sector = if race.active {
        // The player's position in the camera's sector, as the game has it.
        Some(view::root(&play.game.world).sector as usize)
    } else {
        let t = camera.translation;
        let (px, pz, y) = ((t.x / SCALE).floor() as i32, (-t.z / SCALE).floor() as i32, t.y);
        // Free camera (a non-game mode): the observer's sector by position, keeping the current one while it still
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

    // A paused race: the game's base palette (city and car ramps) with the light tint applied here; a race the game
    // steps is tinted by the game (above).
    // NOT 1:1 (R17): for 1–7 scanlines per game frame the game shows the tinted glass instead.
    let base = if race.active {
        let setup = &race.setup;
        let mut base = view::base_palette(&play.game.world);
        [base[192], base[208]] = paint::glass_shades(&tint.rom, setup.record[5], setup.racers[0].heading);
        base
    } else {
        tint.raw.clone()
    };
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
    /// What the sky layer was last drawn for.
    drawn: Option<SkyKey>,
}

/// Environment, sky camera and, in the original frame, the render camera matrix and camera sector.
type SkyKey = (usize, sky::SkyCamera, Option<([i32; 12], u16)>);

/// K switches to the next environment (sky and palette). Redraws the sky layer when the environment or the
/// camera changes: the skyline (`sky::draw_skyline`, only when the portal pass saw an open sky, world `+0xF6`)
/// and the backdrop colour of every screen line (`gradient_buffer[backdrop_entry(gradient_start, y)]`, palette
/// entry 0, which the light tint leaves alone). In the original frame `render::draw_world` then draws the world
/// over it, as the game does into its back buffer.
///
/// NOT 1:1 (R6): the game clears only whole 32-byte blocks above the skyline and never clears below it, so a few
/// bytes keep the page's previous frame (none in the chase view); the viewer starts from a cleared screen.
fn sky(
    keys: Res<ButtonInput<KeyCode>>,
    mut skies: ResMut<Skies>,
    mut tint: ResMut<Tint>,
    race: Res<Race>,
    camera: Single<&Transform, With<Camera3d>>,
    mut images: ResMut<Assets<Image>>,
    play: Res<play::Play>,
) {
    // A race has the game's environment; K there starts a new race start (`keys`).
    if race.active {
        skies.current = race.setup.env % skies.palettes.len();
    }
    // Play mode, original frame: the page the game shows, and the backdrop lines it left.
    if race.original && !play.paused {
        skies.screen.copy_from_slice(play.game.screen());
        let lines: Vec<u8> = play.game.backdrop().into_iter().flat_map(rom::bgr555).collect();
        if let Some(mut image) = images.get_mut(&skies.screen_image) {
            image.data = Some(skies.screen.clone());
        }
        if let Some(mut image) = images.get_mut(&skies.backdrop) {
            image.data = Some(lines);
        }
        skies.drawn = None;
        return;
    }
    if keys.just_pressed(KeyCode::KeyK) && !race.active {
        skies.current = (skies.current + 1) % skies.palettes.len();
        tint.raw = skies.palettes[skies.current].clone();
        tint.dirty = true;
        info!("environment {}", skies.current);
    }
    let forward = camera.forward().as_vec3();
    let cam = match race.frame {
        // The game's sky camera (the chase camera looks along its yaw and never pitches).
        Some(_) => view::sky_camera(&play.game.world),
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
        // The cars: the game's entities and matrix slots.
        let mut scene = view::scene(&play.game.world);
        render::draw_world(&tint.rom, &frame, &tint.rt, &mut scene, &mut visible.clone(), screen);
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

    /// A game angle as a direction on the ground: raw `(sin, 0, cos)`, so 0 faces +z and 0x1000 faces +x.
    fn direction(angle: i32) -> Vec3 {
        let a = f64::from(angle & 0x3FFF) * TAU / 16384.0;
        world(a.sin() as f32, 0.0_f32, a.cos() as f32).normalize()
    }

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

    /// `walls_drawn` agrees with the game's rasteriser: over the chase frames of several routes, every wall of
    /// every drawn entry that it keeps writes pixels when `render::raster_wall_columns` draws it alone, and every
    /// wall it drops (that `draw_sector_walls` would pass to the rasteriser) writes none.
    #[test]
    fn walls_drawn_matches_the_rasteriser() {
        let Some(data) = nfsgba_testkit::rom() else { return };
        let (sectors, routes, textures) = (rom::city(&data), rom::routes(&data), rom::city_textures(&data));
        let rt = game::race_runtime(&sectors);
        let (mut kept, mut dropped, mut started) = (0, 0, 0);
        // Route 0 has no grid of its own.
        for route in 1..routes.len() {
            let Ok(play) = play::Play::grid(data.clone(), 11, route as u32, Handle::default()) else {
                continue;
            };
            started += 1;
            let (frame, root, _) = play::frame(&play, &data);
            for p in render::visible_sectors(&data, &frame, root).portals {
                let Some((mut spans, _)) = render::transform_walls(&data, &frame, &rt, p.sector) else {
                    continue;
                };
                render::setup_wall_spans(&data, &frame, &rt, &p, &mut spans);
                let walls = &sectors[p.sector as usize].walls;
                let mask = walls_drawn(&rt, &p, &spans, walls);
                for (i, (s, w)) in spans.iter().zip(walls).enumerate() {
                    // Opaque textures only: a transparent one can legitimately write nothing.
                    let flags = rt.wall_flags(w.piece, w.flags);
                    let opaque = textures[w.material as usize].pixels[0] != 0;
                    if w.material == 0 || flags & 1 != 0 || s.flags & 4 != 0 {
                        // Never passed to the rasteriser.
                        assert!(mask >> i & 1 == 0, "route {route}, sector {}, wall {i}", p.sector);
                        continue;
                    }
                    if !opaque {
                        continue;
                    }
                    let mut screen = vec![0u8; render::SCREEN_WIDTH * 160];
                    render::raster_wall_columns(&data, &p, s, w.material, w.flags, &mut screen);
                    let wrote = screen.iter().any(|&b| b != 0);
                    let keep = mask >> i & 1 != 0;
                    let deferred_lost = s.flags & 2 != 0 && walls.len() - i >= 8;
                    let rows = (s.top[0] as i32) < p.bottom as i32
                        || (s.top[1] as i32) < p.bottom as i32
                        || p.top as i32 <= s.bottom[0] as i32
                        || p.top as i32 <= s.bottom[1] as i32;
                    if deferred_lost || !rows {
                        assert!(!keep, "route {route}, sector {}, wall {i}", p.sector);
                        continue;
                    }
                    assert_eq!(keep, wrote, "route {route}, sector {}, wall {i}: {s:?}", p.sector);
                    (kept, dropped) = (kept + keep as usize, dropped + !keep as usize);
                }
            }
        }
        eprintln!("{started} route starts, {kept} walls kept, {dropped} dropped, all as the rasteriser draws them");
        assert!(started > 40 && kept > 100 && dropped > 10);
    }

    /// R28: the one path. A race dump and a route's race start are both an `nfsgba_game::Game`; the viewer's camera
    /// is the game's (`play::frame`, the chase camera behind the player in the game's sector) and every racer's
    /// pose comes from its vehicle matrix slot as seen from that camera: it lands on the entity's position.
    #[test]
    fn every_race_mode_reads_the_game() {
        let Some(data) = nfsgba_testkit::rom() else { return };
        let (Some(_), Some(_)) = (
            nfsgba_testkit::dump("game-loop/s18"),
            nfsgba_testkit::fixture("race-init/circuit_pre.wram.bin"),
        ) else {
            return;
        };
        let dump = play::Play::load(data.clone(), "game-loop/s18", Handle::default(), false).unwrap();
        let route = play::Play::grid(data.clone(), 11, 7, Handle::default()).unwrap();
        assert_eq!((dump.grid, route.grid), (None, Some((11, 7))));
        for (name, p, cars) in [("dump", &dump, 1), ("route 7", &route, 1)] {
            assert!(p.paused, "{name}");
            let mem = &p.game.world;
            let (frame, root, visible) = play::frame(p, &data);
            assert_eq!(frame.camera, view::frame(mem).camera, "{name}");
            assert_eq!(visible.portals.first().map(|p| p.sector), Some(root.sector), "{name}");
            let setup = view::RaceView::read(mem);
            let eye = game::frame_transform(&frame, world);
            let player = world(
                setup.racers[0].pos[0] as f32 / 256.0,
                setup.racers[0].pos[1] as f32 / 256.0,
                setup.racers[0].pos[2] as f32 / 256.0,
            );
            // The chase camera: within 10 m of the player (the orbit distance is 344 city units, 7 m), looking at it.
            assert!(
                eye.translation.distance(player) < 10.0,
                "{name}: camera at {:?}, player {player:?} {:?}",
                eye.translation,
                setup.racers[0]
            );
            let mut posed = 0;
            for r in setup.racers.iter().filter(|r| r.slot != 0xFF) {
                let pose = pose_from_matrix(&eye, &view::matrix(mem, r.slot));
                let at = world(
                    r.pos[0] as f32 / 256.0,
                    r.pos[1] as f32 / 256.0,
                    r.pos[2] as f32 / 256.0,
                );
                assert!(pose.translation.distance(at) < 0.1, "{name}: slot {}", r.slot);
                posed += 1;
            }
            assert!(posed >= cars, "{name}: {posed} racers have a matrix slot");
        }
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
        let t = game::frame_transform(&frame, world);
        let view = t.to_matrix().inverse();
        for (x, y, z) in [(118400, 164, -64320), (120000, -300, -65000), (119000, 0, -63000)] {
            let v = view.transform_point3(world(x as f32, y as f32, z as f32)) / SCALE;
            let (cx, depth) = frame.to_camera(x, z);
            assert!(
                (v.x - cx as f32).abs() < 1.5 && (-v.z - depth as f32).abs() < 1.5,
                "{x} {z}: {v}"
            );
            assert!((-v.y - (y - 30) as f32).abs() < 0.01, "height: {v}");
        }
    }
}
