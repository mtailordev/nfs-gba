//! Every route record the ROM can race and every car, started with `race_init::start` and run for a few hundred
//! `Game::frame`s with scripted keys: no `Unported` stop, the player moves, the lap counter is sane. The route list
//! is read from the ROM (`docs/formats/career.md`, "Tracks, route numbers, environments"): 12 circuits (forward and
//! reversed) and 18 sprints in 42 route numbers, plus the menu scene record (0) and one record nothing names (43).

use nfsgba_formats::career::{race_slots, route_track_slot, track_name};
use nfsgba_game::{
    Flow, Game, Timing,
    menu::draw::Ctx,
    race_init,
    race_setup::{Display, Setup, load_pre},
};

/// Game frames per run: the intro and the countdown take about 90, then the race runs.
const FRAMES: usize = 400;
/// The least the player must have moved (city units, 8.8): 160 units.
const MOVED: i64 = 40_000;

/// A race start from the circuit capture's menu state: `env`, `route` (number and record index), `mode` (0 circuit,
/// 1 elimination, 2 hunter, 3 sprint) and the player's car.
fn setup(rom: &[u8], env: u32, route: u32, mode: u32, car: u8, hard: bool) -> (Setup, Display) {
    let path = nfsgba_testkit::fixture("race-init/circuit_pre.wram.bin").expect("the circuit capture");
    let prefix = path.to_str().unwrap().strip_suffix(".wram.bin").unwrap().to_owned();
    let (mut s, d) = load_pre(rom.to_vec(), std::path::Path::new(&prefix)).unwrap();
    s.choose(env, route, mode, car);
    s.g.opponents = 3;
    // Hard with heavy traffic, or easy with none.
    (s.g.difficulty, s.g.u_5604) = if hard { (2, 2) } else { (0, 0) };
    s.g.laps = match mode {
        3 => 1,
        1 => 3, // elimination: laps = opponents
        _ => 2,
    };
    s.g.u_5610 = i32::from(mode != 3 && route.is_multiple_of(2)); // the reversed circuits are the even route numbers
    (s, d)
}

/// Runs `FRAMES` frames with A held (the car drives straight on, into whatever is ahead); returns how far the
/// player's car got (the distance from the grid in city units, 8.8) and its best race progress.
fn run(rom: &[u8], env: u32, route: u32, mode: u32, car: u8, hard: bool) -> Result<(i64, i32), String> {
    let (s, d) = setup(rom, env, route, mode, car, hard);
    let mut g: Game = race_init::start(rom.to_vec(), &s, d, 40).map_err(|e| format!("start: {e}"))?;
    let (laps, start) = (g.world.g.laps, g.world.slots[0].e.pos);
    let mut best = 0;
    for k in 0..FRAMES {
        match g.frame(1, &Timing::steady()) {
            Ok(Flow::Racing) => {}
            Ok(_) => return Err(format!("frame {k}: the race handed over")),
            Err(e) => return Err(format!("frame {k}: {e}")),
        }
        best = best.max(g.world.slots[0].c.progress);
    }
    let (c, end) = (&g.world.slots[0].c, g.world.slots[0].e.pos);
    assert!(
        (0..=laps.max(1)).contains(&i32::from(c.laps_left)),
        "laps left {} of {laps}",
        c.laps_left
    );
    let (dx, dz) = (i64::from(end[0] - start[0]), i64::from(end[2] - start[2]));
    Ok(((dx as f64).hypot(dz as f64) as i64, best))
}

