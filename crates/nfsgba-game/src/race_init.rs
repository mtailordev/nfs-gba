//! The race start: `race_start_from_table_a` (`0x08139e34`), which `game_state_step` runs in state 4, and
//! everything below it (`race_init`, `race_load_level`, the racing-line rebuild, the palettes, the sprite screen
//! and HUD reset, the racers, the camera, the music). It builds the race's first typed [`World`] from a [`Setup`]
//! (`docs/engine/race-init.md`): the race the menus chose, the profile and car records, the RNG index and tick
//! counter, and what the race keeps as the menus left it. It runs no game code.
//!
//! The IRQs that fire while it runs (about 20 VBlanks) are not part of this: like the game loop, a caller that
//! wants the emulator's exact state runs them. The one they feed back is the tick counter, which `setup_race_cars`
//! uses as the rand seed (`seed_vblanks`).
//!
//! NOT 1:1 (R24): one byte image remains, the heap arena (`World::heap`): the start allocates in it exactly as the
//! game's heap does (the node table the menus left decides where the atlases land) and writes what lies around the
//! atlases and rim buffers (the freed decoder rings and material buffers, the level's 4 KiB table, and the typed
//! blocks' declared fields); a block's undeclared bytes are zero there (18 to 128 bytes in the entity, waypoint and
//! older blocks, none next to an atlas).

use nfsgba_formats::{
    atlas,
    career::{LinePoint, RacingLine, Section},
    hud, paint,
    render::Piece,
    ui::{self, Object},
};
use nfsgba_sim::{
    Mem, Result, Unported,
    carworld::{PointExtra, Route, Slot},
    data::GameData,
    heap, math,
    state::{CarRecord, Entity, ListEntry, MaterialInfo, SectionRec, SectorOffset, Sprite, ViewPort, WaypointRec},
    world::Geometry,
};

use crate::{
    race_setup::{Display, Setup},
    slots::M,
    view,
    world::{World, heap_offset},
};
use nfsgba_sim::layout::{Field, Ptr};

const CAR_TABLE: u32 = 0x087F_0BD8;
const LOOKS: u32 = 0x087E_EA44;
const ROUTES: u32 = 0x087F_2798;
const LEVELS: u32 = 0x087F_2B08;
const EWRAM: u32 = 0x0200_0000;
/// The heap's node table and data area pointers (IWRAM).
const NODES: u32 = 0x0300_64CC;
const DATA: u32 = 0x0300_64D0;

