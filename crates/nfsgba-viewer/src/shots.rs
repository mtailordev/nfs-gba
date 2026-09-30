//! R27, the visual check: the GPU view rendered headlessly at exactly 240×160 (the viewer's own scene and systems,
//! the camera aimed at an image instead of a window) against the exact frame (`render::draw_world` on the same
//! game state), per pixel and per object. The exact frame is redrawn on the CPU with one object left out at a time
//! (an entity's material set to 0, a portal entry skipped), so an object's pixels are the ones that change: where
//! the renderer drew it, not a guess from colours. See `docs/engine/viewer-rendering.md`, "Visual check".

use std::sync::{Arc, Mutex};

use bevy::{
    audio::AudioPlugin,
    log::LogPlugin,
    prelude::*,
    render::{
        RenderApp, pipelined_rendering::PipelinedRenderingPlugin,
        render_resource::{CachedPipelineState, PipelineCache, TextureFormat, TextureUsages},
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
    window::ExitCondition,
    winit::WinitPlugin,
};
use nfsgba_formats::render;
use nfsgba_game::{
    Game, race_init,
    race_setup::{Display, Setup},
    trace::Trace,
    view,
};

use crate::{Offscreen, Race, Skies, Start, Tint, add_viewer, play};

const W: usize = 240;
const H: usize = 160;

/// A headless viewer: the viewer's systems, the camera rendering into a 240×160 image, no window.
pub struct Rig {
    app: App,
    shots: Arc<Mutex<Vec<Vec<u8>>>>,
    /// A real frame has been seen (before that, a frame with few colours is the pipelines still compiling).
    warm: bool,
    target: Handle<Image>,
}

impl Rig {
    /// Starts the viewer on `first` (its racers' models and textures are made now, so every state shown later
    /// must be from the same race).
    pub fn new(first: play::Play) -> Rig {
        let mut app = App::new();
        app.add_plugins(
            DefaultPlugins
                .build()
                .disable::<WinitPlugin>()
                .disable::<AudioPlugin>()
                .disable::<LogPlugin>()
                .disable::<PipelinedRenderingPlugin>()
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    close_when_requested: false,
                    ..default()
                }),
        );
        let mut image = Image::new_target_texture(W as u32, H as u32, TextureFormat::Rgba8UnormSrgb, None);
        image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        let target = app.world_mut().resource_mut::<Assets<Image>>().add(image);
        app.insert_resource(Offscreen(target.clone()))
            .insert_resource(Start(Some(first)));
        add_viewer(&mut app);
        app.finish();
        app.cleanup();
        // The startup systems; the HUD layer (the game's sprites) is left out of the comparison.
        app.update();
        let huds: Vec<Entity> = app.world_mut().query_filtered::<Entity, With<ImageNode>>().iter(app.world()).collect();
        for e in huds {
            app.world_mut().despawn(e);
        }
        Rig {
            app,
            shots: Arc::default(),
            warm: false,
            target,
        }
    }

    /// Every render pipeline requested so far is built (they compile in the background).
    fn pipelines_built(&self) -> bool {
        let cache = self.app.get_sub_app(RenderApp).unwrap().world().resource::<PipelineCache>();
        let mut all = cache.pipelines().peekable();
        all.peek().is_some() && all.all(|p| matches!(p.state, CachedPipelineState::Ok(_) | CachedPipelineState::Err(_)))
    }

    /// The GPU view of `play` (paused: shown at the game's own frame), 240×160 RGBA8, once it has stopped changing
    /// (the first frames render while pipelines are still compiling).
    pub fn show(&mut self, mut play: play::Play) -> Vec<u8> {
        play.hud = self.app.world().resource::<Race>().hud.clone();
        self.app.world_mut().insert_resource(play);
        for _ in 0..8 {
            self.app.update();
        }
        self.shots.lock().unwrap().clear();
        for _ in 0..600 {
            let shots = self.shots.clone();
            self.app
                .world_mut()
                .spawn(Screenshot::image(self.target.clone()))
                .observe(move |c: On<ScreenshotCaptured>| {
                    shots.lock().unwrap().push(c.image.data.clone().expect("screenshot data"));
                });
            self.app.update();
            std::thread::sleep(std::time::Duration::from_millis(3));
            let s = self.shots.lock().unwrap();
            if let [.., a, b, c] = &s[..] {
                let colours = a.chunks(4).collect::<std::collections::HashSet<_>>().len();
                if a == b && b == c && self.pipelines_built() && (self.warm || colours > 40) {
                    self.warm = true;
                    return a.clone();
                }
            }
        }
        panic!("the GPU view never settled");
    }
}

