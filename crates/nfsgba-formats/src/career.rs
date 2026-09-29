//! Career mode, race setup, race rules and the EEPROM save (`docs/formats/career.md`).
//!
//! Offsets are ROM offsets. RAM addresses in comments are the game's (IWRAM `0x03…`, EWRAM `0x02…`); the
//! profile struct is `*0x030056EC` (`0x02000808` in the reference run).

use super::{div, i16_at, ptr, u16_at, u32_at};
pub use nfsgba_fixed::{isqrt, rand_table};
use std::io;

/// 66 event records of 8 bytes: zones 1–5 have 12 events each, zone 6 has 6 (`FUN_0812da08`).
pub const EVENT_TABLE: usize = 0x7E_4744;
pub const EVENT_COUNT: usize = 66;
/// Per zone, the two boss events (`u16` event index; zone 6 has none and holds 66, 66).
pub const BOSS_EVENTS: usize = 0x7E_4714;
/// 42 `(u16 text key, u16 route number)` pairs: circuits forward (0–11), reverse (12–23), sprints (24–41).
pub const TRACK_NAMES: usize = 0x7E_4A70;
/// `u32` per route number 0..=42: the slot in [`TRACK_NAMES`] (`FUN_0812b5f0`).
pub const ROUTE_TRACK_SLOT: usize = 0x7E_49C4;
/// 44 records of 0xC bytes per route number: environment and route-table index (`FUN_0812b5f0`).
pub const RACE_SLOTS: usize = 0x7F_2588;
/// Route table (`docs/formats/race-routes.md`): 44 records of 0x14 bytes.
pub const ROUTE_TABLE: usize = 0x7F_2798;
pub const ROUTE_COUNT: usize = 44;
/// Wingman name keys (14 × `u16`) and roles (13 × `(u16 role text key, u16 level)`).
pub const WINGMAN_NAMES: usize = 0x7E_4974;
pub const WINGMAN_ROLES: usize = 0x7E_4990;
pub const WINGMAN_COUNT: usize = 13;
/// Unlocks by career progress: `u16 key, u16 unlock ids…, 0xFFFF`, repeated; a key of `0xFFFF` ends it.
pub const PROGRESS_UNLOCKS: usize = 0x7E_4B18;
/// Six setup screens (0x10-byte headers): options, race info, then the circuit/elimination/hunter/sprint setups.
pub const SETUP_SCREENS: usize = 0x7E_6260;
/// Race mode names (`u16` text keys), indexed by [`RaceMode`].
pub const MODE_NAMES: usize = 0x7E_5070;
/// Per car, the stock value that the style rating's fourth field is measured against (`FUN_0812c30c`).
pub const STYLE_BASE: usize = 0x7F_0626;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RaceMode {
    Circuit,
    Elimination,
    Hunter,
    Sprint,
}

impl RaceMode {
    /// The value of `0x030056E0`.
    pub fn from_index(i: u8) -> Option<Self> {
        [Self::Circuit, Self::Elimination, Self::Hunter, Self::Sprint]
            .get(i as usize)
            .copied()
    }
}

/// One career event (`FUN_0812da08` copies it into the race globals).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Event {
    pub zone: u8,
    pub slot: u8,
    /// `+0`: AI skill 0..100 (`0x030000BC`); `skill / 35` is the difficulty 0..2 (`0x03005608`).
    pub skill: u8,
    /// `+1`: slot in [`TRACK_NAMES`] of the forward track (`reverse` adds 12).
    pub track: u8,
    /// `+2` (`0x030056E0`).
    pub mode: RaceMode,
    /// `+3` (`0x03005610`).
    pub reverse: bool,
    /// `+4` (`0x030056E4`).
    pub laps: u8,
    /// `+5`: 0 none, 1 light, 2 heavy (`0x03005604`).
    pub traffic: u8,
    /// `+6`: cash for the next win, see [`race_payout`].
    pub reward: i16,
}

impl Event {
    pub fn difficulty(&self) -> u8 {
        self.skill / 35
    }
    pub fn track_slot(&self) -> usize {
        self.track as usize + if self.reverse { 12 } else { 0 }
    }
}

pub fn events(rom: &[u8]) -> Vec<Event> {
    (0..EVENT_COUNT)
        .map(|i| {
            let r = &rom[EVENT_TABLE + 8 * i..];
            Event {
                zone: (i / 12) as u8,
                slot: (i % 12) as u8,
                skill: r[0],
                track: r[1],
                mode: RaceMode::from_index(r[2]).expect("event mode"),
                reverse: r[3] != 0,
                laps: r[4],
                traffic: r[5],
                reward: i16_at(r, 6),
            }
        })
        .collect()
}

/// `[zone][0..2]`: the event indices that are boss races.
pub fn boss_events(rom: &[u8]) -> [[u16; 2]; 6] {
    std::array::from_fn(|z| [0, 1].map(|k| u16_at(rom, BOSS_EVENTS + 4 * z + 2 * k)))
}

/// `(text key, route number)` of a [`TRACK_NAMES`] slot.
pub fn track_name(rom: &[u8], slot: usize) -> (usize, usize) {
    (
        u16_at(rom, TRACK_NAMES + 4 * slot) as usize,
        u16_at(rom, TRACK_NAMES + 4 * slot + 2) as usize,
    )
}

/// The [`TRACK_NAMES`] slot of route number `route` (0..=42).
pub fn route_track_slot(rom: &[u8], route: usize) -> usize {
    u32_at(rom, ROUTE_TRACK_SLOT + 4 * route) as usize
}

/// What a route number loads: `+0` environment (level descriptor, `0x0300006C`), `+1` route-table index
/// (`0x03005720`). The other 10 bytes are not decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RaceSlot {
    pub environment: u8,
    pub route: u8,
    pub rest: [u8; 10],
}

pub fn race_slots(rom: &[u8]) -> Vec<RaceSlot> {
    (0..ROUTE_COUNT)
        .map(|i| {
            let r = &rom[RACE_SLOTS + 0xC * i..];
            RaceSlot {
                environment: r[0],
                route: r[1],
                rest: r[2..12].try_into().unwrap(),
            }
        })
        .collect()
}

/// A racing-line section: section 0 is the lap (its last waypoint repeats the first), the others are
/// branches. The table sits at route `+0x04`, right before the waypoints (`+0x08`), 8 bytes per section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Section {
    pub count: u16,
    /// `+2`, always 0.
    pub flags: u16,
    /// Index of the first waypoint in the route's waypoint array.
    pub first: u32,
}

/// Per route-table record, its sections (empty when the record has no racing line).
pub fn route_sections(rom: &[u8]) -> Vec<Vec<Section>> {
    (0..ROUTE_COUNT)
        .map(|i| ROUTE_TABLE + 0x14 * i)
        .map(|r| {
            if u32_at(rom, r + 8) == 0 {
                return Vec::new();
            }
            let (table, line) = (ptr(rom, r + 4), ptr(rom, r + 8));
            (0..(line - table) / 8)
                .map(|k| table + 8 * k)
                .map(|s| Section {
                    count: u16_at(rom, s),
                    flags: u16_at(rom, s + 2),
                    first: u32_at(rom, s + 4),
                })
                .collect()
        })
        .collect()
}

/// A racing-line waypoint as the race code reads it (24 bytes at world `+0x44`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinePoint {
    pub x: i32,
    pub z: i32,
    /// `+0x0C`/`+0x0E`: the same point in another section; `0xFFFF` = none.
    pub link_section: u16,
    pub link_index: u16,
    pub distance: i32,
}

/// Plane-table row (`*0x03005FB4`, 0x20 bytes per waypoint, `FUN_08138c24`): `[0..2]` unit direction to the next
/// waypoint (×256), `[2]`/`[3]` two slopes (×0x1000), `[4..6]` the unit normal of the crossing line through the
/// waypoint (×256), `[6]` its offset (`normal · waypoint`), `[7]` the distance to the next waypoint.
pub type Plane = [i32; 8];

/// Back table (`*0x03005FB8`, 256 `i32` per section, −1 = none): the lap index where a branch leaves.
pub type BackTable = [i32; 256];

/// Per route, a pointer to its branch count (`*0x03006108`; a null pointer means 0), read by `race_load_level`.
pub const ROUTE_BRANCHES: usize = 0x7F_37D8;

/// A route's racing line as the race uses it: world `+0x40` (sections) and `+0x44` (waypoints).
#[derive(Debug, Clone)]
pub struct RacingLine {
    pub sections: Vec<Section>,
    /// The whole 0x1800-byte copy (256 records; those past the route are whatever ROM data follows).
    pub points: Vec<LinePoint>,
    /// Per section, the branch's distance scale onto the lap, ×256 (`0x03006120`; 0 for sections never forked into).
    pub scales: Vec<i32>,
}

impl RacingLine {
    /// The line `race_load_level` builds for route-table record `route` (`None` without one): the ROM copy, two
    /// points longer in sprints (`FUN_081390b0`), with the branch links rebuilt (`FUN_081391f4`).
    pub fn new(rom: &[u8], route: usize, sprint: bool) -> Option<RacingLine> {
        let mut sections = route_sections(rom).swap_remove(route);
        if sections.is_empty() {
            return None;
        }
        let line = ptr(rom, ROUTE_TABLE + 0x14 * route + 8);
        let points = (0..0x100)
            .map(|k| line + 0x18 * k)
            .map(|w| LinePoint {
                x: u32_at(rom, w) as i32,
                z: u32_at(rom, w + 4) as i32,
                link_section: u16_at(rom, w + 0xC),
                link_index: u16_at(rom, w + 0xE),
                distance: u32_at(rom, w + 0x10) as i32,
            })
            .collect();
        let branches = match u32_at(rom, ROUTE_BRANCHES + 4 * route) {
            0 => 0,
            p => u32_at(rom, (p - super::ROM_BASE) as usize) as usize,
        };
        sections.truncate(branches + 1);
        let mut this = RacingLine {
            sections,
            points,
            scales: Vec::new(),
        };
        if sprint {
            this.make_sprint();
        }
        this.rebuild_links();
        this.measure(!sprint);
        Some(this)
    }

