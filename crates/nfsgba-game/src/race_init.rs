//! The race start: `race_start_from_table_a` (`0x08139e34`), which `game_state_step` runs in state 4, and
//! everything below it (`race_init`, `race_load_level`, the racing-line rebuild, the palettes, the sprite screen
//! and HUD reset, the racers, the camera, the music). It turns the machine the menus left into the race's first
//! state, byte for byte (`docs/engine/race-init.md`).
//!
//! Its inputs are that machine: the setup the menus wrote (route number and index, environment, mode, laps,
//! opponents, difficulty, traffic, catch-up, HUD and sound options), the profile with the car records and the
//! wingman, the RNG index, the VBlank tick counter (the race's rand seed), and the previous scene's leftovers the
//! game reads or keeps: the heap's node list (new blocks go into its gaps), the bytes of freed blocks, the lapped
//! flag of the last race, IWRAM's overlay state. Hardware state is the I/O block ([`Io`]).
//!
//! The IRQs that fire while it runs (about 20 VBlanks) are not part of this: like the game loop, a caller that
//! wants the emulator's exact state runs them (the sound mix, the counters). The one they feed back is the tick
//! counter `0x03000044`, which `setup_race_cars` uses as the rand seed.

use nfsgba_formats::{
    atlas,
    career::{LinePoint, RacingLine, Section},
    hud, paint,
    ui::{self, SpriteBank},
};
use nfsgba_sim::{Mem, Result, Unported, heap, math, world};

use crate::{Machine, view};

/// The GBA I/O registers (`0x04000000`, 0x400 bytes): the race start sets DISPCNT's OBJ bits, BLDCNT/BLDALPHA
/// and DISPSTAT's VCount IRQ enable.
pub type Io = [u8; 0x400];

const WORLD: u32 = view::WORLD;
const VIEW: u32 = 0x0300_0080;
const FB: u32 = 0x0300_6410;
const SHADOW_OAM: u32 = 0x0300_64F0;
const TILE_BASE: u32 = 0x0300_64E0;
const PROFILE: u32 = 0x0300_56EC;
const PLAYER: u32 = 0x0300_0060;
const CARS: u32 = 0x0300_611C;
const PAINTS: u32 = 0x0300_5FEC;
const RECORDS: u32 = 0x0300_539C;
const OPPONENTS: u32 = 0x0300_5784;
const AI_CARS: u32 = 0x0300_57EC;
const WINGMAN: u32 = 0x0300_6104;
const SLOTS: u32 = 0x0300_5650;
const RAND: u32 = 0x0300_64C8;
const TICK: u32 = 0x0300_0044;
const MODE: u32 = 0x0300_56E0;
const ROUTE_INDEX: u32 = 0x0300_5720;
const LAPPED: u32 = 0x0300_608C;
const LINK_5624: u32 = 0x0300_5624;
const ATLASES: u32 = 0x0300_6164;
const CAR_TABLE: u32 = 0x087F_0BD8;
const LOOKS: u32 = 0x087E_EA44;
const ROUTES: u32 = 0x087F_2798;
const LEVELS: u32 = 0x087F_2B08;

/// `race_start_from_table_a(world)`: the wingman and the racer count, the opponents' cars and paints, the racer
/// slots, `race_init` with the environment's level descriptor.
///
/// `seed_vblanks`: the VBlank IRQs that run before `setup_race_cars` reads the tick counter `0x03000044` as the
/// rand seed (the only timing the result depends on; 6 or 7 in the recorded race starts, 0 for the IRQ-free
/// oracle). The tick counter in RAM is left alone: advancing it is the IRQs' job.
pub fn race_start(g: &mut Machine, io: &mut Io, seed_vblanks: u32) -> Result<()> {
    let m = &mut g.mem;
    let wingman = m.u32(m.u32(PROFILE) + 0x200);
    m.set_u32(WINGMAN, wingman);
    let opponents = m.u32(OPPONENTS);
    m.set_u32(AI_CARS, opponents);
    if wingman.wrapping_sub(1) < 12 {
        m.set_u32(AI_CARS, opponents + 1);
    }
    if m.u32(AI_CARS) > 3 {
        m.set_u32(OPPONENTS, 2);
        m.set_u32(AI_CARS, 3);
    }
    // pick_opponent_cars (FUN_0813b634)
    let (mut rand, mut cars, mut paints) = (m.u32(RAND), racer_bytes(m, CARS), racer_bytes(m, PAINTS));
    atlas::pick_opponent_cars(&m.rom, &mut rand, m.u32(OPPONENTS), wingman, &mut cars, &mut paints);
    m.set_u32(RAND, rand);
    for i in 0..4 {
        m.set_u8(CARS + i, cars[i as usize] as u8);
        m.set_u8(PAINTS + i, paints[i as usize] as u8);
    }
    // FUN_08139eac: the racer slots in use.
    for i in 0..4 {
        let v = if i < m.u32(AI_CARS) + 1 { i as u8 } else { 0xFF };
        m.set_u8(SLOTS + 0xC + i, v);
    }
    let level = m.u32(0x0300_006C) * 0x68 + LEVELS;
    race_init(g, io, level, seed_vblanks)?;
    g.mem.set_u32(0x0300_6098, 0);
    Ok(())
}

fn racer_bytes(m: &Mem, at: u32) -> [i8; 4] {
    std::array::from_fn(|i| m.i8(at + i as u32))
}

