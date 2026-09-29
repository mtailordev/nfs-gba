# Address map (Carbon `BN7E` v0, SHA-1 `e5298b24…`)

One place for everything located so far: ROM offsets, RAM addresses and struct fields, and functions. Details and evidence are in `docs/formats/*.md`. Functions are also in [symbols.csv](symbols.csv), which `tools/ghidra/ApplySymbols.java` applies to the Ghidra project.

**Keep this current:** every new find goes here and into `symbols.csv` in the same commit.

ROM offsets are file offsets (GBA address minus `0x08000000`). "rec" is the level descriptor, "world" the world struct.

## ROM

| Offset | Size | What | Doc |
|---|---|---|---|
| `0x000000` | 0xC0 | cartridge header; entry `b 0x080000C0` | recon/ROM-INVENTORY |
| `0x000210` | 4 + 40 × 24 | sound-effect table (u32 count, then entries) | formats/audio |
| `0x0005D4–0x04EA11` | | sound-effect sample data (signed 8-bit PCM) | formats/audio |
| `0x04EA14`, `0x04F134`, `0x04F8DC`, `0x050034`, `0x0507FC` | 5 | `GBAMOD30` music modules, music ids 0–4 (Logik State `LS_Play`) | formats/audio |
| `0x050FD4` | 256 × 16 | sample bank headers (26 used) | formats/audio |
| `0x051FD4–0x12A84B` | | sample bank data (signed 8-bit PCM), in header order | formats/audio |
| `0x12A84C–0x16C244` | | Thumb game code (starts at `FUN_0812a84c`, ends with `memset`) | recon/FIRST-LOOK |
| `0x151E34` | | the loader's `"GBAMOD30"` literal (not a module) | formats/audio |
| `0x153BB4` | | note → period table, linear pitch mode (unused by Carbon) | formats/audio |
| `0x154354…` | | per-rate note → step tables, linear mode (pointers at `0x7F5BA4`) | formats/audio |
| `0x15CF2C` / `0x15CFD4` | | LZ77-packed ARM mixer, mode 0 / **mode 1** (→ IWRAM `0x03005A00`, 0x3EC bytes) | formats/audio |
| `0x14FC38`, `0x165154`, `0x168264` | | ARM code copied to IWRAM for races (`0x03000000 + off − 0x164F14` etc.) | below |
| `0x16C244–0x402000` | 294 blobs | BIOS-LZ77 image bank | formats/lz77-images |
| `0x33EF14` | | menu palettes | |
| `0x350000–0x368000` | 96 KiB | ARM code, purpose unknown (not the race IWRAM code) | |
| `0x36C75C` | | second palette block (rec `+0x04`; literal in `load_car_palettes`) | formats/car-paint |
| `0x36C95C` | 0x20 each | car paint ramps (block `+0x200`), 16 colours per paint number; glass = colour 12 | formats/car-paint |
| `0x36CD5C` | 8 × 2 B rows | extra rows for palette slots 240 and 248 (block `+0x600`) | formats/car-paint |
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
| `0x7BFD0C` / `0x7BFD18` | | EEPROM 4 Kbit / 64 Kbit descriptors (`eeprom_select_type`) | formats/career |
| `0x7BFD40` | 32 B | vibrato half sine | formats/audio |
| `0x7BFD60` | 768 × u16 | frequency table, one octave (period mode) | formats/audio |
| `0x7C0390` / `0x7C03A2` / `0x7C03B4` | 8 × u16 / 8 × u16 / 8 × u32 | mixing rates: timer 0 reload / samples per frame / Hz (index 0: 10512 Hz, 176) | formats/audio |
| `0x7C03F0` | 256 × u16 | random table (`rand_table`, index `0x030064C8`) | formats/car-paint |
| `0x7C05F0` | 0x2000 × i16 | sine table, half wave, 0x4000 = 1.0 (`sin_q14`) | formats/car-paint |
| `0x7C45F0` | 32,767 × 4 | reciprocal table: entry k = 2^24/(k+1) (light interpolation, likely more) | FIDELITY R2 |
| `0x7E4714` | 6 × 2 u16 | boss event pairs per zone | formats/career |
| `0x7E4744` | 66 × 8 | career event table (mode, track, reverse, laps, traffic, AI skill, reward) | formats/career |
| `0x7E4954` | 16 × u16 | boss name keys | formats/career |
| `0x7E4974` / `0x7E4990` | 14 × u16 / 13 × 4 | wingman name keys / (role key, level) | formats/career |
| `0x7E49C4` | 43 × u32 | route number → track-name slot | formats/career |
| `0x7E4A70` / `0x7E4AA0` / `0x7E4AD0` | 12 / 12 / 18 × 4 | route name tables: `(u16 text key, u16 route)` for circuits forward, circuits reverse, sprints | formats/race-routes |
| `0x7E4B18` | | progress unlock records | formats/career |
| `0x7E4DE4` | | `(id, price)` pairs for parts (not decoded) | formats/career |
| `0x7E503C` | 15 × i16 | car prices | formats/career |
| `0x7E5070` | 4 × u16 | race mode name keys | formats/career |
| `0x7E544C` | 0x14 each | menu records | formats/career |
| `0x7E5E54…0x7E5EFF` | | setup option key lists | formats/career |
| `0x7E6260` | 6 × 0x10 | setup screens (items 0x18 each) | formats/career |
| `0x7E6EEC` | 20 × 0x80 | paint presets, used only by a menu function (race colours come from `0x36C95C`) | formats/vehicle-models |
| `0x7E86A0` | 5,867 × 4 | text table: 977 keys, 5 × 977 strings, 5 module pointers | formats/text-table |
| `0x7EE238` | 5 × 4 | music table: module pointers | formats/audio |
| `0x7EE24C` | 40 B | Carbon sound id → sound-effect slot | formats/audio |
| `0x7EEA24` | bytes | special ramp numbers: by `cars[1] − 15` (slot 160) or `cars[0]` (slot 208, paint ≥ 20) | formats/car-paint |
| `0x7EEA33` | per car | new-profile per-car record `[6]` defaults | formats/career |
| `0x7EEB70` | 0x9C per car | i16 (x, y) of decal-set materials in the atlas | formats/car-paint |
| `0x7EEBBC` | 6 per set | decal sets: 3 i16 vehicle materials per record `[4]` | formats/car-paint |
| `0x7EF5A0` / `0x7EF672` | 7 per car | overlay material per `(car·7 + rec[1])` / its (x, y) | formats/car-paint |
| `0x7EF816` | 0x10 per entry | decals per `(car·15 + rec[2])`: two (x, y, material) placements | formats/car-paint |
| `0x7F0626` | per car | style base byte | formats/career |
| `0x7F0BD8` | 15 × 0x58 | car table (`+0x0C` first material, `+0x0E` palette bank = 1 for all) | formats/vehicle-models, formats/car-paint |
| `0x7F2588` | 44 × 0xC | race slot per route number: environment, route index (`race_setup_route`) | formats/career |
| `0x7F2798` | 44 × 0x14 | route table | formats/race-routes |
| `0x7F2B08` | 12 × 0x68 | level descriptors = environments (plus variant at `0x7F2FE8`) | formats/city-sectors |
| `0x7F38B8` | 65 × 4 | Thumb function table (states or menus?) | |
| `0x7F5BA4` | | pointers to the linear-mode step tables | formats/audio |
| `0x7F4344` | 12 × u32 | opponent 1's paint per wingman 1..12 (`pick_opponent_cars` reads `[wingman − 1]`; wingman 0 reads `0x7F4340` = 11) | formats/car-paint |

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
| `+0x62` | | s16 skyline row offset (4 in all) |
| `+0x64` | | s16 gradient start entry at a level horizon, plus 4 (25 in all) |

