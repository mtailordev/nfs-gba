//! Reachability proofs for the `Unported` stops that stay (FIDELITY N1): scans of the ROM's code and tables showing
//! that no key, menu, route, car, mode or career record produces what a stop refuses. Code facts are pinned as ROM
//! offsets (literal-pool words, Thumb `BL` calls); the store each site makes is from the decompile
//! (`tools/decomp_show.py`) and the disassembly, named beside it. A missed writer or a different ROM fails here.

use nfsgba_formats::career::{ROUTE_COUNT, events, race_slots, route_sections, route_track_slot, setup_screens};

const ROM_BASE: u32 = 0x0800_0000;

fn u16_at(rom: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([rom[o], rom[o + 1]])
}

fn u32_at(rom: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(rom[o..o + 4].try_into().unwrap())
}

/// Every 4-aligned ROM offset holding `word` (literal pools and data tables).
fn words(rom: &[u8], word: u32) -> Vec<usize> {
    (0..rom.len() - 3)
        .step_by(4)
        .filter(|&o| u32_at(rom, o) == word)
        .collect()
}

/// Every Thumb `BL` (at a 2-aligned offset) whose target is `target`.
fn bl_calls(rom: &[u8], target: u32) -> Vec<usize> {
    (0..rom.len() - 4)
        .step_by(2)
        .filter(|&o| {
            let (hi, lo) = (u16_at(rom, o) as u32, u16_at(rom, o + 2) as u32);
            if hi & 0xF800 != 0xF000 || lo & 0xF800 != 0xF800 {
                return false;
            }
            let off = ((hi & 0x7FF) << 21 | (lo & 0x7FF) << 10) as i32 >> 9; // sign-extended, ×2
            (ROM_BASE + o as u32 + 4).wrapping_add(off as u32) == target
        })
        .collect()
}

/// Nothing calls `f` by `BL` and no word holds its Thumb address: dead code.
fn uncalled(rom: &[u8], f: u32) -> bool {
    bl_calls(rom, f).is_empty() && words(rom, f | 1).is_empty()
}

/// Route-table records (0x14 bytes at `0x7F2798`): `(templates, n_entities)`.
fn route_templates(rom: &[u8], index: usize) -> (usize, usize) {
    let at = 0x7F_2798 + 0x14 * index;
    let counts = (u32_at(rom, at + 0xC) - ROM_BASE) as usize;
    (
        (u32_at(rom, at).wrapping_sub(ROM_BASE)) as usize,
        u16_at(rom, counts + 4) as usize,
    )
}

/// R11 / `camera::dispatch`: the camera view (`0x030055F8`) is only ever 0, 2 or 4 in a race, never 7
/// (`camera_look_at_player`) nor 5.
#[test]
fn camera_view_7_is_unreachable() {
    let Some(rom) = nfsgba_testkit::rom() else { return };
    // Every reference to the view. Stores: camera_init 5 (`param_2 != 0`, 0x137930) and 4 (0x137990),
    // camera_update 2 and 0 (0x137DC8, 0x137E60), camera_toggle_view 0 <-> 2 (0x138698), camera_look_at 7
    // (0x138A08), race_init 2 (0x139AC0), the handler at 0x0814A198 (view 7 -> 0/2, 0x14A250). The rest read it.
    let sites = [
        0x137930, 0x137990, 0x137DC8, 0x137E60, 0x138010, 0x13807C, 0x1380EC, 0x1381A8, 0x138424, 0x1384FC, 0x138644,
        0x138698, 0x1387C0, 0x1389C8, 0x138A08, 0x138A80, 0x139AC0, 0x13A3EC, 0x14A218, 0x14A250, 0x14B214, 0x14BD3C,
        0x14E19C, 0x14EA48, 0x14EB90, 0x14EEC0, 0x14FBD8,
    ];
    assert_eq!(words(&rom, 0x0300_55F8), sites);
    // camera_init: from race_init with param_2 = 0 (then view 2), and from a wrapper nothing calls.
    assert_eq!(bl_calls(&rom, 0x0813_78CC), [0x1399EA, 0x13A124]);
    assert!(uncalled(&rom, 0x0813_A120));
    // camera_look_at (0x081389CC, view 7) and camera_toggle_view are called only by the entity handler at
    // 0x0814A198, which is handler table entries 0x10 and 0x37 alone.
    assert_eq!(bl_calls(&rom, 0x0813_89CC), [0x14A202]);
    assert_eq!(bl_calls(&rom, 0x0813_867C), [0x14A246]);
    assert!(bl_calls(&rom, 0x0814_A198).is_empty());
    assert_eq!(words(&rom, 0x0814_A199), [0x7F_38B8 + 4 * 0x10, 0x7F_38B8 + 4 * 0x37]);
    // camera_look_at_player itself: only the view table's entry 7 (= handler 0x40).
    assert_eq!(words(&rom, 0x0813_7AC9), [0x7F_399C + 4 * 7]);
    // No entity gets those handlers: `entity_handlers_are_ported`.
}

