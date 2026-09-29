//! The game loop's sound stops against the game's own code: from the reference race (`mgba/race`), `snd_stop_all`
//! (`0x08135f38`) and `music_stop` (`0x0813609c`) must leave the sound engine and the music id as the function
//! oracle leaves them (`tools/oracle/cases.py game-edges`, `game-loop/edges_NAME.*`).

use nfsgba_game::{Game, Machine, world::World};

fn machine(prefix: &str) -> Option<Machine> {
    let rom = nfsgba_testkit::rom()?;
    let dir = nfsgba_testkit::dump(prefix)?;
    Some(Machine::load_dump(rom, &dir).unwrap())
}

#[test]
fn sound_stops_match_the_game() {
    let Some(start) = machine("mgba/race") else { return };
    let Some(stop_all) = machine("game-loop/edges_stop_all") else {
        return;
    };
    let Some(music_stop) = machine("game-loop/edges_music_stop") else {
        return;
    };
    let before = World::load(&start);
    assert!(before.lp.music_id >= 0, "the reference race plays music");
    for (name, want, run) in [
        ("snd_stop_all", stop_all, Game::snd_stop_all as fn(&mut Game)),
        ("music_stop", music_stop, Game::music_stop),
    ] {
        let mut game = Game::new(start.clone());
        run(&mut game);
        let want = World::load(&want);
        assert_eq!(want.lp.music_id, -1, "{name}: music id");
        assert_eq!(game.world.lp.music_id, want.lp.music_id, "{name}: music id");
        assert!(
            game.world.audio == want.audio,
            "{name}: the sound engine differs from the game's"
        );
        assert!(game.world.audio != before.audio, "{name}: the call changes something");
    }
}

/// `race_start_from_table_b` (`0x0813af7c`) against the game's own code, from the reference race with the state it
/// reads set per case (`tools/oracle/cases.py game-countdown`, `game-loop/countdown_*`): the phase, the start
/// state, the countdown accumulator, the effect sprite pool and the OBJ tiles must equal the oracle's.
#[test]
fn countdown_matches_the_game() {
    let Some(start) = machine("mgba/race") else { return };
    let Some(cases) = nfsgba_testkit::read_to_string("game-loop/countdown_cases.txt") else {
        return;
    };
    let mut tiles = 0;
    for line in cases.lines() {
        let f: Vec<&str> = line.split(' ').collect();
        let n = |i: usize| f[i].parse::<i32>().unwrap();
        let Some(want) = machine(&format!("game-loop/countdown_{}", f[0])) else {
            return;
        };
        let mut game = Game::new(start.clone());
        let w = &mut game.world;
        (w.g.phase, w.g.fade, w.lp.start_state) = (n(1) as _, n(2), n(3) as u32);
        (w.hud.race_state_changed, w.g.dt, game.dispcnt) = (n(4) as u32, n(5), n(6) as u16);
        game.vram[0x1_3000..0x1_8000].fill(0);
        game.race_start_from_table_b().unwrap();
        let want_world = World::load(&want);
        let (g, w) = (&game.world, &want_world);
        assert_eq!(g.g.phase, w.g.phase, "{}: phase", f[0]);
        assert_eq!(g.lp.start_state, w.lp.start_state, "{}: start state", f[0]);
        assert_eq!(
            g.hud.race_state_changed, w.hud.race_state_changed,
            "{}: accumulator",
            f[0]
        );
        assert!(g.pool == w.pool, "{}: the effect sprites", f[0]);
        assert!(game.vram == want.vram, "{}: OBJ tiles", f[0]);
        tiles += game.vram[0x1_3000..].iter().filter(|&&b| b != 0).count();
    }
    assert!(tiles > 0, "some case uploads tiles");
}
