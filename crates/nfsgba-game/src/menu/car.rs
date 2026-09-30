//! The garage's 3D car on typed state: `garage_load_car_atlas` (`0x0812BEEC`) and `garage_draw_car` (`0x0812BFA4`,
//! the turntable). The race renderer draws the model (`render::draw_car_model`), the atlas and the glass colours are
//! `atlas::player_atlas`/`draw_rim` and `paint::glass_shades`; this is the garage's camera, rotation and order.

use std::sync::OnceLock;

use nfsgba_fixed::{angle_diff, cos_q14, sin_q14};
use nfsgba_formats::render::{View, draw_car_model};
use nfsgba_formats::{Texture, atlas, paint, vehicle_textures};
use nfsgba_sim::state::MenuState;

use super::flow::rom_u16;
use super::typed::TypedHost;

const CAR_TABLE: u32 = 0x087F_0BD8; // 0x58 bytes per car: +0x0C material, +0x14 model
const HAS_MODEL: u32 = 0x087F_0626; // a byte per car
const SPOILERS: u32 = 0x087F_0636; // i16 per (car, level of part 0): the spoiler model (0x20 bytes per car)
const SPOILER_AT: u32 = 0x087F_0816; // i16 (y, -z) per (car, level): 0x40 bytes per car
const MATERIALS: u32 = 0x0845_F5C0; // vehicle materials, 0x24 bytes
const FOCAL: i32 = 0x96;

/// The atlas the garage keeps in EWRAM (`*0x03006164`) and the spoiler model of the entity (`+0x64`).
#[derive(Clone)]
pub struct GarageCar {
    pub atlas: Vec<u8>,
    pub spoiler: i16,
}

fn s16(rom: &[u8], a: u32) -> i32 {
    rom_u16(rom, a) as i16 as i32
}

/// Decoded once per process (one ROM per process; the game decodes the car's own
/// materials on every load.
fn textures(rom: &[u8]) -> &'static [Texture] {
    static T: OnceLock<Vec<Texture>> = OnceLock::new();
    T.get_or_init(|| vehicle_textures(rom))
}

/// `garage_load_car_atlas`: the car ids of the race slots, the spoiler model, then `unpack_player_atlas(world, 0)` and
/// `unpack_decal` (the rim at angle 0).
pub fn load_atlas(st: &mut MenuState, rom: &[u8]) -> GarageCar {
    let car = st.g.player_car as u8;
    st.g.race_car = car;
    st.g.race_car_b = car.wrapping_add(15);
    let spoiler = s16(rom, SPOILERS + 0x20 * car as u32 + 2 * st.g.garage_car[0] as u32) as i16;
    let t = textures(rom);
    let (_, mut atlas) = atlas::player_atlas(rom, t, car as usize, 0, &st.g.garage_car);
    if let Some(rim) = atlas::rim(rom, t, car as usize, &st.g.garage_car) {
        // The pixels of the rim's buffer with the 64 bytes of heap on either side that the game may read.
        let mut around = vec![0; 0x40];
        around.extend(atlas::rim_pixels(t, &rim));
        around.extend([0; 0x40]);
        atlas::draw_rim(rom, &mut atlas, &rim, &around, 0x40, 0);
    }
    GarageCar { atlas, spoiler }
}

/// A 3x4 matrix product `a * b` in 2.14 fixed point with the translations added (`FUN_081602f8`).
fn product(a: &[i32; 12], b: &[i32; 12]) -> [i32; 12] {
    let mut o = [0; 12];
    for r in 0..3 {
        for c in 0..3 {
            o[3 * r + c] = a[3 * r]
                .wrapping_mul(b[c])
                .wrapping_add(a[3 * r + 1].wrapping_mul(b[3 + c]))
                .wrapping_add(a[3 * r + 2].wrapping_mul(b[6 + c]))
                >> 14;
        }
    }
    for k in 9..12 {
        o[k] = a[k].wrapping_add(b[k]);
    }
    o
}

