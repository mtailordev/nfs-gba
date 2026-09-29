//! Collisions on typed state: the car against the sector walls (`FUN_08145ca8`, `FUN_081459b8`, `FUN_081457b8`)
//! and the proximity test against the other racers (`FUN_08145320`).
//!
//! A sector's walls run from each wall's corner to the next wall's corner (the last wraps to the first): the
//! wall record `walls[k - 1]` holds the edge that ends at `walls[k]`. `Wall`: `normal` is the inward normal
//! (1.0 = 0x1000), `piece` its moving piece (world state) or 0xFFFF, `flags` bit 0x1000 solid and 0x4000
//! remembered at `Car::wall_4000`, `search_link` the neighbour sector through the portal or 0xFFFF.

use crate::body;
use crate::carworld::{CarWorld, NONE, Slot};
use crate::math::{add, cross, div, normalize, normalize14, scale, sub, udiv};
use crate::route::{lateral, nearest_lane};
use crate::sound::Command;
use nfsgba_formats::{Wall, career};

/// Walls in the neighbouring sector are only tested this far (city units squared) past a wall's ends.
const END_RADIUS2: i32 = 0x270F;

/// `FUN_08145ca8`: test the point 3/256 of the car's forward axis ahead of it against the walls, and for the
/// player start the scrape or crash sound on the first contact.
pub fn collide(w: &mut CarWorld, i: usize) {
    let (e, c) = (&w.slots[i].e, &w.slots[i].c);
    let x = e.pos[0].wrapping_add(c.body.rot[6] * 3) >> 8;
    let z = e.pos[2].wrapping_add(c.body.rot[8] * 3) >> 8;
    let (sector, player) = (e.sector as u32, w.is_player(i));
    let hit = walls(w, i, x, z, sector, true);
    if hit == 0 {
        if player {
            w.profile.scrape = 0;
        }
    } else if player && w.profile.scrape != 1 {
        w.profile.scrape = 1;
        if hit < 0x4001 {
            let force = hit - 0x800;
            if force > 0 {
                let volume = w.g.volume;
                let v = udiv((force as u32).wrapping_mul(volume), 0x3800);
                w.sounds.push(Command::Stop(0x16));
                let pitch = div(force * 0xDB8, 0x3800);
                w.sounds.push(Command::Start {
                    sample: 0x16,
                    pitch: pitch + 0x1B58,
                    channel: 3,
                    volume: v.wrapping_add(volume.wrapping_mul(3)),
                });
            }
        } else {
            w.sounds.push(Command::Stop(0x15));
            w.sounds.push(Command::Play(0x15));
        }
    }
}

/// `FUN_081459b8`: test (`x`, `z`) against the walls of `sector`, recursing once through open portals; returns
/// the largest impulse applied (0 for none).
pub(crate) fn walls(w: &mut CarWorld, i: usize, x: i32, z: i32, sector: u32, recurse: bool) -> i32 {
    let data = w.data;
    let s = &data.city[sector as usize];
    let n = s.walls.len();
    let mut best = 0;
    w.slots[i].c.wall_4000 = 0;
    for k in 0..n {
        let (prev, k_prev) = (&s.walls[(k + n - 1) % n], (k + n - 1) % n);
        let next = &s.walls[k];
        let flags = w.geometry().wall_flags(prev);
        let near = near(prev, next, x, z);
        if flags & 0x1000 != 0 {
            if near {
                if flags & 0x4000 != 0 {
                    w.slots[i].c.wall_4000 = w.walls_base + (s.first + k_prev) as u32 * 0x44;
                }
                let e = &w.slots[i].e;
                let nrm = [prev.normal[0] as i32, 0, prev.normal[1] as i32];
                let point = [e.pos[0] + nrm[0] * 4, e.pos[1], e.pos[2] + nrm[2] * 4];
                best = best.max(respond(w, i, point, nrm, prev));
            }
        } else if recurse && near {
            let mut link = prev.search_link as u32;
            if link != NONE {
                let t = &data.city[link as usize];
                if t.floor == 0 && t.alias as u32 != NONE {
                    link = t.alias as u32;
                }
                best = best.max(walls(w, i, x, z, link, false));
            }
        }
    }
    best
}

