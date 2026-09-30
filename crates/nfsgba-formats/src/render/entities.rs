//! The entity draw of the race renderer (the cars and their spoilers), reimplemented from the IWRAM code:
//! per-sector entity lists sorted farthest first, the depth and screen culls, LOD and material choice, model
//! projection, back-face culling, polygon clipping and the affine textured polygon rasteriser.
//! Spec with addresses: `docs/engine/renderer.md`, "Entities".

use super::{Frame, Portal, Runtime, SCREEN_WIDTH, draw_sector, recip};
use crate::{LEVEL_TABLE, i16_at, ptr, u16_at, u32_at};

/// An entity (world `+0x3C`, 0xA4-byte records in EWRAM): the fields the renderer reads, and the two it writes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Entity {
    /// `+0x00`: its own index; the draw order links through it.
    pub index: u16,
    /// `+0x02`: next entity in its sector's list (`0xFFFF` ends it; the heads are world `+0x0C`).
    pub next: u16,
    /// `+0x04`: next entity in the sector's draw order (written by the sort).
    pub draw_next: u16,
    /// `+0x08`: bit 0 runs the handler (world `+0x78[+0x4E]`) unless bit 1 is set; bit 2 is sorted and drawn.
    pub state: u16,
    /// `+0x0A`: bit 0 clips to the whole screen instead of the portal; bit 1 always takes the near model; bit 2
    /// "drawn this frame" (written; the next frame's slot assignment `FUN_0814eba0` reads it); bit 3 texture in
    /// RAM (`+0x84`) instead of the ROM; bit 4 draws sector `+0x36` instead of a model; bit 6 draws the far model
    /// beyond depth 0x1000 instead of nothing.
    pub flags: u16,
    /// `+0x0C/+0x10/+0x14`: position in 8.8 fixed point (city units, `-y` up).
    pub pos: [i32; 3],
    /// `+0x28`: `(x² + depth²) >> 8` in camera space, the sort key (written).
    pub key: i32,
    /// `+0x36`: the model drawn at depth ≥ 0x200 (the one before it nearer), or the sector of a bit-4 entity.
    pub model: i16,
    /// `+0x44` (its high byte) and `+0x46` are added to the material `+0x48`.
    pub material_step: u16,
    pub material_offset: u16,
    /// `+0x48`: vehicle material; 0: not drawn.
    pub material: u16,
    /// `+0x4E`: handler index into world `+0x78` (the Thumb table at `0x7F38B8`).
    pub handler: u16,
    /// `+0x64`: a second model (the spoiler) on the next matrix slot, drawn after the first model when positive,
    /// before it (as model `-n`) when negative.
    pub extra_model: i16,
    /// `+0x84`: RAM address of the unpacked atlas (flag bit 3).
    pub atlas: u32,
    /// `+0x88`: matrix slot (world `+0xFC`); `0xFF`: none, not drawn.
    pub slot: u8,
}

impl Entity {
    /// The renderer's fields of a 0xA4-byte entity record.
    pub fn read(r: &[u8]) -> Entity {
        let i32_at = |o: usize| u32_at(r, o) as i32;
        Entity {
            index: u16_at(r, 0),
            next: u16_at(r, 2),
            draw_next: u16_at(r, 4),
            state: u16_at(r, 8),
            flags: u16_at(r, 0xA),
            pos: [i32_at(0xC), i32_at(0x10), i32_at(0x14)],
            key: i32_at(0x28),
            model: i16_at(r, 0x36),
            material_step: u16_at(r, 0x44),
            material_offset: u16_at(r, 0x46),
            material: u16_at(r, 0x48),
            handler: u16_at(r, 0x4E),
            extra_model: i16_at(r, 0x64),
            atlas: u32_at(r, 0x84),
            slot: r[0x88],
        }
    }
}

/// The entity state one frame's draw reads and writes.
pub struct Scene<'a> {
    /// World `+0x3C`: the entity array.
    pub entities: Vec<Entity>,
    /// World `+0x0C`: per sector, its first entity (`0xFFFF`: none).
    pub heads: Vec<u16>,
    /// World `+0xFC`: matrix slots, 0x30 bytes each: the entity's rotation times the camera's (3×3, 2.14 fixed
    /// point, row-major), then its camera-space position. Game code fills them before the draw (the player's in
    /// `draw_vehicle` `0x0814bc30`, the others in `FUN_0814eba0` → `FUN_0814da68`).
    pub matrices: Vec<[i32; 12]>,
    /// EWRAM (`0x02000000`, 256 KiB), where the game unpacks the atlases (entity `+0x84`).
    pub ram: &'a [u8],
    /// The entity handler `world+0x78[+0x4E]` (Thumb game code), which the sort calls for entities with state
    /// bit 0 and not bit 1. NOT 1:1 (R26) by default: the handlers are not reimplemented; the default does nothing.
    pub handler: Box<dyn FnMut(&mut Entity) + 'a>,
    /// Spans left to draw: the rasteriser stops after this many (to match a RAM dump taken mid-frame).
    pub spans: usize,
    /// `0x03006920`: entities already sorted this frame (one bit each, cleared by `begin_frame`).
    visited: [u8; 32],
}

