//! Recordings of `traces2` (`tools/traces2.py`, `tools/recorders/traces2.lua`; headless key scripts, mGBA breakpoints).
//!
//! D14: the grid deal's rand index. A power-on run through the menus into a circuit race and into a circuit race with a
//! wingman logs every `rand_table` draw, every `main_frame` entry (screen, game state, tick and VBlank counters, keys) and
//! the race start (`pick_opponent_cars`, the seed read). The `Session` is fed the recorded `main_frame` sequence with the
//! recorded counters ([`Session::clock`]: the original runs several `main_frame`s per video frame, T1): at every frame it
//! must be on the same screen and game state with the same rand index, and its race start must draw the same opponents and
//! leave the same index.

use nfsgba_audio::{Engine, Rom};
use nfsgba_game::{
    race_init,
    race_setup::{Display, Setup},
    session::{Session, apply_choice},
};

const POWER_ON: [&str; 2] = ["circuit", "wingman"];

/// One `main_frame` entry of the recorded menus.
struct Frame {
    screen: u32,
    state: u32,
    tick: u32,
    vblanks: u32,
    keys: u16,
    /// The rand index the game had at the entry.
    rand: u32,
    /// The scenario poked the race choice just before this frame.
    poke: bool,
}

/// What the log says about the race start.
#[derive(Default, Debug)]
struct Start {
    /// `race_start_from_table_a`'s entry: the tick counter and the rand index.
    tick: u32,
    rand: u32,
    /// `pick_opponent_cars`: opponents and wingman at its entry, then its `rand_table` draws (index before, value).
    opponents: u32,
    wingman: u32,
    draws: Vec<(u32, u32)>,
    /// The seed `setup_race_cars` read (the tick counter).
    seed_tick: u32,
    /// `race_start_from_table_a`'s return: the rand index, the four cars and paints.
    end_rand: u32,
    cars: [i8; 4],
    paints: [i8; 4],
}

fn field(line: &str, key: &str) -> u32 {
    let at = line
        .find(&format!(" {key}="))
        .unwrap_or_else(|| panic!("{key} in {line}"))
        + key.len()
        + 2;
    let text = line[at..].split(' ').next().unwrap();
    text.parse::<i64>().unwrap() as u32
}

fn list<const N: usize>(line: &str, key: &str) -> [i8; N] {
    let at = line.find(&format!(" {key}=")).unwrap() + key.len() + 2;
    let v: Vec<i8> = line[at..]
        .split(' ')
        .next()
        .unwrap()
        .split(',')
        .map(|x| x.parse().unwrap())
        .collect();
    v.try_into().unwrap()
}

/// The menus' frames up to the race start's, and the race start.
fn parse(text: &str) -> (Vec<Frame>, Start) {
    let (mut frames, mut start, mut rand, mut poke, mut picking) = (vec![], Start::default(), 0, false, false);
    for line in text.lines() {
        let tag = line.split(' ').nth(1).unwrap();
        match tag {
            "M" => {
                frames.push(Frame {
                    screen: field(line, "screen"),
                    state: field(line, "state"),
                    tick: field(line, "tick"),
                    vblanks: field(line, "vb"),
                    keys: field(line, "keys") as u16,
                    rand,
                    poke: std::mem::take(&mut poke),
                });
            }
            "POKE" => poke = true,
            "R" => {
                rand = field(line, "idx2");
                if picking {
                    start.draws.push((field(line, "idx"), field(line, "val")));
                }
            }
            "START" => (start.tick, start.rand) = (field(line, "tick"), field(line, "idx")),
            "PICK" => {
                (start.opponents, start.wingman) = (field(line, "opp"), field(line, "wingman"));
                picking = true;
            }
            "PICKED" => picking = false,
            "SEED" => start.seed_tick = field(line, "tick"),
            "END" => {
                start.end_rand = field(line, "idx");
                (start.cars, start.paints) = (list(line, "cars"), list(line, "paints"));
                break;
            }
            _ => {}
        }
    }
    (frames, start)
}

