//! The car entity's per-frame update: the handler (`FUN_0814bd4c`), the racing step (`FUN_0814b168`) and the
//! car dynamics (`FUN_0813d1f0`) with the engine, gearbox, steering, brakes and the order of the sub-steps.
//!
//! Entity (0xA4 bytes) fields used: `+0x00` index, `+0x02` sector-list link, `+0x08/+0x0A` flags, `+0x0C/+0x10/
//! +0x14` position (8.8 city units, `-y` up), `+0x2C` heading << 8, `+0x4A` state (0 init, 0x100 racing, 2
//! finished: set by `lap_crossing`),
//! `+0x72` route segment, `+0x78` sector, `+0x89` handling record, `+0x8C` physics struct, `+0x90` waypoint.
//! The physics struct (0x4FC bytes) is described in `docs/engine/physics.md`.

use crate::body;
use crate::contact;
use crate::math::{atan2, cos, div, dot, mat_mul, mul12, mul64, normalize, recip, scale, shr64, sin, sub};
use crate::mem::Mem;
use crate::route;
use crate::sound::Command;
use crate::walls;
use crate::world::{self, AUTOMATIC, DT, INPUT, NONE, PLAYER, PROFILE, RACE_PHASE, W_SEGMENTS, WORLD, control, entity};
use crate::{Result, Sim};
use nfsgba_formats::career::{self, Race, Racer};

/// Handling records (0x158 bytes), one per car: `+0x54` top gear, `+0x68` idle rpm, `+0x11C` drag,
/// `+0x148` yaw damping (`docs/engine/physics.md`).
pub const HANDLING: u32 = 0x087F_1100;

/// Control actions (binding table rows, `world::control`).
const ACCELERATE: u32 = 0;
const BRAKE: u32 = 1;
const HANDBRAKE: u32 = 6;
const NITRO: u32 = 7;
const WINGMAN: u32 = 8;

/// `FUN_0814bd4c`: the car handler (entity handler table 0x087F38B8, entries 0..3).
pub fn handler(sim: &mut Sim, e: u32) -> Result<()> {
    let index = sim.mem.u16(e) as u32;
    world::unlink_entity(&mut sim.mem, index);
    match sim.mem.u16(e + 0x4A) {
        2 => {
            // Finished: keep driving; the first finisher sets the race-over flag and starts the palette fade,
            // and once the fade is done the race ends (phase 3) with this car's index.
            racing_step(sim, e)?;
            let m = &mut sim.mem;
            if m.i32(0x0300_5780) == 0 {
                m.set_i32(0x0300_5780, 1);
                m.set_i32(0x0300_5630, -0x10);
            }
            if m.i32(0x0300_5624) == 0 && m.i32(0x0300_5780) != 0 && m.i32(0x0300_5630) == 0 {
                m.set_i32(RACE_PHASE, 3);
                m.set_u32(0x0300_57D0, index);
            }
        }
        0 => crate::init::car_init(sim, e)?,
        0x100 => racing_step(sim, e)?,
        _ => {}
    }
    world::link_entity(&mut sim.mem, index);
    Ok(())
}

/// `FUN_0814b168`: nitro drain, dynamics, visibility flags, and (unless 0x0300610C is set) the suspension
/// step `FUN_0814de40` with the body height from the four wheels.
fn racing_step(sim: &mut Sim, e: u32) -> Result<()> {
    let m = &sim.mem;
    let dt = m.i32(DT);
    let phase = m.i32(RACE_PHASE);
    if phase == 0 {
        return Ok(());
    }
    let index = m.u16(e) as u32;
    if phase != 1 && phase != 4 {
        // NOT 1:1 (rendering): for the player, seen from the side, the game redraws the decal onto the car's
        // texture atlas here (`draw_decal_on_atlas`, `FUN_0813bd90`). That belongs to the renderer.
        nitro(&mut sim.mem, e);
        let input = sim.mem.u16(INPUT + index * 2) as u32;
        dynamics(sim, e, input, dt)?;
    }
    let m = &mut sim.mem;
    if m.u32(0x0300_57F8) == index {
        m.set_u16(e + 0xA, m.u16(e + 0xA) | 1);
        if m.u32(0x0300_55F8) > 1 {
            m.set_u16(e + 8, m.u16(e + 8) | 4);
        } else {
            m.set_u16(e + 8, m.u16(e + 8) & 0xFFFB);
        }
    } else {
        m.set_u16(e + 0xA, m.u16(e + 0xA) & 0xFFFE);
        m.set_u16(e + 8, m.u16(e + 8) | 4);
    }
    if m.i32(0x0300_610C) != 0 {
        return Ok(());
    }
    // Unreachable in Carbon: `race_init` and every dynamics step set 0x0300610C to 1 (no trace step has it 0).
    // Four points around the car; their y is uninitialised stack in the game, but always overwritten with the
    // floor height (the sector query falls back to the car's sector, never 0xFFFF at a car step).
    let mut pts = [[-0x20, 0, 0x55], [0x20, 0, 0x55], [-0x20, 0, -0x2A], [0x20, 0, -0x2A]];
    let hits = contact::suspension(m, e, &mut pts, &mut [0; 4], dt);
    let p = m.u32(e + 0x8C);
    m.set_i32(p + 0x48, hits);
    let front = m.i32(p + 0x6C).wrapping_add(m.i32(p + 0x70)) >> 1;
    let rear = m.i32(p + 0x74).wrapping_add(m.i32(p + 0x78)) >> 1;
    m.set_i32(e + 0x10, front.wrapping_add(rear) >> 1);
    Ok(())
}

