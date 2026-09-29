//! The typed state's RAM layouts (`nfsgba_sim::state`) against every replay-trace state: storing what was loaded
//! changes no byte, and the globals and structs a subsystem stores together do not overlap.

mod common;

use nfsgba_game::trace::Trace;
use nfsgba_sim::{
    Mem,
    layout::{Field, Layout},
    state::{Camera, Car, CarGlobals, CarProfile, Input, Query, Race, Screen, WORLD, WorldHeader},
};

const TRACES: [(&str, &str); 5] = [
    ("game-loop", "drive"),
    ("live-race", "live"),
    ("live-race", "trail"),
    ("live-race", "views"),
    ("live-race", "nitro"),
];

/// Every field of `T` at `base`: (name, first address, end).
fn spans<T: Layout>(base: u32) -> Vec<(&'static str, u32, u32)> {
    T::FIELDS.iter().map(|&(n, o, s)| (n, base + o, base + o + s)).collect()
}

/// Stores each typed struct loaded from the state into a copy of it.
fn round_trip(m: &Mem) -> Mem {
    let mut c = m.clone();
    let all = common::instances(m);
    for i in &all {
        (i.copy)(m, &mut c, i.base);
    }
    let cars = all.iter().filter(|i| i.size == Car::SIZE).count();
    assert!(cars >= 2, "the player and an opponent at least");
    c
}

#[test]
fn store_of_load_changes_no_byte() {
    for (session, name) in TRACES {
        let Some(dir) = nfsgba_testkit::fixture(session) else {
            continue;
        };
        nfsgba_testkit::fixture(&format!("{session}/{name}.base.bin")).expect("trace");
        let trace = Trace::load(&dir, name).unwrap_or_else(|e| panic!("{session}/{name}: {e}"));
        let mut checked = nfsgba_testkit::Expect::new(format!("{name} states"), trace.states.len());
        for (k, s) in trace.states.iter().enumerate() {
            checked.tick();
            let m = Mem::new(Vec::new(), s[..0x4_0000].to_vec(), s[0x4_0000..0x4_8000].to_vec());
            let c = round_trip(&m);
            let diff = (0..0x4_0000)
                .find(|&o| c.ewram[o] != m.ewram[o])
                .map(|o| 0x0200_0000 + o as u32);
            let diff = diff.or((0..0x8000)
                .find(|&o| c.iwram[o] != m.iwram[o])
                .map(|o| 0x0300_0000 + o as u32));
            assert_eq!(diff, None, "{name} state {k}: a store changed {diff:#x?}");
        }
    }
}

/// The car step's stores (`nfsgba_sim::ram::store`) write these together: no field of one may overlap another's.
#[test]
fn car_step_globals_are_disjoint() {
    let Some(dir) = nfsgba_testkit::fixture("game-loop") else {
        return;
    };
    let trace = Trace::load(&dir, "drive").unwrap();
    let s = &trace.states[0];
    let m = Mem::new(Vec::new(), s[..0x4_0000].to_vec(), s[0x4_0000..0x4_8000].to_vec());
    let mut all = [
        spans::<CarGlobals>(0),
        spans::<Query>(0),
        spans::<CarProfile>(m.u32(0x0300_56EC)),
    ]
    .concat();
    all.sort_by_key(|&(_, a, _)| a);
    for p in all.windows(2) {
        assert!(p[0].2 <= p[1].1, "{} overlaps {}", p[0].0, p[1].0);
    }
}

/// The camera's state (`World::camera_frame`) is loaded from these together: no field of one may overlap another's.
#[test]
fn camera_globals_are_disjoint() {
    let Some(dir) = nfsgba_testkit::fixture("game-loop") else {
        return;
    };
    let trace = Trace::load(&dir, "drive").unwrap();
    let s = &trace.states[0];
    let m = Mem::new(Vec::new(), s[..0x4_0000].to_vec(), s[0x4_0000..0x4_8000].to_vec());
    let w = WorldHeader::load(&m, WORLD);
    let mut all = [
        spans::<Camera>(0),
        spans::<Screen>(0),
        spans::<Input>(0),
        spans::<Race>(0),
        spans::<WorldHeader>(WORLD),
        spans::<nfsgba_sim::state::ViewPort>(w.view.addr),
    ]
    .concat();
    all.sort_by_key(|&(_, a, _)| a);
    for p in all.windows(2) {
        assert!(p[0].2 <= p[1].1, "{} overlaps {}", p[0].0, p[1].0);
    }
}
