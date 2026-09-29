//! Race HUD logic: an exact port of `hud_update` (`FUN_08142f84`) and everything it calls, on top of the
//! sprite builder `ui::update_sprites` (`docs/formats/ui.md`, "HUD logic").
//!
//! Inputs are the game state the HUD reads ([`Globals`], [`Racer`], [`Driver`]); the port writes what the
//! game writes: the sprite objects (`ui::Object`), the message slots, the minimap tiles, OBJ palette bank 13
//! and two race-state globals (the timer's time-limit side effect).

use super::{div, u32_at};
use crate::ui::{self, Object};

/// The IWRAM variables the HUD reads and writes. Each field names its address.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Globals {
    /// `0x03005698`: HUD setting. 0 = off: only the timer runs, for its side effect.
    pub hud: u32,
    /// `0x030056E0`: race mode (0 circuit, 1 elimination, 2 hunter, 3 sprint).
    pub mode: i32,
    /// `0x03005600`: language, 0 En … 4 Es.
    pub language: u32,
    /// `0x03000040`: speed units, 0 = mph.
    pub units: u32,
    /// `0x03005800`: race time in frames.
    pub frames: i32,
    /// `0x0300615C`: split time in frames; `hud_split` clamps it to 0 after reading it.
    pub split: i32,
    /// `0x03005784`: opponents.
    pub opponents: u32,
    /// `0x030057EC`: AI cars (opponents, plus 1 with a wingman).
    pub ai_cars: u32,
    /// `0x030056E4`: laps.
    pub laps: u32,
    /// `0x03006104`: wingman, 0 = none.
    pub wingman: u32,
    /// `0x030061DC`: wingman portrait frame.
    pub portrait: i32,
    /// `0x030061D4`: portrait blinks when non-zero.
    pub portrait_blink: u32,
    /// `0x030061E4` / `0x03006188`: wingman bar value and full scale (8 steps).
    pub bar: i32,
    pub bar_max: i32,
    /// `0x0300601C`: arrow direction (0 hidden, sign picks the frames).
    pub arrow: i32,
    /// `0x03005388`: route number, 1-based (picks the minimap palette).
    pub route: u32,
    /// Profile `+0x402` (`*0x030056EC`): the needle's rev scale (0x2000 instead of 0x1C00 when non-zero).
    pub needle_scale: u8,
    /// `0x030057F8`: the entity whose driver the HUD shows.
    pub player: usize,
    /// `0x03000048` and `0x030000AC`: race state and its change flag; `hud_timer` sets 8 and 1 past 59:59.98.
    pub race_state: u32,
    pub race_state_changed: u32,
}

/// The entity fields the HUD reads (entity `+0x0C`, `+0x14`, `+0x2C`, and `+0x8C`, the driver pointer).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Racer {
    /// World position, 8.8 fixed point.
    pub x: i32,
    pub z: i32,
    /// Heading, 8.8 (`>> 8`: 0x4000 per turn).
    pub heading: i32,
    pub driver: Option<Driver>,
}

/// The driver-struct fields the HUD reads (`*(entity + 0x8C)`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Driver {
    /// `+0x3C`: engine revs.
    pub revs: i32,
    /// `+0x40`: gear.
    pub gear: i32,
    /// `+0x44`: speed.
    pub speed: i32,
    /// `+0xA8`: race position, 1-based.
    pub position: i32,
    /// `+0xC5`: laps left.
    pub laps_left: i8,
    /// `+0x454`: rev scale of the needle.
    pub rev_scale: i32,
    /// `+0x4C8`: dial value (0x50000 / 9 per step).
    pub dial: i32,
    /// `+0x4D8`: race flags (bit 3: eliminated).
    pub flags: u16,
    /// `+0x4E8`: hunter life, 0..0x80000.
    pub hunter_life: i32,
}

/// The six HUD message slots at `0x03006210`: element, frame, timer, (unused).
pub type Messages = [[u8; 4]; 6];

/// Digit x shift per language (`0x7F4378`).
const DIGIT_SHIFT: usize = 0x7F_4378;
/// Word masks of the minimap copy, per byte shift (`0x7F5CE8`).
const MINIMAP_MASKS: usize = 0x7F_5CE8;
/// HUD material 135, the 512×384 4bpp city map (raw rows of 256 bytes).
const MAP_MATERIAL: usize = 135;