/// `race_start_from_table_a(world)`: the wingman and the racer count, the opponents' cars and paints, the racer
/// slots, `race_init` with the environment's level descriptor. `rom` is the game ROM, `data` its parsed tables.
///
/// `seed_vblanks`: the VBlank IRQs that run before `setup_race_cars` reads the tick counter as the rand seed (the
/// only timing the result depends on; 6 or 7 in the recorded race starts, 0 for the IRQ-free oracle).
///
/// `d` is the display memory the start writes (page clears, HUD tiles, OAM, the I/O registers).
pub fn race_start(rom: &[u8], data: &GameData, s: &Setup, seed_vblanks: u32, d: &mut Display) -> Result<World> {
    // NOT 1:1 (R24): the heap arena, with the ROM alongside for reading its tables.
    let mut m = Mem::new(rom.to_vec(), s.heap.ewram.clone(), vec![0; 0x8000]);
    m.set_u32(NODES, s.heap.nodes);
    m.set_u32(DATA, s.heap.data);
    let mut g = s.g.clone();
    let (mut cars, mut paints) = (s.cars, s.paints);
    // race_start_from_table_a: the wingman, the racer count, the opponents' cars (FUN_0813b634), the racer slots.
    g.wingman = s.wingman as i32;
    g.racers = g.opponents + u32::from(s.wingman.wrapping_sub(1) < 12);
    if g.racers > 3 {
        g.opponents = 2;
        g.racers = 3;
    }
    atlas::pick_opponent_cars(rom, &mut g.rand, g.opponents, s.wingman, &mut cars, &mut paints);
    for i in 0..4u32 {
        g.results[0xC + i as usize] = if i < g.racers + 1 { i as u8 } else { 0xFF };
    }
    let level = LEVELS + 0x68 * g.level as u32;
    let mode = g.mode;

    // race_init: the globals.
    g.phase = 0;
    g.settled = 1;
    g.focus = g.player;
    let (tex, mats) = (m.u32(level + 0xC), m.u32(level + 0x20));
    let mut profile = s.profile.clone();
    profile.accelerating = 0;
    profile.scrape = 0;
    let mut camera = s.camera.clone();
    camera.horizon = 0;
    let mut screen = s.screen.clone();
    screen.centre_y = 0;
    screen.shake = [0, 0];

    // Both mode-4 pages cleared; the OAM hidden, then every entry a 64x64 sprite at (0x3C, 0xA0); OBJ on, 1-D.
    let size = (screen.size[0] as i32 * screen.size[1] as i32) as usize;
    for page in screen.pages {
        let o = (page - 0x0600_0000) as usize;
        d.vram[o..o + (size & !3)].fill(0);
    }
    let mut oam: ui::Oam = [[0; 4]; 128];
    d.io[1] |= 0x10;
    oam_hide(&mut oam, 0, 0x80);
    d.oam = oam_bytes(&oam);
    d.io[0] = d.io[0] & 0xBF | 0x40;
    for e in oam.iter_mut() {
        (e[0], e[1], e[2]) = (0x00A0, 0xC03C, e[2] & 0xFC00 | 0x200);
    }
    d.io[0x50..0x52].copy_from_slice(&0x3F3Fu16.to_le_bytes());
    d.io[0x52..0x54].copy_from_slice(&0x0D0Fu16.to_le_bytes());

    // race_load_level: the counts, the runtime buffers on the heap in the game's order.
    let route_at = ROUTES + g.route_index * 0x14;
    let counts = m.u32(route_at + 0xC);
    let (n_sectors, n_walls, n_entities, n_materials) = (
        m.u16(counts) as u32,
        m.u16(counts + 2) as u32,
        m.u16(counts + 4) as u32,
        m.u16(counts + 6) as u32,
    );
    let (walls_base, sectors_at) = (m.u32(level + 0x14), m.u32(level + 0x18));
    let wall = |k: u32| walls_base + 0x44 * k;
    let pieces_at: Vec<u32> = (0..n_walls).filter(|&k| m.u16(wall(k) + 0x2A) != 0xFFFF).collect();
    let sector = |k: u32| sectors_at + 0x30 * k;
    let with_offsets: Vec<u32> = (0..n_sectors).filter(|&k| m.u16(sector(k) + 0xA) != 0xFFFF).collect();
    let alloc = |m: &mut Mem, size: u32| heap::alloc_zeroed(m, size);
    let at_offsets = alloc(&mut m, with_offsets.len() as u32 * 0x14);
    let at_pieces = alloc(&mut m, (pieces_at.len() as u32) << 5);
    // FUN_08138b80: the moving pieces get the wall's flags; the sector offsets the sector's flags and plane.
    let pieces: Vec<Piece> = pieces_at
        .iter()
        .map(|&k| Piece {
            flags: m.u16(wall(k) + 0x2E),
            ..Piece::default()
        })
        .collect();
    let offsets: Vec<SectorOffset> = with_offsets
        .iter()
        .map(|&k| SectorOffset {
            flags: m.u8(sector(k) + 0x12) as u16,
            plane: [
                m.i16(sector(k) + 0x14),
                m.i16(sector(k) + 0x16),
                m.i16(sector(k) + 0x18),
            ],
            ..SectorOffset::default()
        })
        .collect();
    alloc(&mut m, n_materials << 3);
    let at_heads = alloc(&mut m, n_sectors << 1);
    let at_entities = alloc(&mut m, (n_entities + 0x20) * 0xA4);
    let at_sections = alloc(&mut m, 0x50);
    let at_line = alloc(&mut m, 0x1800);
    let sprint = mode == 3;
    let route = build_route(&m, rom, route_at, &mut g, sprint)?;
    alloc(&mut m, 0x1A00);
    alloc(&mut m, 0x400);
    heap::alloc(&mut m, 0x2000);
    alloc(&mut m, 0x780);
    let at_planes = alloc(&mut m, 0x2000);
    let at_back = alloc(&mut m, 0x400);

    // race_load_palettes: the city palette, the view, the level's 4 KiB table (copied), the matrix slots.
    let mut palette: Vec<u16> = {
        let at = m.u32(level) + m.u16(level + 0x5A) as u32 * 2;
        (0..256).map(|i| m.u16(at + 2 * i)).collect()
    };
    let view = ViewPort {
        page: 0x0600_0000,
        cx: screen.size[0] >> 1,
        cy: screen.size[1] >> 1,
        near: 0x40,
        focal: 0x96,
    };
    let table = heap::alloc(&mut m, 0x1000);
    let bytes = m.bytes(m.u32(level + 0x28), 0x1000).to_vec();
    m.set_bytes(table, &bytes);
    let matrices_at = heap::alloc(&mut m, 0xC00);
    let matrices = Ptr::<M>::new(matrices_at).read_n(&m, 64);

    // The entities: the route's templates, the extra slots, every template pushed onto its sector's list.
    let templates = m.u32(route_at);
    let mut slots: Vec<Slot> = (0..n_entities)
        .map(|k| {
            let mut e = Entity::load(&m, templates + 0xA4 * k);
            e.race_state = 0;
            Slot { e, ..Slot::default() }
        })
        .collect();
    slots.extend((0..0x20).map(|k| Slot {
        e: Entity {
            index: (n_entities + k) as u16,
            ..Entity::default()
        },
        ..Slot::default()
    }));
    let mut heads = vec![0xFFFFu16; n_sectors as usize];
    for (i, sl) in slots.iter_mut().enumerate().take(n_entities as usize) {
        if sl.e.sector != 0xFFFF {
            sl.e.next = heads[sl.e.sector as usize];
            heads[sl.e.sector as usize] = i as u16;
        }
    }
    let grid = (0..(g.opponents + 2).min(slots.len() as u32))
        .map(|k| {
            let at = templates + 0xA4 * k;
            [m.i32(at + 0xC), m.i32(at + 0x14)]
        })
        .collect();
    let total = slots.len();
    let mut w = World {
        slots,
        heads,
        spare: (n_entities as usize, 0x20),
        g,
        profile,
        contact: s.contact,
        needle_scale: s.needle_scale,
        query: s.query.clone(),
        pieces,
        piece_kind: vec![0; pieces_at.len()],
        offsets,
        route,
        grid,
        walls_base,
        camera_player: 0,
        camera,
        screen,
        view,
        input: s.input.clone(),
        root: ListEntry::default(),
        rect: s.rect,
        ceiling: [0, 0],
        sky: s.sky,
        matrices,
        pool: Vec::new(),
        pool_owner: Default::default(),
        pool_first: 0,
        slot_counter: s.slot_counter,
        lights: 1,
        rim_pixels: s.rim_pixels,
        atlases: s.atlases,
        records: s.records.clone(),
        hud: s.hud.clone(),
        lp: s.lp.clone(),
        cars,
        paints,
        palette_base: Vec::new(),
        palette_fade: Vec::new(),
        fade_gradient: {
            let mat = m.u16(level + 0x5E) as u32;
            let src = m.u32(level + 8).wrapping_add(m.u32(m.u32(level + 0x1C) + 36 * mat + 8));
            (0..120).map(|i| m.u16(src + 2 * i)).collect()
        },
        fade_obj: {
            let at = m.u32(level + 4) + m.u16(level + 0x58) as u32 * 2;
            (0..256).map(|i| m.u16(at + 2 * i)).collect()
        },
        gradient: s.gradient.clone(),
        gradient_start: 0,
        lap_lag: 0,
        materials: vec![(0, 0, 0); n_materials as usize],
        material_info: (0..n_materials)
            .map(|k| MaterialInfo::load(&m, mats + 0x24 * k))
            .collect(),
        wall_count: n_walls as u16,
        audio: s.audio.clone(),
        heap: Vec::new(),
        car_at: vec![0; total],
        arena: [s.heap.nodes, s.heap.data, s.heap.records, mats, tex],
    };
    w.lp.descriptor = level;
    w.hud.tile_base = 0x200;
    w.camera_player = w.g.player as usize;

    // The HUD sprite screen (modes 0..=3): the objects, their OBJ tiles, the reset.
    if mode > 3 {
        return Err(Unported("a race mode other than 0..=3 (the HUD sprite screen)"));
    }
    w.hud.screen = [0, 0, 2, 1][mode as usize];
    let bank = ui::sprite_bank(rom, (level - 0x0800_0000) as usize);
    let count = bank.screens[w.hud.screen as usize].count;
    oam_hide(&mut oam, 0, 0x37);
    let at_objects = heap::alloc_zeroed(&mut m, 0x370);
    let listed = m.u16(m.u32(level + 0x2C) + w.hud.screen as u32 * 8 + 6) as usize;
    w.hud.objects = vec![Object::default(); view::hud::OBJECTS as usize];
    for o in w.hud.objects.iter_mut().take(listed.min(0x37)) {
        o.flags = 1;
    }
    let uploads = ui::update_sprites(
        rom,
        &bank,
        w.hud.screen as usize,
        &mut w.hud.objects,
        &mut oam,
        true,
        w.hud.tile_base,
    );
    for u in uploads {
        let at = 0x1_0000 + 32 * u.tile;
        d.vram[at..at + u.len].copy_from_slice(&rom[u.src..u.src + u.len]);
    }
    w.hud.oam = oam;
    let mut f = w.hud_frame();
    hud::reset(rom, &f.g, &mut f.objects, count, &mut f.messages);
    w.set_hud_frame(f);

    // setup_race_cars: the rand seed, the player's atlas, the palettes, the four racer entities.
    let link = w.g.link != 0;
    w.slots[0].e.car = w.cars[0] as u8;
    w.g.rand = w.lp.ticks.wrapping_add(seed_vblanks) & 0xFF;
    let first_car = w.slots[0].e.car as usize;
    let record = w.records[first_car].clone();
    if !link {
        let buf = unpack_player_atlas(&mut m, &w, s.atlases[0], mats, tex, &record)?;
        w.atlases[0] = heap_offset(buf);
        let player = w.g.player as usize;
        let car = w.cars[0] as i32 as u32;
        w.slots[player].e.material = (m.u16(CAR_TABLE + car * 0x58 + 0xC) as u32 + record.exhaust as u32) as u16;
    }
    load_car_palettes(rom, &mut palette, &w);
    let (career, player) = (w.g.career == 1, w.g.player as usize);
    for i in 0..4usize {
        let unused = w.g.results[0xC + i] == 0xFF;
        let e = &mut w.slots[i].e;
        if unused {
            (e.flags, e.state, e.handler) = (0, 0, 0xE);
            continue;
        }
        e.atlas = 0;
        e.flags &= 0xFFF7;
        if i == player && !link {
            e.car = w.cars[0] as u8;
            e.atlas = EWRAM | ((w.atlases[0]) & 0x3_FFFF);
            e.flags |= 8;
            e.model = m.u16(CAR_TABLE + e.car as u32 * 0x58 + 0x10) as i16;
            let spoiler = m.i16(0x087F_0636 + record.spoiler as u32 * 2 + e.car as u32 * 0x20);
            e.extra_model = spoiler.max(0);
        } else {
            let look = atlas::look(rom, w.cars, i, link, career);
            e.car = look.car;
            e.model = look.model as i16;
            e.material = look.material;
            e.flags |= 2;
        }
        e.u_70 = 0x640;
        w.g.results[i] = e.car;
        e.handler = if i == player || link { 0 } else { 0x29 };
    }
    marker_entity(&mut w);
    camera_init(rom, data, &mut w);
    w.g.phase = 9;
    w.camera.view = 2;
    camera_place(rom, &mut w, d);

    // The effect sprites, the glass colour of the player's car, the car palettes again.
    w.pool = vec![Sprite::default(); 0x20];
    w.pool_first = 0x7F;
    let at_pool = heap::alloc_zeroed(&mut m, 0x20 * 0x14);
    w.paints[0] = w.records[w.slots[0].e.car as usize].glass as i8;
    load_car_palettes(rom, &mut palette, &w);
    w.palette_fade = palette.clone();
    w.palette_base = palette;
    // FUN_0813a4f4: the sky gradient restarts, DISPSTAT's VCount IRQ is on.
    w.gradient_start = 0;
    d.io[4] |= 0x20;
    // obj_upload_tiles(0x1C0, 0, HUD material 23's texels): 1-D mapping only.
    let mat = 0x0836_D298;
    let (src, len) = (
        m.u32(mat + 8) + 0x0834_7B74,
        (m.u16(mat + 0xE) as u32 * m.u16(mat + 0xC) as u32) >> 6 << 5,
    );
    if d.io[0] & 0x40 != 0 {
        let at = 0x1_0000 + ((0x1C0 + w.hud.tile_base as u32) & 0xFFFF) as usize * 0x20;
        d.vram[at..at + len as usize].copy_from_slice(m.bytes(src, len as usize));
    }
    w.hud.race_state_changed = 0;
    w.lp.start_state = 0;
    w.g.time = 0;
    // The race music (rand & 3, id + 1; a new id requests its module) and the engine sound.
    let r = nfsgba_fixed::rand_table(rom, &mut w.g.rand);
    let id = (r & 3) as i32 + 1;
    w.lp.music_id = id;
    if s.music != id {
        w.audio.music_request = m.u32(0x087E_E238 + 4 * id as u32);
    }
    let car = w.cars[w.g.player as usize] as i32 as u32;
    let engine = if !link {
        m.u32(car.wrapping_mul(0x58).wrapping_add(CAR_TABLE + 0x48))
    } else {
        m.u32(car.wrapping_mul(0xC).wrapping_add(LOOKS + 8))
    };
    w.profile.engine_sound = engine as u8 as i8;
    w.g.wrong_way = 0;
    // Stale pointers into the previous race's entity array: the wingman setup sets them before they are read.
    (w.g.wingman_car, w.g.wingman_target) = (Default::default(), Default::default());
    if w.hud.enabled == 0 {
        let mut f = w.hud_frame();
        hud::toggle(rom, &f.g, false, &mut f.objects, count, &mut f.messages);
        w.set_hud_frame(f);
    }
    traffic_init(&m, &mut w);
    if w.g.u_5388 != 0 {
        moving_pieces_init(&m, &mut w, walls_base, n_walls);
    }
    // NOT 1:1 (R24): the typed blocks' declared fields into the arena, where they lie next to the atlases and the
    // rim buffers (the block before an atlas is a table or the sprite screen). Undeclared bytes stay zero.
    for (k, o) in w.offsets.iter().enumerate() {
        o.store(&mut m, at_offsets + 0x14 * k as u32);
    }
    for (k, p) in w.pieces.iter().enumerate() {
        p.store(&mut m, at_pieces + 0x20 * k as u32);
        m.set_i16(at_pieces + 0x20 * k as u32 + 0x16, w.piece_kind[k]);
    }
    for (k, &h) in w.heads.iter().enumerate() {
        m.set_u16(at_heads + 2 * k as u32, h);
    }
    for (k, sl) in w.slots.iter().enumerate() {
        sl.e.store(&mut m, at_entities + 0xA4 * k as u32);
    }
    for (k, sec) in w.route.line.sections.iter().enumerate() {
        let rec = SectionRec {
            count: sec.count,
            flags: sec.flags,
            first: sec.first,
        };
        rec.store(&mut m, at_sections + 8 * k as u32);
    }
    for (k, (p, x)) in w.route.line.points.iter().zip(&w.route.extra).enumerate() {
        let rec = WaypointRec {
            x: p.x,
            z: p.z,
            heading: x.heading,
            link_section: p.link_section,
            link_index: p.link_index,
            distance: p.distance,
            sector: x.sector,
        };
        rec.store(&mut m, at_line + 0x18 * k as u32);
    }
    for (k, plane) in w.route.planes.iter().enumerate() {
        plane.store(&mut m, at_planes + 0x20 * k as u32);
    }
    w.route.back.store(&mut m, at_back);
    for (k, o) in w.hud.objects.iter().enumerate() {
        o.store(&mut m, at_objects + 0x10 * k as u32);
    }
    for (k, sp) in w.pool.iter().enumerate() {
        sp.store(&mut m, at_pool + 0x14 * k as u32);
    }
    w.heap = m.ewram;
    Ok(w)
}