    /// `FUN_0813f744` (the player's driver setup, after the planes are built): the lap's distances become running
    /// sums of integer lengths (the first point gets `isqrt(0)` = 1; in sprints point 1 restarts at 0). Each branch
    /// that a lap point forks into (link index 0) is measured the same way from 0 and then scaled onto the lap
    /// between the fork and where the branch rejoins; quirk kept: its first point scales its old distance. The scale
    /// (×256) goes to `0x03006120 + 4 · section`.
    fn measure(&mut self, lapped: bool) {
        let len = |a: LinePoint, b: LinePoint| {
            let (dx, dz) = (b.x.wrapping_sub(a.x), b.z.wrapping_sub(a.z));
            isqrt(dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz)) as u32)
        };
        self.scales = vec![0; self.sections.len()];
        self.scales[0] = 0x100;
        let count = i32::from(self.sections[0].count);
        let (mut prev, mut dist) = (self.point(0, 0), 0i32);
        for k in 0..count {
            let i = self.index(0, k);
            dist = dist.wrapping_add(len(prev, self.points[i]));
            if k == 1 && !lapped {
                dist = 0;
            }
            (prev, self.points[i].distance) = (self.points[i], dist);
        }
        for k in 0..count {
            let fork = self.point(0, k);
            if fork.link_index != 0 {
                continue;
            }
            let s = usize::from(fork.link_section);
            let n = i32::from(self.sections[s].count);
            let rejoin = self.point(0, self.point(s, n - 1).link_index.into());
            let span = rejoin.distance.wrapping_sub(fork.distance) >> 8;
            let (mut prev, mut d) = (self.point(s, 0), 0i32);
            for i in 1..n {
                let j = self.index(s, i);
                d = d.wrapping_add(len(prev, self.points[j]));
                (prev, self.points[j].distance) = (self.points[j], d);
            }
            let branch = self.point(s, n - 1).distance >> 8;
            for i in 0..n {
                let j = self.index(s, i);
                self.points[j].distance = div(span.wrapping_mul(self.points[j].distance), branch) + fork.distance;
            }
            self.scales[s] = div(span << 8, branch);
        }
    }

    /// `FUN_081390b0` (sprints): every point moves up one slot, and the lap gets a point before its start and one
    /// after its end, extrapolated as `5·p − 4·neighbour`. Links past the lap move up one index.
    fn make_sprint(&mut self) {
        let p = &mut self.points;
        let count = usize::from(self.sections[0].count);
        p.copy_within(0..254, 2);
        p.copy_within(2..count + 2, 1);
        let n = count + 2;
        (p[0].x, p[0].z) = (p[0].x * 5 - p[2].x * 4, p[0].z * 5 - p[2].z * 4);
        (p[n - 1].x, p[n - 1].z) = (p[n - 1].x * 5 - p[n - 3].x * 4, p[n - 1].z * 5 - p[n - 3].z * 4);
        for q in &mut p[n..0xFF] {
            q.link_index = q.link_index.saturating_add(1); // 0xFFFF (no link) stays
        }
        self.sections[0].count += 2;
        for s in &mut self.sections[1..] {
            s.first += 2;
        }
    }

    /// `FUN_081391f4`: clears every link of the route's points, then links each branch's start and end with the
    /// nearest point (`(Δ >> 4)²`, first found on ties) of the other sections, excluding their last points.
    fn rebuild_links(&mut self) {
        let total: usize = self.sections.iter().map(|s| usize::from(s.count)).sum();
        for q in &mut self.points[..total] {
            (q.link_section, q.link_index) = (0xFFFF, 0xFFFF);
        }
        for b in 1..self.sections.len() {
            let count = i32::from(self.sections[b].count);
            for (end, back_index) in [(0, 0), (count - 1, count - 1)] {
                let at = self.point(b, end);
                let mut best = (0x7FF_FFFF, 0, 0);
                for s in (0..self.sections.len()).filter(|&s| s != b) {
                    for i in 0..i32::from(self.sections[s].count) - 1 {
                        let q = self.point(s, i);
                        let (dx, dz) = ((q.x - at.x) >> 4, (q.z - at.z) >> 4);
                        let d = dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz));
                        if d < best.0 {
                            best = (d, s, i);
                        }
                    }
                }
                let (_, s, i) = best;
                let here = self.index(b, end);
                (self.points[here].link_section, self.points[here].link_index) = (s as u16, i as u16);
                let there = self.index(s, i);
                (self.points[there].link_section, self.points[there].link_index) = (b as u16, back_index as u16);
            }
        }
    }

    fn index(&self, section: usize, index: i32) -> usize {
        (self.sections[section].first as i32 + index) as usize
    }

    /// `FUN_0814007c`: point `index` of `section` (the game does no range check).
    pub fn point(&self, section: usize, index: i32) -> LinePoint {
        self.points[self.index(section, index)]
    }

    /// Lap length: the distance of the lap's last waypoint (`race_progress`).
    pub fn lap_length(&self) -> i32 {
        self.point(0, i32::from(self.sections[0].count) - 1).distance
    }

    /// `racing_line_step` (`FUN_0813e860`): index `index` of `section`, following links past either end. The lap
    /// wraps in lapped races (period `count − 1`) and clamps otherwise; a branch clamps at an unlinked end. Quirk
    /// kept: stepping back from an unlinked branch start stays in the branch, at `back[section] + index`.
    pub fn step(&self, lapped: bool, back: &BackTable, mut section: usize, mut index: i32) -> (usize, i32) {
        loop {
            let count = i32::from(self.sections[section].count);
            if index > count - 1 {
                if section == 0 {
                    return (0, if lapped { index + 1 - count } else { count - 1 });
                }
                let end = self.point(section, count - 1);
                if end.link_index == 0xFFFF {
                    return (section, count - 1);
                }
                (section, index) = (end.link_section.into(), i32::from(end.link_index) + index - count + 1);
            } else if index < 0 {
                if section == 0 {
                    return (0, if lapped { index - 1 + count } else { 0 });
                }
                let start = self.point(section, 0);
                if start.link_index == 0xFFFF {
                    index += back[section];
                } else {
                    (section, index) = (start.link_section.into(), i32::from(start.link_index) + index);
                }
            } else {
                return (section, index);
            }
        }
    }

    fn stepped(&self, lapped: bool, back: &BackTable, section: usize, index: i32) -> LinePoint {
        let (s, i) = self.step(lapped, back, section, index);
        self.point(s, i)
    }

    /// The plane table and back table that `FUN_08138f30` builds at race start (lap, then each branch where it
    /// leaves the lap, recursively; `FUN_08138dc4`), with the lapped flag left from the previous scene. The game
    /// writes into a `malloc(0x2000)` (256 rows) and only the rows it builds: pass the old contents in `planes`
    /// (a branch entered only at its end is never built, and keeps them). The back table is fully reset.
    pub fn planes(&self, lapped: bool, planes: &mut [Plane]) -> BackTable {
        let mut back = [-1; 256];
        let count = i32::from(self.sections[0].count);
        let row = |this: &Self, back: &BackTable, s: usize, k: i32| {
            plane(
                this.stepped(lapped, back, s, k - 1),
                this.point(s, k),
                this.stepped(lapped, back, s, k + 1),
                this.stepped(lapped, back, s, k + 2),
            )
        };
        for k in 0..count {
            planes[k as usize] = row(self, &back, 0, k);
            let p = self.point(0, k);
            if p.link_index != 0xFFFF && p.link_section != 0 {
                self.branch_planes(lapped, planes, &mut back, p, k);
            }
        }
        planes[0] = row(self, &back, 0, 0);
        back
    }

    /// `FUN_08138dc4`: the planes of the branch that `fork` links to, when the branch starts there.
    fn branch_planes(&self, lapped: bool, planes: &mut [Plane], back: &mut BackTable, fork: LinePoint, at: i32) {
        let s = usize::from(fork.link_section);
        let count = i32::from(self.sections[s].count);
        let d2 = |p: LinePoint| {
            let (dx, dz) = ((p.x - fork.x) >> 4, (p.z - fork.z) >> 4);
            dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz))
        };
        if d2(self.point(s, 0)) >= d2(self.point(s, count - 1)) {
            return;
        }
        back[s] = at;
        for k in 0..count {
            planes[(self.sections[s].first as i32 + k) as usize] = plane(
                self.stepped(lapped, back, s, k - 1),
                self.point(s, k),
                self.stepped(lapped, back, s, k + 1),
                self.stepped(lapped, back, s, k + 2),
            );
            let p = self.point(s, k);
            if p.link_index != 0xFFFF && p.link_section != 0 && k != count - 1 && k != 0 {
                self.branch_planes(lapped, planes, back, p, k);
            }
        }
    }
}

/// `FUN_08138c24`: one plane-table row from four consecutive waypoints.
fn plane(prev: LinePoint, cur: LinePoint, next: LinePoint, next2: LinePoint) -> Plane {
    let unit = |dx: i32, dz: i32| {
        let len = isqrt(dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz)) as u32);
        (div(dx.wrapping_mul(0x100), len), div(dz.wrapping_mul(0x100), len), len)
    };
    let mut r = [0; 8];
    let (ux, uz, len) = unit(next.x - cur.x, next.z - cur.z);
    (r[0], r[1], r[7]) = (ux, uz, len);
    let (ax, az, _) = unit(next2.x - next.x, next2.z - next.z);
    let (sx, sz, _) = unit(ax + ux, az + uz);
    let den = sz * uz + sx * ux;
    r[3] = div((sz * ux - sx * uz) * 0x1000, if den == 0 { 1 } else { den });
    let (bx, bz, _) = unit(cur.x - prev.x, cur.z - prev.z);
    let (nx, nz, _) = unit(bx + ux, bz + uz);
    (r[4], r[5]) = (nx, nz);
    r[6] = nx.wrapping_mul(cur.x).wrapping_add(nz.wrapping_mul(cur.z));
    let den = nz * bz + nx * bx;
    r[2] = div((nz * bx - nx * bz) * 0x1000, if den == 0 { 1 } else { den });
    r
}

/// A selectable wingman (menu order; 0 is "none"). Unlock id `0x127 + index`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wingman {
    pub name_key: u16,
    /// `TEXT_ATTACKER`/`TEXT_DRAFTER` (`TEXT_NONE` for index 0).
    pub role_key: u16,
    /// Shown as icon `0x10B + level` (`FUN_08130d8c`).
    pub level: u16,
}

pub fn wingmen(rom: &[u8]) -> Vec<Wingman> {
    (0..WINGMAN_COUNT)
        .map(|i| Wingman {
            name_key: u16_at(rom, WINGMAN_NAMES + 2 * i),
            role_key: u16_at(rom, WINGMAN_ROLES + 4 * i),
            level: u16_at(rom, WINGMAN_ROLES + 4 * i + 2),
        })
        .collect()
}

/// Unlock ids granted when a zone's completed-event count reaches `key - zone * 12` (`FUN_08135958`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressUnlock {
    pub key: u16,
    pub ids: Vec<u16>,
}

pub fn progress_unlocks(rom: &[u8]) -> Vec<ProgressUnlock> {
    let mut at = PROGRESS_UNLOCKS;
    let mut next = || {
        at += 2;
        u16_at(rom, at - 2)
    };
    let mut out = Vec::new();
    while let key @ 0..=0xFFFE = next() {
        out.push(ProgressUnlock {
            key,
            ids: std::iter::from_fn(|| Some(next()).filter(|&v| v != 0xFFFF)).collect(),
        });
    }
    out
}

/// One line of a setup screen. The value lives in the variable named by `setting` (see `career.md`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupItem {
    pub text_key: u32,
    pub setting: u32,
    pub min: i16,
    pub max: i16,
    /// Text key per value `0..=max`.
    pub options: Vec<u32>,
    pub action: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupScreen {
    pub title_key: u16,
    pub button_key: u16,
    pub items: Vec<SetupItem>,
}

pub fn setup_screens(rom: &[u8]) -> Vec<SetupScreen> {
    (0..6)
        .map(|s| SETUP_SCREENS + 0x10 * s)
        .map(|h| SetupScreen {
            title_key: u16_at(rom, h),
            button_key: u16_at(rom, h + 2),
            items: (0..u16_at(rom, h + 10) as usize)
                .map(|i| ptr(rom, h + 12) + 0x18 * i)
                .map(|it| {
                    let max = i16_at(rom, it + 0x12);
                    SetupItem {
                        text_key: u32_at(rom, it),
                        setting: u32_at(rom, it + 8),
                        min: i16_at(rom, it + 0x10),
                        max,
                        options: (0..=max as usize)
                            .map(|v| u32_at(rom, ptr(rom, it + 0xC) + 4 * v))
                            .collect(),
                        action: u32_at(rom, it + 0x14),
                    }
                })
                .collect(),
        })
        .collect()
}

/// Style rating of the career car (`FUN_0812c30c`); `record` is its 17-byte RAM record ([`Save::cars`]).
pub fn style_rating(rom: &[u8], car: usize, record: &[u8; 17]) -> i32 {
    let r = record.map(i32::from);
    let mut v = if r[5] != 0 { 105 } else { 100 };
    if r[0] != 0 {
        v += tier(r[0], &[7, 12, 15], &[8, 10, 14, 17]);
    }
    if r[1] != 0 {
        v += tier(r[1], &[3, 5], &[8, 13, 15]);
    }
    if r[2] != 0 {
        v += tier(r[2], &[7, 12], &[4, 5, 6]);
    }
    if r[3] != 0 {
        v += (r[3] + 4 - rom[STYLE_BASE + car] as i32) * 15;
    }
    v + match r[4] >> 3 {
        1 => 4,
        2 => 7,
        3 => 10,
        4 => 12,
        _ => 0,
    }
}