/// `update_entities` (lib.rs): every handler an entity can run in a race is ported. The route templates carry 0,
/// 0x28 and 4 (the menu scene, record 0), and only as entities 0..3, which `setup_race_cars` rewrites to 0, 0x29 or
/// 0xE (state 0: never run). Code writes 0xF (`spawn_wingman_marker`), 0x34 (`spawn_spark`), 0x36
/// (`traffic_spawn`), 0x33 (`FUN_0814C0B8`) and 0x16 (`FUN_0814AA48`); the last two only down a dead call chain.
#[test]
fn entity_handlers_are_ported() {
    let Some(rom) = nfsgba_testkit::rom() else { return };
    for index in 0..ROUTE_COUNT {
        let (templates, n) = route_templates(&rom, index);
        for k in 0..n {
            let e = templates + 0xA4 * k;
            let (state, handler) = (u16_at(&rom, e + 8), u16_at(&rom, e + 0x4E));
            assert!(
                k < 4 || state & 3 != 3 || matches!(handler, 0..=3 | 0xF | 0x29 | 0x34 | 0x36),
                "route record {index} template {k}: handler {handler:#x}"
            );
        }
    }
    // 0x33: FUN_0814C0B8 <- FUN_0814C32C <- FUN_0814AE2C, which nothing calls; 0x16: FUN_0814AA48, from itself and
    // handler 0x33 (FUN_0814C19C) only.
    assert_eq!(bl_calls(&rom, 0x0814_C0B8), [0x14C332]);
    assert_eq!(bl_calls(&rom, 0x0814_C32C), [0x14AE60]);
    assert!(uncalled(&rom, 0x0814_AE2C));
    assert_eq!(
        bl_calls(&rom, 0x0814_AA48),
        [0x14AAF8, 0x14C1F0, 0x14C256, 0x14C2C0, 0x14C318]
    );
    assert!(words(&rom, 0x0814_AA49).is_empty());
}

/// `slots::player_matrix`: the raised camera (`0x03006148`) is never set: all three references are loads compared
/// with 0 (camera_update twice, FUN_0814E050), and the boot clears IWRAM.
#[test]
fn raised_camera_is_unreachable() {
    let Some(rom) = nfsgba_testkit::rom() else { return };
    assert_eq!(words(&rom, 0x0300_6148), [0x1382B0, 0x138494, 0x14E0F0]);
}

/// `slots::player_matrix`: the physics orientation (`0x0300610C`) is set to 1 by race_init (0x139904) before any
/// race frame and by car_dynamics (0x13DE48); the other references read it. Nothing stores 0.
#[test]
fn physics_orientation_is_always_set() {
    let Some(rom) = nfsgba_testkit::rom() else { return };
    assert_eq!(
        words(&rom, 0x0300_610C),
        [0x139904, 0x13DE48, 0x14B254, 0x14DB44, 0x14E0EC]
    );
}

/// Link play (`0x03005624`, `lib.rs`/`slots.rs` stops): the boot stores 0 (0x12A900); the only other stores are in
/// the link-cable functions below, which nothing calls; every other reference reads it.
#[test]
fn link_play_is_unreachable() {
    let Some(rom) = nfsgba_testkit::rom() else { return };
    let sites = [
        0x12A900, 0x12AE90, 0x12B05C, 0x12EB6C, 0x139AF4, 0x13AA0C, 0x13ABCC, 0x13B9A0, 0x13BAD8, 0x13BB6C, 0x13BBEC,
        0x13BC84, 0x13BD04, 0x146C9C, 0x146D48, 0x146E64, 0x146FA4, 0x146FF4, 0x1471C0, 0x1472CC, 0x14BDE0,
    ];
    assert_eq!(words(&rom, 0x0300_5624), sites);
    for f in [
        0x0814_6BF8,
        0x0814_6CA4,
        0x0814_6E10,
        0x0814_6F60,
        0x0814_6FAC,
        0x0814_7164,
        0x0814_7250,
    ] {
        assert!(uncalled(&rom, f), "{f:#x}");
    }
}

