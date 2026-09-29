//! Shared by the replay and layout tests: every typed struct (`nfsgba_sim::state`) at its place in a trace state.
#![allow(dead_code)] // each test binary uses a part of it

use nfsgba_sim::{
    Mem,
    layout::{Field, Layout},
    state::{
        Camera, Car, CarGlobals, CarProfile, Entity, Input, ListEntry, Profile, Query, Race, Screen, SectionRec,
        SectorOffset, ViewPort, WORLD, WaypointRec, WorldHeader,
    },
};

/// A typed struct at `base`: its size (0 for a block of globals), its declared fields (name, offset, size) and a
/// copy that loads it from one image and stores it into another.
pub struct Instance {
    pub base: u32,
    pub size: u32,
    pub fields: &'static [(&'static str, u32, u32)],
    pub copy: fn(&Mem, &mut Mem, u32),
}

fn copy<T: Field>(m: &Mem, into: &mut Mem, at: u32) {
    T::load(m, at).store(into, at);
}

fn at<T: Layout>(base: u32) -> Instance {
    Instance {
        base,
        size: T::SIZE,
        fields: T::FIELDS,
        copy: copy::<T>,
    }
}

/// Every typed struct in `m`, following the world header's pointers.
pub fn instances(m: &Mem) -> Vec<Instance> {
    let (w, race) = (WorldHeader::load(m, WORLD), Race::load(m, 0));
    let mut all = vec![
        at::<WorldHeader>(WORLD),
        at::<Race>(0),
        at::<Input>(0),
        at::<Camera>(0),
        at::<Screen>(0),
        at::<CarGlobals>(0),
        at::<Query>(0),
        at::<CarProfile>(m.u32(0x0300_56EC)),
        at::<ViewPort>(w.view.addr),
        at::<ListEntry>(w.visible.addr),
        at::<Profile>(race.profile.addr),
    ];
    all.extend((0..10).map(|k| at::<SectionRec>(w.sections + 8 * k)));
    all.extend((0..0x100).map(|k| at::<WaypointRec>(w.racing_line + 0x18 * k)));
    all.extend((0..w.piece_count as u32).map(|k| at::<nfsgba_formats::render::Piece>(w.pieces.at(k).addr)));
    all.extend((0..w.sector_offset_count as u32).map(|k| at::<SectorOffset>(w.sector_offsets.at(k).addr)));
    for i in 0..(w.first_entity + w.entity_count) as u32 {
        let e = w.entities.at(i);
        let entity = e.read(m);
        all.push(at::<Entity>(e.addr));
        // Cars, opponents and the wingman have a physics struct (traffic uses the word otherwise).
        if matches!(entity.handler, 0..=3 | 0x29) && !entity.driver.is_null() {
            all.push(at::<Car>(entity.driver.addr));
        }
    }
    all
}
