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

/// `fill_results` (`0x0812eaac`, with `results_tiebreak` and `finish_time_estimate` below it) against the game's
/// own code, from the reference race with the state it reads set per case (`tools/oracle/cases.py game-end`,
/// `game-loop/end_cases.txt`, `end_NAME.*`): the results block, the ranked block's finish times, and each driver's
/// best lap, finish time and laps must equal the oracle's.
#[test]
fn fill_results_matches_the_game() {
    use nfsgba_sim::{layout::Field, state::RaceResults};
    let Some(start) = machine("mgba/race") else { return };
    let Some(cases) = nfsgba_testkit::read_to_string("game-loop/end_cases.txt") else {
        return;
    };
    let (mut estimated, mut ties) = (0, 0);
    for line in cases.lines() {
        let f: Vec<i64> = line.split(' ').skip(1).map(|v| v.parse().unwrap()).collect();
        let name = line.split(' ').next().unwrap();
        let Some(want) = machine(&format!("game-loop/end_{name}")) else {
            return;
        };
        let mut game = Game::new(start.clone());
        let w = &mut game.world;
        (w.g.phase, w.g.opponents, w.g.circuit, w.g.laps, w.g.time) =
            (f[0] as i32, f[1] as u32, f[2] as u32, f[3] as i32, f[4] as u32);
        let mut results = game.results();
        for i in 0..4 {
            let r = &f[5 + 6 * i..11 + 6 * i];
            results.finish[i] = r[0] as u32;
            let c = &mut game.world.slots[i].c;
            (c.best_lap, c.progress, c.hunter_life, c.laps_left) = (r[1] as i32, r[2] as i32, r[3] as i32, r[4] as i8);
            game.ranked.finish[i] = r[5] as u32;
        }
        let b = results.to_bytes();
        game.world.g.results.copy_from_slice(&b[..32]);
        game.world.g.results_b.copy_from_slice(&b[32..]);
        estimated += (0..=f[1] as usize).filter(|&i| f[5 + 6 * i] == 0).count();
        ties += (0..f[1] as usize)
            .filter(|&i| f[5 + 6 * i + 5] == f[5 + 6 * (i + 1) + 5])
            .count();
        game.fill_results();
        let want_world = World::load(&want);
        assert_eq!(
            game.results(),
            RaceResults::load(&want.mem, 0x0300_5650),
            "{name}: results"
        );
        assert_eq!(
            game.ranked,
            RaceResults::load(&want.mem, 0x0300_5730),
            "{name}: ranked results"
        );
        assert_eq!(game.world.g.laps, want_world.g.laps, "{name}: laps");
        for i in 0..4 {
            let (c, d) = (&game.world.slots[i], &want_world.slots[i]);
            assert_eq!(
                (c.c.best_lap, c.c.u_0bc, c.c.laps_left, c.c.hunter_life),
                (d.c.best_lap, d.c.u_0bc, d.c.laps_left, d.c.hunter_life),
                "{name}: racer {i}"
            );
        }
    }
    assert!(
        estimated > 0 && ties > 0,
        "the cases estimate finish times and break ties"
    );
}

/// `race_menu_palette_setup` (`0x081372e4`) against the game's own code: both pages cleared, the base palette
/// rebuilt (the fade's target keeps the level's), and with a phase other than 5 the VCount IRQ back on with the
/// sky gradient restarted.
#[test]
fn race_menu_palette_setup_matches_the_game() {
    let Some(start) = machine("mgba/race") else { return };
    for (name, phase) in [("palette_setup", 1), ("palette_setup_preview", 5)] {
        let prefix = format!("game-loop/end_{name}");
        let Some(want) = machine(&prefix) else { return };
        let Some(dir) = nfsgba_testkit::dump(&prefix) else {
            return;
        };
        let io = nfsgba_formats::Dump::load(&dir).unwrap().io;
        let mut game = Game::new(start.clone());
        game.world.g.phase = phase;
        game.vcount_irq = false;
        game.world.gradient_start = 20;
        game.vram[..0x9600].fill(7);
        game.vram[0xA000..0xA000 + 0x9600].fill(7);
        game.race_menu_palette_setup();
        let want_world = World::load(&want);
        assert_eq!(game.world.palette_base, want_world.palette_base, "{name}: base palette");
        assert_eq!(game.world.palette_fade, want_world.palette_fade, "{name}: fade palette");
        assert!(game.vram == want.vram, "{name}: pages");
        assert_eq!(game.vcount_irq, io[4] & 0x20 != 0, "{name}: VCount IRQ");
        assert_eq!(
            game.world.gradient_start, want_world.gradient_start,
            "{name}: gradient start"
        );
    }
}
