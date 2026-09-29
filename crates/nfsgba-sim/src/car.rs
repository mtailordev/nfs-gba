//! The car entity's per-frame update on typed state: the handler (`FUN_0814bd4c`), the racing step
//! (`FUN_0814b168`) and the car dynamics (`FUN_0813d1f0`) with the engine, gearbox, steering, brakes and the order
//! of the sub-steps.
//!
//! The step runs on a [`CarWorld`] (`carworld.rs`); `ram.rs` loads it from the RAM image and stores it back. The
//! traffic spawn (`traffic.rs`) runs inside it.
//! The physics struct is described in `docs/engine/physics.md`.

use crate::body;
use crate::carworld::{CarWorld, NONE};
use crate::contact;
use crate::math::{add, atan2, div, dot, mat_mul, mul12, mul64, normalize, recip, scale, shr64, sub};
use crate::route;
use crate::sound::Command;
use crate::state::{Car, CarGlobals};
use crate::walls;
use nfsgba_formats::career;

/// Control actions (binding table rows, [`CarWorld::control`]).
const ACCELERATE: u32 = 0;
const BRAKE: u32 = 1;
const HANDBRAKE: u32 = 6;
const NITRO: u32 = 7;
const WINGMAN: u32 = 8;

impl CarWorld<'_> {
    /// `FUN_08144f38`: whether control action `action` is active for the held and newly pressed keys.
    pub fn control(&self, held: u32, pressed: u32, action: u32) -> bool {
        let b = self.data.car.bindings[self.g.binding_set as usize][(action & 0xFFFF) as usize];
        (b[0] as u32 & held) == b[1] as u32 && (b[2] as u32 & pressed) == b[3] as u32
    }
}

macro_rules! car {
    ($w:ident, $i:ident) => {
        $w.slots[$i].c
    };
}
macro_rules! ent {
    ($w:ident, $i:ident) => {
        $w.slots[$i].e
    };
}

/// `FUN_0814bd4c`: the car handler (entity handler table entries 0..3).
pub fn handler(w: &mut CarWorld, i: usize) {
    let index = ent!(w, i).index as u32;
    match ent!(w, i).race_state {
        2 => {
            // Finished: keep driving; the first finisher sets the race-over flag and starts the palette fade,
            // and once the fade is done the race ends (phase 3) with this car's index.
            racing_step(w, i);
            let g = &mut w.g;
            if g.race_over == 0 {
                g.race_over = 1;
                g.fade = -0x10;
            }
            if g.link == 0 && g.race_over != 0 && g.fade == 0 {
                g.phase = 3;
                g.over_index = index;
            }
        }
        0 => crate::init::car_init(w, i),
        0x100 => racing_step(w, i),
        _ => {}
    }
}

/// `FUN_0814b168`: nitro drain, dynamics, visibility flags, and (unless `g.settled` is set) the suspension step
/// `FUN_0814de40` with the body height from the four wheels.
fn racing_step(w: &mut CarWorld, i: usize) {
    let (dt, phase) = (w.g.dt, w.g.phase);
    if phase == 0 {
        return;
    }
    let index = ent!(w, i).index as u32;
    if phase != 1 && phase != 4 {
        // Rendering, done by `nfsgba-game` (`slots.rs`), not here: for the player, seen from the side, the game
        // redraws the decal onto the car's texture atlas here (`draw_decal_on_atlas`, `FUN_0813bd90`).
        nitro(&mut car!(w, i), &mut w.g, dt);
        let input = w.g.input[index as usize] as u32;
        dynamics(w, i, input, dt);
    }
    let g = &w.g;
    let e = &mut ent!(w, i);
    if g.focus == index {
        e.flags |= 1;
        if g.view > 1 {
            e.state |= 4;
        } else {
            e.state &= 0xFFFB;
        }
    } else {
        e.flags &= 0xFFFE;
        e.state |= 4;
    }
    if g.settled != 0 {
        return;
    }
    // Unreachable in Carbon: `race_init` and every dynamics step set `settled` to 1 (no trace step has it 0).
    // Four points around the car; their y is uninitialised stack in the game, but always overwritten with the
    // floor height (the sector query falls back to the car's sector, never 0xFFFF at a car step).
    let mut pts = [[-0x20, 0, 0x55], [0x20, 0, 0x55], [-0x20, 0, -0x2A], [0x20, 0, -0x2A]];
    let hits = contact::suspension(w, i, &mut pts, &mut [0; 4], dt);
    let c = &mut car!(w, i);
    c.points_on_floor = hits;
    let front = c.point_height[0].wrapping_add(c.point_height[1]) >> 1;
    let rear = c.point_height[2].wrapping_add(c.point_height[3]) >> 1;
    ent!(w, i).pos[1] = front.wrapping_add(rear) >> 1;
}

