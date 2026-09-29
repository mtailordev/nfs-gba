//! Race-route tracking on typed state: which route section (entity `segment`) and waypoint (entity `waypoint`) a
//! car is at, its progress along the route, laps and the gap to the car ahead. The route is
//! `nfsgba_formats::career::RacingLine` (sections, waypoints, branch scales) with its plane table and back
//! table; section 0 is the main route, the others are side branches. This is the one racing-line model (D16):
//! the tracker (`FUN_0813edd8`) and the lap crossing (`FUN_0813f098`) are the formats' `track_player` and
//! `lap_crossing`.

use crate::carworld::{CarWorld, Slot};
use crate::math::{cos, div, sin};
use crate::state::EntityRef;

/// `FUN_0813e860`: normalise waypoint `index` of section `seg`, following links into the next or previous section
/// and wrapping on circuits. Returns the index and the section it lies in.
pub fn advance(w: &CarWorld, seg: u32, index: i32) -> (i32, u32) {
    let (s, i) = w.route.line.step(w.g.circuit != 0, &w.route.back, seg as usize, index);
    (i, s as u32)
}

/// `FUN_0814007c`: the point number of waypoint `index` of section `seg` (no range check, like the game).
pub fn waypoint(w: &CarWorld, seg: u32, index: i32) -> usize {
    (w.route.line.sections[seg as usize].first as i32).wrapping_add(index) as usize
}

/// `FUN_0814009c`: the normalised waypoint.
pub fn waypoint_at(w: &CarWorld, seg: u32, index: i32) -> usize {
    let (i, s) = advance(w, seg, index);
    waypoint(w, s, i)
}

/// `FUN_0813e93c`: how far the car is along its current waypoint's line (plane row: direction `[0]`/`[1]`,
/// widening terms `[2]`/`[3]`, length `[7]`), divided by 8.
pub fn along_line(w: &CarWorld, i: usize) -> i32 {
    let e = &w.slots[i].e;
    let (i1, s1) = advance(w, e.segment as u32, e.waypoint as i32);
    let line = i1.wrapping_add(w.route.line.sections[s1 as usize].first as i32);
    let p = w.route.line.points[waypoint(w, s1, i1)];
    let dx = (e.pos[0] >> 8).wrapping_sub(p.x);
    let dz = (e.pos[2] >> 8).wrapping_sub(p.z);
    let [a, b, c, d, .., len] = w.route.planes[line as usize];
    let along = dx.wrapping_mul(a).wrapping_add(b.wrapping_mul(dz)) >> 8;
    let across = b.wrapping_mul(dx).wrapping_sub(a.wrapping_mul(dz)) >> 8;
    let num = along + (across.wrapping_mul(c) >> 12);
    let den = len + (across.wrapping_mul(c + d) >> 12);
    if den == 0 {
        0
    } else {
        div(num.wrapping_mul(len), den) >> 3
    }
}

/// `FUN_0814032c`: race progress (distance along the route), 0 before the start line.
pub fn progress(w: &CarWorld, i: usize) -> i32 {
    let Slot { e, c, .. } = &w.slots[i];
    let wp = waypoint_at(w, e.segment as u32, e.waypoint as i32);
    if (w.g.circuit == 0 && e.waypoint == 0) || c.route_flags & 1 != 0 {
        return 0;
    }
    let t = along_line(w, i);
    w.route.line.points[wp].distance + (t.wrapping_mul(8).wrapping_mul(w.g.scales[e.segment as usize]) >> 8)
}

fn dist2_16(w: &CarWorld, i: usize, wp: usize) -> i32 {
    let (e, p) = (&w.slots[i].e, &w.route.line.points[wp]);
    let dx = ((e.pos[0] >> 8) - p.x) >> 4;
    let dz = ((e.pos[2] >> 8) - p.z) >> 4;
    dx * dx + dz * dz
}