/// Reward percentage for a style rating (`FUN_0812efe8`, `FUN_0812dd80`).
pub fn reward_percent(rating: i32) -> i32 {
    tier(rating, &[0x65, 0x7E, 0x97, 0xB0], &[100, 110, 120, 130, 140])
}

/// `add[k]`, where `k` counts the `limits` that `v` has reached (the games' `if v < limit … else if …` chains).
fn tier(v: i32, limits: &[i32], add: &[i32]) -> i32 {
    add[limits.iter().take_while(|&&l| v >= l).count()]
}

/// Career race result (`FUN_0812efe8`, only when `0x030000A0` is 1): `place` is 1 (won), 2 (second) or 3 (anything
/// else), from [`payout_place`]. Updates the event's status (2 bits per event: 1 won, 2 second, 3 not done) and the
/// cash, and returns the payout (also stored at profile `+0x3B8`). The reward is that of the zone's event number
/// "events done" (one fewer when this event already counts); quirk kept: with status 0 (never written by the game)
/// and nothing done that is the word before the zone's first record.
pub fn race_payout(rom: &[u8], save: &mut Save, zone: usize, slot: usize, place: u8, percent: i32) -> i32 {
    let event = zone * 12 + slot;
    let old = save.event_status(event);
    let done = (0..if zone == 5 { 6 } else { 12 }).filter(|&e| matches!(save.event_status(zone * 12 + e), 1 | 2));
    let index = (zone * 12 + done.count()) as isize - isize::from(old != 3);
    let base = i32::from(i16_at(rom, (EVENT_TABLE as isize + 8 * index + 6) as usize));
    if place < old {
        save.events[event >> 2] = save.events[event >> 2] & !(3 << ((event & 3) * 2)) | place << ((event & 3) * 2);
    }
    let mut pay = percent * base / 100;
    if place == 2 {
        pay >>= 1;
    }
    if matches!(old, 1 | 2) {
        pay >>= 1;
    }
    if place == 3 {
        pay = 0;
    }
    save.cash = save.cash.wrapping_add(pay as u32);
    pay
}

/// The career place `career_race_payout` pays for, from the result bytes at `0x03005730`: 1 when `[4]` is 0,
/// else 2 when `[5]` is 0, else 3.
pub fn payout_place(order: &[u8]) -> u8 {
    if order[4] == 0 {
        1
    } else if order[5] == 0 {
        2
    } else {
        3
    }
}

/// How `career_race_payout` ranks the results first: hunter (mode 2) by life, most first; circuit, elimination and
/// sprint by time, least first; other modes not at all. `(key, descending)` for [`rank_results`].
pub fn payout_ranking(mode: u32) -> Option<(u32, bool)> {
    match mode as i32 {
        2 => Some((4, true)),
        0 | 1 | 3 => Some((1, false)),
        _ => None,
    }
}

/// `FUN_0812e8e4(key, descending)` (rows swapped by `FUN_0812e860`) on the ranked results at `0x03005730`: per slot a
/// byte at `+0`, the entity id at `+4`, bytes at `+8` and `+0xC`, and `u32` at `+0x10` (best lap), `+0x20` (time)
/// and `+0x30` (hunter life). Key 1 compares the time (signed), 2 the `+0xC` byte, 4 the life (unsigned); other keys
/// never swap. A bubble sort over `opponents + 1` slots with `opponents + 1` passes; ties keep their order.
pub fn rank_results(t: &mut [u8; 0x40], opponents: u32, key: u32, descending: bool) {
    let value = |t: &[u8; 0x40], k: usize| match key {
        1 => i64::from(u32_at(t, 0x20 + 4 * k) as i32),
        2 => i64::from(t[0xC + k]),
        4 => i64::from(u32_at(t, 0x30 + 4 * k)),
        _ => 0,
    };
    let n = opponents as usize;
    for _ in 0..=n {
        for k in 0..n {
            let (a, b) = (value(t, k), value(t, k + 1));
            if if descending { a < b } else { b < a } {
                for (at, size) in [(0x10, 4), (0, 1), (4, 1), (0x20, 4), (0xC, 1), (0x30, 4), (8, 1)] {
                    for i in 0..size {
                        t.swap(at + size * k + i, at + size * (k + 1) + i);
                    }
                }
            }
        }
    }
}

/// Race progress used for positions (`FUN_081400ec`): laps done times the lap length plus the distance into the
/// lap. `laps_left` counts down from `laps` (driver `+0xC5`); sprints (`0x0300608C` = 0) use the distance alone.
pub fn race_progress(lapped: bool, lap_length: i32, laps: i32, laps_left: i32, distance: i32) -> i32 {
    if lapped {
        lap_length.wrapping_mul(laps - laps_left).wrapping_add(distance)
    } else {
        distance
    }
}

/// The fields of one car that the race rules read and write: its entity (world `+0x3C`, 0xA4 bytes each) and
/// its driver struct (`*(entity + 0x8C)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Racer {
    /// Entity `+0x00`.
    pub id: u16,
    /// Entity `+0x72`: racing-line section (0 = the lap).
    pub section: u16,
    /// Entity `+0x90`: segment (waypoint index) in the section.
    pub segment: i16,
    /// Entity `+0x4A`: 2 once finished or knocked out.
    pub state: u16,
    /// Entity `+0x08`.
    pub entity_flags: u16,
    /// Entity `+0x0C`/`+0x14`: position, 8.8 fixed point.
    pub x: i32,
    pub z: i32,
    /// Driver `+0xA8`, 1-based.
    pub place: i32,
    /// Driver `+0xAC`: distance into the lap.
    pub distance: i32,
    /// Driver `+0xB4`/`+0xB8`/`+0xBC`: best lap, lap start and finish time, in frames of `0x03005800`.
    pub best_lap: u32,
    pub lap_start: u32,
    pub finish: u32,
    /// Driver `+0xC5`: laps left.
    pub laps_left: i8,
    /// Driver `+0x4D8`: bit 0 set while going backwards past the start, bit 1 lap armed, bit 3 knocked out.
    pub flags: u16,
    /// Driver `+0x4E8`: hunter life, `0..=HUNTER_LIFE_MAX`.
    pub life: i32,
    /// Driver `+0x4EC`: wrong-way frames; `+0x4EE`: wall frames; `+0x4F0`: cleared by hunter hits.
    pub wrong_way: i16,
    pub wall: i16,
    pub hit: i16,
    /// Driver `+0xF8..+0x104` (the AI's; a knockout copies `0x7F3DD0` there).
    pub knockout: [u32; 3],
    /// Driver `+0x444`: the AI's side of its next crossing line.
    pub side: i32,
    /// Driver `+0x4D6`: set to 2 when the AI changes section.
    pub section_changed: u16,
}

/// The race globals the rules use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Race {
    /// `0x030056E0`: [`RaceMode`] index.
    pub mode: u32,
    /// `0x0300608C`: laps count (every mode but sprint).
    pub lapped: bool,
    /// `0x030056E4`.
    pub laps: i32,
    /// `0x03005784`: racers besides the player.
    pub opponents: u32,
    /// `0x03005800`: race frames.
    pub time: u32,
    /// `0x030061A4`: someone finished.
    pub finished: bool,
    /// `0x030057F8` (the camera's car) and `0x03000060` (the player's entity).
    pub view: u32,
    pub player: u32,
    /// `0x03005608`: 0..2.
    pub difficulty: u32,
    /// `0x03000048`.
    pub state48: u32,
    /// `0x030064C8`: [`rand_table`] index.
    pub rand: u32,
    /// `0x03005384`: the player has been going the wrong way for over 27 frames.
    pub wrong_way: bool,
    /// `0x03005650..0x03005690`: `+8` a byte per car (8 = knocked out), `+0x20` a finish time per car id.
    pub results: [u8; 0x40],
}

impl Default for Race {
    fn default() -> Self {
        Race {
            mode: 0,
            lapped: false,
            laps: 0,
            opponents: 0,
            time: 0,
            finished: false,
            view: 0,
            player: 0,
            difficulty: 0,
            state48: 0,
            rand: 0,
            wrong_way: false,
            results: [0; 0x40],
        }
    }
}

/// Per difficulty, the roll (`rand & 0xFF`) an AI must beat to take a shortcut (`0x7BFCD4`).
pub const AI_BRANCH_CHANCE: usize = 0x7B_FCD4;
/// What a knocked-out car's driver `+0xF8..` gets (`0x7F3DD0`, three words).
pub const KNOCKOUT_WORDS: usize = 0x7F_3DD0;

impl RacingLine {
    fn side(planes: &[Plane], row: i32, r: &Racer) -> i32 {
        let p = planes[row as usize];
        (r.x >> 8)
            .wrapping_mul(p[4])
            .wrapping_add((r.z >> 8).wrapping_mul(p[5]))
            .wrapping_sub(p[6])
    }

    /// The player's racing-line tracker (`FUN_0813edd8`, every frame). `vectors` are driver `+0x11C` and `+0x140`
    /// (x, y, z each, 2.12). Updates the wrong-way counter and flag, then advances the segment when the car is past
    /// the next waypoint's crossing line (arming the lap on lap segments 1–9 and in branches) or steps it back when
    /// it is behind the current one. Returns true when the game then runs [`RacingLine::lap_crossing`].
    pub fn track_player(
        &self,
        planes: &[Plane],
        back: &BackTable,
        race: &mut Race,
        r: &mut Racer,
        vectors: [i32; 6],
    ) -> bool {
        let (section, seg) = (usize::from(r.section), i32::from(r.segment));
        let head = self.sections[section];
        let (s, i) = self.step(race.lapped, back, section, seg + 1);
        let first = self.sections[s].first as i32;
        // Quirk kept: the direction row is the stepped section's first plus the *current* segment.
        let dir = planes[(first + seg) as usize];
        let dot = |v: &[i32]| (v[0].wrapping_mul(dir[0]) >> 12) + (v[2].wrapping_mul(dir[1]) >> 12);
        let ahead = dot(&vectors[..3]);
        r.wrong_way = if ahead < -10 || (ahead < 1 && dot(&vectors[3..]) < 0) {
            r.wrong_way.wrapping_add(1)
        } else {
            0
        };
        race.wrong_way = r.wrong_way > 27;
        let count = i32::from(head.count);
        if Self::side(planes, first + i, r) > 0 {
            if section == 0 {
                if (seg - 1) as u32 <= 8 {
                    r.flags |= 2;
                }
                if seg == count - 2 || seg == 0 {
                    r.flags &= 0xFFFE;
                }
                r.segment = self.step(race.lapped, back, 0, seg + 1).1 as i16;
                if i32::from(r.segment) == count - 1 {
                    r.segment = 0;
                }
            } else {
                r.segment = r.segment.wrapping_add(1);
                r.flags |= 2;
            }
            return true;
        }
        if Self::side(planes, head.first as i32 + seg, r) >= 0 {
            return false;
        }
        if section != 0 {
            r.segment = r.segment.wrapping_sub(1);
            return false;
        }
        if seg == 2 {
            r.flags &= 0xFFFD;
        }
        if seg == if race.lapped { 0 } else { 1 } {
            r.flags |= 1;
        }
        r.segment = self.step(race.lapped, back, 0, seg - 1).1 as i16;
        false
    }

