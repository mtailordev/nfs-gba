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
        RenderApp,
        pipelined_rendering::PipelinedRenderingPlugin,
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
    /// The pipeline count at the last frame and for how many frames in a row it has not changed.
    pipelines_seen: usize,
    stable_pipelines: usize,
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
        // The startup systems. The HUD layer (the game's sprites) is left out until `hud(true)`.
        app.insert_resource(play::NoHud);
        app.update();
        Rig {
            app,
            shots: Arc::default(),
            warm: false,
            target,
            pipelines_seen: 0,
            stable_pipelines: 0,
        }
    }

    /// The game's sprites are mixed into the view (G2) or left out.
    pub fn hud(&mut self, on: bool) {
        if on {
            self.app.world_mut().remove_resource::<play::NoHud>();
        } else {
            let hud = self.app.world().resource::<Race>().hud.clone();
            if let Some(mut image) = self.app.world_mut().resource_mut::<Assets<Image>>().get_mut(&hud) {
                image.data = Some(vec![0; W * H * 4]);
            }
            self.app.world_mut().insert_resource(play::NoHud);
        }
    }

    /// Every render pipeline requested so far is built (they compile in the background).
    /// Their number when they are (0: some are still compiling, or none was requested yet).
    fn pipelines_built(&self) -> usize {
        let cache = self
            .app
            .get_sub_app(RenderApp)
            .unwrap()
            .world()
            .resource::<PipelineCache>();
        let built = cache
            .pipelines()
            .all(|p| matches!(p.state, CachedPipelineState::Ok(_) | CachedPipelineState::Err(_)));
        if built { cache.pipelines().count() } else { 0 }
    }

    /// How many car materials are clipped to a portal entry (R29) and how many of those show index 0 as the
    /// backdrop (R14) in the state shown.
    pub fn clipped_cars(&self) -> (usize, usize) {
        let modes: Vec<UVec4> = self
            .app
            .world()
            .resource::<Assets<crate::Indexed>>()
            .iter()
            .map(|(_, m)| m.mode)
            .filter(|m| m.w != 0)
            .collect();
        (modes.len(), modes.iter().filter(|m| m.x & crate::OPAQUE != 0).count())
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
        // Ready = `STABLE` identical frames in a row, all pipelines compiled and their number unchanged over those
        // frames (a pipeline requested late means a mesh or material still missing), and, for the first
        // state, a real picture (not the few colours of a frame drawn before the assets reached the GPU).
        const STABLE: usize = 6;
        for _ in 0..1500 {
            let shots = self.shots.clone();
            self.app
                .world_mut()
                .spawn(Screenshot::image(self.target.clone()))
                .observe(move |c: On<ScreenshotCaptured>| {
                    shots
                        .lock()
                        .unwrap()
                        .push(c.image.data.clone().expect("screenshot data"));
                });
            self.app.update();
            std::thread::sleep(std::time::Duration::from_millis(3));
            let pipelines = self.pipelines_built();
            let s = self.shots.lock().unwrap();
            if pipelines != 0 && pipelines == self.pipelines_seen {
                self.stable_pipelines += 1;
            } else {
                self.stable_pipelines = 0;
            }
            self.pipelines_seen = pipelines;
            if s.len() >= STABLE && self.stable_pipelines >= STABLE {
                let last = &s[s.len() - STABLE..];
                let colours = last[0].chunks(4).collect::<std::collections::HashSet<_>>().len();
                if last.iter().all(|f| f == &last[0]) && (self.warm || colours > 40) {
                    self.warm = true;
                    return last[0].clone();
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
    /// Share of its pixels whose colour (within a small tolerance) shows in the GPU view within two pixels.
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
        let mut scene = if empty {
            render::Scene::empty()
        } else {
            view::scene(state)
        };
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
        (-r..=r)
            .flat_map(move |dy| (-r..=r).map(move |dx| (x + dx, y + dy)))
            .filter_map(|(x, y)| {
                ((0..W as i32).contains(&x) && (0..H as i32).contains(&y)).then_some(y as usize * W + x as usize)
            })
    };
    let seen = |p: usize| around(p, 1).any(|q| gpu[q] == exact[p]);
    // "Present": a GPU pixel of nearly the exact colour within two pixels (textures are sampled per window pixel here,
    // affinely per pixel pair there: colours drift, positions do not).
    let close = |p: usize| {
        around(p, 2).any(|q| {
            gpu[q]
                .iter()
                .zip(exact[p])
                .map(|(a, b)| a.abs_diff(b) as u32)
                .sum::<u32>()
                <= 90
        })
    };

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
                let thin = |dx: i32, dy: i32| {
                    (solid(-dx, -dy) || solid(-2 * dx, -2 * dy)) && (solid(dx, dy) || solid(2 * dx, 2 * dy))
                };
                if thin(1, 0) || thin(0, 1) {
                    report.seams.push(p);
                }
            }
        }
    }
    let mut object = |name: String, other: Vec<u8>, base: &[u8], keep: &dyn Fn(usize) -> bool| {
        let pixels: Vec<usize> = (0..W * H).filter(|&p| other[p] != base[p] && keep(p)).collect();
        if !pixels.is_empty() {
            let present = pixels.iter().filter(|&&p| close(p)).count() as f32 / pixels.len() as f32;
            report.objects.push(Object { name, pixels, present });
        }
    };
    // Entities (racers, traffic, ...): the entity's material set to 0 is not sorted or drawn.
    for (i, e) in view::scene(state).entities.iter().enumerate() {
        if e.material == 0 || e.slot == 0xFF {
            continue;
        }
        let kind = if state.slots[i].block.is_some() {
            "traffic"
        } else {
            "car"
        };
        object(
            format!(
                "{kind} entity {i} (slot {}){}",
                e.slot,
                if e.flags & 8 != 0 { " RAM atlas" } else { "" }
            ),
            draw(false, Some(i), None),
            &full,
            &|_| true,
        );
    }
    // Portal entries' walls and flats: the world alone, one entry skipped.
    let world_only = draw(true, None, None);
    for (k, p) in visible.portals.iter().enumerate().filter(|(_, p)| p.flags & 8 == 0) {
        object(
            format!("sector {} (entry {k})", p.sector),
            draw(true, None, Some(k)),
            &world_only,
            &|p| full[p] == world_only[p],
        );
    }
    report
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
const PER_TRACE: usize = 10;

