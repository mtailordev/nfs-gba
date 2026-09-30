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

/// `Game::bldalpha`: the race's BLDALPHA is written once, by race_init (0x0D0F, through `0x04000050 + 2`); the menu
/// scene (`load_menu_descriptor`) and `0x08139F88` write the blend registers too, and nothing calls the latter.
#[test]
fn race_blend_is_set_once() {
    let Some(rom) = nfsgba_testkit::rom() else { return };
    assert_eq!(words(&rom, 0x0400_0050), [0x139928, 0x139DDC, 0x139FD4]);
    assert_eq!(words(&rom, 0x0400_0052), [0x139FF0]);
    assert!(uncalled(&rom, 0x0813_9F88));
}

/// `slots::rim_redraw`'s stop (the rotated read around the rim buffer reaching the atlas box it writes) is
/// unreachable. The atlas is material `car table +0x0C + record[3]`; record[3] (the body kit) is written only by
/// `buy` for ids 0xA0 + k: from the kit page (title 0x95), whose cursor `kind18_update` bounds by the car's kit count
/// (`0x7F0626`), and from `upgrades_changed`, whose `new_part` offers the next kit only below that count. The kit page
/// is page 0 of 0x14, page 10 of 0x13 (its selection is at most 9: the list's items 1..=10, action 0x1F, minus 1) and
/// page 11 of 0x12 (no purchase). With record[3] below the count, the box is at least 0x40 bytes (the read's reach)
/// inside both ends of the atlas buffer, wherever the rim buffer lies.
#[test]
fn rim_redraw_never_reads_its_atlas() {
    let Some(rom) = nfsgba_testkit::rom() else { return };
    let s16 = |o: usize| u16_at(&rom, o) as i16 as i32;
    let page =
        |screen: usize, sel: usize| (u32_at(&rom, 0x7E_6EA4 + 0x10 * screen + 0xC) - ROM_BASE) as usize + 0x14 * sel;
    let kit_pages: Vec<(usize, usize)> = (0..3)
        .flat_map(|i| (0..12).map(move |k| (i, k)))
        .filter(|&(i, k)| s16(page(i, k)) == 0x95)
        .collect();
    assert_eq!(kit_pages, [(0, 10), (1, 0), (2, 11)]);
    let kits = (u32_at(&rom, page(1, 0) + 8) - ROM_BASE) as usize;
    assert!((0..4).all(|k| s16(kits + 10 * k + 6) == 0xA0 + k as i32));
    assert_eq!(u16_at(&rom, 0x7E_544C + 0x14 * 8 + 10), 11); // list slot 8: item 0 (0x8B) and ten 0x1F items
    let slot8 = (u32_at(&rom, 0x7E_544C + 0x14 * 8 + 0x10) - ROM_BASE) as usize;
    assert!((1..11).all(|k| u16_at(&rom, slot8 + 8 * k + 6) == 0x1F));
    for env in 0..12 {
        let mats = (u32_at(&rom, 0x7F_2B08 + 0x68 * env + 0x20) - ROM_BASE) as usize;
        let size = |m: usize| {
            (
                u16_at(&rom, mats + 0x24 * m + 0xC) as i32,
                u16_at(&rom, mats + 0x24 * m + 0xE) as i32,
            )
        };
        for car in 0..15 {
            for kit in 0..rom[0x7F_0626 + car] as usize {
                let (aw, ah) = size(u16_at(&rom, 0x7F_0BD8 + 0x58 * car + 0xC) as usize + kit);
                for r in 0..15 {
                    let at = 0x7E_F816 + 0x10 * (15 * car + r);
                    if s16(at) == -1 {
                        continue;
                    }
                    let (w, h) = size(s16(at + 4) as usize);
                    let (first, second) = ((s16(at), s16(at + 2)), (s16(at + 8), s16(at + 10)));
                    let (x0, y0) = (first.0 + (w >> 3), first.1 + (h >> 3));
                    let (x1, y1) = (first.0 + w - (w >> 3), first.1 + h - (h >> 3));
                    let shift = (second.1 - first.1) * 0x100 + second.0 - first.0;
                    let lo = y0 * 0x100 + x0 + shift.min(0);
                    let hi = (y1 - 1) * 0x100 + x1 + shift.max(0);
                    assert!(
                        aw * ah - hi >= 0x40 && lo >= w * h + 0x40,
                        "environment {env} car {car} kit {kit} rim {r}"
                    );
                }
            }
        }
    }
}

