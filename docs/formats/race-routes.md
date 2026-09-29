# Race routes (Carbon `BN7E`)

**Status: the structure is verified against the reference race.** Parser: `nfsgba_formats::routes` (test `routes_match_the_reference_race`). The viewer draws a route's racing line and puts four cars on its grid (R cycles routes).

## Route table

- **Location:** `0x7F2798` holds 44 records of 0x14 bytes. The records end where `+0x10` stops being zero; the level descriptors follow.
- **Loading:** race init `FUN_08139454` takes the record `global route index × 0x14` (IWRAM `0x03005720`; the reference Quick Play race used **route 23**).

| Offset | Meaning |
|---|---|
| `+0x00` | four **template entities** (0xA4 bytes each), copied into the entity array (world `+0x3C`). Position at `+0x0C/+0x10/+0x14` in 8.8 fixed point, city units |
| `+0x04` | 0x50 bytes copied to world `+0x40` (unknown, probably checkpoints) |
| `+0x08` | the **racing line** (world `+0x44`, a fixed 0x1800-byte copy) |
| `+0x0C` | world counts (sectors, walls, entities = 4, materials, …), the same in every record |
| `+0x10` | 0 |

- **Start grid:** the four template entities are the player and three opponents, side by side across the road. Route 23 has them all at x = 118,400, with z = −64,320, −64,640, −65,120 and −65,600. In the reference race the player's live position at the start matched this.

## Racing line

A list of 24-byte waypoints: `(x, z, ?, -1, cumulative distance, sector)`.
- The distance is measured from the first waypoint and matches the waypoint spacing exactly.
- The `?` field is usually 0; a few are large flag-like values (unknown).
- The list ends at the first record that breaks the pattern.

Route 23 has 19 waypoints, a 58,231-unit line plus a closing straight: a circuit in the south-east district.

## Routes and races

- **Modes:** Circuit, Elimination, Hunter and Sprint (text keys `TEXT_RACE_SETTING_*`, `TEXT_RACE_DESC_*`).
- **Track names:** 10 `TEXT_TRACK*` circuits and 18 `TEXT_ROUTE*` sprints.
- **Pairing:** routes mostly come in pairs sharing a start sector (forward and reverse, per the Quick Play "direction" option). Routes 15/16 and 21/22 close exactly (closing gap 0).
- **Not decoded:** which name and mode go with which route. Route 0 has no racing line; 35 has a single waypoint.

## Open

- The name and mode of each route, and how the level descriptors (`0x7F2B08`, 13 events) select routes.
- The 0x50-byte `+0x04` block (checkpoints?), the waypoint `?` field, and the template entity fields besides position (start sector at `+0x74`).