/// `FUN_0814b098`: drain the nitro tank (`+0x4C8`) while nitro is on (`+0x4D1`).
pub(crate) fn nitro(m: &mut Mem, e: u32) {
    let rate = div(600, div(m.i32(DT) << 6, 0x1C));
    let p = m.u32(e + 0x8C);
    if m.u8(p + 0x4D1) == 0 {
        return;
    }
    // The IWRAM division's quotient is discarded here; only its remainder store (0x03006480) is kept.
    let factor = m.u16(p + 0x4CC);
    if factor == 0 {
        world::iwram_divmod(m, m.i32(p + 0x4C8), 1, 0x0300_6480);
    } else {
        world::iwram_divmod(m, m.i32(p + 0x4C8), factor as i32, 0x0300_6480);
    }
    let divisor = world::iwram_divmod(m, rate << 16, 0x1500, 0x0300_6480);
    let drain = div((factor as i32) << 8, divisor);
    if m.i32(0x0300_6150) == 0 {
        m.set_i32(p + 0x4C8, m.i32(p + 0x4C8) - drain);
    }
    if m.i32(p + 0x4C8) < 0 {
        m.set_i32(p + 0x4C8, 0);
    }
}

/// A piecewise-linear curve: `+0x00` point count, `+0x04` x of the first point, `+0x08` x of the last, `+0x0C`
/// pointer to the y values (`FUN_0813d1f0` inlines this for the tables at 0x087F4164 and 0x087F41A0).
pub(crate) fn curve(m: &Mem, table: u32, x: i32) -> i32 {
    let (count, x0, x1, ys) = (m.i32(table), m.i32(table + 4), m.i32(table + 8), m.u32(table + 0xC));
    let step = div(x1 - x0, count - 1);
    let k = div(x - x0, step);
    if k < 1 {
        m.i32(ys)
    } else if k < count {
        let y = m.i32(ys + k as u32 * 4);
        y + div((m.i32(ys + k as u32 * 4 + 4) - y).wrapping_mul(x - step * k), step)
    } else {
        m.i32(ys + count as u32 * 4 - 4)
    }
}

/// Engine torque at `rpm` from the car's torque curve (`+0x464`, 10 points over 0..`+0x454` rpm).
pub fn torque(m: &Mem, p: u32, rpm: i32) -> i32 {
    let step = m.i32(p + 0x454) >> 3;
    let k = div(rpm, step);
    if k < 9 {
        let t = p + 0x464;
        let y = m.i32(t.wrapping_add((k * 4) as u32));
        y + div(
            (m.i32(t.wrapping_add((k * 4 + 4) as u32)) - y).wrapping_mul(rpm - step * k),
            step,
        )
    } else {
        0
    }
}

fn player_physics(m: &Mem) -> u32 {
    m.u32(entity(m, m.u32(PLAYER)) + 0x8C)
}

/// Flag a gear change for the HUD (profile `+0x2E0`) when it is the player's car.
fn gear_changed(m: &mut Mem, p: u32) {
    if p == player_physics(m) {
        let s = m.u32(PROFILE);
        m.set_i32(s + 0x2E0, 1);
    }
}

/// `FUN_0813c02c`: the automatic gearbox, at most one shift every 5 steps (`+0x9C`).
pub(crate) fn auto_shift(m: &mut Mem, e: u32) {
    let p = m.u32(e + 0x8C);
    if m.i32(p + 0x9C) > 0 {
        return;
    }
    let gear = m.i32(p + 0x40);
    if m.i32(p + 0x444) != 0 && gear != 1 {
        return;
    }
    let handling = HANDLING + m.u8(e + 0x89) as u32 * 0x158;
    let ratio = |m: &Mem, g: i32| m.i32((p + 0x408).wrapping_add((g * 4) as u32));
    let shift = if gear == 1 || (m.i32(p + 0x44C) <= m.i32(p + 0x3C) && gear < m.i32(handling + 0x54) && gear != 0) {
        Some(gear + 1)
    } else if gear >= 3 {
        let wheel_rpm = 0x109A * (m.i32(p + 0x3DC) >> 8);
        let down_rpm = div(shr64(mul64(wheel_rpm, ratio(m, gear - 1)), 30), 6);
        let now = torque(m, p, m.i32(p + 0x3C));
        let down = torque(m, p, down_rpm);
        (down.wrapping_mul(ratio(m, gear - 1)) > ratio(m, gear).wrapping_mul(now)).then_some(gear - 1)
    } else {
        None
    };
    if let Some(new) = shift
        && m.i32(RACE_PHASE) != 9
    {
        if gear != new {
            gear_changed(m, p);
        }
        m.set_i32(p + 0x40, new);
    }
    m.set_i32(p + 0x9C, 5);
}