/// The IWRAM divide routine the HUD calls through `*0x03006494` (`0x03000220`, ROM copy `0x08165134`):
/// shift-and-subtract on the magnitudes, the quotient signed by `n ^ d`, the remainder (stored at
/// `0x03006480`) `n − |q|·d`, so a negative `n` gives an off remainder. The game only passes positive `d`.
pub fn divmod(n: i32, d: i32) -> (i32, i32) {
    debug_assert!(d > 0, "the routine mishandles d <= 0");
    let q = n.unsigned_abs() / d as u32;
    let rem = n.wrapping_sub((q as i32).wrapping_mul(d));
    (
        if (n ^ d) < 0 {
            (q as i32).wrapping_neg()
        } else {
            q as i32
        },
        rem,
    )
}

/// The message table of a race mode (`FUN_08143248`): 16 × (slot, element, frame, 0); slot 0xFF = none.
/// Other modes read an uninitialised register in the game; they never occur.
fn message_table(mode: i32) -> Option<usize> {
    match mode {
        0 | 1 => Some(0x7F_437D),
        2 => Some(0x7F_43FD),
        3 => Some(0x7F_43BD),
        _ => None,
    }
}

fn show(o: &mut Object, visible: bool) {
    o.flags = if visible { o.flags | 1 } else { o.flags & 0xFFFE };
}

/// `hud_update` (`FUN_08142f84`): one frame of HUD logic. `objects` is the sprite screen's object array.
/// Returns the minimap tiles when the minimap ran (64 OBJ tiles, 4bpp, for the minimap element's tile slot).
pub fn update(
    rom: &[u8],
    g: &mut Globals,
    racers: &[Racer; 4],
    objects: &mut [Object],
    messages: &mut Messages,
    obj_palette: &mut [u16],
) -> Option<Vec<u8>> {
    // hud_minimap_palette (FUN_081430c4): OBJ colours 0xD0..0xDF, written every frame.
    obj_palette[0xD0..0xE0].copy_from_slice(&ui::minimap_palette(rom, g.route as usize));
    let mut minimap = None;
    if g.hud != 0 {
        if (0..=3).contains(&g.mode) {
            let d = racers[g.player].driver.expect("the player's entity has a driver");
            let o = &mut *objects;
            panel_language(g, o);
            split_panel_language(rom, g, o);
            timer(g, o);
            split(rom, g, &d, o);
            position(g, &d, o);
            lap(rom, g, &d, o);
            needle(g, &d, o);
            speed(g, &d, o);
            gear(&d, o);
            minimap = Some(self::minimap(rom, g, racers, o));
            if g.mode == 2 {
                portrait(g, o, 51);
                dial(&d, o);
                hunter_bars(g, racers, o);
                arrow(g, o, 53);
            } else {
                portrait(g, o, 43);
                dial(&d, o);
                arrow(g, o, 45);
            }
        }
    } else if (0..=3).contains(&g.mode) {
        timer(g, objects);
    }
    message_tick(objects, messages);
    minimap
}

/// `hud_reset` (`FUN_08142148`): all `count` objects visible and semi-transparent with angle, frame and
/// scale cleared, then the mode's panels (`FUN_08142094`/`FUN_081420d0`/`FUN_0814210c`) and `message_hide`.
pub fn reset(rom: &[u8], g: &Globals, objects: &mut [Object], count: usize, messages: &mut Messages) {
    for o in objects.iter_mut().take(count) {
        o.flags |= 3;
        o.angle = 0;
        o.frame = 0;
        o.scale = [0, 0];
    }
    if (0..=3).contains(&g.mode) {
        objects[14].frame = g.language as i16;
        objects[0].frame = g.language as i16;
        if g.units != 0 {
            objects[32].frame = g.language.wrapping_add(1) as i16;
        }
        let message = if g.mode == 2 { 54 } else { 46 };
        objects[message].flags &= 0xFFFE;
    }
    message_hide(rom, g, objects, messages);
}

/// `FUN_08143010(on)`: the HUD setting toggled in a race: `reset` when on, else every object hidden; then
/// `message_hide`.
pub fn toggle(rom: &[u8], g: &Globals, on: bool, objects: &mut [Object], count: usize, messages: &mut Messages) {
    if on {
        return reset(rom, g, objects, count, messages);
    }
    for o in objects.iter_mut().take(count) {
        o.flags &= 0xFFFE;
    }
    message_hide(rom, g, objects, messages);
}

