//! The per-frame matrix slots (64 of 0x30 bytes, counter `slot_counter`), the 2D effect sprites the race puts into
//! the sprite pool (rear lights, brake lights, exhaust flames, sparks), the spark entities (handler 0x34) and the
//! player's rim redraw. Every function names the game function it is. It runs on typed state, a [`Slots`] frame,
//! which `World::with_slots` builds and writes back (`docs/engine/typed-state.md`).

use nfsgba_fixed::{angle_diff, cos_q14, div, recip, sin_q14};
use nfsgba_formats::atlas;
use nfsgba_sim::{
    Result, Unported,
    data::GameData,
    slot_data::{BILLBOARD, EXHAUST_MATERIAL, FLARE},
    state::{Camera, Car, CarRecord, Entity, MaterialInfo, SlotGlobals, Sprite, ViewPort},
    world::NONE,
};

/// A 3×4 matrix, 2.14 rotation (row-major) then translation.
pub type M = [i32; 12];

/// Everything the slot builder and the effect sprites read and write in one frame.
pub struct Slots<'a> {
    pub rom: &'a [u8],
    pub data: &'a GameData,
    pub g: SlotGlobals,
    /// The race phase, the local player's entity index, 2 in link play.
    pub phase: u32,
    pub player: u32,
    pub link: u32,
    /// The camera (the world's `+0x54` always points at its matrix) and the view.
    pub camera: Camera,
    pub view: ViewPort,
    /// The 64 matrix slots.
    pub slots: Vec<M>,
    /// The effect-sprite pool (its count is the length).
    pub pool: Vec<Sprite>,
    /// All entities, and the physics struct of the cars, opponents and wingman among them.
    pub entities: Vec<Entity>,
    pub cars: Vec<Option<Car>>,
    /// Each sector's first entity (`0xFFFF` none).
    pub heads: Vec<u16>,
    /// The entities of the eight traffic slots.
    pub traffic: [Option<usize>; 8],
    pub records: Vec<CarRecord>,
    /// The profile's per-racer contact.
    pub racer_hits: [i32; 4],
    /// World `+0xF8`/`+0xFA`: the first entity `free_entity` looks at, and how many.
    pub spare: (usize, usize),
    /// The sector search's last query (world scratch): point and start sector.
    pub query: ([i32; 3], u16),
}

pub fn rotation_y(rom: &[u8], a: i32) -> M {
    let (c, s) = (cos_q14(rom, a), sin_q14(rom, a));
    [c, 0, s.wrapping_neg(), 0, 0x4000, 0, s, 0, c, 0, 0, 0]
}

/// IWRAM `0x081694b0` (ROM copy): the rotation about x.
fn rotation_x(rom: &[u8], a: i32) -> M {
    let (c, s) = (cos_q14(rom, a), sin_q14(rom, a));
    [0x4000, 0, 0, 0, c, s, 0, s.wrapping_neg(), c, 0, 0, 0]
}

fn dot3(a: [i32; 3], b: [i32; 3]) -> i32 {
    a[0].wrapping_mul(b[0])
        .wrapping_add(a[1].wrapping_mul(b[1]))
        .wrapping_add(a[2].wrapping_mul(b[2]))
}

/// `rotate_vector` (`0x081608fc`): `v · M >> 14`.
pub fn rotate_vector(a: &M, v: [i32; 3]) -> [i32; 3] {
    std::array::from_fn(|j| dot3(v, [a[j], a[3 + j], a[6 + j]]) >> 14)
}

/// `transform_point` (`0x081607e0`): `(v · M >> 14) + translation`.
pub fn transform_point(a: &M, v: [i32; 3]) -> [i32; 3] {
    let r = rotate_vector(a, v);
    std::array::from_fn(|j| r[j].wrapping_add(a[9 + j]))
}

