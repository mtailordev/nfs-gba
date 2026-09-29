# Address map (Carbon `BN7E` v0, SHA-1 `e5298b24…`)

One place for everything located so far: ROM offsets, RAM addresses and struct fields, and functions. Details and evidence are in `docs/formats/*.md`. Functions are also in [symbols.csv](symbols.csv), which `tools/ghidra/ApplySymbols.java` applies to the Ghidra project.

**Keep this current:** every new find goes here and into `symbols.csv` in the same commit.

ROM offsets are file offsets (GBA address minus `0x08000000`). "rec" is the level descriptor, "world" the world struct.

## ROM

| Offset | Size | What | Doc |
|---|---|---|---|
| `0x000000` | 0xC0 | cartridge header; entry `b 0x080000C0` | recon/ROM-INVENTORY |
| `0x02C000–0x128000` | | PCM-like audio bank (samples, unverified) | recon/FIRST-LOOK |
| `0x04EA14…0x0507FC` | 5 | `GBAMOD30` music modules (Logik State `LS_Play`) | formats/text-table |
| `0x128000–0x16C244` | | Thumb game code (ends with `memset`) | recon/FIRST-LOOK |
| `0x14FC38`, `0x165154`, `0x168264` | | ARM code copied to IWRAM for races (`0x03000000 + off − 0x164F14` etc.) | below |
| `0x16C244–0x402000` | 294 blobs | BIOS-LZ77 image bank | formats/lz77-images |
| `0x33EF14` | | menu palettes | |
| `0x350000–0x368000` | 96 KiB | ARM code, purpose unknown (not the race IWRAM code) | |
| `0x370550` | | vehicle texture base (rec `+0x0C`) | formats/vehicle-models |
| `0x45F5C0` | 146 × 0x24 | vehicle materials (rec `+0x20`) | formats/vehicle-models |
| `0x460A6C` | 102 × 40 | vehicle models (rec `+0x34`) | formats/vehicle-models |
| `0x461A5C` | 3,933 × 6 | model vertices, int16 xyz (rec `+0x38`) | formats/vehicle-models |
| `0x46768C` | u16 | model vertex indices (rec `+0x3C`) | formats/vehicle-models |
| `0x46E4DC` | u16 | model UV indices (rec `+0x40`) | formats/vehicle-models |
| `0x47532C` | 4,651 × 4 | model UVs, 1.15 fixed point (rec `+0x44/+0x48`) | formats/vehicle-models |
| `0x479BD8` | 1 B/poly | model polygon sizes (rec `+0x4C/+0x54`) | formats/vehicle-models |
| `0x47A9A4` | 276 B | unknown (rec `+0x50`) | |
| `0x47A9B8` | | palette block of the variant descriptor | |
| `0x47BC6C` | | city texel base (rec `+0x08`) | formats/city-sectors |
| `0x71F1E8` | 14 × 0x200 | city palettes (rec `+0x00`); an environment picks `+0x5A × 2` | formats/city-sectors |
| `0x720DE8` | 227 × 0x24 | city materials (rec `+0x1C`) | formats/city-sectors |
| `0x722DD4` | | unknown (rec `+0x28` → world `+0x2C`) | |
| `0x723DD4` | 1,113 × 0x30 | sectors (rec `+0x18`) | formats/city-sectors |
| `0x730E84` | 4,423 × 0x44 | walls (rec `+0x14`) | formats/city-sectors |
| `0x77A000–0x78E000` | | route data: template entities, racing lines | formats/race-routes |
| `0x797D10–0x7E53EC` | | text strings | formats/text-table |
| `0x7988B9` / `0x7C0360` | | "Pocketeers" / "LS_Play (C) Logik State 2003" | |
| `0x7C45F0` | 32,767 × 4 | reciprocal table: entry k = 2^24/(k+1) (light interpolation, likely more) | FIDELITY R2 |
| `0x7E4A70` / `0x7E4AA0` / `0x7E4AD0` | 12 / 12 / 18 × 4 | route name tables: `(u16 text key, u16 route)` for circuits forward, circuits reverse, sprints | formats/race-routes |
| `0x7E6EEC` | 20 × 0x80 | car paint presets | formats/vehicle-models |
| `0x7E86A0` | 5,867 × 4 | text table: 977 keys, 5 × 977 strings, 5 module pointers | formats/text-table |
| `0x7F0BD8` | 15 × 0x58 | car table | formats/vehicle-models |
| `0x7F2798` | 44 × 0x14 | route table | formats/race-routes |
| `0x7F2B08` | 12 × 0x68 | level descriptors = environments (plus variant at `0x7F2FE8`) | formats/city-sectors |
| `0x7F38B8` | 65 × 4 | Thumb function table (states or menus?) | |

### Level descriptor (0x68 bytes, `0x7F2B08`)

