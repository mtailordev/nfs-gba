//! The opponents: entity handler 0x29 (`FUN_0814a2a0`), their setup (`FUN_0814a390`), the racing-line
//! steering (`FUN_0814d078`) and the AI's driving model (`FUN_0813c5a8`), which drives the car body with the
//! shared rigid body, walls and car-to-car code but its own wheel contact (`FUN_081489dc`) and a speed curve
//! built at setup (`FUN_0813c394`). `docs/engine/ai.md` documents the fields and the algorithms.
//!
//! Physics struct fields the AI uses beyond the player's (`docs/engine/physics.md`): `+0x94` target heading,
//! `+0x98` steering look-ahead term, `+0x444` distance past the next waypoint line, `+0x4B0` stuck counter,
//! `+0x4D4` lane-change timer, `+0x4D6` preferred lane, `+0x4DA..+0x4E2` blocked lanes (u16 × 5), `+0x4F0`
//! follow timer and `+0x4F4` the entity followed, `+0x4F2` hunter countdown, `+0x4F8` nitro/boost timer.

use crate::math::{angle_diff, atan2, cos, div, dot, isqrt, mat_mul, mul64, recip, scale, sin, sub, udiv};
use crate::mem::Mem;
use crate::ram::body;
use crate::ram::contact::{WHEEL_SIZE, WHEELS};
use crate::ram::route::{self, CIRCUIT, OPPONENTS, RACE_TIME};
use crate::traffic::{atan2_fast, rand};
use crate::world::{
    self, DT, NONE, PLAYER, PROFILE, RACE_PHASE, W_ENTITIES, W_QUERY, W_QUERY_SECTOR, W_SEGMENTS, W_WAYPOINTS, WORLD,
    floor_height, floor_sector,
};
use crate::{Result, Sim, Unported};

/// Race mode (0 circuit/sprint, 1 elimination, 2 hunter, 0xE/0xF special setups).
const MODE: u32 = 0x0300_56E0;
const CAREER: u32 = 0x0300_00A0;
const CATCH_UP: u32 = 0x0300_0050;
const AI_SKILL: u32 = 0x0300_00BC;
const DIFFICULTY: u32 = 0x0300_5608;
const WINGMAN: u32 = 0x0300_6104;
/// Racers besides the player (0x030057EC): the car-to-car loops end here.
const RACERS: u32 = 0x0300_57EC;
/// The player's entity (a pointer).
const PLAYER_ENTITY: u32 = 0x0300_53AC;
/// Counts race frames from the start (the traffic-type counter of `traffic.rs`); the AI waits for 0x78/0xB4.
const RACE_FRAMES: u32 = 0x0300_5628;
/// Waypoint lines (0x20 bytes per waypoint): direction `+0x00/+0x04`, crossing plane `+0x10/+0x14/+0x18`.
const WAYPOINT_LINES: u32 = 0x0300_5FB4;
/// Race-start value per route (the AI's boost phase, 0 off; `race_start_setup` sets it).
const BOOST: u32 = 0x0300_6158;
/// Wheel-spin scale added to the grid/skill value (`+0x188`).
const SPIN_BIAS: u32 = 0x0300_6110;
/// Upgrade level of a computer car that is not a racer (the wingman), by wingman (`+ u·4`).
const AI_UPGRADES: u32 = 0x087F_42B4;
/// Look-ahead distances by lane when the steering term is large.
const LANE_LOOKAHEAD: u32 = 0x087F_5A50;
/// Branch-taking thresholds per difficulty (3 words).
const BRANCH_ODDS: u32 = 0x087B_FCD4;
/// Boost timer reloads and shifts per start value.
const BOOST_TIME: u32 = 0x087F_3DE8;
const BOOST_SHIFT: u32 = 0x087F_3DF4;
/// Wingman settings per wingman (`FUN_081410c0`).
const WINGMAN_GAP: u32 = 0x087F_4284;
const SURFACE_GRIP: u32 = 0x087F_5904;

/// The `opponent_effects` call (`FUN_0814e628`): the car's rear light and exhaust sprites in the 2D layer
/// (sprite pool `*0x03000058`). Rendering, so the simulation reports the call instead of making it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Effects {
    pub entity: u32,
    pub heading: u32,
    pub view: u32,
    pub size: i32,
}

fn is_non_racer(m: &Mem, e: u32) -> bool {
    m.u16(e) as u32 > m.u32(OPPONENTS)
}

/// `FUN_0814a2a0`: entity handler 0x29.
pub fn handler(sim: &mut Sim, e: u32) -> Result<Option<Effects>> {
    let m = &sim.mem;
    let visible = m.u32(e + 8) & 0x4_0004 == 0x4_0004;
    let index = m.u16(e) as u32;
    let state = m.u16(e + 0x4A);
    if state.wrapping_sub(1) < 2 {
        world::unlink_entity(&mut sim.mem, index);
        let phase = sim.mem.u32(RACE_PHASE);
        if phase > 1 && phase != 9 && phase != 4 && step(sim, e)? == 2 {
            return Ok(None);
        }
        let m = &mut sim.mem;
        world::link_entity(m, index);
        m.set_u16(e + 0xA, m.u16(e + 0xA) & 0xFFFE);
        if index == m.u32(PLAYER) {
            // The player's rim redraw (`rim_side_visible`, `draw_decal_on_atlas`) is rendering.
            m.set_u16(e + 0xA, m.u16(e + 0xA) | 1);
        }
    } else if state == 0 {
        init(sim, e)?;
    }
    if !visible {
        return Ok(None);
    }
    let m = &sim.mem;
    Ok(Some(Effects {
        entity: index,
        heading: (m.i32(e + 0x2C) >> 8) as u32 & 0x3FFF,
        view: 0x3FFFu32.wrapping_sub(m.u32(0x0300_5F9C)),
        size: if m.i32(e + 0x24) < 0x80 { 0x40 } else { 0x20 },
    }))
}