/// R27: over a spread of recorded states (the start grid, turning, traffic, sparks, the speed effect, the bumper view,
/// the fades) the GPU view has every object the exact frame draws, no seams, and agrees with the frame on at least
/// the recorded share of pixels. `SHOTS_TRACE=<name>` runs one trace, `SHOTS_DIR=<dir>` saves the comparison images.
#[test]
fn gpu_view_matches_the_exact_frame() {
    let Some(rom) = nfsgba_testkit::rom() else { return };
    let only = std::env::var("SHOTS_TRACE").ok();
    let (mut states, mut exact, mut near, mut clipped_states) = (0, 0.0, 0.0, 0);
    let (mut worst_car, mut worst_traffic, mut worst_wall) = (1.0f32, 1.0f32, 1.0f32);
    let (mut worst_holes, mut worst_seams, mut objects) = (0.0f32, 0, 0);
    // Traffic cars whose atlas the game keeps in RAM (R29): how many objects, and the least share present.
    let (mut ram_objects, mut worst_ram) = (0, 1.0f32);
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
        let state = |k| play::Play::new(game_at(&rom, &trace, k), Handle::default(), true, None);
        let mut rig = Rig::new(state(0));
        for k in (0..PER_TRACE).map(|i| i * (frames - 1) / (PER_TRACE - 1)) {
            let gpu = rig.show(state(k));
            let (clipped, opaque) = rig.clipped_cars();
            assert_eq!(
                clipped, opaque,
                "a clipped car material shows index 0 as the backdrop (R14)"
            );
            clipped_states += (clipped > 0) as usize;
            let r = compare(&rig, &rom, &gpu);
            if r.covered == 0 {
                // The race start's first frame: the world is not drawn yet.
                continue;
            }
            if let Ok(dir) = std::env::var("SHOTS_DIR") {
                save(&std::path::Path::new(&dir).join(format!("{name}-{k}.png")), &gpu, &r);
            }
            let holes = r.holes.len() as f32 / r.covered as f32;
            eprintln!(
                "{name} {k:>3}: exact {:.1}% near {:.1}%, {} objects, {} holes ({} seam pixels) of {} covered",
                100.0 * r.exact,
                100.0 * r.near,
                r.objects.len(),
                r.holes.len(),
                r.seams.len(),
                r.covered
            );
            for o in r.objects.iter() {
                objects += 1;
                // Small far objects are a handful of pixels whose texels the GPU minifies differently.
                let (worst, least) = match o.name.split(' ').next().unwrap() {
                    "car" => (&mut worst_car, 20),
                    "traffic" => (&mut worst_traffic, 20),
                    _ => (&mut worst_wall, 100),
                };
                if o.name.starts_with("traffic") && o.name.contains("RAM atlas") {
                    ram_objects += 1;
                    worst_ram = worst_ram.min(o.present);
                }
                if o.pixels.len() >= least {
                    *worst = worst.min(o.present);
                    if o.present < 0.5 {
                        eprintln!(
                            "    MISSING {}: {} px, {:.0}% present",
                            o.name,
                            o.pixels.len(),
                            100.0 * o.present
                        );
                    }
                }
            }
            states += 1;
            (exact, near) = (exact + r.exact, near + r.near);
            worst_holes = worst_holes.max(holes);
            worst_seams = worst_seams.max(r.seams.len());
        }
    }
    let (exact, near) = (exact / states as f32, near / states as f32);
    eprintln!(
        "{states} states, {objects} objects: exact {:.3}%, within a pixel {:.3}%; least present: car {:.0}%, traffic {:.0}%, \
         walls {:.0}%; worst holes {:.2}% of the geometry, worst {worst_seams} seam pixels; {ram_objects} RAM-atlas traffic \n         objects, least present {:.0}%",
        100.0 * exact,
        100.0 * near,
        100.0 * worst_car,
        100.0 * worst_traffic,
        100.0 * worst_wall,
        100.0 * worst_holes,
        100.0 * worst_ram
    );
    if only.is_some() {
        return;
    }
    assert!(states >= 60, "{states} states");
    // The cars are clipped to their portal entry (R29) in most states.
    assert!(
        clipped_states * 2 > states,
        "cars clipped in {clipped_states} of {states} states"
    );
    // Non-regression floors for the agreement (by design about half the pixels differ: R27).
    assert!(exact > 0.48 && near > 0.79, "exact {exact}, near {near}");
    // Every object the exact frame draws is in the GPU view.
    assert!(worst_car >= 0.5 && worst_traffic >= 0.5 && worst_wall >= 0.5);
    // No gaps in the geometry.
    assert!(
        worst_holes < 0.005 && worst_seams <= 10,
        "holes {worst_holes}, seams {worst_seams}"
    );
}