/// `race_init(world, level)` (`0x081397d8`).
fn race_init(g: &mut Machine, io: &mut Io, level: u32, seed_vblanks: u32) -> Result<()> {
    let m = &mut g.mem;
    m.set_u32(0x0300_0048, 0);
    m.set_u32(0x0300_610C, 1);
    m.set_u32(0x0300_5620, level);
    m.set_u32(0x0300_53E8, 1);
    let profile = m.u32(PROFILE);
    m.set_u8(profile + 0x400, 0);
    m.set_u8(profile + 0x401, 0);
    overlay_load(m, 0x0300_0220, 0x5164);
    overlay_copy(m, 0, 0x1420, 0x0816_5218);
    // Both mode-4 pages cleared.
    let size = (m.i16(FB + 2) as i32 * m.i16(FB) as i32) as usize;
    for page in [m.u32(FB + 0xC), m.u32(FB + 0x10)] {
        let o = (page - 0x0600_0000) as usize;
        g.vram[o..o + (size & !3)].fill(0);
    }
    oam_reset(g, io);
    io[0x50..0x52].copy_from_slice(&0x3F3Fu16.to_le_bytes());
    io[0x52..0x54].copy_from_slice(&0x0D0Fu16.to_le_bytes());
    race_load_level(&mut g.mem, level)?;
    let m = &mut g.mem;
    m.set_u32(0x0300_53AC, m.u32(WORLD + 0x3C) + m.u32(PLAYER) * 0xA4);
    m.set_u32(0x0300_53A0, 0);
    m.set_u32(0x0300_56B8, 0);
    let profile = m.u32(PROFILE);
    m.set_u32(profile + 0x2E8, 0);
    m.set_u8(profile + 0x2EC, 0);
    for o in [0x2FC, 0x300, 0x304, 0x308, 0x314] {
        m.set_u32(profile + o, 0);
    }
    let runtime = m.u32(WORLD + 0x48);
    for k in 0..(m.u16(WORLD + 0xD8) as u32 * 8) >> 2 {
        m.set_u32(runtime + 4 * k, 0);
    }
    race_load_palettes(m, level);
    let mode = m.i32(MODE);
    if (0..=3).contains(&mode) {
        let screen = WORLD + 0xA4;
        m.set_u32(screen, m.u32(WORLD + 0x28));
        m.set_u32(screen + 4, m.u32(WORLD + 8));
        m.set_u32(screen + 8, m.u32(WORLD + 0x50));
        m.set_u32(screen + 0xC, m.u32(level + 0x30));
        m.set_u32(screen + 0x10, m.u32(level + 0x2C));
        m.set_u32(screen + 0x14, 0);
        m.set_u16(screen + 0x1A, 1);
        m.set_u16(screen + 0x18, [0, 0, 2, 1][mode as usize]);
        sprite_screen_alloc(m);
        sprite_screen_init(g);
        hud_reset(&mut g.mem);
    }
    let m = &mut g.mem;
    spawn_template_entities(m);
    link_entities(m);
    let e0 = m.u32(WORLD + 0x3C);
    m.set_u8(e0 + 0x89, m.u8(CARS));
    setup_race_cars(m, seed_vblanks)?;
    marker_entity(m);
    camera_init(m);
    m.set_u32(0x0300_0048, 9);
    m.set_u32(0x0300_55F8, 2);
    camera_place(g);
    let m = &mut g.mem;
    m.set_u16(0x0300_0058 + 6, 0x10);
    effect_list(m, 0x0300_0058, 0x20, 0x7F);
    m.set_u16(0x0300_5FD0 + 0xC, 8);
    m.set_u16(0x0300_5FD0 + 0xA, 0);
    let e0 = m.u32(WORLD + 0x3C);
    let glass = m.u8(m.u32(RECORDS) + 0x11 * m.u8(e0 + 0x89) as u32 + 6);
    m.set_u32(0x0300_00B8, glass as u32);
    m.set_u8(PAINTS, glass);
    load_car_palettes(m);
    // FUN_0813a4f4: the sky gradient's read pointer, and DISPSTAT's VCount IRQ enable.
    m.set_u32(0x0300_56E8, m.u32(0x0300_53B8));
    io[4] |= 0x20;
    // obj_upload_tiles(0x1C0, 0, HUD texels + material 23, …): the tiles of HUD material 23.
    let mat = 0x0836_D298;
    let (src, len) = (
        m.u32(mat + 8) + 0x0834_7B74,
        (m.u16(mat + 0xE) as u32 * m.u16(mat + 0xC) as u32) >> 6 << 5,
    );
    obj_upload(g, io, 0x1C0, src, len);
    let m = &mut g.mem;
    m.set_u32(0x0300_00AC, 0);
    m.set_u32(0x0300_5714, 0);
    m.set_u32(0x0300_5800, 0);
    let mut rand = m.u32(RAND);
    let r = nfsgba_fixed::rand_table(&m.rom, &mut rand);
    m.set_u32(RAND, rand);
    let profile = m.u32(PROFILE);
    m.set_u8(profile + 0x2EE, (r & 3) as u8);
    play_music(m, m.i8(profile + 0x2EE) as i32 + 1);
    let car = m.i8(CARS + m.u32(PLAYER)) as i32 as u32;
    let engine = if m.u32(LINK_5624) == 0 {
        m.u32(car.wrapping_mul(0x58).wrapping_add(CAR_TABLE + 0x48))
    } else {
        m.u32(car.wrapping_mul(0xC).wrapping_add(LOOKS + 8))
    };
    m.set_u8(profile + 0x2EF, engine as u8);
    m.set_u32(0x0300_5384, 0);
    m.set_u32(0x0300_57E8, 0);
    if m.u32(0x0300_5698) == 0 {
        hud_off(m);
    }
    traffic_init(m);
    if m.u32(0x0300_5388) != 0 {
        moving_pieces_init(m);
    }
    Ok(())
}

/// `FUN_0815e5b8(base, size)`: the first 0xE4 bytes of the race's ARM overlay (the divide, the block copy and
/// fill) to IWRAM at `base`, and the overlay pointers.
fn overlay_load(m: &mut Mem, base: u32, size: u32) {
    m.set_u32(0x0300_6490, base);
    m.set_u32(0x0300_6498, size);
    copy(m, base, 0x0816_5134, 0xE4);
    m.set_u32(0x0300_6494, base);
    m.set_u32(0x0300_64A0, base + ((0x0816_51AC - 0x0816_5134) & !3));
    m.set_u32(0x0300_649C, base + ((0x0816_51D4 - 0x0816_5134) & !3));
}

