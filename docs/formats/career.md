# Career, race setup, race rules and the save (Carbon `BN7E`)

**Status:** the tables and the save format are **exact and verified**. Parser: `nfsgba_formats::career` (7 tests).
- The save decodes field for field to the reference run's RAM, including the 40-byte unlock bitfield that the game rebuilds on load.
- The Quick Play variables and the route/environment choice match the reference race's IWRAM.
- The rule functions (payout, hunter life, elimination, progress) are straight transcriptions of the game code. They are **not yet checked against a trace**; see "Not 1:1 / open".

Offsets without a prefix are ROM offsets (GBA address minus `0x08000000`). Functions are GBA addresses. RAM is IWRAM `0x03…` or EWRAM `0x02…`. The **profile struct** is `*0x030056EC` (`0x02000808` in the reference run); "profile `+x`" means an offset into it.

## Career structure

- **6 zones.** Names are `TEXT_ZONE_n_NAME` (keys 963–968, shown as `0x3C3 + zone`) and labels are `TEXT_ZONE_n` (969–974). Zones 1–5 have 12 events each, zone 6 ("The Gauntlet") has 6: **66 events** in total. Event index = `zone × 12 + slot`.
- **Selection:** the zone is profile `+0x1FB`, the slot is profile `+0x388 + zone`, copied to `+0x1FC`. The event screen is `FUN_0812dd80`.

### Event table `0x7E4744` (66 × 8 bytes)

`FUN_0812da08` copies the selected event into the race globals:

| Byte | Meaning | Global |
|---|---|---|
| `+0` | AI skill 0..100; `skill / 35` (unsigned) = difficulty 0..2 | `0x030000BC`; difficulty `0x03005608` |
| `+1` | forward track: slot in the track-name table (0–11 circuits, 24–41 sprints) | — |
| `+2` | mode: 0 circuit, 1 elimination, 2 hunter, 3 sprint (names: `u16` keys at `0x7E5070`) | `0x030056E0` |
| `+3` | reverse (adds 12 to the track slot; never set on sprints) | `0x03005610` |
| `+4` | laps (always 1 on sprints; 2 on every career elimination) | `0x030056E4` |
| `+5` | traffic 0 none, 1 light, 2 heavy | `0x03005604` |
| `+6` | `i16` reward: a cash ladder per zone, +25 per step (zone 1: 100, 150, 175 … 400; zone 2 starts at 350; zone 6 ends at 1,375) | see payout |

- The route number goes to `0x03005388`: `u16` at `0x7E4A70 + 4 × (track + 12·reverse) + 2`.
- The career car (profile `+0x10`) goes to `0x03005718`. `*0x030053BC` goes to byte `0x0300538C + *0x03000060`.
- The opponent count is **not** in the record (see "Not 1:1 / open").

**Boss events** are at `0x7E4714`, 6 × 2 `u16` event indices: (7, 8), (19, 20), (31, 32), (43, 44), (55, 56), (66, 66). Zone 6 has none, since 66 matches no event. The event screen locks boss 1 until unlock `0x122 + zone` and boss 2 until `0x11D + zone`. Boss names are the `u16` keys at `0x7E4954`: CRUNCH, ARJEN, SLY, POORBOY, TRACE, LAYLA, DAEMON, MK, AVI, CLUTCH, EX, then EX padding (16 entries).

### Tracks, route numbers, environments

- **`0x7E4A70` track-name table:** 42 `(u16 text key, u16 route number)` pairs. Slots 0–11 are circuits forward (routes 1, 3, …, 23), 12–23 the same circuits reversed (2, 4, …, 24), 24–41 sprints (25–42).
- **`0x7E49C4`:** `u32` per route number 0..=42, giving its track-name slot (the inverse map).
- **`0x7F2588` race slots**, 44 × 0xC bytes indexed by route number. At race start `FUN_0812b5f0` (screen `0x81`) reads:
  - `+0`: environment, the level-descriptor index (`0x0300006C`); `race_start_from_table_a`/`_b` use `index × 0x68 + 0x7F2B08`;
  - `+1`: route-table index (`0x03005720`).

  Records 1–42 have route = record index; record 0 is the menu scene (environment 12). `+2..+0xB` are not decoded (`00 FF 3D 2A 00…`).
- **Direction fix-up** (`FUN_0812b5f0`):
  - forward: a reverse slot 12–23 has 12 subtracted;
  - reverse: a slot below 12 has 12 added, and a sprint clears the reverse flag.
- **Car at race start:** Quick Play (`0x030000A0` = 0) uses the car at profile `+0x11`; career (`0x030000A0` = 1) uses profile `+0x10`.

Checked: the reference Quick Play race (STORAGE RUN forward) is route number 23, environment 11, route index 23 in IWRAM.