/// G2: the HUD blend in the high-resolution view. At 240×160 the GPU view with the game's sprites mixed in equals, on
/// every pixel, the game's integer blend (`min(31, (obj·EVA + bg·EVB) >> 4)` per 5-bit channel) of the sprite over the
/// GPU view without them (in the recorded races every HUD sprite is semi-transparent); and where the GPU view has the exact frame's colour (`render::draw_world`), the pixel is the
/// exact frame's with the HUD over it, which is what the game shows. Opaque sprite pixels equal the exact frame's
/// everywhere.
#[test]
fn hud_blend_is_the_games_blend() {
    let Some(rom) = nfsgba_testkit::rom() else { return };
    if let Some(path) = nfsgba_testkit::dump("mgba/race") {
        let io = nfsgba_formats::Dump::load(&path).unwrap().io;
        assert_eq!(io[0x0C] & 3, 0, "BG2CNT priority");
    }
    let rgb8 = |c: u16| {
        let [r, g, b, _] = nfsgba_formats::bgr555(c);
        [r, g, b]
    };
    let bgr555 = |p: [u8; 3]| u16::from(p[0] >> 3) | u16::from(p[1] >> 3) << 5 | u16::from(p[2] >> 3) << 10;
    let (mut states, mut semi, mut opaque, mut agree, mut exact_blend) = (0, 0, 0, 0, 0);
    for (session, name) in TRACES {
        let (Some(dir), Some(_)) = (
            nfsgba_testkit::fixture(session),
            nfsgba_testkit::fixture(&format!("{session}/{name}.base.bin")),
        ) else {
            continue;
        };
        let trace = Trace::load(&dir, name).unwrap();
        let frames = trace.timing.len();
        let state = |k| play::Play::new(game_at(&rom, &trace, k), Handle::default(), true, None);
        let mut rig = Rig::new(state(0));
        for k in (0..4).map(|i| i * (frames - 1) / 3) {
            rig.hud(false);
            let base = rig.show(state(k));
            rig.hud(true);
            let shot = rig.show(state(k));
            let r = compare(&rig, &rom, &base);
            if r.covered == 0 {
                continue;
            }
            let world = rig.app.world();
            let (play, race, smooth) = (
                world.resource::<play::Play>(),
                world.resource::<Race>(),
                world.resource::<crate::Smooth>(),
            );
            let blend = (
                u32::from(play.bldalpha & 0x1F).min(16),
                u32::from(play.bldalpha >> 8 & 0x1F).min(16),
            );
            let objects = play::hud_objects(play, race, smooth);
            // OBJ priority against BG2 (G2): the race sets BG2CNT's priority to 0 (`mgba/race.io.bin`: DISPCNT 0x1F44,
            // BG2CNT 0) and every sprite that is drawn has priority 0, and on a tie the OBJ wins, so the sprites are
            // always in front of the bitmap and the model of "HUD over the scene" is the hardware's.
            for e in play.game.oam.chunks(8) {
                let (a0, a2) = (u16::from_le_bytes([e[0], e[1]]), u16::from_le_bytes([e[4], e[5]]));
                if (a0 >> 8) & 3 != 2 && a0 >> 14 != 3 && a0 & 0xFF != 0xA0 {
                    assert_eq!(a2 >> 10 & 3, 0, "{name} {k}: a sprite with priority {}", a2 >> 10 & 3);
                }
            }
            for (p, o) in objects.into_iter().enumerate() {
                let got = [shot[4 * p], shot[4 * p + 1], shot[4 * p + 2]];
                let under = [base[4 * p], base[4 * p + 1], base[4 * p + 2]];
                let (want, want_exact) = match o {
                    None => (under, r.picture[p]),
                    Some((c, false)) => {
                        opaque += 1;
                        assert_eq!(got, rgb8(c), "{name} {k} opaque sprite pixel {p}");
                        (rgb8(c), rgb8(c))
                    }
                    Some((c, true)) => {
                        semi += 1;
                        let mix = |back: [u8; 3]| rgb8(play::blend_555(c, bgr555(back), blend));
                        (mix(under), mix(r.picture[p]))
                    }
                };
                assert_eq!(got, want, "{name} {k} pixel {p} ({}, {})", p % W, p / W);
                if under == r.picture[p] {
                    agree += 1;
                    assert_eq!(
                        got, want_exact,
                        "{name} {k} pixel {p}: not the exact frame with the HUD"
                    );
                    exact_blend += o.is_some_and(|(_, s)| s) as usize;
                }
            }
            states += 1;
        }
    }
    eprintln!(
        "{states} states: {opaque} opaque and {semi} semi-transparent sprite pixels exact; {agree} pixels where the view \
         is the exact frame's, {exact_blend} of them semi-transparent, all equal to the exact frame with the HUD"
    );
    assert!(states >= 20, "{states} states");
    assert!(
        semi > 100_000,
        "{semi} semi-transparent pixels: the blend is not exercised"
    );
}

