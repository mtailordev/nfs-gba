use super::*;
use crate::menu::{Gba, adapt};
use nfsgba_testkit::{dump, read_to_string, rom};

/// The whole-scene check (`tools/oracle/scene_power_on.py`): a headless power-on run, each menu screen's settled
/// state redrawn by the typed host from its state and compared with the shown page, the base palette and the
/// shadow OAM of the capture. Screen 48 (the credits) scrolls and has no enter handler; on 22 (the name) and 0 (the main
/// menu) a few pixels differ: the name cursor and the list items new-mark (`0x081300E0`, not ported: U7).
#[test]
fn power_on_screens_match_the_game() {
    let Some(rom) = rom() else { return };
    let Some(index) = read_to_string("menus3/po-index.json") else {
        return;
    };
    let snaps: Vec<(u32, u32)> = serde_json::from_str(&index).unwrap();
    for (n, (frame, screen)) in snaps.iter().enumerate() {
        let mut g = Gba::from_dump(rom.clone(), &dump(&format!("menus3/po-{screen}-{n}")).unwrap()).unwrap();
        let shot = adapt::screen_of(&g);
        let mut st = adapt::load_state(&mut g);
        assert_eq!(st.g.screen, *screen);
        let h = enter(&rom, &mut st);
        let wrong = h.screen.pages[1]
            .iter()
            .zip(shot.shown())
            .filter(|(a, b)| a != b)
            .count();
        let pal = h.scene.palettes[0]
            .iter()
            .zip(&shot.palette[..256])
            .filter(|(a, b)| a != b)
            .count();
        let oam = h.screen.oam.iter().zip(&shot.oam).filter(|(a, b)| a != b).count();
        eprintln!(
            "frame {frame} screen {screen}: {wrong} of 38400 page bytes, {pal} of 256 base colours, {oam} of 128 OAM entries differ; unported calls {:x?}",
            h.calls
        );
        assert_eq!(pal, 0, "screen {screen}: base palette");
        if *screen != 48 {
            assert_eq!(oam, 0, "screen {screen}: shadow OAM");
            assert!(
                wrong <= if matches!(*screen, 0 | 22) { 64 } else { 0 },
                "screen {screen}: page"
            );
        }
    }
}