/// `FUN_0815e62c(slot, words, src)`: more of the overlay after the first 0xE4 bytes.
fn overlay_copy(m: &mut Mem, slot: u32, words: u32, src: u32) {
    let dst = m.u32(0x0300_6490) + slot * 4 + 0xE4;
    copy(m, dst, src, words << 2);
}

fn copy(m: &mut Mem, dst: u32, src: u32, len: u32) {
    let bytes = m.bytes(src, len as usize).to_vec();
    m.set_bytes(dst, &bytes);
}

/// `FUN_0812b10c`: every OAM entry hidden (copied to OAM) and then, in the shadow only, reset to a 64×64 sprite
/// at (0x3C, 0xA0) with tile 0; OBJ on with 1-D mapping; OBJ tile base 0x200 (mode 4).
fn oam_reset(g: &mut Machine, io: &mut Io) {
    let m = &mut g.mem;
    m.set_u32(TILE_BASE, 0x200);
    for k in 0..0x100 {
        m.set_u32(SHADOW_OAM + 4 * k, 0);
    }
    io[1] |= 0x10;
    oam_hide_range(m, 0, 0x80);
    g.oam.copy_from_slice(g.mem.bytes(SHADOW_OAM, 0x400));
    io[0] = io[0] & 0xBF | 0x40;
    let m = &mut g.mem;
    for i in 0..0x80 {
        let e = SHADOW_OAM + 8 * i;
        m.set_bytes(e, &[0xA0, 0x00, 0x3C, 0xC0]);
        m.set_u16(e + 4, m.u16(e + 4) & 0xFC00 | 0x200);
    }
}

/// `oam_hide_range(first, n)`: y = 160 and affine off.
fn oam_hide_range(m: &mut Mem, first: u32, n: u32) {
    for i in first..first + n {
        let e = SHADOW_OAM + 8 * i;
        m.set_u8(e, 0xA0);
        m.set_u8(e + 1, m.u8(e + 1) & 0xFC);
    }
}

/// `race_load_level(world, level)` (`0x08139454`): the level's tables, the runtime buffers on the heap in the
/// game's order, the route's racing line (rebuilt for the race) and its plane and back tables.
fn race_load_level(m: &mut Mem, level: u32) -> Result<()> {
    let route = ROUTES + m.u32(ROUTE_INDEX) * 0x14;
    let counts = m.u32(route + 0xC);
    m.set_u16(WORLD + 0xDA, m.u16(counts));
    m.set_u16(WORLD + 0xDC, m.u16(counts + 2));
    m.set_u16(WORLD + 0xD8, m.u16(counts + 6));
    m.set_u16(WORLD + 0xF8, m.u16(counts + 4));
    m.set_u16(WORLD + 0xFA, 0x20);
    m.set_u32(WORLD + 0x14, m.u32(level + 0x18));
    m.set_u32(WORLD + 0x10, m.u32(level + 0x14));
    // FUN_08138b20: walls with a moving piece, sectors with an offsets record.
    let walls = m.u32(WORLD + 0x10);
    let pieces = (0..m.u16(WORLD + 0xDC) as u32)
        .filter(|&k| m.u16(walls + 0x44 * k + 0x2A) != 0xFFFF)
        .count();
    m.set_u16(WORLD + 0xE0, pieces as u16);
    let sectors = m.u32(WORLD + 0x14);
    let offsets = (0..m.u16(WORLD + 0xDA) as u32)
        .filter(|&k| m.u16(sectors + 0x30 * k + 0xA) != 0xFFFF)
        .count();
    m.set_u16(WORLD + 0xDE, offsets as u16);
    let a = heap::alloc_zeroed(m, m.u16(WORLD + 0xDE) as u32 * 0x14);
    m.set_u32(WORLD + 0x1C, a);
    let a = heap::alloc_zeroed(m, (m.u16(WORLD + 0xE0) as u32) << 5);
    m.set_u32(WORLD + 0x18, a);
    init_runtime_tables(m);
    m.set_u32(WORLD + 0x20, m.u32(level + 0x1C));
    m.set_u32(WORLD + 0x24, m.u32(level + 0x20));
    m.set_u32(WORLD + 0x28, m.u32(level + 0x24));
    let a = heap::alloc_zeroed(m, (m.u16(WORLD + 0xD8) as u32) << 3);
    m.set_u32(WORLD + 0x48, a);
    m.set_u32(WORLD, m.u32(level + 8));
    m.set_u32(WORLD + 4, m.u32(level + 0xC));
    m.set_u32(WORLD + 8, m.u32(level + 0x10));
    m.set_u32(WORLD + 0x2C, m.u32(level + 0x28));
    let a = heap::alloc_zeroed(m, (m.u16(WORLD + 0xDA) as u32) << 1);
    m.set_u32(WORLD + 0xC, a);
    let a = heap::alloc_zeroed(m, (m.u16(WORLD + 0xF8) as u32 + m.u16(WORLD + 0xFA) as u32) * 0xA4);
    m.set_u32(WORLD + 0x3C, a);
    m.set_u32(WORLD + 0x38, m.u32(route));
    let a = heap::alloc_zeroed(m, 0x50);
    m.set_u32(WORLD + 0x40, a);
    copy_halves(m, a, m.u32(route + 4), 0x50);
    let a = heap::alloc_zeroed(m, 0x1800);
    m.set_u32(WORLD + 0x44, a);
    copy_halves(m, a, m.u32(route + 8), 0x1800);
    if m.u32(ROUTE_INDEX) != 0 {
        let branches = m.u32(0x087F_37D8 + 4 * m.u32(ROUTE_INDEX));
        m.set_u32(0x0300_6108, if branches == 0 { 0 } else { m.u32(branches) });
        rebuild_line(m)?;
    }
    m.set_u32(WORLD + 0x50, VIEW);
    let a = heap::alloc_zeroed(m, 0x1A00);
    m.set_u32(WORLD + 0x68, a);
    m.set_u16(WORLD + 0xEC, 0);
    m.set_u16(WORLD + 0xEA, 0);
    let a = heap::alloc_zeroed(m, 0x400);
    m.set_u32(WORLD + 0x60, a);
    let a = heap::alloc(m, 0x2000);
    m.set_u32(WORLD + 0x64, a);
    let a = heap::alloc_zeroed(m, 0x780);
    m.set_u32(WORLD + 0x6C, a);
    m.set_u32(WORLD + 0x78, 0x087F_38B8);
    for (w, l) in [
        (0x7C, 0x34),
        (0x80, 0x38),
        (0x84, 0x3C),
        (0x88, 0x40),
        (0x8C, 0x44),
        (0x90, 0x48),
        (0x94, 0x4C),
        (0x9C, 0x54),
        (0x98, 0x50),
    ] {
        m.set_u32(WORLD + w, m.u32(level + l));
    }
    m.set_u32(WORLD + 0xA0, 0x0300_53F0);
    m.set_u32(0x0300_57F8, m.u32(PLAYER));
    let a = heap::alloc_zeroed(m, 0x2000);
    m.set_u32(0x0300_5FB4, a);
    let a = heap::alloc_zeroed(m, 0x400);
    m.set_u32(0x0300_5FB8, a);
    build_line_planes(m);
    Ok(())
}