impl<'a> Scene<'a> {
    pub fn new(entities: Vec<Entity>, heads: Vec<u16>, matrices: Vec<[i32; 12]>, ram: &'a [u8]) -> Scene<'a> {
        Scene {
            entities,
            heads,
            matrices,
            ram,
            handler: Box::new(|_| {}),
            spans: usize::MAX,
            visited: [0; 32],
        }
    }

    /// No entities: the world alone.
    pub fn empty() -> Scene<'a> {
        Scene::new(Vec::new(), Vec::new(), Vec::new(), &[])
    }

    /// `draw_visible_sectors` (`FUN_030048c8`) clears the visited bitmap (`FUN_03004c1c(0x03006920, 0x20, 0)`).
    pub fn begin_frame(&mut self) {
        self.visited = [0; 32];
    }

    /// A texel: vehicle materials are read from the ROM in place, atlases from EWRAM (mirrored every 256 KiB).
    fn texel(&self, rom: &[u8], at: u32) -> u8 {
        match at >> 24 {
            2 => self.ram[(at & 0x3_FFFF) as usize],
            8 | 9 => rom[(at - 0x0800_0000) as usize],
            _ => 0, // NOT 1:1 (N1): no texture points elsewhere in Carbon's data
        }
    }
}

/// `collect_sector_entities` (`FUN_03004e24`) then `draw_sector_entities` (`FUN_03001cf0`): sorts the entities
/// of the entry's sector (or of each child of a container sector) and draws them farthest first. Returns the
/// clip columns the draw leaves in world `+0xE2`/`+0xE4` (the deferred walls drawn after it use them).
pub fn draw_entities(
    rom: &[u8],
    frame: &Frame,
    rt: &Runtime,
    scene: &mut Scene,
    entry: &Portal,
    screen: &mut [u8],
) -> (i16, i16) {
    let head = collect(rom, frame, scene, entry.sector);
    draw_sorted(rom, frame, rt, scene, entry, head, screen)
}

/// `FUN_03004e24`: the draw order of a sector's entities (a container's children in chain order, each child's
/// entities inserted into the same list).
fn collect(rom: &[u8], frame: &Frame, scene: &mut Scene, sector: u16) -> u16 {
    let sectors = ptr(rom, LEVEL_TABLE + 0x18);
    let (mut head, mut max_key) = (0xFFFF, -1);
    let s = sectors + 0x30 * sector as usize;
    if rom[s + 0x12] & 8 != 0 {
        let mut child = u16_at(rom, s + 0x24);
        while child != 0xFFFF {
            let list = scene.heads.get(child as usize).copied().unwrap_or(0xFFFF);
            if list != 0xFFFF {
                sort(frame, scene, list, &mut head, &mut max_key);
            }
            child = u16_at(rom, sectors + 0x30 * child as usize + 0x24);
        }
    } else {
        let list = scene.heads.get(sector as usize).copied().unwrap_or(0xFFFF);
        if list != 0xFFFF {
            sort(frame, scene, list, &mut head, &mut max_key);
        }
    }
    head
}

/// `sort_sector_entities` (`FUN_03001ae4`): walks a sector's list (`+0x02`), skipping entities already sorted
/// this frame and those with material 0, runs handlers, and inserts every state-bit-2 entity in front of the
/// camera into the draw list (`+0x04`) by descending key; equal keys go before the older ones.
fn sort(frame: &Frame, scene: &mut Scene, mut e: u16, head: &mut u16, max_key: &mut i32) {
    let m = &frame.camera;
    while e != 0xFFFF {
        let (byte, bit) = ((e as i32 >> 3) as usize, e & 7);
        let i = e as usize;
        if scene.visited[byte] >> bit & 1 == 0 {
            scene.visited[byte] |= 1 << bit;
            if scene.entities[i].material != 0 {
                let state = scene.entities[i].state;
                if state & 2 == 0 && state & 1 != 0 {
                    (scene.handler)(&mut scene.entities[i]);
                }
                let ent = scene.entities[i];
                if ent.state & 4 != 0 {
                    let x = m[9].wrapping_add(ent.pos[0] >> 8);
                    let z = m[11].wrapping_add(ent.pos[2] >> 8);
                    let d = m[2].wrapping_mul(x).wrapping_add(m[8].wrapping_mul(z)) >> 14;
                    let cx = m[0].wrapping_mul(x).wrapping_add(m[6].wrapping_mul(z)) >> 14;
                    let key = cx.wrapping_mul(cx).wrapping_add(d.wrapping_mul(d)) >> 8;
                    scene.entities[i].key = key;
                    if d > 0 {
                        insert(scene, i, key, head, max_key);
                    }
                }
            }
        }
        e = scene.entities[i].next;
    }
}