/// Follows the register a Thumb `LDR rd, [rn, #imm]` at `site` loads through the straight-line code after it
/// (conditional branches fall through; `B`, `BX` and `POP {pc}` end it), with the registers `ADD`/`SUB`/`MOV`
/// derive from it. Returns the offsets where it is used as a store's base or index, stored as a value (a copy
/// escapes) or passed in r0..r3 to a `BL`.
fn thumb_uses(rom: &[u8], site: usize) -> Vec<usize> {
    let mut t: u16 = 1 << (u16_at(rom, site) & 7);
    let (mut p, mut out) = (site + 2, Vec::new());
    let has = |t: u16, r: u16| t >> r & 1 != 0;
    let with = |t: u16, r: u16, on: bool| if on { t | 1 << r } else { t & !(1 << r) };
    for _ in 0..40 {
        let h = u16_at(rom, p);
        let (lo3, mid3, hi3) = (h & 7, h >> 3 & 7, h >> 6 & 7);
        match h >> 11 {
            0x1E if u16_at(rom, p + 2) >> 11 == 0x1F => {
                if t & 0xF != 0 {
                    out.push(p);
                }
                t &= !0x100F;
                p += 2;
            }
            0x1C => break,                                                                         // B
            _ if h & 0xFF00 == 0xBD00 || h & 0xFF80 == 0x4700 => break,                            // POP {.., pc}, BX
            0x0C | 0x0E | 0x10 if has(t, mid3) || has(t, lo3) => out.push(p),                      // STR/STRB/STRH imm
            0x0A if h >> 9 & 7 < 3 && (has(t, mid3) || has(t, hi3) || has(t, lo3)) => out.push(p), // STR/H/B reg
            0x12 if has(t, h >> 8 & 7) => out.push(p),                                             // STR sp
            0x16 if h & 0x0600 == 0x0400 && t & h & 0xFF != 0 => out.push(p),                      // PUSH
            0x18 if has(t, h >> 8 & 7) || t & h & 0xFF != 0 => out.push(p),                        // STMIA
            0x03 => t = with(t, lo3, has(t, mid3) || (h & 0x400 == 0 && has(t, hi3))),             // ADD/SUB
            0x00..=0x02 | 0x0D | 0x0F | 0x11 => t = with(t, lo3, false), // shifts, LDR/LDRB/LDRH imm
            0x0A => t = with(t, lo3, false),                             // loads, reg offset
            0x04 | 0x09 | 0x13 | 0x14 | 0x15 => t = with(t, h >> 8 & 7, false), // MOV imm, LDR pc/sp, ADR
            0x19 | 0x17 if h & 0xFE00 == 0xBC00 || h >> 11 == 0x19 => t &= !(h & 0xFF), // POP, LDMIA
            0x08 if h & 0xFC00 == 0x4000 && !matches!(h >> 6 & 0xF, 8 | 0xA | 0xB) => t = with(t, lo3, false),
            0x08 if h & 0xFF00 == 0x4400 || h & 0xFF00 == 0x4600 => {
                let (d, m) = (lo3 | (h >> 4 & 8), h >> 3 & 0xF);
                let keep = h & 0xFF00 == 0x4400 && has(t, d);
                t = with(t, d, has(t, m) || keep);
            }
            _ => {}
        }
        if t == 0 {
            break;
        }
        p += 2;
    }
    out
}