/// Whether (`x`, `z`) is within 100 units in front of the edge `prev` -> `next` and, past its ends, within
/// [`END_RADIUS2`] of the corner.
fn near(prev: &Wall, next: &Wall, x: i32, z: i32) -> bool {
    let (wx, wz, nx, nz) = (prev.x, prev.z, next.x, next.z);
    let (dx, dz) = (x.wrapping_sub(wx), z.wrapping_sub(wz));
    let dist = (prev.normal[0] as i32)
        .wrapping_mul(dx)
        .wrapping_add((prev.normal[1] as i32).wrapping_mul(dz))
        >> 12;
    if dist > 100 {
        return false;
    }
    if (nx - wx).wrapping_mul(dx).wrapping_add((nz - wz).wrapping_mul(dz)) < 0 {
        return dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz)) <= END_RADIUS2;
    }
    let (ex, ez) = (x.wrapping_sub(nx), z.wrapping_sub(nz));
    !((wx - nx).wrapping_mul(ex).wrapping_add((wz - nz).wrapping_mul(ez)) < 0
        && ex.wrapping_mul(ex).wrapping_add(ez.wrapping_mul(ez)) > END_RADIUS2)
}

/// `FUN_081457b8`: push the car off `wall` (normal `n`) at `point`: an impulse against the contact velocity
/// (restitution from a clamped linear map), damage and side flags, the lane, spin about y.
fn respond(w: &mut CarWorld, i: usize, point: [i32; 3], n: [i32; 3], wall: &Wall) -> i32 {
    let b = &w.slots[i].c.body;
    let r = [point[0] - b.pos[0], 0, point[2] - b.pos[2]];
    let spin = [0, b.ang_momentum[1] >> 4, 0];
    let c = cross(r, spin);
    let vx = (c[0] >> 14) + b.vel[0];
    let vz = (c[2] >> 14) + b.vel[2];
    let vn = (n[0].wrapping_mul(vx) >> 6) + (n[2].wrapping_mul(vz) >> 6);
    if vn >= 0 {
        return 0;
    }
    let k = (-(vn.wrapping_mul(0x1A2).wrapping_add(0x19D1_0000)) >> 24).clamp(-25, -16);
    let mut j = k * vn >> 10;
    let mut broke = false;
    if j > 0x2000 {
        let state = wall.piece as u32;
        if state != NONE && w.piece_kind[state as usize] == 1 {
            break_wall(w, wall);
            j = 0x2000;
            broke = true;
        }
    }
    let impulse = [n[0] * j >> 12, 0, n[2] * j >> 12];
    let index = w.slots[i].e.index as u32;
    if j > 0x4000 {
        w.slots[i].c.hard_hit |= 1;
    }
    if index > w.g.opponents && w.g.u_61f0 != 0 {
        // `FUN_081410a8` / `FUN_08141284`: a non-racer (traffic, cops) hitting a wall.
        w.g.u_61f0 = 0;
    }
    w.slots[i].c.contact |= 0x10;
    if w.g.mode == 2 && !broke {
        // `FUN_0814136c` (`hunter_wall_hit`): hunter races lose hunter life on wall hits.
        let mut r = w.slots[i].racer();
        career::hunter_drain(&mut r, 0, j);
        w.slots[i].set_racer(&r);
    }
    if n[2].abs() <= 0x7FF {
        w.slots[i].c.contact |= if n[0] < 1 { 4 } else { 2 };
    }
    let lane = nearest_lane(w, lateral(w, i), -1);
    let c = &mut w.slots[i].c;
    c.lane = lane as u16;
    let b = &mut c.body;
    b.momentum[0] += impulse[0];
    b.momentum[2] += impulse[2];
    let t = cross(r, impulse).map(|c| c >> 12);
    b.ang_momentum = add(b.ang_momentum, t);
    body::update_velocities(b);
    j
}

/// `FUN_0813b5a0`: a breakable wall gives way: its moving piece and its partner lose the solid flag (and bit 0)
/// and count one more hit.
fn break_wall(w: &mut CarWorld, wall: &Wall) {
    let state = wall.piece as usize;
    let partner = w.data.car.break_partners[state];
    if w.pieces[state].flags & 0x1000 != 0 && partner != -1 {
        for s in [state, partner as usize] {
            let p = &mut w.pieces[s];
            p.flags &= 0xEFFE;
            p.material = p.material.wrapping_add(1);
        }
    }
}

