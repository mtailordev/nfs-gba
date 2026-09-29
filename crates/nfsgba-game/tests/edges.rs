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
