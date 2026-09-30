//! `Session` against the game (FIDELITY G1): a power-on run in headless mGBA (`tools/session_trace.py`: fresh save,
//! the menus into a Quick Play race, the player marked finished after 150 game frames, the results screen) and a
//! `Session` fed the same key script. The two runs are compared where a screen has settled (the last settled frame of each
//! visit to a `(screen, game state)` with no fade and no exit under way): the sequence of visits, the menu state that
//! `boot_matches_the_game` compares, and in the race the choice it was built from. Frame timing differs by design (T1:
//! the menus run `main_frame` several times per video frame, a race frame takes about four), so the race is driven by
//! game frames on our side, not by the video frame of the trace.

use nfsgba_game::{race_setup::load_pre, session::Session};
use serde_json::Value;

const KEYS: [&str; 10] = ["A", "B", "SELECT", "START", "RIGHT", "LEFT", "UP", "DOWN", "R", "L"];

/// What is compared at a settled visit.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Visit {
    screen: i64,
    state: i64,
    /// back_top, language, units, exists, loaded, message_box, name.
    menu: Vec<i64>,
    /// In the race: mode, route, laps, opponents, difficulty, traffic, environment, career, route_flag, player.
    race: Vec<i64>,
    /// The player's id in the results block (the opponents' follow the rand seed, a tick count: T1), the knocked-out bytes, whether the player has a finish time (after a race).
    results: Vec<i64>,
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

/// The game's visits: the first settled frame of each `(screen, state)` run.
fn game_visits(frames: &[Value]) -> Vec<Visit> {
    let n = |f: &Value, k: &str| f[k].as_i64().unwrap_or(0); // the profile fields are absent before the boot sets the pointer
    let mut out = vec![];
    let mut key = None;
    for f in frames {
        let now = (n(f, "screen"), n(f, "state"));
        if key != Some(now) {
            key = Some(now);
            out.push(None);
        }
        let last = out.last_mut().unwrap();
        if n(f, "fade") == 0 && n(f, "menu_exit") == 0 {
            let mut menu = vec![
                n(f, "back_top"),
                n(f, "language"),
                n(f, "units"),
                n(f, "exists"),
                n(f, "loaded"),
                n(f, "message_box"),
            ];
            menu.extend(unhex(f["name"].as_str().unwrap_or("")).iter().map(|&b| b as i64));
            let race = if now.1 == 5 {
                [
                    "mode",
                    "route",
                    "laps",
                    "opponents",
                    "difficulty",
                    "traffic",
                    "env",
                    "career",
                    "route_flag",
                    "player",
                ]
                .iter()
                .map(|k| n(f, k))
                .collect()
            } else {
                vec![]
            };
            let r = unhex(f["results"].as_str().unwrap());
            let results = if now.0 == 12 {
                let mut v: Vec<i64> = std::iter::once(&r[4]).chain(&r[8..12]).map(|&b| b as i64).collect();
                v.push(i64::from(u32::from_le_bytes(r[0x20..0x24].try_into().unwrap()) > 0));
                v
            } else {
                vec![]
            };
            *last = Some(Visit {
                screen: now.0,
                state: now.1,
                menu,
                race,
                results,
            });
        }
    }
    out.into_iter().flatten().collect()
}

