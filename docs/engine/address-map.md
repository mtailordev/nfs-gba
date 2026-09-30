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
| `0x12B454` | 48 × 4 | screen enter jump table (screens 0–0x2F; 0x30 has none) | formats/ui |
| `0x12B640` | 49 × 4 | screen exit jump table (run when the menus are left for a race) | formats/ui |
| `0x12B980` | 49 × 4 | screen update jump table | formats/ui |
| `0x12D370` | 49 × 4 | screen draw jump table | formats/ui |
| `0x141648` | 20 | text function language jump table (text_menu, text_menu_7, _wrapped_colour, text_box): language variable 0..4 reads text-table rows 1, 2, 4, 3, 5 | docs/formats/ui.md |
| `0x14FC38`, `0x165154`, `0x168264` | | ARM code copied to IWRAM for races (`0x03000000 + off − 0x164F14` etc.) | below |
| `0x151E34` | | the loader's `"GBAMOD30"` literal (not a module) | formats/audio |
| `0x153BB4` | | note → period table, linear pitch mode (unused by Carbon) | formats/audio |
| `0x154354…` | | per-rate note → step tables, linear mode (pointers at `0x7F5BA4`) | formats/audio |
| `0x15CF2C` / `0x15CFD4` | | LZ77-packed ARM mixer, mode 0 / **mode 1** (→ IWRAM `0x03005A00`, 0x3EC bytes) | formats/audio |
| `0x165134` / `0x1651AC` | | ARM divide with remainder / 32-byte block copy (IWRAM `0x03000220` / `0x03000298`) | formats/ui |
| `0x169208` | | ARM ring-buffer LZ77 decoder, the game's decompressor (IWRAM copy) | formats/ui |
| `0x169AAC` | | ARM minimap window copy (IWRAM `0x03004B98`) | formats/ui |
| `0x16C244–0x402000` | 294 blobs | LZ77 image bank; `0x16C244` is the menu texel base (menu descriptor `+0x10`) | formats/lz77-images, formats/ui |
| `0x224EE0` | | data inside the image bank; Ghidra's `FUN_08224ee0` is a false function | engine/harness |
| `0x33EF14` | 49 × 0x200 | menu palettes (menu descriptor `+0x04`) | formats/ui |
| `0x345114` | 273 × 0x24 | menu materials (menu descriptor `+0x24`) | formats/ui |
| `0x347778` | 10 × 8 | menu sprite screens (`+0x2C`) | formats/ui |
| `0x3477C8` | 47 × 0x14 | menu sprite elements (`+0x30`) | formats/ui |
| `0x347B74–0x36C55C…` | | HUD sprite texels, 4bpp (rec `+0x10`); includes `0x350000–0x368000`, once mistaken for ARM code (runs of `0xEEEEEEEE`) | formats/ui |
| `0x36C75C` | 4 × 0x200 | OBJ palettes (rec `+0x04`; the loaded one is `+ (+0x58)·2`); also the literal base of `load_car_palettes` | formats/ui, formats/car-paint |
| `0x36C95C` | 0x20 each | car paint ramps (block `+0x200`), 16 colours per paint number; glass = colour 12 | formats/car-paint |
| `0x36CD5C` | 8 × 2 B rows | extra rows for palette slots 240 and 248 (block `+0x600`) | formats/car-paint |
| `0x36CF5C` | 280 × 0x24 | HUD materials (rec `+0x24`) | formats/ui |
| `0x36D010` | 0x24 | material of the sparks (+0x10: lifetime in frames; +0x20 palette) | engine/game-loop |
| `0x36D304` | 0x24 | material of the exhaust flames (+0x10 low byte: frame count) | engine/game-loop |
| `0x36F698` | 0x24 | material of the traffic rear lights | engine/game-loop |
| `0x36F6BC` | 4 × 8 | HUD sprite screens (rec `+0x2C`) | formats/ui |
| `0x36F6DC` | 185 × 0x14 | HUD sprite elements (rec `+0x30`) | formats/ui |
| `0x370550` | | vehicle texture base (rec `+0x0C`) | formats/vehicle-models |
| `0x4018C0–0x45F5C0` | | vehicle materials 116–146, raw 8bpp (opponent atlases in final palette slots) | formats/ui |
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
| `0x71F168` | 128 B | unexplained: zero apart from one word (the only unowned bytes of `0x4018C0–0x71F1E8`) | formats/ui |
| `0x71F1E8` | 14 × 0x200 | city palettes (rec `+0x00`); an environment picks `+0x5A × 2` | formats/city-sectors |
| `0x720DE8` | 227 × 0x24 | city materials (rec `+0x1C`) | formats/city-sectors |
| `0x722DD4` | 4 KiB | unknown table (rec `+0x28` → world `+0x2C`) | |
| `0x723DD4` | 1,113 × 0x30 | sectors (rec `+0x18`) | formats/city-sectors |
| `0x730E84` | 4,423 × 0x44 | walls (rec `+0x14`) | formats/city-sectors |
| `0x77A000–0x78E000` | | route data: template entities, racing lines | formats/race-routes |
| `0x78E714–0x799B88` | 46,196 B | **unknown**, starting right after route 42's racing line; its `0x794000…` part is referenced 49 times from game code (`0x12AD40…`) | engine/harness |
| `0x797CD8` | 3 x 2 | List carousel x positions (previous; next; current) | formats/ui |
| `0x797CE0` | 3 x 2 | List carousel y positions (24; 24; 32) | formats/ui |
| `0x797CE8` | 17 × u16 | upgrade list item ids: the new marks and `upgrades_changed` (`0x081302C4`; items 0..10 on screen 0x1D, 10..17 on 0x1E) | engine/address-map |
| `0x797D0A` | 4 | Quick Play random race: the race modes to draw from | formats/ui |
| `0x7988B9` / `0x7C0360` | | "Pocketeers" / "LS_Play (C) Logik State 2003" | |
| `0x799882` |  | credits pages (u16 line count; (flags; text key) per line; a count of 0 ends) | formats/ui |
| `0x799B6C` | 4 x 4 | race mode name keys (Quick Play summary) | formats/ui |
| `0x799B88–0x7BFC53` | | text strings (the text table's targets) | formats/text-table |
| `0x7BFC68` | 12 | camera probe (0, 0, 72): the camera sector is the one 72 units ahead along the look direction | engine/renderer, engine/game-loop |
| `0x7BFCD4` | 3 × 4 | AI shortcut chance per difficulty: 255 / 192 / 100 (a roll of `rand & 0xFF` must beat it) | formats/career |
| `0x7BFD0C` / `0x7BFD18` | | EEPROM 4 Kbit / 64 Kbit descriptors (`eeprom_select_type`) | formats/career |
| `0x7BFD40` | 32 B | vibrato half sine | formats/audio |
| `0x7BFD60` | 768 × u16 | frequency table, one octave (period mode) | formats/audio |
| `0x7C0390` / `0x7C03A2` / `0x7C03B4` | 8 × u16 / 8 × u16 / 8 × u32 | mixing rates: timer 0 reload / samples per frame / Hz (index 0: 10512 Hz, 176) | formats/audio |
| `0x7C03F0` | 256 × u16 | random table (`rand_table`, index `0x030064C8`) | formats/car-paint |
| `0x7C05F0` | 0x2000 × i16 | sine table, half wave, 0x4000 = 1.0 (`sin_q14`) | formats/car-paint |
| `0x7C45F0` | 32,767 × 4 | reciprocal table: `recip[k] = 2^24/(k+1)`; every projection and divide, and the light interpolation | engine/renderer |
| `0x7E4714` | 6 × 2 u16 | boss event pairs per zone | formats/career |
| `0x7E472C` | 12 x 2 | Quick Play circuit map: track slot per map cursor (screen 7) | formats/ui |
| `0x7E4744` | 66 × 8 | career event table (mode, track, reverse, laps, traffic, AI skill, reward) | formats/career |
| `0x7E4954` | 16 × u16 | boss name keys | formats/career |
| `0x7E4974` / `0x7E4990` | 14 × u16 / 13 × 4 | wingman name keys / (role key, level) | formats/career |
| `0x7E49C4` | 43 × u32 | route number → track-name slot | formats/career |
| `0x7E4A70` / `0x7E4AA0` / `0x7E4AD0` | 12 / 12 / 18 × 4 | route name tables: `(u16 text key, u16 route)` for circuits forward, circuits reverse, sprints | formats/race-routes |
| `0x7E4B18` | | progress unlock records | formats/career |
| `0x7E4DE4` | | `(id, price)` pairs for parts (not decoded) | formats/career |
| `0x7E503C` | 15 × i16 | car prices | formats/career |
| `0x7E5070` | 4 × u16 | race mode name keys (event and results pages) | formats/career, formats/ui |
| `0x7E5078` | 8 x 2 | race mode icon materials (4 normal then 4 selected) | formats/ui |
| `0x7E5090`, `0x7E553C`, `0x7E5DA8` | 0x14 each | menu page records (`0x7E5090`: career event page, screen 0xD: heading, prompts, background, items); `0x7E5DA8`: 8 intro page records (screens 0x15–0x1A, 0x25, 0x2F, 0x30 → 0–7; `+0x08` menu palette, `+0x10` item list) | formats/ui |
| `0x7E50A4` | 4 x 0x14 | map pages (screens 7 8 0xE 0x11): +2/+4 prompts | formats/ui |
| `0x7E510C` | 2 x 0x18 | race results pages (0xB 0xC): heading; +4/+6 prompts; +8/+10 background | formats/ui |
| `0x7E517C` | 15 x 8 | car name keys (i16 at +0) | formats/ui |
| `0x7E544C` | 17 × 0x14 | List pages by `list_slot`: heading, prompts, `+6`/`+8` background | formats/career, formats/ui |
| `0x7E5D10` | 5 × 4 | language per cursor position on screen 0x19 (0–4) | formats/ui |
| `0x7E5D30` | 5 × 10 | language cursor images (material; x; y; …) for screen 0x19 (page 4 items + 2) | formats/ui |
| `0x7E5E48` | 5 × 2 | health screen colours 0–4 | formats/ui |
| `0x7E5E54…0x7E5EFF` | | setup option key lists | formats/career |
| `0x7E6260` | 6 × 0x10 | setup screens (items 0x18 each) | formats/career |
| `0x7E62C0` |  | text shown for a setting value out of its range | formats/ui |
| `0x7E6ED4` | 12 x 2 | page-script flash colours (palette entries 0xC0..0xCB) | formats/ui |
| `0x7E6EEC` | 19 × 0x80 | **portrait palettes** for materials 0x40..0x52 (64 colours each; 0x4D.. out of order). Not car paint presets: race colours come from `0x36C95C` | formats/ui |
| `0x7E786C` | 13 x 4 | crew portrait palette pointers (64 colours each) | formats/ui |
| `0x7E78A0` | 13 x 2 | wingman portrait material per wingman (screen 0x27) | formats/ui |
| `0x7E78B8` | 10-byte records | story screens: (material, palette) | formats/ui |
| `0x7E8534` | 0xC | wingman introduction page record (screen 0x27): enter script; draw script; entries | formats/ui |
| `0x7E8540` | 4 x 0xC | race-mode hint page records (screens 0x28..0x2B) | formats/ui |
| `0x7E8570` | 0xC per hint | career hint page records (screen 0x26) | formats/ui |
| `0x7E86A0` | 5,867 × 4 | text table: 977 keys, 5 × 977 strings, 5 module pointers | formats/text-table |
| `0x7EE238` | 5 × 4 | music table: module pointers | formats/audio |
| `0x7EE24C` | 40 B | Carbon sound id → sound-effect slot | formats/audio |
| `0x7EE274…0x7EE894` | | font glyph widths and y offsets | formats/ui |
| `0x7EE974` | 4 × 0x18 | font descriptors | formats/ui |
| `0x7EEA24` | bytes | special ramp numbers: by `cars[1] − 15` (slot 160) or `cars[0]` (slot 208, paint ≥ 20) | formats/car-paint |
| `0x7EEA33` | per car | new-profile per-car record `[6]` defaults | formats/career |
| `0x7EEA44` | 0xC per car id | opponent dressing: u16 material, model index, car id, `id % 3`, 13, 0 (`look`; car 14 points at car 0's texture and model) | formats/car-paint |
| `0x7EEB70` | 0x9C per car | i16 (x, y) of decal-set materials in the atlas at `+ 0x9C·car + 4·material` (effectively `0x7EEC7C` for materials 67–105) | formats/car-paint |
| `0x7EEBBC` | 6 per set | decal sets: 3 i16 vehicle materials (67–93) per record `[4]`, drawn only over body pixels | formats/car-paint |
| `0x7EF5A0` / `0x7EF672` | 7 per car | overlay material per `(car·7 + rec[1])` / its (x, y) | formats/car-paint |
| `0x7EF816` | 0x10 per entry | **wheel rims** per `(car·15 + rec[2])`: one 40×40 rim (materials 46–60) at two (x, y) placements; redrawn rotated by the wheel angle during the race | formats/car-paint |
| `0x7F0626` | per car | style base byte; also the car's body-kit count: record[3] stays below it (`kind18_update` bounds the kit page's cursor, `new_part` case 3; `reach::rim_redraw_never_reads_its_atlas`) | formats/career |
| `0x7F0636` | i16 per `car·0x10 + rec[0]` | entity `+0x64` source (clamped at 0) | formats/car-paint |
| `0x7F0BD8` | 15 × 0x58 | car table (`+0x0C` first material, `+0x0E` palette bank = 1 for all, `+0x10`/`+0x12` far model, `+0x14`/`+0x16` close model) | formats/vehicle-models, formats/car-paint |
| `0x7F1100` | 15 × 0x158 | handling records | engine/physics |
| `0x7F2528` | 8 × 12 | tipped-over body corners (x y z in car axes) that car_tipped_dynamics rests the car on | engine/physics |
| `0x7F2588` | 44 × 0xC | race slot per route number: environment, route index (`race_setup_route`) | formats/career |
| `0x7F2798` | 44 × 0x14 | route table | formats/race-routes |
| `0x7F2B08` | 12 × 0x68 | level descriptors = environments; the **menu descriptor** at `0x7F2FE8` has the same layout | formats/city-sectors, formats/ui |
| `0x7F3050` | per route | race-start byte | engine/physics |
| `0x7F37D8` | 44 × 4 | per route: pointer (null = none) to its branch count (→ `0x03006108`) and side-segment sector lists | engine/physics, formats/career |
| `0x7F38B8` | 65 × 4 | **entity handler table** (world `+0x78`, by entity `+0x4E`; `update_entities`): 0..3 `car_handler`; 4..0xD `0x0814add4`; 0x29 `opponent_handler` (opponents, wingman); 0x34 `effect_handler`; 0x36 `traffic_handler`; 0x39..0x3F `camera_update`; 0x40 `camera_look_at_player`; 0xE empty | engine/physics, engine/ai; 0x10/0x37 `entity_handler_10` `0x0814A198` (the look-at camera, view 7) |
| `0x7F399C` | 4 per view | camera function per view (camera_dispatch; 0 = none) | engine/game-loop |
| `0x7F39BC` / `0x7F39D4` / `0x7F39EC` | 6 each | per camera view: x offset (all 0), height 8.8 (−100, −140, −150, −115, −80, −105), distance (0, −290, −300, −150, 200, −120); orbit distance = `distance·256 + (0x80 − focal)·0x200` | engine/viewer-rendering |
| `0x7F3A24` | 0x10 per route index | moving pieces that start closed (i16 list; -1 ends; moving_pieces_init) | engine/race-init |
| `0x7F3CDA` | | breakable-wall partners | engine/physics |
| `0x7F3DD0` | 12 | the zero vector (also driver `+0xF8` init) | engine/physics |
| `0x7F3DE8` | 3 × 0x4 | AI boost timer reload per start value 0x03006158: 0 20 40 | engine/ai |
| `0x7F3DF4` | 3 × 0x4 | AI boost timer shift per start value: 0 6 4 | engine/ai |
| `0x7F3E00` | 6 × 2 per row | exhaust flame points per (car·8 + record[3]·2)·3: two pairs of (x; y; z); z = 0 none | engine/game-loop |
| `0x7F40D0` | per (setting, reverse) | race-start word | engine/physics |
| `0x7F4120` | 4 | lane offsets (`nearest_lane`) | engine/physics |
| `0x7F4164`, `0x7F41A0` | | curves: yaw damping by slip angle; rear grip by slip and yaw | engine/physics |
| `0x7F4284` | 12 × 0x4 | wingman gap per wingman (-> 0x030061DC) | engine/ai |
| `0x7F42B4` | 18 × 0x4 | a non-racer's (the wingman's) upgrade level by wingman; one entry fills all ten slots | engine/ai |
| `0x7F42E4` | per wingman | grid/skill value | engine/physics |
| `0x7F4344` | 12 × u32 | opponent 1's paint per wingman 1..12 (`pick_opponent_cars` reads `[wingman − 1]`; wingman 0 reads `0x7F4340` = 11) | formats/car-paint |
| `0x7F4378` | 5 | HUD digit x shift per language | formats/ui |
| `0x7F437D`, `0x7F43BD`, `0x7F43FD` | 16 × 4 | HUD message tables (race modes 0/1, 3, 2) | formats/ui |
| `0x7F4480`, `0x7F44A8` | | map scales and offsets (`map_world_to_screen`) | formats/ui |
| `0x7F44F8` | 5 × 0x20 | minimap palettes | formats/ui |
| `0x7F4598` | 1 per route | minimap palette per route | formats/ui |
| `0x7F53EC` | 4 × 0x20 | traffic tuning per traffic setting: byte type count; words to 0x03006258 0x03006260 0x03006294 0x03006254 0x0300624C 0x03006290 0x03006244 | engine/race-init |
| `0x7F546C` | | traffic types (u16 model, u16 paint) | engine/physics |
| `0x7F5488` | | traffic lane offsets | engine/physics |
| `0x7F5494` | 4 × 9 × 8 | control bindings | engine/physics |
| `0x7F55FC` | 2 | car-to-car axle offsets | engine/physics |
| `0x7F5604` | per traffic type × 0x4 | hit-response shift of the traffic car's velocity (and + 0xD of its wobble) | engine/ai |
| `0x7F5684` | per traffic type × 0x4 | hit-response shift of the racer's impulse | engine/ai |
| `0x7F5704` | per traffic type × 0x4 | traffic collision point count (2 for all) | engine/ai |
| `0x7F5784` | per traffic type × 0x4 | traffic collision radius squared (0x33A900) | engine/ai |
| `0x7F5804` | per traffic type × 2 × 0x4 | traffic collision points along the car (832; -576) | engine/ai |
| `0x7F5904` | 8 words | grip per floor surface | engine/physics |
| `0x7F5924` | per traffic type × 0x2 | non-zero: the player hitting this traffic type sets race phase 7 | engine/ai |
| `0x7F5988` | 10 × 5 words | upgrade weights | engine/physics |
| `0x7F5A50` | 5 × 0x4 | AI look-ahead distance by lane when the curve term abs(driver +0x98) > 0x1000: 0xA28 0xA28 0x960 0x898 0x578 | engine/ai |
| `0x7F5BA4` | | pointers to the linear-mode step tables | formats/audio |
| `0x7F5BC8` | 256 | character → glyph map | formats/ui |
| `0x7F5CC8` | 32 B | unknown byte map (`e0 e1 e2 …`) | engine/harness |
| `0x7F5CE8` | 4 × u32 | minimap copy masks per byte shift (0, 0xFF000000, 0xFFFF0000, 0xFFFFFF00), read by the minimap copy (`0x169B20` literal) | formats/ui |
| `0x7F5CF8–0x800000` | | zero fill to the end of the ROM | engine/harness |
| `0x8797CA8` | 0x2C | first unlock id of each message sub-group (u16 per group; group + 10 above 0x79) | formats/career |
| `0x87E513C` | 0xE | heading text key per unlock message kind 0..5 | formats/career |
| `0x87E514A` | 0x2C | unlock message text keys: u16 per group (the group for ids below 0x79; group + 10 above) added to the index within the group (unlock_message_index) | formats/career |

