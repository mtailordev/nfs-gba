//! The full game inside the viewer's `play` system.

use std::time::Duration;

use bevy::time::TimeUpdateStrategy;
use serde_json::Value;

use super::*;

/// The full game inside the viewer's `play` system (no window, no GPU): the key plan of
/// `session_matches_the_game` (`tests/session.rs`, `session/quickplay.json`) from power-on, the race lent to
/// `Play::game` while it runs, the player marked finished after 150 game frames as there. Asserts a race started
/// (a new identity), the menus took the game back with the results, and the save was written.
#[test]
fn the_full_game_reaches_the_race_and_the_results() {
    let (Some(rom), Some(text), Some(_)) = (
        nfsgba_testkit::rom(),
        nfsgba_testkit::read_to_string("session/quickplay.json"),
        nfsgba_testkit::fixture("race-init/circuit_pre.wram.bin"),
    ) else {
        return;
    };
    let t: Value = serde_json::from_str(&text).unwrap();
    let mut held = vec![0u16; 4600];
    for press in t["script"].as_array().unwrap() {
        let key = KEYS.iter().position(|k| *k == press[2].as_str().unwrap()).unwrap();
        let first = press[0].as_u64().unwrap() as usize;
        held[first..first + press[1].as_u64().unwrap() as usize]
            .iter_mut()
            .for_each(|h| *h |= 1 << key);
    }
    let sav = std::env::temp_dir().join("nfsgba-viewer-test.sav");
    let _ = std::fs::remove_file(&sav);
    let mut play = Play::grid(rom.clone(), 11, 23, Handle::default(), false)
        .unwrap()
        .with_full(Full::new(rom.clone(), Some(sav.clone())));
    play.script = Some(held);
    let first_id = play.id;
    let race = Race {
        routes: rom::routes(&rom),
        floors: vec![],
        current: 0,
        active: false,
        hud: Handle::default(),
        game_camera: false,
        original: false,
        frame: None,
        visible: None,
        drawn: vec![],
        setup: view::RaceView::read(&play.game.world),
        portals: Handle::default(),
    };
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
            1.0 / VIDEO_HZ,
        )))
        .init_resource::<ButtonInput<KeyCode>>()
        .insert_resource(play)
        .insert_resource(race)
        .add_systems(Update, super::play);
    let (mut raced, mut poked, mut results) = (false, false, false);
    let mut seen_race_active = false;
    for _ in 0..30_000 {
        app.update();
        seen_race_active |= app.world().resource::<Race>().active;
        let mut play = app.world_mut().resource_mut::<Play>();
        assert!(play.stopped.is_none(), "stopped: {:?} {:?}", play.stopped, play.banner);
        if play.in_race() {
            raced = true;
            let world = &mut play.game.world;
            if world.lp.game_state == 5 && world.g.race_frames >= 150 && !poked {
                let p = world.g.player as usize;
                world.slots[p].e.race_state = 2;
                poked = true;
            }
        } else if raced {
            results = true;
            break;
        }
    }
    assert!(raced, "a race started");
    assert!(seen_race_active, "the race was shown");
    assert!(results, "the race handed back to the menus");
    let (play, race) = (app.world().resource::<Play>(), app.world().resource::<Race>());
    assert_ne!(play.id, first_id, "the race has its own identity");
    assert!(!race.active, "the menus are on screen");
    let screen = play.full.as_ref().unwrap().session.st.g.screen;
    assert!(matches!(screen, 0xB | 0xC), "the results screens, not {screen:#x}");
    assert!(sav.exists(), "the profile was saved on the way");
}
