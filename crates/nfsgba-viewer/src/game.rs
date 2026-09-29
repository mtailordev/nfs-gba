//! The race as the game sets it up and films it, in the game's integers: the four racers (from a race dump or a
//! route's grid), the chase camera (`camera_update` `0x08137cb0`, view 2) and its projection. Everything here is
//! exact unless marked; `docs/engine/viewer-rendering.md` ("Game camera") has the derivation and the checks.

use std::io;

use bevy::{
    camera::{CameraProjection, SubCameraView},
    math::Vec3A,
    prelude::*,
};
use nfsgba_fixed::cos_q14;
/// `atan2_fast` (IWRAM `0x03004470`): 0 = +y, 0x1000 = +x; `atan(344, 0)` = 0xFFD, the reference race's
/// camera yaw.
pub use nfsgba_fixed::{angle_diff, atan2_fast as atan};
use nfsgba_formats::{self as rom, atlas, paint, render};

/// The race projection's focal length (view struct `0x03000080 +0x1C`); `camera_update` eases it back to 150 while
/// the speed effect is off.
pub const FOCAL: i32 = 150;
/// The near plane (view `+0x10`), in city units.
pub const NEAR: i32 = 64;
/// Camera tables per view (`0x030055F8`; 0 bumper, 2 chase): i32 x offset (`0x7F39BC`, all 0), 8.8 height
/// (`0x7F39D4`, written to `0x03005FA4`) and distance (`0x7F39EC`).
const VIEW_HEIGHTS: usize = 0x7F_39D4;
const VIEW_DISTANCES: usize = 0x7F_39EC;
/// The chase view.
pub const CHASE: usize = 2;
/// New-profile paint per car (record byte 6, `profile_reset` `FUN_081356dc`); the other record bytes start at 0.
const NEW_PROFILE_PAINTS: usize = 0x7E_EA33;
/// The car table (15 × 0x58).
const CAR_TABLE: usize = 0x7F_0BD8;
/// i16 spoiler model per `car·0x10 + record[0]` (entity `+0x64`).
const SPOILERS: usize = 0x7F_0636;
/// The rand index at `pick_opponent_cars` in the reference race: the only one of the 256 that deals its racers
/// (cars 2, 9, 10, 11; paints 11, 11, 11, 5). Derived, not traced: `setup_race_cars` seeds the index with
/// `*0x03000044 & 0xFF` = 3, so 14 other draws come between (open).
pub const REFERENCE_RAND: u32 = 0x11;

/// The renderer's runtime tables in a race as every captured race has them (20 dumps, routes 18 and 23): each
/// moving wall piece (world `+0x18`, one per wall naming one at `+0x2A`) with no offsets and flags 1, so those 122
/// walls are open (not drawn, not blocking the camera); no material animation or scroll.
/// NOT 1:1 (R21): the tables' writers are not decoded, so other races may set them otherwise.
pub fn race_runtime(sectors: &[rom::Sector]) -> render::Runtime {
    let pieces = sectors
        .iter()
        .flat_map(|s| &s.walls)
        .filter(|w| w.piece != 0xFFFF)
        .map(|w| w.piece as usize + 1)
        .max()
        .unwrap_or(0);
    render::Runtime {
        pieces: vec![
            render::Piece {
                flags: 1,
                ..Default::default()
            };
            pieces
        ],
        ..Default::default()
    }
}

/// A wall's flags as the renderer sees them: its moving piece's when it names one.
pub fn wall_flags(rt: &render::Runtime, w: &rom::Wall) -> u16 {
    rt.pieces.get(w.piece as usize).map_or(w.flags, |p| p.flags)
}

fn word(rom: &[u8], at: usize) -> i32 {
    i32::from_le_bytes(rom[at..at + 4].try_into().unwrap())
}

fn half(rom: &[u8], at: usize) -> i16 {
    i16::from_le_bytes(rom[at..at + 2].try_into().unwrap())
}

/// `FUN_08160624`: the rotation about y by `yaw`, 2.14 fixed point, row-major, zero translation.
pub fn rotation(rom: &[u8], yaw: i32) -> [i32; 12] {
    let (c, s) = (cos_q14(rom, yaw), paint::sin_q14(rom, yaw));
    [c, 0, -s, 0, 0x4000, 0, s, 0, c, 0, 0, 0]
}

