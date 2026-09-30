//! The career in `Session` against the game (FIDELITY G1, D3, U3): headless mGBA runs from a chosen save
//! (`tools/career_trace.py`, scripts in `tools/career/`, saves from the `career_save` example) into a career event,
//! its race (the player poked to a finish, a win when the script says so) and back to the career screens. Each screen
//! visit of the game's run is compared with ours where it has settled (no fade, no exit): the sequence of screens, the
//! career fields of the profile (cash, event status, hints, unlocks, the payout) and the shown page, palettes and
//! sprites. Both runs make the same choices: the same boot presses, then a press when the current screen has been
//! settled for the script's number of frames (`tools/career_trace.py` docs).

use nfsgba_game::session::Session;
use nfsgba_sim::{Mem, layout::Field};
use serde_json::Value;

const KEYS: [&str; 10] = ["A", "B", "SELECT", "START", "RIGHT", "LEFT", "UP", "DOWN", "R", "L"];
/// The profile bytes compared: cash and career car, hints, zone and slot, event status, the map step, the per-zone
/// cursors, the record flags and payout, the unlock bits, the unlock messages.
const PROFILE: [(usize, usize); 8] = [
    (0x0C, 0x11),
    (0x1F8, 0x1FD),
    (0x205, 0x217),
    (0x254, 0x258),
    (0x388, 0x38E),
    (0x3B4, 0x3BC),
    (0x42D, 0x455),
    (0x4AA, 0x4C6),
];

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

/// What is compared at a settled visit.
#[derive(Debug, Clone)]
struct Visit {
    screen: i64,
    profile: Vec<u8>,
    /// Page, palettes (512 entries), OAM (128 entries of 4 halfwords), all little-endian bytes.
    shot: Option<[Vec<u8>; 3]>,
}

fn profile_bytes(all: &[u8]) -> Vec<u8> {
    PROFILE.iter().flat_map(|&(a, b)| all[a..b].to_vec()).collect()
}