### Level descriptor (0x68 bytes, `0x7F2B08`)

| Offset | → world | What |
|---|---|---|
| `+0x00` | `+0x30` | city palette block; the loaded palette is `+0x00 + (+0x5A) × 2` |
| `+0x04` | `+0x34` | OBJ palettes (`+ (+0x58) × 2`); menus: menu palettes |
| `+0x08` | `+0x00` | city texel base |
| `+0x0C` | `+0x04` | vehicle texel base |
| `+0x10` | `+0x08` | HUD sprite texels (`0x347B74`); menus: menu texel base |
| `+0x14` | `+0x10` | walls |
| `+0x18` | `+0x14` | sectors |
| `+0x1C` | `+0x20` | city materials |
| `+0x20` | `+0x24` | vehicle materials |
| `+0x24` | `+0x28` | HUD sprite materials (`0x36CF5C`); menus: menu materials |
| `+0x28` | `+0x2C` | ? (`0x722DD4`) |
| `+0x2C`, `+0x30` | | sprite screens / sprite elements |
| `+0x34…+0x54` | `+0x7C…+0x9C` | vehicle model bank arrays |
| `+0x58` | | u16 OBJ palette index × 0x100 |
| `+0x5A` | | u16 city palette index × 0x100 |
| `+0x5E` | | u16 sky gradient material |
| `+0x60` | | u16 skyline material |
| `+0x62` | | s16 skyline row offset (4 in all) |
| `+0x64` | | s16 gradient start entry at a level horizon, plus 4 (25 in all) |

