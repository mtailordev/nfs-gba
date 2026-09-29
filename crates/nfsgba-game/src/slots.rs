//! The per-frame matrix slots (world `+0xFC`, 0x30 bytes each, counter `0x03005394`), the 2D effect sprites the
//! race puts into the sprite pool `*0x03000058` (rear lights, brake lights, exhaust flames, sparks), the spark
//! entities (handler 0x34) and the player's rim redraw. Every function names the game function it is.

use nfsgba_formats::atlas;
use nfsgba_sim::{
    Mem, Result, Unported,
    layout::Field,
    math::{angle_diff, cos, div, sin},
    state::Camera,
    traffic::rand,
    world::{self, NONE, W_ENTITIES, W_QUERY, W_QUERY_SECTOR},
};

use crate::view::WORLD;

/// A 3×4 matrix, 2.14 rotation (row-major) then translation.
pub type M = [i32; 12];

const SLOT_COUNTER: u32 = 0x0300_5394;
const SPRITES: u32 = 0x0300_0058;
const PHASE: u32 = 0x0300_0048;
const PLAYER: u32 = 0x0300_0060;
const VIEW: u32 = 0x0300_0080;
const PHYSICS_ORIENTATION: u32 = 0x0300_610C;
const CAR_TABLE: u32 = 0x087F_0BD8;
const EFFECT_MATERIALS: u32 = 0x0836_CF5C;

pub fn load(m: &Mem, at: u32) -> M {
    std::array::from_fn(|k| m.i32(at + 4 * k as u32))
}

pub fn store(m: &mut Mem, at: u32, v: &M) {
    for (k, x) in v.iter().enumerate() {
        m.set_i32(at + 4 * k as u32, *x);
    }
}

fn view_mode(m: &Mem) -> u32 {
    Camera::load(m, 0).view
}

fn matrix_yaw(m: &Mem) -> i32 {
    Camera::load(m, 0).matrix_yaw
}

/// `rotation_y` (`0x08160624`).
pub fn rotation_y(rom: &[u8], a: i32) -> M {
    let (c, s) = (nfsgba_fixed::cos_q14(rom, a), nfsgba_fixed::sin_q14(rom, a));
    [c, 0, s.wrapping_neg(), 0, 0x4000, 0, s, 0, c, 0, 0, 0]
}