/// `FUN_0813d1f0`: one step of the car: controls, engine and drivetrain, tyres, collisions, integration.
pub fn dynamics(sim: &mut Sim, e: u32, input: u32, frame_time: i32) -> Result<()> {
    let m = &mut sim.mem;
    let handling = HANDLING + m.u8(e + 0x89) as u32 * 0x158;
    let p = m.u32(e + 0x8C);
    let b = p + 0xC8;
    let index = m.u16(e) as u32;
    let player = route::is_player(m, index);
    let old_sector = m.u16(e + 0x78);

    // Off-route warning (0x0300601C): far from the racing line while going fast.
    let (wp, seg) = route::advance(m, m.u16(e + 0x72) as u32, m.i16(e + 0x90) as i32 + 1);
    let line = wp + m.i32(m.u32(W_SEGMENTS) + seg * 8 + 4);
    let w = route::waypoint(m, seg, line);
    let dx = m.i32(w).wrapping_sub(m.i32(p + 0xD0) >> 8);
    let dz = m.i32(w + 4).wrapping_sub(m.i32(p + 0xD8) >> 8);
    let mut off = 0;
    if dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz)) > 0x17_8000 {
        off = m.i32(m.u32(0x0300_5FB4).wrapping_add((line as u32).wrapping_mul(0x20)) + 8);
    }
    m.set_i32(0x0300_601C, 0);
    if div(m.i32(p + 0x44), 0x2393) > 0x3C {
        if off > 0xA00 {
            m.set_i32(0x0300_601C, -1);
        }
        if off < -0xA00 {
            m.set_i32(0x0300_601C, 1);
        }
    }

    let pressed = input & !(m.u16(p + 0x4AE) as u32) & 0xFFFF;
    m.set_u16(p + 0x4AE, input as u16);
    let held = input & 0xFFFF;
    if control(m, held, pressed, WINGMAN) {
        route::wingman_command(m)?;
    }
    if m.i16(p + 0x4E4) > 100 && m.i16(p + 0x4E6) == 0 && m.i32(p + 0x44) <= 0x7FFF {
        // Tipped over for more than 100 steps with a corner down, and slow: back onto the racing line.
        let w = route::waypoint_at(m, m.u16(e + 0x72) as u32, m.i16(e + 0x90) as i32);
        put_back_on_road(m, e, w);
        m.set_i16(p + 0x4E4, 0);
    }
    route::track_segment(m, e);
    if m.u32(0x0300_53AC) == e {
        route::traffic_countdown(m)?;
    }
    m.set_i32(p + 0x9C, m.i32(p + 0x9C) - 1);
    world::select_bindings(m, m.i32(AUTOMATIC));
    let dt = recip(m, frame_time << 8).min(0xC00);
    let ratio = m.i32((p + 0x408).wrapping_add((m.i32(p + 0x40) * 4) as u32));
    m.set_i32(p + 0x28, 0);
    let timer = m.i32(p + 0x42C) - m.i32(DT);
    m.set_i32(p + 0x42C, timer);
    if timer < 0 {
        m.set_i32(p + 0x428, 0x8000);
        m.set_i32(p + 0x42C, 0);
    }

    // Steering (+0x20): raw LEFT/RIGHT bits, not the bindings.
    let mut steer_rate = 0;
    if input & 0x20 == 0 {
        if input & 0x10 == 0 {
            m.set_i32(p + 0x20, 0);
        } else {
            if m.i32(p + 0x20) < 0 {
                m.set_i32(p + 0x20, 0);
            }
            if m.i32(p + 0x20) <= 0x7_FFFF {
                steer_rate = 0xA000;
            }
        }
    } else {
        if m.i32(p + 0x20) > 0 {
            m.set_i32(p + 0x20, 0);
        }
        if m.i32(p + 0x20) > -0x8_0000 {
            steer_rate = -0xA000;
        }
    }
    m.set_i32(p + 0x20, m.i32(p + 0x20) + steer_rate);

    // Speed along the car's forward axis (+0xA0 signed, +0x44 magnitude), race statistics.
    let forward = dot(m.vec3(p + 0x11C), m.vec3(p + 0x140));
    m.set_i32(p + 0xA0, forward * 0x20);
    let speed = if forward * 0x20 < 0 {
        forward * -0x20
    } else {
        forward * 0x20
    };
    m.set_i32(p + 0x44, speed);
    let stats = m.u32(PROFILE);
    m.set_i32(stats + 0x2D0, m.i32(stats + 0x2D0) + (speed >> 12));
    if m.i32(stats + 0x2DC) < m.i32(p + 0x44) {
        m.set_i32(stats + 0x2DC, m.i32(p + 0x44));
    }

    // Throttle (+0x24), brake (+0x28) and the reverse gear; the pedals are frozen once the race is over
    // (0x03005780).
    if m.i32(0x0300_5780) == 0 {
        let throttle = if m.i32(AUTOMATIC) == 0 {
            m.set_i32(p + 0x28, control(m, held, pressed, BRAKE) as i32);
            if control(m, held, pressed, ACCELERATE) {
                if player {
                    m.set_i32(stats + 0x2E8, 1);
                }
                m.i32(p + 0x24) + 0x2000
            } else {
                release_accelerator(m, player, stats);
                0
            }
        } else if m.i32(p + 0x40) == 0 {
            // In reverse: accelerate shifts into first gear (2) at low speed; brake is the throttle.
            if control(m, held, pressed, ACCELERATE) {
                if player {
                    m.set_i32(stats + 0x2E8, 1);
                }
                if m.i32(p + 0x44) <= 0x2FFF && m.i32(RACE_PHASE) != 9 {
                    if m.i32(p + 0x40) != 2 && p == player_physics(m) {
                        m.set_i32(stats + 0x2E0, 1);
                    }
                    m.set_i32(p + 0x40, 2);
                }
                m.set_i32(p + 0x28, 1);
            } else if m.i32(stats + 0x2E8) != 0 && player {
                m.set_i32(stats + 0x2E8, 0);
                m.set_i32(stats + 0x2E0, 1);
            }
            if !control(m, held, pressed, BRAKE) || m.i32(p + 0x28) != 0 {
                0
            } else {
                m.i32(p + 0x24) + 0x2000
            }
        } else {
            // Forward gears: brake at low speed shifts into reverse (0).
            if control(m, held, pressed, BRAKE) {
                let accelerating = control(m, held, pressed, ACCELERATE);
                if m.i32(p + 0x44) <= 0x2FFF && !accelerating && m.i32(RACE_PHASE) != 9 {
                    if m.i32(p + 0x40) != 0 && p == player_physics(m) {
                        m.set_i32(stats + 0x2E0, 1);
                    }
                    m.set_i32(p + 0x40, 0);
                }
                m.set_i32(p + 0x28, 1);
            }
            if control(m, held, pressed, ACCELERATE) {
                if player {
                    m.set_i32(stats + 0x2E8, 1);
                }
                m.i32(p + 0x24) + 0x2000
            } else {
                release_accelerator(m, player, stats);
                0
            }
        };
        m.set_i32(p + 0x24, throttle);
    }

    // Nitro (+0x4D1 on, +0x4C8 tank).
    let nitro_on = control(m, held, pressed, NITRO) && m.i32(p + 0x4C8) >= 2 && m.i32(p + 0x40) >= 2;
    if nitro_on {
        if m.u8(p + 0x4D1) == 0 && player {
            sim.sounds.push(Command::Stop(0x22));
            sim.sounds.push(Command::Play(0x22));
        }
        sim.mem.set_u8(p + 0x4D1, 1);
    } else if sim.mem.u8(p + 0x4D1) != 0 {
        sim.mem.set_u8(p + 0x4D1, 0);
        if player {
            sim.sounds.push(Command::Stop(0x22));
            sim.sounds.push(Command::Play(0x23));
        }
    }
    let m = &mut sim.mem;
    if nitro_on && m.u8(p + 0x4D1) != 0 && m.i32(p + 0x4C8) == 0 {
        m.set_u8(p + 0x4D1, 0);
        if player {
            sim.sounds.push(Command::Stop(0x22));
            sim.sounds.push(Command::Play(0x23));
        }
    }
    let m = &mut sim.mem;

    // Slip angle: velocity direction against the car's heading.
    let mut slip = atan2(m.i32(p + 0x11C), m.i32(p + 0x124)) - atan2(m.i32(p + 0x140), m.i32(p + 0x148));
    if slip > 0x2000 {
        slip -= 0x4000;
    }
    if slip < -0x2000 {
        slip += 0x4000;
    }
    let yaw = m.i32(p + 0x15C) * 0x10;
    if m.u16(p + 0x4D8) & 0x10 == 0 && yaw.wrapping_mul(slip) > 0 {
        // Counter-steer damping of the yaw rate (+0x114 angular momentum y).
        let k = curve(m, 0x087F_4164, slip.abs());
        let damp = mul12(mul12(yaw, m.i32(handling + 0x148)), k >> 1);
        m.set_i32(p + 0x114, m.i32(p + 0x114) - damp);
    }
    // Rear grip from slip angle and yaw rate.
    let x = 0x733 * slip.abs() + yaw.abs() * 0x8CD >> 12;
    let g = curve(m, 0x087F_41A0, x) - 0x1000;
    m.set_i32(p + 0x20C, m.i32(p + 0x210));
    m.set_i32(p + 0x2A0, m.i32(p + 0x2A4));
    let rear = mul12((g >> 1) + 0x1000, m.i32(p + 0x338));
    m.set_i32(p + 0x334, rear);
    m.set_i32(p + 0x3C8, rear);

    // Speed from the normalised velocity (the direction itself goes unused, see the "drag" below).
    let speed_len = normalize(m, &mut m.vec3(p + 0x11C));
    if speed_len < 0x10 {
        m.set_i16(p + 0x4EE, m.i16(p + 0x4EE) + 1);
    } else {
        m.set_i16(p + 0x4EE, 0);
    }
    let drag_speed = speed_len >> 3;

    // Rev-limiter and neutral handling.
    if m.i32(p + 0x40) == 1 {
        if m.i32(p + 0x3C) == m.i32(p + 0x454) {
            m.set_i32(p + 0x444, 0xF);
        } else {
            m.set_i32(p + 0x444, 0);
            let half = m.i32(p + 0x454) >> 1;
            let d = half - m.i32(p + 0x3C);
            let near = if d < 0 {
                m.i32(p + 0x3C) - half <= 0x12B
            } else {
                d <= 0x12B
            };
            m.set_i32(p + 0x460, if near { 0x1800 } else { 0x1000 });
        }
    } else if m.i32(p + 0x444) != 0 {
        if speed_len < 0x801 && m.i32(p + 0x3C) > 0x1387 {
            m.set_i32(p + 0x444, m.i32(p + 0x444) - 1);
            for k in 0..4u32 {
                m.set_i32(p + 0x3C8 - 0x94 * k, 0x80);
            }
        } else {
            m.set_i32(p + 0x444, 0);
        }
    }

    // Engine rpm (+0x3C) from throttle or from the wheels through the gear ratio.
    if m.i32(p + 0x24) > 0x9999 {
        m.set_i32(p + 0x24, 0x9999);
    }
    let mut engine_brake = 0;
    let mut rpm_step = if ratio == 0 {
        m.i32(p + 0x24) * 2 - 300
    } else {
        let wheel_rpm = 0x109A * (m.i32(p + 0x3DC) >> 8);
        div(shr64(mul64(wheel_rpm, ratio), 30), 6) - m.i32(p + 0x3C)
    };
    if rpm_step > 1000 {
        engine_brake = (-ratio >> 4).wrapping_mul(rpm_step - 1000) >> 9;
        rpm_step = 1000;
    }
    if rpm_step < -1000 {
        engine_brake = -((-ratio >> 4).wrapping_mul(1000 - rpm_step) >> 9);
        rpm_step = -1000;
    }
    let rpm = m.i32(p + 0x3C) + rpm_step;
    m.set_i32(p + 0x3C, rpm.min(m.i32(p + 0x454)));
    if m.i32(p + 0x3C) < m.i32(handling + 0x68) {
        m.set_i32(p + 0x3C, m.i32(handling + 0x68));
    }
    if m.i32(p + 0x460) > 0x1000 {
        m.set_i32(p + 0x460, m.i32(p + 0x460) - 0x80);
    }
    let rpm = m.i32(p + 0x3C);
    let tq = torque(m, p, rpm);
    let mut t = if rpm == m.i32(p + 0x454) {
        m.i32(p + 0x460) * tq >> 13
    } else {
        m.i32(p + 0x460) * tq >> 12
    };
    if m.u8(p + 0x4D1) != 0 {
        t = (t as u32).wrapping_mul(m.u16(p + 0x4CE) as u32) as i32 >> 12;
    }
    let drive = shr64(mul64(t.wrapping_mul(m.i32(p + 0x24)), ratio), 15);
    let drive = shr64(mul64(drive, m.i32(p + 0x4BC)), 15);
    let mut drive = shr64(mul64(drive, m.i32(p + 0x428)), 15);
    let gear = m.i32(p + 0x40);
    if gear >= 2 {
        drive = drive.wrapping_sub(rpm.wrapping_mul(m.i32(p + 0x458)));
    } else if gear < 1 {
        drive = drive.wrapping_add(rpm.wrapping_mul(m.i32(p + 0x458)));
    }
    let handbrake = control(m, held, pressed, HANDBRAKE);
    let torque_in = drive >> 10;
    if handbrake {
        for k in 2..4u32 {
            let w = p + contact::WHEELS + contact::WHEEL_SIZE * k;
            m.set_i32(w + 0x80, m.i32(w + 0x80) >> 1);
            let spin = m.i32(w + 0x64);
            let brake = m.i32(w + 0x74) * 4;
            let v = if spin < 0 { spin + brake } else { spin - brake };
            m.set_i32(
                w + 0x64,
                if (spin < 0 && v > 0) || (spin >= 0 && v < 0) {
                    0
                } else {
                    v
                },
            );
        }
    }
    for k in 0..4u32 {
        let w = p + contact::WHEELS + contact::WHEEL_SIZE * k;
        let spin = m.i32(w + 0x64) + ((torque_in + engine_brake).wrapping_mul(m.i32(w + 0x70) >> 4) >> 8);
        m.set_i32(w + 0x64, spin);
        if m.i32(p + 0x28) > 0 {
            let brake = m.i32(w + 0x74);
            let v = if spin < 0 { spin + brake } else { spin - brake };
            m.set_i32(
                w + 0x64,
                if (spin < 0 && v > 0) || (spin >= 0 && v < 0) {
                    0
                } else {
                    v
                },
            );
        }
    }

    // Sector before the step.
    m.set_u16(WORLD + 0xEA, m.u16(e + 0x78));
    let s = world::find_sector(
        m,
        m.u16(e + 0x78) as u32,
        m.i32(e + 0xC),
        m.i32(e + 0x10),
        m.i32(e + 0x14),
    )?;
    m.set_u16(e + 0x78, if s & 0xFFFF == NONE { old_sector as u32 } else { s } as u16);
    m.set_i32(0x0300_610C, 1);
    m.set_i32(
        p + 0xFC,
        m.i32(p + 0xFC) + (dt * (m.i32(0x0300_6030) * m.i32(b) >> 12) >> 11),
    );

    // Ground contact, or the body corners when the car is tipped over (then the wheels stop and +0x4E4 counts).
    let grounded = if m.i32(p + 0x138) < 0xF21 {
        contact::tipped(sim, e, dt);
        let m = &mut sim.mem;
        for k in 0..4u32 {
            m.set_i32(p + contact::WHEELS + contact::WHEEL_SIZE * k + 0x64, 0);
        }
        m.set_i16(p + 0x4E4, m.i16(p + 0x4E4).wrapping_add(1));
        0
    } else {
        let n = contact::wheels(sim, e, dt)?;
        sim.mem.set_i16(p + 0x4E4, 0);
        n
    };
    walls::racers(sim, e, dt)?;
    let m = &mut sim.mem;
    if speed_len < 0xA0
        && grounded == 4
        && m.u32(p + 0x448) & 1 == 0
        && !control(m, held, pressed, ACCELERATE)
        && !control(m, held, pressed, BRAKE)
    {
        // Parked: stop all motion (the zero vector at 0x087F3DD0).
        let rest = [m.i32(0x087F_3DD0), m.i32(0x087F_3DD4), m.i32(0x087F_3DD8)];
        for a in [p + 0xF8, p + 0x11C, p + 0x110, p + 0x158] {
            m.set_vec3(a, rest);
        }
        for k in 0..4u32 {
            m.set_i32(p + contact::WHEELS + contact::WHEEL_SIZE * k + 0x64, 0);
        }
    }
    m.set_u32(p + 0x448, m.u32(p + 0x448) & 8);
    walls::collide(sim, e)?;
    let m = &mut sim.mem;
    let start = m.vec3(e + 0xC);
    body::integrate(m, b, dt);
    body::integrate(m, b, dt);

    // Entity position: body position minus the rotated centre-of-mass offset.
    let offset = [0, m.i32(p + 0x43C), m.i32(p + 0x440)];
    let rot: [i32; 9] = std::array::from_fn(|k| m.i32(p + 0x128 + 4 * k as u32));
    let offset = mat_mul(offset, &rot);
    m.set_vec3(e + 0xC, sub(m.vec3(p + 0xD0), offset));
    let sector = m.u16(e + 0x78) as u32;
    m.set_u16(WORLD + 0xEA, sector as u16);
    let s = world::find_sector(m, sector, m.i32(e + 0xC), m.i32(e + 0x10), m.i32(e + 0x14))?;
    m.set_u16(e + 0x78, s as u16);
    if s as u16 as u32 == NONE {
        // Out of every sector (0x0813DF98): halve the step's move up to 6 times, searching from the sector the
        // step started in, then pull the car back 0x6400 along the move and rebuild the body position.
        let mut mv = sub(m.vec3(e + 0xC), start);
        for _ in 0..6 {
            mv = scale(mv, 0x800);
            m.set_u16(WORLD + 0xEA, old_sector);
            m.set_u16(e + 0x78, old_sector);
            m.set_vec3(e + 0xC, crate::math::add(mv, start));
            let s = world::find_sector(m, old_sector as u32, m.i32(e + 0xC), m.i32(e + 0x10), m.i32(e + 0x14))?;
            m.set_u16(e + 0x78, s as u16);
            if s as u16 as u32 != NONE {
                break;
            }
        }
        normalize(m, &mut mv);
        m.set_vec3(e + 0xC, sub(m.vec3(e + 0xC), scale(mv, 0x6400)));
        m.set_vec3(p + 0xD0, crate::math::add(m.vec3(e + 0xC), offset));
        if m.u16(e + 0x78) as u32 == NONE {
            m.set_u16(e + 0x78, old_sector);
        }
    }

    // "Drag": the game scales the vector in the stack slot that held the normalised velocity, but by now
    // that slot holds the rotated centre-of-mass offset, so the offset direction is what gets subtracted
    // from the velocity (reproduced as is). Then momentum from velocity.
    let drag = scale(
        offset,
        (drag_speed * drag_speed >> 16).wrapping_mul(m.i32(handling + 0x11C)) >> 6,
    );
    let v = sub(m.vec3(p + 0x11C), drag);
    m.set_vec3(p + 0x11C, v);
    m.set_vec3(p + 0xF8, scale(v, m.i32(b)));
    m.set_i32(p + 0x90, m.i32(p + 0x90) + m.i32(p + 0xA0));
    if player {
        m.set_i32(0x0300_0030, 0);
    }
    let heading = atan2(m.i32(p + 0x140) >> 4, m.i32(p + 0x148) >> 4);
    m.set_i32(p, heading);
    m.set_i32(p + 4, heading >> 31);
    m.set_i32(e + 0x2C, heading << 8);
    if player {
        let (rpm, top) = (m.i32(p + 0x3C), m.i32(p + 0x454));
        let r = if rpm < 0 {
            0
        } else if rpm > top {
            top
        } else {
            rpm
        };
        let pitch = div(r * 700, top);
        let effect = m.i8(m.u32(PROFILE) + 0x2EF) as i32 as u32;
        sim.sounds.push(Command::Pitch(effect, pitch + 400));
    }
    let m = &mut sim.mem;
    let slip_sum = (0..4u32)
        .map(|k| m.i32(p + contact::WHEELS + contact::WHEEL_SIZE * k + 0x7C))
        .fold(0i32, i32::wrapping_add)
        >> 2;
    let stats = m.u32(PROFILE);
    if slip_sum > 0x3_D090 {
        m.set_i32(stats + 0x2D8, m.i32(stats + 0x2D8) + 1);
        let flag = stats + 0x318 + index * 4;
        let mut sound = true;
        if m.i32(flag) == 0 {
            m.set_i32(flag, 1);
            if !player {
                sound = false;
            } else {
                sim.sounds.push(Command::Play(0x1C));
            }
        }
        if sound && player {
            let pitch = div(sim.mem.i32(p + 0x44), 0x1388);
            sim.sounds.push(Command::Pitch(0x1C, pitch + 0x4B0));
        }
    } else {
        m.set_i32(stats + 0x318 + index * 4, 0);
        if player {
            sim.sounds.push(Command::Stop(0x1C));
        }
    }
    let m = &mut sim.mem;

    // Gears: automatic, or the manual shift state machine on L/R (0x03006074).
    if m.i32(AUTOMATIC) != 0 {
        if m.i32(p + 0x40) != 0 {
            auto_shift(m, e);
        }
    } else {
        manual_shift(m, p, handling, input, pressed);
    }
    route::track_waypoint(m, e)?;
    if m.u16(e + 0x4A) != 2 {
        let v = route::progress(m, e, p);
        m.set_i32(p + 0xAC, v);
    }
    route::gap(m, e);
    let lane = nearest_lane_of(m, e);
    m.set_u16(p + 0xC0, lane as u16);
    if m.i32(0x0300_56E0) == 2 {
        // `FUN_08140f78` (`hunter_life_tick`).
        let mut r = racer(m, e);
        career::hunter_life_tick(&race(m), &mut r);
        store_racer(m, e, &r);
    }
    Ok(())
}