/// The insertion of `sort_sector_entities`: a new farthest entity becomes the head, others go after the last
/// entity with a greater key.
fn insert(scene: &mut Scene, i: usize, key: i32, head: &mut u16, max_key: &mut i32) {
    let ents = &mut scene.entities;
    let own = ents[i].index;
    if key >= *max_key {
        *max_key = key;
        ents[i].draw_next = *head;
        *head = own;
        return;
    }
    let mut prev = *head as usize;
    let mut next = ents[prev].draw_next;
    while next != 0xFFFF && ents[next as usize].key > key {
        prev = next as usize;
        next = ents[prev].draw_next;
    }
    ents[i].draw_next = next;
    ents[prev].draw_next = own;
}

/// `draw_sector_entities` (`FUN_03001cf0`).
fn draw_sorted(
    rom: &[u8],
    frame: &Frame,
    rt: &Runtime,
    scene: &mut Scene,
    entry: &Portal,
    mut e: u16,
    screen: &mut [u8],
) -> (i16, i16) {
    let (view, m) = (&frame.view, &frame.camera);
    let materials = ptr(rom, LEVEL_TABLE + 0x20);
    let texels = u32_at(rom, LEVEL_TABLE + 0x0C);
    let (e6, e8) = (entry.top as u16 as i32, entry.bottom as u16 as i32);
    let (mut e2, mut e4) = (entry.left, entry.right);
    while e != 0xFFFF {
        let i = e as usize;
        let ent = scene.entities[i];
        let flags = ent.flags;
        (e2, e4) = if flags & 1 != 0 {
            (frame.rect[0], frame.rect[1])
        } else {
            (entry.left, entry.right)
        };
        let rect = [e2 as i32, entry.top as i32, e4 as i32, entry.bottom as i32];
        let x = m[9].wrapping_add(ent.pos[0] >> 8);
        let z = m[11].wrapping_add(ent.pos[2] >> 8);
        let mut d = m[2].wrapping_mul(x).wrapping_add(m[8].wrapping_mul(z)) >> 14;
        let y = m[10].wrapping_add(ent.pos[1] >> 8);
        if flags & 0x10 != 0 {
            if d as u32 <= 9999 {
                let mut sector = Portal {
                    sector: ent.model as u16,
                    left: e2,
                    right: e4,
                    top: entry.top,
                    bottom: entry.bottom,
                    flags: 0,
                    depth: 0,
                };
                draw_sector(rom, frame, rt, scene, &mut sector, false, screen);
            }
        } else if (d as u32) < 0x2000 {
            let r = recip(rom, d).wrapping_mul(view.focal) >> 8;
            let sy = view.cy as i32 + (r.wrapping_mul(y) >> 16);
            let margin = r << 9 >> 16;
            if e6 - margin < sy && sy < e8 + margin {
                draw_entity(rom, frame, scene, i, &mut d, rect, materials, texels, screen);
            }
        }
        e = scene.entities[i].draw_next;
    }
    (e2, e4)
}

/// The model part of `draw_sector_entities` for one entity that passed the depth and screen culls.
#[allow(clippy::too_many_arguments)]
fn draw_entity(
    rom: &[u8],
    frame: &Frame,
    scene: &mut Scene,
    i: usize,
    d: &mut i32,
    rect: [i32; 4],
    materials: usize,
    texels: u32,
    screen: &mut [u8],
) {
    let ent = scene.entities[i];
    let flags = ent.flags;
    let mut material = materials + 0x24 * ent.material as usize;
    if flags & 2 != 0 {
        *d = 0;
    }
    scene.entities[i].flags |= 4;
    if ent.model != 0 && *d > 0x1000 {
        if flags & 0x40 != 0 {
            *d = 0x200;
        } else {
            material += 0x24;
        }
    }
    material += 0x24 * (ent.material_offset as usize + (ent.material_step >> 8) as usize);
    // Beyond 0x1000 the game also offsets the RAM texture by the previous material's size, but nothing more is
    // drawn then (the test below), so that is left out.
    if *d > 0x1000 || ent.model == 0 || ent.slot == 0xFF {
        return;
    }
    let texture = if flags & 8 != 0 {
        ent.atlas
    } else {
        texels.wrapping_add(u32_at(rom, material + 8))
    };
    let far = ent.model.wrapping_add((*d >= 0x200) as i16);
    let body = (far.wrapping_sub(1) as u16, ent.slot as u16);
    let extra = (ent.extra_model.unsigned_abs(), (ent.slot as u16).wrapping_add(1));
    let draws: &[(u16, u16)] = match ent.extra_model {
        0 => &[body],
        n if n < 0 => &[extra, body],
        _ => &[body, extra],
    };
    let tex = Texture {
        at: texture,
        log2w: rom[material + 0x1E] as u32,
        height: u16_at(rom, material + 0xE) as u32,
    };
    for &(model, slot) in draws {
        let matrix = scene.matrices[slot as usize];
        draw_model(rom, frame, scene, model as usize, &matrix, &tex, rect, screen);
    }
}

/// A model's texture: base address, log2 of the row length, and the height its UVs scale to.
struct Texture {
    at: u32,
    log2w: u32,
    height: u32,
}

/// The model bank's arrays (level descriptor `+0x34…+0x54` → world `+0x7C…+0x9C`).
fn bank(rom: &[u8], field: usize) -> usize {
    ptr(rom, LEVEL_TABLE + field)
}