    /// The AI's racing-line advance inside its driver (`FUN_0814d078`, `0x0814D21C..0x0814D3AA`); `segment` is the
    /// driver's working segment there. Past the next crossing line it moves on: through a branch link at the
    /// section's second-to-last point, else one point (the lap wraps); segments 1–7 arm the lap; before a fork it may
    /// take the shortcut when `rand & 0xFF` beats [`AI_BRANCH_CHANCE`]`[difficulty]` and `branch_ok[section]`
    /// (`0x030060C0`) allows it. Returns true when the game then runs [`RacingLine::lap_crossing`].
    pub fn ai_advance(
        &self,
        rom: &[u8],
        (planes, back): (&[Plane], &BackTable),
        race: &mut Race,
        r: &mut Racer,
        segment: i32,
        branch_ok: &[u32],
    ) -> bool {
        let section = usize::from(r.section);
        let count = i32::from(self.sections[section].count);
        let (s, i) = self.step(race.lapped, back, section, segment + 1);
        r.side = Self::side(planes, self.sections[s].first as i32 + i, r);
        if r.side <= 0 {
            return false;
        }
        r.flags &= 0xFFFE;
        let next = self.stepped(race.lapped, back, section, segment + 1);
        let mut seg = segment;
        if next.link_index != 0xFFFF && seg == count - 2 {
            (r.section, r.section_changed, seg) = (next.link_section, 2, i32::from(next.link_index));
        } else {
            seg += 1;
            if seg == count - 1 && r.section == 0 {
                seg = 0;
            }
        }
        let next = self.stepped(race.lapped, back, usize::from(r.section), seg + 1);
        if (seg - 1) as u32 <= 6 {
            r.flags |= 2;
        }
        if next.link_index == 0 && r.place != 1 && u32::from(r.id) <= race.opponents {
            let roll = (rand_table(rom, &mut race.rand) & 0xFF) as i32;
            let chance = u32_at(rom, AI_BRANCH_CHANCE + 4 * race.difficulty as usize) as i32;
            if roll > chance && branch_ok[usize::from(next.link_section)] != 0 {
                (r.section, r.section_changed, seg) = (next.link_section, 2, i32::from(next.link_index) - 1);
                r.flags |= 2;
            }
        }
        r.segment = seg as i16;
        true
    }

    /// `lap_crossing` (`FUN_0813f098`) for `cars[who]`: an armed car on the lap's last segment (`count − 1`,
    /// sprints `count − 2`) or segment 0 finishes a lap: lap time and best lap, one lap fewer, elimination
    /// knock-outs, and the finish when no laps are left (or in a sprint).
    pub fn lap_crossing(&self, rom: &[u8], race: &mut Race, cars: &mut [Racer], who: usize) {
        let target = i32::from(self.sections[0].count) - if race.lapped { 1 } else { 2 };
        let c = cars[who];
        let seg = i32::from(c.segment);
        if c.section != 0 || !(seg == target || seg == 0) || c.flags & 2 == 0 {
            return;
        }
        let c = &mut cars[who];
        c.flags &= 0xFFFD;
        let lap = race.time.wrapping_sub(c.lap_start);
        if lap < c.best_lap || c.best_lap == 0 {
            c.best_lap = lap;
        }
        c.lap_start = race.time;
        c.laps_left = c.laps_left.wrapping_sub(1);
        let (place, left) = (c.place, i32::from(c.laps_left));
        if race.mode == 1 && place == race.opponents as i32 - (race.laps - left) + 1 {
            let words = [0, 4, 8].map(|k| u32_at(rom, KNOCKOUT_WORDS + k));
            for j in 0..=race.opponents as usize {
                let o = &mut cars[race.player as usize + j];
                if o.place == place + 1 {
                    race.results[8 + j] = 8;
                    o.flags |= 8;
                    o.state = 2;
                    o.entity_flags &= 0xFFFB;
                    o.knockout = words;
                }
            }
        }
        if cars[who].laps_left == 0 || !race.lapped {
            race.finished = true;
            if u32::from(cars[who].id) > race.opponents {
                cars[who].state = 2;
            } else {
                Self::finish(race, cars, who);
            }
        }
    }

    /// `FUN_0813f008`: a racer finishes. Quirk kept: in elimination the camera car's finish marks result byte
    /// `opponents + 1` (the loop that finds the last place is computed and ignored).
    fn finish(race: &mut Race, cars: &mut [Racer], who: usize) {
        let c = &mut cars[who];
        c.finish = c.lap_start;
        let t = 0x20 + 4 * usize::from(c.id);
        race.results[t..t + 4].copy_from_slice(&c.lap_start.to_le_bytes());
        c.state = 2;
        if who == race.view as usize && race.mode == 1 {
            race.results[8 + race.opponents as usize + 1] = 8;
        }
    }

    /// Positions (`FUN_0813ea04`, every frame): each car still racing is 1 + the cars ahead on progress (ties go to
    /// the lower index), not counting knocked-out cars; finished cars count as ahead. When `0x03000048` is 9 the
    /// places are just the entity order.
    pub fn update_places(&self, race: &Race, cars: &mut [Racer]) {
        let n = race.opponents as usize + 1;
        let base = race.player as usize;
        if race.state48 == 9 {
            for i in 0..n {
                cars[base + i].place = i as i32 + 1;
            }
            return;
        }
        let len = self.lap_length();
        let progress = |c: &Racer| race_progress(race.lapped, len, race.laps, c.laps_left.into(), c.distance);
        for i in 0..n {
            let me = base + i;
            if cars[me].state == 2 {
                continue;
            }
            let mine = progress(&cars[me]);
            let mut place = 1;
            for (j, o) in cars[..n].iter().enumerate() {
                if o.flags & 8 == 0 && j != me {
                    let d = mine.wrapping_sub(progress(o));
                    if d < 0 || (d == 0 && j < i) || o.state == 2 {
                        place += 1;
                    }
                }
            }
            cars[me].place = place;
        }
    }

    /// `finish_time_estimate` (`FUN_0814f050`) for a car still racing at the end, after `elapsed` frames:
    /// `elapsed + (total − done) · elapsed / done` on the progress scale (64-bit; both at least 1), where anything
    /// outside `1..=359_999` becomes 359 999. Also keeps the best lap at the time per lap. In sprints it sets
    /// `laps` and laps left to 1 and measures to the point before the extra end point.
    pub fn finish_estimate(&self, race: &mut Race, c: &mut Racer, elapsed: u32) -> u32 {
        let count = i32::from(self.sections[0].count);
        let mut len = self.point(0, count - 1).distance;
        if !race.lapped {
            (race.laps, c.laps_left) = (1, 1);
            len = self.point(0, count - 2).distance;
        }
        let len = i64::from(len);
        let done = len * i64::from((race.laps - i32::from(c.laps_left)) as u32) + i64::from(c.distance);
        let rest = (len * i64::from(race.laps as u32) - done).max(1);
        let t = rest * i64::from(elapsed as i32) / done.max(1) + i64::from(elapsed as i32);
        let t = if (1..=359_999).contains(&t) { t as u32 } else { 359_999 };
        c.finish = t;
        let per = t / race.laps as u32;
        if per < c.best_lap || c.best_lap == 0 {
            c.best_lap = per;
        }
        t
    }
}

/// Hunter life (driver `+0x4E8`), `0..=HUNTER_LIFE_MAX`. Tuning from `FUN_081412ec`.
pub const HUNTER_LIFE_MAX: i32 = 0x80000;
/// Life gained per frame by race position 1..4 (`0x030061B0 + 4 * position`).
pub const HUNTER_GAIN: [i32; 5] = [0, 200, 150, 100, 0];
/// Hit damage per impulse unit (`0x0300617C`), and the two drains' factors (`0x03006184`, `0x030061A0`), all ×256.
pub const HUNTER_HIT: i32 = 0x440;
pub const HUNTER_DRAIN: [i32; 2] = [0x240, 0x240];

/// One frame of hunter life (`FUN_08140f78`): driving backwards (driver `+0x4EC` above 27) drains 1000, a wall
/// (`+0x4EE` above 50) drains 100, otherwise life grows by position until someone finishes (`0x030061A4`).
pub fn hunter_life_tick(race: &Race, r: &mut Racer) {
    r.life = if r.wrong_way > 27 {
        (r.life - 1000).max(0)
    } else if r.wall > 50 {
        (r.life - 100).max(0)
    } else if race.finished {
        r.life
    } else {
        (r.life + HUNTER_GAIN[r.place as usize]).min(HUNTER_LIFE_MAX)
    };
}

/// A hunter hit (`FUN_0814101c`), unless either car is finished: the victim loses `impulse · 0x440 >> 8`, and the
/// attacker gains three quarters of that while nobody has finished and the victim is a racer (id ≤ opponents).
pub fn hunter_hit(race: &Race, attacker: &mut Racer, victim: &mut Racer, impulse: i32) {
    if attacker.state == 2 || victim.state == 2 {
        return;
    }
    let damage = impulse.wrapping_mul(HUNTER_HIT) >> 8;
    victim.life = (victim.life - damage).max(0);
    if !race.finished && u32::from(victim.id) <= race.opponents {
        attacker.life = (attacker.life + ((damage * 3) >> 2)).min(HUNTER_LIFE_MAX);
    }
    attacker.hit = 0;
}

/// The other two life drains (`FUN_0814136c` with `kind` 0, `FUN_081413b0` with 1), unless the car is finished:
/// `HUNTER_DRAIN[kind] · amount >> 8`.
pub fn hunter_drain(r: &mut Racer, kind: usize, amount: i32) {
    if r.state != 2 {
        r.life = (r.life - (HUNTER_DRAIN[kind].wrapping_mul(amount) >> 8)).max(0);
        r.hit = 0;
    }
}

pub const SAVE_SIZE: usize = 512;
pub const SAVE_VERSION: u16 = 9;
pub const CHECKSUM_BIAS: u16 = 0xBADD;

/// The EEPROM image as mGBA stores it (`.sav`, bits in wire order) to the game's buffer: each 64-bit block
/// arrives most significant bit first and `FUN_08151194` stores it as four `u16` from the top, so every
/// 8-byte block is byte-reversed.
pub fn eeprom_to_buffer(sav: &[u8]) -> [u8; SAVE_SIZE] {
    let mut out = [0; SAVE_SIZE];
    for (o, s) in out.chunks_mut(8).zip(sav.chunks(8)) {
        o.copy_from_slice(s);
        o.reverse();
    }
    out
}

/// Sum of all 512 bytes with the checksum field zeroed, plus `0xBADD` (`FUN_081492c0`, `FUN_08149d84`).
pub fn checksum(buf: &[u8; SAVE_SIZE]) -> u16 {
    let sum = buf.iter().map(|&b| b as u32).sum::<u32>() - buf[0x100] as u32 - buf[0x101] as u32;
    (sum as u16).wrapping_add(CHECKSUM_BIAS)
}

/// Menu options, as the game's globals hold them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// 0 chase, 1 bumper (`0x030053E4`).
    pub camera: u8,
    /// 0 MPH, 1 KM/H (`0x03000040`).
    pub units: u8,
    /// 0 off, 1 on (`0x03005698`).
    pub hud: u8,
    /// 0 manual, 1 automatic (`0x03005798`).
    pub transmission: u8,
    /// 0 off, 1 low, 2 high; the globals hold `value << 3` (`0x0300578C`, `0x030053A4`).
    pub music: u8,
    pub sfx: u8,
    /// 0 En, 1 Fr, 2 De, 3 It, 4 Es (`0x03005600`; read at boot by `FUN_08149b94`).
    pub language: u8,
    /// 0 off, 1 on (`0x03000050`).
    pub catch_up: u8,
    /// One bit per [`RaceMode`] (`0x03000070`, set by `FUN_08134eb8`, tested by `FUN_0812cf48`).
    pub mode_flags: u8,
}