/// The race globals of `nfsgba_formats::career::Race` from RAM.
pub fn race(m: &Mem) -> Race {
    let mut results = [0; 0x40];
    results.copy_from_slice(m.bytes(0x0300_5650, 0x40));
    Race {
        mode: m.u32(0x0300_56E0),
        lapped: m.i32(route::CIRCUIT) != 0,
        laps: m.i32(0x0300_56E4),
        opponents: m.u32(route::OPPONENTS),
        time: m.u32(0x0300_5800),
        finished: m.i32(0x0300_61A4) != 0,
        view: m.u32(0x0300_57F8),
        player: m.u32(PLAYER),
        difficulty: m.u32(0x0300_5608),
        state48: m.u32(RACE_PHASE),
        rand: m.u32(0x0300_64C8),
        wrong_way: m.i32(0x0300_5384) != 0,
        results,
    }
}

/// Writes back what the race rules change: someone finished (0x030061A4), the result bytes (0x03005650..), the
/// rand_table index.
pub fn store_race(m: &mut Mem, r: &Race) {
    if r.finished != (m.i32(0x0300_61A4) != 0) {
        m.set_i32(0x0300_61A4, r.finished as i32);
    }
    m.set_bytes(0x0300_5650, &r.results);
    m.set_u32(0x0300_64C8, r.rand);
}

