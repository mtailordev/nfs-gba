use super::typed::TypedHost;
use crate::menu::{Gba, adapt, flow::Host};
use nfsgba_testkit::{dump, read_to_string, rom};
use serde_json::Value;

fn unhex(v: &Value) -> Vec<u8> {
    let s = v.as_str().unwrap();
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

fn crc32(data: &[u8]) -> u32 {
    !data.iter().fold(!0u32, |mut c, &b| {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { c >> 1 ^ 0xEDB8_8320 } else { c >> 1 };
        }
        c
    })
}

fn poke(g: &mut Gba, writes: &Value) {
    for w in writes.as_array().unwrap() {
        let at = w[0].as_u64().unwrap() as u32;
        for (i, b) in unhex(&w[1]).into_iter().enumerate() {
            g.set_u8(at + i as u32, b);
        }
    }
}

/// `tools/oracle/cases.py turntable`: the game's atlas loader, palette loader and `garage_draw_car` in a row on a
/// random car, record, turntable state and drawing page: the atlas (length and CRC), both pages, the palettes, the
/// base palette buffers and the turntable variables must equal what the game's code left.
#[test]
fn turntable_matches_the_game() {
    let Some(rom) = rom() else { return };
    let Some(text) = read_to_string("garage2/turntable.jsonl") else {
        return;
    };
    let mut snaps = std::collections::HashMap::new();
    let mut cars = std::collections::HashSet::new();
    for (n, line) in text.lines().enumerate() {
        let c: Value = serde_json::from_str(line).unwrap();
        let snap = c["snap"].as_str().unwrap();
        let mut g = snaps
            .entry(snap.to_owned())
            .or_insert_with(|| Gba::from_dump(rom.clone(), &dump(snap).unwrap()).unwrap())
            .clone();
        poke(&mut g, &c["mem"]);
        let mut want = g.clone();
        ["atlas_writes", "pal_writes", "draw_writes"]
            .iter()
            .for_each(|k| poke(&mut want, &c[*k]));
        let mut st = adapt::load_state(&mut g);
        let mut h = TypedHost::new(&rom, &st);
        h.screen = adapt::screen_of(&g);
        h.scene.palettes = [g.u16s(st.g.second_palette, 256), g.u16s(st.g.race_palette, 256)];
        cars.insert(st.g.player_car);
        let a = c["args"].as_array().unwrap();
        let [x, y, z] = std::array::from_fn(|i| a[i].as_u64().unwrap() as u32);
        h.car_load(&mut st);
        let atlas = &h.car.as_ref().unwrap().atlas;
        let expected = c["atlas"].as_array().unwrap();
        assert_eq!(
            atlas.len() as u64,
            expected[0].as_u64().unwrap(),
            "case {n}: atlas size"
        );
        assert_eq!(crc32(atlas) as u64, expected[1].as_u64().unwrap(), "case {n}: atlas");
        Host::car_palette(&mut h, &mut st);
        h.car_draw(&mut st, x, y, z);

        let ws = adapt::load_state(&mut want);
        let shot = adapt::screen_of(&want);
        let wrong = |a: &[u8], b: &[u8]| a.iter().zip(b).filter(|(p, q)| p != q).count();
        assert_eq!(wrong(&h.screen.pages[0], &shot.pages[0]), 0, "case {n}: page 0");
        assert_eq!(wrong(&h.screen.pages[1], &shot.pages[1]), 0, "case {n}: page 1");
        assert_eq!(h.screen.palette[..], shot.palette[..], "case {n}: palette RAM");
        let buffers = [want.u16s(ws.g.second_palette, 256), want.u16s(ws.g.race_palette, 256)];
        assert_eq!(h.scene.palettes, buffers, "case {n}: base palette buffers");
        assert_eq!(
            (st.g.garage_angle, st.g.race_car, st.g.race_car_b, st.profile.u_2f6),
            (ws.g.garage_angle, ws.g.race_car, ws.g.race_car_b, ws.profile.u_2f6),
            "case {n}: turntable variables"
        );
    }
    assert!(cars.len() >= 8, "the cases cover the cars");
}