/// `FUN_08148 ...` `iwram_divmod`: the quotient, and the remainder into the globals' scratch word.
fn divmod(g: &mut CarGlobals, a: i32, b: i32) -> i32 {
    let (q, r) = nfsgba_fixed::iwram_divmod(a, b);
    g.div_rem = r;
    q
}

/// `FUN_0814b098`: drain the nitro tank while nitro is on.
pub(crate) fn nitro(c: &mut Car, g: &mut CarGlobals, dt: i32) {
    let rate = div(600, div(dt << 6, 0x1C));
    if c.nitro_on == 0 {
        return;
    }
    // The IWRAM division's quotient is discarded here; only its remainder store is kept.
    let factor = c.nitro_drain;
    divmod(g, c.nitro_tank, if factor == 0 { 1 } else { factor as i32 });
    let divisor = divmod(g, rate << 16, 0x1500);
    let drain = div((factor as i32) << 8, divisor);
    if g.nitro_free == 0 {
        c.nitro_tank -= drain;
    }
    if c.nitro_tank < 0 {
        c.nitro_tank = 0;
    }
}

/// Engine torque at `rpm` from the car's torque curve (10 points over 0..`max_rpm`).
pub fn torque(c: &Car, rpm: i32) -> i32 {
    let step = c.max_rpm >> 3;
    let k = div(rpm, step);
    if k < 9 {
        // A negative rpm reads the words before the curve (whatever the struct holds there).
        let at = |k: i32| match k {
            0.. => c.torque_curve[k as usize],
            _ => c.word(0x464 + 4 * k),
        };
        let y = at(k);
        y + div((at(k + 1) - y).wrapping_mul(rpm - step * k), step)
    } else {
        0
    }
}

/// Flag a gear change for the HUD (profile) when it is the player's car.
fn gear_changed(w: &mut CarWorld, player: bool) {
    if player {
        w.profile.gear_changed = 1;
    }
}

/// `FUN_0813c02c`: the automatic gearbox, at most one shift every 5 steps (`gearbox_pause`).
pub(crate) fn auto_shift(w: &mut CarWorld, i: usize) {
    let c = &car!(w, i);
    if c.gearbox_pause > 0 {
        return;
    }
    let gear = c.gear;
    if c.launch != 0 && gear != 1 {
        return;
    }
    let top_gear = w.data.car.handling[ent!(w, i).car as usize][0x15];
    let ratio = |g: i32| c.gear_ratios[g as usize];
    let shift = if gear == 1 || (c.upshift_rpm <= c.revs && gear < top_gear && gear != 0) {
        Some(gear + 1)
    } else if gear >= 3 {
        let wheel_rpm = 0x109A * (c.wheel_spin >> 8);
        let down_rpm = div(shr64(mul64(wheel_rpm, ratio(gear - 1)), 30), 6);
        let now = torque(c, c.revs);
        let down = torque(c, down_rpm);
        (down.wrapping_mul(ratio(gear - 1)) > ratio(gear).wrapping_mul(now)).then_some(gear - 1)
    } else {
        None
    };
    let player = w.is_player(i);
    if let Some(new) = shift
        && w.g.phase != 9
    {
        if gear != new {
            gear_changed(w, player);
        }
        car!(w, i).gear = new;
    }
    car!(w, i).gearbox_pause = 5;
}