/// A racer's entity state (0xA4-byte entities at world `+0x3C`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Racer {
    /// `+0x0C/+0x10/+0x14`: position, 8.8 fixed point, city units (`-y` is up).
    pub pos: [i32; 3],
    /// `+0x2C >> 8`: heading, 0x4000 per turn.
    pub heading: i32,
    /// The driver's heading (`*(entity + 0x8C)`, the driver struct's first word), which the chase camera follows.
    pub driver_heading: i32,
    /// `+0x78`: the sector the racer is in.
    pub sector: u16,
    /// `+0x88`: vehicle matrix slot; 0xFF = not drawn (`draw_sector_entities`).
    pub slot: u8,
    /// `+0x0A`: draw flags (bit 0 whole-screen clip, bit 1 always the near model, bit 6 far model beyond 0x1000).
    pub flags: u16,
    /// `+0x36`: the far model (drawn at depth ≥ 0x200; the one before it nearer).
    pub model: i16,
    /// `+0x64`: the spoiler model on the next matrix slot (0: none).
    pub extra: i16,
}

impl Racer {
    /// The models `draw_sector_entities` draws for this racer at camera depth `d` (`render::entities`), body
    /// then spoiler; none beyond depth 0x2000, nor beyond 0x1000 without flag bit 6, nor without a matrix slot.
    pub fn models_at(&self, d: i32) -> Vec<usize> {
        let mut d = d;
        if d as u32 >= 0x2000 || self.slot == 0xFF || self.model == 0 {
            return Vec::new();
        }
        if self.flags & 2 != 0 {
            d = 0;
        }
        if d > 0x1000 {
            if self.flags & 0x40 == 0 {
                return Vec::new();
            }
            d = 0x200;
        }
        let body = (self.model + (d >= 0x200) as i16 - 1) as usize;
        let spoiler = self.extra.unsigned_abs() as usize;
        match self.extra {
            0 => vec![body],
            n if n < 0 => vec![spoiler, body],
            _ => vec![body, spoiler],
        }
    }
}

/// The chase camera's state between frames (view 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chase {
    /// `0x03005F94`: the yaw the camera orbits the player at; it follows the driver's heading.
    pub yaw: i32,
    /// `0x03000214`: the yaw the camera looks along (the matrix uses `−look`, stored at `0x03005F9C`).
    pub look: i32,
    /// `0x030056A0` / `0x030000A4`: camera x and z, 8.8.
    pub x: i32,
    pub z: i32,
    /// `0x03005614`: the camera's sector.
    pub sector: u16,
    /// View `+0x1C`.
    pub focal: i32,
}

impl Chase {
    /// A camera that has settled behind a standing racer: orbit yaw = the driver's heading, position and look yaw
    /// from one `step`'s rules. A standing racer keeps it there frame after frame.
    pub fn behind(rom: &[u8], rt: &render::Runtime, player: &Racer) -> Chase {
        let mut c = Chase {
            yaw: player.driver_heading,
            look: 0,
            x: player.pos[0],
            z: player.pos[2],
            sector: player.sector,
            focal: FOCAL,
        };
        c.place(rom, rt, player);
        c.look = atan(
            rom,
            (player.pos[0] >> 8) - (c.x >> 8),
            (player.pos[2] >> 8) - (c.z >> 8),
        );
        c
    }

    /// The orbit distance (8.8): the view's table entry plus a dolly that grows as the focal length drops.
    fn distance(&self, rom: &[u8]) -> i32 {
        word(rom, VIEW_DISTANCES + 4 * CHASE) * 256 + (0x80 - self.focal) * 0x200
    }

    /// Camera position on the orbit (8.8), then pushed out of nearby walls.
    fn place(&mut self, rom: &[u8], rt: &render::Runtime, player: &Racer) {
        let d = self.distance(rom);
        self.x = player.pos[0].wrapping_add(paint::sin_q14(rom, self.yaw).wrapping_mul(d) >> 14);
        self.z = player.pos[2].wrapping_add(cos_q14(rom, self.yaw).wrapping_mul(d) >> 14);
        self.push_out_of_walls(rom, rt);
    }