/// `hud_panel_language` (`FUN_081431f0`): timer panel frame = language; separator 4 frame 3 in Italian.
fn panel_language(g: &Globals, o: &mut [Object]) {
    o[0].frame = g.language as i16;
    match g.language {
        3 => o[4].frame = 3,
        0..=2 | 4 => o[4].frame = 0,
        _ => {}
    }
}

/// `hud_split_panel_language` (`FUN_08143210`).
fn split_panel_language(rom: &[u8], g: &Globals, o: &mut [Object]) {
    let shift = rom[DIGIT_SHIFT + g.language as usize] as i16;
    o[14].frame = g.language as i16;
    o[17].dx = shift;
    match g.language {
        3 => o[18].frame = 3,
        0..=2 | 4 => o[18].frame = 0,
        _ => {}
    }
    o[18].dx = shift;
}

/// `hud_timer` (`FUN_081428c0`): mm:ss.cc into objects 5..10; blinks in the last ten seconds before the
/// 59:59.98 limit, where it also sets the race state.
fn timer(g: &mut Globals, o: &mut [Object]) {
    let cs = div(g.frames.wrapping_mul(100), 60);
    if cs > 0x5_7E3E && g.race_state != 8 {
        g.race_state = 8;
        g.race_state_changed = 1;
    }
    if g.hud == 0 {
        return;
    }
    let (seconds, hundredths) = divmod(cs, 100);
    let (minutes, seconds) = divmod(seconds, 60);
    for (k, v) in [5, 7, 9].into_iter().zip([minutes, seconds, hundredths]) {
        let (tens, ones) = divmod(v, 10);
        o[k].frame = tens as i16;
        o[k + 1].frame = ones as i16;
    }
    if cs > 0x5_7A56 {
        for p in &mut o[5..11] {
            show(p, (g.frames >> 3) & 1 != 0);
        }
    }
}

/// `hud_split` (`FUN_081429c4`): sign (object 21: 0 when leading) and mm:ss.cc in objects 22..27, all shifted
/// by the language's digit shift. The split is converted before it is clamped to 0.
fn split(rom: &[u8], g: &mut Globals, d: &Driver, o: &mut [Object]) {
    let cs = div(g.split.wrapping_mul(100), 60);
    let shift = rom[DIGIT_SHIFT + g.language as usize] as i16;
    if g.split < 0 {
        g.split = 0;
    }
    let (seconds, hundredths) = divmod(cs, 100);
    let (minutes, seconds) = divmod(seconds, 60);
    o[21].dx = shift;
    o[21].frame = if d.position == 1 { 0 } else { 1 };
    for (k, v) in [22, 24, 26].into_iter().zip([minutes, seconds, hundredths]) {
        let (tens, ones) = divmod(v, 10);
        o[k].frame = tens as i16;
        o[k].dx = shift;
        o[k + 1].frame = ones as i16;
        o[k + 1].dx = shift;
    }
}

/// `hud_position` (`FUN_0814306c`): position / racers, remembering the previous frames in `loaded`.
fn position(g: &Globals, d: &Driver, o: &mut [Object]) {
    o[19].loaded = o[19].frame;
    o[19].frame = d.position as i16;
    o[20].loaded = o[20].frame;
    o[20].frame = g.opponents.wrapping_add(1) as i16;
}

/// `hud_lap` (`FUN_08142c4c`): lap / laps (1 / 1 in sprints), clamped with an unsigned compare.
fn lap(rom: &[u8], g: &Globals, d: &Driver, o: &mut [Object]) {
    let shift = rom[DIGIT_SHIFT + g.language as usize] as i16;
    let lap = g
        .laps
        .wrapping_sub(d.laps_left as i32 as u32)
        .wrapping_add(1)
        .min(g.laps);
    if g.mode == 3 {
        o[11].frame = 1;
        o[12].frame = 1;
    } else {
        o[11].frame = lap as i16;
        o[12].frame = g.laps as i16;
    }
    o[11].dx = shift;
    o[12].dx = shift;
    o[13].dx = shift;
}

