//! Traffic cars on typed state: entity handler 0x36 (`FUN_081443fc`). A traffic car (spawned by `traffic::spawn`)
//! drives its lane of the main route at a fixed crawl, turning towards the next waypoint over 0x20 steps, until it
//! is far from the entity the camera follows; it is not a rigid body. A racer running into it knocks it away
//! (state 3).
//!
//! Entity fields (`state::Entity`): `dir_x`/`dir_z` direction (1.0 = 0x1000), `speed`, `heading` and
//! `angles[1]` (shown heading), `angles[0]` pitch, `wobble`, `race_state` (0 new, 1 driving, 2 remove, 3 knocked
//! away), `speed_up`, `knocked` (timer), `segment`, `traffic_type`, `direction` (along the route, ±1),
//! `traffic_waypoint`, `traffic_mode` (1 lane driving, 2 follow the waypoints, else stop at the waypoint). The
//! [`TrafficBlock`](crate::state::TrafficBlock): the target point, the turn, flags, lane.
//!
//! The block belongs to the entity (`Slot::block`); the sector lists are `CarWorld::heads` and `Entity::next`.

use crate::carworld::{CarWorld, NONE};
use crate::math::{cos, cross, div, isqrt, sin, sub};
use crate::state::EntityRef;
use crate::{Result, Unported};