/// `FUN_0814a390`: the opponent's setup (state 0).
fn init(sim: &mut Sim, e: u32) -> Result<()> {
    let m = &mut sim.mem;
    let h = crate::ram::car::HANDLING + m.u8(e + 0x89) as u32 * 0x158;
    let p = crate::heap::alloc_zeroed(m, 0x4FC);
    let player_p = m.u32(m.u32(W_ENTITIES) + 0x8C);
    m.set_u16(e + 0x72, 0);
    m.set_u16(e + 0x74, 0);
    if m.i32(MODE) == 0xE {
        m.set_u32(0x0300_6030, 0x2CC);
    }
    m.set_u32(0x0300_614C, 0);
    // `FUN_0814f874`: on the floor (at the entity's stale position), no matrix slot.
    let floor = floor_height(m, m.u16(e + 0x78) as u32, m.i32(e + 0xC) >> 8, m.i32(e + 0x14) >> 8);
    m.set_i32(e + 0x10, floor);
    m.set_u8(e + 0x88, 0xFF);
    m.set_u16(e + 0x64, 0);
    m.set_u32(e + 0x8C, p);
    m.set_u16(e + 0x4A, 1);
    m.set_u16(e + 0x94, m.u16(e + 0x76));
    for off in [0x90, 0x9E, 0x4C, 0x18, 0x1A, 0x1C, 0x1E, 0x20, 0x22] {
        m.set_u16(e + off, 0);
    }
    let template = m.u32(WORLD + 0x38) + m.u16(e) as u32 * 0xA4;
    m.set_i32(e + 0xC, m.i32(template + 0xC));
    m.set_i32(e + 0x14, m.i32(template + 0x14));
    let heading = div((m.i32(e + 0x2C) >> 8) << 15, 0xA30);
    m.set_i32(p, heading);
    m.set_i32(p + 4, heading >> 31);
    m.set_u32(p + 0x18, 0);
    m.set_u32(p + 0x1C, 0);
    m.set_u32(p + 0x40, 1);
    let mass = m.i32(h);
    m.set_i32(
        p + 0x34,
        div(mass.wrapping_mul(m.i32(h + 0xC)).wrapping_mul(10), m.i32(h + 0x10)),
    );
    m.set_i32(
        p + 0x38,
        div(mass.wrapping_mul(m.i32(h + 8)).wrapping_mul(10), m.i32(h + 0x10)),
    );
    m.set_u32(p + 0x48, 4);
    let laps = m.u32(0x0300_56E4) as u8;
    m.set_u8(p + 0xC5, laps);
    m.set_u8(p + 0xC6, laps);
    let r = rand(m);
    m.set_u16(p + 0x4D4, (r >> 7) as u16 & 0xF);
    m.set_u16(p + 0x4D8, 1);
    m.set_u16(p + 0x4E4, 0);
    m.set_u16(p + 0x4E6, 0);
    for k in 0..5 {
        m.set_u16(p + 0x4E2 - 2 * k, 0);
    }
    m.set_u32(p + 0x4E8, 0);
    m.set_u16(p + 0x4EC, 0);
    m.set_u16(p + 0x4EE, 0);
    m.set_u16(p + 0x4F0, 0);
    let r = rand(m);
    m.set_u16(p + 0x4F2, (r as u16 & 0x3F) + 0x40);
    m.set_u16(p + 0x4F8, 0);
    m.set_u32(p + 0x4B8, 0x100);
    let mode = m.i32(MODE);
    if mode != 0xF {
        let lane = start_lane(m, e);
        m.set_u16(p + 0xC0, lane);
        m.set_u16(p + 0x4D6, lane);
    }
    if mode == 0xF || mode == 0xE {
        m.set_u16(p + 0xC0, 2);
    }
    // `FUN_0814dd24`: the floor under four points around the car (the wheels) seeds the suspension state.
    let corners = [[-0x20, 0x55], [0x20, 0x55], [-0x20, -0x2A], [0x20, -0x2A]];
    let (c, s) = (cos(m, m.i32(e + 0x2C) >> 8), sin(m, m.i32(e + 0x2C) >> 8));
    for (k, [px, pz]) in corners.into_iter().enumerate() {
        let ix = c
            .wrapping_mul(px)
            .wrapping_mul(0x100)
            .wrapping_add(pz.wrapping_mul(0x100).wrapping_mul(s))
            >> 14;
        let iz = (-s)
            .wrapping_mul(px)
            .wrapping_mul(0x100)
            .wrapping_add(pz.wrapping_mul(0x100).wrapping_mul(c))
            >> 14;
        m.set_i32(W_QUERY, m.i32(e + 0xC).wrapping_add(ix) >> 8);
        m.set_i32(W_QUERY + 4, m.i32(e + 0x10) >> 8);
        m.set_i32(W_QUERY + 8, m.i32(e + 0x14).wrapping_add(iz) >> 8);
        m.set_u16(W_QUERY_SECTOR, m.u16(e + 0x78));
        let mut s = world::find_sector_near_query(m);
        if s == NONE {
            s = m.u16(W_QUERY_SECTOR) as u32;
        }
        if s == NONE {
            // The point's height would be the caller's uninitialised stack word.
            return Err(Unported("FUN_0814dd24 with the car outside every sector"));
        }
        let y = floor_height(m, s, m.i32(W_QUERY), m.i32(W_QUERY + 8));
        m.set_i32(p + 0x4C + 4 * k as u32, y);
        m.set_i32(p + 0x6C + 4 * k as u32, y);
    }
    let upgrades: [i32; 10] = if !is_non_racer(m, e) {
        if m.i32(CAREER) == 0 {
            std::array::from_fn(|k| m.i32(player_p + 0x3E0 + 4 * k as u32))
        } else {
            [udiv(m.u32(AI_SKILL), 10) as i32 - 1; 10]
        }
    } else {
        let mut u = m.u32(WINGMAN).wrapping_sub(1);
        if m.i32(CAREER) != 0 {
            u = (u & 1) + 6;
        }
        // The game's copy loop never advances its source: all ten levels are the wingman's one entry.
        [m.i32(AI_UPGRADES.wrapping_add(u.wrapping_mul(4))); 10]
    };
    for (k, v) in upgrades.into_iter().enumerate() {
        m.set_i32(p + 0x3E0 + 4 * k as u32, v);
    }
    crate::ram::init::nitro_setup(m, e, p);
    crate::ram::init::setup_handling(m, e, h);
    speed_curve(m, e);
    crate::ram::init::setup_handling(m, e, h);
    wingman_setup(m);
    for k in 0..4 {
        let g = p + 0x210 + WHEEL_SIZE * k;
        m.set_i32(g, m.i32(g).wrapping_mul(0x1400) >> 12);
    }
    crate::ram::init::race_start_setup(sim, e)?;
    let m = &mut sim.mem;
    m.set_u16(e + 0xA, m.u16(e + 0xA) | 0x20);
    if m.u16(e) as u32 == m.u32(PLAYER) {
        return Err(Unported("the player's car on the opponent handler (unpack_decal)"));
    }
    Ok(())
}

/// `FUN_0814059c`: the lane (0..4) the car starts in, from its offset across the route.
fn start_lane(m: &Mem, e: u32) -> u16 {
    let v = route::lateral(m, e);
    let r = if v + 0x80 < 0 { v + 0x17F } else { v + 0x80 };
    ((r >> 8) + 2).clamp(0, 4) as u16
}

/// `FUN_081410c0`: the wingman's globals for the race.
fn wingman_setup(m: &mut Mem) {
    for a in [0x0300_61EC, 0x0300_61E8, 0x0300_61D8, 0x0300_61D4, 0x0300_6200] {
        m.set_u32(a, 0);
    }
    m.set_u32(0x0300_619C, m.u32(W_ENTITIES));
    for a in [0x0300_61F8, 0x0300_618C, 0x0300_61F0, 0x0300_6174] {
        m.set_u32(a, 0);
    }
    let u = m.u32(WINGMAN).wrapping_sub(1);
    if u < 0xC {
        m.set_u32(0x0300_61F8, u & 1);
        m.set_u32(0x0300_61DC, m.u32(WINGMAN_GAP + u * 4));
        m.set_u32(0x0300_6188, 0x7_8000);
    }
    m.set_u32(0x0300_61E4, m.u32(0x0300_6188));
}

/// `FUN_0813c394`: the drive force the AI can use at 20 wheel speeds, a curve in the profile at `+0x26C`
/// (count 0x15, x from 0 to `+0x274`, values at `+0x27C`), found by running the gearbox at each speed.
fn speed_curve(m: &mut Mem, e: u32) {
    let p = m.u32(e + 0x8C);
    let h = crate::ram::car::HANDLING + m.u8(e + 0x89) as u32 * 0x158;
    let prof = m.u32(PROFILE);
    m.set_u32(prof + 0x270, 0);
    m.set_u32(prof + 0x26C, 0x15);
    m.set_u32(prof + 0x278, prof + 0x27C);
    let top = recip(m, m.i32((p + 0x408).wrapping_add(m.u32(h + 0x54).wrapping_mul(4))));
    let span = m.i32(p + 0x454).wrapping_mul(top).wrapping_mul(0x180);
    m.set_i32(prof + 0x274, if span < 0 { span + 0xFF } else { span } >> 8);
    let step = div(span, 0x14);
    for k in 0..0x14i32 {
        m.set_u32(RACE_PHASE, 1);
        m.set_u32(p + 0x40, 1);
        let mut ratio = 0;
        for _ in 0..7 {
            m.set_u32(p + 0x9C, 0);
            ratio = m.i32((p + 0x408).wrapping_add(m.u32(p + 0x40).wrapping_mul(4)));
            let rpm = div((mul64(k.wrapping_mul(step), ratio) >> 30) as i32, 6);
            m.set_i32(p + 0x3C, rpm);
            crate::ram::car::auto_shift(m, e);
        }
        m.set_u32(RACE_PHASE, 9);
        if m.i32(p + 0x454) < m.i32(p + 0x3C) {
            m.set_i32(p + 0x3C, m.i32(p + 0x454));
        }
        if m.i32(p + 0x3C) < m.i32(h + 0x68) {
            m.set_i32(p + 0x3C, m.i32(h + 0x68));
        }
        let rpm = m.i32(p + 0x3C);
        let t = crate::ram::car::torque(m, p, rpm);
        let drive = mul64(t.wrapping_mul(0x9999), ratio) >> 15;
        let drive = (drive.wrapping_mul(m.i32(p + 0x4BC) as i64) >> 15) as i32;
        let prof = m.u32(PROFILE);
        m.set_i32(
            prof + 0x27C + 4 * k as u32,
            (drive.wrapping_add(rpm.wrapping_mul(-0x10)) >> 8).wrapping_mul(0x90),
        );
    }
    let prof = m.u32(PROFILE);
    m.set_u32(prof + 0x27C + 0x14 * 4, 0xFFF6_0000);
    m.set_u32(p + 0x40, 1);
    m.set_i32(p + 0x3C, m.i32(h + 0x68));
}