/// The profile, decoded exactly as `FUN_08149820` loads it (field comments: save offset → RAM).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Save {
    /// `0xB4` → profile `+0x00` (8 bytes, NUL-padded).
    pub name: [u8; 8],
    /// `0x00`, 12 bit-packed bytes per car → the 17-byte records at `*0x0300539C` (profile `+0xF9`).
    pub cars: [[u8; 17]; 15],
    /// Bit 7 of each car's 12th byte → profile `+0x12`.
    pub car_bits: u16,
    /// `0x11C`, 15 bytes per car → profile `+0x14`.
    pub car_extra: [[u8; 15]; 15],
    /// `0xC0` `u16` plus bit 0 of `0xC2` → profile `+0x0C`.
    pub cash: u32,
    /// `u16 0xBC` bits 7–10 → profile `+0x10` (career car).
    pub car: u8,
    /// `0xBC` bits 0–2 → profile `+0x1FB` (selected zone).
    pub zone: u8,
    /// `0xBC` bits 3–6 → profile `+0x1FC` (selected event in the zone).
    pub slot: u8,
    /// `u16 0xBE` bits 7–10 → profile `+0x200` (wingman, [`wingmen`] index).
    pub wingman: u8,
    /// `0xBF` bits 3–6 → profile `+0x254` (unknown).
    pub field_254: u8,
    /// `0x11A` → profile `+0x1F8` (unknown; 1 and 2 unlock wingmen 1 and 2 in `FUN_08135958`).
    pub field_1f8: u8,
    /// `0x104` → profile `+0xF5` (unknown).
    pub field_f5: [u8; 4],
    /// `0x108` → profile `+0x205`: 2 bits per event, see [`Save::event_status`].
    pub events: [u8; 18],
    /// `0xC4` → profile `+0x218`: per track (12 circuits, then 18 sprints), the record time to beat.
    pub best_times: [u16; 30],
    pub options: Options,
    /// `0x11B` bits 0–5 → profile `+0x47C, +0x480, +0x484, +0x48C, +0x488, +0x478` (unlock-everything flags).
    pub unlock_flags: u8,
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

impl Save {
    /// Decodes a game-order buffer (see [`eeprom_to_buffer`]). Requires the checksum (`FUN_08149d84`, load) and
    /// version 9 (`FUN_08149b94`, the boot check that marks the profile as present).
    pub fn parse(b: &[u8; SAVE_SIZE]) -> io::Result<Save> {
        let stored = u16_at(b, 0x100);
        if checksum(b) != stored {
            return Err(invalid("save checksum mismatch"));
        }
        if u16_at(b, 0x102) != SAVE_VERSION {
            return Err(invalid("save version is not 9"));
        }
        let mut car_bits = 0;
        let cars = std::array::from_fn(|i| {
            let p = &b[12 * i..12 * i + 12];
            car_bits |= u16::from(p[11] >> 7) << i;
            let mut r = [0u8; 17];
            r[7] = p[0] & 0x7F;
            r[8] = (p[1] & 0x3F) << 1 | p[0] >> 7;
            r[9] = (p[2] & 0x1F) << 2 | p[1] >> 6;
            r[10] = (p[3] & 0xF) << 3 | p[2] >> 5;
            r[0] = p[3] >> 4;
            r[11] = p[4] & 0x7F;
            r[12] = (p[5] & 0x3F) << 1 | p[4] >> 7;
            r[13] = (p[6] & 0x1F) << 2 | p[5] >> 6;
            r[14] = (p[7] & 0xF) << 3 | p[6] >> 5;
            r[2] = p[7] >> 4;
            r[15] = p[8] & 0x1F;
            r[16] = (p[9] & 3) << 3 | p[8] >> 5;
            r[6] = (p[9] & 0x7F) >> 2;
            r[5] = (p[10] & 0xF) << 1 | p[9] >> 7;
            r[1] = (p[10] & 0x7F) >> 4;
            r[3] = (p[11] & 1) << 1 | p[10] >> 7;
            r[4] = (p[11] & 0x7F) >> 1;
            r
        });
        let (bc, be) = (u16_at(b, 0xBC), u16_at(b, 0xBE));
        Ok(Save {
            name: b[0xB4..0xBC].try_into().unwrap(),
            cars,
            car_bits,
            car_extra: std::array::from_fn(|i| b[0x11C + 15 * i..0x11C + 15 * i + 15].try_into().unwrap()),
            cash: u32::from(b[0xC2] & 1) << 16 | u32::from(u16_at(b, 0xC0)),
            car: ((bc & 0x7FF) >> 7) as u8,
            zone: (bc & 7) as u8,
            slot: ((bc & 0x7F) >> 3) as u8,
            wingman: ((be & 0x7FF) >> 7) as u8,
            field_254: (b[0xBF] & 0x7F) >> 3,
            field_1f8: b[0x11A],
            field_f5: b[0x104..0x108].try_into().unwrap(),
            events: b[0x108..0x11A].try_into().unwrap(),
            best_times: std::array::from_fn(|i| u16_at(b, 0xC4 + 2 * i)),
            options: Options {
                camera: b[0xBD] >> 7,
                units: b[0xBE] & 1,
                hud: (b[0xBE] >> 1) & 1,
                transmission: (b[0xBE] >> 2) & 1,
                music: (b[0xBD] >> 3) & 3,
                sfx: (b[0xBD] & 0x7F) >> 5,
                language: (b[0xBE] & 0x7F) >> 4,
                catch_up: (b[0xBE] & 0xF) >> 3,
                mode_flags: b[0xC3] >> 4,
            },
            unlock_flags: b[0x11B] & 0x3F,
        })
    }

    /// The game's save buffer for this profile (`FUN_081492c0`, in its write order). The game encodes into a fresh
    /// `malloc(0x200)` (`save_write_profile`), and the bits it never writes keep the heap's old bytes: pass them
    /// as `heap` (`0xBF` bit 7, `0xC2` bits 1–7, `0xC3` bits 0–3, `0x11B` bits 6–7, `0x1FD..`).
    /// [`eeprom_to_buffer`] turns the result into the `.sav` image (it is its own inverse).
    pub fn encode(&self, heap: &[u8; SAVE_SIZE]) -> [u8; SAVE_SIZE] {
        let mut b = *heap;
        b[0x100..0x102].fill(0);
        b[0xB4..0xBC].copy_from_slice(&self.name);
        for (i, r) in self.cars.iter().enumerate() {
            b[0x11C + 15 * i..0x11C + 15 * i + 15].copy_from_slice(&self.car_extra[i]);
            let bit = (self.car_bits >> i) as u8;
            b[12 * i..12 * i + 12].copy_from_slice(&[
                r[7] & 0x7F | r[8] << 7,
                r[8] >> 1 & 0x3F | r[9] << 6,
                r[9] >> 2 & 0x1F | r[10] << 5,
                r[10] >> 3 & 0xF | r[0] << 4,
                r[11] & 0x7F | r[12] << 7,
                r[12] >> 1 & 0x3F | r[13] << 6,
                r[13] >> 2 & 0x1F | r[14] << 5,
                r[14] >> 3 & 0xF | r[2] << 4,
                r[15] & 0x1F | r[16] << 5,
                r[16] >> 3 & 3 | (r[6] & 0x1F) << 2 | r[5] << 7,
                r[5] >> 1 & 0xF | (r[1] & 7) << 4 | r[3] << 7,
                r[3] >> 1 & 1 | (r[4] & 0x3F) << 1 | bit << 7,
            ]);
        }
        b[0x108..0x11A].copy_from_slice(&self.events);
        for (i, t) in self.best_times.iter().enumerate() {
            b[0xC4 + 2 * i..0xC6 + 2 * i].copy_from_slice(&t.to_le_bytes());
        }
        b[0x104..0x108].copy_from_slice(&self.field_f5);
        let bc = u16_at(&b, 0xBC) & 0xF87F | u16::from(self.car & 0xF) << 7;
        b[0xBC..0xBE].copy_from_slice(&bc.to_le_bytes());
        b[0x11A] = self.field_1f8;
        b[0xC0..0xC2].copy_from_slice(&(self.cash as u16).to_le_bytes());
        b[0xC2] = b[0xC2] & 0xFE | (self.cash >> 16) as u8 & 1;
        let be = u16_at(&b, 0xBE) & 0xF87F | u16::from(self.wingman & 0xF) << 7;
        b[0xBE..0xC0].copy_from_slice(&be.to_le_bytes());
        b[0xBF] = b[0xBF] & 0x87 | (self.field_254 & 0xF) << 3;
        let o = self.options;
        b[0xBD] = b[0xBD] & 0x7F | o.camera << 7;
        b[0xBE] = b[0xBE] & 0xF8 | o.units & 1 | (o.hud & 1) << 1 | (o.transmission & 1) << 2;
        b[0xBD] = b[0xBD] & 0x87 | (o.music & 3) << 3 | (o.sfx & 3) << 5;
        b[0xBE] = b[0xBE] & 0x8F | (o.language & 7) << 4;
        b[0xBC] = b[0xBC] & 0x80 | self.zone & 7 | (self.slot & 0xF) << 3;
        b[0x11B] = b[0x11B] & 0xC0 | self.unlock_flags & 0x3F;
        b[0xBE] = b[0xBE] & 0xF7 | (o.catch_up & 1) << 3;
        b[0xC3] = b[0xC3] & 0xF | o.mode_flags << 4;
        b[0x102..0x104].copy_from_slice(&SAVE_VERSION.to_le_bytes());
        let sum = checksum(&b);
        b[0x100..0x102].copy_from_slice(&sum.to_le_bytes());
        b
    }

    /// 1 won, 2 second place, 3 not done (a new profile has every event at 3) (`FUN_08135d4c`).
    pub fn event_status(&self, event: usize) -> u8 {
        self.events[event >> 2] >> ((event & 3) * 2) & 3
    }