#[test]
fn session_matches_the_game() {
    let (Some(rom), Some(text), Some(pre)) = (
        nfsgba_testkit::rom(),
        nfsgba_testkit::read_to_string("session/quickplay.json"),
        nfsgba_testkit::fixture("race-init/circuit_pre.wram.bin"),
    ) else {
        return;
    };
    let t: Value = serde_json::from_str(&text).unwrap();
    let frames = t["frames"].as_array().unwrap();
    let finish_at = t["finish_at"].as_u64().unwrap() as u32;
    let sync_at = t["sync_at"].as_u64().unwrap() as usize;
    let mut held = vec![0u16; frames.len()];
    for press in t["script"].as_array().unwrap() {
        let key = KEYS.iter().position(|k| *k == press[2].as_str().unwrap()).unwrap();
        let first = press[0].as_u64().unwrap() as usize;
        held[first..first + press[1].as_u64().unwrap() as usize]
            .iter_mut()
            .for_each(|h| *h |= 1 << key);
    }
    let prefix = pre.to_str().unwrap().strip_suffix(".wram.bin").unwrap().to_owned();
    let template = load_pre(rom.clone(), std::path::Path::new(&prefix)).unwrap();
    let mut s = Session::new(&rom, vec![0xFF; 512], template);

    // Our run: the script's keys by video frame; the race is poked to its finish after `finish_at` game frames.
    let (mut ours, mut key, mut poked, mut race_at) = (vec![], None, false, None);
    for f in 0..frames.len() {
        if f == sync_at {
            sync_choice(&mut s, &frames[f]);
        }
        s.frame(held[f])
            .unwrap_or_else(|e| panic!("frame {f}: {e} ({:x?})", s.host.calls));
        let (mut now, racing) = ((s.st.g.screen as i64, s.st.g.game_state as i64), s.race.is_some());
        if let Some(game) = s.race.as_mut().filter(|_| racing) {
            now.1 = game.world.lp.game_state as i64;
            race_at = race_at.or(Some(f));
            if game.world.lp.game_state == 5 && game.world.g.race_frames >= finish_at && !poked {
                let p = game.world.g.player as usize;
                game.world.slots[p].e.race_state = 2;
                poked = true;
            }
        }
        // A visit settles when the fade is over and no exit is under way (the race: the state 5 with no fade).
        let (fade, exit) = match &s.race {
            Some(g) => (g.world.g.fade as i64, 0),
            None => (s.st.g.fade as i64, s.st.g.menu_exit as i64),
        };
        if key != Some(now) {
            key = Some(now);
            eprintln!("ours: frame {f} screen {} state {}", now.0, now.1);
            ours.push(None);
        }
        let last = ours.last_mut().unwrap();
        if fade == 0 && exit == 0 {
            let (g, p) = (&s.st.g, &s.st.profile);
            let mut menu = vec![
                g.back_top as i64,
                g.language as i64,
                g.units as i64,
                p.profile_exists as i64,
                p.loaded as i64,
                g.message_box as i64,
            ];
            // The name entry types into the profile's name in the game; ours keeps it apart until OK.
            let name = if now.0 == 22 { &g.name } else { &p.name };
            menu.extend(name.iter().map(|&b| b as i64));
            let race = match &s.race {
                Some(game) if now.1 == 5 => {
                    let c = &game.world.g;
                    [
                        c.mode,
                        c.u_5388,
                        c.laps as u32,
                        c.opponents,
                        c.difficulty,
                        c.u_5604,
                        c.level as u32,
                        c.career as u32,
                        c.route_index,
                        c.player,
                    ]
                    .iter()
                    .map(|&v| v as i64)
                    .collect()
                }
                _ => vec![],
            };
            let results = if now.0 == 12 {
                let mut v: Vec<i64> = std::iter::once(&g.results.ids[0])
                    .chain(&g.results.knocked)
                    .map(|&b| b as i64)
                    .collect();
                v.push(i64::from(g.results.finish[0] > 0));
                v
            } else {
                vec![]
            };
            *last = Some(Visit {
                screen: now.0,
                state: now.1,
                menu,
                race,
                results,
            });
        }
    }
    // The transients of the original (boot states 0, the frames of a screen change between menus and race) are not
    // compared: the visits are the menu screens in state 1 and the race.
    let keep = |v: &Visit| (v.state == 1 && v.screen < 0x80) || (v.state == 5 && v.screen == 0x81);
    let ours: Vec<Visit> = ours.into_iter().flatten().filter(keep).collect();
    let game: Vec<Visit> = game_visits(frames).into_iter().filter(keep).collect();
    let seq = |v: &[Visit]| v.iter().map(|v| (v.screen, v.state)).collect::<Vec<_>>();
    assert_eq!(seq(&ours), seq(&game), "the screens visited");
    assert!(race_at.is_some() && poked, "a race was run and finished");
    for (o, g) in ours.iter().zip(&game) {
        assert_eq!(o, g, "screen {} state {}", g.screen, g.state);
    }
    assert!(
        game.iter().any(|v| v.screen == 12 && v.state == 1),
        "the run reaches the results"
    );
}