/// `oam_hide_range(first, n)`: y = 160 and affine off.
fn oam_hide(oam: &mut ui::Oam, first: usize, n: usize) {
    for e in &mut oam[first..first + n] {
        e[0] = e[0] & 0xFC00 | 0x00A0;
    }
}

fn oam_bytes(oam: &ui::Oam) -> Vec<u8> {
    oam.iter().flatten().flat_map(|v| v.to_le_bytes()).collect()
}

/// `race_load_level`'s racing line: the route's sections and waypoints copied, sprints one slot up with two more
/// points (`sprint_line`), the links rebuilt (`RacingLine`), the plane and back tables (`build_line_planes`).
fn build_route(
    m: &Mem,
    rom: &[u8],
    route_at: u32,
    g: &mut nfsgba_sim::state::CarGlobals,
    sprint: bool,
) -> Result<Route> {
    let index = g.route_index;
    if index == 0 {
        return Err(Unported("route index 0 (no racing-line rebuild)"));
    }
    let (sections_at, line_at) = (m.u32(route_at + 4), m.u32(route_at + 8));
    let mut sections: Vec<SectionRec> = (0..10).map(|k| SectionRec::load(m, sections_at + 8 * k)).collect();
    let mut recs: Vec<WaypointRec> = (0..0x100).map(|k| WaypointRec::load(m, line_at + 0x18 * k)).collect();
    let list = m.u32(0x087F_37D8 + 4 * index);
    let branches = if list == 0 { 0 } else { m.u32(list) } as usize;
    if sprint {
        g.circuit = 0;
        let moved = recs[..254].to_vec();
        recs[2..].clone_from_slice(&moved);
        let count = sections[0].count as usize;
        let moved = recs[2..count + 2].to_vec();
        recs[1..count + 1].clone_from_slice(&moved);
        sections[0].count += 2;
        for s in sections.iter_mut().skip(1).take(branches.min(9)) {
            s.first = s.first.wrapping_add(2);
        }
    }
    let Some(rl) = RacingLine::new(rom, index as usize, sprint) else {
        return Err(Unported("a route index with no racing line"));
    };
    for (r, p) in recs.iter_mut().zip(&rl.points) {
        (r.x, r.z, r.link_section, r.link_index) = (p.x, p.z, p.link_section, p.link_index);
    }
    let section = |s: &SectionRec| Section {
        count: s.count,
        flags: s.flags,
        first: s.first,
    };
    let points: Vec<LinePoint> = recs
        .iter()
        .map(|r| LinePoint {
            x: r.x,
            z: r.z,
            link_section: r.link_section,
            link_index: r.link_index,
            distance: r.distance,
        })
        .collect();
    let line = RacingLine {
        sections: sections.iter().take(branches + 1).map(section).collect(),
        points: points.clone(),
        scales: Vec::new(),
    };
    let mut planes = vec![[0i32; 8]; 0x100];
    let back = line.planes(g.circuit != 0, &mut planes);
    Ok(Route {
        line: RacingLine {
            sections: sections.iter().map(section).collect(),
            points,
            scales: g.scales[..10].to_vec(),
        },
        extra: recs
            .iter()
            .map(|r| PointExtra {
                heading: r.heading,
                sector: r.sector,
            })
            .collect(),
        planes,
        back,
    })
}