/// IWRAM `0x081695b8` (3×3 product `a · b`, translation untouched) and `FUN_08160434` (the same, plus the sum of
/// the translations).
fn mul(a: &M, b: &M, out: &mut M, translation: bool) {
    for i in 0..3 {
        for j in 0..3 {
            out[3 * i + j] = dot3([a[3 * i], a[3 * i + 1], a[3 * i + 2]], [b[j], b[3 + j], b[6 + j]]) >> 14;
        }
    }
    if translation {
        for j in 9..12 {
            out[j] = a[j].wrapping_add(b[j]);
        }
    }
}

/// Camera-space x and depth of an entity (`(pos >> 8 + t) · R`).
fn eye(e: &Entity, cam: &M) -> (i32, i32) {
    let x = (e.pos[0] >> 8).wrapping_add(cam[9]);
    let z = (e.pos[2] >> 8).wrapping_add(cam[11]);
    let side = x.wrapping_mul(cam[0]).wrapping_add(z.wrapping_mul(cam[6])) >> 14;
    let depth = x.wrapping_mul(cam[2]).wrapping_add(z.wrapping_mul(cam[8])) >> 14;
    (side, depth)
}

/// A projected point (screen scale `s`, camera-space `out`) inside the 240×160 screen around (`cx`, `cy`).
fn on_screen(s: i32, out: [i32; 3], cx: i32, cy: i32) -> bool {
    let x = (s.wrapping_mul(out[0]) >> 16) + cx;
    let y = (s.wrapping_mul(out[1]) >> 16) + cy;
    0 < x && x < 0xF0 && 0 < y && y < 0xA0
}

/// The driver's physics orientation (rotation, 2.14 after the shift), for the body matrix.
fn orientation(car: Option<&Car>) -> M {
    let mut p = [0; 12];
    if let Some(c) = car {
        for (k, v) in p.iter_mut().take(9).enumerate() {
            *v = c.body.rot[k] << 2;
        }
    }
    p
}