### Wingmen (crew)

- **Names:** `0x7E4974`, 14 `u16` keys (`TEXT_WINGMAN_1..14`: NONE, KITA, DYLAN, JUICE, MARCUS, TURTLE, PIP, LAYLA, TRACE, ARCADY, WRAITH, AVI, CLUTCH, CARTER).
- **Roles:** `0x7E4990`, 13 × `(u16 role key, u16 level)`:
  - NONE for 0, then ATTACKER and DRAFTER alternating, with levels 0,0,1,1,…,5,5;
  - the level is drawn as icon `0x10B + level` (`FUN_08130d8c`).
- The selection menus are `0x7E544C` records 14 (NONE…CLUTCH, 13 entries) and 16 (KITA…CLUTCH, 12). CARTER (the 14th name) is in neither; his unlock would be `0x134`.
- **Selection:** profile `+0x200` (saved). Wingman `w` needs unlock `0x127 + w`.
- **Effect on the grid** (`race_start_from_table_a`, `FUN_08139e34`):
  - `0x03006104 = wingman`;
  - `0x030057EC` (cars besides the player) = opponents + 1 if the wingman is 1..12;
  - above 3 it is clamped to opponents = 2, total = 3.
- The setup screen also caps opponents at 2 when a wingman is set (`FUN_081328f4`, `FUN_08132ab8`).

## Progress, rewards, unlocks

### Event status

2 bits per event at profile `+0x205` (18 bytes; event `i` = byte `i >> 2`, bits `(i & 3) × 2`). Read by `FUN_08135d4c`.

| Value | Meaning |
|---|---|
| 1 | won |
| 2 | second place |
| 3 | not done (a new profile fills the array with `0xFF`) |

`FUN_08135d78` / `FUN_0812fc34` count a zone's completed events (status 1 or 2).

### Race payout (`FUN_0812efe8`, career only)

0. **Ranking** (every race, `rank_results`): the results table at `0x03005730` is sorted first by `FUN_0812e8e4(key, descending)`:
   - hunter (mode 2) by life, most first;
   - circuit, elimination and sprint by time, least first;
   - any other mode value not at all.

   The table holds, per slot: a byte at `+0`, the **entity id** at `+4`, bytes at `+8` and `+0xC`, and `u32` at `+0x10` (best lap), `+0x20` (time) and `+0x30` (hunter life). Key 1 compares the time (signed), key 2 the `+0xC` byte, key 4 the life (unsigned). It is a bubble sort over `opponents + 1` slots with `opponents + 1` passes (`FUN_0812e860` swaps whole rows), so ties keep their order. Nothing else runs unless `0x030000A0` is 1 (a career event). Afterwards `FUN_0812ee14` runs whenever `0x030000A0` is not 0.
1. **Place.** It is 1 if the ranked entity id `0x03005730 + 4` is 0 (the player), 2 if `+5` is 0, otherwise 3 (`payout_place`).
2. **Reward base** = event[`zone·12 + completed − (old status ≠ 3 ? 1 : 0)`].`reward`. `completed` is counted before the update. It is the **ladder position**, not the raced event. Quirk kept: with status 0 (never written by the game, possible in an edited save) and nothing completed, the index is `zone·12 − 1`, the word before the zone's first record.
3. **Status update:** the new place is stored if it is lower (better) than the old status.
4. **Payout** = `percent × base / 100` (signed divide). It is halved for second place and halved again for a replay (old status 1 or 2); it is 0 for place 3. The payout is added to the cash (profile `+0x0C`) and stored at profile `+0x3B8`.
5. **Display:** the event screen shows the same number (`FUN_0812dd80`: `base × percent / 100`, with `base` halved for a replay).

**`percent`** comes from `style_rating` (`FUN_0812c30c`) of the career car's 17-byte record `r`:
- **Start:** 100, or 105 if `r[5] ≠ 0`.
- `r[0]`: +8 / 10 / 14 / 17 for values below 7 / 12 / 15 / above.
- `r[1]`: +8 / 13 / 15 for values below 3 / 5 / above.
- `r[2]`: +4 / 5 / 6 for values below 7 / 12 / above.
- `r[3]`: +`(r[3] + 4 − byte[0x7F0626 + car]) × 15`.
- `r[4] >> 3` = 1, 2, 3, 4: +4, 7, 10, 12.
- Each term applies only when its byte is non-zero.

The rating maps to a percent: below 101 → 100, below 126 → 110, below 151 → 120, below 176 → 130, otherwise 140.

### Unlocks