/// `FUN_08145320`: the swept proximity test against the racers after this one, and the car-to-car response
/// (`FUN_08144fa4`) for each hit.
pub fn racers(w: &mut CarWorld, i: usize, dt: i32) {
    let racers = w.g.racers;
    if w.slots[i].e.index as u32 >= racers {
        return;
    }
    let axle = w.data.car.axle_offsets;
    let (mut mask, mut bit) = (0i32, 1i32);
    let mut o = i + 1;
    loop {
        let (e, c) = (&w.slots[i].e, &w.slots[i].c);
        let (oe, oc) = (&w.slots[o].e, &w.slots[o].c);
        let state = oe.race_state;
        if oe.state & 4 != 0 && (state == 1 || state == 2 || state > 0xFF) {
            let dy = e.pos[1].wrapping_sub(oe.pos[1]);
            let close_y = if dy < 0 {
                oe.pos[1].wrapping_sub(e.pos[1]) <= 0xFFFF
            } else {
                dy <= 0xFFFF
            };
            if close_y {
                let dx = e.pos[0].wrapping_sub(oe.pos[0]) >> 12;
                let dz = e.pos[2].wrapping_sub(oe.pos[2]) >> 12;
                if (dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz)) as u32) < 0x301 {
                    let (av, bv) = (&c.body.vel, &oc.body.vel);
                    let a0 = [e.pos[0] >> 4, e.pos[2] >> 4];
                    let a1 = [
                        (dt.wrapping_mul(av[0]) >> 16) + a0[0],
                        (dt.wrapping_mul(av[2]) >> 16) + a0[1],
                    ];
                    let b0 = [oe.pos[0] >> 4, oe.pos[2] >> 4];
                    let b1 = [
                        (dt.wrapping_mul(bv[0]) >> 16) + b0[0],
                        (dt.wrapping_mul(bv[2]) >> 16) + b0[1],
                    ];
                    if swept_close(sub2(b0, a0), sub2(b1, a1), 0xF4_2400) {
                        for &off in &axle {
                            let ax = c.body.rot[6].wrapping_mul(off) >> 12;
                            let az = c.body.rot[8].wrapping_mul(off) >> 12;
                            let (ea0, ea1) = ([a0[0] + ax, a0[1] + az], [a1[0] + ax, a1[1] + az]);
                            for &off in &axle {
                                let bx = oc.body.rot[6].wrapping_mul(off) >> 12;
                                let bz = oc.body.rot[8].wrapping_mul(off) >> 12;
                                let (eb0, eb1) = ([b0[0] + bx, b0[1] + bz], [b1[0] + bx, b1[1] + bz]);
                                if swept_close(sub2(eb0, ea0), sub2(eb1, ea1), 0x33_A900) {
                                    mask += bit;
                                }
                                bit <<= 1;
                            }
                        }
                    }
                    // The hit mask is never reset between the other cars: after one hit, every later car
                    // within range gets a response too (reproduced as is).
                    if mask != 0 {
                        contact(w, i, o);
                    }
                }
            }
        }
        if racers <= w.slots[o].e.index as u32 {
            return;
        }
        o += 1;
    }
}

/// The tail of `FUN_08145320` for a hit between car `i` and the later car `o`: a unit normal from `i` towards
/// `o` and the midpoint as the contact point, the response, and in hunter races the hunter hit (the slower car
/// takes it; both velocities are normalised in place on the way, as the game does).
fn contact(w: &mut CarWorld, i: usize, o: usize) {
    let (pe, po) = (w.slots[i].e.pos, w.slots[o].e.pos);
    let mut normal = [0, 1, 2].map(|k| po[k].wrapping_sub(pe[k]) >> 2);
    normalize14(&mut normal);
    let point = [0, 1, 2].map(|k| pe[k].wrapping_add(po[k]) >> 1);
    let j = response(w, o, i, point, normal);
    if w.g.mode == 2 && j > 0x1000 {
        let rom = w.rom;
        let speed = |s: &mut Slot| {
            let mut v = s.c.body.vel;
            let len = normalize(rom, &mut v);
            s.c.body.vel = v;
            len
        };
        let (mine, theirs) = (speed(&mut w.slots[i]), speed(&mut w.slots[o]));
        // `FUN_0814101c` (`hunter_hit`): the attacker, then the victim.
        let (attacker, victim) = if mine < theirs { (i, o) } else { (o, i) };
        let (mut a, mut v) = (w.slots[attacker].racer(), w.slots[victim].racer());
        career::hunter_hit(&w.race(), &mut a, &mut v, j);
        w.slots[attacker].set_racer(&a);
        w.slots[victim].set_racer(&v);
    }
}

