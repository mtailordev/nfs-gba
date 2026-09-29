//! Bake-off B1: a synthesized race start. `synth_race NAME ENV ROUTE MODE CAR [check]` builds the typed race start
//! (`Setup::choose` on the `race-init/circuit_pre` capture, `race_start`) and writes the capture with the same
//! setup poked in as `synth/NAME_pre.<domain>.bin`. `tools/oracle/synth.py check NAME` runs the game's own
//! `race_start_from_table_a` on that pre-state (`synth/NAME_oracle.*`); `check` then compares our `World` with
//! `World::load` of the oracle's result.
use nfsgba_game::{
    Machine, race_init,
    race_setup::{Setup, load_pre},
    world::World,
};
use nfsgba_sim::data::GameData;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let [name, env, route, mode, car, rest @ ..] = &a[1..] else {
        panic!("usage: synth_race NAME ENV ROUTE MODE CAR [check]")
    };
    let n = |s: &String| s.parse::<u32>().expect("number");
    let (env, route, mode, car) = (n(env), n(route), n(mode), n(car) as u8);
    let rom = nfsgba_testkit::rom().expect("rom");
    let src = nfsgba_testkit::dump("race-init/circuit_pre").expect("race-init/circuit_pre");
    let dir = src.parent().unwrap().parent().unwrap().join("synth");
    std::fs::create_dir_all(&dir).unwrap();
    let (mut setup, mut display): (Setup, _) = load_pre(rom.clone(), &src).unwrap();
    setup.choose(env, route, mode, car);
    let world = race_init::race_start(&rom, &GameData::parse(&rom), &setup, 0, &mut display).unwrap();
    println!(
        "typed race start: {} entities, {} sectors, {} racers",
        world.slots.len(),
        world.heads.len(),
        world.g.racers + 1
    );
    if rest.first().map(String::as_str) == Some("check") {
        let oracle = World::load(&Machine::load_dump(rom, &dir.join(format!("{name}_oracle"))).unwrap());
        let fields = race_init::differing(&world, &oracle);
        let ok = fields.is_empty() && race_init::arena_view(&world) == race_init::arena_view(&oracle);
        println!(
            "{}",
            if ok {
                "race_start: oracle == Rust".into()
            } else {
                format!("MISMATCH: {fields:?}")
            }
        );
        std::process::exit(i32::from(!ok));
    }
    // The pre-state for the oracle: the capture with the setup poked into the words the menus write.
    let mut g = Machine::load_dump(rom, &src).unwrap();
    let m = &mut g.mem;
    m.set_u32(0x0300_006C, env);
    m.set_u32(0x0300_5388, route);
    m.set_u32(0x0300_5720, route);
    m.set_u32(0x0300_56E0, mode);
    m.set_u8(0x0300_611C, car);
    let io = std::fs::read(format!("{}.io.bin", src.display())).unwrap();
    for (d, b) in [
        ("wram", &g.mem.ewram[..]),
        ("iwram", &g.mem.iwram[..]),
        ("io", &io[..]),
        ("palette", &g.palette[..]),
        ("vram", &g.vram[..]),
        ("oam", &g.oam[..]),
    ] {
        std::fs::write(dir.join(format!("{name}_pre.{d}.bin")), b).unwrap();
    }
    println!("wrote synth/{name}_pre.*");
}