/// `transform_model_vertices` (`FUN_03004018`): rotates and projects every vertex of the model into the
/// projected vertex buffer (world `+0xA0`). `None` (nothing drawn) as soon as one vertex's depth, unsigned, is
/// beyond 0x6000.
pub fn project_model(rom: &[u8], frame: &Frame, m: &[i32; 12], model: usize) -> Option<Vec<[i16; 2]>> {
    let rec = bank(rom, 0x34) + 40 * model;
    let first = u32_at(rom, rec) as usize;
    let verts = bank(rom, 0x38) + 6 * first;
    let view = &frame.view;
    (0..u16_at(rom, rec + 0x24) as usize)
        .map(|k| {
            let v = verts + 6 * k;
            let (x, y, z) = (
                i16_at(rom, v) as i32,
                i16_at(rom, v + 2) as i32,
                i16_at(rom, v + 4) as i32,
            );
            let dot = |a: i32, b: i32, c: i32| {
                x.wrapping_mul(a)
                    .wrapping_add(y.wrapping_mul(b))
                    .wrapping_add(z.wrapping_mul(c))
            };
            let d = m[11].wrapping_add(dot(m[2], m[5], m[8]) >> 14);
            if d as u32 > 0x6000 {
                return None;
            }
            let cx = m[9].wrapping_add(dot(m[0], m[3], m[6]) >> 14);
            let cy = m[10].wrapping_add(dot(m[1], m[4], m[7]) >> 14);
            let r = recip(rom, d).wrapping_mul(view.focal) >> 8;
            Some([
                (view.cx as i32).wrapping_add(r.wrapping_mul(cx) >> 16) as i16,
                (view.cy as i32).wrapping_add(r.wrapping_mul(cy) >> 16) as i16,
            ])
        })
        .collect()
}

/// `draw_model_polygons` for a model outside the race world: the garage turntable's car (`garage_draw_car`
/// `0x0812BFA4` → `draw_model_param_block`), drawn into a 240×160 page clipped to the whole screen. `atlas` is the
/// car's texture as the game keeps it in EWRAM (rows `1 << log2w` bytes apart), `height` the material's height.
pub fn draw_car_model(
    rom: &[u8],
    view: super::View,
    model: usize,
    m: &[i32; 12],
    (atlas, log2w, height): (&[u8], u32, u32),
    screen: &mut [u8],
) {
    const AT: u32 = 0x0200_0000; // where the atlas sits in the scene's EWRAM window
    let mut ram = vec![0; 0x4_0000];
    ram[..atlas.len()].copy_from_slice(atlas);
    let frame = Frame {
        view,
        camera: [0; 12],
        rect: [0, 240, 0, 160],
    };
    let mut scene = Scene::new(Vec::new(), Vec::new(), Vec::new(), &ram);
    let tex = Texture { at: AT, log2w, height };
    draw_model(rom, &frame, &mut scene, model, m, &tex, [0, 0, 240, 160], screen);
}

/// A polygon corner: screen position and texture coordinates (1.15 of the texture's width and height), as the
/// game stores them (halfwords).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Corner {
    x: i16,
    y: i16,
    u: u16,
    v: u16,
}

/// `draw_model_polygons` (`FUN_03004190`): projects the model, then rasterises every polygon whose first three
/// projected corners wind clockwise on screen (cross product ≥ 0).
#[allow(clippy::too_many_arguments)]
fn draw_model(
    rom: &[u8],
    frame: &Frame,
    scene: &mut Scene,
    model: usize,
    m: &[i32; 12],
    tex: &Texture,
    rect: [i32; 4],
    screen: &mut [u8],
) {
    let Some(proj) = project_model(rom, frame, m, model) else {
        return;
    };
    let rec = bank(rom, 0x34) + 40 * model;
    let mut vidx = bank(rom, 0x3C) + 2 * u32_at(rom, rec + 4) as usize;
    let mut uvidx = bank(rom, 0x40) + 2 * u32_at(rom, rec + 0x18) as usize;
    let sizes = bank(rom, 0x54) + u32_at(rom, rec + 0x1C) as usize;
    let mut uvs = bank(rom, 0x48);
    if u16_at(rom, rec + 0x20) & 1 != 0 {
        uvs += 4 * u32_at(rom, rec + 8) as usize;
    }
    for p in 0..u16_at(rom, rec + 0x22) as usize {
        let at = |k: usize| proj[u16_at(rom, vidx + 2 * k) as usize].map(|c| c as i32);
        let ([x0, y0], [x1, y1], [x2, y2]) = (at(0), at(1), at(2));
        let cross = (y1 - y0)
            .wrapping_mul(x2 - x0)
            .wrapping_sub((y2 - y0).wrapping_mul(x1 - x0));
        let n = rom[sizes + p] as usize;
        if cross >= 0 {
            let corners = (0..n)
                .map(|k| {
                    let [x, y] = proj[u16_at(rom, vidx + 2 * k) as usize];
                    let uv = uvs + 4 * u16_at(rom, uvidx + 2 * k) as usize;
                    Corner {
                        x,
                        y,
                        u: u16_at(rom, uv),
                        v: u16_at(rom, uv + 2),
                    }
                })
                .collect();
            raster_polygon(rom, scene, corners, tex, rect, screen);
        }
        vidx += 2 * n;
        uvidx += 2 * n;
    }
}