/// IWRAM `0x081694b0` (ROM copy): the rotation about x.
fn rotation_x(m: &Mem, a: i32) -> M {
    let (c, s) = (cos(m, a), sin(m, a));
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

/// `FUN_0814f8ac`: the next slot, 0xFF past 0x3F.
fn next_slot(m: &mut Mem) -> u8 {
    let s = m.u32(SLOT_COUNTER);
    m.set_u32(SLOT_COUNTER, s.wrapping_add(1));
    if 0x3F < s.wrapping_add(1) as i32 { 0xFF } else { s as u8 }
}

fn slot_addr(m: &Mem, slot: u8) -> u32 {
    m.u32(WORLD + 0xFC) + 0x30 * slot as u32
}

fn camera(m: &Mem) -> M {
    load(m, m.u32(WORLD + 0x54))
}

/// Camera-space x and depth of an entity (`(pos >> 8 + t) · R`).
fn eye(e: u32, m: &Mem, cam: &M) -> (i32, i32) {
    let x = (m.i32(e + 0x0C) >> 8).wrapping_add(cam[9]);
    let z = (m.i32(e + 0x14) >> 8).wrapping_add(cam[11]);
    let side = x.wrapping_mul(cam[0]).wrapping_add(z.wrapping_mul(cam[6])) >> 14;
    let depth = x.wrapping_mul(cam[2]).wrapping_add(z.wrapping_mul(cam[8])) >> 14;
    (side, depth)
}

/// `build_entity_matrix` (`0x0814da68`): `R(+0x30 about x) · camera · R(+0x32 about y)`, or with `+0x0A` bit 5 (and
/// `0x0300610C`) the driver's physics orientation times the camera; then the camera-space position. Nothing is
/// written beyond depth 0xDAC without `+0x0A` bit 1 (the slot keeps last frame's matrix).
fn build_entity_matrix(m: &mut Mem, e: u32, out: u32) {
    let cam = camera(m);
    let (side, depth) = eye(e, m, &cam);
    let flags = m.u16(e + 0x0A);
    if flags & 2 == 0 && depth > 0xDAC {
        return;
    }
    let mut r = load(m, out);
    if flags & 0x20 == 0 || m.i32(PHYSICS_ORIENTATION) == 0 {
        let (a, b) = (
            rotation_x(m, m.i16(e + 0x30) as i32),
            rotation_y(&m.rom, m.i16(e + 0x32) as i32),
        );
        let mut tmp = [0; 12];
        mul(&a, &cam, &mut tmp, false);
        mul(&tmp, &b, &mut r, false);
    } else {
        let d = m.u32(e + 0x8C);
        let mut p = [0; 12];
        for (k, v) in p.iter_mut().take(9).enumerate() {
            *v = m.i32(d + 0x128 + 4 * k as u32) << 2;
        }
        mul(&p, &cam, &mut r, false);
    }
    r[9] = side;
    r[10] = (m.i32(e + 0x10) >> 8).wrapping_add(cam[10]);
    r[11] = depth;
    store(m, out, &r);
}

/// `assign_entity_slot` (`0x0814eba0`): a slot for an entity with a model that was drawn last frame (`+0x0A`
/// bit 2), else `0xFF`; clears bit 2.
fn assign_entity_slot(m: &mut Mem, e: u32) {
    if m.i16(e + 0x36) == 0 || m.u16(e + 0x0A) & 4 == 0 {
        m.set_u8(e + 0x88, 0xFF);
    } else {
        let s = next_slot(m);
        m.set_u8(e + 0x88, s);
        if s != 0xFF {
            let at = slot_addr(m, s);
            build_entity_matrix(m, e, at);
        }
    }
    m.set_u16(e + 0x0A, m.u16(e + 0x0A) & 0xFFFB);
}

/// `assign_billboard_slot` (`0x0814ec0c`): a slot with the camera rotation and the entity's camera-space position;
/// whether it was built.
fn assign_billboard_slot(m: &mut Mem, e: u32) -> bool {
    let s = next_slot(m);
    m.set_u8(e + 0x88, s);
    if s == 0xFF {
        return false;
    }
    let cam = camera(m);
    let (side, depth) = eye(e, m, &cam);
    if m.u16(e + 0x0A) & 2 == 0 && depth > 0xDAC {
        return false;
    }
    let at = slot_addr(m, s);
    let mut r = load(m, at);
    r[..9].copy_from_slice(&cam[..9]);
    r[9] = side;
    r[10] = (m.i32(e + 0x10) >> 8).wrapping_add(cam[10]);
    r[11] = depth;
    store(m, at, &r);
    true
}

/// `FUN_08162048`: the first free object of the effect-sprite pool (`+4` marks it used), or 0.
fn alloc_sprite(m: &mut Mem) -> u32 {
    let (mut at, count) = (m.u32(SPRITES), m.i16(SPRITES + 6) as i32);
    for _ in 0..count {
        if m.i16(at + 4) == 0 {
            m.set_u16(at + 4, 1);
            return at;
        }
        at += 0x14;
    }
    0
}

/// Fills a sprite object: kind (1 one-shot, 3 …), frame, scale, size class, screen position, palette byte.
#[allow(clippy::too_many_arguments)]
fn put_sprite(m: &mut Mem, kind: u8, frame: i16, scale: i16, size: i16, x: i16, y: i16, material: u32) {
    let s = alloc_sprite(m);
    if s == 0 {
        return;
    }
    m.set_u8(s + 0x12, kind);
    m.set_i16(s + 0xE, 0);
    m.set_i16(s + 8, frame);
    m.set_i16(s + 0xA, scale);
    m.set_i16(s + 0xC, scale);
    m.set_i16(s + 0x10, size);
    m.set_i16(s, x);
    m.set_i16(s + 2, y);
    m.set_u8(s + 0x13, m.u16(material + 0x20) as u8);
}

fn view_centre(m: &Mem) -> (i16, i16) {
    (m.i16(VIEW + 8), m.i16(VIEW + 0xA))
}

/// A projected point (screen scale `s`, camera-space `out`) inside the 240×160 screen around (`cx`, `cy`).
fn on_screen(s: i32, out: [i32; 3], cx: i32, cy: i32) -> bool {
    let x = (s.wrapping_mul(out[0]) >> 16) + cx;
    let y = (s.wrapping_mul(out[1]) >> 16) + cy;
    0 < x && x < 0xF0 && 0 < y && y < 0xA0
}

/// `FUN_0814e414` (lights and flames at a fixed size, `0x0814e52c` for traffic with `traffic = true`): projects a
/// point of the car through its slot and adds a sprite when on screen.
#[allow(clippy::too_many_arguments)]
fn light(m: &mut Mem, slot: u32, p: [i32; 3], radius: i32, frame: i32, size: i32, traffic: bool) {
    if m.u32(0x0300_53E8) == 0 {
        return;
    }
    let (cx, cy) = view_centre(m);
    let out = transform_point(&load(m, slot), p);
    if (out[2] as u32).wrapping_sub(0x100) > 0x166F {
        return;
    }
    let s = m
        .i32(VIEW + 0x1C)
        .wrapping_mul(nfsgba_fixed::recip(&m.rom, out[2]) >> 8);
    if !on_screen(s, out, cx as i32, cy as i32) {
        return;
    }
    let mut material = if traffic { 0x0836_F698 } else { EFFECT_MATERIALS };
    if !traffic && frame == 0x1BC {
        material += 0x273C;
    }
    let t = s.wrapping_mul(out[0].wrapping_sub(radius));
    let mut half = (((s.wrapping_mul(radius.wrapping_add(out[0])) >> 16) - (t >> 16)) >> 1) as i16;
    if !traffic && size == 1 {
        material += 0x24;
        half = half.wrapping_sub(0x10);
    }
    if size == 2 {
        half = half.wrapping_sub(0x20);
    }
    let x = ((t as u32 >> 16) as i16).wrapping_add(cx).wrapping_add(half);
    let y = ((s.wrapping_mul(out[1].wrapping_sub(radius)) as u32 >> 16) as i16)
        .wrapping_add(cy)
        .wrapping_add(half);
    put_sprite(m, 3, frame as i16, 0x168, size as i16, x, y, material);
}

/// `spawn_effect_sprite` (`0x0814e7b0`): a one-shot sprite scaled with its depth.
#[allow(clippy::too_many_arguments)]
fn spawn_effect_sprite(m: &mut Mem, slot: u32, p: [i32; 3], radius: i32, frame: i32, size: i32, material: u32) {
    let (cx, cy) = view_centre(m);
    let out = transform_point(&load(m, slot), p);
    if (out[2] as u32).wrapping_sub(8) > 0xF97 {
        return;
    }
    let s = m
        .i32(VIEW + 0x1C)
        .wrapping_mul(nfsgba_fixed::recip(&m.rom, out[2]) >> 8);
    if !on_screen(s, out, cx as i32, cy as i32) {
        return;
    }
    let material = EFFECT_MATERIALS + material * 0x24;
    let t = s.wrapping_mul(out[0].wrapping_sub(radius));
    let w = (s.wrapping_mul(radius.wrapping_add(out[0])) >> 16) - (t >> 16);
    let scale = (nfsgba_fixed::recip(&m.rom, w) >> 10) as i16;
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
    put_sprite(m, 1, frame as i16, scale, size as i16, x, y, material);
}

/// `FUN_0814eca4`: a billboard sprite at the slot's origin, centred on (0x70, 0x50).
fn billboard(m: &mut Mem, slot: u32, radius: i32, frame: i16, size: i16) {
    let out = transform_point(&load(m, slot), [0, 0, 0]);
    if (out[2] as u32).wrapping_sub(8) > 0xF97 {
        return;
    }
    let s = m
        .i32(VIEW + 0x1C)
        .wrapping_mul(nfsgba_fixed::recip(&m.rom, out[2]) >> 8);
    if !on_screen(s, out, 0x70, 0x50) {
        return;
    }
    let t = s.wrapping_mul(out[0].wrapping_sub(radius));
    let w = (s.wrapping_mul(radius.wrapping_add(out[0])) >> 16) - (t >> 16);
    let scale = (nfsgba_fixed::recip(&m.rom, w) >> 10) as i16;
    let half = ((w >> 1) as i16).wrapping_sub(8);
    let x = ((t as u32 >> 16) as i16).wrapping_add(0x70).wrapping_add(half);
    let y = ((s.wrapping_mul(out[1].wrapping_sub(radius)) as u32 >> 16) as i16)
        .wrapping_add(0x50)
        .wrapping_add(half);
    put_sprite(m, 3, frame, scale, size, x, y, 0x0836_D010);
}

/// `opponent_effects` (`0x0814e628`): seen from behind (angle between `heading` and `view` over 0x13FF), the two
/// rear lights; otherwise, for entity 0, the brake lights while its driver brakes (`+0x28`), and entity 0's
/// `+0x64` made positive.
pub fn opponent_effects(m: &mut Mem, e: u32, heading: i32, view: i32, size: i32) {
    let car = CAR_TABLE + 0x58 * m.u8(e + 0x89) as u32;
    let d = m.u32(e + 0x8C);
    let slot = m.u8(e + 0x88);
    if d == 0 || slot == 0xFF {
        return;
    }
    let at = slot_addr(m, slot);
    let h = |o: u32| m.i16(car + o) as i32;
    if 0x13FF < angle_diff(heading, view).abs() {
        let (x, y, z) = (h(0x2C), h(0x2E), h(0x30) + 0x14);
        light(m, at, [x - 5, y, z], 0x40, 0x1BC, 2, false);
        light(m, at, [5 - x, y, z], 0x40, 0x1BC, 2, false);
    } else if m.u16(e) == 0 {
        if m.i32(d + 0x28) != 0 {
            let (x, y, z) = (h(0x32), h(0x34), h(0x36));
            light(m, at, [x, y, z], size, 0x1FC, 1, false);
            light(m, at, [-x, y, z], size, 0x1FC, 1, false);
        }
        if m.i16(e + 0x64) < 0 {
            m.set_i16(e + 0x64, m.i16(e + 0x64).wrapping_neg());
        }
    }
}

/// `FUN_0814e8b4`: the player's exhaust flames. Driver `+0x436` animates the nitro flames (−1 while nitro is off),
/// `+0x434` the other pair; frames from material `0x0836D304 +0x10`.
fn exhaust(m: &mut Mem, e: u32, heading: i32, view: i32) {
    let slot = m.u8(e + 0x88);
    if slot == 0xFF || 0x1000 < angle_diff(heading, view).abs() || m.u32(PHASE) == 9 {
        return;
    }
    let d = m.u32(e + 0x8C);
    let car = m.u8(e + 0x89) as u32;
    let record = m.u32(0x0300_539C) + car * 0x11;
    let row = car * 8 + 2 * m.u8(record + 3) as u32;
    let mut k = row * 3;
    let table = |m: &Mem, i: u32| m.i16(0x087F_3E00 + 2 * i) as i32;
    let frames = (m.u16(0x0836_D304 + 0x10) & 0xFF) as i32;
    let at = slot_addr(m, slot);
    let chase = view_mode(m) == 2;
    'nitro: {
        if m.u8(d + 0x4D1) == 0 {
            m.set_i16(d + 0x436, -1);
            break 'nitro;
        }
        if m.i16(d + 0x436) < 0 {
            m.set_i16(d + 0x436, 0);
        }
        if chase {
            let frame = ((m.u16(d + 0x436) as i16) >> 8) as i32 * 4 + 0x1CC;
            let a = [table(m, k), table(m, k + 1), -table(m, k + 2)];
            let b = [table(m, k + 3), table(m, k + 4), -table(m, k + 5)];
            k += 6;
            if a[2] != 0 {
                spawn_effect_sprite(m, at, a, 0x30, frame, 1, 0x1A);
            }
            if b[2] != 0 {
                spawn_effect_sprite(m, at, b, 0x30, frame, 1, 0x1A);
            }
        }
        if m.u32(PHASE) == 4 {
            break 'nitro;
        }
        let s = m.i16(d + 0x436);
        let next = s.wrapping_add(0x100);
        m.set_i16(d + 0x436, next);
        if (next as i32) < frames * 0x100 {
            break 'nitro;
        }
        m.set_i16(d + 0x436, next.wrapping_sub((frames * 0x100) as i16));
    }
    if m.i16(d + 0x434) < 0 {
        return;
    }
    if chase {
        let frame = ((m.u16(d + 0x436) as i16) >> 8) as i32 * 4 + 0x1CC;
        let a = [table(m, k), table(m, k + 1), table(m, k + 2)];
        let b = [table(m, k + 3), table(m, k + 4), table(m, k + 5)];
        if a[2] != 0 {
            spawn_effect_sprite(m, at, a, 0x30, frame, 1, 0x1A);
        }
        if b[2] != 0 {
            spawn_effect_sprite(m, at, b, 0x30, frame, 1, 0x1A);
        }
    }
    if m.u32(PHASE) != 4 {
        let old = m.u16(d + 0x434);
        let next = old.wrapping_add(0x100);
        m.set_u16(d + 0x434, next);
        if frames * 0x100 <= next as i16 as i32 {
            m.set_u16(d + 0x434, 0xFFFF);
        }
    }
}