/// `FUN_08160d18(dst, src, n, 0x10)`: a halfword copy.
fn copy_halves(m: &mut Mem, dst: u32, src: u32, n: u32) {
    copy(m, dst, src, n & !1);
}

/// `FUN_08138b80`: the moving-piece records (world `+0x18`) get the wall's flags; the sector offsets records
/// (`+0x1C`) the sector's flags byte, `+0x0A |= 0xFFFF` and its three words `+0x14..+0x18`.
fn init_runtime_tables(m: &mut Mem) {
    let (walls, pieces) = (m.u32(WORLD + 0x10), m.u32(WORLD + 0x18));
    let mut n = 0;
    for k in 0..m.u16(WORLD + 0xDC) as u32 {
        let w = walls + 0x44 * k;
        if m.u16(w + 0x2A) != 0xFFFF {
            m.set_u16(pieces + 0x20 * n + 0xE, m.u16(w + 0x2E));
            n += 1;
        }
    }
    let (sectors, offsets) = (m.u32(WORLD + 0x14), m.u32(WORLD + 0x1C));
    let mut o = offsets;
    for k in 0..m.u16(WORLD + 0xDA) as u32 {
        let s = sectors + 0x30 * k;
        if m.u16(s + 0xA) != 0xFFFF {
            m.set_u16(o + 8, m.u8(s + 0x12) as u16);
            m.set_u16(o + 0xA, m.u16(o + 0xA) | 0xFFFF);
            m.set_u16(o + 0xC, m.u16(s + 0x14));
            m.set_u16(o + 0xE, m.u16(s + 0x16));
            m.set_u16(o + 0x10, m.u16(s + 0x18));
            o += 0x14;
        }
    }
}

/// `sprint_line` and `rebuild_line_links` on the RAM copy: sprints (mode 3) clear the lapped flag and get the
/// line one slot up with extrapolated ends (`career::RacingLine` models both; its points give the positions
/// and rebuilt links, the other record fields move with the records here).
fn rebuild_line(m: &mut Mem) -> Result<()> {
    let sprint = m.u32(MODE) == 3;
    let (table, line) = (m.u32(WORLD + 0x40), m.u32(WORLD + 0x44));
    let route = m.u32(ROUTE_INDEX) as usize;
    if sprint {
        m.set_u32(LAPPED, 0);
        let mut raw = m.bytes(line, 0x1800).to_vec();
        raw.copy_within(0..254 * 0x18, 2 * 0x18);
        let count = m.u16(table) as usize;
        raw.copy_within(2 * 0x18..(count + 2) * 0x18, 0x18);
        m.set_bytes(line, &raw);
        m.set_u16(table, m.u16(table) + 2);
        for i in 0..m.i32(0x0300_6108).max(0) as u32 {
            let f = table + 8 * i + 0xC;
            m.set_u32(f, m.u32(f).wrapping_add(2));
        }
    }
    // rebuild_line_links' scratch: the waypoints of sections 0..=branches.
    let total = (0..=m.u32(0x0300_6108))
        .map(|i| m.u16(table + 8 * i) as u32)
        .sum::<u32>();
    m.set_u32(0x0300_6160, total);
    let Some(rl) = RacingLine::new(&m.rom, route, sprint) else {
        return Err(Unported("a route index with no racing line"));
    };
    for (k, p) in rl.points.iter().enumerate() {
        let r = line + 0x18 * k as u32;
        m.set_i32(r, p.x);
        m.set_i32(r + 4, p.z);
        m.set_u16(r + 0xC, p.link_section);
        m.set_u16(r + 0xE, p.link_index);
    }
    Ok(())
}

/// `build_line_planes` (`0x08138f30`) through `career::RacingLine::planes`, on the line as it is in RAM.
fn build_line_planes(m: &mut Mem) {
    let (table, line) = (m.u32(WORLD + 0x40), m.u32(WORLD + 0x44));
    let branches = m.u32(0x0300_6108) as usize;
    let sections = (0..=branches as u32)
        .map(|i| Section {
            count: m.u16(table + 8 * i),
            flags: m.u16(table + 8 * i + 2),
            first: m.u32(table + 8 * i + 4),
        })
        .collect();
    let points = (0..0x100)
        .map(|k| line + 0x18 * k)
        .map(|r| LinePoint {
            x: m.i32(r),
            z: m.i32(r + 4),
            link_section: m.u16(r + 0xC),
            link_index: m.u16(r + 0xE),
            distance: m.i32(r + 0x10),
        })
        .collect();
    let rl = RacingLine {
        sections,
        points,
        scales: Vec::new(),
    };
    let (planes_at, back_at) = (m.u32(0x0300_5FB4), m.u32(0x0300_5FB8));
    let mut planes: Vec<[i32; 8]> = (0..0x100)
        .map(|k| std::array::from_fn(|j| m.i32(planes_at + 0x20 * k + 4 * j as u32)))
        .collect();
    let back = rl.planes(m.u32(LAPPED) != 0, &mut planes);
    for (k, row) in planes.iter().enumerate() {
        for (j, v) in row.iter().enumerate() {
            m.set_i32(planes_at + 0x20 * k as u32 + 4 * j as u32, *v);
        }
    }
    for (k, v) in back.iter().enumerate() {
        m.set_i32(back_at + 4 * k as u32, *v);
    }
}