| Offset | → world | What |
|---|---|---|
| `+0x00` | `+0x30` | city palette block; the loaded palette is `+0x00 + (+0x5A) × 2` |
| `+0x04` | `+0x34` | second palette block (`+ (+0x58) × 2`), probably HUD/objects |
| `+0x08` | `+0x00` | city texel base |
| `+0x0C` | `+0x04` | vehicle texel base |
| `+0x10` | `+0x08` | ? (`0x347B74`) |
| `+0x14` | `+0x10` | walls |
| `+0x18` | `+0x14` | sectors |
| `+0x1C` | `+0x20` | city materials |
| `+0x20` | `+0x24` | vehicle materials |
| `+0x24` | `+0x28` | ? (`0x36CF5C`) |
| `+0x28` | `+0x2C` | ? (`0x722DD4`) |
| `+0x2C`, `+0x30` | | ? (read by the race init) |
| `+0x34…+0x54` | `+0x7C…+0x9C` | vehicle model bank arrays |
| `+0x58` | | u16 second-palette index × 0x100 |
| `+0x5A` | | u16 city palette index × 0x100 |
| `+0x5E` | | u16 sky gradient material |
| `+0x60` | | u16 skyline material |
| `+0x62`, `+0x64` | | ? (4, 0x19) |

## RAM (reference race, `data/work/e5298b24/mgba/race.*`)

| Address | What |
|---|---|
| `0x03000000…` | race IWRAM code (renderer); see Functions |
| `0x030000C0` | **world struct** (below) |
| `0x03000060` | u32 player entity index (0) |
| `0x03005614` | u32 player's current sector (760 at the start of the reference race) |
| `0x03005720` | u32 current route index (23 in the reference race) |
| `0x030055F0` | pointer to the base palette buffer (`0x02001008` in the race) |
| `0x03005630` | pointer to the second palette buffer |
| `0x0201431C` | entity array in the reference race (world `+0x3C`) |
| `0x0201EC24` | vehicle matrix buffer in the reference race (world `+0xFC`) |
| palette RAM `0x05000000` | BG palette. Slots 1–143 and 149–255 are tinted by light (`FUN_0813a514`); 192–223 hold the player car (atlas pixel `i` → `192 + (i ^ 16)`); 160–192 other cars?; 248–255 HUD? |

### World struct (`0x030000C0`)

| Offset | What |
|---|---|
| `+0x00` | city texel base |
| `+0x04` | vehicle texel base |
| `+0x0C` | u16 per sector: sector → entity list head (`0xFFFF` = none) |
| `+0x10` / `+0x14` | walls / sectors |
| `+0x18` | 0x20-byte records (wall `+0x2A`: moving pieces?) |
| `+0x1C` | 0x14-byte records (sector `+0x0A`) |
| `+0x20` / `+0x24` | city / vehicle materials |
| `+0x30` / `+0x34` | loaded palettes |
| `+0x38` | route template entities |
| `+0x3C` | entity array (0xA4 each) |
| `+0x40` / `+0x44` | route 0x50 block / racing line (0x1800) |
| `+0x48` | per-material runtime entries (8 bytes: `+4`, `+6` texture scroll) |
| `+0x50` | view struct (`+0x08`/`+0x0A` screen centre, `+0x10` near limit, `+0x1C` focal) |
| `+0x54` | camera matrix: 12 × i32, 3×3 rotation in 2.14 fixed point then translation |
| `+0x68` | wall draw buffer (0x1A00; 0x34 bytes per wall) |
| `+0x6C` | floor span buffer (0x780) |
| `+0x78` | entity handler table (`FUN_03001ae4` calls `[+0x78][entity +0x4E]`) |
| `+0x7C…+0x9C` | vehicle model bank arrays |
| `+0xA0` | projected vertex buffer (x, y shorts) |
| `+0xD8`, `+0xDA`, `+0xDC` | counts: materials, sectors, walls |
| `+0xE2…+0xE8` | screen clip bounds |
| `+0xF2` | floor span count |
| `+0xF8`, `+0xFA` | entity count (4) and extra entity slots (0x20) |
| `+0xFC` | vehicle matrix buffer (0x30 per slot: rotation, translation) |

### Entity (0xA4 bytes, world `+0x3C`)

| Offset | What |
|---|---|
| `+0x00` | index |
| `+0x04` | next in the sector's draw list (`0xFFFF` ends it) |
| `+0x08` | flags? |
| `+0x0A` | flags (bit 0: use world `+0x58/+0x5A`; bits 1, 3, 4, 6: LOD and texture) |
| `+0x0C/+0x10/+0x14` | position x, y, z in 8.8 fixed point, city units |
| `+0x28` | depth-sort key (distance squared) |
| `+0x36` | LOD piece count? |
| `+0x44/+0x46/+0x48` | material selection (`+0x48` = first atlas; 8 = Cobalt) |
| `+0x64` | second LOD piece offset? |
| `+0x74` | start sector (template entities) |
| `+0x84` | RAM pointer to the unpacked texture |
| `+0x88` | model slot (`0xFF` = none) |
| `+0x89` | index into a 0x11-byte table (car or paint?) |

## Functions

Names are ours; addresses are the Ghidra `FUN_xxxxxxxx`. The IWRAM ones exist in the Ghidra project because the race IWRAM dump is loaded; `FUN_08166c04` = `FUN_03001cf0` (ROM copy, delta `0x164F14`).

See [symbols.csv](symbols.csv) for the full list with one-line descriptions.