/// `hud_needle` (`FUN_08142bd8`): angle = |revs|·(0x1C00 or 0x2000) / rev scale − 0x1770, never 0.
fn needle(g: &Globals, d: &Driver, o: &mut [Object]) {
    let p = &mut o[37];
    p.dx = 0;
    p.dy = 0;
    let revs = d.revs.wrapping_abs();
    let n = if g.needle_scale != 0 {
        revs << 13
    } else {
        revs.wrapping_mul(7) << 10
    };
    let angle = divmod(n, d.rev_scale).0.wrapping_sub(0x1770);
    p.angle = if angle == 0 { 1 } else { angle as u16 };
}

/// `hud_speed` (`FUN_08142724`): speed / 0x163C (km/h), × 256 / 411 in mph; three digits in objects 34..36.
fn speed(g: &Globals, d: &Driver, o: &mut [Object]) {
    let mut v = div(d.speed, 0x163C);
    if g.units == 0 {
        v = divmod(v << 8, 0x19B).0;
    }
    let (hundreds, rest) = divmod(v, 100);
    let (tens, ones) = divmod(rest, 10);
    for (k, digit) in [(34, hundreds), (35, tens), (36, ones)] {
        o[k].frame = digit as i16;
        o[k].angle = 0;
    }
}

/// `hud_gear` (`FUN_0814308c`).
fn gear(d: &Driver, o: &mut [Object]) {
    o[33].frame = d.gear as i16;
}

/// `hud_minimap` (`FUN_08142440`): a 64×64 window of the city map around racer 0, rotated by the minimap
/// object's affine angle, and the dots of racers 0..3 (objects 42..39) turned into the window's frame.
fn minimap(rom: &[u8], g: &Globals, racers: &[Racer; 4], o: &mut [Object]) -> Vec<u8> {
    let r0 = &racers[0];
    let angle = r0.heading.wrapping_neg() >> 8;
    o[38].frame = 0;
    o[38].angle = angle as u16;
    let (cx, cz) = (div(r0.x >> 8, 499), div(r0.z.wrapping_neg() >> 8, 499));
    let (mut x, mut y) = (cx + 0x86, cz + 0x79);
    o[38].dx = -2;
    o[38].dy = -2;
    // The clamped-off part shifts the dots. A negative y is stored into the x offset (a game bug, kept).
    let (mut off_x, mut off_y) = (0, 0);
    if x < 0 {
        off_x = x;
        x = 0;
    } else if x > 0x1C0 {
        off_x = 0x1C0 - x;
        x = 0x1C0;
    }
    if y < 0 {
        off_x = y;
        y = 0;
    } else if y > 0x1C0 {
        off_y = 0x1C0 - y;
        y = 0x1C0;
    }
    let map = ui::HUD_TEXELS + u32_at(rom, ui::HUD_MATERIALS + 0x24 * MAP_MATERIAL + 8) as usize;
    let tiles = minimap_window(
        rom,
        map + ((y as usize) << 8) + ((x as usize & !7) >> 1),
        (x as usize >> 1) & 3,
    );
    let a = angle.wrapping_neg() as u32;
    for (i, r) in racers.iter().enumerate() {
        let p = &mut o[42 - i];
        let eliminated = g.mode == 1 && r.driver.is_none_or(|d| d.flags & 8 != 0);
        if i as u32 > g.ai_cars || eliminated {
            p.dx = -16;
            continue;
        }
        let dx = div(r.x >> 8, 499) - cx + off_x;
        let dy = div(r.z.wrapping_neg() >> 8, 499) - cz + off_y;
        let (c, s) = (ui::cos(rom, a), ui::sin(rom, a));
        let px = (dx.wrapping_mul(c) >> 14) + (dy.wrapping_mul(s) >> 14) + 0x1C;
        let py = (dx.wrapping_mul(s.wrapping_neg()) >> 14) + (dy.wrapping_mul(c) >> 14) + 0x1C;
        // No lower bound on x (kept).
        if px <= 0x37 && py > 0 && py <= 0x37 {
            p.dx = px as i16;
            p.dy = py as i16;
        } else {
            p.dx = -100;
            p.dy = 100;
        }
        if i as u32 > g.opponents {
            p.frame = 4;
        }
    }
    tiles
}