/// `FUN_08137534`: the first entity at world `+0xF8` and after (world `+0xFA` of them) whose state bit 0 is clear.
fn free_entity(m: &Mem) -> u32 {
    let (first, count) = (m.u16(WORLD + 0xF8) as u32, m.u16(WORLD + 0xFA) as u32);
    (0..count)
        .map(|k| world::entity(m, first + k))
        .find(|&e| m.u16(e + 8) & 1 == 0)
        .map_or(NONE, |e| m.u16(e) as u32)
}

/// `FUN_0814c37c`: a spark entity (handler 0x34) at `e`'s position plus `p` rotated by `r`, flying off with the
/// car's velocity / 0xC4 plus random spread.
fn spawn_spark(m: &mut Mem, e: u32, p: [i32; 3], r: &M) -> u32 {
    let i = free_entity(m);
    if m.u32(PHASE) == 4 || i == NONE {
        return NONE;
    }
    let s = world::entity(m, i);
    let o = rotate_vector(r, p);
    m.set_u16(s + 4, m.u16(s + 4) | 0xFFFF);
    m.set_u16(s + 8, 7);
    m.set_u16(s + 0x46, 0);
    m.set_u16(s + 2, m.u16(s + 2) | 0xFFFF);
    m.set_u16(s + 0x36, 0);
    m.set_u16(s + 0x44, 0);
    m.set_u16(s + 0x78, m.u16(e + 0x78));
    for k in 0..3 {
        m.set_i32(s + 0x0C + 4 * k, m.i32(e + 0x0C + 4 * k).wrapping_add(o[k as usize]));
    }
    m.set_i32(s + 0x18, 0);
    m.set_i32(s + 0x20, 0);
    let r1 = rand(m) as i32;
    m.set_i32(s + 0x1C, -0x200 - (r1 >> 5));
    m.set_u16(s + 0x4E, 0x34);
    m.set_u16(s + 0x48, 0);
    let r2 = rand(m) as i32;
    m.set_i16(s + 0x70, (((r2 - 0x3FFF) >> 6) as i16).wrapping_add(0x200));
    m.set_u32(s + 0x8C, 0);
    let r3 = rand(m) as i32;
    m.set_i32(s + 0x18, div(m.i32(e + 0x18), 0xC4).wrapping_add((r3 - 0x3FFF) >> 6));
    let r4 = rand(m) as i32;
    m.set_i32(s + 0x20, div(m.i32(e + 0x20), 0xC4).wrapping_add((r4 - 0x3FFF) >> 6));
    m.set_i32(s + 0x1C, m.i32(s + 0x1C) >> 2);
    world::link_entity(m, i);
    i
}