/// `unpack_player_atlas(world, 0, record)` (`0x0813b828`): the player's car material into a new heap buffer (the
/// old one freed), remapped into the car slots, then the overlay and the decal set, each through a temporary heap
/// buffer. Every freed buffer keeps what was written into it. Returns the buffer.
fn unpack_player_atlas(m: &mut Mem, w: &World, old: u32, mats: u32, tex: u32, record: &CarRecord) -> Result<u32> {
    let car = w.cars[0] as i32 as u32;
    let material = (m.u16(CAR_TABLE + car * 0x58 + 0xC) as u32 + record.exhaust as u32) as u16;
    let mat = mats + material as u32 * 0x24;
    let size = m.u16(mat + 0xE) as u32 * m.u16(mat + 0xC) as u32;
    if old != 0 {
        heap::free(m, EWRAM | old);
    }
    let buf = heap::alloc(m, size);
    let src = tex + m.u32(mat + 8);
    let ring = heap::alloc(m, 0x1011);
    ring_decode(m, src, buf, ring);
    heap::free(m, ring);
    let mut pixels = m.bytes(buf, size as usize).to_vec();
    paint::remap_atlas(&mut pixels, 0xD0, 0xC0);
    m.set_bytes(buf, &pixels);
    let part = car * 7 + record.u_01 as u32;
    let overlay = m.i16(0x087E_F5A0 + 2 * part);
    if overlay >= 0 {
        let at = 0x087E_F672 + car * 0x1C + record.u_01 as u32 * 4;
        let dst = buf.wrapping_add((m.i16(at + 2) as i32 * 0x100 + m.i16(at) as i32) as u32);
        blit_material(m, (mats, tex), dst, overlay as u32, 0xD0, None)?;
    }
    let set = record.u_04 as u32;
    if set != 0 {
        for j in 0..3 {
            let d = m.i16(0x087E_EBBC + (set - 1) * 6 + 2 * j);
            if d >= 0 {
                let at = 0x087E_EB70 + car * 0x9C + d as u32 * 4;
                let dst = buf.wrapping_add((m.i16(at + 2) as i32 * 0x100 + m.i16(at) as i32) as u32);
                blit_material(m, (mats, tex), dst, d as u32, 0xF0, Some((0xD0, 0xDF)))?;
            }
        }
    }
    Ok(buf)
}