/// The minimap copy (ARM, ROM `0x08169AAC`, run from IWRAM `0x03004B98` through `iwram_call_4`): 64 rows of
/// the 512-pixel-wide 4bpp map from `src`, each row shifted left by `shift` bytes, into 8×8 OBJ tiles in 1-D
/// order (8 tiles across, 8 down). It reads a ninth word per row and never clamps, so rows past the map's 384
/// read whatever ROM follows.
pub fn minimap_window(rom: &[u8], src: usize, shift: usize) -> Vec<u8> {
    let mask = u32_at(rom, MINIMAP_MASKS + 4 * shift);
    let mut out = vec![0; 64 * 32];
    for row in 0..64 {
        let w: Vec<u32> = (0..9).map(|k| u32_at(rom, src + 256 * row + 4 * k)).collect();
        for tx in 0..8 {
            // `and r3, r6, r1, lsl r7`: a register shift by 32 (shift 0) gives 0.
            let next = w[tx + 1].checked_shl(32 - 8 * shift as u32).unwrap_or(0);
            let v = (mask & next) | (w[tx] >> (8 * shift));
            let at = ((row / 8) * 8 + tx) * 32 + (row % 8) * 4;
            out[at..at + 4].copy_from_slice(&v.to_le_bytes());
        }
    }
    out
}

/// `hud_portrait` (`FUN_08142b44`): the wingman's portrait (blinking with `portrait_blink`) and its 8-step
/// bar in the next object; both hidden without a wingman.
fn portrait(g: &Globals, o: &mut [Object], k: usize) {
    if g.wingman == 0 {
        o[k].frame = 0;
        o[k].flags &= 0xFFFE;
        o[k + 1].flags &= 0xFFFE;
        return;
    }
    o[k].frame = g.portrait as i16;
    show(&mut o[k], !(g.portrait_blink != 0 && (g.frames & 0x3F) > 0x2C));
    let bar = div(g.bar << 3, g.bar_max).min(7);
    o[k + 1].frame = bar.max(0) as i16;
}

/// `hud_dial` (`FUN_08142ca8`): the dial value in 9 steps (0..8) over objects 29..31.
fn dial(d: &Driver, o: &mut [Object]) {
    let v = div(d.dial.wrapping_mul(9), 0x5_0000).clamp(0, 8);
    o[29].frame = match v {
        8 => 2,
        7 => 1,
        _ => 0,
    };
    o[30].frame = v.min(3) as i16;
    o[31].frame = (v - 2).clamp(0, 5) as i16;
}

/// `FUN_08142674` (hunter mode): two bar objects per racer from object 43: life·28 / 2^19 (0..28) split as
/// 12 + rest. Racers past the AI count are hidden (with a wingman, from the AI count on).
fn hunter_bars(g: &Globals, racers: &[Racer; 4], o: &mut [Object]) {
    for (i, r) in racers.iter().enumerate() {
        let k = 43 + 2 * i;
        let hidden = if g.wingman != 0 {
            i as u32 >= g.ai_cars
        } else {
            i as u32 > g.ai_cars
        };
        if hidden {
            o[k].flags &= 0xFFFE;
            o[k + 1].flags &= 0xFFFE;
            continue;
        }
        // NOT 1:1: a racer without a driver would make the game read BIOS memory at 0x4E8; none has one.
        let life = r.driver.map_or(0, |d| d.hunter_life);
        let v = (life.wrapping_mul(28) / (1 << 19)).clamp(0, 28);
        if v <= 12 {
            o[k].frame = v as i16;
            o[k + 1].frame = 0;
        } else {
            o[k].frame = 12;
            o[k + 1].frame = (v - 12) as i16;
        }
    }
}

/// `hud_arrow` (`FUN_081431a8`): frames 0..2 (3..5 when `arrow` < 0), stepping every 16 frames.
fn arrow(g: &Globals, o: &mut [Object], k: usize) {
    if g.arrow == 0 {
        o[k].frame = 0;
        o[k].flags &= 0xFFFE;
        return;
    }
    o[k].flags |= 1;
    let f = (g.frames >> 4).wrapping_rem(3);
    o[k].frame = if g.arrow < 0 { f + 3 } else { f } as i16;
}

/// `hud_message_tick` (`FUN_08142dec`): each running slot shows its element (with its frame) while bit 2 of
/// the timer is set, hides it otherwise, and counts down.
pub fn message_tick(objects: &mut [Object], messages: &mut Messages) {
    for s in messages.iter_mut().filter(|s| s[2] != 0) {
        if s[0] != 0xFF {
            let o = &mut objects[s[0] as usize];
            if (s[2] >> 2) & 1 != 0 {
                o.flags |= 1;
                o.frame = s[1] as i16;
            } else {
                o.flags &= 0xFFFE;
            }
        }
        s[2] -= 1;
    }
}

