//! The opponents on typed state: entity handler 0x29 (`FUN_0814a2a0`), their setup (`FUN_0814a390`), the
//! racing-line steering (`FUN_0814d078`) and the AI's driving model (`FUN_0813c5a8`), which drives the car body with
//! the shared rigid body, walls and car-to-car code but its own wheel contact (`FUN_081489dc`) and a speed curve
//! built at setup (`FUN_0813c394`). Runs on a [`CarWorld`] (`carworld.rs`); `ram.rs` is the adapter.
//! `docs/engine/ai.md` documents the fields and the algorithms.
//!
//! Physics struct fields the AI uses beyond the player's (`state::Car`): `target_heading`, `curve` (steering
//! look-ahead term), `launch` (distance past the next waypoint line), `stuck`, `lane_timer`, `u_4d6` (preferred
//! lane), `blocked` (lanes), `u_4f0` (follow timer) and `follow` (the entity followed), `hunter_countdown`,
//! `boost_timer`.

use crate::car;
use crate::carworld::{CarWorld, NONE};
use crate::data::Curve;
use crate::math::{angle_diff, atan2, cos, div, dot, isqrt, mat_mul, mul12, mul64, recip, scale, sin, sub, udiv};
use crate::route;
use crate::state::EntityRef;
use crate::{Result, Unported};

macro_rules! car {
    ($w:ident, $i:expr) => {
        $w.slots[$i].c
    };
}
macro_rules! ent {
    ($w:ident, $i:expr) => {
        $w.slots[$i].e
    };
}

/// The `opponent_effects` call (`FUN_0814e628`): the car's rear light and exhaust sprites in the 2D layer
/// (sprite pool `*0x03000058`). Rendering, so the simulation reports the call instead of making it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Effects {
    pub entity: u32,
    pub heading: u32,
    pub view: u32,
    pub size: i32,
}

/// A racer whose index is above the opponent count (the wingman).
fn is_non_racer(w: &CarWorld, i: usize) -> bool {
    ent!(w, i).index as u32 > w.g.opponents
}

/// `FUN_0814a2a0`: entity handler 0x29. The adapter has unlinked the entity from its sector list (states 1 and 2)
/// and links it again after.
pub fn handler(w: &mut CarWorld, i: usize) -> Result<Option<Effects>> {
    let e = &ent!(w, i);
    let visible = e.state & 4 != 0 && e.flags & 4 != 0;
    let (index, state) = (e.index as u32, e.race_state);
    if state.wrapping_sub(1) < 2 {
        let phase = w.g.phase;
        if phase > 1 && phase != 9 && phase != 4 {
            step(w, i)?;
        }
        let e = &mut ent!(w, i);
        e.flags &= 0xFFFE;
        if index == w.g.player {
            // The player's rim redraw (`rim_side_visible`, `draw_decal_on_atlas`) is rendering.
            e.flags |= 1;
        }
    } else if state == 0 {
        init(w, i)?;
    }
    if !visible {
        return Ok(None);
    }
    let e = &ent!(w, i);
    Ok(Some(Effects {
        entity: index,
        heading: (e.heading >> 8) as u32 & 0x3FFF,
        view: 0x3FFFu32.wrapping_sub(w.matrix_yaw as u32),
        size: if e.speed < 0x80 { 0x40 } else { 0x20 },
    }))
}

/// `FUN_0814a390`: the opponent's setup (state 0). The adapter has allocated the physics struct.
fn init(w: &mut CarWorld, i: usize) -> Result<()> {
    let (rom, data) = (w.rom, w.data);
    let h = &data.car.handling[ent!(w, i).car as usize];
    let player_upgrades = car!(w, 0).upgrades;
    let non_racer = is_non_racer(w, i);
    let e = &mut ent!(w, i);
    e.segment = 0;
    e.start_sector = 0;
    if w.g.mode == 0xE {
        w.g.gravity = 0x2CC;
    }
    w.g.u_614c = 0;
    // `FUN_0814f874`: on the floor (at the entity's stale position), no matrix slot.
    let (sector, x, z) = (e.sector as u32, e.pos[0] >> 8, e.pos[2] >> 8);
    let floor = w.floor_height(sector, x, z);
    let e = &mut ent!(w, i);
    e.pos[1] = floor;
    e.slot = 0xFF;
    e.extra_model = 0;
    e.race_state = 1;
    e.u_94 = e.u_76;
    e.waypoint = 0;
    e.traffic_mode = 0;
    e.u_4c = 0;
    e.dir_x = 0;
    e.u_1c = 0;
    e.dir_z = 0;
    let grid = w.grid[e.index as usize];
    e.pos[0] = grid[0];
    e.pos[2] = grid[1];
    let heading = div((e.heading >> 8) << 15, 0xA30);
    let lane_roll = w.rand();
    let c = &mut car!(w, i);
    c.heading = heading;
    c.heading_sign = heading >> 31;
    c.u_018 = 0;
    c.u_01c = 0;
    c.gear = 1;
    let mass = h[0];
    c.inertia[0] = div(mass.wrapping_mul(h[3]).wrapping_mul(10), h[4]);
    c.inertia[1] = div(mass.wrapping_mul(h[2]).wrapping_mul(10), h[4]);
    c.points_on_floor = 4;
    let laps = w.g.laps as u8;
    c.laps_left = laps as i8;
    c.laps = laps;
    c.lane_timer = (lane_roll >> 7) as u16 & 0xF;
    let c = &mut car!(w, i);
    c.route_flags = 1;
    c.tipped = 0;
    c.airborne = 0;
    c.blocked = [0; 5];
    c.hunter_life = 0;
    c.wrong_way = 0;
    c.stationary = 0;
    c.u_4f0 = 0;
    let r = w.rand();
    let c = &mut car!(w, i);
    c.hunter_countdown = (r as u16 & 0x3F) + 0x40;
    c.boost_timer = 0;
    c.u_4b8 = 0x100;
    let mode = w.g.mode;
    if mode != 0xF {
        let lane = start_lane(w, i);
        car!(w, i).lane = lane;
        car!(w, i).u_4d6 = lane;
    }
    if mode == 0xF || mode == 0xE {
        car!(w, i).lane = 2;
    }
    // `FUN_0814dd24`: the floor under four points around the car (the wheels) seeds the suspension state.
    let corners = [[-0x20, 0x55], [0x20, 0x55], [-0x20, -0x2A], [0x20, -0x2A]];
    let e = &ent!(w, i);
    let (c, s) = (cos(rom, e.heading >> 8), sin(rom, e.heading >> 8));
    let (pos, sector) = (e.pos, e.sector);
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
        w.query.pos = [pos[0].wrapping_add(ix) >> 8, pos[1] >> 8, pos[2].wrapping_add(iz) >> 8];
        w.query.sector = sector;
        let mut s = w.near_query();
        if s == NONE {
            s = w.query.sector as u32;
        }
        if s == NONE {
            // The point's height would be the caller's uninitialised stack word.
            return Err(Unported("FUN_0814dd24 with the car outside every sector"));
        }
        let y = w.floor_height(s, w.query.pos[0], w.query.pos[2]);
        car!(w, i).spring[k] = y;
        car!(w, i).point_height[k] = y;
    }
    let upgrades: [i32; 10] = if !non_racer {
        if w.g.career == 0 {
            player_upgrades
        } else {
            [udiv(w.g.career_level as u32, 10) as i32 - 1; 10]
        }
    } else {
        let mut u = (w.g.wingman as u32).wrapping_sub(1);
        if w.g.career != 0 {
            u = (u & 1) + 6;
        }
        // The game's copy loop never advances its source: all ten levels are the wingman's one entry.
        [data.ai.upgrades.get(u as usize).copied().unwrap_or(0); 10]
    };
    car!(w, i).upgrades = upgrades;
    crate::init::nitro_setup(w, i);
    crate::init::setup_handling(w, i, h);
    speed_curve(w, i);
    crate::init::setup_handling(w, i, h);
    wingman_setup(w);
    for wheel in &mut car!(w, i).wheels {
        wheel.base_grip = wheel.base_grip.wrapping_mul(0x1400) >> 12;
    }
    crate::init::race_start_setup(w, i);
    ent!(w, i).flags |= 0x20;
    if ent!(w, i).index as u32 == w.g.player {
        return Err(Unported("the player's car on the opponent handler (unpack_decal)"));
    }
    Ok(())
}