/// D14: the rand index of the grid deal, traced from power-on, and the `Session`'s deal from the same menu frames.
#[test]
fn the_grid_deal_follows_the_menu_draws() {
    let Some(rom) = nfsgba_testkit::rom() else { return };
    for name in POWER_ON {
        let Some(text) = nfsgba_testkit::read_to_string(&format!("traces2/menus-{name}.log")) else {
            return;
        };
        let (frames, start) = parse(&text);
        // The recorded deal: four draws (the car set, then a paint per opponent) at the index the menus left, the table's
        // numbers as the game returned them, the seed read six VBlanks after the start's entry.
        assert_eq!(
            start.draws.len(),
            1 + start.opponents as usize,
            "{name}: the draws of pick_opponent_cars"
        );
        assert!(
            start
                .draws
                .iter()
                .enumerate()
                .all(|(k, d)| d.0 == (start.rand + k as u32) & 0xFF)
        );
        let mut index = start.rand;
        for &(before, value) in &start.draws {
            assert_eq!(before, index);
            assert_eq!(
                nfsgba_fixed::rand_table(&rom, &mut index),
                value,
                "{name}: table value at {before}"
            );
        }
        assert_eq!(
            start.seed_tick,
            start.tick + 6,
            "{name}: the seed is read 6 VBlanks after the start's entry"
        );
        let k = start.draws[0].1 % 5;
        for i in 0..start.opponents as usize {
            assert_eq!(start.cars[i + 1] as u32, 3 * k + i as u32, "{name}: cars 3k+i");
            // (The first opponent's paint is then replaced from the wingman table.)
            assert!(
                i == 0 || start.paints[i + 1] as u32 == start.draws[i + 1].1 % 15,
                "{name}: paint {i}"
            );
        }

        // The Session from power-on on the recorded main_frame sequence, its rand index set to the game's before every
        // frame: the draws of each frame are compared, where the session and the game are on the same screen. (The
        // typed menus change screens at once; the game fades for about 30 main_frames, T1: so the two runs' frame
        // counts, and with them the index at the race start, differ; the draws per frame do not.)
        let mut s = Session::new(&rom, vec![0xFF; 512]);
        let (mut compared, mut menu_frames) = (0, 0);
        for (k, f) in frames.iter().enumerate() {
            if f.state == 4 || s.race.is_some() {
                break;
            }
            if f.poke {
                // The scenario's pokes: the wingman, the opponents (and their settings copy), the race mode.
                let c = if name == "wingman" { (1, 2) } else { (0, 3) };
                s.st.profile.wingman = c.0;
                s.st.profile.settings[3] = c.1;
                (s.st.g.race_mode, s.st.g.opponents) = (0, c.1);
            }
            s.st.g.rand_index = f.rand;
            // The game starts the race in the main_frame after the one that chose it; the session in the same one.
            let next = frames.get(k + 1).filter(|n| n.state == 4).unwrap_or(f);
            s.clock = Some((next.tick, f.vblanks));
            let screen = s.st.g.screen;
            s.frame(f.keys)
                .unwrap_or_else(|e| panic!("{name}: main_frame {k}: {e}"));
            menu_frames += 1;
            let same = frames
                .get(k + 1)
                .filter(|n| n.state == 1 && n.screen == f.screen && f.screen == screen);
            if let (Some(n), true) = (same, s.st.g.screen == screen && s.race.is_none()) {
                assert_eq!(
                    s.st.g.rand_index, n.rand,
                    "{name}: main_frame {k}: the frame's draws (screen {screen})"
                );
                compared += 1;
            }
        }
        assert!(
            compared * 10 > menu_frames * 8,
            "{name}: {compared} of {menu_frames} frames compared"
        );
        eprintln!(
            "{name}: {compared}/{menu_frames} compared; session on screen {} state {}",
            s.st.g.screen, s.st.g.game_state
        );
        assert!(s.race.is_some(), "{name}: the session starts the race");
        // The session's own deal, at the index it reached: the rule of pick_opponent_cars on the table.
        let race = s.race.as_ref().unwrap();
        eprintln!(
            "{name}: {compared} of {menu_frames} menu frames compared; session grid {:?}",
            race.world.cars
        );

        // The session's race start on the game's index and tick: the same opponents, the same index afterwards.
        let records = s.st.profile.car_records;
        let mut st = s.st.clone();
        (st.g.rand_index, st.g.race_car) = (start.rand, start.cars[0] as u8);
        let mut setup = Setup::menus(&rom, Engine::new(Rom(&rom), 16, 16), start.tick, 0, &records, None);
        apply_choice(&mut setup, &st);
        let game = race_init::start(rom.clone(), &setup, Display::from_screen(&s.host.screen), 6).unwrap();
        assert_eq!(
            (game.world.cars, game.world.paints),
            (start.cars, start.paints),
            "{name}: the grid"
        );
        assert_eq!(
            game.world.g.rand, start.end_rand,
            "{name}: the rand index after the start"
        );
    }
}