/// `FUN_0300514c`: 4 when no corner lies inside `left..right` or none inside `top..bottom` (the polygon is
/// dropped, even one that spans the screen); otherwise bit 0 when some corner lies outside horizontally, bit 1
/// vertically.
fn outcode(poly: &[Corner], rect: [i32; 4]) -> u32 {
    let [left, top, right, bottom] = rect;
    let (mut code, mut xs, mut ys) = (0, 0, 0);
    for c in poly {
        let (x, y) = (c.x as i32, c.y as i32);
        if x >= right || x < left {
            code |= 1;
        } else {
            xs += 1;
        }
        if y >= bottom || y < top {
            code |= 2;
        } else {
            ys += 1;
        }
    }
    if xs != 0 && ys != 0 { code } else { 4 }
}

/// `FUN_03003acc`: clips the edge `a → b` (`[x, y, u, v]` each) to `lo..=hi` on axis `c` (0: x, 1: y), moving
/// the other coordinate and u/v by the 16.16 fraction `(recip[den] >> 8) * num`. 0: the edge is outside;
/// otherwise bit 1 start clipped, bit 2 end clipped (the caller then emits both ends).
fn clip_edge(rom: &[u8], lo: i32, hi: i32, e: &mut [[i32; 4]; 2], c: usize) -> u32 {
    let [a, b] = *e;
    let frac = |num: i32, den: i32| (recip(rom, den) >> 8).wrapping_mul(num);
    // Every field but the clipped one moves from `p` towards `q` by `t`.
    let lerp = |p: [i32; 4], q: [i32; 4], t: i32, edge: i32| {
        let mut r = p.map(|_| 0);
        for k in 0..4 {
            r[k] = p[k].wrapping_add(q[k].wrapping_sub(p[k]).wrapping_mul(t) >> 16);
        }
        r[c] = edge;
        r
    };
    let mut result = 1;
    let start = if a[c] < lo {
        if b[c] < lo {
            return 0;
        }
        Some((lo, lo - a[c], b[c] - a[c]))
    } else if a[c] > hi {
        if b[c] > hi {
            return 0;
        }
        Some((hi, a[c] - hi, a[c] - b[c]))
    } else {
        None
    };
    if let Some((edge, num, den)) = start {
        e[0] = lerp(a, b, frac(num, den), edge);
        result |= 2;
    }
    let a = e[0];
    let end = if b[c] < lo {
        Some((lo, lo - b[c], a[c] - b[c]))
    } else if b[c] > hi {
        Some((hi, b[c] - hi, b[c] - a[c]))
    } else {
        None
    };
    if let Some((edge, num, den)) = end {
        e[1] = lerp(b, a, frac(num, den), edge);
        result |= 4;
    }
    result
}

/// `FUN_03003d70`: Sutherland–Hodgman against `left..=right` (when `code` bit 0) then `top..=bottom` (bit 1),
/// storing each result as halfwords (the scratch buffers at `0x03006440`/`0x03006390`, then `0x03006460`/
/// `0x030063B0`, with the identity index table at `0x03006430`).
fn clip_polygon(rom: &[u8], poly: Vec<Corner>, rect: [i32; 4], code: u32) -> Vec<Corner> {
    let mut poly = poly;
    for (bit, c, lo, hi) in [(1, 0, rect[0], rect[2]), (2, 1, rect[1], rect[3])] {
        if code & bit == 0 {
            continue;
        }
        let n = poly.len();
        let mut out = Vec::with_capacity(n + 2);
        for k in 0..n {
            let at = |p: Corner| [p.x as i32, p.y as i32, p.u as i32, p.v as i32];
            let mut e = [at(poly[k]), at(poly[(k + 1) % n])];
            let r = clip_edge(rom, lo, hi, &mut e, c);
            let put = |p: [i32; 4]| Corner {
                x: p[0] as i16,
                y: p[1] as i16,
                u: p[2] as u16,
                v: p[3] as u16,
            };
            if r != 0 {
                out.push(put(e[0]));
                if r & 4 != 0 {
                    out.push(put(e[1]));
                }
            }
        }
        poly = out;
        if poly.is_empty() {
            break;
        }
    }
    poly
}

/// One polygon edge being walked down the screen: 16.16 x, u and v in 1/256 texels, their steps per row, and
/// the rows left.
#[derive(Debug, Clone, Copy, Default)]
struct Edge {
    x: i32,
    u: i32,
    v: i32,
    dx: i32,
    du: i32,
    dv: i32,
    rows: i32,
}

