use super::*;
use crate::menu::{Gba, adapt, intro};
use nfsgba_testkit::{dump, read_to_string, rom};

/// A settled boot screen redrawn by the typed host: the screen entered from `st` (screen 48, the blinking health and
/// safety page, has no enter handler: it is 47 entered, then the update's step to 48), the state the enter resets
/// (name, keyboard cursor) put back, and the blink colour of the capture.
fn redraw<'a>(rom: &'a [u8], st: &mut MenuState, blink: u16) -> TypedHost<'a> {
    let live = st.clone();
    if live.g.screen == 48 {
        st.g.screen = 47;
    }
    let mut h = enter(rom, st);
    st.g.screen = live.g.screen;
    (st.g.name, st.g.name_len) = (live.g.name, live.g.name_len);
    (st.g.keyboard_row, st.g.keyboard_column) = (live.g.keyboard_row, live.g.keyboard_column);
    if live.g.screen == 48 {
        h.set_second_colour(4, blink);
    }
    flow::draw_screen(st, &mut h, 1);
    h
}

/// Page bytes, base colours and OAM entries of the host that differ from the capture.
fn diff(h: &TypedHost, shot: &Screen) -> [usize; 3] {
    [
        h.screen.pages[1].iter().zip(shot.shown()).filter(|(a, b)| a != b).count(),
        h.scene.palettes[0].iter().zip(&shot.palette[..256]).filter(|(a, b)| a != b).count(),
        h.screen.oam.iter().zip(&shot.oam).filter(|(a, b)| a != b).count(),
    ]
}

/// The whole-scene check (`tools/oracle/scene_power_on.py`): a headless power-on run, each menu screen's settled
/// state redrawn by the typed host from its state and compared with the shown page, the base palette and the
/// shadow OAM of the capture.
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
        let h = redraw(&rom, &mut st, shot.palette[4]);
        let d = diff(&h, &shot);
        eprintln!(
            "frame {frame} screen {screen}: {} of 38400 page bytes, {} of 256 base colours, {} of 128 OAM entries differ; unported calls {:x?}",
            d[0], d[1], d[2], h.calls
        );
        assert_eq!(d, [0; 3], "screen {screen}: page, base palette, shadow OAM");
    }
}

/// Consecutive frames (`tools/oracle/scene_frames.py`, menus4): screen 48 (the health page with colour 4 blinking, 24
/// frames) and the name keyboard (22, 60 frames with cursor moves, two letters and a delete). Every frame is redrawn
/// from its own state and equals the capture; on 48 the typed update steps the blink once per frame, and its colour
/// is the capture's every frame.
#[test]
fn power_on_frames_match_the_game() {
    let Some(rom) = rom() else { return };
    let Some(index) = read_to_string("menus4/pf-index.json") else {
        return;
    };
    let snaps: Vec<(u32, u32)> = serde_json::from_str(&index).unwrap();
    let mut health: Option<(MenuState, TypedHost, u16)> = None;
    let (mut history, mut lag) = (Vec::new(), Vec::new());
    for (n, (frame, screen)) in snaps.iter().enumerate() {
        let mut g = Gba::from_dump(rom.clone(), &dump(&format!("menus4/pf-{screen}-{n}")).unwrap()).unwrap();
        let shot = adapt::screen_of(&g);
        let st = adapt::load_state(&mut g);
        assert_eq!(st.g.screen, *screen);
        // The shown page was drawn from this frame's state or a few frames' before it (the game may still be drawing
        // the other page; T1): the newest state that gives the shown page is taken.
        let mut d = [1; 3];
        for (age, s) in std::iter::once(&st).chain(history.iter().rev().take(4)).enumerate() {
            d = diff(&redraw(&rom, &mut s.clone(), shot.palette[4]), &shot);
            if d == [0; 3] {
                lag.push(age);
                break;
            }
        }
        assert_eq!(d, [0; 3], "frame {frame} screen {screen}: page, base palette, shadow OAM");
        history.push(st.clone());
        if *screen != 48 {
            continue;
        }
        // The typed sequence: the first frame's state, then one `intro::update` per video frame. The capture is taken
        // after the frame's update and before its palette copy (`main_frame` copies at the start of the next): the
        // blink colour in RAM is the typed step's, the shown palette is the step before.
        let ram_c4 = g.u16(st.g.second_palette + 8);
        if let Some((s, h, shown)) = health.as_mut() {
            s.g.ticks += 1;
            s.g.keys = 0;
            intro::update(s, h);
            assert_eq!(h.second_colour(4), ram_c4, "frame {frame}: blink colour");
            assert_eq!(s.g.blink_dir, st.g.blink_dir, "frame {frame}: blink direction");
            assert_eq!(*shown, shot.palette[4], "frame {frame}: shown palette is the step before");
            *shown = ram_c4;
        } else {
            let h = redraw(&rom, &mut st.clone(), ram_c4);
            health = Some((st, h, ram_c4));
        }
    }
    eprintln!("frames behind the shown page: {lag:?}");
    assert!(lag.iter().all(|&l| l <= 2), "the shown page is up to two frames behind the state");
}