/// `FUN_081401d4`: how far `target` is ahead of the car along the lap (wrapped into -lap/4 .. 3·lap/4).
fn gap_to(m: &Mem, e: u32, target: u32) -> i32 {
    let lap = lap_length(m);
    let mut d = m
        .i32(m.u32(target + 0x8C) + 0xAC)
        .wrapping_sub(m.i32(m.u32(e + 0x8C) + 0xAC));
    if d < -lap >> 2 {
        d += lap;
    }
    if (lap >> 2) + (lap >> 1) < d {
        d -= lap;
    }
    d
}

/// The last main-route waypoint's distance: the lap length.
fn lap_length(m: &Mem) -> i32 {
    let segs = m.u32(W_SEGMENTS);
    m.i32(
        m.u32(W_WAYPOINTS)
            .wrapping_add(m.u32(segs + 4).wrapping_mul(0x18))
            .wrapping_add(m.u16(segs) as u32 * 0x18)
            .wrapping_sub(8),
    )
}

/// `FUN_081400ec`: race progress over all laps.
fn race_progress(m: &Mem, e: u32) -> i32 {
    let p = m.u32(e + 0x8C);
    if m.i32(CIRCUIT) == 0 {
        m.i32(p + 0xAC)
    } else {
        lap_length(m)
            .wrapping_mul(m.i32(0x0300_56E4) - m.i8(p + 0xC5) as i32)
            .wrapping_add(m.i32(p + 0xAC))
    }
}

/// `FUN_0815fe5c`: the point of line `a`→`b` nearest (`x`, `z`) (`__divdi3` on 64-bit products).
fn nearest_on_line(a: [i32; 2], b: [i32; 2], x: i32, z: i32) -> [i32; 2] {
    let (dx, dz) = (b[0].wrapping_sub(a[0]), b[1].wrapping_sub(a[1]));
    let t = dx
        .wrapping_mul(x.wrapping_sub(a[0]))
        .wrapping_add(z.wrapping_sub(a[1]).wrapping_mul(dz));
    let den = a[0]
        .wrapping_sub(b[0])
        .wrapping_mul(x.wrapping_sub(b[0]))
        .wrapping_add(a[1].wrapping_sub(b[1]).wrapping_mul(z.wrapping_sub(b[1])))
        .wrapping_add(t);
    if den == 0 {
        [
            dx.wrapping_mul(t).wrapping_add(a[0]),
            a[1].wrapping_add(t.wrapping_mul(dz)),
        ]
    } else {
        let q = |d: i32| ((d as i64 * t as i64) / den as i64) as i32;
        [q(dx).wrapping_add(a[0]), a[1].wrapping_add(q(dz))]
    }
}

/// `FUN_0813f530` (an opponent stuck for 0x32 steps): put the car on the route segment its sector belongs to,
/// by the route's side-segment table (per entry: segment, its sectors, -1). On the main route: the side segment
/// whose sectors include the car's, at its nearer end. On a side segment it has left: back to the main route
/// where the segment joins it. Like `route::track_segment`, without marking segments visited and without
/// switching from one side segment to another.
fn resync_segment(m: &mut Mem, e: u32) {
    let list = m.u32(0x087F_37D8 + m.u32(0x0300_5720) * 4);
    if list == 0 {
        return;
    }
    let (sector, cur) = (m.u16(e + 0x78) as u32, m.u16(e + 0x72) as u32);
    let d2 = |m: &Mem, w: u32| {
        let dx = ((m.i32(e + 0xC) >> 8) - m.i32(w)) >> 4;
        let dz = ((m.i32(e + 0x14) >> 8) - m.i32(w + 4)) >> 4;
        dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz))
    };
    let seg_count = |m: &Mem, seg: u32| m.u16(m.u32(W_SEGMENTS) + seg * 8) as i32;
    let mut at = list + 4;
    for _ in 0..m.u32(list) {
        let seg = m.u32(at);
        at += 4;
        if cur == 0 {
            while m.u32(at) != u32::MAX {
                let s = m.u32(at);
                at += 4;
                if s == sector {
                    m.set_u16(e + 0x72, seg as u16);
                    let n = seg_count(m, seg);
                    let near_first = d2(m, route::waypoint(m, seg, 0)) < d2(m, route::waypoint(m, seg, n - 1));
                    m.set_i16(e + 0x90, if near_first { 0 } else { n - 1 } as i16);
                    return;
                }
            }
        } else if seg == cur {
            while m.u32(at) != u32::MAX {
                if m.u32(at) == sector {
                    return;
                }
                at += 4;
            }
            let last = route::waypoint(m, cur, seg_count(m, cur) - 1);
            let v = if d2(m, route::waypoint(m, cur, 0)) < d2(m, last) {
                m.u32(m.u32(0x0300_5FB8) + cur * 4) as u16
            } else {
                m.u16(last + 0xE)
            };
            m.set_u16(e + 0x90, v);
            m.set_u16(e + 0x72, 0);
            return;
        } else {
            while m.u32(at) != u32::MAX {
                at += 4;
            }
        }
        at += 4;
    }
}

fn xz(m: &Mem, w: u32) -> [i32; 2] {
    [m.i32(w), m.i32(w + 4)]
}