/// Car `e` as `nfsgba_formats::career::Racer`: entity `+0x00` id, `+0x08` flags, `+0x0C/+0x14` position, `+0x4A`
/// state, `+0x72` section, `+0x90` segment; driver `+0xA8` place, `+0xAC` distance, `+0xB4/+0xB8/+0xBC` best lap,
/// lap start, finish, `+0xC5` laps left, `+0xF8..` knock-out words, `+0x444` side, `+0x4D6` section changed,
/// `+0x4D8` flags, `+0x4E8` life, `+0x4EC/+0x4EE/+0x4F0` wrong-way, wall and hit counters.
pub fn racer(m: &Mem, e: u32) -> Racer {
    let p = m.u32(e + 0x8C);
    Racer {
        id: m.u16(e),
        section: m.u16(e + 0x72),
        segment: m.i16(e + 0x90),
        state: m.u16(e + 0x4A),
        entity_flags: m.u16(e + 8),
        x: m.i32(e + 0xC),
        z: m.i32(e + 0x14),
        place: m.i32(p + 0xA8),
        distance: m.i32(p + 0xAC),
        best_lap: m.u32(p + 0xB4),
        lap_start: m.u32(p + 0xB8),
        finish: m.u32(p + 0xBC),
        laps_left: m.i8(p + 0xC5),
        flags: m.u16(p + 0x4D8),
        life: m.i32(p + 0x4E8),
        wrong_way: m.i16(p + 0x4EC),
        wall: m.i16(p + 0x4EE),
        hit: m.i16(p + 0x4F0),
        knockout: [m.u32(p + 0xF8), m.u32(p + 0xFC), m.u32(p + 0x100)],
        side: m.i32(p + 0x444),
        section_changed: m.u16(p + 0x4D6),
    }
}