/// `FUN_0813f234`: switch between the main route and side sections by the sector the car is in, using the
/// route's side-section table (per entry: section, its sectors). Returns 1 when nothing changed.
pub fn track_segment(w: &mut CarWorld, i: usize) -> i32 {
    let data = w.data;
    let list = data.car.side_segments[w.g.route_index as usize].as_deref();
    let cur = w.slots[i].e.segment as u32;
    w.g.visited[cur as usize] = 1;
    let Some(list) = list else { return 0 };
    let sector = w.slots[i].e.sector as u32;
    let count = |w: &CarWorld, seg: u32| i32::from(w.route.line.sections[seg as usize].count);
    // The waypoint to rejoin at: the section's join if the car is nearer its first waypoint than its last, else
    // the last waypoint's link.
    let rejoin = |w: &CarWorld, seg: u32| -> u16 {
        let first = waypoint(w, seg, 0);
        let last = waypoint(w, seg, count(w, seg) - 1);
        if dist2_16(w, i, first) < dist2_16(w, i, last) {
            w.route.back[seg as usize] as u16
        } else {
            w.route.line.points[last].link_index
        }
    };
    if cur == 0 {
        for (seg, sectors) in list {
            if sectors.contains(&sector) {
                w.slots[i].e.segment = *seg as u16;
                let first = waypoint(w, *seg, 0);
                let n = count(w, *seg);
                let last = waypoint(w, *seg, n - 1);
                let v = if dist2_16(w, i, first) < dist2_16(w, i, last) {
                    0
                } else {
                    n - 1
                };
                w.slots[i].e.waypoint = v as i16;
                return 1;
            }
        }
        return 0;
    }
    for (seg, sectors) in list {
        if *seg == cur {
            // Still in one of this section's sectors?
            if sectors.contains(&sector) {
                return 1;
            }
            let v = rejoin(w, *seg);
            let e = &mut w.slots[i].e;
            e.waypoint = v as i16;
            e.segment = 0;
            return 0;
        }
        if sectors.contains(&sector) {
            let v = rejoin(w, cur);
            let e = &mut w.slots[i].e;
            e.waypoint = v as i16;
            e.segment = *seg as u16;
            return 0;
        }
    }
    1
}

/// `FUN_0813edd8`: `RacingLine::track_player` on the car: counts steps driving against the route (sets the
/// wrong-way flag past 27), advances or steps back the waypoint when the car crosses a waypoint line, and runs
/// the lap crossing when it advanced.
pub fn track_waypoint(w: &mut CarWorld, i: usize) {
    let mut race = w.race();
    let mut r = w.slots[i].racer();
    let b = &w.slots[i].c.body;
    let vectors = [b.vel[0], b.vel[1], b.vel[2], b.rot[6], b.rot[7], b.rot[8]];
    let armed = w
        .route
        .line
        .track_player(&w.route.planes, &w.route.back, &mut race, &mut r, vectors);
    w.g.wrong_way = race.wrong_way as u32;
    w.slots[i].set_racer(&r);
    if armed {
        lap(w, i);
    }
}

/// `FUN_0813f098` (`lap_crossing`): an armed car crossing the line completes a lap (times, laps left), knocks out
/// the last car in elimination, and finishes when no laps are left or in a sprint. The lap's section count is
/// the race's own, which in sprints is two points longer than the ROM's.
pub fn lap(w: &mut CarWorld, i: usize) {
    let mut race = w.race();
    // The racers, and the car crossing (a wingman's car has an id above the opponents).
    let n = (race.player + race.opponents + 1).max(i as u32 + 1) as usize;
    let mut cars: Vec<_> = w.slots[..n].iter().map(Slot::racer).collect();
    w.route.line.lap_crossing(w.rom, &mut race, &mut cars, i);
    for (s, c) in w.slots.iter_mut().zip(&cars) {
        s.set_racer(c);
    }
    w.set_race(&race);
}

/// `FUN_0813ebac`: the time gap to the car one place ahead (or, for the leader, behind), into `g.gap`.
pub fn gap(w: &mut CarWorld, i: usize) {
    w.g.gap = 0;
    let place = w.slots[i].c.position;
    let target = if place == 1 { 2 } else { place - 1 };
    let (laps, lap_len, circuit) = (w.g.laps, w.route.line.lap_length(), w.g.circuit != 0);
    let distance = |c: &crate::state::Car| {
        if !circuit {
            c.progress
        } else {
            (laps - c.laps_left as i32) * lap_len + c.progress
        }
    };
    for k in 0..(w.g.opponents as i32).max(0) as usize {
        // The game looks at entity k + 1's driver.
        let q = &w.slots[k + 1].c;
        if q.position != target {
            continue;
        }
        let (other, mine) = (distance(q), distance(&w.slots[i].c));
        let time = w.g.time as i32;
        if place == 1 {
            let mine = match mine >> 4 {
                0 => 1,
                v => v,
            };
            if mine <= other >> 4 {
                return;
            }
            w.g.gap = time - div((other >> 4) * time, mine);
        } else {
            let other = match other >> 4 {
                0 => 1,
                v => v,
            };
            if other <= mine >> 4 {
                return;
            }
            w.g.gap = time - div((mine >> 4) * time, other);
        }
        return;
    }
}