fn game_visits(t: &Value) -> Vec<Visit> {
    let blobs: Vec<Vec<u8>> = t["blobs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| unhex(b.as_str().unwrap()))
        .collect();
    let n = |f: &Value, k: &str| f[k].as_i64().unwrap_or(0);
    let mut out = vec![];
    let mut key = None;
    for f in t["frames"].as_array().unwrap() {
        let now = (n(f, "screen"), n(f, "state"));
        // A hint page is a visit of its own (the hint flag counts the pages of one screen).
        let hint = f.get("blob").map_or(0, |b| blobs[b.as_u64().unwrap() as usize][0x1FA])
            * u8::from(matches!(now.0, 0x26..=0x2B));
        if key != Some((now, hint)) {
            key = Some((now, hint));
            out.push(None);
        }
        if let (false, Some(b)) = (f["shot"].is_null(), f.get("blob")) {
            let s = f["shot"].as_array().unwrap();
            *out.last_mut().unwrap() = Some((
                now,
                Visit {
                    screen: now.0,
                    profile: profile_bytes(&blobs[b.as_u64().unwrap() as usize]),
                    shot: Some([0, 1, 2].map(|i| unhex(s[i].as_str().unwrap()))),
                },
            ));
        }
    }
    out.into_iter()
        .flatten()
        .filter(|(k, _)| k.1 == 1 && k.0 < 0x80 && !matches!(k.0, 0x28 | 0x2A))
        .map(|(_, v)| v)
        .collect()
}

fn diffs(a: &[u8], b: &[u8]) -> String {
    let d: Vec<usize> = (0..a.len().min(b.len())).filter(|&i| a[i] != b[i]).collect();
    format!(
        "{} of {} bytes differ, first at {:?}",
        d.len(),
        a.len(),
        &d[..d.len().min(8)]
    )
}

/// Runs `name` (a script in `tools/career/`, its trace in `career/`) and compares every settled menu visit.
fn compare(name: &str) -> Vec<i64> {
    let (Some(rom), Some(text)) = (
        nfsgba_testkit::rom(),
        nfsgba_testkit::read_to_string(&format!("career/{name}.json")),
    ) else {
        return vec![];
    };
    let t: Value = serde_json::from_str(&text).unwrap();
    let cfg = &t["cfg"];
    let save =
        std::fs::read(nfsgba_testkit::fixture(&format!("career/{}", cfg["save"].as_str().unwrap())).unwrap()).unwrap();
    let mut held = vec![0u16; t["frames"].as_array().unwrap().len() + 64];
    let key = |k: &Value| 1u16 << KEYS.iter().position(|x| *x == k.as_str().unwrap()).unwrap();
    for press in cfg["boot"].as_array().unwrap() {
        let first = press[0].as_u64().unwrap() as usize;
        held[first..first + press[1].as_u64().unwrap() as usize]
            .iter_mut()
            .for_each(|h| *h |= key(&press[2]));
    }
    let (boot_frames, tail) = (cfg["boot_frames"].as_u64().unwrap(), cfg["tail"].as_u64().unwrap());
    let (finish_at, win) = (
        cfg["finish_at"].as_u64().unwrap_or(150) as u32,
        cfg["win"].as_bool().unwrap_or(false),
    );
    let rules = cfg["rules"].as_array().unwrap();
    let mut s = Session::new(&rom, save);

    let (mut ours, mut last_key, mut poked) = (vec![], None, false);
    let (mut rule, mut entered, mut fired, mut prev_screen, mut end) = (0, 0u64, -99i64, -1i64, None::<u64>);
    for f in 0..t["frames"].as_array().unwrap().len() as u64 {
        if end.is_some_and(|e| f >= e) {
            break;
        }
        s.frame(held[f as usize]).unwrap_or_else(|e| {
            panic!(
                "frame {f}: {e} ({:x?}), rule {rule}, screen {:#x}",
                s.host.calls, s.st.g.screen
            )
        });
        let racing = s.race.is_some();
        let (screen, state) = if racing {
            (0x81, s.race.as_ref().unwrap().world.lp.game_state as i64)
        } else {
            (s.st.g.screen as i64, s.st.g.game_state as i64)
        };
        if let Some(game) = s.race.as_mut() {
            #[allow(clippy::collapsible_if)]
            if state == 5 && game.world.g.race_frames >= finish_at && !poked {
                let p = game.world.g.player as usize;
                game.world.slots[p].e.race_state = 2;
                if win {
                    let mut r = game.world.slots[p].racer();
                    (r.laps_left, r.distance) = (0, 1_000_000);
                    game.world.slots[p].set_racer(&r);
                }
                poked = true;
            }
        }
        let (fade, exit) = match &s.race {
            Some(g) => (g.world.g.fade as i64, 0),
            None => (s.st.g.fade as i64, s.st.g.menu_exit as i64),
        };
        if !racing {
            poked = false;
        }
        let hint = s.st.profile.hint_flag * u8::from(matches!(screen, 0x26..=0x2B));
        if last_key != Some(((screen, state), hint)) {
            last_key = Some(((screen, state), hint));
            eprintln!("ours: frame {f} screen {screen:#x} state {state}");
            ours.push(None);
        }
        let settled = fade == 0 && exit == 0;
        if settled && state == 1 && !racing {
            let mut m = Mem::new(vec![], vec![0; 0x4_0000], vec![0; 0x8000]);
            s.st.profile.store(&mut m, 0x0200_0000);
            let v = s.view();
            let bytes = |w: &[u16]| w.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
            *ours.last_mut().unwrap() = Some((
                (screen, state),
                Visit {
                    screen,
                    profile: profile_bytes(&m.ewram),
                    shot: Some([
                        v.page.to_vec(),
                        bytes(&v.palette),
                        v.oam.iter().flat_map(|e| bytes(e)).collect(),
                    ]),
                },
            ));
        }
        // The press rules (the same as tools/career_trace.py).
        if screen != prev_screen {
            (prev_screen, entered) = (screen, f);
        }
        if f > boot_frames && rule < rules.len() && !racing && state == 1 && settled {
            let r = &rules[rule];
            if r.as_array().unwrap().len() > 3
                && screen != r[0].as_i64().unwrap()
                && rules[rule + 1..].iter().any(|x| x[0].as_i64().unwrap() == screen)
            {
                rule += 1; // an optional press whose screen was left for a later rule's
                if rule == rules.len() {
                    end = Some(f + 13 + tail);
                }
            } else if screen == r[0].as_i64().unwrap()
                && f as i64 - (entered as i64).max(fired + 12) >= r[1].as_i64().unwrap()
            {
                for g in f + 1..f + 13 {
                    held[g as usize] |= key(&r[2]);
                }
                (rule, fired) = (rule + 1, f as i64);
                if rule == rules.len() {
                    end = Some(f + 13 + tail);
                }
            }
        }
    }
    let ours: Vec<Visit> = ours
        .into_iter()
        .flatten()
        .filter(|(k, _)| k.1 == 1 && k.0 < 0x80 && !matches!(k.0, 0x28 | 0x2A))
        .map(|(_, v)| v)
        .collect();
    let game = game_visits(&t);
    let seq = |v: &[Visit]| v.iter().map(|v| v.screen).collect::<Vec<_>>();
    assert_eq!(seq(&ours), seq(&game), "the screens visited");
    let mut bad = vec![];
    for (i, (o, g)) in ours.iter().zip(&game).enumerate() {
        // The end screen's A press saves (60 VBlank waits in the game, T1) with the screen still shown: the game's last
        // settled frame of it has the hint flag, the zone and the zone step already updated, ours has them one press
        // later. The boot screens come before the save is loaded.
        let (mut op, mut gp) = (o.profile.clone(), g.profile.clone());
        for p in [&mut op, &mut gp] {
            if g.screen == 6 {
                p[7] = 0;
                p[8] = 0;
                p[30] = 0;
            }
            if matches!(g.screen, 0x19 | 0x30 | 0x17) {
                p.fill(0);
            }
            // The last story page: its A press saves (60 VBlank waits) with the page still shown, so the game's hint
            // count is already one up there (ours a press later); the return to screen 3 compares it.
            if g.screen == 0x26 && game.get(i + 1).is_some_and(|n| n.screen == 3) {
                p[5] = 0;
            }
        }
        if op != gp {
            bad.push(format!(
                "visit {i} screen {:#x}: profile {} ours {:x?} game {:x?}",
                g.screen,
                diffs(&o.profile, &g.profile),
                &o.profile[..12],
                &g.profile[..12]
            ));
        }
        // Blinking cursors and PRESS START follow the tick count (T1): the boot screens' phase differs. The standings'
        // times follow the number of race frames the original's video frames ran (T1): their digits differ.
        let phase = matches!(g.screen, 0x19 | 0x30 | 0x17);
        let names = matches!(g.screen, 0xA | 0xC);
        if let (Some(a), Some(b), false) = (&o.shot, &g.shot, phase) {
            for (what, x, y) in [("page", &a[0], &b[0]), ("palette", &a[1], &b[1]), ("oam", &a[2], &b[2])] {
                if x != y && !(what == "page" && names) {
                    bad.push(format!("visit {i} screen {:#x}: {what} {}", g.screen, diffs(x, y)));
                }
            }
        }
    }
    assert!(bad.is_empty(), "{name}:\n{}", bad.join("\n"));
    seq(&ours)
}

#[test]
fn career_ordinary_event() {
    compare("ordinary");
}

/// Boss 1 (a sprint): its first-race hint page (0x2B), the win, the record and unlock messages.
#[test]
fn career_boss_event() {
    compare("boss");
}

/// Boss 2 (a circuit): the zone is done, the district unlocks and their messages, the zone step on the end screen.
#[test]
fn career_zone_completion() {
    compare("boss2");
}

/// The last event of zone 1 (a sprint), everything before it won.
#[test]
fn career_last_event_of_a_zone() {
    compare("zone");
}

/// A boss in zone 5 with the mode hint pages and the unlock messages of a late zone.
#[test]
fn career_late_boss() {
    compare("late");
}

/// The Gauntlet: the last event, then the ending hint pages (0x28, 0x29, 0x2A, ten pages of 0x26).
#[test]
fn career_gauntlet_ending() {
    compare("gauntlet");
}

/// The whole ending: after the Gauntlet the 20 story pages (0x26) and the return to screen 3.
#[test]
fn career_ending_twenty_pages() {
    let seq = compare("ending20");
    if seq.is_empty() {
        return; // no data (skipped)
    }
    assert_eq!(
        seq.iter().filter(|&&s| s == 0x26).count(),
        20,
        "the 20 story pages: {seq:x?}"
    );
    assert_eq!(seq.last(), Some(&3), "back to the Crew House (screen 3)");
}