- **Bitfield:** profile `+0x42D`, 40 bytes. A **set** bit means unlocked. `FUN_0812d7b0(id)` sets a bit; `FUN_0812d784(id)` returns "locked".
- **Rebuild on load:** `FUN_08135958` clears the field and rebuilds it (called on load and on new profile). `Save::unlocks` reproduces it; checked byte-exact against the reference RAM. The rebuild adds, in order:
  1. **Starting set:** `0x108, 0x109, 0x10A, 0x72, 0x76, 0x117, 0x127`; the parts `0, 4, 8, 0xD, …, 0xBF` (the list in `career.rs`); and `0xE0..=0x107`.
  2. **Early wingmen:** profile `+0x1F8` = 1 adds `0x128`; a value above 1 adds `0x128` and `0x129`.
  3. **Progress unlocks** (table at `0x7E4B18`). Records are `u16 key, u16 ids…, 0xFFFF`, and a key of `0xFFFF` ends the table. For each zone, every event that counts moves `key` from `zone·12` up by one; each step grants every record with that key. An event counts with status 1, or with status 2 if it is not a boss event and the zone is not 6.
  4. **Zone and boss unlocks.** If more than 6 events of a zone count, `FUN_0813589c(zone)` runs:
     - zones 1–5 get `0x122 + z` (boss 1 available);
     - boss 1 won adds `0x11D + z` (boss 2 available) and `0x10B + 2z`;
     - boss 2 won adds `0x10C + 2z`, `0x118 + z`, `0x12A + 2z` and `0x12B + 2z`.
  5. **`unlock_flags` ranges** (save `0x11B`, see the save layout; in RAM six `u32` at profile `+0x47C`, `+0x480`, `+0x484`, `+0x48C`, `+0x488`, `+0x478`, each tested `≠ 0`, while the save keeps only bit 0 of each):
     - bit 5: `0x108..=0x116`;
     - bit 4: `0x127..=0x134`;
     - bit 3: `0x117..=0x11C`;
     - bit 1: `0x79..=0x107`;
     - bit 2: `0..=0x78`;
     - bit 0: `0x11D..=0x122` and `0x122..=0x127`.
- **Id ranges:**
  - `0x108 + car`: the 15 cars. Verified: the price lookup `FUN_0812d960` uses a car price table (`0x7E503C`, `i16`) for exactly these ids.
  - `0x127 + w`: wingmen. Verified: `FUN_08130d8c`.
  - `0x11D..0x126`: boss races (verified, see above).
  - `0..0x107`: parts and kits, priced through `(id, price)` pairs at `0x7E4DE4` (not decoded).
  - `0x117..0x11C`: not decoded.

## Quick Play and setup screens

- **Menus** (`0x7E544C`, 0x14-byte records, not decoded further here):
  - the main menu offers Quick Play, Career and My Dash;
  - Quick Play offers RANDOM (action `0xF`) or CUSTOM (action 1), then the race type.
- **Random race** (`FUN_0812ffb0`), all draws from the RNG `FUN_0815fcfc`:

  | Variable | Value |
  |---|---|
  | difficulty | `rand % 3` |
  | catch-up `0x03000050` | `rand & 1` |
  | wingman | `rand % (number of unlocked wingmen)` |
  | opponents | 2 with a wingman, else 3 |
  | laps | `rand % 6 + 1` |
  | traffic | `rand % 3` |
  | `0x0300562C` | `rand % 3 + 1` |
  | `0x03006118` | `rand % 20` |
  | `0x0300580C` | 0 |

  Sprints get 1 lap and elimination gets laps = opponents. The mode and route are drawn in `FUN_081303e8` (mode from the 4 bytes at `0x797D0A`).
- **Setup screens** at `0x7E6260`: 6 headers of 0x10 bytes, `(u16 title key, u16 button key, 0x92, 1, 1, u16 item count, u32 items)`. Items are 0x18 bytes: `(u32 text key, −1, u32 setting id, u32 options → u32 text keys indexed by value, i16 min, i16 max, u32 action)`.

  | Screen | Items (setting id: min..max) |
  |---|---|
  | 0 OPTIONS / SAVE | camera 8, units 9, HUD 10, transmission 11 (0..1); music 12, SFX 13 (0..2: OFF/LOW/HIGH) |
  | 1 RACE INFO | display only (race type, user car, wingman, …) |
  | 2 SETTING CIRCUIT | direction 2 (0..1), laps 3 (1..6), difficulty 4 (0..2), opponents 5 (1..3), traffic 6 (0..2), catch-up 15 (0..1) |
  | 3 ELIMINATION | the same, but laps 1..3; catch-up is listed before traffic |
  | 4 HUNTER | as circuit |
  | 5 SPRINT | difficulty, opponents, traffic, catch-up |