/// `hud_message_show` (`FUN_08142ec0`): starts message `msg` of the race mode's table for `time` frames
/// (at most 0xF4, stored `| 6`), unless its slot is busy and `force` is not set.
pub fn message_show(rom: &[u8], g: &Globals, messages: &mut Messages, msg: u32, time: i32, force: bool) {
    let Some(table) = message_table(g.mode).filter(|_| msg <= 15) else {
        return;
    };
    let e = table + 4 * msg as usize;
    if rom[e] == 0xFF {
        return;
    }
    let s = &mut messages[rom[e] as usize];
    if !force && s[2] != 0 {
        return;
    }
    s[0] = rom[e + 1];
    s[1] = rom[e + 2];
    s[2] = (time.min(0xF4) | 6) as u8;
}

/// `FUN_08142e44`: stops message `msg` and hides its element if the slot holds one.
pub fn message_cancel(rom: &[u8], g: &Globals, objects: &mut [Object], messages: &mut Messages, msg: u32) {
    let Some(table) = message_table(g.mode).filter(|_| msg <= 15) else {
        return;
    };
    let e = table + 4 * msg as usize;
    if rom[e] == 0xFF {
        return;
    }
    let s = &mut messages[rom[e] as usize];
    s[2] = 0;
    if s[0] != 0xFF {
        objects[rom[e + 1] as usize].flags &= 0xFFFE;
    }
}

/// `hud_message_hide` (`FUN_08142d64`): stops every slot and resets the elements of the mode's table.
pub fn message_hide(rom: &[u8], g: &Globals, objects: &mut [Object], messages: &mut Messages) {
    for s in messages.iter_mut() {
        s[2] = 0;
    }
    let Some(table) = message_table(g.mode) else {
        return;
    };
    for e in (0..16).map(|i| table + 4 * i).filter(|&e| rom[e] != 0xFF) {
        let o = &mut objects[rom[e + 1] as usize];
        o.flags &= 0xFFFE;
        o.dx = 0;
        o.dy = 0;
        o.angle = 0;
        o.scale = [0, 0];
        o.frame = 0;
    }
}

