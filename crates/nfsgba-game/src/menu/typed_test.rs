use super::*;
use crate::menu::{Gba, adapt};
use nfsgba_testkit::{dump, read_to_string, rom};

/// The whole-scene check (`tools/oracle/scene_power_on.py`): a headless power-on run, each menu screen's settled
/// state redrawn by the typed host from its state and compared with the shown page, the base palette and the
/// shadow OAM of the capture. Screen 48 (the credits) scrolls and has no enter handler; on 22 (the name) a few pixels
/// differ: the name cursor.
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
            assert!(wrong <= if *screen == 22 { 64 } else { 0 }, "screen {screen}: page");
        }
    }
}

/// The garage screens (`tools/oracle/garage_capture.py`: a headless career run through the part shop 0x13, the upgrade
/// pages 0x14 and the profile 0x12): each settled screen entered by the typed host, the cursor state of the capture
/// put back, redrawn, and compared with the shown page, base palette and shadow OAM. The car model
/// (`garage_draw_car`, a software 3D draw: FIDELITY U3) is the only difference allowed, inside its box.
#[test]
fn garage_screens_match_the_capture() {
    let Some(rom) = rom() else { return };
    let Some(index) = read_to_string("menus3/gar-index.json") else {
        return;
    };
    let snaps: Vec<(u32, u32)> = serde_json::from_str(&index).unwrap();
    for (n, (frame, screen)) in snaps.iter().enumerate() {
        let mut g = Gba::from_dump(rom.clone(), &dump(&format!("menus3/gar-{screen}-{n}")).unwrap()).unwrap();
        let shot = adapt::screen_of(&g);
        let mut st = adapt::load_state(&mut g);
        assert_eq!(st.g.screen, *screen);
        let live = st.clone();
        let mut h = enter(&rom, &mut st);
        st.g.garage_row = live.g.garage_row;
        st.g.garage_picks = live.g.garage_picks;
        st.g.garage_car = live.g.garage_car;
        st.profile.u_338 = live.profile.u_338;
        st.profile.repeats = live.profile.repeats;
        if *screen != 0x13 {
            h.car_palette(&mut st); // the level changes reload the palette in the game
        }
        flow::draw_screen(&mut st, &mut h, 1);
        let wrong: Vec<usize> = (0..240 * 160)
            .filter(|&i| h.screen.pages[1][i] != shot.shown()[i])
            .collect();
        let (x0, x1) = wrong
            .iter()
            .fold((240, 0), |(a, b), i| (a.min(i % 240), b.max(i % 240)));
        let (y0, y1) = wrong
            .iter()
            .fold((160, 0), |(a, b), i| (a.min(i / 240), b.max(i / 240)));
        let pal: Vec<usize> = (0..256)
            .filter(|&i| h.scene.palettes[0][i] != shot.palette[i])
            .collect();
        let oam = h.screen.oam.iter().zip(&shot.oam).filter(|(a, b)| a != b).count();
        eprintln!(
            "frame {frame} screen {screen:#x}: {} page bytes in x {x0}..{x1} y {y0}..{y1}, base colours {pal:?}, {oam} OAM entries differ; unported calls {:x?}",
            wrong.len(),
            h.calls.iter().map(|c| c.0).collect::<Vec<_>>()
        );
        // The glass colours (slots 192 and 208) are `shade_car_paint`'s, part of `garage_draw_car`.
        assert!(
            pal.iter().all(|&i| (192..208).contains(&i) || i == 208),
            "screen {screen:#x}: base palette"
        );
        assert_eq!(oam, 0, "screen {screen:#x}: shadow OAM");
        if *screen == 0x13 {
            assert!(wrong.is_empty(), "screen 0x13: page");
        } else {
            // The car model turns on its turntable and fills most of the top 120 rows; the rest of the page is exact.
            assert!(
                wrong.is_empty() || (y0 >= 16 && y1 < 120),
                "screen {screen:#x}: page differs outside the car rows"
            );
        }
    }
}