/// `FUN_0814d078`: follow the racing line (or the entity at `+0x4F4`): advance the waypoint, maybe take a
/// branch, aim at a point ahead on the line in the car's lane (`+0x94` heading, `+0x98` curve term), then drive.
fn step(sim: &mut Sim, e: u32) -> Result<i32> {
    let m = &mut sim.mem;
    let p = m.u32(e + 0x8C);
    let target = m.u32(p + 0x4F4);
    let mut ahead = 0xC80;
    crate::ram::car::nitro(m, e);
    let seg_rec = |m: &Mem| m.u32(W_SEGMENTS) + m.u16(e + 0x72) as u32 * 8;
    let mut wp = m.i16(e + 0x90) as i32;
    let mut a = route::waypoint_at(m, m.u16(e + 0x72) as u32, wp);
    let mut b = route::waypoint_at(m, m.u16(e + 0x72) as u32, wp + 1);
    if m.i32(p + 0x4B0) == 0x32 {
        resync_segment(m, e);
        wp = m.i16(e + 0x90) as i32;
        a = route::waypoint_at(m, m.u16(e + 0x72) as u32, wp);
        b = route::waypoint_at(m, m.u16(e + 0x72) as u32, wp + 1);
    }
    let stuck = m.i32(p + 0x4B0);
    if stuck > 0x96 {
        let player_wp = m.i16(m.u32(PLAYER_ENTITY) + 0x90) as i32;
        if stuck > 0xFA || (m.u16(e + 0xA) & 4 == 0 && player_wp != wp && player_wp != wp + 1) {
            m.set_i32(p + 0x4B0, 0);
            crate::ram::car::put_back_on_road(m, e, b);
        }
    }
    if m.u16(e + 0x4A) == 2 {
        let n = m.u16(seg_rec(m)) as i32;
        wp = n - 1;
        a = route::waypoint_at(m, m.u16(e + 0x72) as u32, n - 1);
        b = route::waypoint_at(m, m.u16(e + 0x72) as u32, n);
        m.set_u32(p + 0x4B0, 0);
    }
    let mut look = (m.i32(p + 0x44) >> 8) - 200;
    let turn = m.i32(p + 0x98);
    if turn.wrapping_abs() > 0x1000 {
        let lane = m.i16(p + 0xC0) as i32;
        let k = if turn < 1 { 4 - lane } else { lane };
        ahead = m.i32(LANE_LOOKAHEAD.wrapping_add((k * 4) as u32));
    }
    if m.u16(p + 0x4D8) & 0x300 != 0 {
        ahead = 0x708;
        m.set_u16(p + 0x4D8, m.u16(p + 0x4D8) & 0xFCFF);
    }
    if ahead < look {
        look = ahead;
    }
    if look <= 0x63F {
        look = 0x640;
    }
    let (next, s) = route::advance(m, m.u16(e + 0x72) as u32, wp + 1);
    let line = m
        .u32(WAYPOINT_LINES)
        .wrapping_add(((next + m.i32(m.u32(W_SEGMENTS) + s * 8 + 4)) as u32).wrapping_mul(0x20));
    let past = (m.i32(e + 0xC) >> 8)
        .wrapping_mul(m.i32(line + 0x10))
        .wrapping_add(m.i32(line + 0x14).wrapping_mul(m.i32(e + 0x14) >> 8))
        .wrapping_sub(m.i32(line + 0x18));
    m.set_i32(p + 0x444, past);
    if past > 0 {
        m.set_u16(p + 0x4D8, m.u16(p + 0x4D8) & 0xFFFE);
        let count = m.u16(seg_rec(m)) as i32;
        if m.u16(b + 0xE) as u32 == NONE || wp != count - 2 {
            wp += 1;
            if wp == count - 1 && m.u16(e + 0x72) == 0 {
                wp = 0;
            }
            a = route::waypoint_at(m, m.u16(e + 0x72) as u32, wp);
        } else {
            m.set_u16(e + 0x72, m.u16(b + 0xC));
            m.set_u16(p + 0x4D6, 2);
            wp = m.u16(b + 0xE) as i32;
            a = route::waypoint_at(m, m.u16(e + 0x72) as u32, wp);
        }
        b = route::waypoint_at(m, m.u16(e + 0x72) as u32, wp + 1);
        if ((wp - 1) as u32) < 7 {
            m.set_u16(p + 0x4D8, m.u16(p + 0x4D8) | 2);
        }
        if m.i16(b + 0xE) == 0 && m.i32(p + 0xA8) != 1 && !is_non_racer(m, e) {
            let odds = [m.i32(BRANCH_ODDS), m.i32(BRANCH_ODDS + 4), m.i32(BRANCH_ODDS + 8)];
            let Some(&odds) = odds.get(m.u32(DIFFICULTY) as usize) else {
                return Err(Unported(
                    "branch odds for a difficulty above 2 (the game reads its stack)",
                ));
            };
            let r = rand(m);
            let branch = m.u16(b + 0xC) as u32;
            if odds < (r & 0xFF) as i32 && m.i16(b + 0xE) == 0 && m.i32(0x0300_60C0 + branch * 4) != 0 {
                m.set_u16(e + 0x72, branch as u16);
                m.set_u16(p + 0x4D6, 2);
                let first = m.u16(b + 0xE) as i32;
                wp = first - 1;
                a = route::waypoint_at(m, m.u16(e + 0x72) as u32, first - 1);
                b = route::waypoint_at(m, m.u16(e + 0x72) as u32, first);
                m.set_u16(p + 0x4D8, m.u16(p + 0x4D8) | 2);
            }
        }
        m.set_i16(e + 0x90, wp as i16);
        route::lap(m, e)?;
    }
    let following = m.i16(p + 0x4F0) != 0 && {
        let d = gap_to(m, e, target);
        m.i16(p + 0x4F0) != 0 && d >= 0
    };
    if following {
        let h = atan2_fast(
            m,
            (m.i32(target + 0xC) >> 8) - (m.i32(e + 0xC) >> 8),
            (m.i32(target + 0x14) >> 8) - (m.i32(e + 0x14) >> 8),
        );
        m.set_i32(p + 0x94, h);
        m.set_i32(p + 0x98, 0);
    } else {
        let (pa, pb) = (xz(m, a), xz(m, b));
        let near = nearest_on_line(pa, pb, m.i32(e + 0xC) >> 8, m.i32(e + 0x14) >> 8);
        let (mut dx, mut dz) = (pb[0].wrapping_sub(pa[0]), pb[1].wrapping_sub(pa[1]));
        let dir = atan2_fast(m, dz, dx);
        let reach = look * 0x160 >> 8;
        let mut tx = near[0] + (reach.wrapping_mul(cos(m, dir)) >> 14);
        let mut tz = near[1] + (reach.wrapping_mul(sin(m, dir)) >> 14);
        let c = route::waypoint_at(m, m.u16(e + 0x72) as u32, wp + 2);
        let pc = xz(m, c);
        let mid = [(pa[0] + pb[0]) >> 1, (pa[1] + pb[1]) >> 1];
        let d2 = |x: i32, z: i32, o: [i32; 2]| {
            let (u, v) = (x.wrapping_sub(o[0]), z.wrapping_sub(o[1]));
            u.wrapping_mul(u).wrapping_add(v.wrapping_mul(v))
        };
        if d2(tx, tz, mid) >= d2(pa[0], pa[1], mid) {
            // Past the segment: carry the remaining reach onto the next one.
            let r = isqrt(d2(tx, tz, pb) as u32);
            dx = pc[0].wrapping_sub(pb[0]);
            dz = pc[1].wrapping_sub(pb[1]);
            let dir2 = atan2_fast(m, dz, dx);
            tx = pb[0] + (r.wrapping_mul(cos(m, dir2)) >> 14);
            tz = pb[1] + (r.wrapping_mul(sin(m, dir2)) >> 14);
            let mid2 = [(pb[0] + pc[0]) >> 1, (pb[1] + pc[1]) >> 1];
            if d2(pb[0], pb[1], mid2) <= d2(tx, tz, mid2) {
                (tx, tz) = (pc[0], pc[1]);
            }
        }
        let next_dir = atan2_fast(m, pc[1].wrapping_sub(pb[1]), pc[0].wrapping_sub(pb[0]));
        m.set_i32(p + 0x98, angle_diff(dir, next_dir) * 0x14 >> 4);
        let lane = m.i16(p + 0xC0) as i32 - 2;
        let off = if m.i32(0x0300_56F0) < 0x32 {
            lane << 9
        } else {
            lane << 8
        };
        if off != 0 {
            let side = atan2_fast(m, dx.wrapping_neg(), dz);
            tx += off.wrapping_mul(cos(m, side)) >> 14;
            tz += off.wrapping_mul(sin(m, side)) >> 14;
        }
        let h = atan2_fast(m, tx - (m.i32(e + 0xC) >> 8), tz - (m.i32(e + 0x14) >> 8));
        m.set_i32(p + 0x94, h);
    }
    let dt = sim.mem.i32(DT);
    drive(sim, e, dt)?;
    Ok(1)
}