    /// `FUN_08137744`: for each wall of the camera's sector that blocks the camera (flag `0x1000`, or `0x4000`
    /// below the height limit `*0x03005778`; a moving piece's flags replace the wall's), a camera nearer than 81
    /// units to its line (and within its ends, or within √0x18FF of them) is pushed out to 80 units along the wall
    /// normal (`+0x34`/`+0x36`, 4.12). Walls in the game's order: the last one first, then 0, 1, …
    /// NOT 1:1 (R11): the height limit (a smoothed ground height from `FUN_0814ca84`) is taken as passed, as it is for
    /// every wall in the reference race.
    fn push_out_of_walls(&mut self, rom: &[u8], rt: &render::Runtime) {
        let walls = word(rom, rom::LEVEL_TABLE + 0x14) as u32 - rom::ROM_BASE;
        let sectors = word(rom, rom::LEVEL_TABLE + 0x18) as u32 - rom::ROM_BASE;
        let s = (sectors + 0x30 * u32::from(self.sector)) as usize;
        let (first, count) = (half(rom, s) as u16 as usize, half(rom, s + 2) as u16 as usize);
        let wall = |k: usize| walls as usize + 0x44 * (first + k);
        for k in (0..count).map(|k| (k + count - 1) % count) {
            let (w, n) = (wall(k), wall((k + 1) % count));
            let piece = rt.pieces.get(half(rom, w + 0x2A) as u16 as usize);
            let flags = piece.map_or(half(rom, w + 0x2E) as u16, |p| p.flags);
            if flags & 0x5000 == 0 {
                continue;
            }
            let (x3, z3, x2, z2) = (word(rom, w), word(rom, w + 4), word(rom, n), word(rom, n + 4));
            let (nx, nz) = (half(rom, w + 0x34) as i32, half(rom, w + 0x36) as i32);
            let (dx, dz) = ((self.x >> 8) - x3, (self.z >> 8) - z3);
            let dist = (dx * nx + dz * nz) >> 12;
            if dist >= 0x51 {
                continue;
            }
            let outside = if (x2 - x3) * dx + dz * (z2 - z3) < 0 {
                dx * dx + dz * dz > 0x18FF
            } else {
                let (ex, ez) = ((self.x >> 8) - x2, (self.z >> 8) - z2);
                (x3 - x2) * ex + ez * (z3 - z2) < 0 && ex * ex + ez * ez > 0x18FF
            };
            if !outside {
                self.x += ((0x50 - dist) * nx) >> 4;
                self.z += (nz * (0x50 - dist)) >> 4;
            }
        }
    }

    /// One game frame of `camera_update` in the chase view for the player `player` (focal 150, no speed effect,
    /// no shake): look yaw from the previous position (the lag), orbit yaw eased towards the driver's heading by
    /// `clamp(diff, ±0x600) >> 3`, position, the camera sector, and the frame the renderer uses.
    /// NOT 1:1 (R11): the speed effect (driver `+0x4D1` set: focal eases towards `150 − max(0, (0x800 − g) >> 5)`,
    /// `g` the angle between heading and travel) is not modelled; the viewer has no driving.
    pub fn step(&mut self, rom: &[u8], rt: &render::Runtime, player: &Racer) -> (render::Frame, render::Portal) {
        self.focal = if self.focal < FOCAL { self.focal + 4 } else { FOCAL };
        let (px, pz) = (player.pos[0] >> 8, player.pos[2] >> 8);
        let (ex, ez) = (px - (self.x >> 8), pz - (self.z >> 8));
        if ex * ex + ez * ez > 0x10_0000 {
            (self.x, self.z, self.sector) = (player.pos[0], player.pos[2], player.sector);
        }
        self.look = atan(rom, px - (self.x >> 8), pz - (self.z >> 8));
        let diff = angle_diff(player.driver_heading, self.yaw).clamp(-0x600, 0x600);
        self.yaw -= diff >> 3;
        if self.yaw > 0x4000 {
            self.yaw -= 0x4000;
        }
        if self.yaw < -0x4000 {
            self.yaw += 0x4000;
        }
        self.place(rom, rt, player);
        self.find_sector(rom, rt, player);
        let height = word(rom, VIEW_HEIGHTS + 4 * CHASE);
        let mut camera = rotation(rom, -self.look & 0x3FFF);
        camera[9] = -(self.x >> 8);
        camera[10] = -(height >> 8) - ((player.pos[1] >> 8) + 0x10);
        camera[11] = -(self.z >> 8);
        let frame = render::Frame {
            view: render::View {
                cx: 120,
                cy: 79,
                near: NEAR,
                focal: self.focal,
            },
            camera,
            rect: [0, 240, 0, 159],
        };
        let root = render::Portal {
            sector: self.sector,
            left: 0,
            right: 240,
            top: 0,
            bottom: 159,
            flags: 0,
            depth: 0,
        };
        (frame, root)
    }