/// `blit_material_keyed` (`only: None`) / `blit_material_keyed_inside`: vehicle material `index` decoded into a
/// temporary heap buffer (raw format 5 is read in place), then blitted into the 256-wide atlas at `dst`, skipping
/// key 0 and adding `add`; the inside variant writes only over atlas pixels in `lo..=hi`.
fn blit_material(
    m: &mut Mem,
    (mats, tex): (u32, u32),
    dst: u32,
    index: u32,
    add: u8,
    only: Option<(u8, u8)>,
) -> Result<()> {
    let mat = mats + index * 0x24;
    let (w, h) = (m.u16(mat + 0xC) as u32, m.u16(mat + 0xE) as u32);
    let src = tex + m.u32(mat + 8);
    let (format, packed) = (m.u8(mat + 0x22), m.u16(mat + 2) & 0x40 != 0);
    let raw = format == 5 && !packed;
    let tmp = if raw { src } else { heap::alloc(m, w * h) };
    if packed {
        let ring = heap::alloc(m, 0x1011);
        ring_decode(m, src, tmp, ring);
        heap::free(m, ring);
    } else if format == 3 {
        let mut n = (w * h) as i32;
        let (mut s, mut d) = (src, tmp);
        loop {
            let b = m.u8(s);
            m.set_bytes(d, &[b >> 4, b & 0xF]);
            (s, d, n) = (s + 1, d + 2, n - 2);
            if n <= 0 {
                break;
            }
        }
    } else if format == 4 {
        return Err(Unported("vehicle material format 4 (FUN_081636d0)"));
    }
    let pixels = m.bytes(tmp, (w * h) as usize).to_vec();
    for y in 0..h {
        for x in 0..w {
            let p = pixels[(y * w + x) as usize];
            let at = dst.wrapping_add(y * 0x100 + x);
            let keep = only.is_some_and(|(lo, hi)| !(lo..=hi).contains(&m.u8(at)));
            if p != 0 && !keep {
                m.set_u8(at, p.wrapping_add(add));
            }
        }
    }
    if !raw {
        heap::free(m, tmp);
    }
    Ok(())
}