/// `FUN_0813c5a8(world, e, 0, 1, frame_time)`: the AI's driving: steer towards `+0x94`, choose throttle and
/// brake (catch-up, lane changes, boost), spin the wheels, then the body step shared with the player's car.
fn drive(sim: &mut Sim, e: u32, frame_time: i32) -> Result<()> {
    let m = &mut sim.mem;
    let p = m.u32(e + 0x8C);
    let b = p + 0xC8;
    let old_sector = m.u16(e + 0x78);
    let mut boost = false;
    let mut accelerate = false;
    let dt = recip(m, frame_time << 8).min(0xC00);
    if m.i16(p + 0x4E4) > 100 && m.i16(p + 0x4E6) == 0 && m.i32(p + 0x44) <= 0x7FFF {
        let w = route::waypoint_at(m, m.u16(e + 0x72) as u32, m.i16(e + 0x90) as i32);
        crate::ram::car::put_back_on_road(m, e, w);
        m.set_i16(p + 0x4E4, 0);
    }
    for k in 0..4 {
        let w = p + WHEELS + WHEEL_SIZE * k;
        m.set_i32(w + 0x80, m.i32(w + 0x84).wrapping_mul(m.i32(p + 0x4B8)) >> 8);
    }
    if m.i32(p + 0x4B8) < 0x100 {
        m.set_i32(p + 0x4B8, m.i32(p + 0x4B8) + 4);
    }
    let heading = atan2(m.i32(p + 0x140) >> 4, m.i32(p + 0x148) >> 4);
    m.set_i32(p, heading);
    m.set_i32(p + 4, heading >> 31);
    m.set_i32(e + 0x2C, heading << 8);
    let behind = m.i16(p + 0x4F0) != 0 && {
        let d = gap_to(m, e, m.u32(p + 0x4F4));
        m.i16(p + 0x4F0) != 0 && d < 0
    };
    if behind {
        m.set_i32(p + 0x28, 0x9999);
        m.set_u32(p + 0x4B0, 0);
    } else {
        m.set_i32(p + 0x28, 0);
    }
    let mut h = m.i32(e + 0x2C) >> 8;
    if h < 0 {
        h += 0x4000;
    }
    h &= 0x3FFF;
    let f = dot(m.vec3(p + 0x11C), m.vec3(p + 0x140));
    m.set_i32(p + 0xA0, f.wrapping_mul(0x20));
    m.set_i32(
        p + 0x44,
        if f.wrapping_mul(0x20) < 0 {
            f.wrapping_mul(-0x20)
        } else {
            f.wrapping_mul(0x20)
        },
    );
    let mut steer = angle_diff(h, m.i32(p + 0x94)) << 6 >> 8;
    if is_non_racer(m, e) {
        steer = traffic_steer(m, e, steer);
    }
    let speed = m.i32(p + 0x44);
    if speed < 0x2_0001 {
        m.set_i32(p + 0x4B0, m.i32(p + 0x4B0) + 1);
        accelerate = true;
    } else {
        // Brake into sharp curves: scrub momentum and set the brake (+0x28).
        let v = m.i32(p + 0x444) + 0x1_0000;
        if v > 0 {
            let t = m.i32(p + 0x98);
            let at = t.wrapping_abs();
            if at > 0x400 {
                let limit = if at < 0x901 {
                    let w = t.wrapping_mul(0xC0).wrapping_abs();
                    (v - w.wrapping_add(0xFFED_D420u32 as i32)).wrapping_mul(0x11) >> 3
                } else {
                    (m.i32(p + 0x444) + 0x1C_0000 + at * -0x80).wrapping_mul(10) >> 4
                };
                if limit < speed {
                    let (k, shift) = if at < 0x901 {
                        ((at >> 6) + 0x40, 3)
                    } else {
                        ((at >> 2) + 0x80, 2)
                    };
                    let mom = m.vec3(b + body::MOMENTUM);
                    m.set_vec3(b + body::MOMENTUM, sub(mom, scale(mom, k)));
                    m.set_i32(p + 0x28, (m.i32(p + 0x44) - limit) >> shift);
                }
            }
        }
        if m.u16(e + 0x4A) != 2 {
            if m.i32(RACE_FRAMES) > 0x78 && (m.i16(p + 0x4DC) == 0 || m.i16(p + 0x4DE) == 0 || m.i16(p + 0x4E0) == 0) {
                m.set_u16(p + 0x4DA, 1);
                m.set_u16(p + 0x4E2, 1);
            }
            let timer = m.u16(p + 0x4D4).wrapping_add(1);
            m.set_u16(p + 0x4D4, timer);
            let lanes = p + 0x4DA;
            if timer > 300 {
                let l = lanes.wrapping_add((m.i16(p + 0xC0) as i32 * 2) as u32);
                m.set_u16(l, m.u16(l) | 2);
            }
            m.set_u16(p + 0xC0, m.u16(p + 0x4D6));
            let blocked = |m: &Mem, lane: u32| m.i16(lanes.wrapping_add(lane.wrapping_mul(2))) != 0;
            if blocked(m, m.i16(p + 0xC0) as i32 as u32) {
                // Change lane: try ±1, ±2, ±3 from the current one, starting on a random side.
                let dir = if rand(m) & 1 != 0 { -1 } else { 1 };
                let mut d = dir;
                for _ in 1..4 {
                    let cur = m.i16(p + 0xC0) as i32;
                    let up = (cur + d) as u32;
                    if up < 5 && !blocked(m, up) {
                        m.set_u16(p + 0xC0, up as u16);
                        m.set_u16(p + 0x4D6, up as u16);
                        break;
                    }
                    let down = (cur - d) as u32;
                    if down < 5 && !blocked(m, down) {
                        m.set_u16(p + 0xC0, down as u16);
                        m.set_u16(p + 0x4D6, down as u16);
                        break;
                    }
                    d += dir;
                }
                // NOT 1:1 (T1): the race time is read when the AI runs; the VBlank IRQ may have counted it
                // up since the frame began (docs/engine/ai.md).
                m.set_u16(p + 0x4D4, (m.u32(RACE_TIME) & 0x1F) as u16);
            }
            accelerate = true;
        }
        if m.u16(e + 0x72) != 0 && !is_non_racer(m, e) {
            m.set_u16(p + 0xC0, 2);
        }
        if m.u32(p + 0x448) & 0x10 == 0 {
            m.set_u32(p + 0x4B0, 0);
        } else {
            m.set_i32(p + 0x4B0, m.i32(p + 0x4B0) + 1);
        }
    }
    for k in 0..5 {
        m.set_u16(p + 0x4DA + 2 * k, 0);
    }
    steer = steer.clamp(-0x1000, 0x1000);
    m.set_i32(p + 0x20, steer.wrapping_mul(0x15E));
    if m.i32(p + 0x28) == 0 {
        accelerate = true;
    }
    if m.u16(e + 0x4A) == 2 {
        m.set_i32(p + 0x20, 0);
        m.set_i32(p + 0x18, 0);
        m.set_i32(p + 0x1C, 0);
        accelerate = false;
        m.set_i32(p + 0x28, 0x1_9999);
        if m.u16(p + 0x4D8) & 8 != 0 {
            m.set_vec3(b + body::MOMENTUM, m.vec3(0x087F_3DD0));
        }
        m.set_i32(b + body::MOMENTUM, m.i32(b + body::MOMENTUM).wrapping_mul(0xFA) >> 8);
        m.set_i32(
            b + body::MOMENTUM + 8,
            m.i32(b + body::MOMENTUM + 8).wrapping_mul(0xFA) >> 8,
        );
        m.set_u16(p + 0xC0, m.u16(e));
    }
    m.set_i32(p + 0x90, m.i32(p + 0x90).wrapping_add(m.i32(p + 0xA0)));
    let start = m.u32(BOOST);
    if start != 0 {
        let t = m.i16(p + 0x4F8);
        if t < 0 {
            m.set_i16(p + 0x4F8, t + 1);
        } else if m.i32(p + 0x98).wrapping_abs() > 0x3FF {
            if t != 0 {
                // ARM `lsl` by 32 or more gives 0.
                let v = (t as i32)
                    .wrapping_sub(m.i32(BOOST_TIME + start * 4))
                    .checked_shl(m.u32(BOOST_SHIFT + start * 4) & 0xFF)
                    .unwrap_or(0);
                m.set_i16(p + 0x4F8, v as i16);
            }
        } else {
            let (seg, wp) = (m.u16(e + 0x72) as u32, m.i16(e + 0x90) as i32);
            let line = |m: &Mem, k: i32| {
                let (i, s) = route::advance(m, seg, wp + k);
                let first = m.i32(m.u32(W_SEGMENTS) + s * 8 + 4);
                m.u32(WAYPOINT_LINES)
                    .wrapping_add(((i + first) as u32).wrapping_mul(0x20))
            };
            let (l1, l2) = (line(m, 1), line(m, 2));
            if m.i32(l1)
                .wrapping_mul(m.i32(l2))
                .wrapping_add(m.i32(l1 + 4).wrapping_mul(m.i32(l2 + 4)))
                > 0xA000
            {
                if m.u8(p + 0x4D1) == 0 {
                    m.set_i16(p + 0x4F8, m.i32(BOOST_TIME + start * 4) as i16);
                } else {
                    let v = t - 1;
                    m.set_i16(p + 0x4F8, v);
                    if v == 0 {
                        m.set_i16(p + 0x4F8, (m.i32(BOOST_TIME + start * 4) as i16).wrapping_mul(-8));
                    }
                }
                boost = true;
            }
        }
    }
    if m.u8(p + 0xC5) == 1 && m.u16(m.u32(W_SEGMENTS)) as i32 - 3 <= m.i16(e + 0x90) as i32 && start == 2 {
        boost = true;
    }
    if !accelerate {
        m.set_i32(p + 0x24, 0);
    } else {
        if m.i32(p + 0x4C8) == 0 {
            m.set_u8(p + 0x4D1, 0);
        } else {
            m.set_u8(p + 0x4D1, boost as u8);
        }
        m.set_i32(p + 0x24, 0x9999);
        if m.i32(p + 0x44) <= 0x7FFF && m.i32(p + 0x20).wrapping_abs() > 0x4_0000 {
            m.set_i32(p + 0x24, 0x2666);
        }
    }
    if m.i32(p + 0x42C) != 0 {
        m.set_i32(p + 0x42C, m.i32(p + 0x42C) - 1);
        m.set_i32(p + 0x20, 0);
        m.set_i32(p + 0x24, 0);
    }
    if m.i32(MODE) == 2 {
        return Err(Unported("hunter races: FUN_0813fdb0, hunter_life_tick"));
    }
    let player = m.u32(W_ENTITIES) + m.u32(PLAYER) * 0xA4;
    let lead = race_progress(m, player).wrapping_sub(race_progress(m, e));
    let mut throttle = if m.i16(p + 0x4F0) != 0 {
        m.i32(p + 0x24)
    } else if is_non_racer(m, e) {
        // The wingman's throttle, kept to 24 bits (`<< 8 >> 8`).
        let t = wingman_throttle(m, e, m.i32(p + 0x24), lead) << 8 >> 8;
        mark_blocked_lanes(m, e);
        t
    } else {
        let mut t = m.i32(p + 0x24);
        if m.i32(CATCH_UP) != 0 {
            t = if lead < 1 {
                let v = (-0xC0 - lead).clamp(0, 0x800);
                (0x100 - div(v, 0x1D)).wrapping_mul(t)
            } else {
                let v = (lead - 0xC0).clamp(0, 0x400);
                t.wrapping_mul(div(v, 0x34) + 0x100)
            } >> 8;
        }
        mark_blocked_lanes(m, e);
        t
    };
    if m.i32(MODE) == 2 && m.i32(p + 0xA8) == 1 {
        throttle >>= 1;
    }
    if m.u8(p + 0x4D1) != 0 {
        throttle = (throttle as u32).wrapping_mul(m.u16(p + 0x4CE) as u32) as i32 >> 12;
    }
    let brake = if throttle < 0 {
        throttle = 0;
        0x1_9999
    } else {
        let prof = m.u32(PROFILE);
        let x = (m.i32(p + 0x3DC) >> 8).wrapping_mul(0x109A) >> 8;
        throttle = throttle.wrapping_mul(crate::ram::car::curve(m, prof + 0x26C, x) >> 15);
        m.i32(p + 0x28)
    };
    for k in 0..4 {
        let w = p + WHEELS + WHEEL_SIZE * k;
        let push = ((m.i32(w + 0x70) >> 4).wrapping_mul(throttle >> 10) >> 8)
            .wrapping_mul(m.i32(p + 0x188).wrapping_add(m.i32(SPIN_BIAS)))
            >> 8;
        let mut spin = m.i32(w + 0x64).wrapping_add(push);
        m.set_i32(w + 0x64, spin);
        if brake > 0 {
            if spin < 0 {
                spin += brake * 4;
                m.set_i32(w + 0x64, spin.min(0));
            } else {
                spin -= brake * 4;
                m.set_i32(w + 0x64, spin.max(0));
            }
        }
    }
    m.set_u16(W_QUERY_SECTOR, m.u16(e + 0x78));
    let s = world::find_sector(
        m,
        m.u16(e + 0x78) as u32,
        m.i32(e + 0xC),
        m.i32(e + 0x10),
        m.i32(e + 0x14),
    )?;
    m.set_u16(e + 0x78, if s & 0xFFFF == NONE { old_sector as u32 } else { s } as u16);
    m.set_i32(
        b + body::MOMENTUM + 4,
        m.i32(b + body::MOMENTUM + 4) + (dt * (m.i32(0x0300_6030).wrapping_mul(m.i32(b)) >> 12) >> 11),
    );
    if m.i32(p + 0x138) < 0xF21 {
        crate::ram::contact::tipped(sim, e, dt);
        let m = &mut sim.mem;
        for k in 0..4u32 {
            m.set_i32(
                p + crate::ram::contact::WHEELS + crate::ram::contact::WHEEL_SIZE * k + 0x64,
                0,
            );
        }
        m.set_i16(p + 0x4E4, m.i16(p + 0x4E4).wrapping_add(1));
    } else {
        wheels(m, e, dt);
        m.set_u16(p + 0x4E4, 0);
    }
    let m = &mut sim.mem;
    m.set_u32(p + 0x448, 0);
    let (x, z) = (
        m.i32(e + 0xC).wrapping_add(m.i32(p + 0x140) * 3) >> 8,
        m.i32(e + 0x14).wrapping_add(m.i32(p + 0x148) * 3) >> 8,
    );
    let (y, sector) = (m.i32(e + 0x10) >> 8, m.u16(e + 0x78) as u32);
    crate::ram::walls::walls(sim, e, x, z, y, sector, false)?;
    let m = &mut sim.mem;
    let start = m.vec3(e + 0xC);
    body::integrate(m, b, dt << 1);
    let rot: [i32; 9] = std::array::from_fn(|k| m.i32(p + 0x128 + 4 * k as u32));
    let offset = mat_mul([0, m.i32(p + 0x43C), m.i32(p + 0x440)], &rot);
    m.set_vec3(e + 0xC, sub(m.vec3(p + 0xD0), offset));
    m.set_u16(W_QUERY_SECTOR, m.u16(e + 0x78));
    let s = world::find_sector(
        m,
        m.u16(e + 0x78) as u32,
        m.i32(e + 0xC),
        m.i32(e + 0x10),
        m.i32(e + 0x14),
    )?;
    m.set_u16(e + 0x78, s as u16);
    if s & 0xFFFF == NONE {
        // Out of every sector: halve the move up to 6 times, searching from the sector the step started in; if all
        // fail, back to that sector. Unlike the player's step there is no pull-back. The body position follows.
        let mut mv = sub(m.vec3(e + 0xC), start);
        let mut found = false;
        for _ in 0..6 {
            mv = crate::math::scale(mv, 0x800);
            m.set_u16(W_QUERY_SECTOR, old_sector);
            m.set_u16(e + 0x78, old_sector);
            m.set_vec3(e + 0xC, crate::math::add(mv, start));
            let s = world::find_sector(m, old_sector as u32, m.i32(e + 0xC), m.i32(e + 0x10), m.i32(e + 0x14))?;
            m.set_u16(e + 0x78, s as u16);
            if s & 0xFFFF != NONE {
                found = true;
                break;
            }
        }
        if !found && m.u16(e + 0x78) as u32 == NONE {
            m.set_u16(e + 0x78, old_sector);
        }
        m.set_vec3(p + 0xD0, crate::math::add(m.vec3(e + 0xC), offset));
    }
    if m.u16(e + 8) & 4 != 0 {
        crate::ram::walls::racers(sim, e, dt)?;
    }
    let m = &mut sim.mem;
    if m.u16(e + 0x4A) == 2 {
        m.set_i32(p + 0xAC, 0);
    } else {
        let v = route::progress(m, e, p);
        m.set_i32(p + 0xAC, v);
    }
    Ok(())
}