/// `FUN_081443fc`: entity handler 0x36 for entity `i`.
pub fn handler(w: &mut CarWorld, i: usize) -> Result<()> {
    let camera = w.g.focus as usize;
    let e = &w.slots[i].e;
    let (state, old_sector) = (e.race_state, e.sector);
    if w.g.traffic_on == 0 {
        return Ok(());
    }
    match state {
        1 => drive(w, i, camera, old_sector),
        0 => {
            // `FUN_0814f874`: on the floor, no matrix slot.
            let (sector, x, z) = (e.sector as u32, e.pos[0] >> 8, e.pos[2] >> 8);
            let floor = w.floor_height(sector, x, z);
            let e = &mut w.slots[i].e;
            e.pos[1] = floor;
            e.slot = 0xFF;
            e.extra_model = 0;
            e.race_state = 1;
            e.u_a0 = 0;
            Ok(())
        }
        3 => knocked_away(w, i, camera),
        2 => {
            w.slots[i].block = None;
            let e = &mut w.slots[i].e;
            e.state &= 0xFFFE;
            e.slot = 0xFF;
            w.unlink(i);
            w.g.traffic_count = w.g.traffic_count.wrapping_sub(1);
            if let Some(k) = (0..8).find(|&k| w.g.live[k] == EntityRef::to(i)) {
                w.g.live[k] = EntityRef::NONE;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Distance measure used for "far from the camera's car": squared 1/256-scaled city units, absolute value.
fn far(w: &CarWorld, a: usize, e: usize) -> i32 {
    let (a, e) = (&w.slots[a].e, &w.slots[e].e);
    let dx = ((a.pos[0] >> 8) - (e.pos[0] >> 8)) >> 8;
    let dz = ((a.pos[2] >> 8) - (e.pos[2] >> 8)) >> 8;
    dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz)).wrapping_abs()
}

/// State 1: drive along the lane.
fn drive(w: &mut CarWorld, i: usize, camera: usize, old_sector: u16) -> Result<()> {
    let e = &mut w.slots[i].e;
    e.speed = 3;
    e.speed_up = e.speed_up.wrapping_add(1);
    if w.g.speed_up_steps <= e.speed_up as u32 {
        if (e.speed as u32) < w.g.traffic_max_speed {
            e.speed += 1;
        }
        e.speed_up = 0;
    }
    let speed = e.speed;
    if far(w, camera, i) > 0x2000 {
        w.slots[i].e.race_state = 2;
        return Ok(());
    }
    if w.route.line.sections.is_empty() {
        // The game would read its target waypoint through a null segment table (BIOS memory).
        return Err(Unported("traffic car without a route (world +0x40 is null)"));
    }
    let rom = w.rom;
    let seg = w.slots[i].e.segment as usize;
    let (count, first) = {
        let s = &w.route.line.sections[seg];
        (s.count as i32, s.first as i32)
    };
    let wp = |i: i32| (first.wrapping_add(i)) as usize;
    let point = |w: &CarWorld, k: usize| {
        let p = &w.route.line.points[k];
        [p.x, p.z]
    };
    let target = wp(w.slots[i].e.traffic_waypoint as i32);
    // The turn in progress: the direction moves by 1/0x20 of the change per step.
    let s = &mut w.slots[i];
    let b = s.block.as_mut().expect("a driving traffic car has its block");
    if b.turn_steps != 0 {
        b.turn_t += 0x80;
        b.turn_steps -= 1;
        s.e.dir_x = b.turn_from[0] + (b.turn_t.wrapping_mul(b.turn_by[0]) >> 12);
        s.e.dir_z = b.turn_from[1] + (b.turn_t.wrapping_mul(b.turn_by[1]) >> 12);
        let h = nfsgba_fixed::atan2_fast(rom, s.e.dir_x, s.e.dir_z);
        s.e.angles[1] = h as i16;
        s.e.heading = s.e.angles[1] as i32;
        if b.turn_steps == 0 {
            let dx = b.target[0] - (s.e.pos[0] >> 8);
            let dz = b.target[1] - (s.e.pos[2] >> 8);
            let len = isqrt(dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz)) as u32);
            s.e.dir_x = div(dx.wrapping_mul(0x1000), len);
            s.e.dir_z = div(dz.wrapping_mul(0x1000), len);
        }
    }
    let e = &mut w.slots[i].e;
    e.pos[0] = e.pos[0].wrapping_add(speed.wrapping_mul(e.dir_x));
    e.pos[2] = e.pos[2].wrapping_add(speed.wrapping_mul(e.dir_z));
    let wobble = e.wobble;
    if wobble == 0 {
        // Ease the displayed heading towards the travel heading by halves.
        let cur = e.heading;
        let shown = e.angles[1] as i32;
        if shown != cur {
            let d = shown - cur;
            let half = (d >> 1) as i16;
            e.angles[1] = if d < 0 {
                (shown as i16).wrapping_add(half)
            } else {
                (shown as i16).wrapping_sub(half)
            };
        }
    } else {
        e.angles[1] = e.angles[1].wrapping_add(wobble as i16);
        e.wobble = wobble >> 1;
        if wobble >> 1 == 0 {
            e.speed = 1;
            e.speed_up = 0;
        }
    }
    let d2 = |w: &CarWorld, x: i32, z: i32| {
        let e = &w.slots[i].e;
        let dx = x.wrapping_mul(0x100).wrapping_sub(e.pos[0]) >> 8;
        let dz = z.wrapping_mul(0x100).wrapping_sub(e.pos[2]) >> 8;
        dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz)) >> 8
    };
    let block_target = |w: &CarWorld| w.slots[i].block.as_ref().unwrap().target;
    let circuit = w.g.circuit != 0;
    let tp = point(w, target);
    match w.slots[i].e.traffic_mode {
        1 => {
            let bt = block_target(w);
            if d2(w, bt[0], bt[1]) < speed.wrapping_mul(0x41A) {
                let e = &mut w.slots[i].e;
                let mut n = e.traffic_waypoint as i32 + e.direction as i32;
                if !circuit && (count - 1 <= n || n < 1) {
                    e.race_state = 3;
                    e.knocked = 600;
                }
                if !circuit {
                    n = n.min(count - 1).max(0);
                } else {
                    if count - 1 <= n {
                        n = 0;
                    }
                    if n < 0 {
                        n += count - 1;
                    }
                }
                let next = point(w, wp(n));
                w.slots[i].e.traffic_waypoint = n as i16;
                let dx = next[0] - tp[0];
                let dz = next[1] - tp[1];
                let len = isqrt(dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz)) as u32);
                let (ux, uz) = (div(dx.wrapping_mul(0x1000), len), div(dz.wrapping_mul(0x1000), len));
                let b = w.slots[i].block.as_mut().unwrap();
                let lane = w.data.ai.traffic.lanes[b.lane as usize];
                let px = next[0] + (lane.wrapping_mul(uz) >> 8);
                let pz = next[1] - (ux.wrapping_mul(lane) >> 8);
                b.target = [px, pz];
                let nx = div((px - tp[0]).wrapping_mul(0x1000), len);
                let nz = div((pz - tp[1]).wrapping_mul(0x1000), len);
                start_turn(w, i, nx, nz, 0x20);
            }
        }
        2 => {
            if (d2(w, tp[0], tp[1]) as u32) < (speed.wrapping_mul(w.g.traffic_stop)) as u32 {
                let n = w.slots[i].e.traffic_waypoint as i32 + 1;
                if n < count {
                    let next = point(w, wp(n));
                    w.slots[i].e.traffic_waypoint = n as i16;
                    let dx = next[0] - tp[0];
                    let dz = next[1] - tp[1];
                    let len = isqrt(dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz)) as u32);
                    let (nx, nz) = (div(dx.wrapping_mul(0x1000), len), div(dz.wrapping_mul(0x1000), len));
                    start_turn(w, i, nx, nz, 0x10);
                    w.slots[i].block.as_mut().unwrap().target = next;
                } else {
                    w.slots[i].e.race_state = 2;
                }
            }
        }
        _ => {
            if (d2(w, tp[0], tp[1]) as u32) < (speed.wrapping_mul(w.g.traffic_stop)) as u32 {
                w.slots[i].e.race_state = 2;
            }
        }
    }
    w.unlink(i);
    let e = &w.slots[i].e;
    w.query.pos[0] = e.pos[0] >> 8;
    w.query.pos[2] = e.pos[2] >> 8;
    w.query.sector = e.sector;
    let s = find_sector(w);
    w.slots[i].e.sector = if s == NONE { old_sector } else { s as u16 };
    let e = &w.slots[i].e;
    let sector = e.sector as u32;
    let floor = w.floor_height(sector, e.pos[0] >> 8, e.pos[2] >> 8);
    w.slots[i].e.pos[1] = floor;
    let b = w.slots[i].block.as_ref().unwrap();
    if sector as u16 != old_sector || b.flags & 1 == 0 {
        let e = &w.slots[i].e;
        let mode = e.traffic_mode;
        // Pitch from the floor one turn-step ahead.
        let ahead = w.floor_height(
            sector,
            e.pos[0].wrapping_add(b.turn_from[0]).wrapping_add(b.turn_by[0]) >> 8,
            e.pos[2].wrapping_add(b.turn_from[1]).wrapping_add(b.turn_by[1]) >> 8,
        );
        let s = &mut w.slots[i];
        s.block.as_mut().unwrap().flags |= 1;
        if mode == 1 {
            let pitch = nfsgba_fixed::atan2_fast(rom, ahead - s.e.pos[1], 0xC00);
            s.e.angles[0] = (pitch as i16).wrapping_neg();
        }
    }
    w.link(i);
    let hit = collide_racers(w, i, rom);
    if hit & 2 == 0 {
        w.slots[i].e.slot = 0xFF;
    }
    Ok(())
}