/// The route numbers with a name in the ROM's table (1..=42) and what they are.
#[test]
fn routes_in_the_rom() {
    let Some(rom) = nfsgba_testkit::rom() else { return };
    let ctx = Ctx {
        rom: &rom,
        language: 0,
        pitch: 240,
    };
    let (mut circuits, mut sprints) = (Vec::new(), Vec::new());
    for slot in 0..42 {
        let (key, route) = track_name(&rom, slot);
        let name = String::from_utf8_lossy(&ctx.table_text(key as u32)).into_owned();
        assert_eq!(route_track_slot(&rom, route), slot, "the inverse map of route {route}");
        match slot {
            0..24 => circuits.push((route, name)),
            _ => sprints.push((route, name)),
        }
    }
    // 12 circuits, each forward (odd route) and reversed (the next even one), then 18 sprints.
    assert_eq!((circuits.len(), sprints.len()), (24, 18));
    let names: std::collections::BTreeSet<&str> = circuits.iter().map(|c| c.1.as_str()).collect();
    assert_eq!(names.len(), 12, "12 circuits: {names:?}");
    assert_eq!(
        sprints
            .iter()
            .map(|s| s.1.as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        18
    );
    assert!(
        circuits
            .iter()
            .enumerate()
            .all(|(i, c)| c.0 == 2 * (i % 12) + 1 + i / 12),
        "route numbers"
    );
    eprintln!("circuits {circuits:?}\nsprints {sprints:?}");
    // Records 0 and 43 (other bytes 0x00 0x00 0x66 0x26) are not races: 0 is the menu scene (environment 12).
    let slots = race_slots(&rom);
    assert_eq!(
        (slots[0].environment, slots[43].rest[..4].to_vec()),
        (12, slots[0].rest[..4].to_vec())
    );
    assert!((1..=42).all(|r| slots[r].route as usize == r && slots[r].environment < 12));
}

#[test]
fn every_route_runs() {
    let Some(rom) = nfsgba_testkit::rom() else { return };
    let slots = race_slots(&rom);
    let mut stopped = Vec::new();
    let mut runs = 0;
    #[allow(clippy::needless_range_loop)] // the route number is the record index and a value
    for route in 1..=42usize {
        let (env, record) = (slots[route].environment as u32, slots[route].route as u32);
        // Circuits (1..=24) race as circuit, elimination and hunter; sprints (25..=42) as sprints.
        let modes: &[u32] = if route <= 24 { &[0, 1, 2] } else { &[3] };
        for (&mode, hard) in modes.iter().flat_map(|m| [(m, false), (m, true)]) {
            runs += 1;
            match run(&rom, env, record, mode, 0, hard) {
                Ok((far, best)) => {
                    if far < MOVED || best < 1000 {
                        stopped.push(format!(
                            "route {route} mode {mode} hard {hard}: the car got {far} far, progress {best}"
                        ));
                    }
                }
                Err(e) => stopped.push(format!("route {route} mode {mode} hard {hard}: {e}")),
            }
        }
    }
    eprintln!("{runs} route/mode runs, {} problems", stopped.len());
    assert!(stopped.is_empty(), "{stopped:#?}");
}

#[test]
fn every_car_runs() {
    let Some(rom) = nfsgba_testkit::rom() else { return };
    let mut stopped = Vec::new();
    for car in 0..15u8 {
        // A circuit and a sprint.
        for (route, mode) in [(23usize, 0u32), (25, 3)] {
            let slot = &race_slots(&rom)[route];
            match run(
                &rom,
                slot.environment as u32,
                slot.route as u32,
                mode,
                car,
                car % 2 == 1,
            ) {
                Ok((far, best)) if far >= MOVED && best >= 1000 => {}
                Ok((far, best)) => stopped.push(format!("car {car} route {route}: got {far} far, progress {best}")),
                Err(e) => stopped.push(format!("car {car} route {route}: {e}")),
            }
        }
    }
    assert!(stopped.is_empty(), "{stopped:#?}");
}

/// `NFSGBA_ROUTE=6 NFSGBA_MODE=0 cargo test ... probe -- --ignored --nocapture`: the player's progress along a run.
#[test]
#[ignore]
fn probe() {
    let rom = nfsgba_testkit::rom().unwrap();
    let get = |k: &str, d: u32| std::env::var(k).ok().map_or(d, |v| v.parse().unwrap());
    let (route, mode, car) = (
        get("NFSGBA_ROUTE", 6),
        get("NFSGBA_MODE", 0),
        get("NFSGBA_CAR", 0) as u8,
    );
    let slot = &race_slots(&rom)[route as usize];
    let (s, d) = setup(&rom, slot.environment as u32, slot.route as u32, mode, car, true);
    let mut g = race_init::start(rom.clone(), &s, d, 40).unwrap();
    for k in 0..FRAMES {
        g.frame(1, &Timing::steady()).unwrap();
        if k % 25 == 0 {
            let c = &g.world.slots[0].c;
            eprintln!(
                "{k}: progress {} position {} laps_left {} speed {:?}",
                c.progress, c.position, c.laps_left, g.world.slots[0].e.pos
            );
        }
    }
}

/// Career facts the ROM answers (`docs/OPEN-QUESTIONS.md`): the crews, the events per crew and their modes, the
/// wingmen's commands per race and the Summary screen's bars.
#[test]
fn career_facts_in_the_rom() {
    use nfsgba_formats::career::{RaceMode, boss_events, events};
    let Some(rom) = nfsgba_testkit::rom() else { return };
    let ctx = Ctx {
        rom: &rom,
        language: 0,
        pitch: 240,
    };
    let text = |key: u32| String::from_utf8_lossy(&ctx.table_text(key)).into_owned();
    let all = events(&rom);
    let per_zone: Vec<usize> = (0..6).map(|z| all.iter().filter(|e| e.zone == z).count()).collect();
    assert_eq!(
        per_zone,
        [12, 12, 12, 12, 12, 6],
        "66 events: five crews of 12 and the Gauntlet's 6"
    );
    let zones: Vec<String> = (0..6).map(|z| text(0x3C3 + z)).collect();
    assert_eq!(
        zones,
        [
            "Lucky 7's",
            "The Eastsiders",
            "Syrens",
            "The Corps",
            "Krimson Crew",
            "The Gauntlet"
        ]
    );
    // Two boss events per crew (zone 6: none, the index 66 matches no event).
    let bosses = boss_events(&rom);
    assert_eq!(bosses.iter().map(|b| b[0]).collect::<Vec<_>>(), [7, 19, 31, 43, 55, 66]);
    assert_eq!(bosses.iter().map(|b| b[1]).collect::<Vec<_>>(), [8, 20, 32, 44, 56, 66]);
    let gauntlet: Vec<RaceMode> = all.iter().filter(|e| e.zone == 5).map(|e| e.mode).collect();
    use RaceMode::*;
    assert_eq!(gauntlet, [Hunter, Elimination, Circuit, Elimination, Circuit, Sprint]);
    // Commands per race of wingmen 1..=12 (odd numbers attack, even ones draft): `wingman_gap`, 0x7F4284.
    let gap: Vec<i32> = (0..12)
        .map(|i| i32::from_le_bytes(rom[0x7F_4284 + 4 * i..][..4].try_into().unwrap()))
        .collect();
    assert_eq!(gap, [3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8]);
    // The Summary screen's bars (`car_stats_draw`, FUN_08133D30): key 0x8B, 0x2E2, 0x141 and, with the style rating, 0x39A.
    let bars: Vec<String> = [0x8B, 0x2E2, 0x141, 0x39A].map(text).into();
    assert_eq!(bars, ["ACCELERATION", "TOP SPEED", "HANDLING", "VISUAL"]);
}

/// A circuit race on route 23 (the reference race's), easy, no traffic, at its start.
fn circuit_23(rom: &[u8]) -> Game {
    let slot = &race_slots(rom)[23];
    let (s, d) = setup(rom, slot.environment as u32, slot.route as u32, 0, 0, false);
    race_init::start(rom.to_vec(), &s, d, 40).unwrap()
}

/// `ai::init`'s stop (`FUN_0814dd24` with a wheel point outside every sector) is unreachable (FIDELITY N1): a point
/// the sector search misses falls back to the entity's own sector, and `update_entities` never runs an entity whose
/// sector is 0xFFFF. An opponent moved far outside the city before its setup still sets up.
#[test]
fn opponent_setup_outside_the_city() {
    let Some(rom) = nfsgba_testkit::rom() else { return };
    let mut g = circuit_23(&rom);
    let i = (0..4)
        .find(|&i| g.world.slots[i].e.handler == 0x29)
        .expect("an opponent");
    let sector = g.world.slots[i].e.sector;
    assert!(g.world.slots[i].e.race_state == 0 && sector != 0xFFFF);
    g.world.slots[i].e.pos[0] = 0x3FFF_0000;
    g.world.slots[i].e.pos[2] = -0x3FFF_0000;
    g.frame(0, &Timing::steady()).unwrap();
    assert_ne!(g.world.slots[i].e.race_state, 0, "the opponent's setup ran");
}

/// `Game::bldalpha` is race_init's 0x0D0F from the race start on.
#[test]
fn race_blend_from_the_start() {
    let Some(rom) = nfsgba_testkit::rom() else { return };
    let mut g = circuit_23(&rom);
    assert_eq!(g.bldalpha, 0x0D0F);
    for _ in 0..30 {
        g.frame(0, &Timing::steady()).unwrap();
    }
    assert_eq!(g.bldalpha, 0x0D0F);
}

/// `Game::tinted_palette` is what the race frame's tail (`Game::tint`) writes to palette RAM once the fade is done;
/// on a start that has not run a frame, `Game::tint` sends it to the fade's target.
#[test]
fn tint_callable_on_a_paused_start() {
    let Some(rom) = nfsgba_testkit::rom() else { return };
    let mut g = circuit_23(&rom);
    let want = g.tinted_palette().expect("route 23's grid is lit");
    let fading = g.world.g.fade != 0;
    g.tint();
    let ranges = || (1..=143).chain(149..=255);
    if fading {
        assert!(ranges().all(|i| g.world.palette_fade[i] == want[i]));
    }
    let mut frames = 0;
    while g.world.g.fade != 0 || frames == 0 {
        g.frame(0, &Timing::steady()).unwrap();
        frames += 1;
        assert!(frames < 200, "the fade ends");
    }
    g.frame(0, &Timing::steady()).unwrap();
    let want = g.tinted_palette().unwrap();
    assert!(ranges().all(|i| u16::from_le_bytes([g.palette[2 * i], g.palette[2 * i + 1]]) == want[i]));
}