/// `effect_handler` (`0x0814c49c`, entity handler 0x34): a spark moves by its velocity (scaled by timer 3's
/// ticks), ages by 0x80 per frame and dies after `0x0836D010 +0x10` frames; alive, it takes a billboard slot and a
/// sprite.
pub fn effect_handler(m: &mut Mem, e: u32) {
    if m.u32(PHASE) != 4 {
        let dt = m.i32(0x0300_5934);
        for (p, v) in [(0x0C, 0x18), (0x10, 0x1C), (0x14, 0x20)] {
            m.set_i32(e + p, m.i32(e + p).wrapping_add(dt.wrapping_mul(m.i32(e + v)) >> 8));
        }
        let age = m.u16(e + 0x44).wrapping_add(0x80);
        m.set_u16(e + 0x44, age);
        if (m.u16(0x0836_D010 + 0x10) as u32) * 0x100 <= age as u32 {
            m.set_u16(e + 8, 0);
            world::unlink_entity(m, m.u16(e) as u32);
            return;
        }
    }
    if assign_billboard_slot(m, e) {
        let at = slot_addr(m, m.u8(e + 0x88));
        let frame = ((m.u16(e + 0x44) >> 8) * 4 + 0x1E4) as i16;
        billboard(m, at, 0x30, frame, 1);
    }
}