    /// The camera sector: the camera position must be reachable from the previous sector (else the player's
    /// sector is taken), then the sector 72 units ahead along the look yaw (`FUN_081608fc(R(look), (0, 0, 72))`)
    /// becomes the camera sector, the previous one when that search fails.
    /// NOT 1:1 (R11): the second fallback `FUN_0814dbbc` is not decoded (the previous sector is kept); sector `+0x22`
    /// aliases (none in the Carbon city) are not applied.
    fn find_sector(&mut self, rom: &[u8], rt: &render::Runtime, player: &Racer) {
        if render::camera_sector(rom, rt, self.sector, self.x >> 8, self.z >> 8).is_none() {
            self.sector = player.sector;
        }
        let m = rotation(rom, self.look);
        let (ax, az) = ((72 * m[6]) >> 14, (72 * m[8]) >> 14);
        if let Some(s) = render::camera_sector(rom, rt, self.sector, (self.x >> 8) + ax, (self.z >> 8) + az) {
            self.sector = s;
        }
    }
}

/// The game's projection (`render::View`): `sx = 120 + focal·x/(d + 1)`, `sy = 79 + focal·y/(d + 1)` on the
/// 240×160 screen, which the window shows whole; depth is reversed and infinite as Bevy expects, and geometry
/// nearer than the near plane (64 units) is clipped.
/// NOT 1:1 (R11): the game divides through the reciprocal table in integers and rounds down.
#[derive(Debug, Clone)]
pub struct GbaProjection {
    pub focal: f32,
    /// Near plane, in metres.
    pub near: f32,
    /// One city unit in metres (the `+ 1` of `d + 1`).
    pub unit: f32,
}

impl GbaProjection {
    /// View-space (x right, y up, −z ahead) extent of the screen at distance `d`: left, right, bottom, top.
    fn extent(&self, d: f32) -> [f32; 4] {
        [-120.0, 120.0, -81.0, 79.0].map(|e| e * (d + self.unit) / self.focal)
    }
}

impl CameraProjection for GbaProjection {
    fn get_clip_from_view(&self) -> Mat4 {
        // w = d + 1 unit; NDC x = focal·x/(120·w); NDC y = focal·y/(80·w) + 1/80, which puts the optical axis on
        // the top edge of screen row 79 (the game's `cy`) and column 120. Reversed depth near/w with the constant
        // at `near + unit`, so points clip exactly where `d < near`.
        Mat4::from_cols(
            Vec4::new(self.focal / 120.0, 0.0, 0.0, 0.0),
            Vec4::new(0.0, self.focal / 80.0, 0.0, 0.0),
            Vec4::new(0.0, -1.0 / 80.0, 0.0, -1.0),
            Vec4::new(0.0, self.unit / 80.0, self.near + self.unit, self.unit),
        )
    }

    fn get_clip_from_view_for_sub(&self, _: &SubCameraView) -> Mat4 {
        self.get_clip_from_view()
    }

    fn update(&mut self, _: f32, _: f32) {}

    fn far(&self) -> f32 {
        1.0e6
    }

    fn get_frustum_corners(&self, z_near: f32, z_far: f32) -> [Vec3A; 8] {
        let corners = |z: f32| {
            let [l, r, b, t] = self.extent(z.abs());
            [
                Vec3A::new(r, b, z),
                Vec3A::new(r, t, z),
                Vec3A::new(l, t, z),
                Vec3A::new(l, b, z),
            ]
        };
        let (n, f) = (corners(z_near), corners(z_far));
        [n[0], n[1], n[2], n[3], f[0], f[1], f[2], f[3]]
    }
}

