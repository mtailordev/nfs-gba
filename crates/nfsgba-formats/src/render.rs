//! The race world renderer, reimplemented from the IWRAM code (copied from ROM `0x08165134` to `0x03000220`):
//! portal visibility, wall and flat setup, and the software rasterisers that write the 240×160 mode-4 frame.
//! Spec with pseudocode and addresses: `docs/engine/renderer.md`. Everything here is exact: 32-bit wrapping
//! arithmetic, the reciprocal table read straight from the ROM, and the game's quirks. The entities (cars) are
//! drawn by the `entities` module, in pass 1 or with their sector.

use super::{LEVEL_TABLE, i16_at, ptr, u16_at, u32_at};

mod entities;
pub use entities::{Entity, Scene, draw_entities, project_model};

pub use nfsgba_fixed::recip;

/// `FUN_03004ca4`: `a / (b + 1)` as `(a * recip[b]) >> 24` in 64 bits.
pub fn div_recip(rom: &[u8], a: i32, b: i32) -> i32 {
    ((a as i64 * recip(rom, b) as i64) >> 24) as i32
}

/// The view (IWRAM `0x03000080`, world `+0x50`).
#[derive(Debug, Clone, Copy)]
pub struct View {
    /// Screen centre (`+0x08`, `+0x0A`): 120 and 79 in a race.
    pub cx: i16,
    pub cy: i16,
    /// Near plane (`+0x10`): 64.
    pub near: i32,
    /// Focal length (`+0x1C`): 150, lowered for the speed effect (`FUN_08137cb0`).
    pub focal: i32,
}

impl View {
    /// Screen x of a camera-space point (x, depth): `cx + ((recip[d] * focal >> 7) * (x >> 1) >> 16)`.
    pub fn project_x(&self, rom: &[u8], x: i32, depth: i32) -> i32 {
        self.cx as i32 + ((recip(rom, depth).wrapping_mul(self.focal) >> 7).wrapping_mul(x >> 1) >> 16)
    }
}

/// Everything one frame's visibility pass reads.
#[derive(Debug, Clone, Copy)]
pub struct Frame {
    pub view: View,
    /// Camera matrix (world `+0x54`): a 3×3 rotation in 2.14 fixed point, then the translation
    /// (minus the camera position).
    pub camera: [i32; 12],
    /// Screen rectangle (world `+0x58..+0x5E`): left, right, top, bottom.
    pub rect: [i16; 4],
}

impl Frame {
    /// Rotates world point (x, z) into camera space: (x, depth), both `>> 14`, in 32-bit wrapping maths.
    pub fn to_camera(&self, x: i32, z: i32) -> (i32, i32) {
        let m = &self.camera;
        let (x, z) = (x.wrapping_add(m[9]), z.wrapping_add(m[11]));
        (
            x.wrapping_mul(m[0]).wrapping_add(z.wrapping_mul(m[6])) >> 14,
            x.wrapping_mul(m[2]).wrapping_add(z.wrapping_mul(m[8])) >> 14,
        )
    }
}

/// A visible-list entry (world `+0x60`, 16 bytes): a sector seen through a screen span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Portal {
    pub sector: u16,
    /// Screen columns `left..=right`, rows `top..=bottom`.
    pub left: i16,
    pub right: i16,
    pub top: i16,
    pub bottom: i16,
    /// `0x80`: no entities (a portal wall with flag `0x800` on the way); `8`: merged away (`FUN_03004ef0`,
    /// never called in Carbon).
    pub flags: u16,
    /// Portals crossed from the camera sector; entries at depth 5 are not expanded.
    pub depth: u16,
}

/// The visible-sector list of one frame: `FUN_03004828` expanding each entry with `FUN_03002614`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Visibility {
    pub portals: Vec<Portal>,
    /// World `+0xF6` set: an expanded sector has no ceiling (`+0x04 == 0`), so the sky shows.
    pub sky: bool,
}

/// `FUN_03004828`: starts from the camera sector's entry (built by the camera, `FUN_08137cb0`) and expands
/// entries in list order while fewer than 25 exist. The sector map at world `+0x64` is left out: its clear
/// (`FUN_03004c1c(map, 8, 0)`) writes nothing, and its only reader, `FUN_03004ef0`, is never called.
pub fn visible_sectors(rom: &[u8], frame: &Frame, root: Portal) -> Visibility {
    let mut vis = Visibility {
        portals: vec![root],
        sky: false,
    };
    let mut i = 0;
    while i < vis.portals.len() && vis.portals.len() < 25 {
        if vis.portals[i].depth < 5 {
            expand_portal(rom, frame, &mut vis, i);
        }
        i += 1;
    }
    vis
}

/// `FUN_03002614`: appends the sectors seen through the portal walls of entry `parent`.
fn expand_portal(rom: &[u8], frame: &Frame, vis: &mut Visibility, parent: usize) {
    let (view, [screen_left, screen_right, screen_top, screen_bottom]) = (frame.view, frame.rect);
    let p = vis.portals[parent];
    let sector = ptr(rom, LEVEL_TABLE + 0x18) + 0x30 * p.sector as usize;
    let walls = ptr(rom, LEVEL_TABLE + 0x14) + 0x44 * u16_at(rom, sector) as usize;
    let count = u16_at(rom, sector + 2) as usize;
    if u16_at(rom, sector + 4) == 0 {
        vis.sky = true;
    }
    // World +0xE2..+0xE4, read back unsigned.
    let (span_left, span_right) = (p.left as u16 as i32, p.right as u16 as i32);
    for k in 0..count {
        let wall = walls + 0x44 * k;
        let link = u16_at(rom, wall + 0x30);
        if link == 0xFFFF {
            continue;
        }
        let next = if k + 1 < count { wall + 0x44 } else { walls };
        let at = |w: usize| frame.to_camera(u32_at(rom, w) as i32, u32_at(rom, w + 4) as i32);
        let ((mut x0, mut d0), (mut x1, mut d1)) = (at(wall), at(next));
        if d0 > 0x6000 || d1 > 0x6000 {
            break; // the rest of the sector's portals are dropped too
        }
        if d0 < view.near && d1 < view.near {
            continue;
        }
        let (mut clip_left, mut clip_right) = (false, false);
        if d0 < view.near {
            clip_left = true;
            x0 += div_recip(rom, (view.near - d0).wrapping_mul(x1 - x0), d1 - d0);
            d0 = view.near;
        } else if d1 < view.near {
            clip_right = true;
            x1 -= div_recip(rom, (view.near - d1).wrapping_mul(x1 - x0), d0 - d1);
            d1 = view.near;
        }
        let (mut s0, mut s1) = (view.project_x(rom, x0, d0), view.project_x(rom, x1, d1));
        if parent == 0 && s0 > s1 {
            (s0, s1) = (s1, s0);
        }
        if s0 > s1 || s1 < span_left || s0 >= span_right {
            continue;
        }
        let no_entities = p.flags & 0x80 != 0 || u16_at(rom, wall + 0x2E) & 0x800 != 0;
        vis.portals.push(Portal {
            sector: link,
            left: if clip_left {
                screen_left
            } else if s0 < p.left as i32 {
                p.left
            } else {
                s0 as i16
            },
            right: if clip_right {
                screen_right
            } else if s1 > p.right as i32 {
                p.right
            } else {
                s1 as i16
            },
            top: screen_top,
            bottom: screen_bottom,
            flags: if no_entities { 0x80 } else { 0 },
            depth: p.depth + 1,
        });
    }
}

/// `FUN_03004ccc`: `(a * t) >> 24` with `t` unsigned, a 2.24 fraction (clipping).
fn mul_frac(a: i32, t: i32) -> i32 {
    ((a as i64 * t as u32 as i64) >> 24) as i32
}