/// `FUN_0814fa6c`: the four wheel points around the car (heading only), each on the floor of the sector found
/// for it; leaves the last query in world `+0xC0..+0xEA`.
fn wheel_points(m: &mut Mem, e: u32, points: &mut [[i32; 3]; 4]) {
    let r = rotation_y(&m.rom, m.i32(e + 0x2C) >> 8);
    for p in points.iter_mut() {
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
        let (qx, qz) = (
            m.i32(e + 0x0C).wrapping_add(x) >> 8,
            m.i32(e + 0x14).wrapping_add(z) >> 8,
        );
        m.set_i32(W_QUERY, qx);
        m.set_i32(W_QUERY + 4, m.i32(e + 0x10) >> 8);
        m.set_i32(W_QUERY + 8, qz);
        m.set_u16(W_QUERY_SECTOR, m.u16(e + 0x78));
        let mut s = world::find_sector_near_query(m);
        if s == NONE {
            s = m.u16(W_QUERY_SECTOR) as u32;
        }
        p[0] = x;
        p[2] = z;
        if s != NONE {
            p[1] = world::floor_height(m, s, qx, qz);
        }
    }
}

/// Two spark points at the rear corners of a car (car table `+0x26/+0x28/+0x2A`), for `FUN_0814c37c`.
fn spark_points(m: &Mem, e: u32) -> [[i32; 3]; 2] {
    let car = CAR_TABLE + 0x58 * m.u8(e + 0x89) as u32;
    let (x, y, z) = (
        m.i16(car + 0x26) as i32,
        m.i16(car + 0x28) as i32 + 0xC,
        m.i16(car + 0x2A) as i32,
    );
    [[x << 8, y * 0x100, z << 8], [x * -0x100, y * 0x100, z << 8]]
}