// The wingman's globals (`FUN_081410c0` sets them up; `docs/engine/ai.md`).
/// The entity the wingman works with (the player's, world `+0x3C`).
const WING_TARGET: u32 = 0x0300_619C;
/// A per-frame amount the wingman's timers count down by (unit not decoded; also the traffic knock-away timer's).
const FRAME_TICKS: u32 = 0x0300_5934;

/// `|lateral(e) - lateral(target)|` the way `FUN_08140ba8`/`FUN_08140a10` compute it (larger minus smaller).
fn lateral_gap(m: &Mem, e: u32, target: u32) -> i32 {
    let (a, b) = (route::lateral(m, e), route::lateral(m, target));
    if a.wrapping_sub(b) < 0 {
        b.wrapping_sub(a)
    } else {
        a.wrapping_sub(b)
    }
}

/// A PD-like response to a following error: `(now·0x2B + bias − previous·0x2A)` clamped, as a scale of 0x100.
fn follow_gain(now: i32, prev: i32, bias: i32, neg: (i32, i32, i32), pos: (i32, i32, i32, i32)) -> i32 {
    let v = now
        .wrapping_mul(0x2B)
        .wrapping_add(bias)
        .wrapping_add(prev.wrapping_mul(-0x2A));
    if v < 1 {
        let (off, max, den) = neg;
        -div((off - v).clamp(0, max) << 8, den)
    } else {
        let (off, max, mul, den) = pos;
        div((v - off).clamp(0, max).wrapping_mul(mul), den)
    }
}