/// `FUN_03004d20`: `a / (b + 1)` in 16.16 as `(a * recip[b]) >> 8`.
fn div_recip_16(rom: &[u8], a: i32, b: i32) -> i32 {
    ((a as i64 * recip(rom, b) as i64) >> 8) as i32
}

/// ARM `LSL` by a register: shifts of 32 or more give 0.
fn lsl(a: i32, n: u32) -> i32 {
    (a as u32).checked_shl(n).unwrap_or(0) as i32
}

/// A moving wall piece (world `+0x18`, 0x20-byte RAM records), named by wall `+0x2A`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Piece {
    /// `+0x00`, `+0x02`: offset of the wall's corner.
    pub dx: i16,
    pub dz: i16,
    /// `+0x04`, `+0x06`: offsets of the ceiling (wall `+0x3A`) and floor (wall `+0x38`) heights.
    pub ceiling: i16,
    pub floor: i16,
    /// `+0x08`, `+0x0A`: offsets of the wall's top and bottom.
    pub top: i16,
    pub bottom: i16,
    /// `+0x0C`: added to the wall's material; `+0x0E`: replaces the wall's flags when drawing.
    pub material: u16,
    pub flags: u16,
}

/// Renderer state that lives in RAM and changes at run time.
#[derive(Debug, Clone, Default)]
pub struct Runtime {
    /// World `+0x18`: moving wall pieces.
    pub pieces: Vec<Piece>,
    /// World `+0x1C`, named by sector `+0x0A` (no Carbon sector uses one).
    pub sector_offsets: Vec<SectorOffsets>,
    /// World `+0x48`, named by material `+0x00`; slots past the end read as all zero (no animation, no scroll).
    pub materials: Vec<MaterialState>,
}

/// A sector's runtime record (world `+0x1C`, 0x14 bytes).
#[derive(Debug, Clone, Copy, Default)]
pub struct SectorOffsets {
    /// `+0x04`, `+0x06`: added to the ceiling (wall `+0x3A`) and floor (wall `+0x38`) heights.
    pub ceiling: i16,
    pub floor: i16,
    /// `+0x08`: replaces sector `+0x12`'s flags; `0x40` hides the sector, 8 makes it a container.
    pub flags: u16,
}

/// A material's runtime entry (world `+0x48`, 8 bytes).
#[derive(Debug, Clone, Copy, Default)]
pub struct MaterialState {
    /// `+0x02`: animation frame, added to the material index.
    pub frame: u16,
    /// `+0x04`, `+0x06`: texture scroll.
    pub u_scroll: i16,
    pub v_scroll: i16,
}

impl Runtime {
    fn piece(&self, index: u16) -> Option<Piece> {
        (index != 0xFFFF).then(|| self.pieces[index as usize])
    }

    /// A wall's flags (`+0x2E`) as the renderer sees them: its moving piece's (`+0x2A`) when it names one.
    pub fn wall_flags(&self, piece: u16, flags: u16) -> u16 {
        self.piece(piece).map_or(flags, |p| p.flags)
    }

    fn material(&self, slot: u16) -> MaterialState {
        self.materials.get(slot as usize).copied().unwrap_or_default()
    }
}

/// A wall draw-buffer entry (world `+0x68`, 0x34 bytes), filled by `transform_walls` then `setup_wall_spans`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WallSpan {
    /// `+0x00`: camera x of the wall's start and end; screen x once projected.
    pub x: [i16; 2],
    /// `+0x04`: depth of the start and end, clipped to the near plane.
    pub depth: [i16; 2],
    /// `+0x08`: top at start and end; `+0x0C`: bottom (camera y, screen y once projected; `-y` is up).
    pub top: [i16; 2],
    pub bottom: [i16; 2],
    /// `+0x10`: texture u at start and end, in 1/128 texels.
    pub u: [i32; 2],
    /// `+0x18`: v of the top and bottom at the start (wall `+0x10`, `+0x18`); `+0x20`: at the end (`+0x14`,
    /// `+0x1C`). 16,384 = one texture.
    pub v_start: [i32; 2],
    pub v_end: [i32; 2],
    /// `+0x30`: wall `+0x42` · 128 + the material's v scroll / 2.
    pub v_offset: i16,
    /// `+0x32`: the wall's flags (`+0x2E`) plus 4 = not drawn, 8 = start clipped, 0x10 = end clipped, 0x20 =
    /// wholly behind the near plane.
    pub flags: u16,
}

/// A floor/ceiling outline vertex (world `+0x6C`, 0x18 bytes), one per wall end in front of the near plane.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FlatVertex {
    pub x: i32,
    pub floor_y: i32,
    pub ceiling_y: i32,
    /// `recip[depth]`, and the flat's u and v (1/128 texels) over depth + 1 in 16.16.
    pub recip: i32,
    pub u: i32,
    pub v: i32,
}

fn sector_at(rom: &[u8], sector: u16) -> usize {
    ptr(rom, LEVEL_TABLE + 0x18) + 0x30 * sector as usize
}

fn material_at(rom: &[u8], material: u16) -> usize {
    ptr(rom, LEVEL_TABLE + 0x1C) + 0x24 * material as usize
}

/// A sector's first wall record and wall count.
fn sector_walls(rom: &[u8], sector: u16) -> (usize, usize) {
    let s = sector_at(rom, sector);
    (
        ptr(rom, LEVEL_TABLE + 0x14) + 0x44 * u16_at(rom, s) as usize,
        u16_at(rom, s + 2) as usize,
    )
}

/// `transform_walls` (`FUN_03000978`): rotates the sector's wall corners into camera space. `None` when a
/// corner lies deeper than `0x5FFF`: the sector is then not drawn at all. Otherwise the spans and whether every
/// corner is at least `0x200` deep, which makes `draw_sector` set entry flag `0x80` (draw the sector's entities
/// with it, not in the last pass).
pub fn transform_walls(rom: &[u8], frame: &Frame, rt: &Runtime, sector: u16) -> Option<(Vec<WallSpan>, bool)> {
    let m = &frame.camera;
    let (walls, count) = sector_walls(rom, sector);
    let mut spans = vec![WallSpan::default(); count];
    let mut far = true;
    for (k, s) in spans.iter_mut().enumerate() {
        let w = walls + 0x44 * k;
        let (mut x, mut z) = (
            (u32_at(rom, w) as i32).wrapping_add(m[9]),
            (u32_at(rom, w + 4) as i32).wrapping_add(m[11]),
        );
        let (mut dy_top, mut dy_bottom) = (m[10] as i16, m[10] as i16);
        if let Some(p) = rt.piece(u16_at(rom, w + 0x2A)) {
            x = x.wrapping_add(p.dx as i32);
            z = z.wrapping_add(p.dz as i32);
            dy_top = dy_top.wrapping_add(p.top);
            dy_bottom = dy_bottom.wrapping_add(p.bottom);
        }
        s.x[0] = (x.wrapping_mul(m[0]).wrapping_add(z.wrapping_mul(m[6])) >> 14) as i16;
        s.top = [
            i16_at(rom, w + 8).wrapping_add(dy_top),
            i16_at(rom, w + 0xC).wrapping_add(dy_top),
        ];
        s.bottom = [
            i16_at(rom, w + 0xA).wrapping_add(dy_bottom),
            i16_at(rom, w + 0xE).wrapping_add(dy_bottom),
        ];
        let depth = x.wrapping_mul(m[2]).wrapping_add(z.wrapping_mul(m[8])) >> 14;
        if depth > 0x5FFF {
            return None;
        }
        far &= depth >= 0x200;
        s.depth[0] = depth as i16;
    }
    for k in 0..count {
        let next = spans[(k + 1) % count];
        spans[k].x[1] = next.x[0];
        spans[k].depth[1] = next.depth[0];
    }
    Some((spans, far))
}

