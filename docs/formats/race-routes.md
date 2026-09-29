# Race routes (Carbon `BN7E`)

**Status: the structure is verified against the reference race.** Parser: `nfsgba_formats::routes` (test `routes_match_the_reference_race`). The viewer draws a route's racing line and puts four cars on its grid (R cycles routes).

## Route table

- **Location:** `0x7F2798` holds 44 records of 0x14 bytes. The records end where `+0x10` stops being zero; the level descriptors follow.
- **Loading:** race init `FUN_08139454` takes the record `global route index × 0x14` (IWRAM `0x03005720`; the reference Quick Play race used **route 23**).

| Offset | Meaning |
|---|---|
| `+0x00` | four **template entities** (0xA4 bytes each), copied into the entity array (world `+0x3C`). Position at `+0x0C/+0x10/+0x14` in 8.8 fixed point, city units |
| `+0x04` | the **section table** (world `+0x40`): 8 bytes per section, `u16 count, u16 0, u32 first waypoint index`; it sits right before the racing line, so there are `(line − table) / 8` sections (`docs/formats/career.md`) |
| `+0x08` | the **racing line** (world `+0x44`, a fixed 0x1800-byte copy) |
| `+0x0C` | world counts (sectors, walls, entities = 4, materials, …), the same in every record |
| `+0x10` | 0 |

- **Start grid:** the four template entities are the player and three opponents, side by side across the road. Route 23 has them all at x = 118,400, with z = −64,320, −64,640, −65,120 and −65,600. In the reference race the player's live position at the start matched this.

## Racing line

24-byte waypoints: `(i32 x, i32 z, ?, u16 link section, u16 link index, i32 cumulative distance, i32 sector)`, split into sections by the `+0x04` table.
- **Section 0 is the lap.** Its last waypoint repeats the first, and that waypoint's distance is the lap length. The distance is measured from the first waypoint and matches the waypoint spacing exactly.
- **Sections 1.. are shortcut branches.** A link (`0xFFFF` = none) points to the same place in another section: a fork on the lap points to the branch's first waypoint, and the branch's first and last waypoints point back to where it leaves and rejoins the lap.
- The `?` field is usually 0; some are large flag-like values (unknown).
- `routes()` returns the lap as `waypoints` and the branches as `branches`.

Route 23 has a 36-waypoint lap of 108,219 units (a circuit in the south-east district) and one 8-waypoint branch, from lap waypoint 19 to lap waypoint 27.

## Routes and races

- **Modes:** Circuit, Elimination, Hunter and Sprint (text keys `TEXT_RACE_SETTING_*`, `TEXT_RACE_DESC_*`).
- **Track names:** 10 `TEXT_TRACK*` circuits and 18 `TEXT_ROUTE*` sprints.
- **Pairing:** routes mostly come in pairs sharing a start sector (forward and reverse, per the Quick Play "direction" option). Routes 15/16 and 21/22 close exactly (closing gap 0).
- **Names and kinds (verified):** three `(u16 text key, u16 route)` tables:
  - `0x7E4A70`: 12 circuits, forward (routes 1, 3, …, 23);
  - `0x7E4AA0`: the same 12 circuits, reverse (routes 2, 4, …, 24);
  - `0x7E4AD0`: 18 sprints (routes 25–42).

  The reference Quick Play race was **route 23, STORAGE RUN, forward**. Routes 0 and 43 are unnamed: 0 has no racing line, and 43 repeats route 1's data. The list is in `nfsgba_formats::routes` (`name`, `kind`).
- **Still open:** how Elimination and Hunter events pick routes.

| Route | Name | Kind |
|---|---|---|
| 0 | — | unnamed |
| 1 | SHIPYARD CRUISE | circuit |
| 2 | SHIPYARD CRUISE | circuit, reverse |
| 3 | LONGPOINT | circuit |
| 4 | LONGPOINT | circuit, reverse |
| 5 | JUNKTOWN BLITZ | circuit |
| 6 | JUNKTOWN BLITZ | circuit, reverse |
| 7 | EAST TUNNEL | circuit |
| 8 | EAST TUNNEL | circuit, reverse |
| 9 | UNIVERSITY DRIVE | circuit |
| 10 | UNIVERSITY DRIVE | circuit, reverse |
| 11 | CROSSOVER | circuit |
| 12 | CROSSOVER | circuit, reverse |
| 13 | BIG EAST HWY | circuit |
| 14 | BIG EAST HWY | circuit, reverse |
| 15 | SUMMIT DRIVE | circuit |
| 16 | SUMMIT DRIVE | circuit, reverse |
| 17 | MOUNTAIN SPEEDZONE | circuit |
| 18 | MOUNTAIN SPEEDZONE | circuit, reverse |
| 19 | SOUTHSIDE | circuit |
| 20 | SOUTHSIDE | circuit, reverse |
| 21 | PARKSIDE | circuit |
| 22 | PARKSIDE | circuit, reverse |
| 23 | STORAGE RUN | circuit |
| 24 | STORAGE RUN | circuit, reverse |
| 25 | JUNKPOINT | sprint |
| 26 | DOWNTOWN SPRINT | sprint |
| 27 | LONGPOINT DASH | sprint |
| 28 | BIG EAST TUNNEL | sprint |
| 29 | EAST HIGHWAY | sprint |
| 30 | CROSS HIGHWAY | sprint |
| 31 | CROSSDRIVE | sprint |
| 32 | LIBRARY CRUISE | sprint |
| 33 | DOUBLE SWITCH | sprint |
| 34 | SHIPYARD DRIVE | sprint |
| 35 | SUMMIT CRUISE | sprint |
| 36 | SHIPYARD SPRINT | sprint |
| 37 | MOUNTAIN SIDE | sprint |
| 38 | PARK ZONE | sprint |
| 39 | CROSS TOWN SPRINT | sprint |
| 40 | SOUTH RUN | sprint |
| 41 | STORAGE SIDE | sprint |
| 42 | SIDE RUN | sprint |
| 43 | — | unnamed |

## Open

- How career events choose route, mode (Elimination, Hunter) and environment. Some sectors carry a track name key at `+0x10` (e.g. sector records near `0x72C8C2`); purpose unknown.
- The waypoint `?` field, and the template entity fields besides position (start sector at `+0x74`).