/// `race_load_palettes(world, level)` (`0x08138a9c`): the environment's city palette into both base buffers,
/// the OBJ palette pointer, the view struct, a copy of the level's 4 KiB table (`+0x28`, world `+0x2C`) and the
/// vehicle matrix slots.
fn race_load_palettes(m: &mut Mem, level: u32) {
    let block = m.u32(level);
    m.set_u32(WORLD + 0x30, block);
    let palette = block + m.u16(level + 0x5A) as u32 * 2;
    for buffer in [0x0300_577C, 0x0300_55F0] {
        copy(m, m.u32(buffer), palette, 0x200);
    }
    m.set_u32(0x0300_563C, 1);
    m.set_u32(WORLD + 0x34, m.u32(level + 4) + m.u16(level + 0x58) as u32 * 2);
    m.set_u32(VIEW + 0xC, m.i16(FB) as i32 as u32);
    m.set_u32(VIEW + 4, 0x0300_53D0);
    m.set_i16(VIEW + 8, m.i16(FB) >> 1);
    m.set_i16(VIEW + 0xA, m.i16(FB + 2) >> 1);
    m.set_u32(VIEW, 0x0600_0000);
    m.set_u32(VIEW + 0x10, 0x40);
    m.set_u32(VIEW + 0x1C, 0x96);
    let a = heap::alloc(m, 0x1000);
    m.set_u32(VIEW + 0x14, a);
    copy(m, a, m.u32(WORLD + 0x2C), 0x1000);
    let a = heap::alloc(m, 0xC00);
    m.set_u32(WORLD + 0xFC, a);
}

/// `sprite_screen_alloc(world + 0xA4)`: the 0x37 sprite objects, visible for the screen's elements.
fn sprite_screen_alloc(m: &mut Mem) {
    let screen = WORLD + 0xA4;
    let count = m.u16(m.u32(screen + 0x10) + m.u16(screen + 0x18) as u32 * 8 + 6) as u32;
    oam_hide_range(m, 0, 0x37);
    let old = m.u32(screen + 0x14);
    if old != 0 {
        heap::free(m, old);
    }
    m.set_u32(screen + 0x14, 0);
    let a = heap::alloc_zeroed(m, 0x370);
    m.set_u32(screen + 0x14, a);
    for k in 0..count.min(0x37) {
        m.set_u16(a + 0x10 * k, 1);
    }
}

fn sprite_bank(m: &Mem) -> SpriteBank {
    ui::sprite_bank(&m.rom, (m.u32(0x0300_5620) - 0x0800_0000) as usize)
}

/// `sprite_screen_update(world + 0xA4, 1)` through `ui::update_sprites`, with its VRAM uploads.
fn sprite_screen_init(g: &mut Machine) {
    let m = &g.mem;
    let bank = sprite_bank(m);
    let screen = m.u16(view::HUD_SCREEN + 0x18) as usize;
    let (mut objects, mut shadow) = (view::hud_objects(m), view::shadow_oam(m));
    let uploads = ui::update_sprites(&m.rom, &bank, screen, &mut objects, &mut shadow, true, m.u16(TILE_BASE));
    view::store_hud_objects(&mut g.mem, &objects);
    view::store_shadow_oam(&mut g.mem, &shadow);
    for u in uploads {
        let at = 0x1_0000 + 32 * u.tile;
        g.vram[at..at + u.len].copy_from_slice(&g.mem.rom[u.src..u.src + u.len]);
    }
}

/// `hud_reset(world + 0xA4)` through `hud::reset`.
fn hud_reset(m: &mut Mem) {
    hud_with(m, |rom, g, objects, count, messages| {
        hud::reset(rom, g, objects, count, messages)
    });
}

/// `hud_toggle(0)`: the HUD option is off.
fn hud_off(m: &mut Mem) {
    hud_with(m, |rom, g, objects, count, messages| {
        hud::toggle(rom, g, false, objects, count, messages)
    });
}

fn hud_with(m: &mut Mem, f: impl FnOnce(&[u8], &hud::Globals, &mut [ui::Object], usize, &mut hud::Messages)) {
    let bank = sprite_bank(m);
    let count = bank.screens[m.u16(view::HUD_SCREEN + 0x18) as usize].count;
    let g = view::hud_globals(m);
    let (mut objects, mut messages) = (view::hud_objects(m), view::messages(m));
    f(&m.rom, &g, &mut objects, count, &mut messages);
    view::store_hud_objects(m, &objects);
    view::store_messages(m, &messages);
}

/// `race_spawn_template_entities(world, route entities)`: every sector's entity list emptied, the route's
/// template entities copied, the extra slots numbered and cleared.
fn spawn_template_entities(m: &mut Mem) {
    let heads = m.u32(WORLD + 0xC);
    for k in 0..m.u16(WORLD + 0xDA) as u32 {
        m.set_u16(heads + 2 * k, 0xFFFF);
    }
    let (ents, count) = (m.u32(WORLD + 0x3C), m.u16(WORLD + 0xF8) as u32);
    copy(m, ents, m.u32(WORLD + 0x38), (count * 0xA4) & !3);
    let mut e = ents;
    for _ in 0..count {
        m.set_u16(e + 0x4A, 0);
        m.set_u16(e + 0x8C, 0);
        m.set_u16(e + 0x8E, 0);
        e += 0xA4;
    }
    for k in 0..m.u16(WORLD + 0xFA) as u32 {
        m.set_u16(e, (count + k) as u16);
        m.set_u16(e + 8, 0);
        m.set_u16(e + 0x8C, 0);
        m.set_u16(e + 0x8E, 0);
        m.set_u16(e + 0x4A, 0);
        e += 0xA4;
    }
}