/// The game's decompressor (`lz77_ring_decode`, ARM `0x030042f4`, the loop in `ui::ring_decode`): the BIOS LZ77
/// stream at `src` (ROM) into `dst`, through the 4 KiB ring at `ring`, whose bytes 0..0xFEE are 0xFF-filled first
/// (the rest keeps what the heap held). It writes the header size (8 more than the image, past its block), and
/// the ring's final contents stay in RAM.
fn ring_decode(m: &mut Mem, src: u32, dst: u32, ring: u32) {
    struct Ram<'a> {
        m: &'a mut Mem,
        src: u32,
        dst: u32,
        ring: u32,
    }
    impl ui::RingIo for Ram<'_> {
        fn next(&mut self) -> u8 {
            let b = self.m.u8(self.src);
            self.src += 1;
            b
        }
        fn ring(&self, i: usize) -> u8 {
            self.m.u8(self.ring + i as u32)
        }
        fn set_ring(&mut self, i: usize, b: u8) {
            self.m.set_u8(self.ring + i as u32, b);
        }
        fn put(&mut self, b: u8) {
            self.m.set_u8(self.dst, b);
            self.dst += 1;
        }
    }
    let size = m.u32(src) >> 8;
    for k in 0..0xFEE {
        m.set_u8(ring + k, 0xFF);
    }
    ui::ring_decode(
        &mut Ram {
            m,
            src: src + 4,
            dst,
            ring,
        },
        size,
    );
}