/// Writes car `e`'s `Racer` fields back (unchanged ones rewrite the same bytes).
pub fn store_racer(m: &mut Mem, e: u32, r: &Racer) {
    let p = m.u32(e + 0x8C);
    m.set_u16(e + 0x72, r.section);
    m.set_i16(e + 0x90, r.segment);
    m.set_u16(e + 0x4A, r.state);
    m.set_u16(e + 8, r.entity_flags);
    m.set_i32(p + 0xA8, r.place);
    m.set_i32(p + 0xAC, r.distance);
    m.set_u32(p + 0xB4, r.best_lap);
    m.set_u32(p + 0xB8, r.lap_start);
    m.set_u32(p + 0xBC, r.finish);
    m.set_u8(p + 0xC5, r.laps_left as u8);
    m.set_u16(p + 0x4D8, r.flags);
    m.set_i32(p + 0x4E8, r.life);
    m.set_i16(p + 0x4EC, r.wrong_way);
    m.set_i16(p + 0x4EE, r.wall);
    m.set_i16(p + 0x4F0, r.hit);
    m.set_vec3(p + 0xF8, r.knockout.map(|v| v as i32));
    m.set_i32(p + 0x444, r.side);
    m.set_u16(p + 0x4D6, r.section_changed);
}

/// `FUN_0814efa8`: put car `e` back on the road at waypoint `w` (24 bytes: x, z, `+0x0A` u16 heading, `+0x14`
/// sector): the entity on the floor there with heading `+0x0A` as is, the body 0x1900 above it, upright along the
/// waypoint's line (`*0x03005FB4`, 0x20 bytes per line). Momenta and velocities are kept.
pub(crate) fn put_back_on_road(m: &mut Mem, e: u32, w: u32) {
    let p = m.u32(e + 0x8C);
    let (index, seg) = route::advance(m, m.u16(e + 0x72) as u32, m.i16(e + 0x90) as i32);
    let first = m.i32(m.u32(W_SEGMENTS) + seg * 8 + 4);
    let (x, z) = (m.i32(w) << 8, m.i32(w + 4) << 8);
    m.set_i32(e + 0xC, x);
    m.set_i32(e + 0x14, z);
    m.set_u32(e + 0x2C, m.u16(w + 10) as u32);
    m.set_u16(e + 0x78, m.i32(w + 0x14) as u16);
    let y = world::floor_height(m, m.u16(e + 0x78) as u32, x >> 8, z >> 8);
    m.set_i32(e + 0x10, y);
    m.set_vec3(p + 0xD0, [x, y.wrapping_sub(0x1900), z]);
    let line = m.u32(0x0300_5FB4).wrapping_add((index + first) as u32 * 0x20);
    let heading = atan2(m.i32(line), m.i32(line + 4));
    crate::init::orient(m, p + 0xC8, heading);
}