/// `FUN_0300121c`: moves the start (`start`) or the end of a span to the near plane; `t` is the 2.24 fraction
/// of the span to cut.
fn clip_span(s: &mut WallSpan, start: bool, t: i32, wall_flags: u16) {
    let f = |a: i32| mul_frac(a, t);
    let dx = f(s.x[1] as i32 - s.x[0] as i32) as i16;
    let du = f(s.u[1].wrapping_sub(s.u[0]));
    let dtop = f(s.top[1] as i32 - s.top[0] as i32) as i16;
    let dbottom = f(s.bottom[1] as i32 - s.bottom[0] as i32) as i16;
    let v = wall_flags & 0x80 != 0;
    let (dv_top, dv_bottom) = (
        f(s.v_end[0].wrapping_sub(s.v_start[0])),
        f(s.v_end[1].wrapping_sub(s.v_start[1])),
    );
    if start {
        s.x[0] = s.x[0].wrapping_add(dx);
        s.u[0] = s.u[0].wrapping_add(du);
        s.top[0] = s.top[0].wrapping_add(dtop);
        s.bottom[0] = s.bottom[0].wrapping_add(dbottom);
        if v {
            s.v_start = [s.v_start[0].wrapping_add(dv_top), s.v_start[1].wrapping_add(dv_bottom)];
        }
        s.flags |= 8;
    } else {
        s.x[1] = s.x[1].wrapping_sub(dx);
        s.u[1] = s.u[1].wrapping_sub(du);
        s.top[1] = s.top[1].wrapping_sub(dtop);
        s.bottom[1] = s.bottom[1].wrapping_sub(dbottom);
        if v {
            s.v_end = [s.v_end[0].wrapping_sub(dv_top), s.v_end[1].wrapping_sub(dv_bottom)];
        }
        s.flags |= 0x10;
    }
}

/// `setup_wall_spans` (`FUN_030013ac`): texture setup, near clipping and projection of the spans from
/// `transform_walls`, for the sector drawn through `portal` (its left/right are world `+0xE2`/`+0xE4`).
/// Returns the floor/ceiling outline (its length is world `+0xF2`); empty when the sector has neither.
pub fn setup_wall_spans(
    rom: &[u8],
    frame: &Frame,
    rt: &Runtime,
    portal: &Portal,
    spans: &mut [WallSpan],
) -> Vec<FlatVertex> {
    let view = frame.view;
    let (cx, cy, near, focal) = (view.cx, view.cy, view.near, view.focal);
    let (span_left, span_right) = (portal.left as u16 as i32, portal.right as u16 as i32);
    let sector = sector_at(rom, portal.sector);
    let (walls, count) = sector_walls(rom, portal.sector);
    let (floor_mat, ceiling_mat) = (u16_at(rom, sector + 8), u16_at(rom, sector + 4));
    let has_flat = floor_mat != 0 || ceiling_mat != 0;
    let ty = frame.camera[10];
    let (mut floor_dy, mut ceiling_dy) = (ty, ty);
    let offsets = u16_at(rom, sector + 0xA);
    if offsets != 0xFFFF {
        let o = rt.sector_offsets[offsets as usize];
        (floor_dy, ceiling_dy) = (ty + o.floor as i32, ty + o.ceiling as i32);
    }
    let flat = material_at(rom, if floor_mat != 0 { floor_mat } else { ceiling_mat });
    let (flat_log2_w, flat_log2_h) = (rom[flat + 0x1E] as u32, rom[flat + 0x1F] as u32);
    let mut out = Vec::new();
    for (k, s) in spans.iter_mut().enumerate() {
        let w = walls + 0x44 * k;
        let n = if k + 1 < count || count == 1 { w + 0x44 } else { walls };
        let mat = material_at(rom, u16_at(rom, w + 0x2C));
        let MaterialState { u_scroll, v_scroll, .. } = rt.material(u16_at(rom, mat));
        let (mut floor0, mut floor1) = (
            i16_at(rom, w + 0x38) as i32 + floor_dy,
            i16_at(rom, n + 0x38) as i32 + floor_dy,
        );
        let (mut ceil0, mut ceil1) = (
            i16_at(rom, w + 0x3A) as i32 + ceiling_dy,
            i16_at(rom, n + 0x3A) as i32 + ceiling_dy,
        );
        for (piece, floor, ceil) in [(w, &mut floor0, &mut ceil0), (n, &mut floor1, &mut ceil1)] {
            if let Some(p) = rt.piece(u16_at(rom, piece + 0x2A)) {
                *floor += p.floor as i32;
                *ceil += p.ceiling as i32;
            }
        }
        let (mut fu0, mut fv0) = (u32_at(rom, w + 0x20) as i32, u32_at(rom, w + 0x24) as i32);
        let (mut fu1, mut fv1) = (u32_at(rom, n + 0x20) as i32, u32_at(rom, n + 0x24) as i32);
        let (d0, d1) = (s.depth[0] as i32, s.depth[1] as i32);
        let wall_flags = u16_at(rom, w + 0x2E);
        s.flags = wall_flags;
        let u0 = (u16_at(rom, w + 0x28) as i32 * 0x80).wrapping_add(u_scroll as i32 >> 1);
        let u1 = u0.wrapping_add(lsl(
            u16_at(rom, w + 0x40) as i32,
            (rom[mat + 0x1E] as u32).wrapping_sub(1) & 0xFF,
        ));
        s.u = if wall_flags & 2 == 0 { [u0, u1] } else { [u1, u0] };
        s.v_offset = (i16_at(rom, w + 0x42) as i32 * 0x80 + (v_scroll as i32 >> 1)) as i16;
        s.v_start = [u32_at(rom, w + 0x10) as i32, u32_at(rom, w + 0x18) as i32];
        s.v_end = [u32_at(rom, w + 0x14) as i32, u32_at(rom, w + 0x1C) as i32];
        let mut behind = false;
        if d0 < near {
            if d1 < near {
                behind = true;
            } else {
                let t = (near - d0).wrapping_mul(recip(rom, d1 - d0));
                clip_span(s, true, t, wall_flags);
                floor0 += mul_frac(floor1 - floor0, t);
                ceil0 += mul_frac(ceil1 - ceil0, t);
                fu0 += mul_frac(fu1.wrapping_sub(fu0), t);
                fv0 += mul_frac(fv1.wrapping_sub(fv0), t);
                s.depth[0] = near as i16;
            }
        } else if d1 < near {
            let t = (near - d1).wrapping_mul(recip(rom, d0 - d1));
            clip_span(s, false, t, wall_flags);
            floor1 -= mul_frac(floor1 - floor0, t);
            ceil1 -= mul_frac(ceil1 - ceil0, t);
            fu1 -= mul_frac(fu1.wrapping_sub(fu0), t);
            fv1 -= mul_frac(fv1.wrapping_sub(fv0), t);
            s.depth[1] = near as i16;
        }
        let mut flags = s.flags;
        if behind {
            s.flags = flags | 0x24;
            continue;
        }
        let (d0, d1) = (s.depth[0] as i32, s.depth[1] as i32);
        let (r0, r1) = (
            recip(rom, d0).wrapping_mul(focal) >> 7,
            recip(rom, d1).wrapping_mul(focal) >> 7,
        );
        let screen = |r: i32, y: i16| ((r.wrapping_mul(y as i32 >> 1)) >> 16) as i16;
        s.x = [cx.wrapping_add(screen(r0, s.x[0])), cx.wrapping_add(screen(r1, s.x[1]))];
        if has_flat {
            let vertex = |x: i16, r: i32, floor: i32, ceil: i32, d: i32, fu: i32, fv: i32| FlatVertex {
                x: x as i32,
                floor_y: cy as i32 + (r.wrapping_mul(floor >> 1) >> 16),
                ceiling_y: cy as i32 + ((ceil >> 1).wrapping_mul(r) >> 16),
                recip: recip(rom, d),
                u: div_recip_16(rom, lsl(fu, flat_log2_w) >> 7, d),
                v: div_recip_16(rom, lsl(fv, flat_log2_h) >> 7, d),
            };
            if flags & 8 != 0 {
                out.push(vertex(s.x[0], r0, floor0, ceil0, d0, fu0, fv0));
            }
            out.push(vertex(s.x[1], r1, floor1, ceil1, d1, fu1, fv1));
        }
        if flags & 0x2000 != 0 && s.x[1] < s.x[0] {
            flags |= 2;
        }
        if flags & 2 != 0 {
            s.x.swap(0, 1);
        }
        if s.x[0] < s.x[1] && span_left <= s.x[1] as i32 && (s.x[0] as i32) < span_right {
            s.top = [
                cy.wrapping_add(screen(r0, s.top[0])),
                cy.wrapping_add(screen(r1, s.top[1])),
            ];
            s.bottom = [
                cy.wrapping_add(screen(r0, s.bottom[0])),
                cy.wrapping_add(screen(r1, s.bottom[1])),
            ];
        } else {
            flags |= 4;
        }
        s.flags = flags;
    }
    out
}