/// The eye and view direction of a render frame in the viewer's world (see `world` in `main.rs`): the eye is
/// minus the matrix translation, the view direction its depth row `(m2, m8)`.
pub fn frame_transform(frame: &render::Frame, world: impl Fn(f32, f32, f32) -> Vec3) -> Transform {
    let m = &frame.camera;
    let eye = world(-m[9] as f32, -m[10] as f32, -m[11] as f32);
    let ahead = world(m[2] as f32, 0.0, m[8] as f32).normalize();
    Transform::from_translation(eye).looking_to(ahead, Vec3::Y)
}

/// A race dump (`nfsgba_formats::Dump`: `<prefix>.iwram.bin`, `<prefix>.wram.bin`, …) with the reads the race
/// setup needs.
pub struct Dump(rom::Dump);

impl Dump {
    /// `prefix` is relative to `$NFSGBA_DATA/work/e5298b24/`, e.g. `mgba/race`.
    pub fn load(prefix: &str) -> io::Result<Dump> {
        Ok(Dump(rom::Dump::load(
            &rom::data_dir().join("work/e5298b24").join(prefix),
        )?))
    }

    /// Live RAM (play mode: `nfsgba_game::Game`'s IWRAM and EWRAM).
    pub fn from_ram(iwram: Vec<u8>, wram: Vec<u8>) -> Dump {
        Dump(rom::Dump {
            iwram,
            ewram: wram,
            ..Default::default()
        })
    }

    fn at(&self, a: u32) -> &[u8] {
        self.0.at(a)
    }

    fn word(&self, a: u32) -> i32 {
        self.0.u32(a) as i32
    }

    fn half(&self, a: u32) -> u16 {
        self.0.u16(a)
    }

    /// `0x03005720`: the race's route index.
    pub fn route(&self) -> usize {
        self.word(0x0300_5720) as usize
    }

    /// World struct field `+off` (`0x030000C0`).
    fn world(&self, off: u32) -> u32 {
        self.word(0x0300_00C0 + off) as u32
    }

    /// Vehicle matrix slot `s` (world `+0xFC`, 0x30 bytes each).
    pub fn matrix(&self, s: u8) -> [i32; 12] {
        let at = self.world(0xFC) + 0x30 * s as u32;
        std::array::from_fn(|k| self.word(at + 4 * k as u32))
    }

    /// Racer `i`'s atlas as the race holds it in EWRAM (entity `+0x84`, when flag bit 3), rim and all.
    pub fn atlas(&self, i: u32, len: usize) -> Option<Vec<u8>> {
        let e = self.world(0x3C) + 0xA4 * i;
        (self.half(e + 0x0A) & 8 != 0).then(|| self.at(self.word(e + 0x84) as u32)[..len].to_vec())
    }

    /// The entities as the renderer draws them (`render::Scene`): the entity array (world `+0x3C`, count
    /// `+0xF8` + `+0xFA`), the sector list heads (`+0x0C`), the matrix slots (`+0xFC`) and EWRAM for the atlases.
    pub fn scene(&self, sectors: usize) -> render::Scene<'_> {
        let (entities, heads) = (self.world(0x3C), self.world(0x0C));
        let count = (self.half(0x0300_00C0 + 0xF8) + self.half(0x0300_00C0 + 0xFA)) as u32;
        render::Scene::new(
            (0..count)
                .map(|i| render::Entity::read(self.at(entities + 0xA4 * i)))
                .collect(),
            (0..sectors as u32).map(|s| self.half(heads + 2 * s)).collect(),
            (0..64).map(|s| self.matrix(s)).collect(),
            &self.0.ewram,
        )
    }
}

/// Who races and how they look, and where they and the camera are.
#[derive(Debug, Clone)]
pub struct RaceSetup {
    pub racers: [Racer; 4],
    /// `0x0300611C` / `0x03005FEC`: car id and paint per racer.
    pub cars: [i8; 4],
    pub paints: [i8; 4],
    /// The player's car record (0x11 bytes at `*0x0300539C + 0x11·car`).
    pub record: [u8; 0x11],
    pub chase: Chase,
}