/// State 3: knocked away (or run off its route): slide with friction (1/32 per step), bounce off solid walls
/// and spin down the wobble, until the timer (`knocked`, up by 600 / (`frame_ticks` / 0x1C) per step) passes
/// 0x1F4 while unseen, or the car is far from the camera's car; then it is removed.
fn knocked_away(w: &mut CarWorld, i: usize, camera: usize) -> Result<()> {
    let rom = w.rom;
    let step = div(600, div(w.g.frame_ticks, 0x1C));
    let e = &mut w.slots[i].e;
    let t = (e.knocked as i32).wrapping_add(step);
    e.knocked = t as u16;
    if t.wrapping_mul(0x1_0000) >= 0x1F4_0001 && e.flags & 4 == 0 {
        e.race_state = 2;
        return Ok(());
    }
    if far(w, camera, i) > 1000 {
        w.slots[i].e.race_state = 2;
        return Ok(());
    }
    let e = &w.slots[i].e;
    if e.dir_x.wrapping_abs() >= 2 || e.dir_z.wrapping_abs() > 1 {
        w.query.sector = e.sector;
        bounce_off_walls(w, i);
    }
    let e = &mut w.slots[i].e;
    let (vx, vz) = (e.dir_x, e.dir_z);
    e.pos[0] = e.pos[0].wrapping_add(vx);
    e.pos[2] = e.pos[2].wrapping_add(vz);
    let slow = |v: i32| match v - (v >> 5) {
        -1 => 0,
        v => v,
    };
    e.dir_x = slow(vx);
    e.dir_z = slow(vz);
    w.query.pos = [e.pos[0] >> 8, e.pos[1] >> 8, e.pos[2] >> 8];
    w.query.sector = e.sector;
    let wobble = e.wobble;
    if wobble != 0 {
        e.angles[1] = e.angles[1].wrapping_add(wobble as i16);
        let v = wobble - (wobble >> 4);
        e.wobble = if v.wrapping_abs() < 0x10 { 0 } else { v };
    }
    let old = e.sector;
    w.unlink(i);
    let e = &w.slots[i].e;
    w.query.pos[0] = e.pos[0] >> 8;
    w.query.pos[2] = e.pos[2] >> 8;
    w.query.sector = e.sector;
    let s = find_sector(w);
    w.slots[i].e.sector = if s == NONE { old } else { s as u16 };
    w.link(i);
    let e = &w.slots[i].e;
    let floor = w.floor_height(e.sector as u32, e.pos[0] >> 8, e.pos[2] >> 8);
    w.slots[i].e.pos[1] = floor;
    collide_racers(w, i, rom);
    w.slots[i].e.slot = 0xFF;
    Ok(())
}