/// `load_car_palettes(1)` (`0x0813b6d0`) through `paint::load_car_palettes`, on the base palette.
pub(crate) fn load_car_palettes(rom: &[u8], palette: &mut [u16], w: &World) {
    let r = &w.records[w.cars[0] as usize];
    let mut record = [0u8; 0x11];
    record[..7].copy_from_slice(&[r.spoiler, r.u_01, r.rim, r.exhaust, r.u_04, r.paint, r.glass]);
    record[7..].copy_from_slice(&r.upgrades);
    paint::load_car_palettes(rom, palette, w.cars, w.paints, &record, true);
}

/// `FUN_0813b0c8`: with a wingman, a marker entity (model 0x62, material 0x92) in the first free extra slot,
/// 0x80 above the last AI car.
fn marker_entity(w: &mut World) {
    let (count, extra) = w.spare;
    let free = (count..count + extra).find(|&i| w.slots[i].e.state & 1 == 0);
    let Some(free) = free.filter(|_| w.g.wingman > 0) else {
        return;
    };
    let src = w.slots[w.g.racers as usize].e.clone();
    let e = &mut w.slots[free].e;
    e.race_state = 0;
    e.handler = 0xF;
    e.model = 0x62;
    e.material = 0x92;
    e.pos = [src.pos[0], src.pos[1] - 0x80, src.pos[2]];
    e.sector = src.sector;
    e.draw_next = 0xFFFF;
    e.state = 7;
    e.flags = 2;
    e.material_offset = 0;
    e.next = 0xFFFF;
    e.material_step = 0;
    (e.dir_x, e.u_1c, e.dir_z, e.heading) = (0, 0, 0, 0);
    e.angles[1] = 0;
    e.slot = 0xFF;
}

/// `rotation_y` then `transform_point` with the view table's (x, ?, distance): rows 0 and 2 of the result (the
/// y input is an uninitialised stack word in the game; rotation about y never mixes it into x or z).
fn orbit(rom: &[u8], angle: i32, x: i32, z: i32) -> (i32, i32) {
    let (c, s) = (math::cos(rom, angle), math::sin(rom, angle));
    (
        x.wrapping_mul(c).wrapping_add(z.wrapping_mul(s)) >> 14,
        x.wrapping_mul(-s).wrapping_add(z.wrapping_mul(c)) >> 14,
    )
}

/// The chase camera around the followed entity for view `view` (`camera_place`'s placement).
fn place_camera(rom: &[u8], w: &mut World) {
    let rd = |o: usize| i32::from_le_bytes(rom[o..o + 4].try_into().unwrap());
    let view = w.camera.view as usize * 4;
    let e = &w.slots[w.g.focus as usize].e;
    let (x, z) = (rd(0x7F_39BC + view) << 8, rd(0x7F_39EC + view) << 8);
    w.camera.height = rd(0x7F_39D4 + view);
    let yaw = e.heading >> 8;
    w.camera.orbit = yaw;
    let (ox, oz) = orbit(rom, yaw, x, z);
    w.camera.x = e.pos[0].wrapping_add(ox);
    w.camera.z = e.pos[2].wrapping_add(oz);
}

/// `camera_init(world, 0)` (`0x081378cc`): view 4 around the followed entity, the camera sector and look
/// direction from it, the floor height below the camera.
fn camera_init(rom: &[u8], data: &GameData, w: &mut World) {
    let e = &w.slots[w.g.focus as usize].e;
    let (sector, heading) = (e.sector, e.heading);
    w.camera.view = 4;
    w.camera.look = ((heading as u32 & 0x3F_FFFF) >> 8) as i32;
    w.camera.sector = sector;
    place_camera(rom, w);
    let geometry = Geometry {
        rom,
        city: &data.city,
        pieces: &w.pieces,
        offsets: &w.offsets,
    };
    let floor = geometry.floor_height(sector as u32, w.camera.x >> 8, w.camera.z >> 8);
    w.camera.floor = floor.wrapping_add(-0x8000);
}