/// `FUN_08140cf4`: the throttle of a computer car that is not a racer (the wingman), from the player's `lead`.
/// Attacker (0x030061F8 = 0) and drafter (1) differ once a command (0x030061E8) is running.
fn wingman_throttle(m: &mut Mem, e: u32, throttle: i32, lead: i32) -> i32 {
    let p = m.u32(e + 0x8C);
    let tp = m.u32(m.u32(WING_TARGET) + 0x8C);
    m.set_u32(0x0300_6048, m.u32(0x0300_61D8));
    let mut t = if m.i32(0x0300_61E8) == 0 {
        follow_into_branch(m, e);
        let left = m.i32(0x0300_61D8).wrapping_sub(m.i32(FRAME_TICKS));
        m.set_i32(0x0300_61D8, left);
        if left < 0 {
            m.set_u32(0x0300_61D8, 0);
            if m.i32(0x0300_61DC) != 0 {
                m.set_u32(0x0300_61E4, m.u32(0x0300_6188));
            }
        }
        let t = if m.i32(0x0300_6200) == 0 {
            let prev = m.i32(0x0300_61EC);
            m.set_i32(0x0300_61EC, lead);
            let g = follow_gain(
                lead,
                prev,
                0xFFFF_F600u32 as i32,
                (-0x10, 0x800, 0x7F0),
                (0x10, 0x400, 0x140, 0x3F0),
            );
            throttle.wrapping_mul(g + 0x100) >> 8
        } else {
            // `FUN_08140ba8`: keep the gap to the player.
            let gap = lateral_gap(m, e, m.u32(WING_TARGET));
            let prev = m.i32(0x0300_61FC);
            m.set_i32(0x0300_61FC, lead + 500);
            if gap > 0x200 {
                m.set_u32(0x0300_6200, 0);
            }
            let g = follow_gain(lead + 500, prev, 0x100, (-5, 0x10, 0xB), (1, 0x200, 0xC0, 0x1FF));
            (g + 0x100).wrapping_mul(throttle) >> 8
        };
        if m.i32(RACE_FRAMES) > 0xB4 && ((lead + 0x5FF) as u32) <= 0x9FE {
            // Keep out of the player's lane and its neighbours.
            let lane = m.i16(m.u32(m.u32(W_ENTITIES) + 0x8C) + 0xC0) as i32;
            let lanes = p + 0x4DA;
            m.set_u16(lanes.wrapping_add((lane * 2) as u32), 1);
            if lane > 0 {
                m.set_u16(lanes.wrapping_add(((lane - 1) * 2) as u32), 1);
            }
            if lane < 4 {
                m.set_u16(lanes.wrapping_add(((lane + 1) * 2) as u32), 1);
            }
        }
        if t < 1 {
            m.set_u32(p + 0x4B0, 0);
        }
        let near = m.i32(0x0300_61F8) != 1 || (m.i32(tp + 0x4C8) == 0 && ((lead + 0x1FFF) as u32) <= 0x1FFF + 199);
        m.set_u32(0x0300_61D4, near as u32);
        t
    } else if m.i32(0x0300_61F8) == 0 {
        attacker_command(m, e, throttle)
    } else {
        drafter_command(m, e, throttle, lead)
    };
    let c = m.i32(0x0300_6174) - 1;
    m.set_i32(0x0300_6174, c.max(0));
    if (m.i32(0x0300_61E8) == 0 && m.i32(0x0300_61D8) != 0) || m.i32(0x0300_61DC) == 0 {
        m.set_u32(0x0300_61D4, 0);
    }
    let count = m.u16(m.u32(W_SEGMENTS)) as i32;
    if m.u8(p + 0xC5) == 1 && count - 3 <= m.i16(e + 0x90) as i32 {
        t = if m.i16(e + 0x90) as i32 == count - 2 { -1 } else { 0 };
        m.set_u16(p + 0x4D6, 0);
        m.set_u32(0x0300_61D4, 0);
    }
    t
}

/// `FUN_08140c8c`: follow the player into a side segment it has taken, from the fork.
fn follow_into_branch(m: &mut Mem, e: u32) {
    let target = m.u32(WING_TARGET);
    if m.i16(target + 0x72) == 0 || m.i16(e + 0x72) != 0 {
        return;
    }
    let w = route::waypoint_at(m, m.u16(e + 0x72) as u32, m.i16(e + 0x90) as i32 + 1);
    if m.i16(w + 0xE) == 0 && m.i16(w + 0xC) == m.i16(target + 0x72) {
        m.set_u16(e + 0x72, m.u16(w + 0xC));
        m.set_u16(m.u32(e + 0x8C) + 0x4D6, 2);
        m.set_i16(e + 0x90, m.i16(w + 0xE) - 1);
    }
}

/// `FUN_081408c4`: the attacker's command: catch the car at `*0x03006178` (full throttle while engaged).
fn attacker_command(m: &mut Mem, e: u32, throttle: i32) -> i32 {
    let mut t = 0x9999;
    if m.i32(0x0300_61F0) == 0 {
        let other = m.u32(0x0300_6178);
        let d = gap_to(m, e, other);
        if ((d + 0xB3) as u32) < 0x9F && m.i16(e + 0x72) == m.i16(other + 0x72) {
            m.set_u32(0x0300_61F0, 1);
            m.set_u32(0x0300_6180, 6);
            m.set_u32(0x0300_6174, 0x12);
        }
        let prev = m.i32(0x0300_61FC);
        m.set_i32(0x0300_61FC, d + 100);
        let v = (d + 100)
            .wrapping_mul(0x2B)
            .wrapping_add(0x100)
            .wrapping_add(prev.wrapping_mul(-0x2A));
        let g = if v < 1 {
            0x40 - div((-5 - v).clamp(0, 0x10) << 8, 0xB)
        } else {
            div((v - 1).clamp(0, 0x100) << 6, 0xFF) + 0x100
        };
        t = throttle.wrapping_mul(g) >> 8;
        let left = m.i32(0x0300_61E4).wrapping_sub(m.i32(FRAME_TICKS));
        m.set_i32(0x0300_61E4, left);
        if left < 0 {
            m.set_u32(0x0300_61E4, 0);
            m.set_u32(0x0300_61E8, 0);
        }
    } else {
        let c = m.i32(0x0300_6180) - 1;
        m.set_i32(0x0300_6180, c);
        if c == 0 {
            m.set_u32(0x0300_61F0, 0);
        }
    }
    m.set_u32(0x0300_61D4, 0);
    t
}

/// `FUN_08140a10`: the drafter's command: run just ahead of the player in its lane, filling the player's nitro
/// tank (`+0x4C8`, by 0x2AAA per step up to 0x50000).
fn drafter_command(m: &mut Mem, e: u32, throttle: i32, lead: i32) -> i32 {
    let target = m.u32(WING_TARGET);
    let tp = m.u32(target + 0x8C);
    if lead < -0x200 {
        m.set_u16(m.u32(e + 0x8C) + 0x4D6, 2);
    }
    let gap = lateral_gap(m, e, target);
    if lead < -0x40 && -0x380 < lead && gap < 0x100 {
        let tank = m.i32(tp + 0x4C8).wrapping_add(0x2AAA);
        m.set_i32(tp + 0x4C8, tank);
        if tank > 0x5_0000 {
            m.set_i32(tp + 0x4C8, 0x5_0000);
            for a in [0x0300_61E4, 0x0300_61E8, 0x0300_618C] {
                m.set_u32(a, 0);
            }
            m.set_u32(0x0300_6200, 1);
        }
    }
    let prev = m.i32(0x0300_61FC);
    m.set_i32(0x0300_61FC, lead + 500);
    let v = (lead + 0x2F4).wrapping_add((lead + 500).wrapping_sub(prev).wrapping_mul(0x2A));
    let g = if v < 1 {
        -div((-5 - v).clamp(0, 0x10) << 8, 0xB)
    } else {
        div((v - 1).clamp(0, 0x200).wrapping_mul(0xC0), 0x1FF)
    };
    let left = m.i32(0x0300_61E4).wrapping_sub(m.i32(FRAME_TICKS));
    m.set_i32(0x0300_61E4, left);
    if left < 0 {
        for a in [0x0300_61E4, 0x0300_61E8, 0x0300_618C] {
            m.set_u32(a, 0);
        }
        m.set_u32(0x0300_6200, 1);
    }
    m.set_u32(0x0300_61D4, 0);
    (g + 0x100).wrapping_mul(throttle) >> 8
}

/// `FUN_081412ac`: steering of computer cars that are not racers (the wingman).
fn traffic_steer(m: &Mem, e: u32, steer: i32) -> i32 {
    if m.i32(0x0300_61F0) == 0 {
        return steer;
    }
    let other = m.u32(m.u32(0x0300_6178) + 0x8C);
    if m.i16(other + 0xC0) < m.i16(m.u32(e + 0x8C) + 0xC0) {
        -0x400
    } else {
        0x400
    }
}