/// `FUN_0814658c`: bounce the knocked-away car's velocity (`dir_x`/`dir_z`) off the solid walls of its sector
/// within 100 units (the same end tests as the car's walls, `walls::walls`): restitution 0x13/0x400 along the
/// wall normal when moving into it. Returns whether any wall was near.
fn bounce_off_walls(w: &mut CarWorld, i: usize) -> bool {
    let data = w.data;
    let e = &mut w.slots[i].e;
    let walls = &data.city[e.sector as usize].walls;
    let n = walls.len();
    let (x, z) = (e.pos[0] >> 8, e.pos[2] >> 8);
    let mut near = false;
    for k in 0..n {
        let (wall, next) = (&walls[(k + n - 1) % n], &walls[k]);
        if wall.piece as u32 == NONE && wall.flags & 0x1000 != 0 {
            let (wx, wz) = (wall.x, wall.z);
            let (dx, dz) = (x.wrapping_sub(wx), z.wrapping_sub(wz));
            let (nx, nz) = (wall.normal[0] as i32, wall.normal[1] as i32);
            if dx.wrapping_mul(nx).wrapping_add(dz.wrapping_mul(nz)) >> 12 <= 100 {
                let (cx, cz) = (next.x, next.z);
                let d2 = |a: i32, b: i32| a.wrapping_mul(a).wrapping_add(b.wrapping_mul(b));
                let past = if (cx - wx).wrapping_mul(dx).wrapping_add(dz.wrapping_mul(cz - wz)) < 0 {
                    d2(dx, dz) > 0x270F
                } else {
                    let (ex, ez) = (x.wrapping_sub(cx), z.wrapping_sub(cz));
                    (wx - cx).wrapping_mul(ex).wrapping_add((wz - cz).wrapping_mul(ez)) < 0 && d2(ex, ez) > 0x270F
                };
                if !past {
                    near = true;
                    let vn = (e.dir_x.wrapping_mul(nx) >> 6) + (e.dir_z.wrapping_mul(nz) >> 6);
                    if vn < 0 {
                        let j = vn.wrapping_mul(-0x13) >> 10;
                        e.dir_x += nx.wrapping_mul(j) >> 12;
                        e.dir_z += j.wrapping_mul(nz) >> 12;
                    }
                }
            }
        }
    }
    near
}