impl Slots<'_> {
    /// `FUN_0814f8ac`: the next slot, 0xFF past 0x3F.
    fn next_slot(&mut self) -> u8 {
        let s = self.g.slot_counter;
        self.g.slot_counter = s.wrapping_add(1);
        if 0x3F < s.wrapping_add(1) as i32 { 0xFF } else { s as u8 }
    }

    /// `build_entity_matrix` (`0x0814da68`): `R(+0x30 about x) · camera · R(+0x32 about y)`, or with flag bit 5 (and
    /// the physics orientation) the driver's physics orientation times the camera; then the camera-space position.
    /// Nothing is written beyond depth 0xDAC without flag bit 1 (the slot keeps last frame's matrix).
    fn build_entity_matrix(&mut self, i: usize, slot: u8) {
        let cam = self.camera.matrix;
        let e = &self.entities[i];
        let (side, depth) = eye(e, &cam);
        if e.flags & 2 == 0 && depth > 0xDAC {
            return;
        }
        let mut r = self.slots[slot as usize];
        if e.flags & 0x20 == 0 || self.g.physics_orientation == 0 {
            let (a, b) = (
                rotation_x(self.rom, e.angles[0] as i32),
                rotation_y(self.rom, e.angles[1] as i32),
            );
            let mut tmp = [0; 12];
            mul(&a, &cam, &mut tmp, false);
            mul(&tmp, &b, &mut r, false);
        } else {
            mul(&orientation(self.cars[i].as_ref()), &cam, &mut r, false);
        }
        r[9] = side;
        r[10] = (e.pos[1] >> 8).wrapping_add(cam[10]);
        r[11] = depth;
        self.slots[slot as usize] = r;
    }

    /// `assign_entity_slot` (`0x0814eba0`): a slot for an entity with a model that was drawn last frame (flag bit
    /// 2), else `0xFF`; clears bit 2.
    fn assign_entity_slot(&mut self, i: usize) {
        let e = &self.entities[i];
        if e.model == 0 || e.flags & 4 == 0 {
            self.entities[i].slot = 0xFF;
        } else {
            let s = self.next_slot();
            self.entities[i].slot = s;
            if s != 0xFF {
                self.build_entity_matrix(i, s);
            }
        }
        self.entities[i].flags &= 0xFFFB;
    }

    /// `assign_billboard_slot` (`0x0814ec0c`): a slot with the camera rotation and the entity's camera-space
    /// position; whether it was built.
    fn assign_billboard_slot(&mut self, i: usize) -> bool {
        let s = self.next_slot();
        self.entities[i].slot = s;
        if s == 0xFF {
            return false;
        }
        let cam = self.camera.matrix;
        let e = &self.entities[i];
        let (side, depth) = eye(e, &cam);
        if e.flags & 2 == 0 && depth > 0xDAC {
            return false;
        }
        let r = &mut self.slots[s as usize];
        r[..9].copy_from_slice(&cam[..9]);
        r[9] = side;
        r[10] = (e.pos[1] >> 8).wrapping_add(cam[10]);
        r[11] = depth;
        true
    }

    /// `FUN_08162048` and the fill of the object: the first free sprite of the pool (`used` marks it), kind (1
    /// one-shot, 3 …), frame, scale, size class, screen position, and the palette byte of an effect `material`.
    #[allow(clippy::too_many_arguments)]
    fn put_sprite(&mut self, kind: u8, frame: i16, scale: i16, size: i16, x: i16, y: i16, material: usize) {
        let palette = self.data.effects.palette[material];
        let Some(s) = self.pool.iter_mut().find(|s| s.used == 0) else {
            return;
        };
        *s = Sprite {
            x,
            y,
            used: 1,
            frame,
            scale_x: scale,
            scale_y: scale,
            angle: 0,
            size,
            kind,
            palette,
        };
    }

    fn centre(&self) -> (i16, i16) {
        (self.view.cx, self.view.cy)
    }

    /// The projection of a camera-space point through the view: its scale.
    fn scale(&self, z: i32) -> i32 {
        self.view.focal.wrapping_mul(recip(self.rom, z) >> 8)
    }

    /// `FUN_0814e414` (lights and flames at a fixed size, `0x0814e52c` for traffic with `traffic = true`): projects a
    /// point of the car through its slot and adds a sprite when on screen.
    #[allow(clippy::too_many_arguments)]
    fn light(&mut self, slot: usize, p: [i32; 3], radius: i32, frame: i32, size: i32, traffic: bool) {
        if self.g.lights == 0 {
            return;
        }
        let (cx, cy) = self.centre();
        let out = transform_point(&self.slots[slot], p);
        if (out[2] as u32).wrapping_sub(0x100) > 0x166F {
            return;
        }
        let s = self.scale(out[2]);
        if !on_screen(s, out, cx as i32, cy as i32) {
            return;
        }
        let mut material = if traffic { FLARE } else { 0 };
        if !traffic && frame == 0x1BC {
            material += FLARE;
        }
        let t = s.wrapping_mul(out[0].wrapping_sub(radius));
        let mut half = (((s.wrapping_mul(radius.wrapping_add(out[0])) >> 16) - (t >> 16)) >> 1) as i16;
        if !traffic && size == 1 {
            material += 1;
            half = half.wrapping_sub(0x10);
        }
        if size == 2 {
            half = half.wrapping_sub(0x20);
        }
        let x = ((t as u32 >> 16) as i16).wrapping_add(cx).wrapping_add(half);
        let y = ((s.wrapping_mul(out[1].wrapping_sub(radius)) as u32 >> 16) as i16)
            .wrapping_add(cy)
            .wrapping_add(half);
        self.put_sprite(3, frame as i16, 0x168, size as i16, x, y, material);
    }

    /// `spawn_effect_sprite` (`0x0814e7b0`): a one-shot sprite scaled with its depth.
    #[allow(clippy::too_many_arguments)]
    fn spawn_effect_sprite(&mut self, slot: usize, p: [i32; 3], radius: i32, frame: i32, size: i32, material: usize) {
        let (cx, cy) = self.centre();
        let out = transform_point(&self.slots[slot], p);
        if (out[2] as u32).wrapping_sub(8) > 0xF97 {
            return;
        }
        let s = self.scale(out[2]);
        if !on_screen(s, out, cx as i32, cy as i32) {
            return;
        }
        let t = s.wrapping_mul(out[0].wrapping_sub(radius));
        let w = (s.wrapping_mul(radius.wrapping_add(out[0])) >> 16) - (t >> 16);
        let scale = (recip(self.rom, w) >> 10) as i16;
        let mut half = (w >> 1) as i16;
        if size == 1 {
            half = half.wrapping_sub(0x10);
        }
        if size == 2 {
            half = half.wrapping_sub(0x20);
        }
        let x = ((t as u32 >> 16) as i16).wrapping_add(cx).wrapping_add(half);
        let y = ((s.wrapping_mul(out[1].wrapping_sub(radius)) as u32 >> 16) as i16)
            .wrapping_add(cy)
            .wrapping_add(half);
        self.put_sprite(1, frame as i16, scale, size as i16, x, y, material);
    }

    /// `FUN_0814eca4`: a billboard sprite at the slot's origin, centred on (0x70, 0x50).
    fn billboard(&mut self, slot: usize, radius: i32, frame: i16, size: i16) {
        let out = transform_point(&self.slots[slot], [0, 0, 0]);
        if (out[2] as u32).wrapping_sub(8) > 0xF97 {
            return;
        }
        let s = self.scale(out[2]);
        if !on_screen(s, out, 0x70, 0x50) {
            return;
        }
        let t = s.wrapping_mul(out[0].wrapping_sub(radius));
        let w = (s.wrapping_mul(radius.wrapping_add(out[0])) >> 16) - (t >> 16);
        let scale = (recip(self.rom, w) >> 10) as i16;
        let half = ((w >> 1) as i16).wrapping_sub(8);
        let x = ((t as u32 >> 16) as i16).wrapping_add(0x70).wrapping_add(half);
        let y = ((s.wrapping_mul(out[1].wrapping_sub(radius)) as u32 >> 16) as i16)
            .wrapping_add(0x50)
            .wrapping_add(half);
        self.put_sprite(3, frame, scale, size, x, y, BILLBOARD);
    }

    /// `opponent_effects` (`0x0814e628`) for entity `i`: seen from behind (angle between `heading` and `view` over
    /// 0x13FF), the two rear lights; otherwise, for entity 0, the brake lights while its driver brakes, and entity
    /// 0's extra model made positive.
    pub fn opponent_effects(&mut self, i: usize, heading: i32, view: i32, size: i32) {
        let e = &self.entities[i];
        let (car, slot) = (self.data.effects.cars[e.car as usize], e.slot);
        let Some(brake) = self.cars[i].as_ref().map(|c| c.brake) else {
            return;
        };
        if slot == 0xFF {
            return;
        }
        let at = slot as usize;
        let h = |o: usize| car[o / 2] as i32;
        if 0x13FF < angle_diff(heading, view).abs() {
            let (x, y, z) = (h(0x2C), h(0x2E), h(0x30) + 0x14);
            self.light(at, [x - 5, y, z], 0x40, 0x1BC, 2, false);
            self.light(at, [5 - x, y, z], 0x40, 0x1BC, 2, false);
        } else if self.entities[i].index == 0 {
            if brake != 0 {
                let (x, y, z) = (h(0x32), h(0x34), h(0x36));
                self.light(at, [x, y, z], size, 0x1FC, 1, false);
                self.light(at, [-x, y, z], size, 0x1FC, 1, false);
            }
            let extra = &mut self.entities[i].extra_model;
            if *extra < 0 {
                *extra = extra.wrapping_neg();
            }
        }
    }

    /// `FUN_0814e8b4`: the player's exhaust flames. Driver `+0x436` animates the nitro flames (−1 while nitro is
    /// off), `+0x434` the other pair; frames from the exhaust material.
    fn exhaust(&mut self, i: usize, heading: i32, view: i32) {
        let e = &self.entities[i];
        let slot = e.slot;
        if slot == 0xFF || 0x1000 < angle_diff(heading, view).abs() || self.phase == 9 {
            return;
        }
        let Some(mut car) = self.cars[i].take() else {
            return;
        };
        let id = e.car as usize;
        let row = id * 8 + 2 * self.records[id].exhaust as usize;
        let mut k = row * 3;
        let data = self.data;
        let table = |i: usize| data.effects.exhaust[i] as i32;
        let frames = (self.data.effects.frames[EXHAUST_MATERIAL] & 0xFF) as i32;
        let at = slot as usize;
        let chase = self.camera.view == 2;
        let phase = self.phase;
        // The flame points of both exhausts, mirrored in z for the nitro pair.
        let points = |k: usize, sign: i32| {
            [
                [table(k), table(k + 1), sign * table(k + 2)],
                [table(k + 3), table(k + 4), sign * table(k + 5)],
            ]
        };
        'nitro: {
            if car.nitro_on == 0 {
                car.u_436 = 0xFFFF;
                break 'nitro;
            }
            if (car.u_436 as i16) < 0 {
                car.u_436 = 0;
            }
            if chase {
                let frame = ((car.u_436 as i16) >> 8) as i32 * 4 + 0x1CC;
                for p in points(k, -1) {
                    if p[2] != 0 {
                        self.spawn_effect_sprite(at, p, 0x30, frame, 1, EXHAUST_MATERIAL);
                    }
                }
                k += 6;
            }
            if phase == 4 {
                break 'nitro;
            }
            let next = (car.u_436 as i16).wrapping_add(0x100);
            car.u_436 = next as u16;
            if (next as i32) < frames * 0x100 {
                break 'nitro;
            }
            car.u_436 = next.wrapping_sub((frames * 0x100) as i16) as u16;
        }
        if (car.u_434 as i16) >= 0 {
            if chase {
                let frame = ((car.u_436 as i16) >> 8) as i32 * 4 + 0x1CC;
                for p in points(k, 1) {
                    if p[2] != 0 {
                        self.spawn_effect_sprite(at, p, 0x30, frame, 1, EXHAUST_MATERIAL);
                    }
                }
            }
            if phase != 4 {
                let next = car.u_434.wrapping_add(0x100);
                car.u_434 = next;
                if frames * 0x100 <= next as i16 as i32 {
                    car.u_434 = 0xFFFF;
                }
            }
        }
        self.cars[i] = Some(car);
    }

    /// `FUN_08137534`: the first entity at world `+0xF8` and after (world `+0xFA` of them) whose state bit 0 is clear.
    fn free_entity(&self) -> Option<usize> {
        let (first, count) = self.spare;
        self.entities[first..first + count]
            .iter()
            .find(|e| e.state & 1 == 0)
            .map(|e| e.index as usize)
    }

    /// `FUN_081375ac`: unlink entity `index` from its sector's entity list.
    fn unlink_entity(&mut self, index: usize) {
        let sector = self.entities[index].sector as u32;
        if sector == NONE {
            return;
        }
        let link = self.entities[index].next;
        let mut cur = self.heads[sector as usize] as usize;
        if cur == index {
            self.heads[sector as usize] = link;
            return;
        }
        loop {
            let next = self.entities[cur].next as u32;
            if next == NONE {
                return;
            }
            if next as usize == index {
                self.entities[cur].next = link;
                return;
            }
            cur = next as usize;
        }
    }

    /// `FUN_08137578`: push entity `index` onto its sector's entity list.
    fn link_entity(&mut self, index: usize) {
        let sector = self.entities[index].sector;
        if sector as u32 != NONE {
            self.entities[index].next = self.heads[sector as usize];
            self.heads[sector as usize] = index as u16;
        }
    }

    fn rand(&mut self) -> i32 {
        nfsgba_fixed::rand_table(self.rom, &mut self.g.rand) as i32
    }

    /// `FUN_0814c37c`: a spark entity (handler 0x34) at entity `src`'s position plus `p` rotated by `r`, flying off
    /// with the car's velocity / 0xC4 plus random spread.
    fn spawn_spark(&mut self, src: usize, p: [i32; 3], r: &M) {
        let free = self.free_entity();
        let Some(i) = free.filter(|_| self.phase != 4) else {
            return;
        };
        let o = rotate_vector(r, p);
        let (pos, vel, sector) = {
            let e = &self.entities[src];
            (e.pos, (e.dir_x, e.dir_z), e.sector)
        };
        let s = &mut self.entities[i];
        s.draw_next = 0xFFFF;
        s.state = 7;
        s.material_offset = 0;
        s.next = 0xFFFF;
        s.model = 0;
        s.material_step = 0;
        s.sector = sector;
        for k in 0..3 {
            s.pos[k] = pos[k].wrapping_add(o[k]);
        }
        s.dir_x = 0;
        s.dir_z = 0;
        s.handler = 0x34;
        s.material = 0;
        let r1 = self.rand();
        self.entities[i].u_1c = -0x200 - (r1 >> 5);
        let r2 = self.rand();
        self.entities[i].u_70 = (((r2 - 0x3FFF) >> 6) as i16).wrapping_add(0x200) as u16;
        let r3 = self.rand();
        self.entities[i].dir_x = div(vel.0, 0xC4).wrapping_add((r3 - 0x3FFF) >> 6);
        let r4 = self.rand();
        self.entities[i].dir_z = div(vel.1, 0xC4).wrapping_add((r4 - 0x3FFF) >> 6);
        self.entities[i].u_1c >>= 2;
        self.link_entity(i);
    }

    /// `effect_handler` (`0x0814c49c`, entity handler 0x34) for entity `i`: a spark moves by its velocity (scaled by
    /// timer 3's ticks), ages by 0x80 per frame and dies after the billboard's frame count; alive, it takes a
    /// billboard slot and a sprite.
    pub fn effect_handler(&mut self, i: usize) {
        if self.phase != 4 {
            let dt = self.g.timer3;
            let e = &mut self.entities[i];
            for (p, v) in [(0, e.dir_x), (1, e.u_1c), (2, e.dir_z)] {
                e.pos[p] = e.pos[p].wrapping_add(dt.wrapping_mul(v) >> 8);
            }
            e.material_step = e.material_step.wrapping_add(0x80);
            if (self.data.effects.frames[BILLBOARD] as u32) * 0x100 <= e.material_step as u32 {
                e.state = 0;
                let index = e.index as usize;
                self.unlink_entity(index);
                return;
            }
        }
        if self.assign_billboard_slot(i) {
            let e = &self.entities[i];
            let frame = ((e.material_step >> 8) * 4 + 0x1E4) as i16;
            self.billboard(e.slot as usize, 0x30, frame, 1);
        }
    }

    /// `FUN_0814fa6c`, all that is left of it: it finds the sector under each of the four wheel points around the
    /// car (heading only), and its only effect is the last query it leaves in the world scratch (the sector search
    /// and the floor height it computes are not kept).
    fn wheel_points(&mut self, i: usize) {
        let e = &self.entities[i];
        let r = rotation_y(self.rom, e.heading >> 8);
        // The fourth point, (0x20, 0, -0x2A), rotated by the heading (the earlier points leave nothing behind).
        let p = [0x20, 0, -0x2A];
        let x = r[0]
            .wrapping_mul(p[0])
            .wrapping_mul(0x100)
            .wrapping_add(p[2].wrapping_mul(0x100).wrapping_mul(r[6]))
            >> 14;
        let z = r[2]
            .wrapping_mul(p[0])
            .wrapping_mul(0x100)
            .wrapping_add(p[2].wrapping_mul(0x100).wrapping_mul(r[8]))
            >> 14;
        let (qx, qz) = (e.pos[0].wrapping_add(x) >> 8, e.pos[2].wrapping_add(z) >> 8);
        self.query = ([qx, e.pos[1] >> 8, qz], e.sector);
    }

    /// Two spark points at the rear corners of a car (car table `+0x26/+0x28/+0x2A`), for `FUN_0814c37c`.
    fn spark_points(&self, i: usize) -> [[i32; 3]; 2] {
        let car = self.data.effects.cars[self.entities[i].car as usize];
        let (x, y, z) = (car[19] as i32, car[20] as i32 + 0xC, car[21] as i32);
        [[x << 8, y * 0x100, z << 8], [x * -0x100, y * 0x100, z << 8]]
    }

    /// `FUN_0814e050` with the physics orientation set (the only case the car step runs): the slot from the
    /// driver's physics orientation, the sparks of a car-to-car contact, and the spoiler's slot.
    fn player_matrix(&mut self, i: usize, slot: usize) -> Result<()> {
        if self.g.physics_orientation == 0 {
            return Err(Unported(
                "FUN_0814e050 without the physics orientation (the wheel-plane orientation)",
            ));
        }
        let r = orientation(self.cars[i].as_ref());
        if self.camera.raised != 0 {
            return Err(Unported("FUN_08140224 (the raised camera's orientation)"));
        }
        if self.g.sparks_from_ai == 0 {
            let index = self.entities[i].index as u32;
            if index == self.player && 0 < self.racer_hits.get(index as usize).copied().unwrap_or(0) {
                for p in self.spark_points(i) {
                    self.spawn_spark(i, p, &r);
                }
            }
        } else {
            let w = i + self.g.ai_cars as usize;
            for p in self.spark_points(w) {
                self.spawn_spark(w, p, &r);
            }
        }
        let cam = self.camera.matrix;
        let mut out = self.slots[slot];
        mul(&r, &cam, &mut out, true);
        let e = &self.entities[i];
        let (side, depth) = eye(e, &cam);
        out[9] = side;
        out[10] = (e.pos[1] >> 8).wrapping_add(cam[10]);
        out[11] = depth;
        self.slots[slot] = out;
        if e.extra_model != 0 {
            let car = e.car as usize;
            let at = self.data.effects.spoiler[self.records[car].spoiler as usize + car * 16];
            let p = [0, at[0] as i32, (at[1] as i32).wrapping_neg()];
            let mut spoiler = out;
            spoiler[9..].copy_from_slice(&transform_point(&out, p));
            self.slots[slot + 1] = spoiler;
        }
        Ok(())
    }

    /// `build_player_matrices` (`0x0814bc30`) for entity `i`: the player's body and spoiler slots, its lights and
    /// flames, and the horizon shift in the views before the chase view.
    fn build_player_matrices(&mut self, i: usize) -> Result<()> {
        let heading = ((self.entities[i].heading as u32 & 0x3F_FFFF) >> 8) as i32;
        let s = self.next_slot();
        self.entities[i].slot = s;
        if s != 0xFF {
            self.next_slot();
            self.wheel_points(i);
            self.player_matrix(i, s as usize)?;
            if self.camera.view != 0 {
                let view = 0x4000 - self.camera.matrix_yaw;
                self.opponent_effects(i, heading, view, 0x20);
                self.exhaust(i, heading, view);
            }
        }
        let e = &self.entities[i];
        if self.camera.view < 2 && e.index as u32 == self.player && e.slot != 0xFF {
            self.camera.horizon = self.slots[e.slot as usize][7].wrapping_mul(-0x80) >> 14;
        }
        Ok(())
    }

    /// `FUN_08144c98`: the live traffic cars' slots and, seen from behind, their rear lights.
    fn traffic_slots(&mut self) {
        if self.g.traffic_on == 0 {
            return;
        }
        for t in self.traffic.into_iter().flatten() {
            if !matches!(self.entities[t].race_state as i16, 1 | 3) {
                continue;
            }
            self.assign_entity_slot(t);
            let e = &self.entities[t];
            if e.slot != 0xFF && 0x13FF < angle_diff(e.angles[1] as i32, 0x4000 - self.camera.matrix_yaw).abs() {
                let at = e.slot as usize;
                self.light(at, [-0x19, -0x20, 100], 0x40, 0x1BC, 2, true);
                self.light(at, [0x19, -0x20, 100], 0x40, 0x1BC, 2, true);
            }
        }
    }

    /// The slot part of `race_frame_update` after the camera: the player's slots, each other racer's slot and
    /// `opponent_effects`, then the traffic.
    pub fn race_slots(&mut self) -> Result<()> {
        if self.link == 2 {
            return Err(Unported("link play: build_player_matrices for every racer"));
        }
        let player = self.player as usize;
        self.build_player_matrices(player)?;
        for i in 0..(self.g.ai_cars + 1).max(0) as usize {
            if i == player {
                continue;
            }
            self.assign_entity_slot(i);
            let size = if self.entities[i].speed < 0x80 { 0x40 } else { 0x20 };
            let heading = (self.entities[i].heading >> 8) & 0x3FFF;
            self.opponent_effects(i, heading, 0x3FFF - self.camera.matrix_yaw, size);
        }
        self.traffic_slots();
        Ok(())
    }
}

