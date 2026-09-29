//! The game's top level and its menus (`docs/formats/ui.md`, "Menus"): palette fades, the game-state step, the
//! menu dispatcher and the screens. Every function follows the game's code and is checked against it through
//! the function oracle (`tools/ui_menu_oracle.py`).

/// One fade-in step of `n` BGR555 colours from `pal[first..]` towards `target[..]` (read from its start), as
/// `palette_fade_in_step` (`0x0815E530`, a buffer), `FUN_0815E2F0` (BG palette RAM) and `FUN_08161838` (OBJ palette
/// RAM) do it: each channel below its target gains `step`, then is capped at the target. The OBJ variant reads
/// `target` from `first` as well, so its callers pass `&target[first..]`.
pub fn fade_in_step(pal: &mut [u16], target: &[u16], first: usize, n: usize, step: u32) {
    for (c, &t) in pal[first..first + n].iter_mut().zip(target) {
        let ch = |v: u16, s: u32| (v as u32 >> s) & 0x1F;
        let (r, g, b) = [0, 5, 10]
            .map(|s| {
                let (mut c, t) = (ch(*c, s), ch(t, s));
                if c < t {
                    c += step;
                }
                c.min(t)
            })
            .into();
        *c = (b << 10 | g << 5 | r) as u16;
    }
}

/// One fade-out step of `n` colours from `pal[first..]`, as `palette_fade_out_step` (`0x0815E4D4`, a buffer),
/// `FUN_0815E290` (BG palette RAM) and `FUN_081617D4` (OBJ palette RAM): each non-zero channel loses `step`, floored
/// at 0. Bit 15 is dropped, as the game rebuilds the colour from the three channels.
pub fn fade_out_step(pal: &mut [u16], first: usize, n: usize, step: u32) {
    for c in &mut pal[first..first + n] {
        let [r, g, b] = [0, 5, 10].map(|s| ((*c as u32 >> s) & 0x1F).saturating_sub(step));
        *c = (b << 10 | g << 5 | r) as u16;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_dir;

    /// Oracle cases saved by `tools/ui_menu_oracle.py <name>`.
    fn cases(name: &str) -> Option<Vec<serde_json::Value>> {
        let path = data_dir().join(format!("work/e5298b24/menus/{name}.jsonl"));
        let text = std::fs::read_to_string(&path)
            .map_err(|e| eprintln!("skipping: no oracle cases {} ({e})", path.display()))
            .ok()?;
        Some(text.lines().map(|l| serde_json::from_str(l).unwrap()).collect())
    }

    fn words(v: &serde_json::Value) -> Vec<u16> {
        let b: Vec<u8> = (0..v.as_str().unwrap().len() / 2)
            .map(|i| u8::from_str_radix(&v.as_str().unwrap()[2 * i..2 * i + 2], 16).unwrap())
            .collect();
        b.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
    }

    #[test]
    fn fades_match_the_game() {
        std::env::set_current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")).unwrap();
        let Some(cases) = cases("fades") else { return };
        for c in &cases {
            let n = |k: &str| c[k].as_u64().unwrap() as usize;
            let mut pal = words(&c["before"]);
            if c["kind"].as_str().unwrap().starts_with("in") {
                // The OBJ variant reads the target from `first` too; the others from its start.
                let skip = if c["kind"] == "in_obj" { n("first") } else { 0 };
                fade_in_step(
                    &mut pal,
                    &words(&c["target"])[skip..],
                    n("first"),
                    n("count"),
                    n("step") as u32,
                );
            } else {
                fade_out_step(&mut pal, n("first"), n("count"), n("step") as u32);
            }
            let want = words(&c["after"]);
            if let Some(i) = (0..256).find(|&i| pal[i] != want[i]) {
                let before = words(&c["before"])[i];
                panic!(
                    "{} first {} count {} step {}: [{i}] {before:#06x} -> ours {:#06x}, game {:#06x}",
                    c["kind"],
                    n("first"),
                    n("count"),
                    n("step"),
                    pal[i],
                    want[i]
                );
            }
        }
        eprintln!("fades: {} oracle cases match", cases.len());
    }
}