/// `FUN_08137618`: each template entity pushed onto its sector's list (entity `+0x78` sector, `+0x02` next).
fn link_entities(m: &mut Mem) {
    let (ents, heads) = (m.u32(WORLD + 0x3C), m.u32(WORLD + 0xC));
    for i in 0..m.u16(WORLD + 0xF8) as u32 {
        let e = ents + 0xA4 * i;
        let sector = m.u16(e + 0x78);
        if sector != 0xFFFF {
            let head = heads + 2 * sector as u32;
            m.set_u16(e + 2, m.u16(head));
            m.set_u16(head, i as u16);
        }
    }
}

/// `setup_race_cars(world, racers, 0, 0)` (`0x0813b9b8`): the rand seed from the tick counter, the player's atlas
/// and the car palettes, then each of the four racer entities dressed (the player from its record, opponents
/// from `0x7EEA44`) or emptied.
fn setup_race_cars(m: &mut Mem, seed_vblanks: u32) -> Result<()> {
    let ents = m.u32(WORLD + 0x3C);
    let records = m.u32(RECORDS);
    let first_car = m.u8(ents + 0x89) as u32;
    // rand_seed (FUN_0815fd1c)
    m.set_u32(RAND, m.u32(TICK).wrapping_add(seed_vblanks) & 0xFF);
    unpack_player_atlas(m, records + first_car * 0x11)?;
    load_car_palettes(m);
    let (link, flag_a0, player) = (m.u32(LINK_5624) != 0, m.u32(0x0300_00A0) == 1, m.u32(PLAYER));
    let cars = racer_bytes(m, CARS);
    for i in 0..4u32 {
        let e = ents + 0xA4 * i;
        if m.u8(SLOTS + 0xC + i) == 0xFF {
            m.set_u16(e + 0xA, 0);
            m.set_u16(e + 8, 0);
            m.set_u16(e + 0x4E, 0xE);
            continue;
        }
        m.set_u32(e + 0x84, 0);
        m.set_u16(e + 0xA, m.u16(e + 0xA) & 0xFFF7);
        if i == player && !link {
            m.set_u8(e + 0x89, m.u8(CARS));
            m.set_u32(e + 0x84, m.u32(ATLASES));
            m.set_u16(e + 0xA, m.u16(e + 0xA) | 8);
            let car = m.u8(e + 0x89) as u32;
            m.set_u16(e + 0x36, m.u16(CAR_TABLE + car * 0x58 + 0x10));
            let spoiler =
                m.i16(0x087F_0636 + m.u8(records + first_car * 0x11) as u32 * 2 + m.u8(e + 0x89) as u32 * 0x20);
            m.set_i16(e + 0x64, spoiler.max(0));
        } else {
            let look = atlas::look(&m.rom, cars, i as usize, link, flag_a0);
            m.set_u8(e + 0x89, look.car);
            m.set_u16(e + 0x36, look.model);
            m.set_u16(e + 0x48, look.material);
            m.set_u16(e + 0xA, m.u16(e + 0xA) | 2);
        }
        m.set_u16(e + 0x70, 0x640);
        m.set_u8(SLOTS + i, m.u8(e + 0x89));
        m.set_u16(e + 0x4E, if i == player || link { 0 } else { 0x29 });
    }
    Ok(())
}

/// `unpack_player_atlas(world, 0, record)` (`0x0813b828`): the player's car material into a new heap buffer
/// (`0x03006164[0]`; the old one freed), remapped into the car slots, then the overlay and the decal set, each
/// through a temporary heap buffer. Every freed buffer keeps what was written into it.
fn unpack_player_atlas(m: &mut Mem, record: u32) -> Result<()> {
    if m.u32(LINK_5624) != 0 {
        return Ok(());
    }
    let car = m.i8(CARS) as i32 as u32;
    let player = m.u32(WORLD + 0x3C) + m.u32(PLAYER) * 0xA4;
    let material = (m.u16(CAR_TABLE + car * 0x58 + 0xC) as u32 + m.u8(record + 3) as u32) as u16;
    m.set_u16(player + 0x48, material);
    let mat = m.u32(WORLD + 0x24) + material as u32 * 0x24;
    let size = m.u16(mat + 0xE) as u32 * m.u16(mat + 0xC) as u32;
    let old = m.u32(ATLASES);
    if old != 0 {
        heap::free(m, old);
        m.set_u32(ATLASES, 0);
    }
    let buf = heap::alloc(m, size);
    m.set_u32(ATLASES, buf);
    let src = m.u32(WORLD + 4) + m.u32(mat + 8);
    let ring = heap::alloc(m, 0x1011);
    ring_decode(m, src, buf, ring);
    heap::free(m, ring);
    let mut pixels = m.bytes(buf, size as usize).to_vec();
    paint::remap_atlas(&mut pixels, 0xD0, 0xC0);
    m.set_bytes(buf, &pixels);
    let part = car * 7 + m.u8(record + 1) as u32;
    let overlay = m.i16(0x087E_F5A0 + 2 * part);
    if overlay >= 0 {
        let at = 0x087E_F672 + car * 0x1C + m.u8(record + 1) as u32 * 4;
        let dst = buf.wrapping_add((m.i16(at + 2) as i32 * 0x100 + m.i16(at) as i32) as u32);
        blit_material(m, dst, overlay as u32, 0xD0, None)?;
    }
    let set = m.u8(record + 4) as u32;
    if set != 0 {
        for j in 0..3 {
            let d = m.i16(0x087E_EBBC + (set - 1) * 6 + 2 * j);
            if d >= 0 {
                let at = 0x087E_EB70 + car * 0x9C + d as u32 * 4;
                let dst = buf.wrapping_add((m.i16(at + 2) as i32 * 0x100 + m.i16(at) as i32) as u32);
                blit_material(m, dst, d as u32, 0xF0, Some((0xD0, 0xDF)))?;
            }
        }
    }
    Ok(())
}