/// The mode-4 frame the renderer draws into: 240×160 palette indices, row-major.
pub const SCREEN_WIDTH: usize = 240;

/// `FUN_03004cf8`: `(a << 8) / (b + 1)` as `(a * recip[b]) >> 16`.
fn div_recip_8(rom: &[u8], a: i32, b: i32) -> i32 {
    ((a as i64 * recip(rom, b) as i64) >> 16) as i32
}

/// `draw_sector_walls` (`FUN_03002084`): draws the sector's walls. The first call (`deferred == None`) draws
/// every visible wall without span flag 2 and returns the walls it deferred, bit `k` for the wall `k` from the
/// end, truncated to 8 bits as the caller keeps it; `draw_sector` passes that mask back after the flats.
pub fn draw_sector_walls(
    rom: &[u8],
    rt: &Runtime,
    portal: &Portal,
    spans: &[WallSpan],
    deferred: Option<u8>,
    screen: &mut [u8],
) -> u8 {
    let (walls, count) = sector_walls(rom, portal.sector);
    let (e6, e8) = (portal.top as u16 as i32, portal.bottom as u16 as i32);
    let mut mask = 0u32;
    for (i, s) in spans.iter().enumerate() {
        let (w, k) = (walls + 0x44 * i, (count - i) as u32);
        let piece = rt.piece(u16_at(rom, w + 0x2A));
        let material = u16_at(rom, w + 0x2C).wrapping_add(piece.map_or(0, |p| p.material));
        if u16_at(rom, material_at(rom, material)) == 0 {
            continue; // material 0
        }
        let flags = rt.wall_flags(u16_at(rom, w + 0x2A), u16_at(rom, w + 0x2E));
        let draw = flags & 1 == 0
            && match deferred {
                None if s.flags & 4 != 0 => false,
                None if s.flags & 2 != 0 => {
                    mask |= 1u32.checked_shl(k).unwrap_or(0);
                    false
                }
                None => true,
                Some(m) => (m as u32).checked_shr(k).unwrap_or(0) & 1 != 0,
            };
        let rows =
            (s.top[0] as i32) < e8 || (s.top[1] as i32) < e8 || e6 <= s.bottom[0] as i32 || e6 <= s.bottom[1] as i32;
        if draw && rows {
            let frame = material.wrapping_add(rt.material(material).frame);
            raster_wall_columns(rom, portal, s, frame, u16_at(rom, w + 0x2E), screen);
        }
    }
    mask as u8
}

/// `raster_wall_columns` (`FUN_03000304`) with its column writers `FUN_03004db0` / `FUN_03004d48`: draws one
/// wall span in 2-pixel columns inside the portal's rectangle. Each pixel of a column pair samples its own
/// texture column; the rows are shared. Textures whose first texel is 0 skip pairs with a 0 texel.
pub fn raster_wall_columns(
    rom: &[u8],
    portal: &Portal,
    s: &WallSpan,
    material: u16,
    wall_flags: u16,
    screen: &mut [u8],
) {
    let base = ptr(rom, LEVEL_TABLE + 0x08);
    let rec = material_at(rom, material);
    let (log2w, log2h) = (rom[rec + 0x1E] as u32, rom[rec + 0x1F] as u32);
    let colmap = base.wrapping_add(u32_at(rom, rec + 4) as usize);
    let texels = base.wrapping_add(u32_at(rom, rec + 8) as usize);
    let end = (s.x[1] as i32 + 1) >> 1;
    let mut start = s.x[0] as i32 >> 1;
    let mut cols = end - start;
    if cols <= 1 {
        return;
    }
    let transparent = rom[texels] == 0;
    let (wmask, hmask) = ((1u32 << log2w) - 1, (1u32 << log2h) - 1);
    let (ds, de, us, ue, ts, te, bs, be) = if s.flags & 2 == 0 {
        (
            s.depth[0],
            s.depth[1],
            s.u[0],
            s.u[1],
            s.top[0],
            s.top[1],
            s.bottom[0],
            s.bottom[1],
        )
    } else {
        (
            s.depth[1],
            s.depth[0],
            s.u[1],
            s.u[0],
            s.top[1],
            s.top[0],
            s.bottom[1],
            s.bottom[0],
        )
    };
    let (ds, de) = (ds as i32, de as i32);
    let step = |a: i32, b: i32| div_recip(rom, b.wrapping_sub(a), cols);
    let mut uz = div_recip_16(rom, us, ds);
    let uz_step = step(uz, div_recip_16(rom, ue, de));
    let (mut vz_top, mut vz_bottom) = (div_recip_16(rom, s.v_start[0], ds), div_recip_16(rom, s.v_start[1], ds));
    let vz_top_step = step(vz_top, div_recip_16(rom, s.v_end[0], de));
    let vz_bottom_step = step(vz_bottom, div_recip_16(rom, s.v_end[1], de));
    let mut iz = recip(rom, ds).wrapping_mul(0x100);
    let iz_step = step(iz, recip(rom, de).wrapping_mul(0x100));
    let (mut top, mut bottom) = (ts as i32 * 0x4000, bs as i32 * 0x4000);
    let (top_step, bottom_step) = (step(top, te as i32 * 0x4000), step(bottom, be as i32 * 0x4000));
    let left = (portal.left as u16 >> 1) as i32;
    if start < left {
        if end < left {
            return;
        }
        let n = left - start;
        uz = uz.wrapping_add(n.wrapping_mul(uz_step));
        iz = iz.wrapping_add(n.wrapping_mul(iz_step));
        top = top.wrapping_add(n.wrapping_mul(top_step));
        bottom = bottom.wrapping_add(n.wrapping_mul(bottom_step));
        vz_top = vz_top.wrapping_add(n.wrapping_mul(vz_top_step));
        vz_bottom = vz_bottom.wrapping_add(n.wrapping_mul(vz_bottom_step));
        (start, cols) = (left, end - left);
    }
    let right = (portal.right as u16 >> 1) as i32;
    if right < end {
        if right < start {
            return;
        }
        cols = right - start;
    }
    if cols <= 1 {
        return;
    }
    let (e6, e8) = (portal.top as u16 as i32, portal.bottom as u16 as i32);
    for col in start..start + cols {
        let mut h = (bottom.wrapping_add(0x3FFF) >> 14) - (top.wrapping_sub(0x3FFF) >> 14);
        if (iz >> 12) as u32 >= 0x6000 {
            return;
        }
        let z = recip(rom, iz >> 12);
        let u_left = z.wrapping_mul(uz >> 4) >> 23;
        let u_right =
            (uz.wrapping_add(uz_step >> 1) >> 4).wrapping_mul(recip(rom, iz.wrapping_add(iz_step >> 1) >> 12)) >> 23;
        let (mut v, v_step) = if wall_flags & 0x80 == 0 {
            (s.v_start[0] << 8, div_recip_8(rom, s.v_start[1], h))
        } else {
            let v = z.wrapping_mul(vz_top >> 4) >> 8;
            (v, div_recip(rom, (vz_bottom.wrapping_mul(z) >> 12) - v, h))
        };
        v = v.wrapping_add(s.v_offset as i32 * 0x100);
        let mut row = top;
        if top < e6 << 14 {
            let n = e6 - (top >> 14);
            h -= n;
            v = v.wrapping_add(v_step.wrapping_mul(n));
            row = e6 << 14;
        }
        if e8 << 14 <= bottom {
            h = h - 1 - ((bottom >> 14) - e8);
        }
        if h > 0 {
            let column = |u: i32| texels + ((rom[colmap + (u as u32 & wmask) as usize] as usize) << log2h);
            let (a, b) = (column(u_left), column(u_right));
            let mut at = SCREEN_WIDTH * (row as u32 >> 14) as usize + 2 * col as usize;
            for _ in 0..h {
                let t = ((v as u32 >> 15) & hmask) as usize;
                let (pa, pb) = (rom[a + t], rom[b + t]);
                if !transparent || (pa != 0 && pb != 0) {
                    screen[at] = pa;
                    screen[at + 1] = pb;
                }
                at += SCREEN_WIDTH;
                v = v.wrapping_add(v_step);
            }
        }
        uz = uz.wrapping_add(uz_step);
        vz_top = vz_top.wrapping_add(vz_top_step);
        vz_bottom = vz_bottom.wrapping_add(vz_bottom_step);
        top = top.wrapping_add(top_step);
        bottom = bottom.wrapping_add(bottom_step);
        iz = iz.wrapping_add(iz_step);
    }
}