/// `FUN_0814059c`: the lane (0..4) the car starts in, from its offset across the route.
fn start_lane(w: &CarWorld, i: usize) -> u16 {
    let v = route::lateral(w, i);
    let r = if v + 0x80 < 0 { v + 0x17F } else { v + 0x80 };
    ((r >> 8) + 2).clamp(0, 4) as u16
}

/// `FUN_081410c0`: the wingman's globals for the race.
fn wingman_setup(w: &mut CarWorld) {
    let g = &mut w.g;
    g.u_61ec = 0;
    g.wingman_running = 0;
    g.wingman_cooldown = 0;
    g.u_61d4 = 0;
    g.u_6200 = 0;
    g.wingman_car = EntityRef::to(0);
    g.wingman_attacker = 0;
    g.u_618c = 0;
    g.u_61f0 = 0;
    g.u_6174 = 0;
    let u = (g.wingman as u32).wrapping_sub(1);
    if u < 0xC {
        g.wingman_attacker = (u & 1) as i32;
        g.wingman_commands = w.data.ai.wingman_gap[u as usize];
        g.u_6188 = 0x7_8000;
    }
    g.u_61e4 = g.u_6188;
}

/// `FUN_0813c394`: the drive force the AI can use at 20 wheel speeds, a curve in the profile (`curve_*`: count
/// 0x15, x from 0 to `curve_x1`), found by running the gearbox at each speed.
fn speed_curve(w: &mut CarWorld, i: usize) {
    let (rom, data) = (w.rom, w.data);
    let h = &data.car.handling[ent!(w, i).car as usize];
    w.profile.curve_x0 = 0;
    w.profile.curve_count = 0x15;
    let top = recip(rom, car!(w, i).word(0x408 + 4 * h[21]));
    let span = car!(w, i).max_rpm.wrapping_mul(top).wrapping_mul(0x180);
    w.profile.curve_x1 = if span < 0 { span + 0xFF } else { span } >> 8;
    let step = div(span, 0x14);
    for k in 0..0x14i32 {
        w.g.phase = 1;
        car!(w, i).gear = 1;
        let mut ratio = 0;
        for _ in 0..7 {
            let c = &mut car!(w, i);
            c.gearbox_pause = 0;
            ratio = c.word(0x408 + 4 * c.gear);
            c.revs = div((mul64(k.wrapping_mul(step), ratio) >> 30) as i32, 6);
            car::auto_shift(w, i);
        }
        w.g.phase = 9;
        let c = &mut car!(w, i);
        if c.max_rpm < c.revs {
            c.revs = c.max_rpm;
        }
        if c.revs < h[26] {
            c.revs = h[26];
        }
        let rpm = c.revs;
        let t = car::torque(c, rpm);
        let drive = mul64(t.wrapping_mul(0x9999), ratio) >> 15;
        let drive = (drive.wrapping_mul(c.torque_scale as i64) >> 15) as i32;
        w.profile.curve[k as usize] = (drive.wrapping_add(rpm.wrapping_mul(-0x10)) >> 8).wrapping_mul(0x90);
    }
    w.profile.curve[0x14] = 0xFFF6_0000u32 as i32;
    let c = &mut car!(w, i);
    c.gear = 1;
    c.revs = h[26];
}

/// The drive-force curve (the game's evaluation reads the word after the last value: the profile's `distance`).
fn speed_at(w: &CarWorld, x: i32) -> i32 {
    let p = &w.profile;
    let mut ys = p.curve.to_vec();
    ys.push(p.distance);
    Curve {
        x0: p.curve_x0,
        x1: p.curve_x1,
        ys,
    }
    .eval(x)
}

/// `FUN_081401d4`: how far entity `target` is ahead of car `i` along the lap (wrapped into -lap/4 .. 3·lap/4).
fn gap_to(w: &CarWorld, i: usize, target: usize) -> i32 {
    let lap = w.route.line.lap_length();
    let mut d = car!(w, target).progress.wrapping_sub(car!(w, i).progress);
    if d < -lap >> 2 {
        d += lap;
    }
    if (lap >> 2) + (lap >> 1) < d {
        d -= lap;
    }
    d
}