/// R21 (`render::Runtime`): the renderer's sector offsets (world `+0x1C`) and material animation and scroll table
/// (world `+0x48`) never change in Carbon, so `draw_world` reading them as all zero is exact.
///
/// Sector offsets: `FUN_08138b20` counts the sectors whose `+0x0A` names a record (world `+0xDE`), `race_load_level`
/// allocates that many, `FUN_08138b80` fills them from the sectors; the renderer reaches one only through a
/// sector's `+0x0A`. No sector of any route record names one, so the table is empty.
///
/// Materials: `race_load_level` allocates the table zeroed (`+0xD8` × 8 bytes); `race_init` and
/// `load_menu_descriptor` clear it again with `fill_units`, as does the uncalled `FUN_0813A020`. The engine's
/// level-animation step, `race_frame_nop_a(world, 1)` from `FUN_08138b80` and `(world, 0)` from
/// `race_frame_update`, is an empty `BX LR`. Every other Thumb load of a `+0x48` field whose register a store, a
/// copy or a call follows (the ROM's whole code, decompiled or not) is listed below by its function: none holds the
/// world. The IWRAM renderer (ARM) loads the table at the sites below and only reads it.
#[test]
fn renderer_runtime_tables_never_change() {
    let Some(rom) = nfsgba_testkit::rom() else { return };
    let sectors = (u32_at(&rom, 0x7F_2B08 + 0x18) - ROM_BASE) as usize;
    for index in 0..ROUTE_COUNT {
        let counts = (u32_at(&rom, 0x7F_2798 + 0x14 * index + 0xC) - ROM_BASE) as usize;
        for s in 0..u16_at(&rom, counts) as usize {
            assert_eq!(
                u16_at(&rom, sectors + 0x30 * s + 0xA),
                0xFFFF,
                "route record {index} sector {s}"
            );
        }
    }
    assert_eq!(u16_at(&rom, 0x13_B62C), 0x4770); // race_frame_nop_a: BX LR
    assert_eq!(bl_calls(&rom, 0x0813_B62C), [0x138C14, 0x13A992]);
    assert!(words(&rom, 0x0813_B62D).is_empty());
    assert!(uncalled(&rom, 0x0813_A020));
    let flagged: Vec<usize> = (0x12_A000..0x16_C400)
        .step_by(2)
        .filter(|&o| u16_at(&rom, o) & 0xFFC0 == 0x6C80 && !thumb_uses(&rom, o).is_empty()) // LDR rd, [rn, #0x48]
        .collect();
    assert_eq!(
        flagged,
        [
            0x13964A, // race_load_level: the level descriptor's +0x48 (a model bank array) into world +0x90
            0x139704, // race_cleanup: heap_free(table)
            0x1398B4, // race_init: fill_units(table, 0)
            0x139BBC, // menu_scene_free: heap_free(table)
            0x139D4C, 0x13A022, // load_menu_descriptor, FUN_0813A020: fill_units(table, 0)
            0x148400, 0x1488FC, 0x148E7E, // car_wheel_contact, car_tipped_dynamics, ai_wheel_contact: car state
            0x1514EC, 0x151522, 0x152208, 0x15260C, 0x152DE6, 0x152EFE, 0x152F6E, 0x153018, 0x153454, 0x153598,
            0x1537B8, 0x153B26, // the sound engine's own structs (snd_*)
        ]
    );
    for (ldr, bl) in [(0x1398B4, 0x1398C2), (0x139D4C, 0x139D5A), (0x13A022, 0x13A030)] {
        assert_eq!(thumb_uses(&rom, ldr), [bl]);
        assert!(bl_calls(&rom, 0x0816_0CD0).contains(&bl));
    }
    // The ARM overlay (ROM 0x08165134, 0x5164 bytes, run in IWRAM): every LDR rd, [rn, #0x48].
    let arm: Vec<usize> = (0x16_5134..0x16_A298)
        .step_by(4)
        .filter(|&o| u32_at(&rom, o) & 0x0FF0_0FFF == 0x0590_0048)
        .collect();
    // raster_wall_columns, clip_flat_outline and draw_flat_textured load a stack slot (sp + 0x48); setup_wall_spans
    // (three), draw_sector_walls and draw_sector (two) load the table and read an entry's frame or scroll.
    assert_eq!(
        arm,
        [
            0x16542C, 0x165E7C, 0x166450, 0x16673C, 0x166744, 0x1670F4, 0x167340, 0x1673FC, 0x167DE4
        ]
    );
}