/// `FUN_03003464` (walking the corners backwards) and `FUN_03003634` (forwards): moves `index` to the next
/// corner below, skipping horizontal edges. `None` ends the polygon: a corner above, or the shared count of
/// edges left (`0x03006904`) exhausted.
fn next_edge(
    rom: &[u8],
    poly: &[Corner],
    index: &mut usize,
    left: &mut i32,
    back: bool,
    tex: &Texture,
) -> Option<Edge> {
    let n = poly.len();
    let y0 = poly[*index].y as i32;
    loop {
        let from = *index;
        *index = if back { (from + n - 1) % n } else { (from + 1) % n };
        *left -= 1;
        if *left < 0 {
            return None;
        }
        let (a, b) = (poly[from], poly[*index]);
        let dy = b.y as i32 - y0;
        if dy < 0 {
            return None;
        }
        if dy == 0 {
            continue;
        }
        let r = recip(rom, dy) >> 8;
        let v = |c: Corner| (c.v as u32).wrapping_mul(tex.height) >> 7;
        let u = |c: Corner| ((c.u as i32) << tex.log2w) >> 7;
        let (v0, u0) = (v(a) as i32, u(a));
        return Some(Edge {
            x: (a.x as i32) << 16,
            u: u0,
            v: v0,
            dx: r.wrapping_mul(b.x as i32 - a.x as i32),
            du: r.wrapping_mul(u(b).wrapping_sub(u0)) >> 16,
            dv: r.wrapping_mul((v(b) as i32).wrapping_sub(v0)) >> 16,
            rows: dy,
        });
    }
}

/// `raster_polygon` (`FUN_03003808`): outcode, clip, then scanlines from the top corner (the first with the
/// least y), between the edge walked backwards (`a`) and the one walked forwards (`b`). A row is drawn only when
/// `a` lies right of `b`, from `b.x` rounded down to `a.x` rounded up (exclusive), with the texture stepped by
/// `(a − b) / (width + 1)`.
fn raster_polygon(rom: &[u8], scene: &mut Scene, poly: Vec<Corner>, tex: &Texture, rect: [i32; 4], screen: &mut [u8]) {
    let code = outcode(&poly, rect);
    if code == 4 {
        return;
    }
    let poly = if code != 0 {
        clip_polygon(rom, poly, rect, code)
    } else {
        poly
    };
    if poly.is_empty() {
        return; // the game reads a stale corner, then its first edge finds no edges left
    }
    // `FUN_03005320`: the first corner with the least y.
    let top = (0..poly.len()).fold(0, |best, k| if poly[k].y < poly[best].y { k } else { best });
    let mut row = poly[top].y as i32;
    let mut left = poly.len() as i32;
    let (mut ia, mut ib) = (top, top);
    let (mut a, mut b) = (Edge::default(), Edge::default());
    loop {
        a.rows -= 1;
        if a.rows <= 0 {
            match next_edge(rom, &poly, &mut ia, &mut left, true, tex) {
                Some(e) => a = e,
                None => return,
            }
        }
        b.rows -= 1;
        if b.rows <= 0 {
            match next_edge(rom, &poly, &mut ib, &mut left, false, tex) {
                Some(e) => b = e,
                None => return,
            }
        }
        if a.x > b.x {
            let x = b.x >> 16;
            let count = (a.x.wrapping_add(0xFFFF) >> 16) - x;
            if count != 0 {
                if scene.spans == 0 {
                    return;
                }
                scene.spans -= 1;
                let r = recip(rom, count) >> 8;
                let du = r.wrapping_mul(a.u.wrapping_sub(b.u)) >> 16;
                let dv = r.wrapping_mul(a.v.wrapping_sub(b.v)) >> 16;
                let at = (row * SCREEN_WIDTH as i32 + x) as usize;
                span(rom, scene, tex, &mut screen[at..at + count as usize], b.u, b.v, du, dv);
            }
        }
        a.u = a.u.wrapping_add(a.du);
        a.x = a.x.wrapping_add(a.dx);
        a.v = a.v.wrapping_add(a.dv);
        b.x = b.x.wrapping_add(b.dx);
        b.u = b.u.wrapping_add(b.du);
        b.v = b.v.wrapping_add(b.dv);
        row += 1;
    }
}