/// `FUN_08138744(world)`: every OAM entry hidden and copied to OAM; the camera placed for the view (2, chase).
fn camera_place(rom: &[u8], w: &mut World, d: &mut Display) {
    oam_hide(&mut w.hud.oam, 0, 0x80);
    d.oam = oam_bytes(&w.hud.oam);
    if w.camera.view != 0 {
        place_camera(rom, w);
    }
}

/// `FUN_08144d94`: traffic state reset and the traffic setting's tuning (`0x7F53EC`, 0x20 per setting).
fn traffic_init(m: &Mem, w: &mut World) {
    let g = &mut w.g;
    g.traffic_count = 0;
    g.traffic_timer = 0x60;
    g.live = Default::default();
    let mut setting = (g.u_5604 as i32).max(0) as u32;
    if setting > 4 {
        setting = 3;
    }
    let t = 0x087F_53EC + setting * 0x20;
    g.traffic_period = m.u32(t + 8);
    g.traffic_stop = m.i32(t + 0xC);
    g.speed_up_steps = m.u32(t + 0x14);
    g.traffic_max_speed = m.u32(t + 0x18);
    g.traffic_on = m.u32(t) as u8;
    g.traffic_models = 7;
}

/// `FUN_0813b4d0(world)`: the moving pieces' start state; the route's list (`0x7F3A24`, 0x10 per route index)
/// names the pieces that start closed.
fn moving_pieces_init(m: &Mem, w: &mut World, walls: u32, n_walls: u32) {
    for k in 0..n_walls {
        let at = walls + 0x44 * k;
        let piece = m.u16(at + 0x2A);
        if piece == 0xFFFF {
            continue;
        }
        let r = piece as usize;
        if m.u8(at + 0x2A + 0x15) == 1 {
            w.piece_kind[r] = 1;
            w.pieces[r].material = 0;
            w.pieces[r].flags |= 0x1000;
        } else {
            w.pieces[r].flags = w.pieces[r].flags & 0xEFFF | 1;
        }
    }
    let mut at = 0x087F_3A24 + w.g.route_index * 0x10;
    loop {
        let piece = m.i16(at);
        if piece == -1 {
            break;
        }
        let r = piece as usize;
        w.pieces[r].flags = (w.pieces[r].flags | 0x1000) & 0xFFFE;
        w.piece_kind[r] = 0;
        at += 2;
    }
}

/// Game state 4 (the race start) with `race_start` done: a `Game::frame` on it runs the rest of that frame
/// (`Game::state4_tail`: the fade in, the first `race_frame_update`) and the race from there, the intro and the
/// countdown included.
pub fn enter_race(w: &mut World) {
    w.lp.game_state = 4;
}

/// The top-level fields of two worlds that differ, by name, leaving out the heap arena (the tests and `synth_race`
/// compare that with [`arena_view`]).
pub fn differing(a: &World, b: &World) -> Vec<&'static str> {
    let mut out = Vec::new();
    macro_rules! d {
        ($($f:ident),*) => { $( if a.$f != b.$f { out.push(stringify!($f)); } )* }
    }
    d!(
        slots,
        heads,
        spare,
        g,
        profile,
        contact,
        needle_scale,
        query,
        pieces,
        piece_kind,
        offsets,
        route,
        grid,
        walls_base,
        camera_player,
        camera,
        screen,
        view,
        input,
        root,
        rect,
        ceiling,
        sky,
        matrices,
        pool,
        pool_first,
        slot_counter,
        lights,
        rim_pixels,
        atlases,
        records,
        hud,
        lp,
        cars,
        paints,
        palette_base,
        palette_fade,
        fade_gradient,
        fade_obj,
        gradient,
        gradient_start,
        materials,
        material_info,
        wall_count,
        audio,
        car_at,
        arena
    );
    out
}

/// The heap arena as the renderer and the rim redraw read it (R24): the atlases (`World::atlas`) and the heap's
/// node table, which fixes where every block landed.
pub fn arena_view(w: &World) -> (Vec<Option<Vec<u8>>>, Vec<u8>) {
    let nodes = (w.arena[0] & 0x3_FFFF) as usize;
    (
        (0..4).map(|i| w.atlas(i).map(<[u8]>::to_vec)).collect(),
        w.heap[nodes..nodes + 0x800].to_vec(),
    )
}

/// A [`Game`](crate::Game) on the race start of `setup`, in game state 5 ([`enter_race`]).
pub fn start(rom: Vec<u8>, setup: &Setup, mut display: Display, seed_vblanks: u32) -> Result<crate::Game> {
    let data = std::sync::Arc::new(GameData::parse(&rom));
    let mut world = race_start(&rom, &data, setup, seed_vblanks, &mut display)?;
    enter_race(&mut world);
    let mut game = crate::Game::with_world(rom, data, world, display.palette, display.vram, display.oam);
    game.dispcnt = u16::from_le_bytes([display.io[0], display.io[1]]);
    Ok(game)
}