/// `race_init::build_route` and `traffic_ai`: the route index (`0x03005720`) of a race is byte 1 of the route
/// number's record (`menu_frame` 0x12B874); 0 is written only by the boot (0x12A920) and for the menu scene
/// (`load_menu_descriptor` 0x139DC4, record 0). Route numbers come from the track slots 0..42 (menu lists, Quick
/// Play's random pick, career events) or are 1 and 3 (the tutorials): each has a racing line, so the route is never
/// index 0 and traffic always has one.
#[test]
fn every_race_route_has_a_racing_line() {
    let Some(rom) = nfsgba_testkit::rom() else { return };
    assert_eq!(
        words(&rom, 0x0300_5720),
        [
            0x12A920, 0x12B874, 0x13959C, 0x139DC4, 0x13B524, 0x13E48C, 0x13F2B8, 0x13F59C
        ]
    );
    let (slots, sections) = (race_slots(&rom), route_sections(&rom));
    assert_eq!(slots[0].route, 0);
    for slot in 0..42 {
        let route = u16_at(&rom, 0x7E_4A70 + 4 * slot + 2) as usize;
        let index = slots[route].route as usize;
        assert!(
            index != 0 && !sections[index].is_empty(),
            "track slot {slot}: route {route}"
        );
    }
    // The route number -> slot map `menu_frame` uses to flip a circuit's direction stays within the slots.
    assert!((1..=42).all(|r| route_track_slot(&rom, r) < 42));
    assert!(events(&rom).iter().all(|e| e.track_slot() < 42));
}

/// `race_init`'s HUD screen (mode > 3) and the AI's branch odds (difficulty > 2): every writer's source is in range.
/// Mode (`0x030056E0`): career events (the parser takes 0..=3 only), the RACE TYPE list's cursor (4 items), Quick
/// Play's random pick (`0x797D0A`), an editable setup item for setting 0 (none), the tutorials (0). Difficulty (`0x03005608`): a
/// career event's skill / 35, `rand % 3`, 1 (boot, free race, tutorials), the setup item for setting 4.
#[test]
fn race_modes_and_difficulties_are_in_range() {
    let Some(rom) = nfsgba_testkit::rom() else { return };
    assert!(events(&rom).iter().all(|e| (0..105).contains(&(e.skill as i8)))); // signed char, unsigned divide
    assert_eq!(u16_at(&rom, 0x7E_544C + 0x14 + 10), 4); // list slot 1 (screen 1, RACE TYPE)
    assert!(rom[0x79_7D0A..0x79_7D0E].iter().all(|&m| m <= 3));
    // Record 1 is Quick Play (0xA), whose items are shown, not edited (`setup::update`); its mode item lists five
    // names, Quick Play's random pick sets the mode.
    for (k, s) in setup_screens(&rom).into_iter().enumerate().filter(|&(k, _)| k != 1) {
        for it in s.items {
            match it.setting {
                0 => assert!(it.min >= 0 && it.max <= 3, "record {k} mode item {it:?}"),
                4 => assert!(it.min >= 0 && it.max <= 2, "record {k} difficulty item {it:?}"),
                _ => {}
            }
        }
    }
}

/// `race_init::blit_material`: no overlay or decal material of any environment is unpacked format 4.
#[test]
fn no_car_overlay_is_format_4() {
    let Some(rom) = nfsgba_testkit::rom() else { return };
    let overlays = (0..15 * 7).map(|k| u16_at(&rom, 0x7E_F5A0 + 2 * k) as i16);
    let decals = (0..(0x7E_EC7C - 0x7E_EBBC) / 2).map(|k| u16_at(&rom, 0x7E_EBBC + 2 * k) as i16);
    let used: Vec<usize> = overlays.chain(decals).filter(|&m| m >= 0).map(|m| m as usize).collect();
    for env in 0..12 {
        let mats = (u32_at(&rom, 0x7F_2B08 + 0x68 * env + 0x20) - ROM_BASE) as usize;
        for &m in &used {
            let at = mats + 0x24 * m;
            assert!(
                rom[at + 0x22] != 4 || u16_at(&rom, at + 2) & 0x40 != 0,
                "environment {env} material {m}"
            );
        }
    }
}