impl RaceSetup {
    /// A race dump as it was: the entities (world `+0x3C`, player `*0x03000060`), racers, the player's record and
    /// the camera state.
    pub fn from_dump(d: &Dump) -> RaceSetup {
        let entities = d.word(0x0300_00C0 + 0x3C) as u32;
        let racer = |i: u32| {
            let e = entities + 0xA4 * i;
            Racer {
                pos: [0x0C, 0x10, 0x14].map(|k| d.word(e + k)),
                heading: d.word(e + 0x2C) >> 8,
                driver_heading: d.word(d.word(e + 0x8C) as u32),
                sector: d.half(e + 0x78),
                slot: d.at(e + 0x88)[0],
                flags: d.half(e + 0x0A),
                model: d.half(e + 0x36) as i16,
                extra: d.half(e + 0x64) as i16,
            }
        };
        let player = d.word(0x0300_0060) as u32;
        let order = [player, (player + 1) % 4, (player + 2) % 4, (player + 3) % 4];
        let bytes = |a: u32| <[u8; 4]>::try_from(&d.at(a)[..4]).unwrap().map(|b| b as i8);
        let cars = bytes(0x0300_611C);
        let record = d.at(d.word(0x0300_539C) as u32 + 0x11 * cars[0] as u32)[..0x11]
            .try_into()
            .unwrap();
        RaceSetup {
            racers: order.map(racer),
            cars,
            paints: bytes(0x0300_5FEC),
            record,
            chase: Chase {
                yaw: d.word(0x0300_5F94),
                look: d.word(0x0300_0214),
                x: d.word(0x0300_56A0),
                z: d.word(0x0300_00A4),
                sector: d.half(0x0300_5614),
                focal: d.word(0x0300_0080 + 0x1C),
            },
        }
    }

