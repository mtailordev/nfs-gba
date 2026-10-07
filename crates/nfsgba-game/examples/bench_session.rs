//! Times the session's frames: the `session/quickplay.json` key plan from power-on into a Quick Play race, then
//! `RACE` more race frames at full throttle. Prints the menus' and the race's cost per frame (mean, p99, max).
//! `cargo run --release -p nfsgba-game --example bench_session`

use std::time::Instant;

use nfsgba_game::session::Session;

const KEYS: [&str; 10] = ["A", "B", "SELECT", "START", "RIGHT", "LEFT", "UP", "DOWN", "R", "L"];
const RACE: usize = 2000;

fn report(name: &str, mut t: Vec<f64>) {
    t.sort_by(f64::total_cmp);
    let mean = t.iter().sum::<f64>() / t.len() as f64;
    let p99 = t[t.len() * 99 / 100];
    println!(
        "{name}: {} frames, mean {mean:.3} ms, p99 {p99:.3} ms, max {:.3} ms",
        t.len(),
        t[t.len() - 1]
    );
}

fn main() {
    let rom = nfsgba_testkit::rom().expect("the ROM");
    let text = nfsgba_testkit::read_to_string("session/quickplay.json").expect("session/quickplay.json");
    let t: serde_json::Value = serde_json::from_str(&text).unwrap();
    let mut held = vec![0u16; 4600];
    for press in t["script"].as_array().unwrap() {
        let key = KEYS.iter().position(|k| *k == press[2].as_str().unwrap()).unwrap();
        let first = press[0].as_u64().unwrap() as usize;
        held[first..first + press[1].as_u64().unwrap() as usize]
            .iter_mut()
            .for_each(|h| *h |= 1 << key);
    }
    let mut s = Session::new(&rom, vec![0xFF; 512]);
    let (mut menus, mut race) = (vec![], vec![]);
    let mut f = 0;
    while race.len() < RACE {
        let racing = s.race.is_some();
        let keys = if racing { 1 } else { held.get(f).copied().unwrap_or(0) };
        let t0 = Instant::now();
        s.frame(keys).unwrap();
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        if racing && s.race.is_some() {
            race.push(ms);
        } else if !racing {
            menus.push(ms);
        }
        f += 1;
    }
    report("menus (one per video frame, budget 16.7 ms)", menus);
    report("race (one per 4 video frames, budget 67 ms)", race);
}