/// The choice the menus draw at random on the race setup screen (the mode, the track, the laps, the opponents, the
/// traffic, the difficulty and the car), taken from the game's run: the random sequence follows the number of menu
/// frames drawn (T1), which differs between the two runs by design. Everything after it is ours.
fn sync_choice(s: &mut Session, row: &Value) {
    let n = |k: &str| row[k].as_u64().unwrap() as u32;
    let g = &mut s.st.g;
    (g.race_mode, g.route, g.reverse, g.laps, g.opponents) =
        (n("mode"), n("route"), n("reverse"), n("laps"), n("opponents"));
    (g.difficulty, g.traffic) = (n("difficulty"), n("traffic"));
    // The setup screen works on a copy of the options (reverse, laps, difficulty, opponents, traffic).
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

/// START in the race opens the pause menu (screen 5, the race is kept), A on its first item resumes the race.
/// No original to compare with: the pause hand-over itself is checked by `handovers_match_the_game`.
#[test]
fn session_pauses_and_resumes() {
    let (Some(rom), Some(text), Some(pre)) = (
        nfsgba_testkit::rom(),
        nfsgba_testkit::read_to_string("session/quickplay.json"),
        nfsgba_testkit::fixture("race-init/circuit_pre.wram.bin"),
    ) else {
        return;
    };
    let t: Value = serde_json::from_str(&text).unwrap();
    let frames = t["frames"].as_array().unwrap();
    let mut held = vec![0u16; 2900];
    for press in t["script"].as_array().unwrap() {
        let key = KEYS.iter().position(|k| *k == press[2].as_str().unwrap()).unwrap();
        let first = press[0].as_u64().unwrap() as usize;
        held[first..first + press[1].as_u64().unwrap() as usize]
            .iter_mut()
            .for_each(|h| *h |= 1 << key);
    }
    let prefix = pre.to_str().unwrap().strip_suffix(".wram.bin").unwrap().to_owned();
    let mut s = Session::new(
        &rom,
        vec![0xFF; 512],
        load_pre(rom.clone(), std::path::Path::new(&prefix)).unwrap(),
    );
    let sync_at = t["sync_at"].as_u64().unwrap() as usize;
    for (f, keys) in held.iter().enumerate().take(2700) {
        if f == sync_at {
            sync_choice(&mut s, &frames[f]);
        }
        s.frame(*keys).unwrap();
    }
    let race_frames = |s: &Session| s.race.as_ref().unwrap().world.g.race_frames;
    for _ in 0..200 {
        s.frame(0).unwrap(); // the intro and the countdown
    }
    let before = race_frames(&s);
    assert!(before >= 100);
    for _ in 0..12 {
        s.frame(0x8).unwrap(); // START
    }
    for _ in 0..60 {
        s.frame(0).unwrap();
    }
    assert_eq!(s.st.g.screen, 5, "the pause menu");
    let paused = race_frames(&s);
    for _ in 0..30 {
        s.frame(0).unwrap();
    }
    assert_eq!(race_frames(&s), paused, "the race waits");
    for _ in 0..12 {
        s.frame(0x1).unwrap(); // A: resume
    }
    for _ in 0..90 {
        s.frame(0).unwrap();
    }
    assert!(race_frames(&s) > paused + 20, "the race runs again");
}