/// FIDELITY N1 (was R26), the ROM side of `tests/coverage.rs` `entity_draw_path`: the entity records a race starts
/// from (at most 4 route templates per record, which `race_spawn_template_entities` copies and `setup_race_cars` rewrites)
/// have state 0x100F (bits 0 and 1), no flag bit 4, zero material steps and no negative second model. The second
/// model's writers in a race, `setup_race_cars` and `setup_player_car`, read the spoiler table and store 0 for a
/// negative entry (the third reference is the garage's). From the decompile: the spawners store state 7 and zero
/// steps (`spawn_wingman_marker` after a transient 1, `spawn_spark` with material 0, `traffic_spawn`); later state
/// stores only clear bit 0 or 2 (`traffic_handler`, `FUN_0814a6c0`, `FUN_0814a818`, `lap_crossing`), toggle bit 2
/// (`car_racing_step`) or store 0; `effect_handler` steps `+0x44` only on sparks, which have no material.
#[test]
fn entity_records() {
    let Some(rom) = nfsgba_testkit::rom() else { return };
    for index in 0..ROUTE_COUNT {
        let (templates, n) = route_templates(&rom, index);
        assert!(n <= 4, "route record {index}"); // record 0, the menu scene, has one
        for k in 0..n {
            let e = templates + 0xA4 * k;
            let (state, flags) = (u16_at(&rom, e + 8), u16_at(&rom, e + 0xA));
            let (steps, second) = (
                (u16_at(&rom, e + 0x44), u16_at(&rom, e + 0x46)),
                u16_at(&rom, e + 0x64) as i16,
            );
            assert!(
                state == 0x100F && flags & 0x10 == 0 && steps == (0, 0) && second >= 0,
                "route record {index} template {k}"
            );
        }
    }
    assert_eq!(words(&rom, 0x087F_0636), [0x12BF44, 0x13BB18, 0x13BD4C]);
}

