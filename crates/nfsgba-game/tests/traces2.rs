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
            if let Some(n) = frames
                .get(k + 1)
                .filter(|n| n.state == 1 && n.screen == f.screen && f.screen == screen)
            {
                if s.st.g.screen == screen && s.race.is_none() {
                    assert_eq!(
                        s.st.g.rand_index, n.rand,
                        "{name}: main_frame {k}: the frame's draws (screen {screen})"
                    );
                    compared += 1;
                }
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
        let records = s.st.profile.car_records.clone();
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