    /// A Quick Play race on `route`'s grid with a new profile: the player in car `car` with its default record,
    /// three opponents dealt by `pick_opponent_cars` from rand index `rand` (no wingman) and dressed by `look`.
    /// Models as `setup_race_cars` sets them: the player's far model is the car table's `+0x14` + 1 and its
    /// spoiler `i16 0x7F0636[car·0x10 + record[0]]` (at least 0); the draw flags are the reference race's (player
    /// 0x0D, opponents 0x22). `floor_y(sector, x, z)` gives the racer's height, 8.8.
    /// NOT 1:1 (D2): the game spawns racers at the template's height and lets the physics settle them.
    pub fn grid(
        rom: &[u8],
        rt: &render::Runtime,
        route: &rom::Route,
        car: i8,
        rand: u32,
        floor_y: impl Fn(u16, i32, i32) -> i32,
    ) -> RaceSetup {
        let mut record = [0; 0x11];
        record[6] = rom[NEW_PROFILE_PAINTS + car as usize];
        let (mut cars, mut paints, mut rand) = ([car, 0, 0, 0], [record[6] as i8, 0, 0, 0], rand);
        atlas::pick_opponent_cars(rom, &mut rand, 3, 0, &mut cars, &mut paints);
        let racers: [Racer; 4] = std::array::from_fn(|i| {
            let [x, _, z] = route.positions[i];
            let (flags, model, extra) = if i == 0 {
                let close = half(rom, CAR_TABLE + 0x58 * car as usize + 0x14);
                let spoiler = half(rom, SPOILERS + 2 * (0x10 * car as usize + record[0] as usize));
                (0x0D, close + 1, spoiler.max(0))
            } else {
                (0x22, atlas::look(rom, cars, i, false, false).model as i16, 0)
            };
            Racer {
                pos: [x, floor_y(route.sectors[i], x >> 8, z >> 8), z],
                heading: route.headings[i],
                driver_heading: route.headings[i],
                sector: route.sectors[i],
                slot: i as u8,
                flags,
                model,
                extra,
            }
        });
        RaceSetup {
            chase: Chase::behind(rom, rt, &racers[0]),
            racers,
            cars,
            paints,
            record,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nfsgba_testkit::rom;

    #[test]
    fn angle_helpers() {
        assert_eq!((angle_diff(0x1000, 0x0FFD), angle_diff(0x0FFD, 0x1000)), (-3, 3));
        assert_eq!(angle_diff(0x3F00, 0x0100), 0x200);
        assert_eq!(angle_diff(0x0100, 0x3F00), -0x200);
        let Some(rom) = rom() else { return };
        // The reference race's look yaw, and the polynomial's value on the diagonal.
        assert_eq!(atan(&rom, 344, 0), 0xFFD);
        assert_eq!(atan(&rom, 100, 100), 0x800);
        assert_eq!(atan(&rom, -344, 0) & 0x3FFF, 0x3003);
    }

    /// The chase camera rule on the reference race's player gives the dump's camera exactly: its state
    /// (`0x03005F94`, `0x03000214`, `0x030056A0`, `0x030000A4`, `0x03005614`) and the frame the renderer used
    /// (matrix `0x030057A0`, the same as `render`'s reference test), both from the dump's own state and settled
    /// from scratch behind the player. The Quick Play deal from `REFERENCE_RAND` gives the dump's racers.
    #[test]
    fn chase_camera_reproduces_the_race() {
        let Some(rom) = rom() else { return };
        let Some(_) = nfsgba_testkit::dump("mgba/race") else {
            return;
        };
        let d = Dump::load("mgba/race").unwrap();
        let setup = RaceSetup::from_dump(&d);
        let rt = race_runtime(&rom::city(&rom));
        let player = setup.racers[0];
        assert_eq!(
            (player.heading, player.driver_heading, player.sector),
            (0x1000, 0x1000, 760)
        );
        let want = [18, 0, 16383, 0, 16384, 0, -16383, 0, 18, -118039, -30, 64321];
        let mut chase = setup.chase;
        let (frame, root) = chase.step(&rom, &rt, &player);
        assert_eq!(chase, setup.chase, "a frame later the camera is where it was");
        assert_eq!((frame.camera, root.sector), (want, 760));
        assert_eq!((frame.view.cx, frame.view.cy, frame.view.focal), (120, 79, 150));
        let mut settled = Chase::behind(&rom, &rt, &player);
        assert_eq!(settled, setup.chase);
        assert_eq!(settled.step(&rom, &rt, &player).0.camera, want);

        let routes = rom::routes(&rom);
        let grid = RaceSetup::grid(&rom, &rt, &routes[23], 2, REFERENCE_RAND, |_, _, _| 0);
        assert_eq!(
            (grid.cars, grid.paints, grid.record),
            (setup.cars, setup.paints, setup.record)
        );
        let draw = |r: &Racer| (r.flags, r.model, r.extra, r.heading);
        assert_eq!(grid.racers.map(|r| draw(&r))[0], draw(&setup.racers[0]));
        // The opponents raced off (their headings differ); their models and flags are as dealt.
        let dressed = |r: &Racer| (r.flags, r.model, r.extra);
        assert_eq!(
            grid.racers[1..].iter().map(dressed).collect::<Vec<_>>(),
            setup.racers[1..].iter().map(dressed).collect::<Vec<_>>()
        );
        // LOD: the player's medium model near, low model from depth 0x200, spoiler 12 on top; opponents stay near.
        assert_eq!(
            (setup.racers[0].models_at(343), setup.racers[0].models_at(0x200)),
            (vec![9, 12], vec![10, 12])
        );
        assert_eq!(setup.racers[1].models_at(0x1800), Vec::<usize>::new()); // matrix slot 0xFF: not drawn
    }

    /// The projection puts view-space points where `render::View` puts them on the 240×160 screen.
    #[test]
    fn projection_matches_the_game() {
        // In city units (unit = 1): the game's own numbers.
        let p = GbaProjection {
            focal: 150.0,
            near: 64.0,
            unit: 1.0,
        };
        let m = p.get_clip_from_view();
        for (x, y, d) in [(0.0, 0.0, 400.0), (-323.0, 0.0, 64.0), (100.0, -50.0, 700.0)] {
            // Game camera space has y down; view space has y up and looks along −z.
            let clip = m * Vec4::new(x, -y, -d, 1.0);
            let (sx, sy) = ((clip.x / clip.w + 1.0) * 120.0, (1.0 - clip.y / clip.w) * 80.0);
            assert!((sx - (120.0 + 150.0 * x / (d + 1.0))).abs() < 1e-3, "{x} {y} {d}: {sx}");
            assert!((sy - (79.0 + 150.0 * y / (d + 1.0))).abs() < 1e-3, "{x} {y} {d}: {sy}");
            // The near plane: depth 1 exactly at d = 64.
            assert!(d != 64.0 || (clip.z / clip.w - 1.0).abs() < 1e-6);
        }
    }
}