/// One edge of a flat polygon being scanned (`FUN_03002a0c`'s state): x in 16.16 and, for textured flats, u/z,
/// v/z and 1/z (the vertex `recip`), each with its per-row step.
#[derive(Debug, Clone, Copy, Default)]
struct Edge {
    index: usize,
    rows: i32,
    x: i32,
    dx: i32,
    uz: i32,
    duz: i32,
    vz: i32,
    dvz: i32,
    iz: i32,
    diz: i32,
}

impl Edge {
    fn step(&mut self) {
        self.x = self.x.wrapping_add(self.dx);
        self.uz = self.uz.wrapping_add(self.duz);
        self.vz = self.vz.wrapping_add(self.dvz);
        self.iz = self.iz.wrapping_add(self.diz);
    }
}

/// `FUN_03002a0c`: from vertex `index`, walks the outline (`forward` or backwards) to the next edge that ends
/// below row `top` and spans at least one row, clipped to start at `top`. `remaining` counts the outline's
/// vertices for both edges together. Returns the edge and its first row, or `None` when the polygon is done.
fn next_edge(
    rom: &[u8],
    v: &[FlatVertex],
    mut index: usize,
    forward: bool,
    remaining: &mut i32,
    textured: bool,
    (top, bottom): (i32, i32),
) -> Option<(Edge, i32)> {
    let n = v.len();
    let mut cur = v[index];
    if cur.floor_y >= bottom {
        return None;
    }
    let (prev, mut rows) = loop {
        let prev = cur;
        index = if forward { (index + 1) % n } else { (index + n - 1) % n };
        *remaining -= 1;
        if *remaining < 0 {
            return None;
        }
        cur = v[index];
        let rows = if cur.floor_y < top {
            0
        } else {
            cur.floor_y - prev.floor_y
        };
        if rows >= 1 {
            break (prev, rows);
        }
    };
    let slope = |a: i32, b: i32| {
        if textured {
            div_recip(rom, b.wrapping_sub(a), rows)
        } else {
            0
        }
    };
    let mut e = Edge {
        index,
        dx: (recip(rom, rows) >> 8).wrapping_mul(cur.x - prev.x),
        x: prev.x.wrapping_mul(0x10000),
        duz: slope(prev.u, cur.u),
        uz: prev.u,
        dvz: slope(prev.v, cur.v),
        vz: prev.v,
        diz: slope(prev.recip, cur.recip),
        iz: prev.recip,
        rows: 0,
    };
    let mut y = prev.floor_y;
    if y < top {
        let d = top - y;
        e.x = e.x.wrapping_add(d.wrapping_mul(e.dx));
        e.uz = e.uz.wrapping_add(d.wrapping_mul(e.duz));
        e.vz = e.vz.wrapping_add(d.wrapping_mul(e.dvz));
        e.iz = e.iz.wrapping_add(d.wrapping_mul(e.diz));
        rows -= d;
        y = top;
    }
    e.rows = rows;
    Some((e, y))
}

/// A flat's texture: texels, log2 size and the material's scroll (world `+0x48` `+4`/`+6`, `<< 7`).
struct FlatTexture {
    texels: usize,
    log2w: u32,
    log2h: u32,
    scroll: (i32, i32),
}

/// `FUN_03004fa8`: `n` textured pixel pairs from `at`. The game stores one byte per pair; mode-4 VRAM writes a
/// byte into both pixels of its halfword.
#[allow(clippy::too_many_arguments)]
fn flat_span_affine(
    rom: &[u8],
    t: &FlatTexture,
    screen: &mut [u8],
    at: usize,
    n: i32,
    u: i32,
    v: i32,
    du: i32,
    dv: i32,
) {
    let umask = ((1u64 << (t.log2w + 15)) - 1) as u32;
    let vmask = ((1u32 << t.log2h) - 1) << t.log2w;
    let (mut u, mut v, mut at) = (u, v, at);
    for _ in 0..n {
        let tu = u as u32 & umask;
        u = u.wrapping_add(du);
        let tv = (v >> ((15 - t.log2w) & 0xFF)) as u32;
        v = v.wrapping_add(dv);
        let texel = rom[t.texels + (vmask & tv) as usize + (tu >> 15) as usize];
        screen[at] = texel;
        screen[at + 1] = texel;
        at += 2;
    }
}

/// `FUN_03005094`: fills pixels `x..x + n` of a row with `colour`, in halfwords and words. A halfword step at
/// the end can write one pixel past `n`, as the game does.
fn fill_span(screen: &mut [u8], row: usize, x: i32, n: i32, colour: u8) {
    let (mut x, mut n) = (x as usize, n);
    let mut put = |x: usize, count: usize| screen[row + x..row + x + count].fill(colour);
    if x & 1 != 0 {
        put(x, 1);
        n -= 1;
        if n < 1 {
            return;
        }
        x += 1;
    }
    if x & 2 != 0 {
        put(x, 2);
        n -= 2;
        if n < 1 {
            return;
        }
        x += 2;
    }
    put(x, 4 * (n >> 2) as usize);
    x += 4 * (n >> 2) as usize;
    n &= 3;
    if n > 1 {
        put(x, 2);
        n -= 2;
        if n < 1 {
            return;
        }
        x += 2;
    }
    if n != 0 {
        put(x, 1);
    }
}