/// `FUN_0814e050` with `0x0300610C` set (the only case the car step runs): the slot from the driver's physics
/// orientation, the sparks of a car-to-car contact, and the spoiler's slot.
fn player_matrix(m: &mut Mem, e: u32, slot: u32) -> Result<()> {
    let d = m.u32(e + 0x8C);
    if m.i32(PHYSICS_ORIENTATION) == 0 {
        return Err(Unported(
            "FUN_0814e050 without 0x0300610C (the wheel-plane orientation)",
        ));
    }
    let mut r = [0; 12];
    for (k, v) in r.iter_mut().take(9).enumerate() {
        *v = m.i32(d + 0x128 + 4 * k as u32) << 2;
    }
    if m.u32(0x0300_6148) != 0 {
        return Err(Unported("FUN_08140224 (the raised camera's orientation)"));
    }
    if m.u32(0x0300_618C) == 0 {
        let i = m.u16(e) as u32;
        if i == m.u32(PLAYER) && 0 < m.i32(m.u32(0x0300_56EC) + 0x318 + 4 * i) {
            for p in spark_points(m, e) {
                spawn_spark(m, e, p, &r);
            }
        }
    } else {
        let w = e + 0xA4 * m.u32(0x0300_57EC);
        for p in spark_points(m, w) {
            spawn_spark(m, w, p, &r);
        }
    }
    let cam = camera(m);
    let mut out = load(m, slot);
    mul(&r, &cam, &mut out, true);
    let (side, depth) = eye(e, m, &cam);
    out[9] = side;
    out[10] = (m.i32(e + 0x10) >> 8).wrapping_add(cam[10]);
    out[11] = depth;
    store(m, slot, &out);
    if m.u16(e + 0x64) != 0 {
        let spoiler = slot + 0x30;
        store(m, spoiler, &out);
        let car = m.u8(e + 0x89) as u32;
        let record = m.u32(0x0300_539C) + car * 0x11;
        let at = 0x087F_0816 + 4 * m.u8(record) as u32 + car * 0x40;
        let p = [0, m.i16(at) as i32, (m.i16(at + 2) as i32).wrapping_neg()];
        let t = transform_point(&out, p);
        for (k, v) in t.iter().enumerate() {
            m.set_i32(spoiler + 0x24 + 4 * k as u32, *v);
        }
    }
    Ok(())
}