/// D8: the player's rim redraw rule, traced angle by angle. A circuit with the player holding A and LEFT (`circle.log`:
/// every entry of the two functions that can redraw the rim, `0x0814A2A0` and `car_racing_step` `0x0814B168`, with the
/// inputs of the decision, every `rim_side_visible` call with its arguments and result, and every
/// `draw_decal_on_atlas` call with its caller). Our rule must decide the same at every entry and return the same from
/// every call, over a full turn of the heading.
#[test]
fn the_rim_redraw_rule_matches_a_full_turn() {
    use nfsgba_fixed::angle_diff;
    use nfsgba_game::slots::{rim_side_visible, rim_wanted};

    let Some(text) = nfsgba_testkit::read_to_string("traces2/circle.log") else {
        return;
    };
    const STEP: u32 = 0x0814_B168;
    // (inputs of the entry, whether draw_decal_on_atlas was called from the step before the next entry)
    let mut entries: Vec<([u32; 6], bool)> = vec![];
    let (mut calls, mut sectors, mut classes, mut player_calls) = (0, [0u32; 16], [0u32; 3], 0);
    for line in text.lines() {
        let tag = line.split(' ').nth(1).unwrap();
        match tag {
            "E" => {
                let f = |k: &str| field(line, k);
                entries.push((
                    [f("phase"), f("n"), f("player"), f("view"), f("heading"), f("yaw")],
                    false,
                ));
                if f("fn") == STEP && f("n") == f("player") {
                    let heading = (f("heading") & 0x3F_FFFF) >> 8;
                    sectors[(heading >> 10) as usize] += 1;
                }
            }
            "V" => {
                let (a, b, ret) = (field(line, "a") as i32, field(line, "b") as i32, field(line, "ret"));
                assert_eq!(rim_side_visible(a, b), ret != 0, "rim_side_visible({a:#x}, {b:#x})");
                let d = angle_diff(a, b).abs();
                classes[if d < 0x400 {
                    0
                } else if 0x1C00 < d && d < 0x2400 {
                    1
                } else {
                    2
                }] += 1;
                calls += 1;
            }
            "D" => {
                // (the start's unpack_decal also calls it, from outside the step)
                let lr = field(line, "lr");
                if (STEP..STEP + 0x400).contains(&lr) {
                    entries.last_mut().expect("a redraw follows an entry").1 = true;
                    player_calls += 1;
                }
            }
            _ => {}
        }
    }
    let mut redrawn = 0;
    for (k, ([phase, index, player, view, heading, yaw], drawn)) in entries.iter().enumerate() {
        let wanted = rim_wanted(*phase, *index, *player, *view, *heading as i32, *yaw as i32);
        assert_eq!(
            wanted, *drawn,
            "entry {k}: phase {phase} entity {index} view {view} heading {heading:#x} yaw {yaw}"
        );
        redrawn += u32::from(wanted);
    }
    assert!(
        entries.len() > 1000 && calls > 500,
        "{} entries, {calls} rim_side_visible calls",
        entries.len()
    );
    assert_eq!(redrawn, player_calls, "every redraw is one the rule wants");
    assert!(
        sectors.iter().all(|&n| n > 0),
        "the heading turned through a full circle: {sectors:?}"
    );
    assert!(
        classes.iter().all(|&n| n > 0),
        "both hidden bands and the visible range were met: {classes:?}"
    );
    assert!(redrawn > 100 && redrawn < calls as u32, "{redrawn} redraws");
}