/// The turn towards direction (`nx`, `nz`) over `steps` steps, from the current direction.
fn start_turn(w: &mut CarWorld, i: usize, nx: i32, nz: i32, steps: i32) {
    let s = &mut w.slots[i];
    let (dx, dz) = (s.e.dir_x, s.e.dir_z);
    let b = s.block.as_mut().unwrap();
    b.turn_from = [dx, dz];
    b.turn_by = [nx - dx, nz - dz];
    b.turn_t = 0;
    b.turn_steps = steps;
}

/// `FUN_08144b7c` ([`crate::world::Geometry::traffic_find_sector`]) on the query point from the query sector.
fn find_sector(w: &CarWorld) -> u32 {
    let (x, z, start) = (w.query.pos[0], w.query.pos[2], w.query.sector as u32);
    w.geometry().traffic_find_sector(start, x, z)
}

/// `FUN_08146094`: test the traffic car against the racers; bit 0 = a racer is near, bit 1 = it was hit (the
/// response `FUN_08145dac` knocks it away).
fn collide_racers(w: &mut CarWorld, e: usize, rom: &[u8]) -> u32 {
    let racers = w.g.racers;
    let dt = crate::math::recip(rom, w.g.dt << 8).min(0xC00);
    let t = &w.slots[e].e;
    let (vx, vz) = if t.race_state == 3 {
        (t.dir_x, t.dir_z)
    } else {
        (t.speed.wrapping_mul(t.dir_x), t.speed.wrapping_mul(t.dir_z))
    };
    let (vx, vz) = (vx >> 4, vz >> 4);
    let mut result = 0;
    // Once set, the hit flag stays set for the rest of the loop (the game's `local_90`).
    let mut hit = false;
    for o in 0..racers.wrapping_add(1) as usize {
        let (t, r) = (&w.slots[e].e, &w.slots[o].e);
        let dx = t.pos[0].wrapping_sub(r.pos[0]) >> 12;
        let dz = t.pos[2].wrapping_sub(r.pos[2]) >> 12;
        let d2 = dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz));
        if d2 <= 0x1_D4C0 {
            result |= 1;
            let dy = t.pos[1].wrapping_sub(r.pos[1]);
            let close_y = if dy < 0 {
                r.pos[1].wrapping_sub(t.pos[1]) <= 0xFFFF
            } else {
                dy <= 0xFFFF
            };
            if close_y && d2 < 0x801 {
                hit |= hits(w, e, o, [vx, vz], dt);
                if hit && respond(w, o, e) {
                    let t = &mut w.slots[e].e;
                    t.race_state = 3;
                    t.knocked = 0;
                    result |= 2;
                    let kind = t.traffic_type as usize;
                    w.slots[o].c.contact |= 1;
                    if w.data.ai.traffic.ends_race[kind] != 0 && w.slots[o].e.index as u32 == w.g.player {
                        w.g.phase = 7;
                    }
                }
            }
        }
    }
    result
}