/// `FUN_0813d1f0`: one step of the car: controls, engine and drivetrain, tyres, collisions, integration.
pub fn dynamics(w: &mut CarWorld, i: usize, input: u32, frame_time: i32) {
    let (data, rom) = (w.data, w.rom);
    let handling = &data.car.handling[ent!(w, i).car as usize];
    let index = ent!(w, i).index as u32;
    let player = index == w.g.player;
    let held = input & 0xFFFF;
    let old_sector;
    let pressed;
    {
        old_sector = ent!(w, i).sector;

        // Off-route warning: far from the racing line while going fast.
        let (wp, seg) = route::advance(w, ent!(w, i).segment as u32, ent!(w, i).waypoint as i32 + 1);
        let line = wp + w.route.line.sections[seg as usize].first as i32;
        // (`line` is added to the section's first waypoint again: the game's own arithmetic)
        let at = route::waypoint(w, seg, line);
        let pt = w.route.line.points[at];
        let pos = car!(w, i).body.pos;
        let dx = pt.x.wrapping_sub(pos[0] >> 8);
        let dz = pt.z.wrapping_sub(pos[2] >> 8);
        let mut off = 0;
        if dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz)) > 0x17_8000 {
            off = w.route.planes[line as usize][2];
        }
        w.g.off_route = 0;
        if div(car!(w, i).speed, 0x2393) > 0x3C {
            if off > 0xA00 {
                w.g.off_route = -1;
            }
            if off < -0xA00 {
                w.g.off_route = 1;
            }
        }

        pressed = input & !(car!(w, i).prev_control as u32) & 0xFFFF;
        car!(w, i).prev_control = input as u16;
        if w.control(held, pressed, WINGMAN) {
            route::wingman_command(w);
        }
        let c = &car!(w, i);
        if c.tipped > 100 && c.airborne == 0 && c.speed <= 0x7FFF {
            // Tipped over for more than 100 steps with a corner down, and slow: back onto the racing line.
            let wp = route::waypoint_at(w, ent!(w, i).segment as u32, ent!(w, i).waypoint as i32);
            put_back_on_road(w, i, wp);
            car!(w, i).tipped = 0;
        }
        route::track_segment(w, i);
        if w.camera_player == i as u32 && route::traffic_wants_spawn(w) {
            // `FUN_08143b2c`'s spawn: near the entity the camera follows.
            let near = w.g.focus as usize;
            let made = crate::traffic::spawn(w, near, 1).is_some();
            route::traffic_spawned(w, made);
        }
    }
    car!(w, i).gearbox_pause -= 1;
    w.g.binding_set = (w.g.automatic == 0) as u8;
    let dt = recip(rom, frame_time << 8).min(0xC00);
    let ratio = car!(w, i).gear_ratios[car!(w, i).gear as usize];
    car!(w, i).brake = 0;
    let timer = car!(w, i).torque_timer - w.g.dt;
    car!(w, i).torque_timer = timer;
    if timer < 0 {
        car!(w, i).torque_multiplier = 0x8000;
        car!(w, i).torque_timer = 0;
    }

    // Steering: raw LEFT/RIGHT bits, not the bindings.
    let c = &mut car!(w, i);
    let mut steer_rate = 0;
    if input & 0x20 == 0 {
        if input & 0x10 == 0 {
            c.steering = 0;
        } else {
            if c.steering < 0 {
                c.steering = 0;
            }
            if c.steering <= 0x7_FFFF {
                steer_rate = 0xA000;
            }
        }
    } else {
        if c.steering > 0 {
            c.steering = 0;
        }
        if c.steering > -0x8_0000 {
            steer_rate = -0xA000;
        }
    }
    c.steering += steer_rate;

    // Speed along the car's forward axis (signed and magnitude), race statistics.
    let fwd = [c.body.rot[6], c.body.rot[7], c.body.rot[8]];
    let forward = dot(c.body.vel, fwd);
    c.forward_speed = forward * 0x20;
    let speed = if forward * 0x20 < 0 {
        forward * -0x20
    } else {
        forward * 0x20
    };
    c.speed = speed;
    w.profile.distance += speed >> 12;
    if w.profile.top_speed < speed {
        w.profile.top_speed = speed;
    }

    // Throttle, brake and the reverse gear; the pedals are frozen once the race is over.
    if w.g.race_over == 0 {
        let throttle = if w.g.automatic == 0 {
            car!(w, i).brake = w.control(held, pressed, BRAKE) as i32;
            if w.control(held, pressed, ACCELERATE) {
                if player {
                    w.profile.accelerating = 1;
                }
                car!(w, i).throttle + 0x2000
            } else {
                release_accelerator(w, player);
                0
            }
        } else if car!(w, i).gear == 0 {
            // In reverse: accelerate shifts into first gear (2) at low speed; brake is the throttle.
            if w.control(held, pressed, ACCELERATE) {
                if player {
                    w.profile.accelerating = 1;
                }
                if car!(w, i).speed <= 0x2FFF && w.g.phase != 9 {
                    if car!(w, i).gear != 2 {
                        gear_changed(w, player);
                    }
                    car!(w, i).gear = 2;
                }
                car!(w, i).brake = 1;
            } else if w.profile.accelerating != 0 && player {
                w.profile.accelerating = 0;
                w.profile.gear_changed = 1;
            }
            if !w.control(held, pressed, BRAKE) || car!(w, i).brake != 0 {
                0
            } else {
                car!(w, i).throttle + 0x2000
            }
        } else {
            // Forward gears: brake at low speed shifts into reverse (0).
            if w.control(held, pressed, BRAKE) {
                let accelerating = w.control(held, pressed, ACCELERATE);
                if car!(w, i).speed <= 0x2FFF && !accelerating && w.g.phase != 9 {
                    if car!(w, i).gear != 0 {
                        gear_changed(w, player);
                    }
                    car!(w, i).gear = 0;
                }
                car!(w, i).brake = 1;
            }
            if w.control(held, pressed, ACCELERATE) {
                if player {
                    w.profile.accelerating = 1;
                }
                car!(w, i).throttle + 0x2000
            } else {
                release_accelerator(w, player);
                0
            }
        };
        car!(w, i).throttle = throttle;
    }

    // Nitro.
    let nitro_on = w.control(held, pressed, NITRO) && car!(w, i).nitro_tank >= 2 && car!(w, i).gear >= 2;
    if nitro_on {
        if car!(w, i).nitro_on == 0 && player {
            w.sounds.push(Command::Stop(0x22));
            w.sounds.push(Command::Play(0x22));
        }
        car!(w, i).nitro_on = 1;
    } else if car!(w, i).nitro_on != 0 {
        car!(w, i).nitro_on = 0;
        if player {
            w.sounds.push(Command::Stop(0x22));
            w.sounds.push(Command::Play(0x23));
        }
    }
    if nitro_on && car!(w, i).nitro_on != 0 && car!(w, i).nitro_tank == 0 {
        car!(w, i).nitro_on = 0;
        if player {
            w.sounds.push(Command::Stop(0x22));
            w.sounds.push(Command::Play(0x23));
        }
    }

    // Slip angle: velocity direction against the car's heading.
    let c = &mut car!(w, i);
    let mut slip = atan2(c.body.vel[0], c.body.vel[2]) - atan2(c.body.rot[6], c.body.rot[8]);
    if slip > 0x2000 {
        slip -= 0x4000;
    }
    if slip < -0x2000 {
        slip += 0x4000;
    }
    let yaw = c.body.ang_vel[1] * 0x10;
    if c.route_flags & 0x10 == 0 && yaw.wrapping_mul(slip) > 0 {
        // Counter-steer damping of the yaw rate (angular momentum y).
        let k = data.car.slip_damping.eval(slip.abs());
        let damp = mul12(mul12(yaw, handling[0x52]), k >> 1);
        c.body.ang_momentum[1] -= damp;
    }
    // Rear grip from slip angle and yaw rate.
    let x = 0x733 * slip.abs() + yaw.abs() * 0x8CD >> 12;
    let g = data.car.rear_grip.eval(x) - 0x1000;
    c.wheels[0].grip = c.wheels[0].base_grip;
    c.wheels[1].grip = c.wheels[1].base_grip;
    let rear = mul12((g >> 1) + 0x1000, c.wheels[2].base_grip);
    c.wheels[2].grip = rear;
    c.wheels[3].grip = rear;

    // Speed from the normalised velocity (the direction itself goes unused, see the "drag" below).
    let speed_len = normalize(rom, &mut c.body.vel.clone());
    if speed_len < 0x10 {
        c.stationary = c.stationary.wrapping_add(1);
    } else {
        c.stationary = 0;
    }
    let drag_speed = speed_len >> 3;

    // Rev-limiter and neutral handling.
    if c.gear == 1 {
        if c.revs == c.max_rpm {
            c.launch = 0xF;
        } else {
            c.launch = 0;
            let half = c.max_rpm >> 1;
            let d = half - c.revs;
            let near = if d < 0 { c.revs - half <= 0x12B } else { d <= 0x12B };
            c.neutral_torque = if near { 0x1800 } else { 0x1000 };
        }
    } else if c.launch != 0 {
        if speed_len < 0x801 && c.revs > 0x1387 {
            c.launch -= 1;
            for wheel in &mut c.wheels {
                wheel.grip = 0x80;
            }
        } else {
            c.launch = 0;
        }
    }

    // Engine rpm from throttle or from the wheels through the gear ratio.
    if c.throttle > 0x9999 {
        c.throttle = 0x9999;
    }
    let mut engine_brake = 0;
    let mut rpm_step = if ratio == 0 {
        c.throttle * 2 - 300
    } else {
        let wheel_rpm = 0x109A * (c.wheel_spin >> 8);
        div(shr64(mul64(wheel_rpm, ratio), 30), 6) - c.revs
    };
    if rpm_step > 1000 {
        engine_brake = (-ratio >> 4).wrapping_mul(rpm_step - 1000) >> 9;
        rpm_step = 1000;
    }
    if rpm_step < -1000 {
        engine_brake = -((-ratio >> 4).wrapping_mul(1000 - rpm_step) >> 9);
        rpm_step = -1000;
    }
    let rpm = c.revs + rpm_step;
    c.revs = rpm.min(c.max_rpm);
    if c.revs < handling[0x1A] {
        c.revs = handling[0x1A];
    }
    if c.neutral_torque > 0x1000 {
        c.neutral_torque -= 0x80;
    }
    let rpm = c.revs;
    let tq = torque(c, rpm);
    let mut t = if rpm == c.max_rpm {
        c.neutral_torque * tq >> 13
    } else {
        c.neutral_torque * tq >> 12
    };
    if c.nitro_on != 0 {
        t = (t as u32).wrapping_mul(c.nitro_torque as u32) as i32 >> 12;
    }
    let drive = shr64(mul64(t.wrapping_mul(c.throttle), ratio), 15);
    let drive = shr64(mul64(drive, c.torque_scale), 15);
    let mut drive = shr64(mul64(drive, c.torque_multiplier), 15);
    let gear = c.gear;
    if gear >= 2 {
        drive = drive.wrapping_sub(rpm.wrapping_mul(c.engine_braking));
    } else if gear < 1 {
        drive = drive.wrapping_add(rpm.wrapping_mul(c.engine_braking));
    }
    let handbrake = w.control(held, pressed, HANDBRAKE);
    let torque_in = drive >> 10;
    let c = &mut car!(w, i);
    if handbrake {
        for wheel in &mut c.wheels[2..] {
            wheel.grip >>= 1;
            let spin = wheel.spin;
            let brake = wheel.brake * 4;
            let v = if spin < 0 { spin + brake } else { spin - brake };
            wheel.spin = if (spin < 0 && v > 0) || (spin >= 0 && v < 0) {
                0
            } else {
                v
            };
        }
    }
    for wheel in &mut c.wheels {
        let spin = wheel.spin + ((torque_in + engine_brake).wrapping_mul(wheel.drive >> 4) >> 8);
        wheel.spin = spin;
        if c.brake > 0 {
            let brake = wheel.brake;
            let v = if spin < 0 { spin + brake } else { spin - brake };
            wheel.spin = if (spin < 0 && v > 0) || (spin >= 0 && v < 0) {
                0
            } else {
                v
            };
        }
    }

    // Sector before the step.
    let e = &ent!(w, i);
    w.query.sector = e.sector;
    let s = w.find_sector(e.sector as u32, e.pos[0], e.pos[1], e.pos[2]);
    ent!(w, i).sector = if s & 0xFFFF == NONE { old_sector as u32 } else { s } as u16;
    w.g.settled = 1;
    let pull = dt * (w.g.gravity * car!(w, i).body.mass >> 12) >> 11;
    car!(w, i).body.momentum[1] += pull;

    // Ground contact, or the body corners when the car is tipped over (then the wheels stop and `tipped` counts).
    let grounded = if car!(w, i).body.rot[4] < 0xF21 {
        contact::tipped(w, i, dt);
        let c = &mut car!(w, i);
        for wheel in &mut c.wheels {
            wheel.spin = 0;
        }
        c.tipped = c.tipped.wrapping_add(1);
        0
    } else {
        let n = contact::wheels(w, i, dt);
        car!(w, i).tipped = 0;
        n
    };
    walls::racers(w, i, dt);
    if speed_len < 0xA0
        && grounded == 4
        && car!(w, i).contact & 1 == 0
        && !w.control(held, pressed, ACCELERATE)
        && !w.control(held, pressed, BRAKE)
    {
        // Parked: stop all motion.
        let rest = data.car.rest;
        let c = &mut car!(w, i);
        c.body.momentum = rest;
        c.body.vel = rest;
        c.body.ang_momentum = rest;
        c.body.ang_vel = rest;
        for wheel in &mut c.wheels {
            wheel.spin = 0;
        }
    }
    car!(w, i).contact &= 8;
    walls::collide(w, i);
    let start = ent!(w, i).pos;
    let c = &mut car!(w, i);
    body::integrate(rom, &mut c.body, dt);
    body::integrate(rom, &mut c.body, dt);

    // Entity position: body position minus the rotated centre-of-mass offset.
    let offset = [0, c.centre_of_mass[1], c.centre_of_mass[2]];
    let offset = mat_mul(offset, &c.body.rot);
    ent!(w, i).pos = sub(car!(w, i).body.pos, offset);
    let sector = ent!(w, i).sector as u32;
    w.query.sector = sector as u16;
    let pos = ent!(w, i).pos;
    let s = w.find_sector(sector, pos[0], pos[1], pos[2]);
    ent!(w, i).sector = s as u16;
    if s as u16 as u32 == NONE {
        // Out of every sector: halve the step's move up to 6 times, searching from the sector the step started
        // in, then pull the car back 0x6400 along the move and rebuild the body position.
        let mut mv = sub(ent!(w, i).pos, start);
        for _ in 0..6 {
            mv = scale(mv, 0x800);
            w.query.sector = old_sector;
            ent!(w, i).sector = old_sector;
            ent!(w, i).pos = add(mv, start);
            let p = ent!(w, i).pos;
            let s = w.find_sector(old_sector as u32, p[0], p[1], p[2]);
            ent!(w, i).sector = s as u16;
            if s as u16 as u32 != NONE {
                break;
            }
        }
        normalize(rom, &mut mv);
        ent!(w, i).pos = sub(ent!(w, i).pos, scale(mv, 0x6400));
        car!(w, i).body.pos = add(ent!(w, i).pos, offset);
        if ent!(w, i).sector as u32 == NONE {
            ent!(w, i).sector = old_sector;
        }
    }

    // "Drag": the game scales the vector in the stack slot that held the normalised velocity, but by now that
    // slot holds the rotated centre-of-mass offset, so the offset direction is what gets subtracted from the
    // velocity (reproduced as is). Then momentum from velocity.
    let c = &mut car!(w, i);
    let drag = scale(
        offset,
        (drag_speed * drag_speed >> 16).wrapping_mul(handling[0x47]) >> 6,
    );
    let v = sub(c.body.vel, drag);
    c.body.vel = v;
    c.body.momentum = scale(v, c.body.mass);
    c.wheel_angle += c.forward_speed;
    if player {
        w.g.u_0030 = 0;
    }
    let heading = atan2(c.body.rot[6] >> 4, c.body.rot[8] >> 4);
    c.heading = heading;
    c.heading_sign = heading >> 31;
    ent!(w, i).heading = heading << 8;
    if player {
        let c = &car!(w, i);
        let (rpm, top) = (c.revs, c.max_rpm);
        let r = if rpm < 0 {
            0
        } else if rpm > top {
            top
        } else {
            rpm
        };
        let pitch = div(r * 700, top);
        let effect = w.profile.engine_sound as i32 as u32;
        w.sounds.push(Command::Pitch(effect, pitch + 400));
    }
    let slip_sum = car!(w, i)
        .wheels
        .iter()
        .map(|wheel| wheel.slip)
        .fold(0i32, i32::wrapping_add)
        >> 2;
    if slip_sum > 0x3_D090 {
        w.profile.skids += 1;
        let mut sound = true;
        if w.profile.skid_sound[index as usize] == 0 {
            w.profile.skid_sound[index as usize] = 1;
            if !player {
                sound = false;
            } else {
                w.sounds.push(Command::Play(0x1C));
            }
        }
        if sound && player {
            let pitch = div(car!(w, i).speed, 0x1388);
            w.sounds.push(Command::Pitch(0x1C, pitch + 0x4B0));
        }
    } else {
        w.profile.skid_sound[index as usize] = 0;
        if player {
            w.sounds.push(Command::Stop(0x1C));
        }
    }

    // Gears: automatic, or the manual shift state machine on L/R.
    if w.g.automatic != 0 {
        if car!(w, i).gear != 0 {
            auto_shift(w, i);
        }
    } else {
        manual_shift(w, i, input, pressed);
    }
    route::track_waypoint(w, i);
    if ent!(w, i).race_state != 2 {
        car!(w, i).progress = route::progress(w, i);
    }
    route::gap(w, i);
    let lane = route::nearest_lane(w, route::lateral(w, i), -1);
    car!(w, i).lane = lane as u16;
    if w.g.mode == 2 {
        // `FUN_08140f78` (`hunter_life_tick`).
        let mut r = w.slots[i].racer();
        career::hunter_life_tick(&w.race(), &mut r);
        w.slots[i].set_racer(&r);
    }
}