/// R29: traffic whose atlas the game keeps in RAM (entity flag bit 3) is drawn. No recorded state has one, so each
/// traffic car of a recorded state gets its ROM atlas copied into free EWRAM and its flag bit 3 set; the GPU view is
/// then the very same picture as with the ROM atlas (and `compare`'s exact frame reads the RAM atlas too).
#[test]
fn traffic_with_a_ram_atlas_is_drawn() {
    let Some(rom) = nfsgba_testkit::rom() else { return };
    let textures = nfsgba_formats::vehicle_textures(&rom);
    let mut moved = 0;
    for (session, name) in [("live-race", "live"), ("live-race", "trail"), ("game-loop", "drive")] {
        let (Some(dir), Some(_)) = (
            nfsgba_testkit::fixture(session),
            nfsgba_testkit::fixture(&format!("{session}/{name}.base.bin")),
        ) else {
            continue;
        };
        let trace = Trace::load(&dir, name).unwrap();
        let frames = trace.timing.len();
        let state = |k, ram: bool| {
            let mut game = game_at(&rom, &trace, k);
            let mut n = 0;
            let traffic: Vec<usize> = (0..game.world.slots.len())
                .filter(|&i| ram && game.world.slots[i].block.is_some())
                .collect();
            for i in traffic {
                let e = game.world.slots[i].e.clone();
                let Some(info) = game.world.material_info.get(e.material as usize) else {
                    continue;
                };
                let (size, texture) = (
                    info.width as usize * info.height as usize,
                    textures.get(e.material as usize),
                );
                let free = game.world.heap.windows(size).position(|w| w.iter().all(|&b| b == 0));
                let (Some(texture), Some(at), true) = (texture, free, e.slot != 0xFF && e.flags & 0x10 == 0) else {
                    continue;
                };
                if e.material_offset != 0
                    || e.material_step >> 8 != 0
                    || e.flags & 8 != 0
                    || texture.pixels.len() != size
                {
                    continue;
                }
                game.world.heap[at..at + size].copy_from_slice(&texture.pixels);
                let e = &mut game.world.slots[i].e;
                (e.atlas, e.flags) = (0x0200_0000 | at as u32, e.flags | 8);
                n += 1;
            }
            (play::Play::new(game, Handle::default(), true, None), n)
        };
        let mut rig = Rig::new(state(0, false).0);
        for k in (0..6).map(|i| i * (frames - 1) / 5) {
            let rom_view = rig.show(state(k, false).0);
            let (ram_play, n) = state(k, true);
            let ram_view = rig.show(ram_play);
            moved += n;
            let differ = (0..W * H)
                .filter(|&p| rom_view[4 * p..4 * p + 4] != ram_view[4 * p..4 * p + 4])
                .count();
            eprintln!(
                "{name} {k:>3}: {n} traffic cars with a RAM atlas, {differ} pixels differ from the ROM atlas's view"
            );
            assert_eq!(differ, 0, "{name} {k}");
        }
    }
    assert!(moved >= 5, "{moved} traffic cars moved to RAM atlases");
}