fn release_accelerator(m: &mut Mem, player: bool, stats: u32) {
    if player && m.i32(stats + 0x2E8) != 0 {
        m.set_i32(stats + 0x2E8, 0);
        m.set_i32(stats + 0x2E0, 1);
    }
}

/// The lane nearest the car (`FUN_0813d1f0` tail, as `FUN_081402bc` + `FUN_08140274` with all lanes).
fn nearest_lane_of(m: &Mem, e: u32) -> u32 {
    let w = route::waypoint_at(m, m.u16(e + 0x72) as u32, m.i16(e + 0x90) as i32);
    let x = if m.u32(W_SEGMENTS) == 0 {
        0
    } else {
        let a = m.u16(w + 0xA) as i32 - 0x1000;
        let v = cos(m, a)
            .wrapping_mul((m.i32(e + 0xC) >> 8) - m.i32(w))
            .wrapping_add(((m.i32(e + 0x14) >> 8) - m.i32(w + 4)).wrapping_mul(sin(m, a)));
        (if v < 0 { v + 0x3FFF } else { v }) >> 14
    };
    route::nearest_lane(m, x, -1)
}

/// Manual gearbox: R shifts up, L down, both together wait for release (state at 0x03006074).
fn manual_shift(m: &mut Mem, p: u32, handling: u32, input: u32, pressed: u32) {
    const STATE: u32 = 0x0300_6074;
    let state = m.i16(STATE);
    let next = match state {
        1 => {
            if input & 0x100 != 0 {
                3
            } else {
                if input & 0x200 != 0 {
                    return;
                }
                let g = (m.i32(p + 0x40) - 1).max(0);
                if m.i32(RACE_PHASE) != 9 {
                    if m.i32(p + 0x40) != g {
                        gear_changed(m, p);
                    }
                    m.set_i32(p + 0x40, g);
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
                let g = (m.i32(p + 0x40) + 1).min(m.i32(handling + 0x54));
                if m.i32(RACE_PHASE) != 9 {
                    if m.i32(p + 0x40) != g {
                        gear_changed(m, p);
                    }
                    m.set_i32(p + 0x40, g);
                }
                0
            }
        }
        3 if input & 0x300 == 0 => 0,
        _ => return,
    };
    m.set_i16(STATE, next);
}