## RAM (reference race, `data/work/e5298b24/mgba/race.*`)

| Address | What |
|---|---|
| `0x03000000…` | race IWRAM code (renderer); see Functions |
| `0x030000C0` | **world struct** (below) |
| `0x03000060` | u32 player entity index (0) |
| `0x0300006C` | environment index (**11** in the reference race) |
| `0x0300003C` | current music id (−1 none) |
| `0x03005A00` | IWRAM block (0x54C): mode-1 mixer code, then mix buffers `0x03005DEC` / `0x03005E9C` (176 samples each) |
| `0x03005F4C` / `0x03005F50` | sound work-area pointer (0x26AC allocated) / 28-byte engine config |
| `0x03006370` | LS_Play engine pointer (`0x0200EE28` in the reference runs; layout in formats/audio) |
| `0x03006378` | mixer-driver call counter |
| `0x0300637C` | current module state (engine `+0x5C`) |
| `0x03000040` | units setting |
| `0x03000050` | catch-up |
| `0x03000070` | mode flags |
| `0x030000A0` | career flag |
| `0x030000BC` | event AI skill |
| `0x030053A4` | sound option (volume = option × 4, at most 63) |
| `0x030053AC` | pointer to the player entity |
| `0x030053E4` | camera setting |
| `0x03005388` | route number (menu numbering; `0x7F2588` maps it) |
| `0x03005600` | language |
| `0x03005604` | traffic |
| `0x03005608` | difficulty |
| `0x03005610` | reverse |
| `0x03005698` | HUD setting |
| `0x030056E0` | race mode |
| `0x030056E4` | laps |
| `0x03005718` | player car |
| `0x03005730` | finishing order bytes |
| `0x0300578C` | music option (volume = option × 4, at most 63) |
| `0x03005798` | transmission |
| `0x030057EC` | AI car count (opponents, + 1 with a wingman) |
| `0x03005800` | race frame counter |
| `0x03005FB8` | pointer to the backward-step table for branch starts (`racing_line_step`) |
| `0x0300608C` | lapped race (0 = sprint) |
| `0x030061A4` | someone finished |
| `0x030061B0…0x030061F4`, `0x0300617C` | hunter tuning |
| `0x03005620` | pointer to the current level descriptor (`0x087F2F80` in the race) |
| `0x03000080` | view struct (world `+0x50`): `+0` draw page, `+8`/`+0xA` centre, `+0x1C` focal (0x96) |
| `0x03000214` | camera yaw (0x4000 per turn) |
| `0x03005390` / `0x03005392` | s16 screen shake x / y (always 0 in play) |
| `0x030056B8` | horizon shift in rows (bumper view: car pitch, ±32; 0 otherwise) |
| `0x030055F8` | camera view (0 bumper, 2 chase; per-view tables `0x7F39BC`/`0x7F39D4`/`0x7F39EC`) |
| `0x030053A0` | extra projection-centre y offset (8.8) |
| `0x030053D0` | view rect x0, y0, x1, y1 |
| `0x03006410` | screen struct (width, height) |
| `0x03007FFC` → `0x03005810` | IRQ dispatcher (ARM); handler table `0x030056D0` (`+0` mask 0xA0, `+4` VBlank, `+8` VCount) |
| `0x030001C0` | VCount IRQ: palette entry 0 = next gradient entry, LYC += 2, lines 0–78 |
| `0x0200120C` | sky gradient buffer, 0x200 BGR555 (pointer `0x030053B8`); only the first 64 are the gradient |
| `0x030056E8` | gradient read pointer (VBlank sets base + 2·start) |
| `0x03006490` | IWRAM overlay base − 0xE4 (`0x03000220`): ROM `0x08165218` ↔ IWRAM `0x03000304` |
| `0x0300649C` | pointer to the 32-byte-block fill `0x030002C0` |
| `0x03005614` | u32 player's current sector (760 at the start of the reference race) |
| `0x03005720` | u32 current route index (23 in the reference race) |
| `0x030055F0` | pointer to the base palette buffer (`0x02001008` in the race); the light tint reads it |
| `0x0300577C` | pointer to the second base palette buffer (`0x02000E04`) |
| `0x03005630` | i32 palette fade step (±0x10, stepped by 2 to 0); while non-zero the tint and shade write the second buffer. **Not a pointer** |
| `0x0300563C` | palette dirty flag: `copy_palette_to_ram` copies `*0x0300577C` to palette RAM (outside races) |
| `0x03005808` | game state (5 = race; `main_frame` tints every game frame) |
| `0x0300611C` | i8 car id per racer ([0] player → entity 0 `+0x89`; [1..3] opponents) |
| `0x03005FEC` | i8 paint per racer ([0] = player record `[6]`, also at `0x030000B8`) |
| `0x0300539C` | pointer to the car records (`0x02000901`, 0x11 bytes per car id; = profile `+0xF9`) |
| `0x030056EC` | pointer to the profile (`0x02000808`, saved to EEPROM) |
| `0x03005700` | car record being edited in the garage |
| `0x03005784` | opponent count (3) |
| `0x03006104` | wingman (0 none, 1..12; `race_start_from_table_a`) |
| `0x030057F8` | entity index whose heading `shade_car_paint` uses (0) |
| `0x03006164` | per racer: pointer to the unpacked atlas (player `0x0201FB9C`) |
| `0x03006094` | per entity: pointer to the unpacked decal |
| `0x03005780` | race-over flag (`shade_car_paint` skips while set) |
| `0x030064C8` | `rand_table` index |
| `0x0201431C` | entity array in the reference race (world `+0x3C`) |
| `0x0201EC24` | vehicle matrix buffer in the reference race (world `+0xFC`) |
| `0x02001008` / `0x02000E04` | the two base palette buffers (0x200 bytes each) |
| palette RAM `0x05000000` | BG palette. Entry 0 = backdrop (`0x4A2E` in the race; opaque index-0 wall texels show it). Slots 1–143 and 149–255 are tinted by light (`FUN_0813a514`). 160–175 / 176–191: ramps `paints[1]` / `paints[2]`; 192 and 208: glass shades; 193–207: city trim; 208–223: player ramp (atlas pixel `i` → `192 + (i ^ 16)`); 240–247 / 248–255: extra car rows (`load_car_palettes`) |

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
| `+0x40` / `+0x44` | racing-line section table / racing line (0x1800) |
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
| `+0xF6` | sky enabled |
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
| `+0x2C` | heading, 8.8 fixed point (`>> 8`: 0x4000 per turn) |
| `+0x36` | LOD piece count? |
| `+0x44/+0x46/+0x48` | material selection (`+0x48` = atlas: player = car table `+0x0C` + record `[3]`; opponents 140–142 in the race) |
| `+0x64` | second LOD piece offset? |
| `+0x74` | start sector (template entities) |
| `+0x84` | RAM pointer to the unpacked atlas (`0x03006164[racer]`) |
| `+0x88` | model slot (`0xFF` = none) |
| `+0x89` | car id (index into the 0x11-byte car records) |
| `+0x8C` | pointer to the driver struct (below; race: `0x0202C624` player, then `0x0202D168` + 0x500·k) |
| `+0x90` | racing-line segment |

### Driver (`*(entity + 0x8C)`)

| Offset | What |
|---|---|
| `+0xA8` | position |
| `+0xAC` | distance |
| `+0xB4` | best lap |
| `+0xB8` | lap start |
| `+0xBC` | finish time |
| `+0xC5` | laps left |
| `+0x4D8` | race flags (bit 1: lap armed) |
| `+0x4E8` | hunter life |

### Profile (`*0x030056EC` = `0x02000808`)

| Offset | What |
|---|---|
| `+0xF9` | car records, 0x11 bytes per car id |
| `+0x200` | wingman selection |
| `+0x205` | event status (2 bits per event) |
| `+0x218` | record times |
| `+0x3B8…+0x3F8` | setting values |
| `+0x42D` | unlock bits (40 bytes) |

## Functions

Names are ours; addresses are the Ghidra `FUN_xxxxxxxx`. The IWRAM ones exist in the Ghidra project because the race IWRAM dump is loaded; `FUN_08166c04` = `FUN_03001cf0` (ROM copy, delta `0x164F14`).

See [symbols.csv](symbols.csv) for the full list with one-line descriptions.