/// `build_player_matrices` (`0x0814bc30`): the player's body and spoiler slots, its lights and flames, and the
/// horizon shift in the views before the chase view.
fn build_player_matrices(m: &mut Mem, e: u32) -> Result<()> {
    let heading = (m.u32(e + 0x2C) & 0x3F_FFFF) >> 8;
    let s = next_slot(m);
    m.set_u8(e + 0x88, s);
    if s != 0xFF {
        next_slot(m);
        let mut points = [[-0x20, 0, 0x55], [0x20, 0, 0x55], [-0x20, 0, -0x2A], [0x20, 0, -0x2A]];
        wheel_points(m, e, &mut points);
        let at = slot_addr(m, s);
        player_matrix(m, e, at)?;
        if view_mode(m) != 0 {
            let view = 0x4000 - matrix_yaw(m);
            opponent_effects(m, e, heading as i32, view, 0x20);
            exhaust(m, e, heading as i32, view);
        }
    }
    if view_mode(m) < 2 && m.u16(e) as u32 == m.u32(PLAYER) && m.u8(e + 0x88) != 0xFF {
        let at = slot_addr(m, m.u8(e + 0x88));
        m.set_i32(0x0300_56B8, m.i32(at + 0x1C).wrapping_mul(-0x80) >> 14);
    }
    Ok(())
}

/// `FUN_08144c98`: the live traffic cars' slots (`0x03006270`, 8 of them) and, seen from behind, their rear lights.
fn traffic_slots(m: &mut Mem) {
    if m.u32(0x0300_6090) == 0 {
        return;
    }
    for k in 0..8 {
        let t = m.u32(0x0300_6270 + 4 * k);
        if t == 0 || !matches!(m.i16(t + 0x4A), 1 | 3) {
            continue;
        }
        assign_entity_slot(m, t);
        let slot = m.u8(t + 0x88);
        if slot != 0xFF && 0x13FF < angle_diff(m.i16(t + 0x32) as i32, 0x4000 - matrix_yaw(m)).abs() {
            let at = slot_addr(m, slot);
            light(m, at, [-0x19, -0x20, 100], 0x40, 0x1BC, 2, true);
            light(m, at, [0x19, -0x20, 100], 0x40, 0x1BC, 2, true);
        }
    }
}

/// The slot part of `race_frame_update` after the camera: the player's slots, each other racer's slot and
/// `opponent_effects`, then the traffic.
pub fn race_slots(m: &mut Mem) -> Result<()> {
    if m.u32(0x0300_5624) == 2 {
        return Err(Unported("link play: build_player_matrices for every racer"));
    }
    let ents = m.u32(W_ENTITIES);
    let player = m.u32(PLAYER);
    build_player_matrices(m, ents + 0xA4 * player)?;
    let racers = m.i32(0x0300_57EC);
    for i in 0..(racers + 1).max(0) as u32 {
        if i == player {
            continue;
        }
        let e = ents + 0xA4 * i;
        assign_entity_slot(m, e);
        let size = if m.i32(e + 0x24) < 0x80 { 0x40 } else { 0x20 };
        let heading = (m.i32(e + 0x2C) >> 8) & 0x3FFF;
        opponent_effects(m, e, heading, 0x3FFF - matrix_yaw(m), size);
    }
    traffic_slots(m);
    Ok(())
}

