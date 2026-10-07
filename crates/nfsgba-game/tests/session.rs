//! `Session` against the game (FIDELITY G1): a power-on run in headless mGBA (`tools/session_trace.py`: fresh save,
//! the menus into a Quick Play race, the player marked finished after 150 game frames, the results screen) and a
//! `Session` fed the same key script. The two runs are compared where a screen has settled (the last settled frame of each
//! visit to a `(screen, game state)` with no fade and no exit under way): the sequence of visits, the menu state that
//! `boot_matches_the_game` compares, and in the race the choice it was built from. Frame timing differs by design (T1:
//! the menus run `main_frame` several times per video frame, a race frame takes about four), so the race is driven by
//! game frames on our side, not by the video frame of the trace.

use nfsgba_game::session::Session;
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
    session_matches("session/quickplay.json", 0);
}

/// The same run with the language screen set to German first (`tools/session_trace.py quickplay-de --language 2 --out
/// session2`: the cursor RIGHT twice, the rest of the key script 100 frames later): every menu is German, the race
/// takes the language into its HUD, and the visits and choices equal the game's.
#[test]
fn session_matches_the_game_in_german() {
    session_matches("session2/quickplay-de.json", 2);
}

/// One recorded power-on run (`fixture`) against a `Session` fed its key script; `language` is the one the run chose.
fn session_matches(fixture: &str, language: u32) {
    let (Some(rom), Some(text)) = (
        nfsgba_testkit::rom(),
        nfsgba_testkit::read_to_string(fixture),
    ) else {
        return;
    };
    let t: Value = serde_json::from_str(&text).unwrap();
    let frames = t["frames"].as_array().unwrap();
    assert_eq!(t["language"].as_u64().unwrap_or(0) as u32, language, "the recording's language");
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
    let mut s = Session::new(&rom, vec![0xFF; 512]);

    // Our run: the script's keys by video frame; the race is poked to its finish after `finish_at` game frames.
    let (mut ours, mut key, mut poked, mut race_at) = (vec![], None, false, None);
    // The racer ids the menus pick (screen 15) and the ones the results screens show.
    let (mut picked, mut shown) = (None, None);
    for f in 0..frames.len() {
        if f == sync_at {
            sync_choice(&mut s, &frames[f]);
        }
        s.frame(held[f])
            .unwrap_or_else(|e| panic!("frame {f}: {e} ({:x?})", s.host.calls));
        let (mut now, racing) = ((s.st.g.screen as i64, s.st.g.game_state as i64), s.race.is_some());
        match (now.0, racing) {
            (15, false) => picked = Some(s.st.g.results.ids),
            (12, false) => shown = shown.or(Some(s.st.g.results.ids)),
            _ => {}
        }
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
    // The results block is one block in the game: the race keeps the ids the menus picked (in the recording too).
    let ids = |screen: u64| {
        frames
            .iter()
            .rev()
            .find(|f| f["screen"].as_u64() == Some(screen))
            .map(|f| unhex(f["results"].as_str().unwrap())[4..8].to_vec())
    };
    assert_eq!(ids(15), ids(12), "the game's ids on screens 15 and 12");
    let picked = picked.expect("screen 15");
    assert!(picked[1..].iter().all(|&i| i != 0), "opponent ids {picked:?}");
    assert_eq!(shown, Some(picked), "the results show the racers the menus picked");
    assert!(s.keeps_the_race_heap(), "the race's heap is kept for the next race");
    // The text arguments live for one frame (they grew without bound before).
    assert!(s.host.texts.len() < 64, "{} text arguments kept", s.host.texts.len());
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
    let (Some(rom), Some(text)) = (
        nfsgba_testkit::rom(),
        nfsgba_testkit::read_to_string("session/quickplay.json"),
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
    let mut s = Session::new(&rom, vec![0xFF; 512]);
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

/// The menus' sound against the game's (`tools/menu_audio_trace.py`: the sound buffer the DMA plays in each video frame
/// of a headless power-on run) for the boot script into a Quick Play race. The engine runs from power-on and takes the
/// menus' requests: the start-up, the intro sound and the button clicks are sample-exact frame by frame while the
/// two runs are in step; then the menu music (requested by the flow, the same module) is sample-exact from its first
/// sample for 600 frames, but 18 video frames later in the game (T1: the original's `main_frame` takes several video
/// frames to draw a screen, ours takes one, so its screens come later).
#[test]
fn menu_audio_matches_the_game() {
    let (Some(rom), Some(text), Some(path)) = (
        nfsgba_testkit::rom(),
        nfsgba_testkit::read_to_string("session/quickplay.json"),
        nfsgba_testkit::fixture("session/quickplay-audio.bin"),
    ) else {
        return;
    };
    let orig = std::fs::read(path).unwrap();
    let t: Value = serde_json::from_str(&text).unwrap();
    let frames = orig.len() / 176;
    let mut held = vec![0u16; 4600];
    for press in t["script"].as_array().unwrap() {
        let key = KEYS.iter().position(|k| *k == press[2].as_str().unwrap()).unwrap();
        let first = press[0].as_u64().unwrap() as usize;
        held[first..first + press[1].as_u64().unwrap() as usize]
            .iter_mut()
            .for_each(|h| *h |= 1 << key);
    }
    let mut s = Session::new(&rom, vec![0xFF; 512]);
    let mut ours = vec![];
    for keys in &held {
        if s.race.is_some() {
            break;
        }
        s.frame(*keys).unwrap();
        ours.push(s.sound().to_vec());
    }
    let theirs = |f: usize| &orig[f * 176..][..176];
    let audible = |b: &[u8]| b.iter().any(|&x| x != 0);
    // The frames before the menu music: the start-up silence and the clicks.
    let music = (100..ours.len())
        .find(|&f| ours[f] != theirs(f))
        .expect("the menu music");
    assert!(
        music > 800,
        "sample-exact until the menu music is requested (frame {music})"
    );
    assert!(
        (0..music).any(|f| audible(&ours[f])),
        "the clicks are in the compared frames"
    );
    // The music: the first audible frame after it in each run, then 600 frames in step.
    let (a, b) = (
        (music..ours.len()).find(|&f| audible(&ours[f])).expect("music in ours"),
        (music..frames)
            .find(|&f| audible(theirs(f)))
            .expect("music in the game"),
    );
    assert!(
        b > a && b - a < 40,
        "the game's menu is a few frames behind (ours {a}, game {b})"
    );
    assert!(
        (0..600).all(|k| ours[a + k] == theirs(b + k)),
        "600 frames of the menu music"
    );
}

/// The session builds its race start from its own state: no captured machine state (a RAM image, `load_pre`,
/// `Machine`, `Setup::load`) is read anywhere in `session.rs`. What remains a byte image is the heap arena (R24).
#[test]
fn session_uses_no_ram_image() {
    let src = include_str!("../src/session.rs");
    for banned in ["Machine", "load_pre", "Setup::load", "Mem::new", "wram"] {
        assert!(!src.contains(banned), "session.rs mentions {banned}");
    }
}

/// R24 measured: the race start on `Heap::menus` (the menus' blocks with zeroed scratch) instead of the game's heap
/// (the 11 recorded first-race starts): the same atlases at the same places, and the same screen and OAM in each of 900
/// frames, driving from frame 250 (the rim redraw reads next to its buffer). The bytes of the heap differ (menu scratch:
/// about 44 KB) and the vehicle matrix slots hold other stale bytes; nothing visible depends on them.
#[test]
fn the_default_heap_is_invisible() {
    use nfsgba_game::{
        Flow, Timing,
        race_init::{arena_view, race_start, start},
        race_setup::{Heap, load_pre},
    };
    let (Some(rom), Some(pre)) = (
        nfsgba_testkit::rom(),
        nfsgba_testkit::fixture("race-init/circuit_pre.wram.bin"),
    ) else {
        return;
    };
    let dir = pre.parent().unwrap().to_owned();
    let data = nfsgba_sim::data::GameData::parse(&rom);
    let firsts = [
        "circuit",
        "circuitb",
        "elimination",
        "golf",
        "ref",
        "refb",
        "rx7",
        "sprint",
        "sprintb",
        "wingman",
        "wingmanb",
    ];
    for name in firsts {
        let (setup, display) = load_pre(rom.clone(), &dir.join(format!("{name}_pre"))).unwrap();
        let records: Vec<[u8; 17]> = setup
            .records
            .iter()
            .map(|r| {
                let mut b = [0u8; 17];
                b[..7].copy_from_slice(&[r.spoiler, r.u_01, r.rim, r.exhaust, r.u_04, r.paint, r.glass]);
                b[7..].copy_from_slice(&r.upgrades);
                b
            })
            .collect();
        let mut other = setup.clone();
        other.heap = Heap::menus(&rom);
        other.heap.set_records(&records);
        let (mut d0, mut d1) = (display.clone(), display.clone());
        let (w0, w1) = (
            race_start(&rom, &data, &setup, 6, &mut d0).unwrap(),
            race_start(&rom, &data, &other, 6, &mut d1).unwrap(),
        );
        assert_eq!(arena_view(&w0), arena_view(&w1), "{name}: atlases and node table");
        let (mut g0, mut g1) = (
            start(rom.clone(), &setup, display.clone(), 6).unwrap(),
            start(rom.clone(), &other, display, 6).unwrap(),
        );
        for f in 0..900u32 {
            let keys = if f < 250 {
                0
            } else {
                1 | [0x10u16, 0x20, 0][(f as usize / 40) % 3]
            };
            let (r0, r1) = (
                g0.frame(keys, &Timing::steady()).unwrap(),
                g1.frame(keys, &Timing::steady()).unwrap(),
            );
            assert!(matches!((r0, r1), (Flow::Racing, Flow::Racing)));
            assert!(g0.screen() == g1.screen() && g0.oam == g1.oam, "{name}: frame {f}");
        }
    }
}

/// A second race in a session: the first race's player atlas and rim buffer stay on the heap (as in the recorded
/// second-race starts) and the second start runs from that heap.
#[test]
fn a_second_race_starts_from_the_first_ones_heap() {
    use nfsgba_game::{
        Flow, Timing,
        race_init::start,
        race_setup::{Display, Heap, Setup},
    };
    let Some(rom) = nfsgba_testkit::rom() else { return };
    let records = [[0u8; 17]; 15];
    let mut first = Setup::menus(
        &rom,
        nfsgba_audio::Engine::new(nfsgba_audio::Rom(&rom), 16, 16),
        100,
        0,
        &records,
        None,
    );
    first.choose(1, 3, 0, 0);
    let display = Display::from_screen(&Default::default());
    let mut game = start(rom.clone(), &first, display.clone(), 6).unwrap();
    for _ in 0..300 {
        assert!(matches!(game.frame(0, &Timing::steady()).unwrap(), Flow::Racing));
    }
    let previous = Heap::after_race(&rom, &game.world).expect("the atlas and the rim buffer stay allocated");
    let mut second = Setup::menus(&rom, game.world.audio.clone(), 200, 0, &records, Some(&previous));
    second.choose(1, 3, 0, 0);
    assert_eq!(second.atlases[0], previous.1[0]);
    let mut game = start(rom.clone(), &second, display, 6).unwrap();
    for _ in 0..300 {
        assert!(matches!(game.frame(0, &Timing::steady()).unwrap(), Flow::Racing));
    }
}

/// START, then the quit item (RIGHT, A) and YES (A) on its question: the race ends and the menus come back on the
/// screen the original shows after the same presses (`coverage3/cov-pause-quit.png`, the headless mGBA run's last frame:
/// the Quick Play menu), with the race's palettes gone.
#[test]
fn pause_quit_returns_to_the_menus() {
    let (Some(rom), Some(text)) = (
        nfsgba_testkit::rom(),
        nfsgba_testkit::read_to_string("session/quickplay.json"),
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
    let mut s = Session::new(&rom, vec![0xFF; 512]);
    let sync_at = t["sync_at"].as_u64().unwrap() as usize;
    for (f, keys) in held.iter().enumerate().take(2700) {
        if f == sync_at {
            sync_choice(&mut s, &frames[f]);
        }
        s.frame(*keys).unwrap();
    }
    let run = |s: &mut Session, k: u16, n: usize| {
        for _ in 0..n {
            s.frame(k).unwrap();
        }
    };
    run(&mut s, 0, 200);
    for (keys, wait) in [(0x8, 60), (0x10, 30), (0x1, 30), (0x1, 120)] {
        assert!(s.race.is_some(), "the race is still there");
        run(&mut s, keys, 12);
        run(&mut s, 0, wait);
    }
    assert!(s.race.is_none() && !s.racing(), "the race ended");
    assert!(s.keeps_the_race_heap(), "the race's heap is kept for the next race");
    // The Quick Play menu (Random / choose a car), as the original's last frame shows it.
    assert_eq!(s.st.g.screen, 0x1C, "the screen after the quit");
    if let Ok(d) = std::env::var("PAUSE_DUMP") {
        let v = s.view();
        std::fs::write(format!("{d}/quit.page"), v.page).unwrap();
        let pal: Vec<u8> = v.palette.iter().flat_map(|c| c.to_le_bytes()).collect();
        std::fs::write(format!("{d}/quit.pal"), pal).unwrap();
    }
}