/// `FUN_081400ec`: race progress over all laps.
fn race_progress(w: &CarWorld, i: usize) -> i32 {
    let c = &car!(w, i);
    if w.g.circuit == 0 {
        c.progress
    } else {
        w.route
            .line
            .lap_length()
            .wrapping_mul(w.g.laps - c.laps_left as i32)
            .wrapping_add(c.progress)
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
/// by the route's side-segment table (per entry: segment, its sectors). On the main route: the side segment
/// whose sectors include the car's, at its nearer end. On a side segment it has left: back to the main route
/// where the segment joins it. Like `route::track_segment`, without marking segments visited and without
/// switching from one side segment to another.
fn resync_segment(w: &mut CarWorld, i: usize) {
    let Some(Some(list)) = w.data.car.side_segments.get(w.g.route_index as usize) else {
        return;
    };
    let e = &ent!(w, i);
    let (sector, cur) = (e.sector as u32, e.segment as u32);
    let (ex, ez) = (e.pos[0] >> 8, e.pos[2] >> 8);
    let d2 = |wp: usize| {
        let p = &w.route.line.points[wp];
        let dx = (ex - p.x) >> 4;
        let dz = (ez - p.z) >> 4;
        dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz))
    };
    let seg_count = |seg: u32| w.route.line.sections[seg as usize].count as i32;
    for (seg, sectors) in list {
        if cur == 0 {
            if sectors.contains(&sector) {
                let n = seg_count(*seg);
                let near_first = d2(route::waypoint(w, *seg, 0)) < d2(route::waypoint(w, *seg, n - 1));
                let e = &mut ent!(w, i);
                e.segment = *seg as u16;
                e.waypoint = if near_first { 0 } else { n - 1 } as i16;
                return;
            }
        } else if *seg == cur {
            if sectors.contains(&sector) {
                return;
            }
            let last = route::waypoint(w, cur, seg_count(cur) - 1);
            let v = if d2(route::waypoint(w, cur, 0)) < d2(last) {
                w.route.back[cur as usize] as u16
            } else {
                w.route.line.points[last].link_index
            };
            let e = &mut ent!(w, i);
            e.waypoint = v as i16;
            e.segment = 0;
            return;
        }
    }
}

fn xz(w: &CarWorld, point: usize) -> [i32; 2] {
    let p = &w.route.line.points[point];
    [p.x, p.z]
}

/// `FUN_0814d078`: follow the racing line (or the entity at `follow`): advance the waypoint (`career::RacingLine::
/// ai_advance`, D19), maybe take a branch, aim at a point ahead on the line in the car's lane
/// (`target_heading`, `curve`), then drive.
fn step(w: &mut CarWorld, i: usize) -> Result<()> {
    let rom = w.rom;
    let mut ahead = 0xC80;
    let dt = w.g.dt;
    car::nitro(&mut car!(w, i), &mut w.g, dt);
    let seg = |w: &CarWorld| ent!(w, i).segment as u32;
    let mut wp = ent!(w, i).waypoint as i32;
    let mut a = route::waypoint_at(w, seg(w), wp);
    let mut b = route::waypoint_at(w, seg(w), wp + 1);
    if car!(w, i).stuck == 0x32 {
        resync_segment(w, i);
        wp = ent!(w, i).waypoint as i32;
        a = route::waypoint_at(w, seg(w), wp);
        b = route::waypoint_at(w, seg(w), wp + 1);
    }
    let stuck = car!(w, i).stuck;
    if stuck > 0x96 {
        let player_wp = w.slots[w.camera_player as usize].e.waypoint as i32;
        if stuck > 0xFA || (ent!(w, i).flags & 4 == 0 && player_wp != wp && player_wp != wp + 1) {
            car!(w, i).stuck = 0;
            car::put_back_on_road(w, i, b);
        }
    }
    if ent!(w, i).race_state == 2 {
        let n = w.route.line.sections[seg(w) as usize].count as i32;
        wp = n - 1;
        a = route::waypoint_at(w, seg(w), n - 1);
        b = route::waypoint_at(w, seg(w), n);
        car!(w, i).stuck = 0;
    }
    let c = &mut car!(w, i);
    let mut look = (c.speed >> 8) - 200;
    let turn = c.curve;
    if turn.wrapping_abs() > 0x1000 {
        let lane = c.lane as i16 as i32;
        let k = if turn < 1 { 4 - lane } else { lane };
        ahead = w.data.ai.lane_lookahead.get(k as usize).copied().unwrap_or(0);
    }
    let c = &mut car!(w, i);
    if c.route_flags & 0x300 != 0 {
        ahead = 0x708;
        c.route_flags &= 0xFCFF;
    }
    if ahead < look {
        look = ahead;
    }
    if look <= 0x63F {
        look = 0x640;
    }
    if w.g.difficulty > 2 {
        return Err(Unported(
            "branch odds for a difficulty above 2 (the game reads its stack)",
        ));
    }
    // Past the next crossing line the car moves on (D19: the one copy of the advance, in the formats crate).
    let advanced = {
        let mut race = w.race();
        let mut r = w.slots[i].racer();
        let advanced = w.route.line.ai_advance(
            rom,
            (&w.route.planes, &w.route.back),
            &mut race,
            &mut r,
            wp,
            &w.g.visited,
        );
        w.slots[i].set_racer(&r);
        w.set_race(&race);
        advanced
    };
    if advanced {
        wp = ent!(w, i).waypoint as i32;
        a = route::waypoint_at(w, seg(w), wp);
        b = route::waypoint_at(w, seg(w), wp + 1);
        route::lap(w, i);
    }
    let following = car!(w, i).u_4f0 != 0 && {
        let target = w.entity_of(car!(w, i).follow);
        gap_to(w, i, target) >= 0
    };
    let (ex, ez) = (ent!(w, i).pos[0] >> 8, ent!(w, i).pos[2] >> 8);
    if following {
        let target = w.entity_of(car!(w, i).follow);
        let t = &w.slots[target].e;
        let h = w.atan2_fast((t.pos[0] >> 8) - ex, (t.pos[2] >> 8) - ez);
        let c = &mut car!(w, i);
        c.target_heading = h;
        c.curve = 0;
    } else {
        let (pa, pb) = (xz(w, a), xz(w, b));
        let near = nearest_on_line(pa, pb, ex, ez);
        let (mut dx, mut dz) = (pb[0].wrapping_sub(pa[0]), pb[1].wrapping_sub(pa[1]));
        let dir = w.atan2_fast(dz, dx);
        let reach = look * 0x160 >> 8;
        let mut tx = near[0] + (reach.wrapping_mul(cos(rom, dir)) >> 14);
        let mut tz = near[1] + (reach.wrapping_mul(sin(rom, dir)) >> 14);
        let c2 = route::waypoint_at(w, seg(w), wp + 2);
        let pc = xz(w, c2);
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
            let dir2 = w.atan2_fast(dz, dx);
            tx = pb[0] + (r.wrapping_mul(cos(rom, dir2)) >> 14);
            tz = pb[1] + (r.wrapping_mul(sin(rom, dir2)) >> 14);
            let mid2 = [(pb[0] + pc[0]) >> 1, (pb[1] + pc[1]) >> 1];
            if d2(pb[0], pb[1], mid2) <= d2(tx, tz, mid2) {
                (tx, tz) = (pc[0], pc[1]);
            }
        }
        let next_dir = w.atan2_fast(pc[1].wrapping_sub(pb[1]), pc[0].wrapping_sub(pb[0]));
        car!(w, i).curve = angle_diff(dir, next_dir) * 0x14 >> 4;
        let lane = car!(w, i).lane as i16 as i32 - 2;
        let off = if (w.g.steps as i32) < 0x32 {
            lane << 9
        } else {
            lane << 8
        };
        if off != 0 {
            let side = w.atan2_fast(dx.wrapping_neg(), dz);
            tx += off.wrapping_mul(cos(rom, side)) >> 14;
            tz += off.wrapping_mul(sin(rom, side)) >> 14;
        }
        car!(w, i).target_heading = w.atan2_fast(tx - ex, tz - ez);
    }
    let dt = w.g.dt;
    drive(w, i, dt)
}