/// `FUN_030051f4`: one textured span. The texel pointer starts at `texture + (v >> 8 << log2w) + (u >> 8)` and
/// steps by the integer parts of du and dv plus the carries of their 8-bit fractions: no wrap in u or v.
#[allow(clippy::too_many_arguments)]
fn span(rom: &[u8], scene: &Scene, tex: &Texture, out: &mut [u8], u: i32, v: i32, du: i32, dv: i32) {
    let mut at = tex
        .at
        .wrapping_add(((v >> 8) << tex.log2w) as u32)
        .wrapping_add((u >> 8) as u32);
    let (mut fu, mut fv) = ((u as u32) << 24, (v as u32) << 24);
    let (su, sv) = ((du as u32) << 24, (dv as u32) << 24);
    let step = ((du >> 8) + ((dv >> 8) << tex.log2w)) as u32;
    for px in out {
        *px = scene.texel(rom, at);
        let (nu, cu) = fu.overflowing_add(su);
        let (nv, cv) = fv.overflowing_add(sv);
        (fu, fv) = (nu, nv);
        at = at.wrapping_add(step).wrapping_add(cu as u32);
        if cv {
            at = at.wrapping_add(1 << tex.log2w);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{MaterialState, Piece, View, Visibility, draw_world, visible_sectors};
    use super::*;
    use nfsgba_testkit::{fixture, rom};

    /// A race RAM dump (`NAME.iwram.bin`, `NAME.wram.bin`, `NAME.vram.bin` from `tools/mgba_remote.lua` or
    /// `tools/mgba_frame_probe.lua`): everything the renderer reads, straight from the game's memory.
    struct Dump(crate::Dump);

    const WORLD: u32 = 0x0300_00C0;

    impl Dump {
        fn load(name: &str) -> Option<Dump> {
            crate::Dump::fixture(name, &["vram"]).map(Dump)
        }

        fn mem(&self, at: u32) -> &[u8] {
            self.0.at(at)
        }

        fn u32(&self, at: u32) -> u32 {
            u32_at(self.mem(at), 0)
        }

        fn u16(&self, at: u32) -> u16 {
            u16_at(self.mem(at), 0)
        }

        fn i16(&self, at: u32) -> i16 {
            i16_at(self.mem(at), 0)
        }

        fn frame(&self) -> Frame {
            let view = self.u32(WORLD + 0x50);
            let cam = self.u32(WORLD + 0x54);
            Frame {
                view: View {
                    cx: self.i16(view + 8),
                    cy: self.i16(view + 0xA),
                    near: self.u32(view + 0x10) as i32,
                    focal: self.u32(view + 0x1C) as i32,
                },
                camera: std::array::from_fn(|k| self.u32(cam + 4 * k as u32) as i32),
                rect: std::array::from_fn(|k| self.i16(WORLD + 0x58 + 2 * k as u32)),
            }
        }

        /// The runtime tables: material animation and scroll (world `+0x48`, count world `+0xD8`) and the moving
        /// wall pieces (world `+0x18`) the city names.
        fn runtime(&self, rom: &[u8]) -> Runtime {
            let materials = self.u32(WORLD + 0x48);
            let walls = ptr(rom, LEVEL_TABLE + 0x14);
            let pieces = (0..self.u16(WORLD + 0xDC) as usize)
                .map(|k| u16_at(rom, walls + 0x44 * k + 0x2A))
                .filter(|&p| p != 0xFFFF)
                .max()
                .map_or(0, |p| p as u32 + 1);
            let piece_at = self.u32(WORLD + 0x18);
            Runtime {
                pieces: (0..pieces)
                    .map(|k| {
                        let h = |o: u32| self.i16(piece_at + 0x20 * k + o);
                        Piece {
                            dx: h(0),
                            dz: h(2),
                            ceiling: h(4),
                            floor: h(6),
                            top: h(8),
                            bottom: h(0xA),
                            material: h(0xC) as u16,
                            flags: h(0xE) as u16,
                        }
                    })
                    .collect(),
                sector_offsets: Vec::new(),
                materials: (0..self.u16(WORLD + 0xD8) as u32)
                    .map(|k| MaterialState {
                        frame: self.u16(materials + 8 * k + 2),
                        u_scroll: self.i16(materials + 8 * k + 4),
                        v_scroll: self.i16(materials + 8 * k + 6),
                    })
                    .collect(),
            }
        }

        fn scene(&self) -> Scene<'_> {
            let ents = self.u32(WORLD + 0x3C);
            let count = self.u16(WORLD + 0xF8) as u32 + self.u16(WORLD + 0xFA) as u32;
            let heads = self.u32(WORLD + 0xC);
            let mats = self.u32(WORLD + 0xFC);
            Scene::new(
                (0..count).map(|i| Entity::read(self.mem(ents + 0xA4 * i))).collect(),
                (0..1113).map(|s| self.u16(heads + 2 * s)).collect(),
                (0..64)
                    .map(|s| std::array::from_fn(|k| self.u32(mats + 0x30 * s + 4 * k as u32) as i32))
                    .collect(),
                &self.0.ewram,
            )
        }

        /// List entry 0 as the camera builds it (`FUN_08137cb0`): the camera sector over the whole screen.
        fn root(&self) -> Portal {
            let list = self.u32(WORLD + 0x60);
            Portal {
                sector: self.u16(list),
                left: self.i16(list + 2),
                right: self.i16(list + 4),
                top: self.i16(list + 6),
                bottom: self.i16(list + 8),
                flags: 0,
                depth: 0,
            }
        }

        /// The mode-4 page being drawn (view `+0x00`).
        fn page(&self) -> &[u8] {
            &self.mem(self.u32(self.u32(WORLD + 0x50)))[..SCREEN_WIDTH * 160]
        }
    }

    /// Draws a whole frame on a background `fill`, with at most `spans` entity spans; also returns the spans
    /// drawn.
    fn draw(rom: &[u8], dump: &Dump, fill: u8, spans: usize) -> (Vec<u8>, Visibility, usize) {
        let (frame, rt) = (dump.frame(), dump.runtime(rom));
        let mut scene = dump.scene();
        scene.spans = spans;
        let mut screen = vec![fill; SCREEN_WIDTH * 160];
        let mut vis = visible_sectors(rom, &frame, dump.root());
        draw_world(rom, &frame, &rt, &mut scene, &mut vis, &mut screen);
        (screen, vis, spans - scene.spans)
    }

    /// The pixels the renderer wrote (equal on two backgrounds), and those of them that differ from the page.
    fn differences(a: &[u8], b: &[u8], page: &[u8]) -> (usize, Vec<usize>) {
        let written: Vec<usize> = (0..a.len()).filter(|&i| a[i] == b[i]).collect();
        let differ = written.iter().copied().filter(|&i| a[i] != page[i]).collect();
        (written.len(), differ)
    }

    /// The reference race dump was taken inside pass 1 (`mgba/log.txt`: pc `0x03003A7C`, just after the span
    /// call in `raster_polygon`; its row pointer `0x06011080` is row 120). Cutting the entity spans at the right
    /// count must reproduce every written pixel of the page, car included, and the last span must lie on row 120.
    #[test]
    fn race_frame_matches_up_to_the_car_span_being_drawn() {
        let Some(rom) = rom() else { return };
        let Some(dump) = Dump::load("mgba/race") else { return };
        let page = dump.page();
        let (_, _, total) = draw(&rom, &dump, 0, usize::MAX);
        let matches: Vec<usize> = (0..=total)
            .filter(|&n| {
                let (a, b) = (draw(&rom, &dump, 0, n).0, draw(&rom, &dump, 255, n).0);
                differences(&a, &b, page).1.is_empty()
            })
            .collect();
        eprintln!("{total} entity spans; the page matches after {matches:?}");
        assert_eq!(matches.len(), 1);
        let n = matches[0];
        let (before, after) = (draw(&rom, &dump, 0, n - 1).0, draw(&rom, &dump, 0, n).0);
        let rows: Vec<usize> = (0..after.len())
            .filter(|&i| before[i] != after[i])
            .map(|i| i / SCREEN_WIDTH)
            .collect();
        assert!(
            !rows.is_empty() && rows.iter().all(|&r| r == 120),
            "last span rows {rows:?}"
        );
        let (a, b) = (draw(&rom, &dump, 0, n).0, draw(&rom, &dump, 255, n).0);
        eprintln!("{} written pixels match", differences(&a, &b, page).0);
    }

    /// Whole frames from `tools/mgba_frame_probe.lua` (inputs at the start of `draw_visible_sectors`, the page at
    /// its end) in `work/e5298b24/entity-draw/`: every pixel the renderer writes must equal the game's.
    #[test]
    fn probe_frames_match() {
        let Some(rom) = rom() else { return };
        let Some(dir) = fixture("entity-draw") else { return };
        let files = std::fs::read_dir(&dir).unwrap();
        let mut names: Vec<String> = files
            .filter_map(|f| {
                f.ok()?
                    .file_name()
                    .to_str()?
                    .strip_suffix(".iwram.bin")
                    .map(String::from)
            })
            .collect();
        names.sort();
        for name in names {
            let dump = Dump::load(&format!("entity-draw/{name}")).unwrap();
            let ((a, _, spans), (b, _, _)) = (draw(&rom, &dump, 0, usize::MAX), draw(&rom, &dump, 255, usize::MAX));
            let (written, differ) = differences(&a, &b, dump.page());
            // Per entity that passed the screen cull: its depth and the pixels it shows (the frame without it).
            let frame = dump.frame();
            let mut scene = dump.scene();
            let rt = dump.runtime(&rom);
            let mut vis = visible_sectors(&rom, &frame, dump.root());
            draw_world(
                &rom,
                &frame,
                &rt,
                &mut scene,
                &mut vis,
                &mut vec![0; SCREEN_WIDTH * 160],
            );
            let m = frame.camera;
            let drawn: Vec<String> = (0..scene.entities.len())
                .filter(|&i| scene.entities[i].flags & 4 != 0)
                .map(|i| {
                    let e = scene.entities[i];
                    let (x, z) = (m[9].wrapping_add(e.pos[0] >> 8), m[11].wrapping_add(e.pos[2] >> 8));
                    let d = m[2].wrapping_mul(x).wrapping_add(m[8].wrapping_mul(z)) >> 14;
                    let mut without = dump.scene();
                    without.entities[i].material = 0;
                    let mut screen = vec![0; SCREEN_WIDTH * 160];
                    let mut vis = visible_sectors(&rom, &frame, dump.root());
                    draw_world(&rom, &frame, &rt, &mut without, &mut vis, &mut screen);
                    let shown = (0..a.len()).filter(|&k| a[k] != screen[k]).count();
                    format!("#{i} +0x36 {} slot {} depth {d:#x}: {shown} px", e.model, e.slot)
                })
                .collect();
            eprintln!(
                "{name}: {written} pixels written, {} differ; {spans} entity spans; {}",
                differ.len(),
                drawn.join(", ")
            );
            assert!(
                differ.is_empty(),
                "{name}: first differences at {:?}",
                &differ[..differ.len().min(8)]
            );
        }
    }
}