/// `FUN_0814efa8`: put car `i` back on the road at waypoint `wp`: the entity on the floor there with the
/// waypoint's heading as is, the body 0x1900 above it, upright along the waypoint's line. Momenta and velocities
/// are kept.
pub(crate) fn put_back_on_road(w: &mut CarWorld, i: usize, wp: usize) {
    let e = &ent!(w, i);
    let (index, seg) = route::advance(w, e.segment as u32, e.waypoint as i32);
    let first = w.route.line.sections[seg as usize].first as i32;
    let (p, extra) = (w.route.line.points[wp], w.route.extra[wp]);
    let (x, z) = (p.x << 8, p.z << 8);
    let sector = extra.sector as u16;
    let y = w.floor_height(sector as u32, x >> 8, z >> 8);
    let line = w.route.planes[(index + first) as usize];
    let heading = atan2(line[0], line[1]);
    let e = &mut ent!(w, i);
    e.pos = [x, y, z];
    e.heading = extra.heading as i32;
    e.sector = sector;
    let rom = w.rom;
    let body = &mut car!(w, i).body;
    body.pos = [x, y.wrapping_sub(0x1900), z];
    crate::init::orient(rom, body, heading);
}

fn release_accelerator(w: &mut CarWorld, player: bool) {
    if player && w.profile.accelerating != 0 {
        w.profile.accelerating = 0;
        w.profile.gear_changed = 1;
    }
}