/// `FUN_081402bc`: signed distance of the car from its waypoint across the route direction (the waypoint's
/// heading), in city units.
pub fn lateral(w: &CarWorld, i: usize) -> i32 {
    if w.route.line.sections.is_empty() {
        return 0;
    }
    let e = &w.slots[i].e;
    let wp = waypoint_at(w, e.segment as u32, e.waypoint as i32);
    let (p, x) = (&w.route.line.points[wp], &w.route.extra[wp]);
    let a = x.heading as i32 - 0x1000;
    let v = cos(w.rom, a)
        .wrapping_mul((e.pos[0] >> 8) - p.x)
        .wrapping_add(((e.pos[2] >> 8) - p.z).wrapping_mul(sin(w.rom, a)));
    (if v < 0 { v + 0x3FFF } else { v }) >> 14
}

/// `FUN_08140274`: which of the four lane offsets (enabled by bit mask `lanes`) is nearest `x`.
pub fn nearest_lane(w: &CarWorld, x: i32, lanes: i32) -> u32 {
    let (mut best, mut lane) = (i32::MAX, 0);
    for k in 0..4u32 {
        if (lanes >> k) & 1 != 0 {
            let d = (w.data.car.lanes[k as usize] - x).wrapping_abs();
            if d < best {
                best = d;
                lane = k;
            }
        }
    }
    lane
}

/// `FUN_08143b2c` (player only), up to the spawn: the traffic-spawn countdown. True when a traffic car is to be
/// spawned now (the caller spawns it and reports back with [`traffic_spawned`]).
pub fn traffic_wants_spawn(w: &mut CarWorld) -> bool {
    let g = &mut w.g;
    if g.u_6250 != 0 {
        g.u_6250 = 0;
        return false;
    }
    if g.traffic_on != 0 && g.traffic_count < 4 {
        g.traffic_timer = g.traffic_timer.wrapping_sub(1);
        return g.traffic_timer == 0;
    }
    false
}

/// The rest of the countdown after the spawn (`made`: the spawner found a free entity and a place).
pub fn traffic_spawned(w: &mut CarWorld, made: bool) {
    let g = &mut w.g;
    if made {
        g.traffic_count = g.traffic_count.wrapping_add(1);
    }
    g.traffic_timer = g.traffic_period as u8;
}

/// `FUN_0814078c` (control action 8, R+L): the wingman command; only with a wingman (`g.wingman` = 1..=12),
/// commands left, none running and the cooldown over. The attacker (`wingman_attacker` = 0) targets the
/// best-placed racer other than the wingman's car; otherwise the command flag is set. A given command costs one
/// command and starts the cooldown 0x1E000.
pub fn wingman_command(w: &mut CarWorld) {
    let g = &w.g;
    if g.wingman.wrapping_sub(1) as u32 >= 0xC {
        return;
    }
    if g.wingman_commands == 0 || g.wingman_running != 0 || g.wingman_cooldown != 0 {
        return;
    }
    let given = if g.wingman_attacker == 0 {
        let racers = g.opponents;
        let (mut target, mut place) = (EntityRef::NONE, 100);
        if racers != u32::MAX {
            for (k, s) in w.slots.iter().enumerate().take(racers as usize + 1) {
                let at = EntityRef::to(k);
                if at != g.wingman_car && s.e.index as u32 <= racers && s.c.position < place {
                    (target, place) = (at, s.c.position);
                }
            }
        }
        let g = &mut w.g;
        g.u_61e4 = g.u_6188;
        g.wingman_target = target;
        g.u_61fc = 0;
        g.u_61f0 = 0;
        !target.is_none()
    } else {
        let g = &mut w.g;
        g.u_618c = 1;
        g.u_61e4 = g.u_6188;
        true
    };
    if given {
        let g = &mut w.g;
        g.wingman_commands -= 1;
        g.wingman_running = 1;
        g.wingman_cooldown = 0x1_E000;
    }
}