const KEYS: [&str; 10] = ["A", "B", "SELECT", "START", "RIGHT", "LEFT", "UP", "DOWN", "R", "L"];

/// The key script of `session/quickplay.json` (power-on to a Quick Play race) per video frame, and the choice the
/// game's run made on the race setup screen, to be set at `sync_at` (see `tests/session.rs`).
fn quickplay() -> Option<(Vec<u16>, usize, serde_json::Value)> {
    let t: serde_json::Value = serde_json::from_str(&nfsgba_testkit::read_to_string("session/quickplay.json")?).ok()?;
    let mut held = vec![0u16; 2900];
    for press in t["script"].as_array()? {
        let key = KEYS.iter().position(|k| *k == press[2].as_str().unwrap()).unwrap();
        let first = press[0].as_u64().unwrap() as usize;
        held[first..first + press[1].as_u64().unwrap() as usize]
            .iter_mut()
            .for_each(|h| *h |= 1 << key);
    }
    let sync_at = t["sync_at"].as_u64()? as usize;
    Some((held, sync_at, t["frames"][sync_at].clone()))
}

/// The race choice on the setup screen, as `tests/session.rs` sets it, with the race mode `mode`.
fn choose(s: &mut Session, row: &serde_json::Value, mode: u32) {
    let n = |k: &str| row[k].as_u64().unwrap() as u32;
    let g = &mut s.st.g;
    (g.race_mode, g.route, g.reverse, g.laps, g.opponents) =
        (mode, n("route"), n("reverse"), n("laps"), n("opponents"));
    (g.difficulty, g.traffic) = (n("difficulty"), n("traffic"));
    s.st.profile.settings[..5].copy_from_slice(&[
        n("reverse"),
        n("laps"),
        n("difficulty"),
        n("opponents"),
        n("traffic"),
    ]);
    s.st.profile.car = n("car") as i8;
    s.st.profile.wingman = u32::from(n("opponents") != 3);
}

/// G1: a hunter race (mode 2) and an elimination race (mode 1) through `Session`, from power-on through the menus,
/// the intro and the countdown, to their results screen (the player's car marked finished, as `tools/session_trace.py`
/// does for the circuit), with no `Unported` on the way. The reach audit (`src/reach.rs`) shows every race path is
/// ported or unreachable; this plays the two modes whose races `session_matches_the_game` (a circuit) does not.
#[test]
fn hunter_and_elimination_races_reach_their_results() {
    let (Some(rom), Some((held, sync_at, row))) = (nfsgba_testkit::rom(), quickplay()) else {
        return;
    };
    for (name, mode) in [("elimination", 1), ("hunter", 2)] {
        let mut s = Session::new(&rom, vec![0xFF; 512]);
        let (mut poked, mut results_at) = (false, None);
        for f in 0..6000usize {
            if f == sync_at {
                choose(&mut s, &row, mode);
            }
            s.frame(held.get(f).copied().unwrap_or(0))
                .unwrap_or_else(|e| panic!("{name}: frame {f}: {e}"));
            if let Some(game) = s
                .race
                .as_mut()
                .filter(|g| g.world.lp.game_state == 5 && g.world.g.race_frames >= 150)
                && !poked
            {
                assert_eq!(game.world.g.mode, mode, "{name}: the race mode");
                let p = game.world.g.player as usize;
                game.world.slots[p].e.race_state = 2;
                poked = true;
            }
            if poked && s.race.is_none() && s.st.g.screen == 12 {
                results_at = Some(f);
                break;
            }
        }
        assert!(poked, "{name}: a race was started and run");
        assert!(results_at.is_some(), "{name}: the results screen was reached");
    }
}