/// `FUN_08144fa4`: the car-to-car response between cars `a` and `b` at `point` along the unit `normal` (0x4000,
/// from `b` towards `a`); returns the impulse `j` (0 when the cars are separating). Velocities change by the
/// impulse, angular momenta by the contact arm crossed with the impulse shifted down by 25 (only 0 or -1 per
/// component survive, reproduced as is), momenta follow the velocities.
fn response(w: &mut CarWorld, a: usize, b: usize, point: [i32; 3], n: [i32; 3]) -> i32 {
    let (sa, sb) = crate::carworld::pair(&mut w.slots, a, b);
    let (ca, cb) = (&mut sa.c, &mut sb.c);
    // Contact arm and the contact point's velocity (angular part >> 14, linear part scaled by 0x1555 >> 8).
    let velocity = |c: &crate::state::Car, r: [i32; 3]| {
        let (wv, v) = (cross(r, c.body.ang_vel), c.body.vel);
        [0, 1, 2].map(|k| (wv[k] >> 14).wrapping_add(v[k].wrapping_mul(0x1555) >> 8))
    };
    let (ra, rb) = (sub(point, ca.body.pos), sub(point, cb.body.pos));
    let (va, vb) = (velocity(ca, ra), velocity(cb, rb));
    let dv = [0, 1, 2].map(|k| va[k].wrapping_sub(vb[k]) >> 8);
    let vn = (dv[0].wrapping_mul(n[0]) >> 6)
        .wrapping_add(dv[2].wrapping_mul(n[2]) >> 6)
        .wrapping_add(dv[1].wrapping_mul(n[1]) >> 6);
    if vn >= 0 {
        return 0;
    }
    let k = (-(vn.wrapping_add(0x1_C000) >> 15) - 0x10).clamp(-0x15, -0x11);
    let j = k.wrapping_mul(vn) >> 8;
    let imp = n.map(|c| (c.wrapping_mul(j) >> 10 >> 4).wrapping_mul(0x3072) >> 14);
    if j > 0x4_0000 {
        ca.hard_hit |= 2;
        cb.hard_hit |= 2;
    }
    cb.body.vel = sub(cb.body.vel, imp);
    ca.body.vel = add(ca.body.vel, imp);
    let spin = imp.map(|c| c >> 25);
    cb.body.ang_momentum = add(cb.body.ang_momentum, cross(rb, spin));
    ca.body.ang_momentum = sub(ca.body.ang_momentum, cross(ra, spin));
    for c in [&mut *ca, &mut *cb] {
        c.body.momentum = scale(c.body.vel, c.body.mass);
        c.contact |= 1;
    }
    let opponents = w.g.opponents;
    for (x, y) in [(a, b), (b, a)] {
        if w.slots[x].e.index as u32 > opponents {
            non_racer_hit(w, x, y);
        }
    }
    // The other car of a collision with the player gets the torque timer.
    let player = w.g.player;
    let (a_player, b_player) = (w.slots[a].e.index as u32 == player, w.slots[b].e.index as u32 == player);
    if b_player && !a_player {
        w.slots[a].c.torque_timer = 0x12;
    }
    if a_player && !b_player {
        w.slots[b].c.torque_timer = 0x12;
    }
    for x in [a, b] {
        let lane = nearest_lane(w, lateral(w, x), -1);
        w.slots[x].c.lane = lane as u16;
    }
    if a_player || b_player {
        w.sounds.push(Command::Stop(0x17));
        w.sounds.push(Command::Stop(0x18));
        if j > 0x800 {
            w.sounds.push(Command::Play(if j > 0x5000 { 0x17 } else { 0x18 }));
        }
    }
    j
}

/// `FUN_08141250`: a non-racer (a traffic or wingman car, `x`) in a car-to-car hit with `y`.
fn non_racer_hit(w: &mut CarWorld, x: usize, y: usize) {
    if w.g.u_6174 != 0 && x != 0 {
        w.slots[y].c.u_4b8 = 0x40;
    }
    if w.g.u_61f0 != 0 {
        w.g.u_61f0 = 0;
    }
}

fn sub2(a: [i32; 2], b: [i32; 2]) -> [i32; 2] {
    [a[0].wrapping_sub(b[0]), a[1].wrapping_sub(b[1])]
}

/// Whether the gap going from `d0` to `d1` closes and comes within `radius2` (the nearest point of the
/// segment d0..d1 to the origin, `FUN_08145320`).
fn swept_close(d0: [i32; 2], d1: [i32; 2], radius2: i32) -> bool {
    let dd = sub2(d1, d0);
    let dot = |a: [i32; 2], b: [i32; 2]| a[0].wrapping_mul(b[0]).wrapping_add(a[1].wrapping_mul(b[1]));
    if dot(dd, d0) >= 1 {
        return false;
    }
    let t = dot(dd, d1);
    let (cx, cz) = if t < 0 {
        (d1[0], d1[1])
    } else {
        let l = dot(dd, dd);
        if l == 0 {
            return false;
        }
        (
            d1[0] - div(dd[0].wrapping_mul(t), l),
            d1[1] - div(dd[1].wrapping_mul(t), l),
        )
    };
    cx.wrapping_mul(cx).wrapping_add(cz.wrapping_mul(cz)) < radius2
}