/// Manual gearbox: R shifts up, L down, both together wait for release (state in `g.shift_state`).
fn manual_shift(w: &mut CarWorld, i: usize, input: u32, pressed: u32) {
    let state = w.g.shift_state;
    let top_gear = w.data.car.handling[ent!(w, i).car as usize][0x15];
    let player = w.is_player(i);
    let next = match state {
        1 => {
            if input & 0x100 != 0 {
                3
            } else {
                if input & 0x200 != 0 {
                    return;
                }
                let gear = car!(w, i).gear;
                let g = (gear - 1).max(0);
                if w.g.phase != 9 {
                    if gear != g {
                        gear_changed(w, player);
                    }
                    car!(w, i).gear = g;
                }
                0
            }
        }
        0 => {
            if pressed & 0x300 == 0x300 {
                3
            } else if pressed & 0x100 != 0 {
                2
            } else if pressed & 0x200 != 0 {
                1
            } else {
                return;
            }
        }
        2 => {
            if input & 0x200 != 0 {
                3
            } else {
                if input & 0x100 != 0 {
                    return;
                }
                let gear = car!(w, i).gear;
                let g = (gear + 1).min(top_gear);
                if w.g.phase != 9 {
                    if gear != g {
                        gear_changed(w, player);
                    }
                    car!(w, i).gear = g;
                }
                0
            }
        }
        3 if input & 0x300 == 0 => 0,
        _ => return,
    };
    w.g.shift_state = next;
}