/// `FUN_0813c5a8(world, e, 0, 1, frame_time)`: the AI's driving: steer towards `target_heading`, choose throttle
/// and brake (catch-up, lane changes, boost), spin the wheels, then the body step shared with the player's car.
fn drive(w: &mut CarWorld, i: usize, frame_time: i32) -> Result<()> {
    let (rom, data) = (w.rom, w.data);
    let old_sector = ent!(w, i).sector;
    let mut boost = false;
    let mut accelerate = false;
    let dt = recip(rom, frame_time << 8).min(0xC00);
    let non_racer = is_non_racer(w, i);
    let c = &car!(w, i);
    if c.tipped > 100 && c.airborne == 0 && c.speed <= 0x7FFF {
        let e = &ent!(w, i);
        let wp = route::waypoint_at(w, e.segment as u32, e.waypoint as i32);
        car::put_back_on_road(w, i, wp);
        car!(w, i).tipped = 0;
    }
    let c = &mut car!(w, i);
    for wheel in &mut c.wheels {
        wheel.grip = wheel.base_grip.wrapping_mul(c.u_4b8) >> 8;
    }
    if c.u_4b8 < 0x100 {
        c.u_4b8 += 4;
    }
    let heading = atan2(c.body.rot[6] >> 4, c.body.rot[8] >> 4);
    c.heading = heading;
    c.heading_sign = heading >> 31;
    ent!(w, i).heading = heading << 8;
    let behind = car!(w, i).u_4f0 != 0 && {
        let target = w.entity_of(car!(w, i).follow);
        gap_to(w, i, target) < 0
    };
    let c = &mut car!(w, i);
    if behind {
        c.brake = 0x9999;
        c.stuck = 0;
    } else {
        c.brake = 0;
    }
    let mut h = ent!(w, i).heading >> 8;
    if h < 0 {
        h += 0x4000;
    }
    h &= 0x3FFF;
    let c = &mut car!(w, i);
    let fwd = [c.body.rot[6], c.body.rot[7], c.body.rot[8]];
    let f = dot(c.body.vel, fwd);
    c.forward_speed = f.wrapping_mul(0x20);
    c.speed = if f.wrapping_mul(0x20) < 0 {
        f.wrapping_mul(-0x20)
    } else {
        f.wrapping_mul(0x20)
    };
    let mut steer = angle_diff(h, c.target_heading) << 6 >> 8;
    if non_racer {
        steer = traffic_steer(w, i, steer);
    }
    let c = &mut car!(w, i);
    let speed = c.speed;
    if speed < 0x2_0001 {
        c.stuck += 1;
        accelerate = true;
    } else {
        // Brake into sharp curves: scrub momentum and set the brake.
        let v = c.launch + 0x1_0000;
        if v > 0 {
            let t = c.curve;
            let at = t.wrapping_abs();
            if at > 0x400 {
                let limit = if at < 0x901 {
                    let wt = t.wrapping_mul(0xC0).wrapping_abs();
                    (v - wt.wrapping_add(0xFFED_D420u32 as i32)).wrapping_mul(0x11) >> 3
                } else {
                    (c.launch + 0x1C_0000 + at * -0x80).wrapping_mul(10) >> 4
                };
                if limit < speed {
                    let (k, shift) = if at < 0x901 {
                        ((at >> 6) + 0x40, 3)
                    } else {
                        ((at >> 2) + 0x80, 2)
                    };
                    let mom = c.body.momentum;
                    c.body.momentum = sub(mom, scale(mom, k));
                    c.brake = (c.speed - limit) >> shift;
                }
            }
        }
        if ent!(w, i).race_state != 2 {
            let c = &mut car!(w, i);
            if w.g.race_frames as i32 > 0x78 && (c.blocked[1] == 0 || c.blocked[2] == 0 || c.blocked[3] == 0) {
                c.blocked[0] = 1;
                c.blocked[4] = 1;
            }
            c.lane_timer = c.lane_timer.wrapping_add(1);
            if c.lane_timer > 300
                && let Some(b) = c.blocked.get_mut(c.lane as i16 as usize)
            {
                *b |= 2;
            }
            c.lane = c.u_4d6;
            let blocked = |c: &crate::state::Car, lane: u32| c.blocked.get(lane as usize).is_some_and(|&b| b != 0);
            if blocked(&car!(w, i), car!(w, i).lane as i16 as i32 as u32) {
                // Change lane: try ±1, ±2, ±3 from the current one, starting on a random side.
                let dir = if w.rand() & 1 != 0 { -1 } else { 1 };
                let mut d = dir;
                for _ in 1..4 {
                    let c = &mut car!(w, i);
                    let cur = c.lane as i16 as i32;
                    let up = (cur + d) as u32;
                    if up < 5 && !blocked(c, up) {
                        c.lane = up as u16;
                        c.u_4d6 = up as u16;
                        break;
                    }
                    let down = (cur - d) as u32;
                    if down < 5 && !blocked(c, down) {
                        c.lane = down as u16;
                        c.u_4d6 = down as u16;
                        break;
                    }
                    d += dir;
                }
                // NOT 1:1 (T1): the race time is read when the AI runs; the VBlank IRQ may have counted it
                // up since the frame began (docs/engine/ai.md).
                car!(w, i).lane_timer = (w.g.time & 0x1F) as u16;
            }
            accelerate = true;
        }
        if ent!(w, i).segment != 0 && !non_racer {
            car!(w, i).lane = 2;
        }
        let c = &mut car!(w, i);
        if c.contact & 0x10 == 0 {
            c.stuck = 0;
        } else {
            c.stuck += 1;
        }
    }
    let c = &mut car!(w, i);
    c.blocked = [0; 5];
    steer = steer.clamp(-0x1000, 0x1000);
    c.steering = steer.wrapping_mul(0x15E);
    if c.brake == 0 {
        accelerate = true;
    }
    if ent!(w, i).race_state == 2 {
        let index = ent!(w, i).index;
        let c = &mut car!(w, i);
        c.steering = 0;
        c.u_018 = 0;
        c.u_01c = 0;
        accelerate = false;
        c.brake = 0x1_9999;
        if c.route_flags & 8 != 0 {
            c.body.momentum = data.car.rest;
        }
        c.body.momentum[0] = c.body.momentum[0].wrapping_mul(0xFA) >> 8;
        c.body.momentum[2] = c.body.momentum[2].wrapping_mul(0xFA) >> 8;
        c.lane = index;
    }
    let c = &mut car!(w, i);
    c.wheel_angle = c.wheel_angle.wrapping_add(c.forward_speed);
    let start = w.g.u_6158 as u32;
    if start != 0 {
        let t = c.boost_timer;
        if t < 0 {
            c.boost_timer = t + 1;
        } else if c.curve.wrapping_abs() > 0x3FF {
            if t != 0 {
                // ARM `lsl` by 32 or more gives 0.
                let v = (t as i32)
                    .wrapping_sub(data.ai.boost_time[start as usize])
                    .checked_shl(data.ai.boost_shift[start as usize] & 0xFF)
                    .unwrap_or(0);
                c.boost_timer = v as i16;
            }
        } else {
            let (seg, wp) = (ent!(w, i).segment as u32, ent!(w, i).waypoint as i32);
            let row = |k: i32| {
                let (idx, s) = route::advance(w, seg, wp + k);
                w.route.planes[(idx + w.route.line.sections[s as usize].first as i32) as usize]
            };
            let (l1, l2) = (row(1), row(2));
            if l1[0].wrapping_mul(l2[0]).wrapping_add(l1[1].wrapping_mul(l2[1])) > 0xA000 {
                let reload = data.ai.boost_time[start as usize] as i16;
                let c = &mut car!(w, i);
                if c.nitro_on == 0 {
                    c.boost_timer = reload;
                } else {
                    let v = t.wrapping_sub(1);
                    c.boost_timer = v;
                    if v == 0 {
                        c.boost_timer = reload.wrapping_mul(-8);
                    }
                }
                boost = true;
            }
        }
    }
    let c = &mut car!(w, i);
    if c.laps_left == 1 && w.route.line.sections[0].count as i32 - 3 <= ent!(w, i).waypoint as i32 && start == 2 {
        boost = true;
    }
    let c = &mut car!(w, i);
    if !accelerate {
        c.throttle = 0;
    } else {
        if c.nitro_tank == 0 {
            c.nitro_on = 0;
        } else {
            c.nitro_on = boost as u8;
        }
        c.throttle = 0x9999;
        if c.speed <= 0x7FFF && c.steering.wrapping_abs() > 0x4_0000 {
            c.throttle = 0x2666;
        }
    }
    if c.torque_timer != 0 {
        c.torque_timer -= 1;
        c.steering = 0;
        c.throttle = 0;
    }
    if w.g.mode == 2 {
        return Err(Unported("hunter races: FUN_0813fdb0, hunter_life_tick"));
    }
    let lead = race_progress(w, w.g.player as usize).wrapping_sub(race_progress(w, i));
    let c = &car!(w, i);
    let mut throttle = if c.u_4f0 != 0 {
        c.throttle
    } else if non_racer {
        // The wingman's throttle, kept to 24 bits (`<< 8 >> 8`).
        let t = wingman_throttle(w, i, car!(w, i).throttle, lead) << 8 >> 8;
        mark_blocked_lanes(w, i);
        t
    } else {
        let mut t = c.throttle;
        if w.g.catch_up != 0 {
            t = if lead < 1 {
                let v = (-0xC0 - lead).clamp(0, 0x800);
                (0x100 - div(v, 0x1D)).wrapping_mul(t)
            } else {
                let v = (lead - 0xC0).clamp(0, 0x400);
                t.wrapping_mul(div(v, 0x34) + 0x100)
            } >> 8;
        }
        mark_blocked_lanes(w, i);
        t
    };
    let c = &mut car!(w, i);
    if w.g.mode == 2 && c.position == 1 {
        throttle >>= 1;
    }
    if c.nitro_on != 0 {
        throttle = (throttle as u32).wrapping_mul(c.nitro_torque as u32) as i32 >> 12;
    }
    let brake = if throttle < 0 {
        throttle = 0;
        0x1_9999
    } else {
        let x = (c.wheel_spin >> 8).wrapping_mul(0x109A) >> 8;
        throttle = throttle.wrapping_mul(speed_at(w, x) >> 15);
        car!(w, i).brake
    };
    let c = &mut car!(w, i);
    for wheel in &mut c.wheels {
        let push = ((wheel.drive >> 4).wrapping_mul(throttle >> 10) >> 8)
            .wrapping_mul(c.grid.wrapping_add(w.g.spin_bias))
            >> 8;
        let mut spin = wheel.spin.wrapping_add(push);
        wheel.spin = spin;
        if brake > 0 {
            if spin < 0 {
                spin += brake * 4;
                wheel.spin = spin.min(0);
            } else {
                spin -= brake * 4;
                wheel.spin = spin.max(0);
            }
        }
    }
    let (sector, pos) = (ent!(w, i).sector, ent!(w, i).pos);
    let s = w.find_sector(sector as u32, pos[0], pos[1], pos[2]);
    ent!(w, i).sector = if s & 0xFFFF == NONE { old_sector as u32 } else { s } as u16;
    let c = &mut car!(w, i);
    c.body.momentum[1] += dt * (w.g.gravity.wrapping_mul(c.body.mass) >> 12) >> 11;
    if c.body.rot[4] < 0xF21 {
        crate::contact::tipped(w, i, dt);
        let c = &mut car!(w, i);
        for wheel in &mut c.wheels {
            wheel.spin = 0;
        }
        c.tipped = c.tipped.wrapping_add(1);
    } else {
        wheels(w, i, dt);
        car!(w, i).tipped = 0;
    }
    let c = &mut car!(w, i);
    c.contact = 0;
    let rot = c.body.rot;
    let e = &ent!(w, i);
    let (x, z) = (
        e.pos[0].wrapping_add(rot[6] * 3) >> 8,
        e.pos[2].wrapping_add(rot[8] * 3) >> 8,
    );
    let sector = e.sector as u32;
    crate::walls::walls(w, i, x, z, sector, false);
    let start_pos = ent!(w, i).pos;
    let c = &mut car!(w, i);
    crate::body::integrate(rom, &mut c.body, dt << 1);
    let rot = c.body.rot;
    let offset = mat_mul([0, c.centre_of_mass[1], c.centre_of_mass[2]], &rot);
    let pos = sub(c.body.pos, offset);
    ent!(w, i).pos = pos;
    let s = w.find_sector(ent!(w, i).sector as u32, pos[0], pos[1], pos[2]);
    ent!(w, i).sector = s as u16;
    if s & 0xFFFF == NONE {
        // Out of every sector: halve the move up to 6 times, searching from the sector the step started in; if all
        // fail, back to that sector. Unlike the player's step there is no pull-back. The body position follows.
        let mut mv = sub(ent!(w, i).pos, start_pos);
        let mut found = false;
        for _ in 0..6 {
            mv = scale(mv, 0x800);
            w.query.sector = old_sector;
            ent!(w, i).sector = old_sector;
            let pos = crate::math::add(mv, start_pos);
            ent!(w, i).pos = pos;
            let s = w.find_sector(old_sector as u32, pos[0], pos[1], pos[2]);
            ent!(w, i).sector = s as u16;
            if s & 0xFFFF != NONE {
                found = true;
                break;
            }
        }
        if !found && ent!(w, i).sector as u32 == NONE {
            ent!(w, i).sector = old_sector;
        }
        let back = crate::math::add(ent!(w, i).pos, offset);
        car!(w, i).body.pos = back;
    }
    if ent!(w, i).state & 4 != 0 {
        crate::walls::racers(w, i, dt);
    }
    if ent!(w, i).race_state == 2 {
        car!(w, i).progress = 0;
    } else {
        let v = route::progress(w, i);
        car!(w, i).progress = v;
    }
    Ok(())
}