/// The garage screens (`tools/oracle/garage_capture.py`: a headless career run through the part shop 0x13, the upgrade
/// pages 0x14 and the profile 0x12): each settled screen entered by the typed host, the cursor state of the capture
/// put back, redrawn, and compared with the shown page, base palette and shadow OAM, the turntable car included.
#[test]
fn garage_screens_match_the_capture() {
    let Some(rom) = rom() else { return };
    // The career run (menus3/gar-*), and the profile screen on 30 consecutive frames (garage2/tt-*, garage_turntable.py).
    let mut all = Vec::new();
    for (index, prefix) in [
        ("menus3/gar-index.json", "menus3/gar"),
        ("garage2/tt-index.json", "garage2/tt"),
    ] {
        let Some(index) = read_to_string(index) else {
            return;
        };
        let snaps: Vec<(u32, u32)> = serde_json::from_str(&index).unwrap();
        all.extend(snaps.into_iter().enumerate().map(|(n, (f, s))| (prefix, n, f, s)));
    }
    for (prefix, n, frame, screen) in &all {
        let mut g = Gba::from_dump(rom.clone(), &dump(&format!("{prefix}-{screen}-{n}")).unwrap()).unwrap();
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
        h.car_load(&mut st);
        if *screen != 0x13 {
            h.car_palette(&mut st); // the level changes reload the palette in the game
        }
        // The shown page was drawn at the capture's angle or up to five turns before it (the game may be drawing the other page, and
        // runs several menu frames per video frame: T1); the draw turns the car first. The first count of turns
        // that gives the shown page is taken.
        let base = (st.clone(), h.clone());
        let mut wrong = Vec::new();
        for turns in 1..=6u32 {
            let (mut s, mut host) = (base.0.clone(), base.1.clone());
            s.g.garage_angle = live.g.garage_angle.wrapping_sub(0x40 * turns);
            flow::draw_screen(&mut s, &mut host, 1);
            wrong = (0..240 * 160)
                .filter(|&i| host.screen.pages[1][i] != shot.shown()[i])
                .collect();
            (st, h) = (s, host);
            if wrong.is_empty() {
                break;
            }
        }
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
            "frame {frame} angle {:#x} u2f6 {} screen {screen:#x}: {} page bytes in x {x0}..{x1} y {y0}..{y1}, base colours {pal:?}, {oam} OAM entries differ; unported calls {:x?}",
            live.g.garage_angle,
            live.profile.u_2f6,
            wrong.len(),
            h.calls.iter().map(|c| c.0).collect::<Vec<_>>()
        );
        assert!(
            pal.is_empty(),
            "screen {screen:#x}: base palette (glass slots 192, 208 included)"
        );
        assert_eq!(oam, 0, "screen {screen:#x}: shadow OAM");
        assert!(wrong.is_empty(), "screen {screen:#x}: page (car rows included)");
    }
}