- **Setting id → variable** (`FUN_08132790`). The screen edits profile `+0x3B8 + 4·id`, and `FUN_08132ab8` copies the values out:

  | Id | Variable |
  |---|---|
  | 0 | `0x030056E0` (mode) |
  | 1 | profile `+0x200` (wingman) |
  | 2 | direction `0x03005610` |
  | 3 | laps `0x030056E4` |
  | 4 | difficulty `0x03005608` |
  | 5 | opponents `0x03005784` |
  | 6 | traffic `0x03005604` |
  | 7 | `0x0300580C` |
  | 8 | camera `0x030053E4` |
  | 9 | units `0x03000040` |
  | 10 | HUD `0x03005698` |
  | 11 | transmission `0x03005798` |
  | 12 | music `0x0300578C` (value `<< 3`) |
  | 13 | SFX `0x030053A4` (`<< 3`) |
  | 14 | language `0x03005600` |
  | 15 | catch-up `0x03000050` |
  | 16 | profile `+0x3F8` |

  In elimination, editing laps also sets opponents and vice versa.

Checked: the reference race (screenshot `s11`: forward, 3 laps, easy, 3 opponents, no traffic, catch-up off) has mode 0, direction 0, laps 3, difficulty 0, opponents 3, traffic 0, catch-up 0 in IWRAM, and the same values at profile `+0x3C0..`.

## Race rules

Each car has a driver struct, pointed to by entity `+0x8C` (reference race: `0x0202C624` for the player, `0x0202D168` + 0x500·k for the opponents). Entities are 0xA4 bytes at world `+0x3C`; the player is entity `*0x03000060`, at `*0x030053AC`. `Racer` in `career.rs` holds the fields the rules use; `Race` the globals.

All of this is exact and checked against the game (see "Race-rule checks" below).

### The racing line at race time (`RacingLine::new`)

- **ROM data** (route `+0x04`/`+0x08`, also in `race-routes.md`):
  - The section table is 8 bytes per section: `u16 count, u16 0, u32 first waypoint`. It sits directly before the waypoints, so the section count is `(line − table) / 8`.
  - Section 0 is the lap; its last waypoint repeats the first. Other sections are branches.
  - Waypoint `+0x0C` (`u16` section) and `+0x0E` (`u16` index) link it to another section; `0xFFFF` means none.