/// `map_world_to_screen` (`FUN_08143144`, used by the map screens, not the HUD): map `i`'s pixel of a world
/// position, `((x >> 8) << 6) / scale + x0` and the same for −z, with scales at `0x7F4480` and origins at
/// `0x7F44A8`.
pub fn map_world_to_screen(rom: &[u8], i: usize, x: i32, z: i32) -> (i32, i32) {
    let scale = u32_at(rom, 0x7F_4480 + 4 * i) as i32;
    let origin = |k: usize| u32_at(rom, 0x7F_44A8 + 8 * i + 4 * k) as i32;
    (
        div((x >> 8) << 6, scale).wrapping_add(origin(0)),
        div((z.wrapping_neg() >> 8) << 6, scale).wrapping_add(origin(1)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{Oam, sprite_bank, update_sprites};
    use crate::{LEVEL_TABLE, canonical_rom, data_dir};

    fn rom() -> Option<Vec<u8>> {
        std::env::set_current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")).unwrap();
        canonical_rom()
            .map_err(|e| eprintln!("skipping: no ROM vault ({e})"))
            .ok()
    }

    fn le16(b: &[u8], o: usize) -> u16 {
        u16::from_le_bytes([b[o], b[o + 1]])
    }

    fn le32(b: &[u8], o: usize) -> u32 {
        u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
    }

    /// One record of `tools/ui_hud_trace.lua`.
    struct Snap<'a> {
        tag: u32,
        frame: u32,
        iwram_lo: &'a [u8],
        iwram_hi: &'a [u8],
        objects: Vec<Object>,
        racers: [Racer; 4],
        needle_scale: u8,
        vram: &'a [u8],
        palette: Vec<u16>,
        dispcnt: u16,
    }

    const RECORD: usize = 8 + 0x200 + 0x1600 + 0x370 + 4 * 0xA4 + 4 * (4 + 0x500) + 4 + 0x4000 + 0x200 + 2;

    impl<'a> Snap<'a> {
        fn parse(b: &'a [u8]) -> Self {
            let mut at = 8;
            let mut take = |n: usize| {
                at += n;
                &b[at - n..at]
            };
            let (iwram_lo, iwram_hi) = (take(0x200), take(0x1600));
            let objects = take(0x370).chunks(16).map(Object::from_bytes).collect();
            let entities = take(4 * 0xA4);
            let racers = std::array::from_fn(|i| {
                let d = take(4 + 0x500);
                let e = &entities[0xA4 * i..];
                Racer {
                    x: le32(e, 0x0C) as i32,
                    z: le32(e, 0x14) as i32,
                    heading: le32(e, 0x2C) as i32,
                    driver: (le32(d, 0) != 0).then(|| {
                        let d = &d[4..];
                        Driver {
                            revs: le32(d, 0x3C) as i32,
                            gear: le32(d, 0x40) as i32,
                            speed: le32(d, 0x44) as i32,
                            position: le32(d, 0xA8) as i32,
                            laps_left: d[0xC5] as i8,
                            rev_scale: le32(d, 0x454) as i32,
                            dial: le32(d, 0x4C8) as i32,
                            flags: le16(d, 0x4D8),
                            hunter_life: le32(d, 0x4E8) as i32,
                        }
                    }),
                }
            });
            let needle_scale = take(4)[0];
            let vram = take(0x4000);
            let palette = take(0x200).chunks(2).map(|c| le16(c, 0)).collect();
            let dispcnt = le16(take(2), 0);
            Snap {
                tag: le32(b, 0),
                frame: le32(b, 4),
                iwram_lo,
                iwram_hi,
                objects,
                racers,
                needle_scale,
                vram,
                palette,
                dispcnt,
            }
        }

        fn iw(&self, addr: u32, n: usize) -> &[u8] {
            match addr {
                0x0300_0000..0x0300_0200 => &self.iwram_lo[(addr - 0x0300_0000) as usize..][..n],
                _ => &self.iwram_hi[(addr - 0x0300_5300) as usize..][..n],
            }
        }

        fn iw32(&self, addr: u32) -> u32 {
            le32(self.iw(addr, 4), 0)
        }

        fn globals(&self) -> Globals {
            Globals {
                hud: self.iw32(0x0300_5698),
                mode: self.iw32(0x0300_56E0) as i32,
                language: self.iw32(0x0300_5600),
                units: self.iw32(0x0300_0040),
                frames: self.iw32(0x0300_5800) as i32,
                split: self.iw32(0x0300_615C) as i32,
                opponents: self.iw32(0x0300_5784),
                ai_cars: self.iw32(0x0300_57EC),
                laps: self.iw32(0x0300_56E4),
                wingman: self.iw32(0x0300_6104),
                portrait: self.iw32(0x0300_61DC) as i32,
                portrait_blink: self.iw32(0x0300_61D4),
                bar: self.iw32(0x0300_61E4) as i32,
                bar_max: self.iw32(0x0300_6188) as i32,
                arrow: self.iw32(0x0300_601C) as i32,
                route: self.iw32(0x0300_5388),
                needle_scale: self.needle_scale,
                player: self.iw32(0x0300_57F8) as usize,
                race_state: self.iw32(0x0300_0048),
                race_state_changed: self.iw32(0x0300_00AC),
            }
        }

        fn messages(&self) -> Messages {
            let b = self.iw(0x0300_6210, 24);
            std::array::from_fn(|i| b[4 * i..4 * i + 4].try_into().unwrap())
        }

        fn oam(&self) -> Oam {
            let b = self.iw(0x0300_64F0, 0x400);
            std::array::from_fn(|i| std::array::from_fn(|j| le16(b, 8 * i + 2 * j)))
        }
    }

    /// Replays a trace: every frame, from the state before `hud_update`, the port must reproduce the state
    /// after `hud_update` + `sprite_screen_update`: objects, globals, message slots, shadow OAM, OBJ palette
    /// and OBJ VRAM (tiles 0x200..0x3FF: the minimap and every uploaded HUD frame). Returns frames checked.
    fn replay(rom: &[u8], name: &str) -> Option<usize> {
        let path = data_dir().join(format!("work/e5298b24/hud-logic/{name}.trace"));
        let trace = std::fs::read(&path)
            .map_err(|e| eprintln!("skipping: no trace {} ({e})", path.display()))
            .ok()?;
        assert_eq!(trace.len() % (2 * RECORD), 0, "{name}: truncated trace");
        let bank = sprite_bank(rom, LEVEL_TABLE);
        let mut frames = 0;
        for pair in trace.chunks(2 * RECORD) {
            let (pre, post) = (Snap::parse(&pair[..RECORD]), Snap::parse(&pair[RECORD..]));
            // A game frame can straddle a VBlank, so the two records' video frames may differ by one.
            assert_eq!((pre.tag, post.tag), (0, 1), "{name}: records out of step");
            assert_ne!(pre.dispcnt & 0x40, 0, "OBJ 1-D mapping is off");
            let at = format!("{name} frame {}", pre.frame);
            let screen = le16(pre.iw(0x0300_017C, 2), 0) as usize;
            let tile_base = pre.iw32(0x0300_64E0) as u16;

            // The race time (0x03005800) is counted by the VBlank IRQ and can tick while the HUD runs; the HUD
            // then read the old or the new value. It never writes it.
            let (pre_g, post_g) = (pre.globals(), post.globals());
            let results: Vec<String> = [pre_g.frames, post_g.frames]
                .into_iter()
                .take(if pre_g.frames == post_g.frames { 1 } else { 2 })
                .map(|t| {
                    let mut g = Globals {
                        frames: t,
                        ..pre_g.clone()
                    };
                    let (mut messages, mut objects, mut palette) =
                        (pre.messages(), pre.objects.clone(), pre.palette.clone());
                    let minimap = update(rom, &mut g, &pre.racers, &mut objects, &mut messages, &mut palette);
                    let mut oam = pre.oam();
                    let uploads = update_sprites(rom, &bank, screen, &mut objects, &mut oam, false, tile_base);
                    let mut vram = pre.vram.to_vec();
                    let mut write = |tile: usize, bytes: &[u8]| {
                        let o = 32 * tile - 0x4000;
                        vram[o..o + bytes.len()].copy_from_slice(bytes);
                    };
                    if let Some(tiles) = minimap {
                        let e = bank.elements[bank.screens[screen].first + 38];
                        write(e.tile.wrapping_add(tile_base) as usize, &tiles);
                    }
                    for u in &uploads {
                        write(u.tile, &rom[u.src..u.src + u.len]);
                    }
                    g.frames = post_g.frames;
                    let mut diff = Vec::new();
                    if g != post_g {
                        diff.push(format!("globals {g:?}"));
                    }
                    if messages != post.messages() {
                        diff.push(format!("message slots {messages:?} != {:?}", post.messages()));
                    }
                    for (k, (got, want)) in objects
                        .iter()
                        .zip(&post.objects)
                        .enumerate()
                        .filter(|(_, (a, b))| a != b)
                    {
                        diff.push(format!("object {k}: {got:?} != {want:?}"));
                    }
                    if let Some(i) = (0..128).find(|&i| oam[i] != post.oam()[i]) {
                        diff.push(format!("OAM entry {i}: {:x?} != {:x?}", oam[i], post.oam()[i]));
                    }
                    if palette != post.palette {
                        diff.push("OBJ palette".into());
                    }
                    if let Some(i) = (0..vram.len()).find(|&i| vram[i] != post.vram[i]) {
                        diff.push(format!("OBJ VRAM at tile {:#x} (+{})", 0x200 + i / 32, i % 32));
                    }
                    diff.join("; ")
                })
                .collect();
            assert!(results.iter().any(String::is_empty), "{at}: {results:#?}");
            frames += 1;
        }
        eprintln!("{name}: {frames} frames replayed exactly");
        Some(frames)
    }

    #[test]
    fn divmod_matches_the_iwram_routine() {
        assert_eq!(divmod(12345, 100), (123, 45));
        // Negative dividend: the quotient is negated, the remainder uses |q| (the routine's quirk).
        assert_eq!(divmod(-17, 10), (-1, -27));
        assert_eq!(divmod(-7, 10), (0, -7));
    }

    #[test]
    fn hud_replays_a_circuit_race() {
        let Some(rom) = rom() else { return };
        if let Some(n) = replay(&rom, "circuit") {
            assert!(n >= 600, "{n} frames");
        }
    }
}
