//! `unlock` helpers against the function oracle's cases (`tools/oracle/unlock.py`, `menus/unlock.jsonl`).

use nfsgba_formats::unlock;
use serde_json::Value;

#[test]
fn unlock_helpers_match_the_game() {
    let (Some(rom), Some(text)) = (
        nfsgba_testkit::rom(),
        nfsgba_testkit::read_to_string("menus/unlock.jsonl"),
    ) else {
        return;
    };
    let mut n = 0;
    for line in text.lines() {
        let c: Value = serde_json::from_str(line).unwrap();
        let (id, car) = (c["id"].as_u64().unwrap() as u32, c["car"].as_u64().unwrap() as u32);
        let want = c["ret"].as_u64().unwrap() as u32;
        let got = match c["fn"].as_str().unwrap() {
            "table_index" => unlock::table_index(&rom, id) as u32,
            "id_adjust" => unlock::id_adjust(&rom, id, car),
            "group" => unlock::group(id),
            f => panic!("unknown fn {f}"),
        };
        assert_eq!(got, want, "{line}");
        n += 1;
    }
    assert_eq!(n, 9000, "cases");
}