- **`race_load_level`** copies 0x50 bytes of sections to world `+0x40` and 0x1800 bytes (256 waypoints) to world `+0x44`, then for a route with a line:
  1. **Sprints** (`FUN_081390b0`, mode 3): sets `0x0300608C` = 0. Every waypoint moves up one slot, and the lap gets a point before its start and one after its end, extrapolated as `5·p − 4·neighbour`. The lap count grows by 2, the branches' `first` by 2, and links in waypoints past the lap move up one index.
  2. **Links** (`FUN_081391f4`): the branch count N is `*0x7F37D8[route]` (a null pointer means 0), stored at `0x03006108`. Every link of sections 0..=N is cleared. Then each branch's start and end link to the nearest point (`(Δx >> 4)² + (Δz >> 4)²`, first found on ties) among the other sections' points `0..count − 2`, and that point links back to (branch, 0) or (branch, count − 1).
  3. **Planes** (`FUN_08138f30`, `FUN_08138dc4`, `FUN_08138c24`): a 0x20-byte row per waypoint in `*0x03005FB4` (`Plane`):
     - `[0..2]` the unit direction to the next point (×256), `[7]` the length;
     - `[2]`/`[3]` two slopes (×0x1000);
     - `[4..6]` the unit normal of the crossing line through the point (the mean of the directions in and out, ×256), `[6]` = normal · point.

     Each row uses points `k − 1 … k + 2` through `racing_line_step`. It builds the lap, then each branch where a lap point forks into it (only when the branch's start is nearer the fork than its end), recursively. `back[branch]` (`*0x03005FB8`, 256 `i32`, reset to −1) = the lap index where it leaves.
     - **History quirk:** the build uses `0x0300608C` as the previous scene left it (0 after power-on and in sprints; the menu's 3D scene and every race set it). With it the lap wraps; without it the ends clamp, so the lap's last row points at itself.
     - **Buffer quirk:** the table is a `malloc(0x2000)`, and rows the build skips (a branch only ever entered at its end, route 21's first branch) keep the old heap contents.
  4. **Distances** (`FUN_0813f744`, in the player's setup `FUN_0814b98c`, right after `FUN_0813e430` set `0x0300608C` = mode ≠ sprint):
     - The lap's distances become running sums of integer lengths (`isqrt`, `FUN_0815fa54`: 16 two-bit steps, 0 → 1), so the first point gets 1. In sprints point 1 restarts at 0.
     - Each branch forked into is measured from 0 and scaled onto the lap between fork and rejoin: `div(span · d, branch_len >> 8) + fork.distance`, with `span = (rejoin − fork) >> 8`. Quirk kept: its first point scales its old value.
     - The scale `div(span << 8, branch_len >> 8)` goes to `0x03006120 + 4·branch` (`[0]` = 0x100).
     - Route 23's lap becomes 108,217 units (ROM: 108,219).
- **`racing_line_step`** (`FUN_0813e860`, `RacingLine::step`): index `i` of section `s`, following links past either end. The lap wraps with period `count − 1` in lapped races and clamps otherwise; a branch clamps at an unlinked end. Quirk kept: stepping back from an unlinked branch start stays in the branch at `back[s] + i`.

### Lap arming (FIDELITY D6)

Driver `+0x4D8` bit 1 ("armed") is set by the racing-line trackers. A lap counts only for an armed car.

- **The player** (`FUN_0813edd8`, `RacingLine::track_player`), every frame from the player's update:
  1. **Wrong way:** `d1` = dot(driver `+0x11C`, row direction) and `d2` = dot(driver `+0x140`, same), each term `(v·w) >> 12`. The row is `first(stepped section) + current segment` (quirk kept). The counter `+0x4EC` goes up by one if `d1 < −10`, or if `d1 < 1` and `d2 < 0`; otherwise it resets to 0. `0x03005384` = counter > 27.
  2. **Advance** past the next point's crossing line (`side = (x >> 8)·n.x + (z >> 8)·n.z − d > 0`):
     - on the lap: segments 1–9 arm the lap, and segments `count − 2` and 0 clear bit 0;
     - the segment becomes `step(seg + 1)` (index only), and `count − 1` becomes 0;
     - in a branch: segment + 1, and the lap is armed.

     Then `lap_crossing` runs.
  3. **Otherwise, behind the current point's line** (`side < 0`):
     - in a branch the segment drops by one;
     - on the lap, segment 2 disarms; segment 1 (sprints) or 0 (lapped) sets bit 0 ("backwards past the start"); and the segment becomes `step(seg − 1)`.

  The player's section changes elsewhere (`FUN_0813f234`, by sector).
- **The AI** (inside the AI driver `FUN_0814d078`, `0x0814D21C..0x0814D3AA`, `RacingLine::ai_advance`):
  1. The driver's side of the next point's line goes to driver `+0x444`. When it is > 0, bit 0 is cleared.
  2. **Move on:**
     - At the section's second-to-last point with a linked next point, it jumps to the link (section and index; driver `+0x4D6` = 2).
     - Otherwise it moves one point; the lap's `count − 1` becomes 0.
  3. Segments 1–7 arm the lap.
  4. **Shortcuts:** before a fork (the next point's link index is 0), a racer that is not leading may roll `rand_table() & 0xFF`. If the roll beats `0x7BFCD4[difficulty]` (255, 192, 100: easy AIs never take shortcuts) and `0x030060C0[branch]` allows it, it takes the branch at segment −1 and arms the lap.
  5. The segment is stored and `lap_crossing` runs.

### Lap crossing, elimination, finish (`FUN_0813f098`, `RacingLine::lap_crossing`)

- **Crossing test**, all required:
  - section 0;
  - segment `count − 1` (lapped) or `count − 2` (sprints), or 0;
  - armed.
- **On a crossing:**
  - disarm;
  - lap time = `0x03005800 − lap start` (unsigned), kept at `+0xB4` when lower or when `+0xB4` is 0;
  - lap start `+0xB8` = now;
  - laps left `+0xC5` − 1.
- **Elimination** (mode 1): if the crossing car's place equals `opponents − (laps − laps left) + 1`, then every racer (from the player's entity on) whose place is one more is knocked out:
  - result byte `0x03005658 + j` = 8;
  - driver `+0x4D8 |= 8`;
  - entity `+0x4A` = 2 and entity `+8 &= 0xFFFB`;
  - driver `+0xF8..+0x100` = the three words at `0x7F3DD0` (all 0).
- **Finish:** at laps left 0, or any crossing in a sprint:
  - `0x030061A4` = 1;
  - entities beyond the racers (id > opponents, e.g. the wingman) get `+0x4A` = 2;
  - racers run `FUN_0813f008`: finish `+0xBC` = lap start, result time `0x03005670 + 4·id` = lap start, `+0x4A` = 2. Quirk kept: in elimination the camera car (`0x030057F8`) also sets result byte `0x03005658 + opponents + 1` = 8 (it computes the last place and ignores it).
- **Race time** `0x03005800` counts frames (incremented in `vblank_irq`, reset in `race_init`). Times display as `frames × 100 / 60` centiseconds (`FUN_08142f74`).

### Positions and progress

- **Progress** (`FUN_081400ec`, `race_progress`): `lap length × (laps − laps left) + distance` (driver `+0xAC`), or the distance alone when not lapped.
- **Places** (`FUN_0813ea04`, every frame, `RacingLine::update_places`):
  - Each racer still racing (`+0x4A` ≠ 2) gets 1 + the number of other racers that are not knocked out (`+0x4D8` bit 3) and are either ahead on progress, level with a lower index, or finished.
  - When `0x03000048` is 9, the places are just the entity order.
- **Finish estimate** (`FUN_0814f050`, `RacingLine::finish_estimate`) for cars still racing at the end:
  - `total = len × laps` and `done = len × (laps − left) + distance`, in 64 bits; `laps − left` is taken unsigned. `total − done` and `done` are each at least 1.
  - time = `(total − done) × elapsed / done + elapsed`, where anything outside 1..=359,999 becomes 359,999. It is stored at `+0xBC`, and the best lap becomes `time / laps` when lower or 0.
  - In sprints it first sets `laps` = 1 and laps left = 1, and measures to the point before the extra end point.

### Hunter life (mode 2; FIDELITY D6)

Life is driver `+0x4E8`, 0..=`0x80000`. `hunter_tuning_init` (`FUN_081412ec`) runs for every race (all 121 dumps hold the same values; `0x030061B0` itself is never written, so a place-0 car such as the wingman gains 0).

- **Per frame** (`FUN_08140f78`, from the player's and the AI's updates in hunter mode):
  - wrong-way counter `+0x4EC` > 27: −1000;
  - otherwise wall counter `+0x4EE` > 50: −100;
  - otherwise, while nobody has finished: + `[0, 200, 150, 100, 0][place]`.

  Results clamp to 0..=`0x80000`.
- **Hits** (`FUN_0814101c`), unless either car's `+0x4A` is 2:
  - damage `impulse · 0x440 >> 8` comes off the victim's life (not below 0);
  - while nobody has finished and the victim's id ≤ opponents, the attacker gains `damage · 3 >> 2` (not above `0x80000`);
  - the attacker's `+0x4F0` is cleared.
- **Two more drains** (`FUN_0814136c` and `FUN_081413b0`, from the collision code `FUN_081457b8` and `FUN_08145dac`), unless `+0x4A` is 2: `0x240 · amount >> 8` (`0x03006184`, `0x030061A0`), not below 0, and `+0x4F0` = 0.
- **Life at zero:** nothing reacts in the race. Every reader of `+0x4E8` is listed here:
  - the tick, the hit and the drains;
  - the HUD bar (`FUN_08142674`: `life · 0x1C >> 0x13`, clamped 0..28);
  - the driver setups, which zero it;
  - the results copy (`FUN_0812eaac` → `0x03005650 + 0x30 + 4·id`).

  A car at 0 keeps racing and keeps gaining by place. Hunter results are ranked by life, most first (the payout's ranking above), so the lowest life ranks last and ties keep their order.
- Hunter also halves one speed term for the leader (`FUN_0813c5a8`). Other tuning words have unknown use: `0x03006170` = `0x80`, `0x03006194` = `0x20`, `0x03006198` = 400.

### Race-rule checks

Two sources, one replay test (`race_rules_match_the_traces` in `career.rs`), which reads every file in `data/work/e5298b24/race-rules/` and skips when there are none.

1. **mGBA traces** (`tools/recorders/rules.lua`, `*.log`). The tracer is loaded through `NFSGBA_MGBA_EXTRA` (tools/mgba_remote.lua) and logs each traced call with the racers and globals before and after:
   - `lap_crossing`, the player's tracker and the AI advance (with the state at a nested `lap_crossing`);
   - `race_progress`, positions, the hunter functions and the finish estimate;
   - payout, style rating, unlock rebuild and `save_encode` (with the heap bytes it overwrote);
   - the plane build.

   An autopilot (`auto on`) steers along the racing line. Captured: the story races (routes 1 and 3, wingmen), a career event (lost, place 3), five game-written saves (two with the `.sav` mGBA wrote).
2. **Oracle cases** (`tools/oracle/cases.py rules`, `oracle-*.jsonl`, the same keys). They run each function in the function oracle (`tools/oracle`, `docs/engine/harness.md`) on generated inputs over the reference race's RAM:
   - random and edge values for places, laps, flags, times, lives, impulses, statuses and records;
   - the racing line of all 43 routes, circuit and sprint, with both build flags, through the real load functions.

| Rule | Traced calls | Oracle cases |
|---|---|---|
| `lap_crossing` (incl. elimination, finish, wingman) | 1,663 | 2,000 |
| player tracker `FUN_0813edd8` | 814 | 2,000 |
| AI advance (`FUN_0814d078` block) | 39,548 | — |
| `race_progress` | 1,430 | 2,000 |
| positions `FUN_0813ea04` | 258 | 2,000 |
| finish estimate | — | 2,000 |
| hunter tick / hit / drains | — | 2,000 each |
| ranking `FUN_0812e8e4` | (in payout) | 2,000 |
| `career_race_payout` | 4 (one career event, flag 1) | 3,000 (652 wins, 690 second places paid) |
| `style_rating` | 336 | 2,000 |
| `rebuild_unlocks` | 8 | 2,000 |
| `save_encode` | 5 (+ 2 `.sav`) | 2,000 |
| racing line, planes, distances | 3 builds, 1 mid-race | 43 routes × 2 × 2 |

Every line matches. The AI advance block can't be called on its own, so it is checked only on traces (39,548 calls, including branch choices). Not captured: a career **win** in the emulator (the autopilot is about 30% slower per lap than the skill-0 AI); the paying payout paths and saves with won events are covered by the oracle cases only.

## Save (EEPROM)

- **Hardware.** 4 Kbit EEPROM, 64 blocks of 8 bytes, over DMA3 at `0x0D000000`. The type descriptor at `0x7BFD0C` (`{0x200, 64 blocks, …, 6 address bits}`) is selected by `FUN_081510cc(4)`; the 64 Kbit descriptor at `0x7BFD18` is unused.
  - **I/O routines:**
    - block read `FUN_08151194`;
    - compare `FUN_081513b8`;
    - write `FUN_08151244`, with write+verify ×3 in `FUN_08151410`;
    - the load path reads each block up to 50 times.
  - While it runs, IE `&= 0xFF5A` masks VBlank, VCount, timer 2 and serial. Afterwards IE is restored and WAITCNT is set to `0x4014`.
- **One profile.** The code addresses `slot × 64` blocks, but only slot 0 fits and the game only uses slot 0.
  - **Boot probe** `FUN_08149b94(0)`: accepts the profile if the checksum and version match. It copies the name to profile `+0x496` and the language (`0xBE` bits 4–6) to `+0x4E8` and then to `0x03005600`.
  - **Load** `FUN_08149d84`: checksum only, then `FUN_08149820` (decode) and `FUN_08135958` (unlocks).
  - **Save** `FUN_08149fd8`: `FUN_081492c0` (encode) plus the writes. It is called from `FUN_081303e8`, `FUN_08132ab8`, `FUN_081338e0` and `FUN_08134eb8`. A failed load resets the profile (`FUN_081356dc`) and saves.
- **Byte order.** Each 64-bit block travels most significant bit first, and the read routine stores it as four `u16` from the top. So the game's buffer is the mGBA `.sav` with **every 8-byte block reversed** (`eeprom_to_buffer`).
  - The mGBA Lua `eeprom` memory domain (`race.eeprom.bin`) is **not** the save: it equals the first 512 bytes of the BIOS.
- **Checksum.** `u16` at `0x100` = (sum of all 512 bytes, with `0x100..0x101` zeroed) + `0xBADD`, mod 2¹⁶. The version is the `u16` at `0x102` = 9.

### Layout (game order) → RAM, from `FUN_081492c0` (write) and `FUN_08149820` (read)

| Offset | Bits | RAM |
|---|---|---|
| `0x00..0xB3` | 15 cars × 12 bytes, bit-packed (below) | car records `*0x0300539C` = profile `+0xF9 + 17·car`; bit 7 of byte 11 → bit `car` of profile `+0x12` |
| `0xB4..0xBB` | name, 8 bytes | profile `+0x00` (NUL at `+0x08`) |
| `0xBC` `u16` | bits 0–2 zone, 3–6 slot, 7–10 career car | `+0x1FB`, `+0x1FC`, `+0x10` |
| `0xBD` | bits 3–4 music, 5–6 SFX (0..2), 7 camera | `0x0300578C`/`0x030053A4` (= value `<< 3`), `0x030053E4` |
| `0xBE` `u16` | bit 0 units, 1 HUD, 2 transmission, 3 catch-up, 4–6 language, 7–10 wingman | `0x03000040`, `0x03005698`, `0x03005798`, `0x03000050`, `0x03005600` (boot), `+0x200` |
| `0xBF` | bits 3–6 | `+0x254` (unknown) |
| `0xC0` `u16` + `0xC2` bit 0 | cash, 17 bits | `+0x0C` |
| `0xC3` | bits 4–7: one bit per race mode | `0x03000070` (set by `FUN_08134eb8`, tested by `FUN_0812cf48`) |
| `0xC4..0xFF` | 30 `u16`: record time per track (12 circuits, then 18 sprints), in frames | `+0x218`; a new profile gets `10800 + rand % 3600` each |
| `0x100` | checksum | — |
| `0x102` | version 9 | — |
| `0x104..0x107` | 4 bytes | `+0xF5` (unknown) |
| `0x108..0x119` | event status, 2 bits per event | `+0x205` |
| `0x11A` | byte | `+0x1F8` (early wingmen, see unlocks) |
| `0x11B` | bits 0–5 → `+0x47C`, `+0x480`, `+0x484`, `+0x48C`, `+0x488`, `+0x478` | unlock-everything flags |
| `0x11C..0x1FC` | 15 cars × 15 bytes | profile `+0x14 + 15·car` |

**Unused bits** are never written by the encoder; they keep whatever the 0x200-byte heap buffer held:
- `0xBF` bit 7;
- `0xC2` bits 1–7 and `0xC3` bits 0–3;
- `0x103` (written as 0);
- `0x11B` bits 6–7;
- `0x1FD..0x1FF`.

**Car record packing** (12 bytes `p` → 17-byte RAM record `r`):
- `r[7..=10]` are four 7-bit fields from `p[0..4]`, least significant bit first; `r[0]` = `p[3] >> 4`.
- `r[11..=14]` are likewise from `p[4..8]`; `r[2]` = `p[7] >> 4`.
- `r[15]` = `p[8] & 0x1F`, `r[16]` = `(p[9] & 3) << 3 | p[8] >> 5`.
- `r[6]` = `p[9] bits 2–6`, `r[5]` = `(p[10] & 0xF) << 1 | p[9] >> 7`.
- `r[1]` = `p[10] bits 4–6`, `r[3]` = `(p[11] & 1) << 1 | p[10] >> 7`.
- `r[4]` = `p[11] bits 1–6`, owned/flag = `p[11] >> 7`.

`r[6]` defaults to the per-car bytes at `0x7EEA33` (5, 6, 11, 13, …). The style rating reads `r[0..=5]`. The other fields are presumably the upgrade levels (10 performance categories, visual parts), but that is **inferred, not verified**.

**New profile** (`FUN_081356dc`):
- **Globals:** units = language ≠ 0 (KM/H outside English), camera 0, HUD 1, transmission 1, music and SFX 16 (HIGH), catch-up 1, `0x03000070` = 0.
- **Profile:**
  - all events at status 3;
  - car records zeroed, then `r[6]` from `0x7EEA33`;
  - cash 0;
  - record times random.

Checked: the reference `.sav` (profile "A", created before the Quick Play race) decodes to exactly the profile, car records, flags and globals in `race.wram.bin`/`race.iwram.bin`. The one exception is catch-up: the save holds the default 1, and the race setup set the global to 0 without saving.

### Encoder (`Save::encode`, FIDELITY D7)

- **Write order.** `Save::encode(heap)` follows `save_encode` (`FUN_081492c0`) in its write order.
- **Heap buffer.** The game encodes into a fresh `malloc(0x200)` (`save_write_profile`), so the unused bits above come from `heap`.
- **Lossy image.** Fields wider than their bits are cut: the car records' 5-, 6- and 7-bit fields, camera bit 0, and bit 0 of each unlock flag word. Decoding a save therefore need not give back the RAM that was saved.
- **Checks:**
  - five game-written saves (after the story races and the career event), encoded from the RAM at `save_encode`'s entry over the heap bytes it overwrote, are byte-identical, and so are the two `.sav` files mGBA wrote (`eeprom_to_buffer`, its own inverse);
  - 2,000 oracle cases with random profiles, car records, flag words and globals match;
  - decode then encode round-trips the reference `.sav`.

## Not 1:1 / open

- **Opponent count and AI for career events.** The event record has none; where career races set `0x03005784` and the AI skill use of `0x030000BC` are not located. Boss races presumably differ.
- **Unknowns:**
  - `RaceSlot.rest` (`0x7F2588 + 2..`);
  - profile `+0x254`, `+0xF5..`, `+0x3F8`, `0x0300580C`;
  - the style-record field names;
  - the hunter tuning words of unknown use (`0x03006170`, `0x03006194`, `0x03006198`);
  - unlock ids `0x117..0x11C` and the parts ids;
  - the ranked results' bytes at `+0` and `+0xC` (key 2 sorts by the latter; the payout never uses it);
  - what `FUN_0812ee14` does after a payout (outside the race rules).
- **The player's section changes** (`FUN_0813f234`, by sector membership) are not ported; `track_player` takes the section as given.
- **Stale plane rows.** Rows the build skips hold whatever the heap had; the rewrite needs the previous buffer contents (the previous race's table, if the allocator returns the same block) to be exact in a branch that is only entered at its end. Unmeasured.