/// `FUN_08145dac`: racer `o` hits traffic car `t` at the midpoint of their centres: an impulse along the line
/// between them (restitution clamped to −0x16..−0x11), split by the traffic type's shifts between the traffic car
/// (velocity, wobble) and the racer's body (momentum, spin); the racer's lane, hunter life and the crash sound
/// for the player. Returns whether they were closing.
fn respond(w: &mut CarWorld, o: usize, t: usize) -> bool {
    let rom = w.rom;
    let dt = crate::math::recip(rom, w.g.dt << 8).min(0xC00);
    let (oe, te) = (&w.slots[o].e, &w.slots[t].e);
    let mut n = [(oe.pos[0] - te.pos[0]) >> 2, 0, (oe.pos[2] - te.pos[2]) >> 2];
    crate::math::normalize14(&mut n);
    let n = [n[0] >> 8, 0, n[2] >> 8];
    let point = [(te.pos[0] + oe.pos[0]) >> 1, (te.pos[2] + oe.pos[2]) >> 1];
    let r_o = [point[0] - oe.pos[0], 0, point[1] - oe.pos[2]];
    let r_t = [point[0] - te.pos[0], 0, point[1] - te.pos[2]];
    let knocked = te.race_state == 3;
    let speed = te.speed;
    let (tx, tz) = if knocked {
        (te.dir_x, te.dir_z)
    } else {
        (speed.wrapping_mul(te.dir_x), speed.wrapping_mul(te.dir_z))
    };
    let body = &w.slots[o].c.body;
    let rel = [
        (dt.wrapping_mul(body.vel[0]) >> 11) - tx,
        (dt.wrapping_mul(body.vel[2]) >> 11) - tz,
    ];
    let vn = (rel[0].wrapping_mul(n[0]) >> 6) + (rel[1].wrapping_mul(n[2]) >> 6);
    if vn >= 0 {
        return false;
    }
    let k = (-0x11 - ((vn + 0x3000) >> 11)).clamp(-0x16, -0x11);
    let te = &mut w.slots[t].e;
    if !knocked {
        te.dir_x = speed.wrapping_mul(te.dir_x);
        te.dir_z = speed.wrapping_mul(te.dir_z);
    }
    let j = vn.wrapping_mul(k) >> 4;
    let imp = [n[0].wrapping_mul(j) >> 6, 0, n[2].wrapping_mul(j) >> 6];
    let kind = te.traffic_type as usize;
    let tables = &w.data.ai.traffic;
    let shift = tables.knock_shift[kind] & 0xFF;
    let asr = |v: i32, s: u32| crate::math::asr(v, s);
    if knocked {
        te.dir_x -= asr(imp[0], shift);
        te.dir_z -= asr(imp[2], shift);
    } else {
        te.dir_x -= div(asr(imp[0], shift), speed);
        te.dir_z -= div(asr(imp[2], shift), speed);
    }
    let spin = cross(r_t, imp);
    let wobble_shift = tables.knock_shift[kind].wrapping_add(0xD) & 0xFF;
    te.wobble = te.wobble.wrapping_add(asr(spin[1], wobble_shift));
    let q = &mut w.slots[o].c;
    if j > 0x4000 {
        q.hard_hit |= 4;
    }
    let (o_index, o_state) = (w.slots[o].e.index as u32, w.slots[o].e.race_state);
    if o_index > w.g.opponents && w.g.u_61f0 != 0 {
        w.g.u_61f0 = 0;
    }
    let q = &mut w.slots[o].c;
    if w.g.mode == 2 && o_state != 2 {
        // `FUN_081413b0`: hunter races: the hit costs the racer hunter life.
        let v = q.hunter_life - (w.g.hunter_damage.wrapping_mul(j) >> 8);
        q.hunter_life = v.max(0);
        q.u_4f0 = 0;
    }
    let shift = tables.racer_shift[kind] & 0xFF;
    let imp = [asr(imp[0], shift), 0, asr(imp[2], shift)];
    let b = &mut q.body;
    b.momentum[0] = b.momentum[0].wrapping_add(imp[0]);
    b.momentum[2] = b.momentum[2].wrapping_add(imp[2]);
    let turn = cross(r_o, [imp[0] >> 8, 0, imp[2] >> 8]);
    b.ang_momentum = sub(b.ang_momentum, turn);
    crate::body::update_velocities(b);
    let lateral = crate::route::lateral(w, o);
    let lane = crate::route::nearest_lane(w, lateral, -1);
    w.slots[o].c.lane = lane as u16;
    if o_index == w.g.player {
        w.sounds.push(crate::sound::Command::Stop(0x15));
        w.sounds.push(crate::sound::Command::Stop(0x16));
        if j > 0x800 {
            w.sounds
                .push(crate::sound::Command::Play(if j < 0x5001 { 0x16 } else { 0x15 }));
        }
    }
    true
}