## RAM (reference race, `data/work/e5298b24/mgba/race.*`)

| Address | What |
|---|---|
| `0x02001008` / `0x02000E04` | the two base palette buffers (0x200 bytes each) |
| `0x0200120C` | sky gradient buffer, 0x200 BGR555 (pointer `0x030053B8`); only the first 64 are the gradient |
| `0x02012404` / `0x020123F8` / `0x02013348` | moving pieces (world `+0x18`) / sector offsets (`+0x1C`) / material runtime table (`+0x48`) |
| `0x0201431C` | entity array in the reference race (world `+0x3C`) |
| `0x02017288` | wall draw buffer (world `+0x68`) |
| `0x02018C8C` | visible-sector list, 64 × 16 bytes (world `+0x60`) |
| `0x02019090` | sector map, 8 bytes per sector, never cleared, unused (world `+0x64`) |
| `0x0201B094` | flat vertex buffer (world `+0x6C`) |
| `0x0201EC24` | vehicle matrix buffer in the reference race (world `+0xFC`) |
| `0x0201F828` | HUD objects in the reference race |
| `0x03000000…` | race IWRAM code (renderer); see Functions |
| `0x03000000` | health screen blink direction (IWRAM's first word) (u32) |
| `0x0300003C` | current music id (−1 none) |
| `0x03000040` | speed units (0 = mph) |
| `0x03000044` | running tick counter (intro deadlines); `setup_race_cars` uses its value as the race's rand seed |
| `0x03000048` | race phase (9 intro, in which places stay in entity order; 2 racing, 3 over; 1 and 4 skip the dynamics; `hud_timer` sets 8 past 59:59.98, the countdown timer 7 at zero) |
| `0x03000050` | catch-up |
| `0x03000058` | effect-sprite pool header (8): `+0` objects (0x0202C3A0 in the race; 20 bytes each: `+0` x, `+2` y, `+4` used, `+8` frame, `+A`/`+C` scale, `+0x10` size, `+0x12` kind, `+0x13` palette), i16 first OAM entry (127), `+6` count (32) |
| `0x03000060` | u32 player entity index (0) |
| `0x0300006C` | environment index (**11** in the reference race) |
| `0x03000070` | mode flags |
| `0x03000080` | view struct (world `+0x50`): `+0` draw page, `+8`/`+0xA` centre (120, 79), `+0x0C` pitch 240, `+0x10` near 64, `+0x1C` focal 150 |
| `0x030000A0` | career flag; when set, opponents take entry 0's car id |
| `0x030000AC` | race-state changed flag; also the countdown accumulator: `race_start_from_table_b` adds 50000 / frame time, each 0x4000 advances `0x03005714`; phases 6–8 count it down by timer-3 ticks to the race end |
| `0x030000B0` | camera: 0 at camera_init (4) |
| `0x030000BC` | event AI skill |
| `0x030000C0` | **world struct** (below) |
| `0x03000164` | race sprite screen (world `+0xA4`): `+0x00` materials, `+0x04` texels, `+0x0C` elements, `+0x10` screens, `+0x14` objects, `+0x18` u16 screen |
| `0x030001C0` | VCount IRQ: palette entry 0 = next gradient entry, LYC += 2, lines 0–78 |
| `0x03000214` | camera look yaw (0x4000 per turn; the skyline scrolls by it) = `atan(player − camera)` from the previous frame's camera position (`atan2_fast`); 0xFFD in the reference race, the atan's value at (344, 0) |
| `0x03000220` | IWRAM image start; `iwram_divmod` (signed divide storing a remainder in `0x03006480`) |
| `0x03000298` | IWRAM 32-byte block copy (used by `obj_upload_tiles`) |
| `0x03004B98` | IWRAM minimap window copy |
| `0x03005384` | the player has gone the wrong way for over 27 frames |
| `0x03005388` | route number, 1-based (23 in the reference race; `0x7F2588` maps it to environment and route index) |
| `0x03005390` / `0x03005392` | s16 screen shake x / y, added to the screen centre (always 0 in play) |
| `0x03005394` | matrix slot counter (≥ 0x40 → `0xFF`) |
| `0x03005398` | race paused (set by the START block of `race_frame_update`; stops the race time); cleared when a paused race resumes (`goto_screen` 0x82) |
| `0x0300539C` | pointer to the car records (`0x02000901`, 0x11 bytes per car id; = profile `+0xF9`) |
| `0x030053A0` | extra projection-centre y offset (8.8) |
| `0x030053A4` | sound option (volume = option × 4, at most 63) |
| `0x030053A8` | camera_init: waypoint offset of the start camera (0) (4) |
| `0x030053AC` | pointer to the player entity |
| `0x030053B4` | VBlank counter (`vblank_irq`); bit 4 blinks menu cursors and PRESS START |
| `0x030053D0` | view rect (left, top, right, bottom, i32; view `+0x04`): `draw_sector` sets it from the list entry, `draw_sector_entities` rewrites left/right per entity |
| `0x030053E4` | camera setting: 0 chase / 1 bumper (SELECT toggles it in `camera_update`) |
| `0x030053E8` | effect sprites on (`light_sprite` draws nothing while 0); set to 1 by `race_init` |
| `0x030053F0` | projected model vertices (world `+0xA0`), x/y halfwords |
| `0x030055F0` | pointer to the base palette buffer (`0x02001008` in the race); the light tint reads it |
| `0x030055F4` | camera: 0 at camera_init (4) |
| `0x030055F8` | camera view (0 bumper, 2 chase; per-view tables `0x7F39BC`/`0x7F39D4`/`0x7F39EC`); 4 at camera_init, then 2 by race_init; 7 only by `camera_look_at` `0x081389CC` from entity handler 0x10/0x37 (`0x0814A198`), which no entity gets: views are 0, 2, 4 (`reach::camera_view_7_is_unreachable`) |
| `0x030055FC` | camera: two halfwords cleared at camera_init (4) |
| `0x03005600` | language (0 En … 4 Es) |
| `0x03005604` | traffic |
| `0x03005608` | difficulty |
| `0x03005610` | reverse |
| `0x03005614` | u32 **camera** sector (written by `camera_update`, searched 72 units ahead of the camera; `apply_sector_light_to_palette` reads it with the player's position; 760 in the reference race) |
| `0x03005620` | pointer to the current level descriptor (`0x087F2F80` in the race) |
| `0x03005624` | flag: no atlases, all racers dressed from `0x7EEA44`; the link-play flag: 0 at boot, set only by uncalled link functions (`0x08146BF8`…`0x08147250`; `reach::link_play_is_unreachable`) |
| `0x03005628` | frame counter (`main_frame` adds 1; race start sets 0); its parity picks the page to draw; picks the traffic type |
| `0x0300562C` | free race (RACE TYPE): rand % 3 + 1 (4) |
| `0x03005630` | i32 palette fade step (±0x10, stepped by 2 to 0); while non-zero the tint and shade write the second buffer. **Not a pointer** |
| `0x0300563C` | palette dirty flag: `copy_palette_to_ram` copies `*0x0300577C` to palette RAM (outside races) |
| `0x03005640` | frame time (25,500 / timer-3 ticks, 10..100; 15 if `0x03005624` is 2), written by `main_frame` |
| `0x03005644` | camera: 0 at camera_init (4) |
| `0x03005650` | 0x40-byte results block per racer: `+0` car id copy (entity `+0x89`), `+8` knockout bytes, `+0x10` best lap, `+0x20` finish time, `+0x30` hunter life (`0x0300565C` is inside it) |
| `0x03005658`/`0x03005660`/`0x03005670`/`0x03005680` | per-racer slots cleared when Quick Play starts a race (4 × (u8 / u32 / u32 / u32)) |
| `0x0300565C` | 4 bytes: per-racer byte, 0xFF = empty slot |
| `0x03005698` | HUD setting |
| `0x030056A0` / `0x030000A4` | camera x / z, 8.8 |
| `0x030056B8` | horizon shift in rows (bumper view: car pitch, ±32; 0 otherwise) |
| `0x030056E0` | race mode (also selects the HUD screen and messages) |
| `0x030056E4` | laps |
| `0x030056E8` | gradient read pointer (VBlank sets base + 2·start) |
| `0x030056EC` | pointer to the profile (`0x02000808`, saved to EEPROM) |
| `0x030056F0` | race frame counter (`race_frame_update`; cleared at race start); below 0x32 the AI's lane offset is 0x200 per lane (else 0x100) |
| `0x03005700` | car record being edited in the garage |
| `0x03005707` | garage working copy of the car record bytes 7..16 (0x03005700 holds 0..6) (10) |
| `0x03005714` | countdown digit 0..2 (3 = GO: phase 2; the next frame's set-up uploads four tile sets and sets 4); race start set-up state; cleared by `race_init` |
| `0x03005718` | player car |
| `0x03005720` | u32 current route index (23 in the reference race) |
| `0x03005724` | counter incremented by vblank_irq (4) |
| `0x03005728` | bit 0 set by vblank_irq (IRQ seen) (4) |
| `0x03005730` | ranked results, 0x40 bytes (same arrays as `0x03005650`; `+4` = entity id at each rank) |
| `0x03005750` | per-car result places tie-broken by distance at the race end (FUN_0812e9e8) (16) |
| `0x03005778` | smoothed floor height at the camera (`floor_height`): the height limit for flag-0x4000 walls in the camera wall push |
| `0x0300577C` | pointer to the second base palette buffer (`0x02000E04`); while `FADE` `0x03005630` ≠ 0 the light tint writes here instead of palette RAM, and the BG fade moves palette RAM towards it |
| `0x03005780` | race-over flag (`shade_car_paint` skips while set); in the menus the exit request (7 = leave the menus for the race once the fade is done) |
| `0x03005784` | opponent count (3) |
| `0x0300578C` | music option (volume = option × 4, at most 63) |
| `0x03005798` | transmission |
| `0x030057A0` | camera matrix in the race |
| `0x030057D0` | index of the car whose finish ended the race (car_handler at phase 3) (4) |
| `0x030057D8` | u16 control word per entity (`0xFC00 \| keys` for the player) |
| `0x030057E0` | set to 0x10 at race start (with the fade); it is `input[4]` of the control-word array at `0x030057D8` (u32) |
| `0x030057E8` | cleared by race_init (4) |
| `0x030057EC` | AI car count (opponents, + 1 with a wingman) |
| `0x030057F0` | pointer to the unpack buffer |
| `0x030057F8` | entity index whose heading `shade_car_paint` uses (0) |
| `0x03005800` | race time in frames |
| `0x03005804` | set while the bumper view is on; back in the chase view camera_update resets the camera behind the car (4) |
| `0x03005808` | game state (5 = race; `main_frame` tints every game frame) |
| `0x03005934` | timer 3 ticks of the last frame (1024-cycle units since the previous frame; 0 read as 0x200), read by `main_frame`; frame time `0x03005640` = clamp(25500 / ticks); the wingman's and knocked-away traffic timers count down by it |
| `0x03005938` | screen entered flag (1 after enter_screen unless screen 5; 0 after the exit handler) (u32) |
| `0x0300593C` | top of the menu back stack (screens at profile +0x344) (i8) |
| `0x03005940` | back stack top saved while racing (restored after the race) (u8) |
| `0x03005944` | menu screen (0..=0x30; 0x80/0x81 = a race was chosen; 0x81 = Quick Play) (i32) |
| `0x03005948` | screen changed: the next draw_screen draws in full (u32) |
| `0x0300594C` | screen whose exit handler runs when the menus are left (−1 none) (i32) |
| `0x03005954` | upgrades changed (the 0x8B save question) (4) |
| `0x03005960` | language cursor (screen 0x19) (i32) |
| `0x03005964` | current credits page (pointer) |
| `0x03005970` | profile name being typed (screen 0x16) (9 bytes) |
| `0x0300597C` / `0x03005990` | name keyboard row (0–4) / column (0–9) (i32) |
| `0x03005980` / `0x03005984` | cleared when the credits are entered (u32) |
| `0x0300598C` | typed name length (0–8) (i32) |
| `0x03005994` | cleared by setup_screen_enter (4) |
| `0x03005998` | settings changed (options screen: ask to save) (4) |
| `0x030059E8` | page wait: frame counter value page 0x29 and page-script sounds wait for (4) |
| `0x030059EC` | message box argument (-1: none) (4) |
| `0x030059F0` | open message box (−1 none; 2 also closes on B) (i32) |
| `0x030059F4` | message box result (1 A / −1 B / 0) (i32) |
| `0x030059F8` | message box text key (4) |
| `0x03005A00` | IWRAM block (0x54C): mode-1 mixer code, then mix buffers `0x03005DEC` / `0x03005E9C` (176 samples each) |
| `0x03005F4C` / `0x03005F50` | sound work-area pointer (0x26AC allocated) / 28-byte engine config |
| `0x03005F8C` | camera: 0xFFFEF000 at camera_init (4) |
| `0x03005F94` | chase orbit yaw; eases towards the driver's heading by `clamp(diff, ±0x600) >> 3` unless `0x03006148` |
| `0x03005F9C` | −look yaw & 0x3FFF: the camera matrix's yaw (14-bit); the effects and the rim redraw test use 0x4000 − it |
| `0x03005FA0` | camera: 0x400 at camera_init (4) |
| `0x03005FA4` | camera height offset (8.8) = the view's height table entry (chase −150·256) |
| `0x03005FB0` | camera: -1 at camera_init (4) |
| `0x03005FB4` | pointer to the plane table (malloc 0x2000; 0x20 per waypoint: direction, widening, crossing plane, length); built with the lapped flag the previous scene left, so skipped rows keep old contents |
| `0x03005FB8` | pointer to the back table (256 × i32: lap index where a branch leaves; −1 none), used by `racing_line_step` for backward steps from a branch start |
| `0x03005FD0` | +0x0A = 0 and +0x0C = 8 at race start (meaning unknown) (0x10) |
| `0x03005FEC` | i8 paint per racer ([0] = player record `[6]`, also at `0x030000B8`) |
| `0x03006000` | 5 words: the player's upgrade totals; `+0x10` nitro level × 10 |
| `0x0300601C` | off-route warning (−1/0/1), written by `car_dynamics`; the HUD arrow shows it |
| `0x03006030` | gravity (0x4F0, set by `car_init`) |
| `0x03006074` | manual gearbox state |
| `0x03006078`, `0x03006084`, `0x03006088`, `0x03006028`, `0x03006190` | race-start globals (`race_start_setup`) |
| `0x0300608C` | lapped race (0 = sprint) |
| `0x03006090` | traffic slots on (`traffic_slots` does nothing while 0); set by `race_start_setup` |
| `0x03006094` | per entity: pointer to the unpacked decal |
| `0x03006098` | cleared at the end of race_start_from_table_a (4) |
| `0x030060A4` | the player's final drive |
| `0x030060C0` | per section (16 words): non-zero = the AI may take this branch as a shortcut |
| `0x03006104` | wingman (0 none, 1..12; `race_start_from_table_a`) |
| `0x03006108` | branch count of the race's route |
| `0x0300610C` | set by the dynamics; while 0 the racing step runs the suspension step; non-zero lets `build_entity_matrix` take the physics orientation (entity `+0x0A` bit 5); 1 from race_init on, nothing stores 0 (`reach::physics_orientation_is_always_set`) |
| `0x03006110` | added to driver +0x188 in the AI's wheel drive (4) |
| `0x03006118` | free race (RACE TYPE): rand % 20 (4) |
| `0x0300611C` | i8 car id per racer ([0] player → entity 0 `+0x89`; [1..3] opponents) |
| `0x03006120` | per section: branch distance scale onto the lap ×256 (`[0]` = 0x100) |
| `0x03006148` | raised camera: 0x82 above the car instead of 0x10, and the orbit does not follow; never written (`reach::raised_camera_is_unreachable`) |
| `0x03006150`, `0x0300614C` | nitro full-tank flag; grip-doubling flag |
| `0x03006154` | time limit (0x4650; 0x2328 in career); `hud_countdown_timer` counts it down against the race time |
| `0x03006158` | AI boost start value 0..2 (`0x7F3DE8`/`0x7F3DF4`); 2 also boosts over the last lap's last 3 segments; set by `race_start_setup` |
| `0x0300615C` | split time in frames: gap to the car ahead/behind (`route_gap`; `hud_split` clamps it to 0) |
| `0x03006160` | scratch: waypoints in sections 0..=N (link rebuild) |
| `0x03006164` | per racer: pointer to the unpacked atlas (player `0x0201FB9C`) |
| `0x03006174` | wingman: attack timer (0x12) (4) |
| `0x03006178` | the entity the wingman attacks (and the lane reference of wingman_steer) (4) |
| `0x0300617C` | hunter: hit damage factor ×256 (0x440) |
| `0x03006180` | wingman: engaged steps left (6) (4) |
| `0x03006184` / `0x030061A0` | hunter: drain factor ×256 of `hunter_wall_hit` / `hunter_drain_b` (0x240 each) |
| `0x0300618C` | wingman: cleared when a command ends (4); set when a command is given in the drafter role (`0x030061F8` set) |
| `0x0300619C` | the wingman's partner entity (the player's; world +0x3C) (4) |
| `0x030061A4` | someone finished |
| `0x03006198` | hunter: chase target time (400), the AI's `+0x4F0` reload |
| `0x030061B0` | hunter life gain by place, 5 × 4 (0, 200, 150, 100, 0) |
| `0x030061D0` / `0x030061E0` | hunter: wrong-way frames (27) / wall frames (50) before the drain |
| `0x030061D4` / `0x030061DC` | wingman: command available (the HUD portrait blinks) / command count (loaded from `0x7F4284`; one spent per command; the HUD portrait frame) |
| `0x030061D8` | wingman: cooldown (down by 0x03005934; copied to 0x03006048) (4); 0x1E000 after a command |
| `0x030061E4` / `0x03006188` | wingman: command time left / command length 0x78000 (the HUD bar value / full scale); each wingman command refills the bar to full scale |
| `0x030061E8` | wingman: command running (4); set by `wingman_command` |
| `0x030061EC` | wingman: previous lead (follow gain) (4) |
| `0x030061F0` | wingman: engaged with its target (attacker; `wingman_steer` on); cleared by `traffic_wall_hit` |
| `0x030061F4` / `0x030061A8` | hunter: wrong-way drain per frame (1000) / wall drain per frame (100) |
| `0x030061F8` | wingman role: 0 attacker; 1 drafter (4) |
| `0x030061FC` | wingman: previous lead + 500 or gap + 100 (gains) (4); cleared by the wingman command (attacker role) |
| `0x03006200` | wingman: keep-gap phase (4) |
| `0x03006210` | 6 × 4 HUD message slots |
| `0x03006230` | map state: +0/+4 view x/y 8.8; +8 cursor i8; +9 moved (0xC) |
| `0x03006240`, `0x03006264`, `0x03006260`, `0x03006298`, `0x0300625C` | traffic: spawned count (max 4), countdown (byte), countdown reload, traffic on, type count |
| `0x03006244` | traffic tuning word (0x7F53EC +0x1C) (4) |
| `0x03006248` | traffic setting count (4) (4) |
| `0x0300624C` | traffic speed-up period (steps of +0x52) (4) |
| `0x03006250` | set when a traffic spawner has the camera car in range (4) |
| `0x03006254` | traffic tuning word (0x7F53EC +0x10) (4) |
| `0x03006258` | traffic tuning word (0x7F53EC +4) (4); traffic spawner range squared (camera car to spawner) |
| `0x03006270` | 8 × pointer: live traffic cars |
| `0x03006290` | traffic top speed (entity +0x24) (4) |
| `0x03006294` | traffic stop distance factor (modes 2 and other) (4) |
| `0x0300629C` | u8 control binding set (0 automatic, 1 manual) |
| `0x03006370` | LS_Play engine pointer (`0x0200EE28` in the reference runs; layout in formats/audio) |
| `0x03006378` | mixer-driver call counter |
| `0x0300637C` | current module state (engine `+0x5C`) |
| `0x03006390` / `0x030063B0` | clipped UVs (x pass / y pass) |
| `0x03006410` | frame buffer struct (0x14): `+0` width 240, `+2` height 160, `+8` byte (8), `+0x0C` page 0x06000000, `+0x10` page 0x0600A000 |
| `0x0300641C` | frame buffer page 1 pointer (0x06000000) (4) |
| `0x03006420` | frame buffer page 2 pointer (0x0600A000) (4) |
| `0x03006430` | identity index table 0…7 for clipped polygons |
| `0x03006440` / `0x03006460` | clipped screen corners (x pass / y pass) |
| `0x03006480` / `0x03006494` | IWRAM divider remainder / pointer to the divide routine (`0x03000220`) |
| `0x03006490` | IWRAM overlay base − 0xE4 (`0x03000220`): ROM `0x08165218` ↔ IWRAM `0x03000304` |
| `0x03006498` | race overlay size (0x5164) (4) |
| `0x0300649C` | pointer to the 32-byte-block fill `0x030002C0` |
| `0x030064A0` | pointer to the IWRAM 32-byte block copy (copy_shadow_oam and obj_upload_tiles call through it) (4) |
| `0x030064B0` | debug text console: buffer pointer; +8 column; +0xA row (FUN_0815e850) (12) |
| `0x030064C0` | keys newly pressed this frame (the menus compare it with 1 = A and 2 = B) (u16) |
| `0x030064C4` | a second key word (car select turns the car on 0x40/0x80) (2) |
| `0x030064C8` | `rand_table` index |
| `0x030064CC` / `0x030064D0` | heap descriptor table (8 bytes per block) / arena (`heap_alloc`) |
| `0x030064E0` | u16 OBJ tile base (0x200 in mode 4) |
| `0x030064F0` | shadow OAM (128 × 8 bytes) |
| `0x030068F0…0x0300694C` | polygon rasteriser state (edges, spans, u/v steps; renderer.md) |
| `0x03006920` | entity visited bitmap, 32 bytes, cleared each frame by `draw_visible_sectors` |
| `0x03006940` / `0x03006900` | draw-list head / greatest sort key of the sector being drawn |
| `0x03007FF8` | BIOS IntrCheck flags (vblank_irq sets bit 0) (2) |
| `0x03007FFC` → `0x03005810` | IRQ dispatcher (ARM); handler table `0x030056D0` (`+0` mask 0xA0, `+4` VBlank, `+8` VCount) |
| palette RAM `0x05000000` | BG palette. Entry 0 = backdrop (`0x4A2E` in the race; opaque index-0 wall texels show it). Slots 1–143 and 149–255 are tinted by light (`FUN_0813a514`). 160–175 / 176–191: ramps `paints[1]` / `paints[2]`; 192 and 208: glass shades; 193–207: city trim; 208–223: player ramp (atlas pixel `i` → `192 + (i ^ 16)`); 240–247 / 248–255: extra car rows (`load_car_palettes`) |

### I/O registers (`0x04000000`)

| Address | What |
|---|---|
| `0x04000000` | DISPCNT: mode 4 in races (8bpp framebuffer, page flip), OBJ 1-D mapping |
| `0x04000050` | race BLDCNT 0x3F3F (every layer a second target) and BLDALPHA 0x0D0F (EVA 15/16, EVB 13/16): semi-transparent HUD sprites brighten what is under them (4) |

### World struct (`0x030000C0`)

| Offset | What |
|---|---|
| `+0x00` | city texel base |
| `+0x04` | vehicle texel base |
| `+0x0C` | u16 per sector: head of its entity list (`0xFFFF` = none) |
| `+0x10` / `+0x14` | walls / sectors |
| `+0x18` | moving wall pieces (in every captured race all 122 are zero offsets with flags 1, open), 0x20 bytes, by wall `+0x2A`: dx, dz, ceiling dy, floor dy, top dy, bottom dy, material offset, flags |
| `+0x1C` | sector offsets, 0x14 bytes, by sector `+0x0A`: `+4` ceiling dy, `+6` floor dy, `+8` flags replacing sector `+0x12` (`0x40` = hidden); empty in Carbon (no sector names one) |
| `+0x20` / `+0x24` | city / vehicle materials |
| `+0x30` / `+0x34` | loaded palettes |
| `+0x38` | route template entities |
| `+0x3C` | entity array (0xA4 each) |
| `+0x40` / `+0x44` | racing-line section table / racing line (0x1800) |
| `+0x48` | per-material runtime entries (8 bytes: `+2` animation frame, `+4`/`+6` u/v scroll); allocated zeroed by `race_load_level`, cleared by `race_init`/`load_menu_descriptor`, never written otherwise (the animation step `race_frame_nop_a` is empty; `reach::renderer_runtime_tables_never_change`) |
| `+0x50` | view struct (`0x03000080`) |
| `+0x54` | camera matrix: 12 × i32, 3×3 rotation in 2.14 fixed point then translation |
| `+0x58…+0x5E` | screen rectangle (0, 240, 0, 159) |
| `+0x60` | visible-sector list |
| `+0x64` | sector map |
| `+0x68` | wall draw buffer (0x1A00; 0x34 bytes per wall) |
| `+0x6C` | flat vertex buffer (0x18 per vertex: x, floor y, ceiling y, recip, u/z, v/z) |
| `+0x78` | entity handler table (`0x7F38B8`; the sort calls `[+0x78][entity +0x4E]`) |
| `+0x7C…+0x9C` | vehicle model bank arrays |
| `+0xA0` | projected model vertices (`0x030053F0`) |
| `+0xC0` / `+0xC8` | point searched by `find_camera_sector`; scratch shared by the camera and entity code |
| `+0xD8`, `+0xDA`, `+0xDC` | counts: materials, sectors, walls |
| `+0xDE` | sectors with an offsets record (the world +0x1C count) (2) |
| `+0xE0` | walls with a moving piece (the world +0x18 count) (2) |
| `+0xE2…+0xE8` | current portal span (left, right, top, bottom); the race clip rect is (0, 240, 0, 159), so row 159 is never drawn |
| `+0xEA` | camera sector |
| `+0xEC` | cleared at race start (2) |
| `+0xEE` | visible count |
| `+0xF0` | 0 each frame |
| `+0xF2` / `+0xF4` | flat outline / clipped outline lengths |
| `+0xF6` | sky visible |
| `+0xF8`, `+0xFA` | first entity and entity count for the update loop and `free_entity` (race: 4 and 0x20) |
| `+0xFC` | matrix slots, 0x30 each, 64 slots: entity rotation × camera rotation (2.14) + camera-space position |

### Entity (0xA4 bytes, world `+0x3C`)

Renderer fields confirmed from the code and 17 captured frames (engine/renderer.md "Entities").

| Offset | What |
|---|---|
| `+0x00` | own index (the draw order links through it) |
| `+0x02` | next entity in its sector's list (heads: world `+0x0C`, u16 per sector) |
| `+0x04` | next entity in the sector's draw order (written by the sort; `0xFFFF` ends it) |
| `+0x08` | state: bit 0 runs the handler `world+0x78[+0x4E]` during the sort unless bit 1; bit 2 takes part in the sort; `update_entities` runs entities with `& 3 == 3` |
| `+0x0A` | flags: bit 0 clip to the whole screen (world `+0x58/+0x5A`); bit 1 depth taken as 0 for LOD and the far cut (near model always; opponents `0x22`, markers `0x02`); bit 2 passed the screen cull this frame; bit 3 texture in RAM (`+0x84`); bit 4 draw sector `+0x36` instead of a model; bit 5 matrix from the driver's physics orientation; bit 6 beyond depth 0x1000 draw the far model instead of nothing. Player `0x0D` |
| `+0x0C/+0x10/+0x14` | position x, y, z in 8.8 fixed point, city units |
| `+0x18` | traffic: direction x (1.0 = 0x1000) (4) |
| `+0x20` | traffic: direction z (4) |
| `+0x24` | traffic: speed (3 or 4 while driving) (4) |
| `+0x28` | sort key `(x'² + d²) >> 8`, camera space |
| `+0x2C` | heading, 8.8 (`>> 8`: 0x4000 per turn); template entities: start heading (`0x100000` for route 23) |
| `+0x30` / `+0x32` | angles for `build_entity_matrix`'s two rotations (axes not checked); traffic: pitch / shown heading |
| `+0x36` | **far model** (depth ≥ 0x200); near = `+0x36 − 1`. Cars: the low model, so races draw medium near and low far, never high. Bit 4: a sector index |
| `+0x38` | traffic: heading wobble (4) |
| `+0x44` (high byte) / `+0x46` | material steps added to `+0x48` (0 in every capture) |
| `+0x48` | vehicle material (the atlas); player = car table `+0x0C` + record `[3]`; opponents from `0x7EEA44`; 0 = not drawn |
| `+0x4E` | handler index (`0x7F38B8`): 0 player, 0x29 opponents, 0xE empty slots, 0x36 AI/traffic |
| `+0x52` | traffic: speed-up counter (2) |
| `+0x56` | traffic: knocked-away timer (2) |
| `+0x64` | second model on matrix slot `+0x88 + 1` (spoiler: 12 Cobalt, 4 car 0; from `0x7F0636[car·0x10 + rec[0]]`, clamped at 0); negative: drawn before as model `−n` |
| `+0x70` | 0x640 at setup |
| `+0x74` / `+0x78` | start sector (template entities; the viewer reads `+0x78`) |
| `+0x7C` | traffic: type (0x7F546C) (2) |
| `+0x84` | RAM address of the unpacked atlas (`0x03006164[racer]`, bit 3) |
| `+0x88` | matrix slot (world `+0xFC`); `0xFF` = not drawn |
| `+0x89` | car id (index into the 0x11-byte car records) |
| `+0x8C` | pointer to the driver struct (below; race: `0x0202C624` player, then `0x0202D168` + 0x500·k) |
| `+0x90` | racing-line segment |
| `+0x94` | AI: copy of +0x76 at setup (2) |
| `+0x9A` | traffic: direction along the route (+-1) (2) |
| `+0x9C` | traffic: waypoint (2); 1 for spawns at a section start |
| `+0x9E` | traffic: mode (1 lane driving; 2 waypoints; else stop at the waypoint) (2); spawn kind: 0 section start at half speed, 1 near the player, 2 section start at full speed |

### Driver (`*(entity + 0x8C)`)

| Offset | What |
|---|---|
| `+0x00` | heading the chase camera follows (0x1000 in the reference race) |
| `+0x08` | suspension step: vertical speed per point (FUN_0814de40) (4 × 4) |
| `+0x28` | brake lights on (opponent_effects draws them for entity 0) (4) |
| `+0x3C` / `+0x40` / `+0x44` | revs / gear / speed (read by the HUD) |
| `+0x48` | suspension step: points on the floor (its return value; 4 at car init) (4) |
| `+0x4C` | suspension step: spring position per point (4 × 4) |
| `+0x5C` | suspension step: spring rate per point (damped to 160/256 each step) (4 × 4) |
| `+0x6C` | suspension step: height per point (floor-clamped); entity y = the mean of the four (4 × 4) |
| `+0x90` | wheel angle (`>> 8`; the rim redraw rotates by it) |
| `+0x94` | AI: target heading (14-bit) (4) |
| `+0x98` | AI: curve term = angle between the next two line segments × 0x14 >> 4 (4) |
| `+0xA8` | position |
| `+0xAC` | distance |
| `+0xB4` | best lap |
| `+0xB8` | lap start |
| `+0xBC` | finish time |
| `+0xC5` | laps left |
| `+0x11C` | vector (x, y, z, 2.12) dotted with the racing-line direction for the wrong-way test; also read by the speed effect on the focal (hypothesis) |
| `+0x140` | second vector for the wrong-way test (used when the first is near 0) |
| `+0x434` | exhaust flame animation (−1 off) (2) |
| `+0x436` | nitro flame animation (−1 while nitro is off) (2) |
| `+0x444` | AI: side of its next crossing line |
| `+0x454` | needle rev scale |
| `+0x4B0` | AI stuck counter (0x32: `ai_resync_segment` `FUN_0813F530`; over 0x96/0xFA: `car_put_back_on_road`) |
| `+0x4B8` | AI: grip scale 0..0x100 (+4 per step) of the base grips (4) |
| `+0x4C8` | nitro tank (`car_nitro_drain`; the HUD dial shows it) |
| `+0x4D4` | AI: lane-change timer (reset to race time & 0x1F; over 300 marks its lane blocked) (2) |
| `+0x4D6` | AI: preferred lane (2 when it changed section) |
| `+0x4D8` | race flags: bit 0 backwards past the start, bit 1 lap armed, bit 3 knocked out |
| `+0x4DA` | AI: blocked lanes 0..4 (set each step for the next) (5 × 0x2) |
| `+0x4E4` | steps tipped over (car_tipped_dynamics); over 100 with a corner down and slow: car_put_back_on_road (2) |
| `+0x4E6` | tipped steps with no corner down (airborne); landing sound 0x16 or 0x19 after more than 4 (2) |
| `+0x4E8` | hunter life (clamped at 0; nothing else happens at 0) |
| `+0x4EC` / `+0x4EE` | wrong-way frames / wall frames |
| `+0x4F0` | AI: follow timer (with `+0x4F4`); cleared by hunter hits and drains |
| `+0x4F2` | AI: hunter countdown (0x28) (2) |
| `+0x4F4` | AI: the entity followed (4) |
| `+0x4F8` | AI: boost timer (2) |

### Profile (`*0x030056EC` = `0x02000808`)

| Offset | What |
|---|---|
| `+0x10` / `+0x11` | player's car in career / Quick Play (0x81 copies it to 0x03005718 and 0x0300611C); screen 28 sets +0x11 to a random unlocked car (i8) |
| `0x14` | car_extra: 15 cars x 15 bytes (save 0x11C..; MenuProfile::car_extra) (225) |
| `0xF5` | field_f5 (save 0x104) (4) |
| `+0xF9` | car records, 0x11 bytes per car id |
| `+0x1F8` | career hints seen (1) |
| `+0x1F9` | career hints pending (added to +0x1F8 on save) (1) |
| `+0x1FA` | hint page within the current hint; cleared by hint_due (1) |
| `+0x1FC` | event slot of the selected career event (1) |
| `+0x200` | wingman selection |
| `+0x205` | event status (2 bits per event) |
| `+0x218` | record times |
| `+0x254` | map grid cursor of the zone-3 hint (0..11) (2) |
| `+0x256` | advance the zone when the hints finish (2) |
| `+0x258` | upgrades save question open (2) |
| `+0x26C` | AI drive-force curve (count 0x15; x0 0; x1; pointer to +0x27C; 21 values) (0x60) |
| `+0x2EE` | race music choice, i8 (`rand & 3` at race start; music id = choice + 1; resuming a race plays it again) |
| `+0x2EF` | engine sound id (car table +0x48; 0x7EEA44 +8 in link play) (1) |
| `0x2F4` | camera-reset flag: at the race end in phase 6 it is cleared and the camera setting 0x030053E4 = 1 (1) |
| `+0x2F6` / `+0x2F8` | car select turn: speed (0x80) / target angle; cleared on every screen enter (u16 / u32) |
| `+0x318` | sparks: the car's matrix code spawns two sparks while it is > 0 (4 per racer) |
| `0x328` | loaded: the profile was decoded from a save (save_load_profile) (1) |
| `+0x338` | cleared when an upgrade page opens (4) |
| `+0x33C…+0x343` | key-repeat delays (3 on a new press; count down each menu frame): keys 0x20 0x10 0x40 0x80 1 2 0x200 0x100 (8 × i8) |
| `+0x344…` | menu back stack (screen ids) (bytes) |
| `+0x350…` | List screens' cursor slots (list_slot) (bytes) |
| `+0x350` | List cursor per list_slot (17) |
| `+0x364` | upgrade page selections (+0x364 performance; +0x365 visual/aero/accessories) (2) |
| `+0x368` | setup screen cursors (one per setup record) (6) |
| `+0x374` | setup arrow delays (left/right per item) (0x10) |
| `+0x388` | career event cursor per zone (6) |
| `+0x3B0` | intro screen deadline (tick counter 0x03000044 + 0xF0/0x5A/…) (u32) |
| `+0x3B4` | new track record flag (4) |
| `+0x3B8…+0x3F8` | setting values |
| `+0x3B8` | last race payout (4) |
| `+0x3BC` | settings copies edited by the setup screens (setting_variable ids 2..0x10) (0x3C) |
| `+0x400` | the camera sector has a ceiling (`camera_update`; `+0x401` last frame's); both cleared at race start (2) |
| `+0x402` | needle scale (0x2000 instead of 0x1C00) |
| `+0x404` | map mode: 0 pick a track, 1 pick a district, 2/3 record pages; 3 on screen 0x11: B calls `FUN_081439C0` instead of going back (u8) |
| `+0x42D` | unlock bits (40 bytes) |
| `+0x450` | bits the hint conditions test (4) |
| `+0x478…+0x48C` | cleared on entering the name screen unless +0x494 is 2 (6 × u32) |
| `+0x490` | a profile exists (title START loads it; else name entry) (u16) |
| `+0x494` | profile/name page mode: 1 new, 2 rename (came from the menus; OK goes back) (u16) |
| `0x496` | slot_names: saved names of save slots 0 and 1 (9 bytes each) (18) |
| `+0x4A8` | record message pending (results page 0xB) (2) |
| `+0x4AA` | unlock message text keys (u16; 0 ends) |
| `+0x4E8` | language of the saved profile (title: units follow the language when they differ) (u16) |

## Functions

Names are ours; addresses are the Ghidra `FUN_xxxxxxxx`. The IWRAM ones exist in the Ghidra project because the race IWRAM dump is loaded; `FUN_08166c04` = `FUN_03001cf0` (ROM copy, delta `0x164F14`).

See [symbols.csv](symbols.csv) for the full list with one-line descriptions.