/// The game at the entry of traced frame `k` (a trace that begins at the race start has no typed state to load
/// there: `race_init::start` builds it, as in `nfsgba-game`'s replay test).
pub fn game_at(rom: &[u8], trace: &Trace, k: usize) -> Game {
    let m = trace.machine(rom, k);
    if m.mem.iwram[0x5808..0x580C] != 4u32.to_le_bytes() {
        return Game::new(m);
    }
    let display = Display {
        palette: m.palette.clone(),
        vram: m.vram.clone(),
        oam: m.oam.clone(),
        io: [0; 0x400],
    };
    race_init::start(rom.to_vec(), &Setup::load(&m), display, trace.timing[k].seed.unwrap()).unwrap()
}

/// One object of the exact frame: what it is and the pixels it owns.
pub struct Object {
    pub name: String,
    pub pixels: Vec<usize>,
    /// Share of its pixels whose exact colour shows in the GPU view within one pixel.
    pub present: f32,
}

#[derive(Default)]
pub struct Report {
    /// Share of the 38,400 pixels where the GPU view has the exact frame's colour.
    pub exact: f32,
    /// The same, allowing one pixel of displacement (the exact colour is among the 3×3 GPU neighbours).
    pub near: f32,
    pub objects: Vec<Object>,
    /// Interior pixels of the exact frame's geometry (the 5×5 block around it is geometry too) where the GPU view
    /// shows only the sky: gaps and seams.
    pub holes: Vec<usize>,
    /// The holes that are at most two pixels thin between GPU geometry (a hairline seam), not an edge shifted.
    pub seams: Vec<usize>,
    pub covered: usize,
    /// The exact frame in RGB.
    pub picture: Vec<Rgb>,
}

type Rgb = [u8; 3];