fn sub2(a: [i32; 2], b: [i32; 2]) -> [i32; 2] {
    [a[0].wrapping_sub(b[0]), a[1].wrapping_sub(b[1])]
}

fn dot2(a: [i32; 2], b: [i32; 2]) -> i32 {
    a[0].wrapping_mul(b[0]).wrapping_add(a[1].wrapping_mul(b[1]))
}

/// The squared distance of the closest approach when the gap goes from `d0` to `d1` and closes, else `None`.
fn closest(d0: [i32; 2], d1: [i32; 2]) -> Option<i32> {
    let dd = sub2(d1, d0);
    if dot2(dd, d0) >= 1 {
        return None;
    }
    let t = dot2(dd, d1);
    if t < 0 {
        return Some(dot2(d1, d1));
    }
    let l = dot2(dd, dd);
    if l == 0 {
        return None;
    }
    let c = [
        d1[0].wrapping_sub(div(dd[0].wrapping_mul(t), l)),
        d1[1].wrapping_sub(div(dd[1].wrapping_mul(t), l)),
    ];
    Some(dot2(c, c))
}

/// The swept test of traffic car `e` (moving by `v`) against racer `o` over the frame: first the centres, then
/// the traffic type's collision points against the racer's two axle points (`FUN_08146094`).
fn hits(w: &CarWorld, e: usize, o: usize, v: [i32; 2], dt: i32) -> bool {
    let (t, q) = (&w.slots[e].e, &w.slots[o]);
    let (rom, c) = (w.rom, &q.c);
    let a0 = [t.pos[0] >> 4, t.pos[2] >> 4];
    let a1 = [a0[0].wrapping_add(v[0]), a0[1].wrapping_add(v[1])];
    let b0 = [q.e.pos[0] >> 4, q.e.pos[2] >> 4];
    let b1 = [
        (dt.wrapping_mul(c.body.vel[0]) >> 15).wrapping_add(b0[0]),
        (dt.wrapping_mul(c.body.vel[2]) >> 15).wrapping_add(b0[1]),
    ];
    if !closest(sub2(b0, a0), sub2(b1, a1)).is_some_and(|d| d < 0xF4_2400) {
        return false;
    }
    let kind = t.traffic_type as usize;
    let tables = &w.data.ai.traffic;
    let points = tables.point_count[kind];
    let heading = t.angles[1] as i32;
    let mut hit = false;
    for i in 0..points.max(0) as usize {
        let off = tables.points[kind][i];
        let ox = off.wrapping_mul(sin(rom, heading)) >> 14;
        let oz = off.wrapping_mul(cos(rom, heading)) >> 14;
        let (c0, c1) = ([a0[0] + ox, a0[1] + oz], [a1[0] + ox, a1[1] + oz]);
        for j in 0..2 {
            let axle = w.data.car.axle_offsets[j];
            let rx = c.body.rot[6].wrapping_mul(axle) >> 12;
            let rz = c.body.rot[8].wrapping_mul(axle) >> 12;
            let (r0, r1) = ([b0[0] + rx, b0[1] + rz], [b1[0] + rx, b1[1] + rz]);
            let radius = tables.hit_radius[kind];
            hit |= closest(sub2(r0, c0), sub2(r1, c1)).is_some_and(|d| d < radius);
        }
    }
    hit
}