/// `FUN_0813c240` (its value is passed through): mark the lanes (`+0x4DA` + 2·lane) blocked by traffic cars
/// just ahead on the main route and, after 0xB4 race frames, by racers close ahead.
fn mark_blocked_lanes(m: &mut Mem, e: u32) {
    let p = m.u32(e + 0x8C);
    if m.u16(e + 0x72) == 0 {
        for k in 0..8 {
            let t = m.u32(0x0300_6270 + 4 * k);
            if t == 0 {
                continue;
            }
            let d = m.i16(t + 0x9C) as i32 - m.i16(e + 0x90) as i32;
            let lim = if m.i16(t + 0x9A) == 1 { 3 } else { 6 };
            if d < lim && d >= 0 && m.i16(t + 0x4A) == 1 {
                m.set_u16(p + 0x4DE, 1);
                if m.i16(t + 0x9A) == 1 {
                    m.set_u16(p + 0x4E0, 1);
                    m.set_u16(p + 0x4D8, m.u16(p + 0x4D8) | 0x100);
                } else {
                    m.set_u16(p + 0x4DC, 1);
                    m.set_u16(p + 0x4D8, m.u16(p + 0x4D8) | 0x200);
                }
            }
        }
    }
    if m.i32(RACE_FRAMES) > 0xB4 {
        let mut o = m.u32(W_ENTITIES);
        loop {
            if o != e {
                let q = m.u32(o + 0x8C);
                let d = m.i32(q + 0xAC).wrapping_sub(m.i32(p + 0xAC));
                let s = (m.i32(p + 0xA0).wrapping_sub(m.i32(q + 0xA0)) >> 8).max(0);
                if d < s + 0x400 && d > -0x80 {
                    m.set_u16((p + 0x4DA).wrapping_add((m.i16(q + 0xC0) as i32 * 2) as u32), 1);
                }
            }
            if m.u16(o) as u32 >= m.u32(RACERS) {
                break;
            }
            o += 0xA4;
        }
    }
}

/// `FUN_081489dc`: the opponent's four wheels on the ground. Like the player's (`contact::wheels`) but every
/// wheel stands on the car's own sector, the front wheels always turn by `+0x20 >> 8`, the floor normal is taken
/// as it is, and no slip is recorded. Returns the number of wheels on the ground.
fn wheels(m: &mut Mem, e: u32, dt: i32) -> i32 {
    let p = m.u32(e + 0x8C);
    let mut grounded = 0;
    let sector = m.u16(e + 0x78) as u32;
    let h = floor_height(m, sector, m.i32(p + 0xD0) >> 8, m.i32(p + 0xD8) >> 8);
    if m.i32(p + 0xD4) - h > 0x800 && m.i32(p + 0x120) > 0 {
        let my = m.i32(p + 0xFC) - m.i32(p + 0x120);
        m.set_i32(p + 0xFC, my);
        m.set_i32(p + 0x120, m.i32(p + 0xCC).wrapping_mul(my) >> 12);
    }
    let (rear_x, rear_z) = (m.i32(p + 0x128), m.i32(p + 0x130));
    let (c, s) = (cos(m, m.i32(p + 0x20) >> 8), sin(m, m.i32(p + 0x20) >> 8));
    let mut fx = ((c >> 2).wrapping_mul(m.i32(p + 0x128)) + m.i32(p + 0x140).wrapping_mul(-s >> 2)) >> 12;
    let mut fz = ((c >> 2).wrapping_mul(m.i32(p + 0x130)) + m.i32(p + 0x148).wrapping_mul(-s >> 2)) >> 12;
    m.set_i32(p + 0x3DC, 0);
    let mul12 = |a: i32, b: i32| a.wrapping_mul(b) >> 12;
    for i in 0..4 {
        let w = p + WHEELS + WHEEL_SIZE * i;
        let local = m.vec3(w + 0x48);
        let r = |k: u32| m.i32(p + 0x128 + 4 * k);
        let pos = [
            mul12(local[0], r(0)) + mul12(r(3), local[1]) + mul12(r(6), local[2]),
            mul12(local[0], r(1)) + mul12(r(4), local[1]) + mul12(r(7), local[2]),
            mul12(local[0], r(2)) + mul12(r(5), local[1]) + mul12(r(8), local[2]),
        ];
        m.set_vec3(w + 0x18, pos);
        if i == 2 {
            fx = rear_x;
            fz = rear_z;
        }
        let fs = floor_sector(m, sector);
        m.set_i32(W_QUERY, pos[0].wrapping_add(m.i32(p + 0xD0)) >> 8);
        m.set_i32(W_QUERY + 4, pos[1].wrapping_add(m.i32(p + 0xD4)) >> 8);
        m.set_i32(W_QUERY + 8, pos[2].wrapping_add(m.i32(p + 0xD8)) >> 8);
        m.set_u32(w + 0x90, sector);
        let floor = floor_height(m, sector, m.i32(W_QUERY), m.i32(W_QUERY + 8));
        let pen = m.i32(p + 0xD4) + pos[1] + m.i32(p + 0x204) - floor;
        if pen > 0 {
            grounded += 1;
            let contact = [pos[0], pos[1] + (m.i32(p + 0x204) - pen), pos[2]];
            m.set_vec3(w + 0xC, contact);
            let omega = m.vec3(p + 0x158);
            let mut vx = m.i32(p + 0x11C) + (mul12(omega[2], contact[1]) - mul12(omega[1], contact[2]));
            let vy = m.i32(p + 0x120) + (mul12(omega[0], contact[2]) - mul12(omega[2], contact[0]));
            let mut vz = m.i32(p + 0x124) + (mul12(omega[1], contact[0]) - mul12(omega[0], contact[1]));
            let surface = (m.u16(fs + 8) as u32).min(7);
            let damping = if vy < 1 { m.i32(w + 0x6C) - 10 } else { m.i32(w + 0x6C) };
            let normal = mul12(m.i16(fs + 0x14) as i32, vx)
                + mul12(m.i16(fs + 0x16) as i32, vy)
                + mul12(m.i16(fs + 0x18) as i32, vz);
            let load = ((pen.wrapping_mul(m.i32(w + 0x68)) >> 8) + (normal.wrapping_mul(damping) >> 7)).max(0);
            let (mut ax, mut az) = (0, 0);
            let spin = m.i32(w + 0x64);
            vx += (spin >> 7).wrapping_mul(fz) >> 12;
            vz += (-fx).wrapping_mul(spin >> 7) >> 12;
            let slip2 = vx.wrapping_mul(vx).wrapping_add(vz.wrapping_mul(vz));
            if slip2 != 0 {
                let grip = ((m.i32(w + 0x80).wrapping_mul(load) >> 8).wrapping_mul(m.i32(SURFACE_GRIP + surface * 4))
                    >> 8)
                    .min(0x4000);
                if grip.wrapping_mul(grip) < slip2 {
                    let r = recip(m, isqrt(slip2 as u32));
                    let (ux, uz) = (r.wrapping_mul(vx) >> 12, r.wrapping_mul(vz) >> 12);
                    ax = (-(grip >> 4)).wrapping_mul(ux) >> 12;
                    az = (-(grip >> 4)).wrapping_mul(uz) >> 12;
                } else {
                    ax = -vx >> 4;
                    az = -vz >> 4;
                }
                m.set_i32(w + 0x64, spin + (dt.wrapping_mul(mul12(ax, fz) + mul12(az, -fx)) >> 1));
            }
            let ix = dt.wrapping_mul(ax) >> 12;
            let iy = dt.wrapping_mul(-load) >> 12;
            let iz = dt.wrapping_mul(az) >> 12;
            m.set_i32(p + 0xF8, m.i32(p + 0xF8) + ix);
            m.set_i32(p + 0xFC, m.i32(p + 0xFC) + iy);
            m.set_i32(p + 0x100, m.i32(p + 0x100) + iz);
            let [cx, cy, cz] = contact;
            m.set_i32(p + 0x110, m.i32(p + 0x110) - (mul12(iz, cy) - mul12(iy, cz)));
            m.set_i32(p + 0x114, m.i32(p + 0x114) - (mul12(ix, cz) - mul12(iz, cx)));
            m.set_i32(p + 0x118, m.i32(p + 0x118) - (mul12(iy, cx) - mul12(ix, cy)));
        }
        m.set_i32(p + 0x3DC, m.i32(p + 0x3DC).wrapping_add(m.i32(w + 0x64)));
    }
    if grounded == 0 {
        m.set_i16(p + 0x4E6, m.i16(p + 0x4E6) + 1);
    } else {
        m.set_i16(p + 0x4E6, 0);
    }
    body::update_velocities(m, p + 0xC8);
    grounded
}