/// `draw_flat_textured` (`FUN_03002da0`) and `draw_flat_fill` (`FUN_03003180`): scans the clipped outline from
/// its top vertex (`FUN_03005008`) down to row `portal.bottom` (exclusive). The ceiling walks the outline the
/// other way round. Textured spans run in pixel pairs: up to 32 pairs affine between perspective-correct ends,
/// longer ones in 16-pair perspective segments (`FUN_03002c40`). Fill spans are full resolution.
fn draw_flat(
    rom: &[u8],
    portal: &Portal,
    v: &[FlatVertex],
    texture: Option<&FlatTexture>,
    fill: u8,
    ceiling: bool,
    screen: &mut [u8],
) {
    let clip = (portal.top as u16 as i32, portal.bottom as u16 as i32);
    let Some(first) = v.first() else { return };
    let (mut start, mut min, mut max) = (0, first.floor_y, first.floor_y);
    for (i, p) in v.iter().enumerate() {
        if p.floor_y < min {
            (start, min) = (i, p.floor_y);
        }
        max = max.max(p.floor_y);
    }
    if min == max || clip.1 <= min || max <= clip.0 {
        return;
    }
    let mut remaining = v.len() as i32;
    let (mut l, mut r) = (
        Edge {
            index: start,
            ..Edge::default()
        },
        Edge {
            index: start,
            ..Edge::default()
        },
    );
    let (mut l_rows, mut r_rows, mut row) = (0, 0, min);
    loop {
        l_rows -= 1;
        if l_rows < 1 {
            let Some((e, y)) = next_edge(rom, v, l.index, ceiling, &mut remaining, texture.is_some(), clip) else {
                return;
            };
            (l, l_rows, row) = (e, e.rows, y);
        }
        r_rows -= 1;
        if r_rows < 1 {
            let Some((e, y)) = next_edge(rom, v, r.index, !ceiling, &mut remaining, texture.is_some(), clip) else {
                return;
            };
            (r, r_rows, row) = (e, e.rows, y);
        }
        let at = SCREEN_WIDTH * row as usize;
        if let Some(t) = texture {
            let x = l.x >> 17;
            let n = (r.x.wrapping_add(0x1FFFF) >> 17) - x;
            if n > 0 {
                let (inv, at) = (recip(rom, n) >> 8, at + 2 * x as usize);
                let (zl, zr) = (recip(rom, l.iz >> 4), recip(rom, r.iz >> 4));
                let (u, v) = (zl.wrapping_mul(l.uz >> 8) >> 4, (l.vz >> 8).wrapping_mul(zl) >> 4);
                if n < 0x21 {
                    let du = inv.wrapping_mul(((zr.wrapping_mul(r.uz >> 8) >> 4) - u) >> 8) >> 8;
                    let dv = inv.wrapping_mul(((zr.wrapping_mul(r.vz >> 8) >> 4) - v) >> 8) >> 8;
                    flat_span_affine(
                        rom,
                        t,
                        screen,
                        at,
                        n,
                        u.wrapping_add(t.scroll.0),
                        v.wrapping_add(t.scroll.1),
                        du,
                        dv,
                    );
                } else {
                    let d_iz = ((r.iz.wrapping_sub(l.iz) as i64 * inv as i64) >> 12) as i32;
                    let (duz, dvz) = (r.uz.wrapping_sub(l.uz) >> 8, r.vz.wrapping_sub(l.vz) >> 8);
                    let (mut iz, mut uz, mut vz, mut u0, mut v0) = (l.iz, l.uz, l.vz, u, v);
                    let (mut left, mut at) = (n, at);
                    while left > 0 {
                        iz = iz.wrapping_add(d_iz);
                        uz = uz.wrapping_add(inv.wrapping_mul(duz) >> 4);
                        let z = recip(rom, iz >> 4);
                        vz = vz.wrapping_add(inv.wrapping_mul(dvz) >> 4);
                        let (u1, v1) = (z.wrapping_mul(uz >> 8) >> 4, (vz >> 8).wrapping_mul(z) >> 4);
                        let (du, dv) = ((u1 - u0) >> 4, (v1 - v0) >> 4);
                        let (u, v) = (u0.wrapping_add(t.scroll.0), v0.wrapping_add(t.scroll.1));
                        flat_span_affine(rom, t, screen, at, left.min(16), u, v, du, dv);
                        (at, u0, v0, left) = (at + 32, u1, v1, left - 16);
                    }
                }
            }
        } else {
            let x = l.x >> 16;
            let n = (r.x.wrapping_add(0xFFFF) >> 16) - x;
            if n > 0 {
                fill_span(screen, at, x, n, fill);
            }
        }
        l.step();
        r.step();
        row += 1;
        if row >= clip.1 {
            return;
        }
    }
}

/// The floor and ceiling part of `draw_sector` for the clipped outline (`clip_flat`): the floor (sector `+0x08`)
/// then the ceiling (`+0x04`, at the outline's ceiling heights), each filled with sector `+0x0D` / `+0x0C`
/// when that is not 0, else textured with the material's current animation frame.
pub fn draw_sector_flats(rom: &[u8], rt: &Runtime, portal: &Portal, clipped: &[FlatVertex], screen: &mut [u8]) {
    let sector = sector_at(rom, portal.sector);
    let base = ptr(rom, LEVEL_TABLE + 0x08);
    let ceiling_outline: Vec<_> = clipped
        .iter()
        .map(|p| FlatVertex {
            floor_y: p.ceiling_y,
            ..*p
        })
        .collect();
    for (material, fill, outline, ceiling) in [
        (u16_at(rom, sector + 8), rom[sector + 0xD], clipped, false),
        (u16_at(rom, sector + 4), rom[sector + 0xC], &ceiling_outline[..], true),
    ] {
        if material == 0 {
            continue;
        }
        let state = rt.material(u16_at(rom, material_at(rom, material)));
        let rec = material_at(rom, material.wrapping_add(state.frame));
        let texture = FlatTexture {
            texels: base.wrapping_add(u32_at(rom, rec + 8) as usize),
            log2w: rom[rec + 0x1E] as u32,
            log2h: rom[rec + 0x1F] as u32,
            scroll: ((state.u_scroll as i32) << 7, (state.v_scroll as i32) << 7),
        };
        draw_flat(
            rom,
            portal,
            outline,
            (fill == 0).then_some(&texture),
            fill,
            ceiling,
            screen,
        );
    }
}

/// `draw_sector` (`FUN_0300224c`). Pass 0: walls, then the floor and ceiling, then the sector's entities when
/// `entry.flags & 0x80`, then the walls it deferred. Pass 1 (`entities_only`): only the entities.
pub fn draw_sector(
    rom: &[u8],
    frame: &Frame,
    rt: &Runtime,
    scene: &mut Scene,
    entry: &mut Portal,
    entities_only: bool,
    screen: &mut [u8],
) {
    let sector = sector_at(rom, entry.sector);
    let mut flags = rom[sector + 0x12] as u16;
    let offsets = u16_at(rom, sector + 0xA);
    if offsets != 0xFFFF {
        flags = rt.sector_offsets[offsets as usize].flags;
        if flags & 0x40 != 0 {
            return;
        }
    }
    if entities_only {
        draw_entities(rom, frame, rt, scene, entry, screen);
        return;
    }
    if flags & 8 != 0 {
        // A container: draws its chain of children (`+0x24`) through the same entry.
        let own = entry.sector;
        let mut child = u16_at(rom, sector + 0x24);
        while child != 0xFFFF {
            entry.sector = child;
            draw_sector(rom, frame, rt, scene, entry, false, screen);
            child = u16_at(rom, sector_at(rom, child) + 0x24);
        }
        entry.sector = own;
        return;
    }
    let Some((mut spans, far)) = transform_walls(rom, frame, rt, entry.sector) else {
        return;
    };
    if far {
        entry.flags |= 0x80;
    }
    let outline = setup_wall_spans(rom, frame, rt, entry, &mut spans);
    let deferred = draw_sector_walls(rom, rt, entry, &spans, None, screen);
    if u16_at(rom, sector + 8) != 0 || u16_at(rom, sector + 4) != 0 {
        let clipped = clip_flat(rom, &outline, entry.left, entry.right);
        draw_sector_flats(rom, rt, entry, &clipped, screen);
    }
    // The entity draw leaves its last clip columns in world +0xE2/+0xE4, which the deferred walls then use.
    let mut columns = *entry;
    if entry.flags & 0x80 != 0 {
        (columns.left, columns.right) = draw_entities(rom, frame, rt, scene, entry, screen);
    }
    if deferred != 0 {
        draw_sector_walls(rom, rt, &columns, &spans, Some(deferred), screen);
    }
}