/// `blit_material_keyed` (`only: None`) / `blit_material_keyed_inside`: vehicle material `index` decoded into a
/// temporary heap buffer (raw format 5 is read in place), then blitted into the 256-wide atlas at `dst`, skipping
/// key 0 and adding `add`; the inside variant writes only over atlas pixels in `lo..=hi`.
fn blit_material(m: &mut Mem, dst: u32, index: u32, add: u8, only: Option<(u8, u8)>) -> Result<()> {
    let mat = m.u32(WORLD + 0x24) + index * 0x24;
    let (w, h) = (m.u16(mat + 0xC) as u32, m.u16(mat + 0xE) as u32);
    let src = m.u32(WORLD + 4) + m.u32(mat + 8);
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

/// `load_car_palettes(1)` (`0x0813b6d0`) through `paint::load_car_palettes`, into both base buffers.
fn load_car_palettes(m: &mut Mem) {
    let (cars, paints) = (racer_bytes(m, CARS), racer_bytes(m, PAINTS));
    let record = m.bytes(m.u32(RECORDS) + 0x11 * m.u8(CARS) as u32, 0x11).to_vec();
    for buffer in [0x0300_577C, 0x0300_55F0] {
        let at = m.u32(buffer);
        let mut base: Vec<u16> = (0..256).map(|i| m.u16(at + 2 * i)).collect();
        paint::load_car_palettes(&m.rom, &mut base, cars, paints, &record, true);
        for (i, c) in base.into_iter().enumerate() {
            m.set_u16(at + 2 * i as u32, c);
        }
    }
    m.set_u32(0x0300_563C, 1);
}

/// `FUN_0813b0c8`: with a wingman, a marker entity (model 0x62, material 0x92) in the first free extra slot,
/// 0x80 above the last AI car.
fn marker_entity(m: &mut Mem) {
    let ents = m.u32(WORLD + 0x3C);
    let (count, extra) = (m.u16(WORLD + 0xF8) as u32, m.u16(WORLD + 0xFA) as u32);
    let free = (count..count + extra)
        .map(|i| ents + 0xA4 * i)
        .find(|&e| m.u16(e + 8) & 1 == 0)
        .map_or(0xFFFF, |e| m.u16(e) as u32);
    if m.i32(WINGMAN) <= 0 || free == 0xFFFF {
        return;
    }
    let src = ents + m.u32(AI_CARS) * 0xA4;
    let e = ents + free * 0xA4;
    m.set_u16(e + 0x4A, 0);
    m.set_u16(e + 8, 1);
    m.set_u16(e + 0x4E, 0xF);
    m.set_u16(e + 0x36, 0x62);
    m.set_u16(e + 0x48, 0x92);
    m.set_u32(e + 0xC, m.u32(src + 0xC));
    m.set_i32(e + 0x10, m.i32(src + 0x10) - 0x80);
    m.set_u32(e + 0x14, m.u32(src + 0x14));
    m.set_u16(e + 0x78, m.u16(src + 0x78));
    m.set_u16(e + 4, m.u16(e + 4) | 0xFFFF);
    m.set_u16(e + 8, 7);
    m.set_u16(e + 0xA, 2);
    m.set_u16(e + 0x46, 0);
    m.set_u16(e + 2, m.u16(e + 2) | 0xFFFF);
    m.set_u16(e + 0x44, 0);
    for o in [0x18, 0x1C, 0x20, 0x2C] {
        m.set_u32(e + o, 0);
    }
    m.set_u16(e + 0x32, 0);
    m.set_u8(e + 0x88, 0xFF);
}

/// `rotation_y` then `transform_point` with the view table's (x, ?, distance): rows 0 and 2 of the result (the
/// y input is an uninitialised stack word in the game; rotation about y never mixes it into x or z).
fn orbit(m: &Mem, angle: i32, x: i32, z: i32) -> (i32, i32) {
    let (c, s) = (math::cos(m, angle), math::sin(m, angle));
    (
        x.wrapping_mul(c).wrapping_add(z.wrapping_mul(s)) >> 14,
        x.wrapping_mul(-s).wrapping_add(z.wrapping_mul(c)) >> 14,
    )
}

/// `camera_init(world, 0)` (`0x081378cc`): view 4 at the route's camera waypoint (level descriptor `+0x66`
/// section), the camera sector and yaw from the followed entity, the floor height below it.
fn camera_init(m: &mut Mem) {
    let e = m.u32(WORLD + 0x3C) + m.u32(0x0300_57F8) * 0xA4;
    m.set_u32(0x0300_5FA0, 0x400);
    m.set_u32(0x0300_53A8, 0);
    m.set_u32(0x0300_5FB0, 0xFFFF_FFFF);
    m.set_u16(0x0300_5390, 0);
    m.set_u16(0x0300_5392, 0);
    m.set_u32(0x0300_55F8, 4);
    m.set_u32(0x0300_5778, m.u32(e + 0x10).wrapping_add(0xFFFF_0000));
    m.set_u32(0x0300_55F4, 0);
    m.set_u32(0x0300_5644, 0);
    m.set_u32(0x0300_00B0, 0);
    m.set_u32(0x0300_0210 + 4, (m.u32(e + 0x2C) & 0x3F_FFFF) >> 8);
    m.set_u16(0x0300_5614, m.u16(e + 0x78));
    let view = m.u32(0x0300_55F8) * 4;
    m.set_u32(0x0300_5FA4, m.u32(0x087F_39D4 + view));
    let section = m.u16(m.u32(0x0300_5620) + 0x66) as u32;
    let first = m.u32(m.u32(WORLD + 0x40) + section * 8 + 4);
    let w = m.u32(WORLD + 0x44) + first * 0x18 + m.u32(0x0300_53A8) * 0x18;
    m.set_i32(0x0300_56A0, m.i32(w) << 8);
    m.set_i32(0x0300_00A4, m.i32(w + 4) << 8);
    let e = m.u32(WORLD + 0x3C) + m.u32(0x0300_57F8) * 0xA4;
    let (x, z) = (m.i32(0x087F_39BC + view) << 8, m.i32(0x087F_39EC + view) << 8);
    m.set_u32(0x0300_5FA4, m.u32(0x087F_39D4 + view));
    let yaw = m.i32(e + 0x2C) >> 8;
    m.set_i32(0x0300_5F94, yaw);
    let (ox, oz) = orbit(m, yaw, x, z);
    m.set_i32(0x0300_56A0, m.i32(e + 0xC).wrapping_add(ox));
    m.set_i32(0x0300_00A4, m.i32(e + 0x14).wrapping_add(oz));
    m.set_u16(0x0300_55FC, 0);
    m.set_u16(0x0300_55FE, 0);
    m.set_u32(0x0300_5F8C, 0xFFFE_F000);
    let floor = world::floor_height(m, m.u32(0x0300_5614), m.i32(0x0300_56A0) >> 8, m.i32(0x0300_00A4) >> 8);
    m.set_i32(0x0300_5778, floor.wrapping_add(-0x8000));
}

/// `FUN_08138744(world)`: every OAM entry hidden and copied to OAM; the camera placed for the view (2, chase).
fn camera_place(g: &mut Machine) {
    oam_hide_range(&mut g.mem, 0, 0x80);
    g.oam.copy_from_slice(g.mem.bytes(SHADOW_OAM, 0x400));
    let m = &mut g.mem;
    let view = m.u32(0x0300_55F8) * 4;
    if view == 0 {
        return;
    }
    let e = m.u32(WORLD + 0x3C) + m.u32(0x0300_57F8) * 0xA4;
    let (x, z) = (m.i32(0x087F_39BC + view) << 8, m.i32(0x087F_39EC + view) << 8);
    m.set_u32(0x0300_5FA4, m.u32(0x087F_39D4 + view));
    let yaw = m.i32(e + 0x2C) >> 8;
    m.set_i32(0x0300_5F94, yaw);
    let (ox, oz) = orbit(m, yaw, x, z);
    m.set_i32(0x0300_56A0, m.i32(e + 0xC).wrapping_add(ox));
    m.set_i32(0x0300_00A4, m.i32(e + 0x14).wrapping_add(oz));
}

/// `FUN_0816200c(list, n, v)`: the effect-sprite list header and its `n` 0x14-byte objects on the heap.
fn effect_list(m: &mut Mem, list: u32, n: u32, v: u16) {
    m.set_u16(list + 6, n as u16);
    m.set_u16(list + 4, v);
    let a = heap::alloc_zeroed(m, n * 0x14);
    m.set_u32(list, a);
}

/// `obj_upload_tiles(slot, 0, src, n)`: `len` bytes of tiles to OBJ VRAM at `slot` + the tile base, when
/// DISPCNT has 1-D mapping.
fn obj_upload(g: &mut Machine, io: &Io, slot: u32, src: u32, len: u32) {
    if io[0] & 0x40 == 0 {
        return;
    }
    let at = 0x1_0000 + ((slot + g.mem.u32(TILE_BASE)) & 0xFFFF) as usize * 0x20;
    let bytes = g.mem.bytes(src, len as usize).to_vec();
    g.vram[at..at + len as usize].copy_from_slice(&bytes);
}

/// `carbon_play_music(id)` (`0x08136054`): a new id requests its module (`snd_play_module`: engine `+0x1478`).
fn play_music(m: &mut Mem, id: i32) {
    if m.i32(0x0300_003C) == id {
        return;
    }
    let module = m.u32(0x087E_E238 + 4 * id as u32);
    let engine = m.u32(0x0300_6370);
    m.set_u32(engine + 0x1478, module);
    m.set_i32(0x0300_003C, id);
}

/// `FUN_08144d94`: traffic state reset and the traffic setting's tuning (`0x7F53EC`, 0x20 per setting).
fn traffic_init(m: &mut Mem) {
    m.set_u32(0x0300_6240, 0);
    m.set_u32(0x0300_6264, 0x60);
    for k in 0..8 {
        m.set_u32(0x0300_6270 + 4 * k, 0);
    }
    m.set_u32(0x0300_6248, 4);
    let mut setting = m.i32(0x0300_5604).max(0) as u32;
    if setting > m.u32(0x0300_6248) {
        setting = m.u32(0x0300_6248) - 1;
    }
    let t = 0x087F_53EC + setting * 0x20;
    for (at, o) in [
        (0x0300_6258, 4),
        (0x0300_6260, 8),
        (0x0300_6294, 0xC),
        (0x0300_6254, 0x10),
        (0x0300_624C, 0x14),
        (0x0300_6290, 0x18),
        (0x0300_6244, 0x1C),
    ] {
        m.set_u32(at, m.u32(t + o));
    }
    m.set_u8(0x0300_6298, m.u32(t) as u8);
    m.set_u32(0x0300_625C, 7);
}

/// `FUN_0813b4d0(world)`: the moving pieces' start state; the route's list (`0x7F3A24`, 0x10 per route index)
/// names the pieces that start closed.
fn moving_pieces_init(m: &mut Mem) {
    let (walls, pieces) = (m.u32(WORLD + 0x10), m.u32(WORLD + 0x18));
    for k in 0..m.u16(WORLD + 0xDC) as u32 {
        let w = walls + 0x44 * k;
        let piece = m.u16(w + 0x2A);
        if piece == 0xFFFF {
            continue;
        }
        let r = pieces + piece as u32 * 0x20;
        if m.u8(w + 0x2A + 0x15) == 1 {
            m.set_u16(r + 0x16, 1);
            m.set_u16(r + 0xC, 0);
            m.set_u16(r + 0xE, m.u16(r + 0xE) | 0x1000);
        } else {
            m.set_u16(r + 0xE, m.u16(r + 0xE) & 0xEFFF | 1);
        }
    }
    let mut at = 0x087F_3A24 + m.u32(ROUTE_INDEX) * 0x10;
    loop {
        let piece = m.i16(at);
        if piece == -1 {
            break;
        }
        let r = pieces.wrapping_add((piece as i32 * 0x20) as u32);
        m.set_u16(r + 0xE, (m.u16(r + 0xE) | 0x1000) & 0xFFFE);
        m.set_u16(r + 0x16, 0);
        at += 2;
    }
}