/// What the player's rim redraw reads (`World::rim_redraw`).
pub struct RimFrame {
    pub phase: u32,
    pub player: u32,
    /// The camera view and matrix yaw.
    pub view: u32,
    pub matrix_yaw: i32,
    pub entity: Entity,
    /// The car's wheel angle (`>> 8` rotates the rim), if it has a driver.
    pub wheel_angle: Option<i32>,
    pub record: CarRecord,
    /// The car's atlas and the rim buffer, as byte offsets into the heap (0: none).
    pub atlas: usize,
    pub pixels: usize,
    /// Every material's size.
    pub materials: Vec<MaterialInfo>,
}

/// The player's rim redraw at the start of `car_racing_step` (`0x0814b168`): seen from the side
/// (`rim_side_visible`), `draw_decal_on_atlas` (`0x0813bd90`) draws the rim into the car's atlas, rotated by the
/// wheel angle. The rotated read can reach up to 63 bytes around the rim's buffer on the game's heap; here it
/// reads the real `heap`, so it is exact (FIDELITY R24), unless that window overlaps the atlas it writes, which
/// this refuses.
pub fn rim_redraw(rom: &[u8], data: &GameData, f: &RimFrame, heap: &mut [u8]) -> Result<()> {
    let e = &f.entity;
    if f.phase == 0 || f.phase == 1 || f.phase == 4 || e.index as u32 != f.player || f.view == 0 {
        return Ok(());
    }
    let d = angle_diff(((e.heading as u32 & 0x3F_FFFF) >> 8) as i32, 0x4000 - f.matrix_yaw).abs();
    if d < 0x400 || (0x1C00 < d && d < 0x2400) {
        return Ok(());
    }
    if f.atlas == 0 {
        return Ok(());
    }
    let entry = data.effects.rims[e.car as usize * 15 + f.record.rim as usize];
    if entry[0] == -1 {
        return Ok(());
    }
    let material = entry[2] as i32;
    let info = &f.materials[material as usize];
    let rim = atlas::Rim {
        material: material as usize,
        width: info.width as usize,
        height: info.height as usize,
        first: (entry[0] as i32, entry[1] as i32),
        second: (entry[4] as i32, entry[5] as i32),
    };
    let angle = f.wheel_angle.map_or(0, |a| a >> 8);
    let (atlas_at, pixels) = (f.atlas as i64, f.pixels as i64);
    let read = pixels - 0x40..pixels + (rim.width * rim.height) as i64 + 0x40;
    // The box `draw_rim` writes: rows and columns inset by 1/8 at `first`, and again shifted to `second`.
    let (w, h) = (rim.width as i32, rim.height as i32);
    let (x0, y0) = (rim.first.0 + (w >> 3), rim.first.1 + (h >> 3));
    let (x1, y1) = (rim.first.0 + w - (w >> 3), rim.first.1 + h - (h >> 3));
    let second = (rim.second.1 - rim.first.1) * 0x100 + rim.second.0 - rim.first.0;
    let (lo, hi) = (y0 * 0x100 + x0, (y1 - 1) * 0x100 + x1);
    let written = atlas_at + (lo + second.min(0)) as i64..atlas_at + (hi + second.max(0)) as i64;
    if x0 < x1 && y0 < y1 && read.start < written.end && written.start < read.end {
        return Err(Unported("the rim redraw reads the atlas it writes (heap layout)"));
    }
    let around = heap.to_vec();
    atlas::draw_rim(rom, &mut heap[f.atlas..], &rim, &around, f.pixels, angle);
    Ok(())
}
