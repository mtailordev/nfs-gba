use super::*;
use crate::menu::{Gba, adapt::screen_of, draw::Screen};
use nfsgba_testkit::{dump, read_to_string, rom};

const SS: u32 = 0x0300_00C0 + 0xA4;

fn unhex(v: &serde_json::Value) -> Vec<u8> {
    let s = v.as_str().unwrap();
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

/// The first difference between two screens, if any.
fn diff(got: &Screen, want: &Screen) -> Option<String> {
    let first = |a: &[u8], b: &[u8]| a.iter().zip(b).position(|(x, y)| x != y);
    for p in 0..2 {
        if let Some(i) = first(&got.pages[p], &want.pages[p]) {
            return Some(format!(
                "page {p} byte {i}: {} != {}",
                got.pages[p][i], want.pages[p][i]
            ));
        }
    }
    if let Some(i) = first(&got.obj_tiles, &want.obj_tiles) {
        return Some(format!(
            "OBJ tile byte {i:#x}: {} != {} ({:x?} / {:x?})",
            got.obj_tiles[i],
            want.obj_tiles[i],
            &got.obj_tiles[i..i + 8],
            &want.obj_tiles[i..i + 8]
        ));
    }
    if let Some(i) = (0..512).find(|&i| got.palette[i] != want.palette[i]) {
        return Some(format!("palette {i}: {:#x} != {:#x}", got.palette[i], want.palette[i]));
    }
    if let Some(i) = (0..128).find(|&i| got.oam[i] != want.oam[i]) {
        return Some(format!("OAM {i}: {:x?} != {:x?}", got.oam[i], want.oam[i]));
    }
    let (a, b) = (
        [
            got.dispcnt,
            got.tile_base,
            got.bldcnt,
            got.bldalpha,
            got.dispstat,
            got.timer3,
        ],
        [
            want.dispcnt,
            want.tile_base,
            want.bldcnt,
            want.bldalpha,
            want.dispstat,
            want.timer3,
        ],
    );
    (a != b).then(|| format!("registers (dispcnt, tile base, bldcnt, bldalpha, dispstat, timer3) {a:x?} != {b:x?}"))
}

fn replay(kind: &str) {
    let Some(rom) = rom() else { return };
    let Some(text) = read_to_string(&format!("menus3/scene-{kind}.jsonl")) else {
        return;
    };
    let mut snaps = std::collections::HashMap::new();
    let mut n = 0;
    for (i, line) in text.lines().enumerate() {
        let c: serde_json::Value = serde_json::from_str(line).unwrap();
        let snap = c["snap"].as_str().unwrap();
        let base = snaps.entry(snap.to_owned()).or_insert_with(|| {
            Gba::from_dump(rom.clone(), &dump(snap).unwrap_or_else(|| panic!("snapshot {snap}"))).unwrap()
        });
        let mut g = base.clone();
        for m in c["mem"].as_array().unwrap() {
            for (k, b) in unhex(&m[1]).into_iter().enumerate() {
                g.set_u8(m[0].as_u64().unwrap() as u32 + k as u32, b);
            }
        }
        let f = u32::from_str_radix(&c["fn"].as_str().unwrap()[2..], 16).unwrap();
        let regs: Vec<u32> = c["regs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap() as u32)
            .collect();
        // The typed state before: the screen, and the scene the snapshot's blocks hold.
        let mut s = screen_of(&g);
        let mut sc = Scene {
            sprite_screen: g.u16(SS + 0x18) as usize,
            ..Scene::default()
        };
        let heap = g.u32(SS + 0x14);
        if heap != 0 {
            sc.objects = (0..OBJECTS as u32)
                .map(|k| Object::from_bytes(&(0..16).map(|b| g.u8(heap + 16 * k + b)).collect::<Vec<_>>()))
                .collect();
        }
        let dst = c.get("dst").map(|d| (d[0].as_u64().unwrap() as u32, unhex(&d[1])));
        let mut buffer_check = None; // the material whose unpacked pixels the buffer must hold
        match f {
            0x0812_B10C => oam_reset(&mut s),
            0x0813_9C8C => sc.load_menu_descriptor(&rom, &mut s),
            0x0816_1EEC => sc.sprite_screen_select(&rom, &mut s, regs[1] as usize),
            0x0816_1C24 => sc.sprite_screen_update(&rom, &mut s, regs[1] != 0),
            0x0813_70D4 | 0x0813_71A4 => {
                sc.setup(&rom, &mut s, f == 0x0813_70D4, regs[1], regs[2], regs[3]);
                buffer_check = Some(regs[1]);
            }
            0x0816_3D30 => {
                let got = ui::unpack(&rom, regs[0] as usize & 0x1FF_FFFF);
                let want = &dst.as_ref().unwrap().1;
                let n = got.len().min(want.len());
                assert_eq!(got[..n], want[..n], "case {i} {kind}: unpacked bytes");
                continue;
            }
            0x0813_64C4 => {
                sc.buffer = (0..240 * 160).map(|k| g.u8(regs[0] + k)).collect();
                // The drawing page is the one the game's page cell points at.
                let hidden = (g.u32(g.u32(0x0300_0110)) == 0x0600_A000) as u16;
                let dispcnt = s.dispcnt;
                s.dispcnt = dispcnt & !0x10 | ((hidden ^ 1) << 4);
                sc.intro_page_setup(s.page());
                s.dispcnt = dispcnt;
            }
            0x0813_644C => {
                sc.health_screen_image(&rom, regs[1]);
                buffer_check = Some(regs[1]);
            }
            0x0813_9B7C => sc.free(&mut s),
            0x0815_DFD8 => {
                let src = (regs[0] != 0).then(|| g.u16s(regs[0], 256));
                let r = copy_palette_to_ram(&mut s, src.as_deref());
                assert_eq!(r as u32, c["r0"].as_u64().unwrap() as u32, "case {i} {kind}: result");
            }
            0x0816_200C => {
                let e = EffectList::new(regs[1] as u16, regs[2] as u16);
                let d = &dst.as_ref().unwrap().1;
                assert_eq!(
                    (u16::from_le_bytes([d[4], d[5]]), u16::from_le_bytes([d[6], d[7]])),
                    (e.limit, e.capacity),
                    "case {i} {kind}: header"
                );
                continue;
            }
            _ => panic!("scene case {i}: function {f:#x}"),
        }
        // What the game left in the screen's registers, memory and palettes.
        let mut after = g.clone();
        for w in c["writes"].as_array().unwrap() {
            for (k, b) in unhex(&w[1]).into_iter().enumerate() {
                after.set_u8(w[0].as_u64().unwrap() as u32 + k as u32, b);
            }
        }
        if let Some(d) = diff(&s, &screen_of(&after)) {
            panic!("case {i} {kind} {snap} {regs:x?}: {d}");
        }
        if let Some(o) = c.get("objects").filter(|o| !o.as_str().unwrap().is_empty()) {
            let got: Vec<u8> = sc.objects.iter().flat_map(|o| o.to_bytes()).collect();
            let want = unhex(o);
            if let Some(k) = (0..OBJECTS).find(|&k| got[16 * k..16 * k + 16] != want[16 * k..16 * k + 16]) {
                panic!(
                    "case {i} {kind} {snap} {regs:x?}: sprite object {k} {:x?} != {:x?}",
                    &got[16 * k..16 * k + 16],
                    &want[16 * k..16 * k + 16]
                );
            }
        }
        if let Some(p) = c.get("palettes") {
            for (k, want) in p.as_array().unwrap().iter().enumerate() {
                let want = unhex(want);
                let got: Vec<u8> = sc.palettes[k].iter().flat_map(|c| c.to_le_bytes()).collect();
                assert!(
                    want.is_empty() || got == want,
                    "case {i} {kind} {snap} {regs:x?}: palette buffer {k}"
                );
            }
            assert_eq!(
                sc.palette_dirty as u64,
                c["dirty"].as_u64().unwrap(),
                "case {i} {kind}: dirty"
            );
        }
        if let (Some(m), Some((_, want))) = (buffer_check, &dst)
            && ui::materials(&rom, MENU_MATERIALS)[m as usize].kind & 0x40 != 0
        {
            let n = sc.buffer.len().min(want.len());
            assert!(
                n > 0 && sc.buffer[..n] == want[..n],
                "case {i} {kind} {snap} {regs:x?}: unpack buffer"
            );
        }
        n += 1;
    }
    eprintln!("scene-{kind}: {n} oracle cases match");
}

/// The menu scene and sprite functions against the game's own code (`tools/oracle/scene.py`): the pages, OBJ
/// tiles, palettes, shadow OAM and display registers they change, the sprite objects, the base palette buffers
/// and the unpacked background.
#[test]
fn scenes_match_the_game() {
    for kind in [
        "oam",
        "descriptor",
        "select",
        "update",
        "setup",
        "unpack",
        "page",
        "health",
        "free",
        "palette",
        "effects",
    ] {
        replay(kind);
    }
}