/// G1, race-end phases 6 to 8 (`Game::race_end_phases`). Every store to the race phase (`0x03000048`) is a constant
/// (decompile): `list_update` 5, `camera_init` 1, `race_init` 0 and 9, `ai_speed_curve` 1 and 9, `race_start_from_table_b`
/// 2 (through a pointer), `car_handler` 3, and the three that end a race: `hud_timer` 8 (race time past 59:59.98:
/// reachable, recorded in `traces3/overtime`), `hud_countdown_timer` 7 (a countdown nothing starts: no caller) and
/// `traffic_contact` 7 when the traffic type the player hit has `ends_race` set. Nothing stores 6: phase 6 never
/// happens. Traffic types come from `race_frames % 7` (`FUN_08144d94` stores 7 in `0x0300625C`, `traffic_spawn` reads
/// it), and `ends_race` (`0x7F5924`) is non-zero for types 18, 20 and 21 only: so the phase-7 hit never happens either.
#[test]
fn race_end_phases_6_and_7_are_unreachable() {
    let Some(rom) = nfsgba_testkit::rom() else { return };
    let sites = [
        0x12ACB8, 0x12AE38, 0x12EB58, 0x12F1FC, 0x12F328, 0x130C64, 0x137374, 0x137934, 0x137D2C, 0x1382A4, 0x139900,
        0x139ABC, 0x13AAEC, 0x13ABB4, 0x13ABF4, 0x13B008, 0x13C0C8, 0x13C230, 0x13C4D8, 0x13D510, 0x13D614, 0x13E2AC,
        0x13E320, 0x13EA5C, 0x14058C, 0x1427F0, 0x142988, 0x146588, 0x14A2F0, 0x14B20C, 0x14BDE4, 0x14C478, 0x14C504,
        0x14C594, 0x14EA38, 0x14EB98, 0x14EF28, 0x14FBC8,
    ];
    assert_eq!(words(&rom, 0x0300_0048), sites);
    // The functions holding the stores (their words above): list_update, camera_init, race_init, race_start_from_table_b,
    // ai_speed_curve, hud_countdown_timer, hud_timer, traffic_contact, car_handler. No function sets a variable phase.
    // hud_countdown_timer: no BL and no pointer to it.
    assert!(uncalled(&rom, 0x0814_279C));
    // hud_timer is called by the HUD's modes.
    assert_eq!(bl_calls(&rom, 0x0814_28C0).len(), 4);
    // The traffic models: stored once (7) and read once.
    assert_eq!(words(&rom, 0x0300_625C), [0x1443B0, 0x144DE0]);
    let ends_race: Vec<i16> = (0..32).map(|k| u16_at(&rom, 0x7F_5924 + 2 * k) as i16).collect();
    assert!(ends_race[..7].iter().all(|&v| v == 0), "{ends_race:?}");
    assert_eq!(
        ends_race
            .iter()
            .enumerate()
            .filter(|(_, v)| **v != 0)
            .map(|(k, _)| k)
            .collect::<Vec<_>>(),
        [18, 20, 21]
    );
}

/// D4, cops: Carbon's GBA game has no police or pursuit entity type. The entity handlers any route's templates carry
/// are the car (0..3), the wingman marker (0xF), the opponent (0x29), sparks (0x34) and traffic (0x36), by the handler
/// table's pointers; nothing else is placed by the routes or spawned (`entity_handlers_are_ported`: the other
/// handlers are reached only down dead chains) and traffic has 7 civilian models with no race-ending or pursuing
/// type (`race_end_phases_6_and_7_are_unreachable`). "Cops" occur only in the dialogue texts (the story's lines about
/// patrols: ASCII and translated strings inside the text tables, `0x79xxxx..0x7Bxxxx`), never in code or data tables.
#[test]
fn no_police_entity_type_exists() {
    let Some(rom) = nfsgba_testkit::rom() else { return };
    let table = |id: usize| u32_at(&rom, 0x7F_38B8 + 4 * id);
    let live = [
        (0, 0x0814_BD4D),
        (1, 0x0814_BD4D),
        (2, 0x0814_BD4D),
        (3, 0x0814_BD4D),
        (0xF, 0x0814_BF99),
        (0x29, 0x0814_A2A1),
        (0x34, 0x0814_C49D),
        (0x36, 0x0814_43FD),
    ];
    for (id, f) in live {
        assert_eq!(table(id), f, "handler {id:#x}");
    }
    for index in 0..ROUTE_COUNT {
        let (templates, n) = route_templates(&rom, index);
        for k in 0..n {
            let e = templates + 0xA4 * k;
            let handler = u16_at(&rom, e + 0x4E) as usize;
            assert!(k < 4 || u16_at(&rom, e + 8) & 3 != 3 || live.iter().any(|l| l.0 == handler));
        }
    }
    let lower: Vec<u8> = rom.iter().map(u8::to_ascii_lowercase).collect();
    for word in [&b"polic"[..], b"cops", b"pursu", b"busted", b"arrest"] {
        let mut at = 0;
        while let Some(p) = lower[at..].windows(word.len()).position(|w| w == word) {
            let o = at + p;
            assert!(
                (0x79_0000..0x7C_0000).contains(&o),
                "{:?} at {o:#x} outside the text tables",
                std::str::from_utf8(word)
            );
            at = o + 1;
        }
    }
}
