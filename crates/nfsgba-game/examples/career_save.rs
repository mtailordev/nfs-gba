//! `career_save NAME`: a save at a chosen career point, written to `career/NAME.sav` (test oracle input for
//! `tools/career_trace.py`; the base is `career/base.sav`, a game-written career save). Points: `boss` (zone 0, seven
//! events won, the first boss next), `boss2` (the first boss won, the second next), `zone` (every event of zone 0 but
//! the last won), `late` (zones 0-3 done, zone 4 in progress), `gauntlet` (zones 0-4 done, the Gauntlet's last race).
use nfsgba_formats::career::{Save, eeprom_to_buffer};

fn main() {
    let name = std::env::args().nth(1).expect("usage: career_save NAME");
    let dir = nfsgba_testkit::fixture("career/base.sav").unwrap();
    let base = std::fs::read(&dir).unwrap();
    let heap = eeprom_to_buffer(&base);
    let mut s = Save::decode(&heap);
    let won = |s: &mut Save, events: &[usize]| {
        for &e in events {
            s.events[e >> 2] = s.events[e >> 2] & !(3 << ((e & 3) * 2)) | 1 << ((e & 3) * 2);
        }
    };
    let (zone, slot, hints, cash) = match name.as_str() {
        "boss" => {
            won(&mut s, &(0..7).collect::<Vec<_>>());
            (0, 7, 5, 3000)
        }
        "boss2" => {
            won(&mut s, &(0..8).collect::<Vec<_>>());
            (0, 8, 5, 3500)
        }
        "zone" => {
            won(&mut s, &(0..11).collect::<Vec<_>>());
            (0, 11, 5, 5000)
        }
        "late" => {
            won(&mut s, &(0..48).collect::<Vec<_>>());
            won(&mut s, &(48..55).collect::<Vec<_>>());
            (4, 7, 0x15, 20000)
        }
        "gauntlet" => {
            won(&mut s, &(0..65).collect::<Vec<_>>());
            (5, 5, 0x17, 40000)
        }
        _ => panic!("unknown point {name}"),
    };
    (s.zone, s.slot, s.field_1f8, s.cash) = (zone, slot, hints, cash);
    // The last zone-0 event is left undone for `zone`; boss2 is not won in `boss`.
    let out = eeprom_to_buffer(&s.encode(&heap));
    let path = dir.with_file_name(format!("{name}.sav"));
    std::fs::write(&path, out).unwrap();
    println!(
        "{}: {} events won",
        path.display(),
        (0..66).filter(|&e| s.event_status(e) == 1).count()
    );
}