/// `transform_point` (`0x081607E0`).
fn transform(m: &[i32; 12], v: [i32; 3]) -> [i32; 3] {
    std::array::from_fn(|c| {
        (v[0]
            .wrapping_mul(m[c])
            .wrapping_add(v[1].wrapping_mul(m[3 + c]))
            .wrapping_add(v[2].wrapping_mul(m[6 + c]))
            >> 14)
            .wrapping_add(m[9 + c])
    })
}

impl TypedHost<'_> {
    /// `garage_load_car_atlas`.
    pub fn car_load(&mut self, st: &mut MenuState) {
        self.car = Some(load_atlas(st, self.rom));
    }

    /// `garage_draw_car(x, y, z)`: turns the car, sets the glass colours of its paint and draws it (and its spoiler
    /// behind or in front of it) into the page being drawn. Nothing for a car without a model.
    pub fn car_draw(&mut self, st: &mut MenuState, x: u32, y: u32, z: u32) {
        let rom = self.rom;
        let car = st.g.player_car;
        if rom[(HAS_MODEL + car) as usize & 0x1FF_FFFF] == 0 {
            return;
        }
        st.g.race_car = car as u8;
        let angle = &mut st.g.garage_angle;
        if st.profile.u_2f6 == 0 {
            *angle = angle.wrapping_add(0x40);
        } else {
            *angle = angle.wrapping_add((angle_diff(*angle as i32, st.profile.u_2f8 as i32) >> 2) as u32);
            st.profile.u_2f6 = st.profile.u_2f6.wrapping_sub(1);
        }
        *angle &= 0x3FFF;
        let angle = *angle as i32;
        // The camera-space placement: the car turned about y, tipped by 0x400 about x, `z` deep.
        let q = |a: i32| (cos_q14(rom, a), sin_q14(rom, a));
        let (c, s) = q(angle);
        let (c1, s1) = q(0x400);
        let ry = [c, 0, -s, 0, 0x4000, 0, s, 0, c, 0, 0, 0];
        let rx = [0x4000, 0, 0, 0, c1, s1, 0, -s1, c1, 0, 0, 0];
        let mut body = product(&ry, &rx);
        body[9..].copy_from_slice(&[0, 0, z as u16 as i16 as i32]);
        let view = View {
            cx: x as i16,
            cy: y as i16,
            near: 0x40,
            focal: FOCAL,
        };
        if st.g.menu_exit == 0 {
            let glass = paint::glass_shades(rom, st.g.garage_car[5], angle);
            for (slot, colour) in [(192, glass[0]), (208, glass[1])] {
                self.scene
                    .palettes
                    .iter_mut()
                    .filter_map(|p| p.get_mut(slot))
                    .for_each(|c| *c = colour);
                if st.g.fade == 0 {
                    self.screen.palette[slot] = colour;
                }
            }
        }
        let car_data = self.car.get_or_insert_with(|| load_atlas(st, rom));
        let (atlas, spoiler) = (&car_data.atlas, car_data.spoiler as i32);
        let record = CAR_TABLE + 0x58 * car;
        let material = MATERIALS + 0x24 * s16(rom, record + 0xC) as u32;
        let tex = (
            &atlas[..],
            rom[(material as usize + 0x1E) & 0x1FF_FFFF] as u32,
            rom_u16(rom, material + 0xE) as u32,
        );
        let mut draws = vec![(s16(rom, record + 0x14) - 1, body)];
        if spoiler > 0 {
            let at = SPOILER_AT + 4 * st.g.garage_car[0] as u32 + 0x40 * car;
            let mut m = body;
            m[9..].copy_from_slice(&transform(&body, [0, s16(rom, at), -s16(rom, at + 2)]));
            draws.push((spoiler - 1, m));
        }
        if (angle as u32).wrapping_sub(0x1800) <= 0xFFF {
            draws.reverse();
        }
        for (model, m) in draws {
            if model >= 0 {
                draw_car_model(rom, view, model as usize, &m, tex, self.screen.page());
            }
        }
    }
}