/// `|lateral(e) - lateral(target)|` the way `FUN_08140ba8`/`FUN_08140a10` compute it (larger minus smaller).
fn lateral_gap(w: &CarWorld, i: usize, target: usize) -> i32 {
    let (a, b) = (route::lateral(w, i), route::lateral(w, target));
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
/// Attacker (`wingman_attacker` = 0) and drafter (1) differ once a command (`wingman_running`) is running.
fn wingman_throttle(w: &mut CarWorld, i: usize, throttle: i32, lead: i32) -> i32 {
    let target = w.entity_of(w.g.wingman_car);
    w.g.u_6048 = w.g.wingman_cooldown as u32;
    let mut t = if w.g.wingman_running == 0 {
        follow_into_branch(w, i);
        let g = &mut w.g;
        let left = g.wingman_cooldown.wrapping_sub(g.frame_ticks);
        g.wingman_cooldown = left;
        if left < 0 {
            g.wingman_cooldown = 0;
            if g.wingman_commands != 0 {
                g.u_61e4 = g.u_6188;
            }
        }
        let t = if g.u_6200 == 0 {
            let prev = g.u_61ec;
            g.u_61ec = lead;
            let gain = follow_gain(
                lead,
                prev,
                0xFFFF_F600u32 as i32,
                (-0x10, 0x800, 0x7F0),
                (0x10, 0x400, 0x140, 0x3F0),
            );
            throttle.wrapping_mul(gain + 0x100) >> 8
        } else {
            // `FUN_08140ba8`: keep the gap to the player.
            let gap = lateral_gap(w, i, target);
            let g = &mut w.g;
            let prev = g.u_61fc as i32;
            g.u_61fc = (lead + 500) as u32;
            if gap > 0x200 {
                g.u_6200 = 0;
            }
            let gain = follow_gain(lead + 500, prev, 0x100, (-5, 0x10, 0xB), (1, 0x200, 0xC0, 0x1FF));
            (gain + 0x100).wrapping_mul(throttle) >> 8
        };
        if w.g.race_frames as i32 > 0xB4 && ((lead + 0x5FF) as u32) <= 0x9FE {
            // Keep out of the player's lane and its neighbours.
            let lane = car!(w, 0).lane as i16 as i32;
            let blocked = &mut car!(w, i).blocked;
            blocked[lane as usize] = 1;
            if lane > 0 {
                blocked[lane as usize - 1] = 1;
            }
            if lane < 4 {
                blocked[lane as usize + 1] = 1;
            }
        }
        if t < 1 {
            car!(w, i).stuck = 0;
        }
        let near =
            w.g.wingman_attacker != 1 || (car!(w, target).nitro_tank == 0 && ((lead + 0x1FFF) as u32) <= 0x1FFF + 199);
        w.g.u_61d4 = near as u32;
        t
    } else if w.g.wingman_attacker == 0 {
        attacker_command(w, i, throttle)
    } else {
        drafter_command(w, i, throttle, lead)
    };
    let g = &mut w.g;
    let c = g.u_6174 - 1;
    g.u_6174 = c.max(0);
    if (g.wingman_running == 0 && g.wingman_cooldown != 0) || g.wingman_commands == 0 {
        g.u_61d4 = 0;
    }
    let count = w.route.line.sections[0].count as i32;
    let waypoint = ent!(w, i).waypoint as i32;
    if car!(w, i).laps_left == 1 && count - 3 <= waypoint {
        t = if waypoint == count - 2 { -1 } else { 0 };
        car!(w, i).u_4d6 = 0;
        w.g.u_61d4 = 0;
    }
    t
}

/// `FUN_08140c8c`: follow the player into a side segment it has taken, from the fork.
fn follow_into_branch(w: &mut CarWorld, i: usize) {
    let target = w.entity_of(w.g.wingman_car);
    if w.slots[target].e.segment == 0 || ent!(w, i).segment != 0 {
        return;
    }
    let wp = route::waypoint_at(w, ent!(w, i).segment as u32, ent!(w, i).waypoint as i32 + 1);
    let p = w.route.line.points[wp];
    if p.link_index == 0 && p.link_section == w.slots[target].e.segment {
        let e = &mut ent!(w, i);
        e.segment = p.link_section;
        e.waypoint = (p.link_index as i16).wrapping_sub(1);
        car!(w, i).u_4d6 = 2;
    }
}

/// `FUN_081408c4`: the attacker's command: catch the car at `wingman_target` (full throttle while engaged).
fn attacker_command(w: &mut CarWorld, i: usize, throttle: i32) -> i32 {
    let mut t = 0x9999;
    if w.g.u_61f0 == 0 {
        let other = w.entity_of(w.g.wingman_target);
        let d = gap_to(w, i, other);
        if ((d + 0xB3) as u32) < 0x9F && ent!(w, i).segment == w.slots[other].e.segment {
            let g = &mut w.g;
            g.u_61f0 = 1;
            g.u_6180 = 6;
            g.u_6174 = 0x12;
        }
        let g = &mut w.g;
        let prev = g.u_61fc as i32;
        g.u_61fc = (d + 100) as u32;
        let v = (d + 100)
            .wrapping_mul(0x2B)
            .wrapping_add(0x100)
            .wrapping_add(prev.wrapping_mul(-0x2A));
        let gain = if v < 1 {
            0x40 - div((-5 - v).clamp(0, 0x10) << 8, 0xB)
        } else {
            div((v - 1).clamp(0, 0x100) << 6, 0xFF) + 0x100
        };
        t = throttle.wrapping_mul(gain) >> 8;
        let left = (g.u_61e4 as i32).wrapping_sub(g.frame_ticks);
        g.u_61e4 = left as u32;
        if left < 0 {
            g.u_61e4 = 0;
            g.wingman_running = 0;
        }
    } else {
        let g = &mut w.g;
        g.u_6180 -= 1;
        if g.u_6180 == 0 {
            g.u_61f0 = 0;
        }
    }
    w.g.u_61d4 = 0;
    t
}

/// `FUN_08140a10`: the drafter's command: run just ahead of the player in its lane, filling the player's nitro
/// tank (by 0x2AAA per step up to 0x50000).
fn drafter_command(w: &mut CarWorld, i: usize, throttle: i32, lead: i32) -> i32 {
    let target = w.entity_of(w.g.wingman_car);
    if lead < -0x200 {
        car!(w, i).u_4d6 = 2;
    }
    let gap = lateral_gap(w, i, target);
    if lead < -0x40 && -0x380 < lead && gap < 0x100 {
        let tank = car!(w, target).nitro_tank.wrapping_add(0x2AAA);
        car!(w, target).nitro_tank = tank;
        if tank > 0x5_0000 {
            car!(w, target).nitro_tank = 0x5_0000;
            let g = &mut w.g;
            g.u_61e4 = 0;
            g.wingman_running = 0;
            g.u_618c = 0;
            g.u_6200 = 1;
        }
    }
    let g = &mut w.g;
    let prev = g.u_61fc as i32;
    g.u_61fc = (lead + 500) as u32;
    let v = (lead + 0x2F4).wrapping_add((lead + 500).wrapping_sub(prev).wrapping_mul(0x2A));
    let gain = if v < 1 {
        -div((-5 - v).clamp(0, 0x10) << 8, 0xB)
    } else {
        div((v - 1).clamp(0, 0x200).wrapping_mul(0xC0), 0x1FF)
    };
    let left = (g.u_61e4 as i32).wrapping_sub(g.frame_ticks);
    g.u_61e4 = left as u32;
    if left < 0 {
        g.u_61e4 = 0;
        g.wingman_running = 0;
        g.u_618c = 0;
        g.u_6200 = 1;
    }
    g.u_61d4 = 0;
    (gain + 0x100).wrapping_mul(throttle) >> 8
}

/// `FUN_081412ac`: steering of computer cars that are not racers (the wingman).
fn traffic_steer(w: &CarWorld, i: usize, steer: i32) -> i32 {
    if w.g.u_61f0 == 0 {
        return steer;
    }
    let other = w.entity_of(w.g.wingman_target);
    if (car!(w, other).lane as i16) < (car!(w, i).lane as i16) {
        -0x400
    } else {
        0x400
    }
}

/// `FUN_0813c240` (its value is passed through): mark the lanes (`blocked`) taken by traffic cars just ahead on
/// the main route and, after 0xB4 race frames, by racers close ahead.
fn mark_blocked_lanes(w: &mut CarWorld, i: usize) {
    if ent!(w, i).segment == 0 {
        for k in 0..8 {
            let live = w.g.live[k];
            if live.is_none() {
                continue;
            }
            let t = &w.slots[w.entity_of(live)].e;
            let d = t.traffic_waypoint as i32 - ent!(w, i).waypoint as i32;
            let lim = if t.direction == 1 { 3 } else { 6 };
            if d < lim && d >= 0 && t.race_state == 1 {
                let dir = t.direction;
                let c = &mut car!(w, i);
                c.blocked[2] = 1;
                if dir == 1 {
                    c.blocked[3] = 1;
                    c.route_flags |= 0x100;
                } else {
                    c.blocked[1] = 1;
                    c.route_flags |= 0x200;
                }
            }
        }
    }
    if w.g.race_frames as i32 > 0xB4 {
        let mut o = 0;
        loop {
            if o != i {
                let (p, q) = (&car!(w, i), &car!(w, o));
                let d = q.progress.wrapping_sub(p.progress);
                let s = (p.forward_speed.wrapping_sub(q.forward_speed) >> 8).max(0);
                if d < s + 0x400 && d > -0x80 {
                    let lane = q.lane as i16;
                    if let Some(b) = car!(w, i).blocked.get_mut(lane as usize) {
                        *b = 1;
                    }
                }
            }
            if ent!(w, o).index as u32 >= w.g.racers {
                break;
            }
            o += 1;
        }
    }
}

/// `FUN_081489dc`: the opponent's four wheels on the ground. Like the player's (`contact::wheels`) but every
/// wheel stands on the car's own sector, the front wheels always turn by `steering >> 8`, the floor normal is
/// taken as it is, and no slip is recorded. Returns the number of wheels on the ground.
fn wheels(w: &mut CarWorld, i: usize, dt: i32) -> i32 {
    let (rom, data) = (w.rom, w.data);
    let mut grounded = 0;
    let sector = ent!(w, i).sector as u32;
    let c = &car!(w, i);
    let h = w.floor_height(sector, c.body.pos[0] >> 8, c.body.pos[2] >> 8);
    let c = &mut car!(w, i);
    if c.body.pos[1] - h > 0x800 && c.body.vel[1] > 0 {
        let my = c.body.momentum[1] - c.body.vel[1];
        c.body.momentum[1] = my;
        c.body.vel[1] = mul12(c.body.inv_mass, my);
    }
    let rot = c.body.rot;
    let (rear_x, rear_z) = (rot[0], rot[2]);
    let (cs, sn) = (cos(rom, c.steering >> 8), sin(rom, c.steering >> 8));
    let mut fx = ((cs >> 2).wrapping_mul(rot[0]) + rot[6].wrapping_mul(-sn >> 2)) >> 12;
    let mut fz = ((cs >> 2).wrapping_mul(rot[2]) + rot[8].wrapping_mul(-sn >> 2)) >> 12;
    c.wheel_spin = 0;
    let fs = &data.city[w.geometry().floor_sector(sector) as usize];
    for k in 0..4 {
        let c = &mut car!(w, i);
        let local = c.wheels[k].car_pos;
        let pos =
            [0, 1, 2].map(|j| mul12(local[0], rot[j]) + mul12(rot[3 + j], local[1]) + mul12(rot[6 + j], local[2]));
        c.wheels[k].world_pos = pos;
        if k == 2 {
            fx = rear_x;
            fz = rear_z;
        }
        let body_pos = c.body.pos;
        let ride = c.wheels[0].ride_height;
        w.query.pos = [0, 1, 2].map(|j| pos[j].wrapping_add(body_pos[j]) >> 8);
        let floor = w.floor_height(sector, w.query.pos[0], w.query.pos[2]);
        let c = &mut car!(w, i);
        c.wheels[k].sector = sector as u16;
        c.wheels[k].u_92 = (sector >> 16) as u16;
        let pen = body_pos[1] + pos[1] + ride - floor;
        if pen > 0 {
            grounded += 1;
            let contact = [pos[0], pos[1] + (ride - pen), pos[2]];
            c.wheels[k].contact = contact;
            let omega = c.body.ang_vel;
            let mut vx = c.body.vel[0] + (mul12(omega[2], contact[1]) - mul12(omega[1], contact[2]));
            let vy = c.body.vel[1] + (mul12(omega[0], contact[2]) - mul12(omega[2], contact[0]));
            let mut vz = c.body.vel[2] + (mul12(omega[1], contact[0]) - mul12(omega[0], contact[1]));
            let surface = (fs.floor as usize).min(7);
            let wheel = &mut c.wheels[k];
            let damping = if vy < 1 { wheel.damping - 10 } else { wheel.damping };
            let normal = mul12(fs.plane[0] as i32, vx) + mul12(fs.plane[1] as i32, vy) + mul12(fs.plane[2] as i32, vz);
            let load = ((pen.wrapping_mul(wheel.spring) >> 8) + (normal.wrapping_mul(damping) >> 7)).max(0);
            let (mut ax, mut az) = (0, 0);
            let spin = wheel.spin;
            vx += (spin >> 7).wrapping_mul(fz) >> 12;
            vz += (-fx).wrapping_mul(spin >> 7) >> 12;
            let slip2 = vx.wrapping_mul(vx).wrapping_add(vz.wrapping_mul(vz));
            if slip2 != 0 {
                let grip = ((wheel.grip.wrapping_mul(load) >> 8).wrapping_mul(data.car.surface_grip[surface]) >> 8)
                    .min(0x4000);
                if grip.wrapping_mul(grip) < slip2 {
                    let r = recip(rom, isqrt(slip2 as u32));
                    let (ux, uz) = (r.wrapping_mul(vx) >> 12, r.wrapping_mul(vz) >> 12);
                    ax = (-(grip >> 4)).wrapping_mul(ux) >> 12;
                    az = (-(grip >> 4)).wrapping_mul(uz) >> 12;
                } else {
                    ax = -vx >> 4;
                    az = -vz >> 4;
                }
                wheel.spin = spin + (dt.wrapping_mul(mul12(ax, fz) + mul12(az, -fx)) >> 1);
            }
            let ix = dt.wrapping_mul(ax) >> 12;
            let iy = dt.wrapping_mul(-load) >> 12;
            let iz = dt.wrapping_mul(az) >> 12;
            let b = &mut c.body;
            b.momentum[0] += ix;
            b.momentum[1] += iy;
            b.momentum[2] += iz;
            let [cx, cy, cz] = contact;
            b.ang_momentum[0] -= mul12(iz, cy) - mul12(iy, cz);
            b.ang_momentum[1] -= mul12(ix, cz) - mul12(iz, cx);
            b.ang_momentum[2] -= mul12(iy, cx) - mul12(ix, cy);
        }
        c.wheel_spin = c.wheel_spin.wrapping_add(c.wheels[k].spin);
    }
    let c = &mut car!(w, i);
    if grounded == 0 {
        c.airborne += 1;
    } else {
        c.airborne = 0;
    }
    crate::body::update_velocities(&mut c.body);
    grounded
}