/// `draw_visible_sectors` (`FUN_030048c8`): pass 0 draws every visible sector from the last list entry to the
/// first (painter's order), skipping merged entries (flag 8); pass 1 then draws, in the same order, the
/// entities of every entry without flag `0x80` (set by pass 0 when all its corners are 0x200 or deeper) or 8.
pub fn draw_world(rom: &[u8], frame: &Frame, rt: &Runtime, scene: &mut Scene, vis: &mut Visibility, screen: &mut [u8]) {
    scene.begin_frame();
    for entry in vis.portals.iter_mut().rev() {
        if entry.flags & 8 == 0 {
            draw_sector(rom, frame, rt, scene, entry, false, screen);
        }
    }
    for entry in vis.portals.iter_mut().rev() {
        if entry.flags & 0x88 == 0 {
            draw_sector(rom, frame, rt, scene, entry, true, screen);
        }
    }
}

/// `FUN_03004c40`: `recip[n]`, or `recip[n >> 1] >> 1` above `0x7FFE`.
fn recip_wide(rom: &[u8], n: i32) -> i32 {
    if n > 0x7FFE {
        recip(rom, n >> 1) >> 1
    } else {
        recip(rom, n)
    }
}

/// `draw_sector_pass_b` (`FUN_03000b44`): clips the flat outline to the columns `left..=right` (world
/// `+0xE2`/`+0xE4`, the portal's span). The game appends the result to the outline's buffer and stores its
/// length at world `+0xF4`; the floor and ceiling rasterisers read it.
pub fn clip_flat(rom: &[u8], outline: &[FlatVertex], left: i16, right: i16) -> Vec<FlatVertex> {
    let (l, r) = (left as u16 as i32, right as u16 as i32);
    // Moves every field but x from `a` towards `b` by the 2.24 fraction `t`.
    let lerp = |a: FlatVertex, b: &FlatVertex, t: i32| {
        let f = |p: i32, q: i32| p.wrapping_add(((q.wrapping_sub(p) as i64 * t as i64) >> 24) as i32);
        FlatVertex {
            floor_y: f(a.floor_y, b.floor_y),
            ceiling_y: f(a.ceiling_y, b.ceiling_y),
            recip: f(a.recip, b.recip),
            u: f(a.u, b.u),
            v: f(a.v, b.v),
            ..a
        }
    };
    let mut out = Vec::new();
    for (i, &b) in outline.iter().enumerate() {
        let (mut a, mut b) = (outline[(i + outline.len() - 1) % outline.len()], b);
        if a.x < l {
            if b.x < l {
                continue;
            }
            a = lerp(a, &b, (l - a.x).wrapping_mul(recip_wide(rom, b.x - a.x)));
            a.x = l;
        } else if a.x > r {
            if b.x > r {
                continue;
            }
            a = lerp(a, &b, (a.x - r).wrapping_mul(recip_wide(rom, a.x - b.x)));
            a.x = r;
        }
        out.push(a);
        if b.x < l {
            b = lerp(b, &a, (l - b.x).wrapping_mul(recip_wide(rom, a.x - b.x)));
            b.x = l;
            out.push(b);
        } else if b.x > r {
            b = lerp(b, &a, (b.x - r).wrapping_mul(recip_wide(rom, b.x - a.x)));
            b.x = r;
            out.push(b);
        }
    }
    out
}

/// `FUN_030047a8`: whether (x, z) lies inside the sector: on the inner side of, or on, every wall edge.
pub fn point_in_sector(rom: &[u8], sector: u16, x: i32, z: i32) -> bool {
    let (walls, count) = sector_walls(rom, sector);
    let corner = |k: usize| {
        (
            u32_at(rom, walls + 0x44 * k) as i32,
            u32_at(rom, walls + 0x44 * k + 4) as i32,
        )
    };
    (0..count).all(|k| {
        let ((px, pz), (cx, cz)) = (corner((k + count - 1) % count), corner(k));
        let cross = (cz.wrapping_sub(pz))
            .wrapping_mul(x.wrapping_sub(px))
            .wrapping_sub(z.wrapping_sub(pz).wrapping_mul(cx.wrapping_sub(px)));
        cross >= 0
    })
}