/// Compares the GPU view `gpu` with the exact frame of the state the rig shows.
pub fn compare(rig: &Rig, rom: &[u8], gpu: &[u8]) -> Report {
    let world = rig.app.world();
    let (play, tint, skies, images) = (
        world.resource::<play::Play>(),
        world.resource::<Tint>(),
        world.resource::<Skies>(),
        world.resource::<Assets<Image>>(),
    );
    let data = |h: &Handle<Image>| images.get(h).and_then(|i| i.data.clone()).expect("image data");
    let (palette, backdrop) = (data(&tint.palette), data(&skies.backdrop));
    let colour = |idx: u8, p: usize| -> Rgb {
        let at = if idx == 0 { 4 * (p / W) } else { 4 * idx as usize };
        let src = if idx == 0 { &backdrop } else { &palette };
        [src[at], src[at + 1], src[at + 2]]
    };
    let state = &play.game.world;
    let frame = view::frame(state);
    let visible = render::visible_sectors(rom, &frame, view::root(state));
    // `skies.screen` holds the skyline alone: the GPU view was not the original-resolution one.
    let draw = |empty: bool, leave_out: Option<usize>, skip: Option<usize>| -> Vec<u8> {
        let mut screen = skies.screen.clone();
        let mut scene = if empty { render::Scene::empty() } else { view::scene(state) };
        if let Some(i) = leave_out {
            scene.entities[i].material = 0;
        }
        let mut vis = visible.clone();
        if let Some(p) = skip {
            vis.portals[p].flags |= 8;
        }
        render::draw_world(rom, &frame, &tint.rt, &mut scene, &mut vis, &mut screen);
        screen
    };
    let full = draw(false, None, None);
    let exact: Vec<Rgb> = (0..W * H).map(|p| colour(full[p], p)).collect();
    let gpu_alpha: Vec<u8> = gpu.chunks(4).map(|c| c[3]).collect();
    let gpu: Vec<Rgb> = gpu.chunks(4).map(|c| [c[0], c[1], c[2]]).collect();
    let around = |p: usize, r: i32| {
        let (x, y) = ((p % W) as i32, (p / W) as i32);
        (-r..=r).flat_map(move |dy| (-r..=r).map(move |dx| (x + dx, y + dy))).filter_map(|(x, y)| {
            ((0..W as i32).contains(&x) && (0..H as i32).contains(&y)).then_some(y as usize * W + x as usize)
        })
    };
    let seen = |p: usize| around(p, 1).any(|q| gpu[q] == exact[p]);

    let mut report = Report {
        exact: (0..W * H).filter(|&p| gpu[p] == exact[p]).count() as f32 / (W * H) as f32,
        near: (0..W * H).filter(|&p| seen(p)).count() as f32 / (W * H) as f32,
        picture: exact.clone(),
        ..Default::default()
    };
    // Geometry: the exact frame drew something over the skyline. Row 159 is left out: the game draws it only when a
    // wall's bottom edge lies within row 158, which a continuous surface does not reproduce (R27).
    let is_geometry = |p: usize| full[p] != skies.screen[p];
    let sky_shown = |p: usize| gpu_alpha[p] < 255;
    for p in 0..W * H {
        if is_geometry(p) {
            report.covered += 1;
            if p / W < H - 1 && sky_shown(p) && around(p, 2).all(is_geometry) {
                report.holes.push(p);
                let solid = |dx: i32, dy: i32| {
                    let (x, y) = ((p % W) as i32 + dx, (p / W) as i32 + dy);
                    (0..W as i32).contains(&x) && (0..H as i32).contains(&y) && !sky_shown(y as usize * W + x as usize)
                };
                let thin = |dx: i32, dy: i32| (solid(-dx, -dy) || solid(-2 * dx, -2 * dy)) && (solid(dx, dy) || solid(2 * dx, 2 * dy));
                if thin(1, 0) || thin(0, 1) {
                    report.seams.push(p);
                }
            }
        }
    }
    let mut object = |name: String, other: Vec<u8>, base: &[u8], keep: &dyn Fn(usize) -> bool| {
        let pixels: Vec<usize> = (0..W * H).filter(|&p| other[p] != base[p] && keep(p)).collect();
        if !pixels.is_empty() {
            let present = pixels.iter().filter(|&&p| seen(p)).count() as f32 / pixels.len() as f32;
            report.objects.push(Object { name, pixels, present });
        }
    };
    // Entities (racers, traffic, ...): the entity's material set to 0 is not sorted or drawn.
    for (i, e) in view::scene(state).entities.iter().enumerate() {
        if e.material == 0 || e.slot == 0xFF {
            continue;
        }
        let kind = if state.slots[i].block.is_some() { "traffic" } else { "car" };
        object(format!("{kind} entity {i} (slot {})", e.slot), draw(false, Some(i), None), &full, &|_| true);
    }
    // Portal entries' walls and flats: the world alone, one entry skipped.
    let world_only = draw(true, None, None);
    for (k, p) in visible.portals.iter().enumerate().filter(|(_, p)| p.flags & 8 == 0) {
        object(format!("sector {} (entry {k})", p.sector), draw(true, None, Some(k)), &world_only, &|p| full[p] == world_only[p]);
    }
    for &p in report.seams.iter().step_by(4).take(6) {
        let who = |q: usize| -> Vec<String> {
            report.objects.iter().filter(|o| o.pixels.contains(&q)).map(|o| o.name.clone()).collect()
        };
        eprintln!("      seam ({}, {}) idx {}: {:?}; above {:?}; below {:?}", p % W, p / W, full[p], who(p), who(p - W), who(p + W));
    }
    report
}

#[test]
fn scratch_sector() {
    let Some(rom) = nfsgba_testkit::rom() else { return };
    let s = &nfsgba_formats::city(&rom)[760];
    for (k, w) in s.walls.iter().enumerate() {
        eprintln!("{k}: x {} z {} floor {} ceil {} top {:?} bottom {:?} flags {:x} link {} mat {}", w.x, w.z, w.floor_y, w.ceiling_y, w.top, w.bottom, w.flags, w.link, w.material);
    }
}