/// The player's rim redraw at the start of `car_racing_step` (`0x0814b168`): seen from the side
/// (`rim_side_visible`), `draw_decal_on_atlas` (`0x0813bd90`) draws the rim into the car's atlas, rotated by the
/// wheel angle (driver `+0x90 >> 8`). The rotated read can reach up to 63 bytes around the rim's buffer on the
/// game's heap; here it reads the real heap, so it is exact (FIDELITY R24), unless that window overlaps the atlas
/// it writes, which this refuses.
pub fn rim_redraw(m: &mut Mem, e: u32) -> Result<()> {
    let phase = m.u32(PHASE);
    if phase == 0 || phase == 1 || phase == 4 || m.u16(e) as u32 != m.u32(PLAYER) || view_mode(m) == 0 {
        return Ok(());
    }
    let d = angle_diff(((m.u32(e + 0x2C) & 0x3F_FFFF) >> 8) as i32, 0x4000 - matrix_yaw(m)).abs();
    if d < 0x400 || (0x1C00 < d && d < 0x2400) {
        return Ok(());
    }
    let i = m.u16(e) as u32;
    let atlas_at = m.u32(0x0300_6164 + 4 * i);
    if atlas_at == 0 {
        return Ok(());
    }
    let car = m.u8(e + 0x89) as usize;
    let record = m.u32(0x0300_539C) + 0x11 * car as u32;
    let entry = 0x087E_F816 + 0x10 * (car as u32 * 15 + m.u8(record + 2) as u32);
    if m.i16(entry) == -1 {
        return Ok(());
    }
    let material = m.i16(entry + 4) as i32;
    let mat = m.u32(WORLD + 0x24).wrapping_add((material * 0x24) as u32);
    let rim = atlas::Rim {
        material: material as usize,
        width: m.u16(mat + 0xC) as usize,
        height: m.u16(mat + 0xE) as usize,
        first: (m.i16(entry) as i32, m.i16(entry + 2) as i32),
        second: (m.i16(entry + 8) as i32, m.i16(entry + 0xA) as i32),
    };
    let driver = m.u32(e + 0x8C);
    let angle = if driver == 0 { 0 } else { m.i32(driver + 0x90) >> 8 };
    let pixels = m.u32(0x0300_6094 + 4 * i);
    let read = pixels.wrapping_sub(0x40)..pixels + (rim.width * rim.height) as u32 + 0x40;
    // The box `draw_rim` writes: rows and columns inset by 1/8 at `first`, and again shifted to `second`.
    let (w, h) = (rim.width as i32, rim.height as i32);
    let (x0, y0) = (rim.first.0 + (w >> 3), rim.first.1 + (h >> 3));
    let (x1, y1) = (rim.first.0 + w - (w >> 3), rim.first.1 + h - (h >> 3));
    let second = (rim.second.1 - rim.first.1) * 0x100 + rim.second.0 - rim.first.0;
    let (lo, hi) = (y0 * 0x100 + x0, (y1 - 1) * 0x100 + x1);
    let written =
        atlas_at.wrapping_add((lo + second.min(0)) as u32)..atlas_at.wrapping_add((hi + second.max(0)) as u32);
    if x0 < x1 && y0 < y1 && read.start < written.end && written.start < read.end {
        return Err(Unported("the rim redraw reads the atlas it writes (heap layout)"));
    }
    let (ew, at, dst) = (
        m.ewram.clone(),
        (pixels & 0x3_FFFF) as usize,
        (atlas_at & 0x3_FFFF) as usize,
    );
    atlas::draw_rim(&m.rom, &mut m.ewram[dst..], &rim, &ew, at, angle);
    Ok(())
}