/// `FUN_03000800`: the sector holding (x, z) (world `+0xC0`/`+0xC8`), searched from `sector` (world `+0xEA`)
/// and then its neighbours across walls without flag `0x1000` (the moving piece's flags replace the wall's).
/// Neighbours come from wall `+0x32`, not the render link `+0x30`. A found neighbour with no floor is replaced
/// by its `+0x20` alias unless that is `0xFFFF`. `None` is the game's `0xFFFF`.
pub fn camera_sector(rom: &[u8], rt: &Runtime, sector: u16, x: i32, z: i32) -> Option<u16> {
    if point_in_sector(rom, sector, x, z) {
        return Some(sector);
    }
    let (walls, count) = sector_walls(rom, sector);
    (0..count).find_map(|k| {
        let w = walls + 0x44 * k;
        let flags = rt.wall_flags(u16_at(rom, w + 0x2A), u16_at(rom, w + 0x2E));
        let next = u16_at(rom, w + 0x32);
        if flags & 0x1000 != 0 || next == 0xFFFF || !point_in_sector(rom, next, x, z) {
            return None;
        }
        let s = sector_at(rom, next);
        let alias = u16_at(rom, s + 0x20);
        Some(if u16_at(rom, s + 8) == 0 && alias != 0xFFFF {
            alias
        } else {
            next
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nfsgba_testkit::rom;

    /// The reference race frame (`data/work/e5298b24/mgba/race.ss`, RAM dumped mid-frame): view `0x03000080`,
    /// camera matrix `0x030057A0`, screen rectangle, camera sector 760 (world `+0xEA`).
    const RACE: Frame = Frame {
        view: View {
            cx: 120,
            cy: 79,
            near: 64,
            focal: 150,
        },
        camera: [18, 0, 16383, 0, 16384, 0, -16383, 0, 18, -118039, -30, 64321],
        rect: [0, 240, 0, 159],
    };

    #[test]
    fn reciprocal_table() {
        let Some(rom) = rom() else { return };
        assert_eq!(
            [recip(&rom, 0), recip(&rom, 1), recip(&rom, 2), recip(&rom, 63)],
            [1 << 24, 1 << 23, 5592405, 262144]
        );
        assert_eq!(div_recip(&rom, 1000, 9), 99); // recip[9] is truncated, so the quotient rounds down
        assert_eq!(RACE.view.project_x(&rom, -323, 64), -628);
    }

    /// The visible list at `0x02018C8C` (world `+0x60`), count 9 (world `+0xEE`), sky flag world `+0xF6` = 1.
    #[test]
    fn visibility_reproduces_the_race_frame() {
        let Some(rom) = rom() else { return };
        let root = Portal {
            sector: 760,
            left: 0,
            right: 240,
            top: 0,
            bottom: 159,
            flags: 0,
            depth: 0,
        };
        let vis = visible_sectors(&rom, &RACE, root);
        let got: Vec<_> = vis
            .portals
            .iter()
            .map(|p| (p.sector, p.left, p.right, p.flags, p.depth))
            .collect();
        assert_eq!(
            got,
            [
                (760, 0, 240, 0, 0),
                (759, 106, 187, 0, 1),
                (772, 115, 144, 0, 2),
                (775, 115, 140, 0, 3),
                (771, 140, 144, 0, 3),
                (770, 115, 115, 0, 3),
                (773, 115, 135, 0, 4),
                (786, 115, 115, 0, 4),
                (773, 115, 115, 0, 5),
            ]
        );
        assert!(vis.portals[1..].iter().all(|p| (p.top, p.bottom) == (0, 159)));
        assert!(vis.sky);
        // The dump was taken during the draw: `draw_sector` has since set flag 0x80 on every entry whose
        // corners are all at least 0x200 deep. The camera's own sector is the only one with a nearer corner.
        let rt = Runtime::default();
        let far: Vec<_> = vis
            .portals
            .iter()
            .map(|p| transform_walls(&rom, &RACE, &rt, p.sector).unwrap().1)
            .collect();
        assert_eq!(far, [false, true, true, true, true, true, true, true, true]);
    }

    fn span(
        x: [i16; 2],
        depth: [i16; 2],
        top: [i16; 2],
        bottom: [i16; 2],
        u: [i32; 2],
        v: i32,
        flags: u16,
    ) -> WallSpan {
        WallSpan {
            x,
            depth,
            top,
            bottom,
            u,
            v_start: [0, v],
            v_end: [0, v],
            v_offset: 0,
            flags,
        }
    }

    /// The wall buffer at `0x02017288` (world `+0x68`) and the flat outline at `0x0201B094` (world `+0x6C`,
    /// count world `+0xF2` = 4) hold the camera sector 760, the last sector the frame drew. The material scroll
    /// slots it uses (world `+0x48`) are all zero in the dump, and it has no moving pieces.
    #[test]
    fn wall_setup_reproduces_the_race_wall_buffer() {
        let Some(rom) = rom() else { return };
        let rt = Runtime::default();
        let portal = Portal {
            sector: 760,
            left: 0,
            right: 240,
            top: 0,
            bottom: 159,
            flags: 0,
            depth: 0,
        };
        let (mut spans, far) = transform_walls(&rom, &RACE, &rt, 760).unwrap();
        assert!(!far);
        let flat = setup_wall_spans(&rom, &RACE, &rt, &portal, &mut spans);
        assert_eq!(
            spans,
            [
                span(
                    [-628, 106],
                    [64, 3558],
                    [-2525, 23],
                    [429, 77],
                    [27860, 65536],
                    16384,
                    0x1008
                ),
                span([106, 187], [3558, 3556], [50, -31], [77, 77], [0, 384], 2048, 0x601),
                span(
                    [187, 3803],
                    [3556, 64],
                    [23, -2520],
                    [77, 434],
                    [0, 37678],
                    16384,
                    0x1010
                ),
                span(
                    [1593, -324],
                    [-2519, -2522],
                    [-350, -350],
                    [290, 290],
                    [0, 3072],
                    2048,
                    0x625
                ),
            ]
        );
        let v = |x, floor_y, ceiling_y, recip, u, v| FlatVertex {
            x,
            floor_y,
            ceiling_y,
            recip,
            u,
            v,
        };
        assert_eq!(
            flat,
            [
                v(-628, 429, -5478, 258111, 66076416, -37772932),
                v(106, 77, -31, 4714, 1206784, 697672),
                v(187, 77, -31, 4716, 0, 697968),
                v(3803, 434, -5474, 258111, 0, -37775957),
            ]
        );
        // The clipped outline follows in the buffer (count world +0xF4 = 6).
        assert_eq!(
            clip_flat(&rom, &flat, portal.left, portal.right),
            [
                v(240, 429, -5478, 258111, 53113911, -37773526),
                v(0, 429, -5478, 258111, 56708974, -37773362),
                v(0, 128, -824, 41604, 10650740, -4903019),
                v(106, 77, -31, 4714, 1206784, 697672),
                v(187, 77, -31, 4716, 0, 697968),
                v(240, 82, -113, 8522, 0, 120024),
            ]
        );
    }

    /// World `+0xC0`/`+0xC8` in the dump (the last point searched this frame) lies in sector 760 (world `+0xEA`).
    #[test]
    fn camera_sector_search() {
        let Some(rom) = rom() else { return };
        let rt = Runtime::default();
        assert!(point_in_sector(&rom, 760, 118341, -64353));
        assert!(!point_in_sector(&rom, 759, 118341, -64353));
        assert_eq!(camera_sector(&rom, &rt, 760, 118341, -64353), Some(760));
        // Across the portal wall 3047 (+0x32 = 759, no flag 0x1000) into the next sector...
        assert_eq!(camera_sector(&rom, &rt, 760, 124000, -65000), Some(759));
        // ...but not through the solid wall 3046 (flag 0x1000), nor two sectors away.
        assert_eq!(camera_sector(&rom, &rt, 760, 118341, -63000), None);
        assert_eq!(camera_sector(&rom, &rt, 760, 130000, -65000), None);
    }

    /// Pass 0 of the whole reference frame (every visible sector, farthest first: walls, floors, deferred walls)
    /// against the frame being drawn in the dump (VRAM page `0x0600A000`, world `+0x50` → `+0x00`). Needs
    /// `race.vram.bin` from the reference run; skipped without it.
    #[test]
    fn world_pixels_match_the_race_frame() {
        let Some(rom) = rom() else { return };
        let Some(vram) = nfsgba_testkit::read("mgba/race.vram.bin") else {
            return;
        };
        let page = &vram[0xA000..0xA000 + SCREEN_WIDTH * 160];
        let rt = Runtime::default();
        let root = Portal {
            sector: 760,
            left: 0,
            right: 240,
            top: 0,
            bottom: 159,
            flags: 0,
            depth: 0,
        };
        let draw = |fill: u8| {
            let mut screen = vec![fill; SCREEN_WIDTH * 160];
            let mut vis = visible_sectors(&rom, &RACE, root);
            draw_world(&rom, &RACE, &rt, &mut Scene::empty(), &mut vis, &mut screen);
            screen
        };
        // Draw on two backgrounds: the pixels that differ between them were not written.
        let (a, b) = (draw(0), draw(255));
        let written: Vec<usize> = (0..a.len()).filter(|&i| a[i] == b[i]).collect();
        let differ: Vec<usize> = written.iter().copied().filter(|&i| a[i] != page[i]).collect();
        eprintln!("{} of {} pixels match", written.len() - differ.len(), written.len());
        // Everything but the sky (seen through sector 760's missing ceiling) and row 159 is world. The only
        // differences are the player car (car palette slots 160..224, in a 40×60 box), which pass 1 was drawing
        // over the floor when the dump was taken.
        assert_eq!(written.len(), 33350);
        let car = |&i: &usize| {
            (160..224).contains(&page[i]) && (100..140).contains(&(i % SCREEN_WIDTH)) && i / SCREEN_WIDTH >= 100
        };
        assert!(differ.iter().all(car), "a pixel differs outside the car");
        assert_eq!(differ.len(), 536);
    }
}