/// GPU view, exact frame and their difference (holes red, other mismatches grey), side by side at 2×.
fn save(path: &std::path::Path, gpu: &[u8], r: &Report) {
    use bevy::render::render_resource::{Extent3d, TextureDimension};
    let scale = 2;
    let (w, h) = (3 * W * scale, H * scale);
    let mut out = vec![255u8; 4 * w * h];
    for y in 0..h {
        for x in 0..w {
            let (panel, px, py) = (x / (W * scale), x / scale % W, y / scale);
            let p = py * W + px;
            let g = [gpu[4 * p], gpu[4 * p + 1], gpu[4 * p + 2]];
            let rgb = match panel {
                0 => g,
                1 => r.picture[p],
                _ if r.holes.contains(&p) => [255, 0, 0],
                _ if g != r.picture[p] => [90, 90, 90],
                _ => [0, 0, 0],
            };
            out[4 * (y * w + x)..][..3].copy_from_slice(&rgb);
        }
    }
    let size = Extent3d {
        width: w as u32,
        height: h as u32,
        ..default()
    };
    let image = Image::new(
        size,
        TextureDimension::D2,
        out,
        TextureFormat::Rgba8UnormSrgb,
        default(),
    );
    image.try_into_dynamic().unwrap().save(path).unwrap();
}

/// (session, trace): the recorded runs of `nfsgba-game`'s replay test.
const TRACES: [(&str, &str); 8] = [
    ("live-race", "start"),
    ("game-loop", "fadeout"),
    ("game-loop", "fadein"),
    ("game-loop", "drive"),
    ("live-race", "live"),
    ("live-race", "trail"),
    ("live-race", "views"),
    ("live-race", "nitro"),
];

/// Game frames shown from each trace, spread evenly (the first and the last included).
const PER_TRACE: usize = 6;

#[test]
fn gpu_view_matches_the_exact_frame() {
    let Some(rom) = nfsgba_testkit::rom() else { return };
    let only = std::env::var("SHOTS_TRACE").ok();
    let (mut n, mut exact, mut near) = (0, 0.0, 0.0);
    for (session, name) in TRACES {
        if only.as_deref().is_some_and(|o| o != name) {
            continue;
        }
        let (Some(dir), Some(_)) = (
            nfsgba_testkit::fixture(session),
            nfsgba_testkit::fixture(&format!("{session}/{name}.base.bin")),
        ) else {
            continue;
        };
        let trace = Trace::load(&dir, name).unwrap();
        let frames = trace.timing.len();
        let states = |k| play::Play::new(game_at(&rom, &trace, k), Handle::default(), true, None);
        let mut rig = Rig::new(states(0));
        for k in (0..PER_TRACE).map(|i| i * (frames - 1) / (PER_TRACE - 1)) {
            let gpu = rig.show(states(k));
            let r = compare(&rig, &rom, &gpu);
            let worst = r.objects.iter().map(|o| o.present).fold(1.0, f32::min);
            eprintln!(
                "{name} {k:>3}: exact {:.1}% near {:.1}%, {} objects (worst {:.0}% present), {} holes ({} seam pixels) of {} covered",
                100.0 * r.exact,
                100.0 * r.near,
                r.objects.len(),
                100.0 * worst,
                r.holes.len(),
                r.seams.len(),
                r.covered
            );
            let mut rows = [0; H];
            r.holes.iter().for_each(|p| rows[p / W] += 1);
            let heavy: Vec<_> = rows.iter().enumerate().filter(|(_, n)| **n >= 10).collect();
            if !heavy.is_empty() {
                eprintln!("    hole rows (row, count): {heavy:?}");
            }
            for o in r.objects.iter().filter(|o| o.present < 0.8) {
                eprintln!("    {}: {} px, {:.0}% present", o.name, o.pixels.len(), 100.0 * o.present);
            }
            if let Ok(dir) = std::env::var("SHOTS_DIR") {
                save(&std::path::Path::new(&dir).join(format!("{name}-{k}.png")), &gpu, &r);
            }
            n += 1;
            exact += r.exact;
            near += r.near;
        }
    }
    eprintln!("{n} states: mean exact {:.1}%, near {:.1}%", 100.0 * exact / n as f32, 100.0 * near / n as f32);
}