/// U8: the pause menu's resume. `pause.log` (a circuit, START, A on the first item): `goto_screen(0x82)` calls
/// `VBlankIntrWait` 15 times, then `restart_engine_sound` and the race music (id 1) in the VBlank the race's next frame
/// starts in. The `Session` must hold the race and the menu page for the same number of video frames after the A, then
/// restart the engine loop and the music, and run the race.
#[test]
fn the_resume_waits_as_the_game_does() {
    let (Some(rom), Some((held, sync_at, row)), Some(text)) = (
        nfsgba_testkit::rom(),
        quickplay(),
        nfsgba_testkit::read_to_string("traces2/pause.log"),
    ) else {
        return;
    };
    // The recording: the resume's goto_screen, its VBlankIntrWait calls, the first race frame after it.
    let vb = |line: &str| field(line, "vb");
    let lines: Vec<&str> = text.lines().collect();
    let goto = lines
        .iter()
        .position(|l| l.contains(" G arg=130 "))
        .expect("goto_screen(0x82)");
    let first_frame = lines[goto..]
        .iter()
        .position(|l| l.contains(" M ") && field(l, "state") == 5)
        .expect("the race's first main_frame")
        + goto;
    let waits = lines[goto..first_frame].iter().filter(|l| l.contains(" W ")).count();
    let (start, end) = (vb(lines[goto]), vb(lines[first_frame]));
    let restart = lines[goto..=first_frame + 8]
        .iter()
        .find(|l| l.contains(" X "))
        .expect("restart_engine_sound");
    let music = lines[goto..]
        .iter()
        .find(|l| l.contains(" MP "))
        .expect("the race music");
    assert_eq!(
        (waits, end - start),
        (15, 15),
        "15 VBlankIntrWait, the race 15 VBlanks after the goto"
    );
    assert_eq!(
        (vb(restart), vb(music), field(music, "id")),
        (end, end, 1),
        "the engine loop and the music restart after"
    );

    // The session: to the pause menu, then the A.
    let mut s = Session::new(&rom, vec![0xFF; 512]);
    for (f, keys) in held.iter().enumerate().take(2700) {
        if f == sync_at {
            choose(&mut s, &row, 0);
        }
        s.frame(*keys).unwrap();
    }
    let race_frames = |s: &Session| s.race.as_ref().unwrap().world.g.race_frames;
    (0..200).for_each(|_| s.frame(0).unwrap());
    (0..12).for_each(|_| s.frame(0x8).unwrap()); // START
    (0..60).for_each(|_| s.frame(0).unwrap());
    assert_eq!(s.st.g.screen, 5, "the pause menu");
    let paused = race_frames(&s);
    let page = s.view().page.to_vec();
    s.frame(0x1).unwrap(); // A: resume (the press is call 0)
    let mut first_race_call = None;
    for call in 1..40 {
        s.frame(0).unwrap();
        if race_frames(&s) > paused && first_race_call.is_none() {
            first_race_call = Some(call);
        }
        // (The race is handed back at the end of the last wait: its first frame is the next call.)
        if first_race_call.is_none() && call < end - start - 1 {
            assert!(!s.racing(), "call {call}: the race stands still");
            assert!(s.view().page == &page[..], "call {call}: the menu page stays");
        }
    }
    assert_eq!(
        first_race_call,
        Some(end - start),
        "video frames from the A to the race's next frame"
    );
    let game = s.race.as_mut().unwrap();
    assert!(game.world.audio.sfx_playing(1), "the engine loop runs");
    assert_eq!(game.world.lp.music_id, 1, "the race music");
}
