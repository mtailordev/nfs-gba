//! Bake-off B1: a synthesized race start. `synth_race NAME ENV ROUTE MODE CAR` pokes the setup into
//! `race-init/circuit_pre` (`race_init::apply_setup`), writes that pre-state as `synth/NAME_pre.<domain>.bin`, runs
//! `race_start` and writes the result as `synth/NAME.<domain>.bin` (oracle snapshots). `tools/oracle/synth.py check`
//! compares it with the game's own code.
use nfsgba_game::{Machine, race_init};

fn write(dir: &std::path::Path, name: &str, g: &Machine, io: &race_init::Io) {
    for (d, b) in [
        ("wram", &g.mem.ewram[..]),
        ("iwram", &g.mem.iwram[..]),
        ("io", &io[..]),
        ("palette", &g.palette[..]),
        ("vram", &g.vram[..]),
        ("oam", &g.oam[..]),
    ] {
        std::fs::write(dir.join(format!("{name}.{d}.bin")), b).unwrap();
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let [name, env, route, mode, car] = &a[1..] else {
        panic!("usage: synth_race NAME ENV ROUTE MODE CAR")
    };
    let n = |s: &String| s.parse::<u32>().expect("number");
    let rom = nfsgba_testkit::rom().expect("rom");
    let src = nfsgba_testkit::dump("race-init/circuit_pre").expect("race-init/circuit_pre");
    let dir = src.parent().unwrap().parent().unwrap().join("synth");
    std::fs::create_dir_all(&dir).unwrap();
    let (mut g, mut io) = race_init::load_pre(rom, &src).unwrap();
    race_init::apply_setup(&mut g, n(env), n(route), n(mode), n(car) as u8);
    write(&dir, &format!("{name}_pre"), &g, &io);
    race_init::race_start(&mut g, &mut io, 0).unwrap();
    write(&dir, name, &g, &io);
    println!("wrote synth/{name}_pre.* and synth/{name}.*");
}