    /// The unlock bitfield (profile `+0x42D`, 40 bytes, bit `id` set = unlocked) that `FUN_08135958` rebuilds
    /// after loading: fixed starting unlocks, progress unlocks per zone, zone and boss unlocks
    /// (`FUN_0813589c`) and the `unlock_flags` ranges.
    pub fn unlocks(&self, rom: &[u8]) -> [u8; 40] {
        let mut bits = [0u8; 40];
        let mut set = |id: u16| bits[id as usize >> 3] |= 1 << (id & 7);
        const START: [u16; 40] = [
            0x108, 0x109, 0x10A, 0x72, 0x76, 0x117, 0x127, 0, 4, 8, 0xD, 0x11, 0x15, 0x1A, 0x1E, 0x22, 0x27, 0x2B,
            0x2F, 0x34, 0x38, 0x3C, 0x41, 0x45, 0x49, 0x4E, 0x52, 0x56, 0x5B, 0x5F, 0x63, 0x68, 0x6C, 0x71, 0x75, 0x79,
            0x89, 0x90, 0xA0, 0xBF,
        ];
        START.into_iter().chain(0xE0..=0x107).for_each(&mut set);
        match self.field_1f8 {
            0 => {}
            1 => set(0x128),
            _ => [0x128, 0x129].into_iter().for_each(&mut set),
        }
        let table = progress_unlocks(rom);
        for (zone, boss) in boss_events(rom).iter().enumerate() {
            let first = zone * 12;
            let mut key = first; // advances by one per counted event
            for i in first..first + if zone == 5 { 6 } else { 12 } {
                let counts = match self.event_status(i) {
                    1 => true,
                    2 => zone != 5 && !boss.contains(&(i as u16)),
                    _ => false,
                };
                if counts {
                    key += 1;
                    table
                        .iter()
                        .filter(|u| u.key as usize == key)
                        .flat_map(|u| &u.ids)
                        .for_each(|&id| set(id));
                }
            }
            if key - first > 6 {
                let z = zone as u16;
                if zone < 5 {
                    set(0x122 + z);
                    if self.event_status(boss[0] as usize) == 1 {
                        [0x11D + z, 0x10B + 2 * z].into_iter().for_each(&mut set);
                    }
                }
                if self.event_status(boss[1] as usize) == 1 && zone < 5 {
                    [0x10C + 2 * z, 0x118 + z, 0x12A + 2 * z, 0x12B + 2 * z]
                        .into_iter()
                        .for_each(&mut set);
                }
            }
        }
        let f = |bit: u8| self.unlock_flags >> bit & 1 != 0;
        if f(5) {
            (0x108..0x117).for_each(&mut set);
        }
        if f(4) {
            (0x127..0x135).for_each(&mut set);
        }
        if f(3) {
            (0x117..0x11D).for_each(&mut set);
        }
        if f(1) {
            (0x79..=0x107).for_each(&mut set);
        }
        if f(2) {
            (0..0x79).for_each(&mut set);
        }
        if f(0) {
            (0..6).flat_map(|i| [0x11D + i, 0x122 + i]).for_each(&mut set);
        }
        bits
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text;
    use nfsgba_testkit::{fixture, rom};

    /// Files from the reference run (`mgba/`).
    fn reference(name: &str) -> Option<Vec<u8>> {
        nfsgba_testkit::read(&format!("mgba/{name}"))
    }

    #[test]
    fn career_events_are_consistent() {
        let Some(rom) = rom() else { return };
        let events = events(&rom);
        let name = |e: &Event| text(&rom, track_name(&rom, e.track_slot()).0, Some(0));
        for e in &events {
            let sprint = e.mode == RaceMode::Sprint;
            assert_eq!(sprint, e.track >= 24);
            assert!(!sprint || (e.laps == 1 && !e.reverse));
            assert!(e.track < 12 || sprint);
            assert!(e.difficulty() <= 2 && e.traffic <= 2 && (1..=5).contains(&e.laps));
        }
        assert_eq!(
            (events[0].mode, name(&events[0]).as_str(), events[0].reward),
            (RaceMode::Circuit, "LONGPOINT", 100)
        );
        assert_eq!(
            (events[65].skill, events[65].difficulty(), name(&events[65]).as_str()),
            (100, 2, "SOUTH RUN")
        );
        assert_eq!(
            boss_events(&rom),
            [[7, 8], [19, 20], [31, 32], [43, 44], [55, 56], [66, 66]]
        );
        // Every track slot maps back to itself through the route number.
        for slot in 0..42 {
            assert_eq!(route_track_slot(&rom, track_name(&rom, slot).1), slot);
        }
        assert_eq!(text(&rom, u16_at(&rom, MODE_NAMES + 2 * 2) as usize, Some(0)), "HUNTER");
    }

    #[test]
    fn race_slots_and_sections_match_the_reference_race() {
        let Some(rom) = rom() else { return };
        let slots = race_slots(&rom);
        // Quick Play STORAGE RUN forward is route number 23: environment 11 and route 23 in the race's IWRAM.
        assert_eq!(track_name(&rom, route_track_slot(&rom, 23)).1, 23);
        assert_eq!((slots[23].environment, slots[23].route), (11, 23));
        if let Some(iw) = reference("race.iwram.bin") {
            assert_eq!((iw[0x6C], iw[0x5720], iw[0x5388]), (11, 23, 23));
        }
        assert!(
            slots[1..43]
                .iter()
                .enumerate()
                .all(|(i, s)| s.route as usize == i + 1 && s.environment < 12)
        );
        let sections = route_sections(&rom);
        assert!(sections[0].is_empty() && sections[1..].iter().all(|s| !s.is_empty() && s[0].first == 0));
        assert_eq!(
            sections[23].iter().map(|s| (s.count, s.first)).collect::<Vec<_>>(),
            [(36, 0), (8, 36)]
        );
        // The lap closes on its first waypoint; the lap length is the last main waypoint's distance.
        let line = ptr(&rom, ROUTE_TABLE + 0x14 * 23 + 8);
        assert_eq!(rom[line..line + 8], rom[line + 35 * 0x18..line + 35 * 0x18 + 8]);
        assert_eq!(u32_at(&rom, line + 35 * 0x18 + 0x10), 108_219);
        // The race measures the lap again with integer lengths (`FUN_0813f744`): 108,217.
        assert_eq!(RacingLine::new(&rom, 23, false).unwrap().lap_length(), 108_217);
    }

    /// The world's racing line and the plane table that `race_load_level` built for the reference race (route 23,
    /// the first race after boot, so the lapped flag was still 0 when the planes were built).
    #[test]
    fn racing_line_and_planes_match_the_reference_race() {
        let Some(rom) = rom() else { return };
        let (Some(iw), Some(ew)) = (reference("race.iwram.bin"), reference("race.wram.bin")) else {
            return;
        };
        let at = |p: u32| (p - 0x0200_0000) as usize;
        let world = |o: usize| at(u32_at(&iw, 0xC0 + o));
        let line = RacingLine::new(&rom, 23, false).unwrap();
        let total: usize = line.sections.iter().map(|s| usize::from(s.count)).sum();
        for (s, sec) in line.sections.iter().enumerate() {
            let h = world(0x40) + 8 * s;
            assert_eq!((u16_at(&ew, h), u32_at(&ew, h + 4)), (sec.count, sec.first));
        }
        for (k, p) in line.points[..total].iter().enumerate() {
            let w = world(0x44) + 0x18 * k;
            let ram = (
                u32_at(&ew, w) as i32,
                u32_at(&ew, w + 4) as i32,
                u16_at(&ew, w + 0xC),
                u16_at(&ew, w + 0xE),
                u32_at(&ew, w + 0x10) as i32,
            );
            assert_eq!((p.x, p.z, p.link_section, p.link_index, p.distance), ram, "point {k}");
        }
        let (table, back_table) = (at(u32_at(&iw, 0x5FB4)), at(u32_at(&iw, 0x5FB8)));
        let mut planes = vec![[0; 8]; 256];
        let back = line.planes(false, &mut planes);
        for (k, row) in planes[..total].iter().enumerate() {
            let ram: Plane = std::array::from_fn(|i| u32_at(&ew, table + 0x20 * k + 4 * i) as i32);
            assert_eq!(*row, ram, "plane row {k}");
        }
        assert!((0..256).all(|s| back[s] == u32_at(&ew, back_table + 4 * s) as i32));
        assert_eq!((back[1], line.step(true, &back, 1, -1)), (19, (0, 18))); // the branch start links back to 19
    }

    #[test]
    fn setup_screens_and_the_reference_quick_play() {
        let Some(rom) = rom() else { return };
        let screens = setup_screens(&rom);
        let key = |k: u32| text(&rom, k as usize, Some(0));
        assert_eq!(key(screens[2].title_key.into()), "SETTING CIRCUIT ");
        let circuit: Vec<_> = screens[2]
            .items
            .iter()
            .map(|i| (key(i.text_key), i.setting, i.min, i.max))
            .collect();
        let want = [
            ("DIRECTION", 2, 0, 1),
            ("LAPS", 3, 1, 6),
            ("DIFFICULTY", 4, 0, 2),
            ("OPPONENTS", 5, 1, 3),
        ];
        for (got, want) in circuit.iter().zip(want) {
            assert_eq!((got.0.as_str(), got.1, got.2, got.3), want);
        }
        assert_eq!(screens[3].items[1].max, 3); // elimination: at most 3 laps
        assert_eq!(
            screens[0].items.iter().map(|i| key(i.options[1])).collect::<Vec<_>>()[..2],
            ["BUMPER", "KM/H"]
        );
        // The reference race (screenshot s11): forward, 3 laps, easy, 3 opponents, no traffic, catch-up off.
        let Some(iw) = reference("race.iwram.bin") else { return };
        let g = |a: usize| u32_at(&iw, a - 0x0300_0000);
        let got = [
            0x0300_56E0,
            0x0300_5610,
            0x0300_56E4,
            0x0300_5608,
            0x0300_5784,
            0x0300_5604,
            0x0300_0050,
        ]
        .map(g);
        assert_eq!(got, [0, 0, 3, 0, 3, 0, 0]);
    }

    #[test]
    fn wingmen_and_unlocks() {
        let Some(rom) = rom() else { return };
        let w = wingmen(&rom);
        let key = |k: u16| text(&rom, k as usize, Some(0));
        assert_eq!(
            (key(w[0].name_key).as_str(), key(w[0].role_key).as_str()),
            ("NONE", "NONE")
        );
        assert_eq!(
            (key(w[1].name_key).as_str(), key(w[1].role_key).as_str(), w[1].level),
            ("KITA", "ATTACKER", 0)
        );
        assert_eq!(
            (key(w[12].name_key).as_str(), key(w[12].role_key).as_str(), w[12].level),
            ("CLUTCH", "DRAFTER", 5)
        );
        let unlocks = progress_unlocks(&rom);
        assert!(unlocks.windows(2).all(|p| p[0].key <= p[1].key));
        assert_eq!((unlocks[0].key, &unlocks[0].ids[..]), (1, &[0x69, 0x6D][..]));
        assert_eq!(unlocks.last().map(|u| u.key), Some(66));
    }

    #[test]
    fn save_decodes_to_the_reference_ram() {
        let Some(sav) = reference("BN7E_v0_e5298b24.sav") else {
            return;
        };
        let buf = eeprom_to_buffer(&sav);
        let save = Save::parse(&buf).unwrap();
        // The encoder reproduces the game's image bit for bit (unused bits from the buffer it overwrites).
        assert_eq!(eeprom_to_buffer(&save.encode(&buf)), sav[..SAVE_SIZE]);
        assert_eq!(Save::parse(&save.encode(&[0x5A; SAVE_SIZE])).unwrap(), save);
        assert_eq!(&save.name, b"A\0\0\0\0\0\0\0");
        assert!((0..EVENT_COUNT).all(|e| save.event_status(e) == 3));
        let (Some(iw), Some(ew)) = (reference("race.iwram.bin"), reference("race.wram.bin")) else {
            return;
        };
        let g32 = |a: usize| match a {
            0x0300_0000.. => u32_at(&iw, a - 0x0300_0000),
            _ => u32_at(&ew, a - 0x0200_0000),
        };
        let p = g32(0x0300_56EC) as usize - 0x0200_0000;
        let cars = g32(0x0300_539C) as usize - 0x0200_0000;
        assert_eq!(cars, p + 0xF9);
        assert_eq!(ew[p..p + 8], save.name);
        assert!((0..15).all(|i| ew[cars + 17 * i..cars + 17 * i + 17] == save.cars[i]));
        assert!((0..15).all(|i| ew[p + 0x14 + 15 * i..p + 0x14 + 15 * i + 15] == save.car_extra[i]));
        assert_eq!(u16_at(&ew, p + 0x12), save.car_bits);
        assert_eq!(u32_at(&ew, p + 0xC), save.cash);
        assert_eq!(
            [ew[p + 0x10], ew[p + 0x1FB], ew[p + 0x1FC], ew[p + 0x1F8]],
            [save.car, save.zone, save.slot, save.field_1f8]
        );
        assert_eq!(
            (u32_at(&ew, p + 0x200), i16_at(&ew, p + 0x254)),
            (save.wingman as u32, save.field_254 as i16)
        );
        assert_eq!(ew[p + 0xF5..p + 0xF9], save.field_f5);
        assert_eq!(ew[p + 0x205..p + 0x217], save.events);
        assert!((0..30).all(|i| u16_at(&ew, p + 0x218 + 2 * i) == save.best_times[i]));
        let o = save.options;
        let ram = [
            0x0300_53E4,
            0x0300_0040,
            0x0300_5698,
            0x0300_5798,
            0x0300_578C,
            0x0300_53A4,
            0x0300_5600,
        ];
        let want = [
            o.camera,
            o.units,
            o.hud,
            o.transmission,
            o.music << 3,
            o.sfx << 3,
            o.language,
        ];
        assert_eq!(ram.map(g32), want.map(u32::from));
        // Catch-up was saved at its new-profile default (on); the race setup turned it off without saving.
        assert_eq!((o.catch_up, g32(0x0300_0050)), (1, 0));
        assert_eq!(g32(0x0300_0070), u32::from(o.mode_flags));
        assert_eq!(
            save.cars.map(|c| c[6]),
            [5, 6, 11, 13, 1, 2, 11, 6, 14, 11, 6, 13, 0, 1, 19]
        );
        let Some(rom) = rom() else { return };
        assert_eq!(save.cars.map(|c| c[6]), rom[0x7E_EA33..0x7E_EA42]); // new-profile defaults (`FUN_081356dc`)
        assert_eq!(ew[p + 0x42D..p + 0x455], save.unlocks(&rom));
    }

    /// A valid save with every event not done.
    fn blank_save() -> Save {
        let mut b = [0u8; SAVE_SIZE];
        b[0x102] = 9;
        b[0x108..0x11A].fill(0xFF);
        let c = checksum(&b).to_le_bytes();
        b[0x100..0x102].copy_from_slice(&c);
        Save::parse(&b).unwrap()
    }

    #[test]
    fn progress_and_zone_unlocks() {
        let Some(rom) = rom() else { return };
        let mut save = blank_save();
        save.events[..2].copy_from_slice(&[0b01_01_01_01, 0b11_01_01_01]); // events 0..=6 won
        let bits = save.unlocks(&rom);
        let has = |id: u16| bits[id as usize >> 3] >> (id & 7) & 1 == 1;
        // Keys 1 and 7 of the progress table; seven wins open the zone's first boss race (0x122).
        assert!([0x69, 0x6D, 0xA4, 0x42, 0x122].into_iter().all(has));
        assert!(![0xA2, 0x11D, 0x10B].into_iter().any(has)); // key 8 and the boss-1 rewards are not reached
    }

    #[test]
    fn race_rules() {
        let Some(rom) = rom() else { return };
        let mut save = blank_save();
        // Zone 1 rewards: 100, 150, 175, …
        assert_eq!(race_payout(&rom, &mut save, 0, 3, 1, 110), 110); // first win: the first reward in line
        assert_eq!(save.event_status(3), 1);
        assert_eq!(race_payout(&rom, &mut save, 0, 3, 1, 100), 50); // replay: half the last reward
        assert_eq!(race_payout(&rom, &mut save, 0, 4, 2, 100), 75); // second place: half the next (150 / 2)
        assert_eq!(race_payout(&rom, &mut save, 0, 5, 3, 100), 0);
        assert_eq!(save.cash, 235);
        assert_eq!(
            (reward_percent(100), reward_percent(101), reward_percent(176)),
            (100, 110, 140)
        );
        assert_eq!(race_progress(true, 108_219, 3, 2, 500), 108_719);
        let race = Race {
            opponents: 3,
            ..Race::default()
        };
        let mut r = Racer {
            life: HUNTER_LIFE_MAX - 10,
            place: 1,
            ..Racer::default()
        };
        hunter_life_tick(&race, &mut r);
        assert_eq!(r.life, HUNTER_LIFE_MAX);
        let (mut a, mut v) = (
            Racer {
                hit: 5,
                ..Racer::default()
            },
            Racer {
                id: 1,
                life: 100,
                ..Racer::default()
            },
        );
        hunter_hit(&race, &mut a, &mut v, 0x100);
        assert_eq!((a.life, v.life, a.hit), (0x330, 0, 0));
    }

    /// One line of a race-rule trace (`tools/trace_race_rules.lua`).
    struct Trace {
        file: String,
        frame: u64,
        func: String,
        kv: std::collections::HashMap<String, String>,
    }

    impl Trace {
        fn get(&self, k: &str) -> &str {
            self.kv
                .get(k)
                .unwrap_or_else(|| panic!("{} frame {}: no {k}", self.file, self.frame))
        }
        fn ints(&self, k: &str) -> Vec<i64> {
            self.get(k).split(',').map(|v| v.parse().unwrap()).collect()
        }
        fn bytes(&self, k: &str) -> Vec<u8> {
            let s = self.get(k);
            (0..s.len() / 2)
                .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
                .collect()
        }
        fn racer(&self, k: &str) -> Racer {
            let v = self.ints(k);
            Racer {
                id: v[0] as u16,
                section: v[1] as u16,
                segment: v[2] as i16,
                state: v[3] as u16,
                entity_flags: v[4] as u16,
                x: v[5] as i32,
                z: v[6] as i32,
                place: v[7] as i32,
                distance: v[8] as i32,
                best_lap: v[9] as u32,
                lap_start: v[10] as u32,
                finish: v[11] as u32,
                laps_left: v[12] as i8,
                flags: v[13] as u16,
                life: v[14] as i32,
                wrong_way: v[15] as i16,
                wall: v[16] as i16,
                hit: v[17] as i16,
                knockout: [v[18] as u32, v[19] as u32, v[20] as u32],
                side: v[21] as i32,
                section_changed: v[22] as u16,
            }
        }
        /// The race globals `g`, with the results block `res` when the line has one; also the route index.
        fn race(&self, g: &str, res: &str) -> (Race, usize) {
            let v = self.ints(g);
            let mut results = [0; 0x40];
            if self.kv.contains_key(res) {
                results.copy_from_slice(&self.bytes(res));
            }
            let race = Race {
                mode: v[0] as u32,
                lapped: v[1] != 0,
                laps: v[2] as i32,
                opponents: v[3] as u32,
                time: v[4] as u32,
                finished: v[5] != 0,
                view: v[6] as u32,
                player: v[7] as u32,
                difficulty: v[8] as u32,
                state48: v[9] as u32,
                rand: v[10] as u32,
                wrong_way: v[11] != 0,
                results,
            };
            (race, v[12] as usize)
        }
        /// Every captured car: the racers, and for a lap crossing every entity up to the crossing one.
        fn cars(&self, tag: &str) -> Vec<Racer> {
            (0..)
                .map(|i| format!("{tag}.c{i}"))
                .take_while(|k| self.kv.contains_key(k))
                .map(|k| self.racer(&k))
                .collect()
        }
    }

    /// Every line of `data/work/e5298b24/race-rules/*.log` (mGBA traces) and `oracle-*.jsonl` (oracle cases from
    /// `tools/oracle_race_rules.py`, the same keys; the line number stands for the frame), file by file.
    fn traces() -> Vec<Trace> {
        let Some(dir) = fixture("race-rules") else {
            return Vec::new();
        };
        let entries = std::fs::read_dir(&dir).unwrap();
        let mut files: Vec<_> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "log" || x == "jsonl"))
            .collect();
        files.sort();
        let mut out = Vec::new();
        for f in files {
            let name = f.file_name().unwrap().to_string_lossy().into_owned();
            let json = name.ends_with(".jsonl");
            for (i, line) in std::fs::read_to_string(&f).unwrap().lines().enumerate() {
                let mut kv: std::collections::HashMap<String, String> = if json {
                    let v: serde_json::Map<String, serde_json::Value> = serde_json::from_str(line).unwrap();
                    v.into_iter()
                        .map(|(k, v)| (k, v.as_str().map_or_else(|| v.to_string(), str::to_owned)))
                        .collect()
                } else {
                    line.split_whitespace()
                        .filter_map(|w| w.split_once('='))
                        .map(|(k, v)| (k.to_owned(), v.to_owned()))
                        .collect()
                };
                if json {
                    kv.insert("frame".into(), i.to_string());
                }
                if let (Some(frame), Some(func)) = (kv.get("frame"), kv.get("fn")) {
                    out.push(Trace {
                        file: name.clone(),
                        frame: frame.parse().unwrap(),
                        func: func.clone(),
                        kv,
                    });
                }
            }
        }
        out
    }

    /// The race rules reproduce every traced call of the reference build (see `career.md`, "Race-rule traces").
    #[test]
    fn race_rules_match_the_traces() {
        let Some(rom) = rom() else { return };
        let mut counts = std::collections::BTreeMap::<String, usize>::new();
        // Per file: the lapped flag the plane table was built with (0 when a file has no build line).
        let mut built: std::collections::HashMap<String, bool> = Default::default();
        let mut last_sav = std::collections::HashMap::new();
        let mut tables: std::collections::HashMap<String, Vec<Plane>> = Default::default();
        for t in traces() {
            let at = || format!("{} frame {} {}", t.file, t.frame, t.func);
            // Oracle cases name the snapshot's line (`sprint`) and plane build (`built`); traces follow the race.
            let lapped_at_build =
                t.kv.get("built")
                    .map_or(*built.get(&t.file).unwrap_or(&false), |b| b != "0");
            let sprint = t.kv.get("sprint").map(|s| s != "0");
            let line_for =
                |race: &Race, route: usize| RacingLine::new(&rom, route, sprint.unwrap_or(race.mode == 3)).unwrap();
            // Scenes without a racing line (route 0: an all-zero world copy) have no rules to check.
            let g = ["g", "pre.g"].into_iter().find(|k| t.kv.contains_key(*k));
            if let Some(g) = g
                && RacingLine::new(&rom, t.race(g, "").1, false).is_none()
            {
                if t.func == "build_planes" {
                    assert!(t.bytes("sections").iter().all(|&b| b == 0), "{}", at());
                }
                continue;
            }
            match t.func.as_str() {
                // At the build the line is not measured yet (distances are checked mid-race only), and the build's
                // lapped flag is known; a mid-race capture must match the build with one of the two flags.
                "build_planes" | "planes_now" => {
                    let (race, route) = t.race("g", "");
                    let line = line_for(&race, route);
                    let (sections, points) = (t.bytes("sections"), t.bytes("points"));
                    let total: usize = line.sections.iter().map(|s| usize::from(s.count)).sum();
                    for (s, sec) in line.sections.iter().enumerate() {
                        let got = (u16_at(&sections, 8 * s), u32_at(&sections, 8 * s + 4));
                        assert_eq!(got, (sec.count, sec.first), "{} section {s}", at());
                    }
                    let now = t.func == "planes_now";
                    for (k, p) in line.points[..total].iter().enumerate() {
                        let w = 0x18 * k;
                        let got = (u32_at(&points, w) as i32, u32_at(&points, w + 4) as i32);
                        let links = (u16_at(&points, w + 0xC), u16_at(&points, w + 0xE));
                        let dist = if now {
                            u32_at(&points, w + 0x10) as i32
                        } else {
                            p.distance
                        };
                        let want = ((p.x, p.z), (p.link_section, p.link_index), p.distance);
                        assert_eq!(want, (got, links, dist), "{} point {k}", at());
                    }
                    let rows = |b: &[u8]| -> Vec<Plane> {
                        (0..256)
                            .map(|k| std::array::from_fn(|i| u32_at(b, 0x20 * k + 4 * i) as i32))
                            .collect()
                    };
                    let (table, back_table) = (rows(&t.bytes("planes")), t.bytes("back"));
                    // Rows the build skips keep the buffer's old contents: oracle cases record them (`old`), traces
                    // do not, so there those rows are left out.
                    const UNSET: Plane = [i32::MIN; 8];
                    let old = t.kv.get("old").map_or(vec![UNSET; 256], |_| rows(&t.bytes("old")));
                    let build = |lapped: bool| {
                        let mut planes = old.clone();
                        let back = line.planes(lapped, &mut planes);
                        (planes, back)
                    };
                    let matches = |lapped: bool| {
                        let (planes, back) = build(lapped);
                        (0..total).all(|k| planes[k] == UNSET || planes[k] == table[k])
                            && (0..256).all(|s| back[s] == u32_at(&back_table, 4 * s) as i32)
                    };
                    let lapped = if now {
                        [false, true].into_iter().find(|&l| matches(l))
                    } else {
                        Some(t.get("lapped") != "0").filter(|&l| matches(l))
                    };
                    let Some(lapped) = lapped else {
                        let (planes, _) = build(t.get("lapped") != "0");
                        let row = (0..total).find(|&k| planes[k] != UNSET && planes[k] != table[k]);
                        panic!(
                            "{}: plane row {row:?}: {:?} vs {:?}",
                            at(),
                            row.map(|k| planes[k]),
                            row.map(|k| table[k])
                        );
                    };
                    built.insert(t.file.clone(), lapped);
                    tables.insert(t.file.clone(), table);
                }
                "lap_crossing" => {
                    let (mut race, route) = t.race("pre.g", "pre.res");
                    let mut cars = t.cars("pre");
                    let who: usize = t.get("who").parse().unwrap();
                    if who >= cars.len() {
                        continue; // captured before the tracer logged non-racers (the wingman)
                    }
                    line_for(&race, route).lap_crossing(&rom, &mut race, &mut cars, who);
                    let (want, _) = t.race("post.g", "post.res");
                    assert_eq!((race, cars), (want, t.cars("post")), "{}", at());
                }
                "ai_advance" if !t.kv.contains_key("ok") => continue, // captured before the tracer logged 0x030060C0
                "track_player" | "ai_advance" => {
                    let (mut race, route) = t.race("g", "");
                    let line = line_for(&race, route);
                    // The table as the race had it: the build over the captured table keeps its unbuilt rows.
                    let mut planes = tables.get(&t.file).cloned().unwrap_or_else(|| vec![[0; 8]; 256]);
                    let back = line.planes(lapped_at_build, &mut planes);
                    let mut r = t.racer("pre");
                    let crossed = if t.func == "track_player" {
                        let v = t.ints("vec");
                        line.track_player(&planes, &back, &mut race, &mut r, std::array::from_fn(|i| v[i] as i32))
                    } else {
                        let ok: Vec<u32> = t.ints("ok").iter().map(|&v| v as u32).collect();
                        let seg = t.get("seg").parse().unwrap();
                        let crossed = line.ai_advance(&rom, (&planes, &back), &mut race, &mut r, seg, &ok);
                        assert_eq!(race.rand, t.get("rand").parse::<u32>().unwrap(), "{} rand", at());
                        crossed
                    };
                    let want = if t.kv.contains_key("mid") {
                        t.racer("mid")
                    } else {
                        t.racer("post")
                    };
                    assert_eq!((crossed, r), (t.kv.contains_key("mid"), want), "{}", at());
                    if t.func == "track_player" {
                        assert_eq!(race.wrong_way, t.get("ww_flag") != "0", "{}", at());
                    }
                }
                "race_progress" => {
                    let (race, route) = t.race("g", "");
                    let c = t.racer("c");
                    let len = line_for(&race, route).lap_length();
                    let got = race_progress(race.lapped, len, race.laps, c.laps_left.into(), c.distance);
                    assert_eq!(got, t.get("ret").parse::<i32>().unwrap(), "{}", at());
                }
                "update_places" => {
                    let (race, route) = t.race("pre.g", "pre.res");
                    let mut cars = t.cars("pre");
                    line_for(&race, route).update_places(&race, &mut cars);
                    assert_eq!(cars, t.cars("post"), "{}", at());
                }
                "hunter_life_tick" | "hunter_drain_a" | "hunter_drain_b" => {
                    let (race, _) = t.race("g", "");
                    assert_eq!(
                        t.ints("tune")[..],
                        [0, 200, 150, 100, 0, 27, 50, 1000, 100, 0x440, 0x240, 0x240],
                        "{}",
                        at()
                    );
                    let mut r = t.racer("pre");
                    match t.func.as_str() {
                        "hunter_life_tick" => hunter_life_tick(&race, &mut r),
                        f => hunter_drain(
                            &mut r,
                            usize::from(f == "hunter_drain_b"),
                            t.get("amount").parse().unwrap(),
                        ),
                    }
                    assert_eq!(r, t.racer("post"), "{}", at());
                }
                "hunter_hit" => {
                    let (race, _) = t.race("g", "");
                    let (mut a, mut v) = (t.racer("pre.a"), t.racer("pre.v"));
                    hunter_hit(&race, &mut a, &mut v, t.get("impulse").parse().unwrap());
                    assert_eq!((a, v), (t.racer("post.a"), t.racer("post.v")), "{}", at());
                }
                "finish_estimate" => {
                    let (mut race, route) = t.race("g", "");
                    let mut c = t.racer("pre");
                    let got =
                        line_for(&race, route).finish_estimate(&mut race, &mut c, t.get("elapsed").parse().unwrap());
                    assert_eq!(got as i64, t.get("ret").parse::<i64>().unwrap(), "{}", at());
                    assert_eq!(
                        (c, race.laps),
                        (t.racer("post"), t.race("post.g", "").0.laps),
                        "{}",
                        at()
                    );
                }
                "style_rating" => {
                    let rec: [u8; 17] = t.bytes("record").try_into().unwrap();
                    assert_eq!(
                        style_rating(&rom, t.get("car").parse().unwrap(), &rec),
                        t.get("ret").parse::<i32>().unwrap(),
                        "{}",
                        at()
                    );
                }
                "rank_results" => {
                    let mut ranked: [u8; 0x40] = t.bytes("pre.ranked").try_into().unwrap();
                    let (key, desc) = (t.get("key").parse().unwrap(), t.get("desc") != "0");
                    rank_results(&mut ranked, t.get("opponents").parse().unwrap(), key, desc);
                    assert_eq!(ranked[..], t.bytes("post.ranked")[..], "{}", at());
                }
                "career_race_payout" => {
                    if t.kv.contains_key("pre.ranked") {
                        let mut ranked: [u8; 0x40] = t.bytes("pre.ranked").try_into().unwrap();
                        let opponents = t.get("opponents").parse().unwrap();
                        if let Some((key, desc)) = payout_ranking(t.get("mode").parse().unwrap()) {
                            rank_results(&mut ranked, opponents, key, desc);
                        }
                        assert_eq!(ranked[..], t.bytes("post.ranked")[..], "{} ranking", at());
                    }
                    // Afterwards `FUN_0812ee14` runs whenever 0x030000A0 is not 0 (the oracle stubs it).
                    if let Some(after) = t.kv.get("after") {
                        assert_eq!(after != "0", t.get("career") != "0", "{}", at());
                    }
                    if t.get("career") != "1" {
                        // Only career events (0x030000A0 = 1) pay.
                        assert_eq!(
                            (t.get("pre.cash"), t.get("pre.events")),
                            (t.get("post.cash"), t.get("post.events"))
                        );
                        *counts.entry(t.func.clone()).or_default() += 1;
                        continue;
                    }
                    let mut save = blank_save();
                    save.cash = t.get("pre.cash").parse().unwrap();
                    save.events.copy_from_slice(&t.bytes("pre.events"));
                    let car: usize = t.get("car").parse().unwrap();
                    let rec: [u8; 17] = t.bytes("records")[17 * car..17 * car + 17].try_into().unwrap();
                    let percent = reward_percent(style_rating(&rom, car, &rec));
                    let (zone, slot) = (t.get("zone").parse().unwrap(), t.get("slot").parse().unwrap());
                    let paid = race_payout(&rom, &mut save, zone, slot, payout_place(&t.bytes("order")), percent);
                    assert_eq!(paid, t.get("paid").parse::<i32>().unwrap(), "{}", at());
                    assert_eq!(
                        (save.cash, save.events.to_vec()),
                        (t.get("post.cash").parse().unwrap(), t.bytes("post.events")),
                        "{}",
                        at()
                    );
                }
                "rebuild_unlocks" => {
                    let mut save = blank_save();
                    save.events.copy_from_slice(&t.bytes("events"));
                    save.field_1f8 = t.get("f1f8").parse().unwrap();
                    let flags = t.ints("flags");
                    // The rebuild tests the RAM flag words for != 0 (the save stores bit 0 of each).
                    save.unlock_flags = (0..6).filter(|&b| flags[b] != 0).map(|b| 1 << b).sum();
                    assert_eq!(save.unlocks(&rom)[..], t.bytes("unlocks")[..], "{}", at());
                }
                "save_encode" => {
                    // The profile, car records and globals as the game held them when it saved (the image is
                    // lossy: fields wider than their bits are cut), encoded over the heap bytes the game wrote into.
                    let (p, cars, g) = (t.bytes("profile"), t.bytes("cars"), t.ints("globals"));
                    let flag = |o: usize| u32_at(&p, o) as u8 & 1;
                    let save = Save {
                        name: p[..8].try_into().unwrap(),
                        cars: std::array::from_fn(|i| cars[17 * i..17 * i + 17].try_into().unwrap()),
                        car_bits: u16_at(&p, 0x12),
                        car_extra: std::array::from_fn(|i| p[0x14 + 15 * i..0x23 + 15 * i].try_into().unwrap()),
                        cash: u32_at(&p, 0xC),
                        car: p[0x10],
                        zone: p[0x1FB],
                        slot: p[0x1FC],
                        wingman: p[0x200],
                        field_254: p[0x254],
                        field_1f8: p[0x1F8],
                        field_f5: p[0xF5..0xF9].try_into().unwrap(),
                        events: p[0x205..0x217].try_into().unwrap(),
                        best_times: std::array::from_fn(|i| u16_at(&p, 0x218 + 2 * i)),
                        options: Options {
                            camera: g[0] as u8,
                            units: g[1] as u8,
                            hud: g[2] as u8,
                            transmission: g[3] as u8,
                            music: (g[4] >> 3) as u8,
                            sfx: (g[5] >> 3) as u8,
                            language: g[6] as u8,
                            catch_up: g[7] as u8,
                            mode_flags: g[8] as u8,
                        },
                        unlock_flags: [0x47C, 0x480, 0x484, 0x48C, 0x488, 0x478]
                            .iter()
                            .enumerate()
                            .map(|(b, &o)| flag(o) << b)
                            .sum(),
                    };
                    let (heap, out) = (t.bytes("heap"), t.bytes("out"));
                    assert_eq!(save.encode(heap[..].try_into().unwrap())[..], out[..], "{}", at());
                    // And a decode of the game's image encodes back to it over the same heap bytes.
                    let back = Save::parse(out[..].try_into().unwrap()).unwrap();
                    assert_eq!(back.encode(heap[..].try_into().unwrap())[..], out[..], "{}", at());
                    // `<log>.sav`, when kept, is the .sav mGBA wrote after the log's last save.
                    let sav = t
                        .file
                        .strip_suffix(".log")
                        .and_then(|f| Some(fixture("race-rules")?.join(f)));
                    if let Some(Ok(sav)) = sav.map(|f| std::fs::read(f.with_extension("sav"))) {
                        last_sav.insert(t.file.clone(), (eeprom_to_buffer(&sav) == out[..], at()));
                    }
                }
                _ => continue,
            }
            *counts.entry(t.func).or_default() += 1;
        }
        for (same, at) in last_sav.values() {
            assert!(same, "{at}: the .sav mGBA wrote is not this image");
        }
        eprintln!("race-rule trace lines checked: {counts:?}");
    }
}
