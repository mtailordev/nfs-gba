//! Bake-off B1: `race_start` on a synthesized pre-state; writes `synth/NAME.<domain>.bin` (an oracle snapshot).
//! `tools/oracle/synth.py` makes the pre-state and checks the result against the game's own code.
use nfsgba_game::{Machine, race_init};

fn main() {
    let name = std::env::args().nth(1).expect("NAME");
    let rom = nfsgba_testkit::rom().expect("rom");
    let dir = nfsgba_testkit::fixture("synth").expect("synth");
    let mut g = Machine::load_dump(rom, &dir.join(format!("{name}_pre"))).unwrap();
    let mut io: race_init::Io = std::fs::read(dir.join(format!("{name}_pre.io.bin"))).unwrap()[..0x400]
        .try_into()
        .unwrap();
    race_init::race_start(&mut g, &mut io, 0).unwrap();
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
    println!("wrote synth/{name}.*");
}
