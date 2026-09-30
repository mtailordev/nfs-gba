//! The full game inside the viewer's `play` system.

use std::time::Duration;

use bevy::time::TimeUpdateStrategy;
use serde_json::Value;

use super::*;

/// The full game inside the viewer's `play` system (no window, no GPU): the key plan of
/// `session_matches_the_game` (`tests/session.rs`, `session/quickplay.json`) from power-on, the race lent to
/// `Play::game` (through the spare, `Play::spare`) while it runs, the player marked finished after 150 game frames as there. Asserts a race started
/// (a new identity), the menus took the game back with the results, and the save was written.
#[test]
fn the_full_game_reaches_the_race_and_the_results() {
    let (Some(rom), Some(text)) = (
        nfsgba_testkit::rom(),
        nfsgba_testkit::read_to_string("session/quickplay.json"),
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
    // The spare as `main` makes it (no capture: the web build has none).
    let mut play = Play::spare(rom.clone(), 11, 23, Handle::default())
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
        clip: vec![],
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

/// R30: the lights and flames the game places through a car's matrix slot carry that slot (the sprites are drawn and
/// freed inside the frame, so the owners are read after it; `pool_owner` keeps the last owner of each pool object).
#[test]
fn effect_sprites_know_their_car() {
    let (Some(rom), Some(dir)) = (nfsgba_testkit::rom(), nfsgba_testkit::fixture("live-race")) else {
        return;
    };
    let trace = nfsgba_game::trace::Trace::load(&dir, "live").unwrap();
    let mut game = crate::shots::game_at(&rom, &trace, 0);
    let (mut owned, mut named) = (0, 0);
    for timing in &trace.timing {
        game.world.pool_owner.0.clear();
        game.frame(0, timing).unwrap();
        let w = &game.world;
        // An owner set this frame is a matrix slot some entity holds this frame.
        for &o in w.pool_owner.0.iter().filter(|&&o| o != 0xFF) {
            assert!(o < 64, "not a matrix slot: {o}");
            owned += 1;
            named += w.slots.iter().any(|s| s.e.slot == o) as u32;
        }
    }
    assert!(owned > 0, "no effect sprite had an owner");
    // (Cleared before each frame, so every owner was set by this frame.)
    assert_eq!(named, owned, "owners that name no entity");
}

/// R29: a paused route start shows the game's own tint. `paused_palette` equals what `Game::tint` writes to palette
/// RAM once the start's fade is over (all 256 colours), the tint changed the loaded palette, and the race's blend
/// register is the game's (`Game::bldalpha`, 0x0D0F).
#[test]
fn a_paused_start_is_tinted_by_the_game() {
    let (Some(rom), Some(_)) = (
        nfsgba_testkit::rom(),
        nfsgba_testkit::fixture("race-init/circuit_pre.wram.bin"),
    ) else {
        return;
    };
    let play = Play::grid(rom.clone(), 11, 23, Handle::default(), false).unwrap();
    let shown = paused_palette(&play.game);
    // The same start again (deterministic), on which the game's own `tint` runs.
    let mut tinted = Play::grid(rom, 11, 23, Handle::default(), false).unwrap().game;
    tinted.world.g.fade = 0;
    tinted.tint();
    let ram: Vec<u16> = tinted
        .palette
        .chunks(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    assert_eq!(shown, ram[..256], "the paused start's palette is the game's tint");
    let loaded: Vec<u16> = play
        .game
        .palette
        .chunks(2)
        .take(256)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    assert_ne!(shown, loaded, "the tint changed the palette");
    assert_eq!(play.game.bldalpha, 0x0D0F);
}
