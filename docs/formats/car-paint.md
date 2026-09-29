# Car paint and the car palette slots (Carbon `BN7E`)

**Status: exact and verified against the reference build.** Code: `nfsgba_formats::paint` (palette slots) and `nfsgba_formats::atlas` (the player's atlas with overlay, decal set and rims; the opponents' cars, paints and textures).

The tests rebuild the reference race's palette from the ROM and the race state:
- `car_slots_reproduce_the_race_palette`: base buffer slots 160–255 and palette RAM slots 160–255, entry for entry.
- `glass_shades_follow_the_heading`: glass shades at five more headings, from a driving session.
- `sine_table_quadrants`.

And the car textures (`atlas`):
- `player_atlas_matches_every_race_start`: all 51,200 atlas pixels at four race starts, covering four cars/records, two overlays, two decal sets and three rims.
- `rim_redraws_match_the_drive`: 76 in-race rim redraws while driving, including the texels the game reads from outside the rim buffer.
- `opponents_match_every_race_start`: the opponents' cars and paints (from the traced rand index), plus every opponent entity's material, model and car id.
- `wingman_zero_reads_before_the_paint_table`.

Everything is raw BGR555. ROM offsets are file offsets (GBA address minus `0x08000000`).

## Palette slots in a race

`FUN_0813b6d0` ("load_car_palettes") copies ramps into both base palette buffers (`*0x030055F0` = `0x02001008` and `*0x0300577C` = `0x02000E04`). The light tint then derives palette RAM from the first buffer (`apply_sector_light_to_palette`, slots 1–143 and 149–255).

| Slots | Filled with | Source |
|---|---|---|
| 160–175 | ramp `paints[1]`; when `cars[1] ≥ 15`, ramp `special[cars[1] − 15]` | `FUN_0813b6d0` |
| 176–191 | ramp `paints[2]` | `FUN_0813b6d0` |
| 192 | glass shade at `angle` | `shade_car_paint` (overwrites the city colour) |
| 193–207 | the city palette's own colours (trim: greys, head lights, tail lights). Nothing car-specific writes them | city palette 13 (= palette 3) in the reference race |
| 208 | glass shade at `angle + 0x1000` | `shade_car_paint` (overwrites ramp colour 0) |
| 209–223 | the player's ramp, colours 1–15 | `FUN_0813b6d0` |
| 240–247 | extra row `record[4] − 1`, only when `record[4] > 0`; otherwise the city colours stay | `FUN_0813b6d0` |
| 248–255 | extra row `cars[1] − 15`, always, even for an ordinary opponent (then the row lies before `EXTRA_ROWS`) | `FUN_0813b6d0` |

The player's atlas pixel `i` (0–31) is drawn with slot `192 + (i ^ 16)`: body 0–15 lands on 208–223 and trim 16–31 on 192–207. The opponents' 128×100 atlases already hold final slot numbers (see "Opponent atlases" below).

## ROM data

| Offset | What |
|---|---|
| `0x36C75C` | `PAINT_BLOCK`: the second palette block (level record `+0x04`; the code uses the literal `0x0836C75C`) |
| `0x36C95C` | `PAINT_RAMPS` = block + 0x200: 16-colour ramps, 0x20 bytes each, indexed by paint number. Ramp 11 is the reference race's red. Ramp 0 is greys |
| `0x36C95C + 0x20·g + 0x18` | glass colour `g` = colour 12 of ramp `g` (`shade_car_paint` reads block `+0x218 + 0x20·g`) |
| `0x36CD5C` | `EXTRA_ROWS` = block + 0x600: 8-colour rows for slots 240 and 248 |
| `0x7EEA24` | bytes: special ramp numbers, indexed by `cars[1] − 15` (slot 160) or by `cars[0]` (slot 208 when the paint code is ≥ 20) |
| `0x7F0BD8 + 0x58·car + 0x0E` | i16 palette bank of the car's own ramp: slot 208 = block + 2·(0x100·bank + 0x10·paint). The bank is 1 for all 15 cars, so the player's ramp is `PAINT_RAMPS` too |
| `0x7C05F0` | sine table: 0x2000 i16, the first half wave, 0x4000 = 1.0 (`FUN_0815f948`) |
| `0x7C03F0` | 256 u16 random numbers (`rand_table` `FUN_0815fcfc`: `index = (index + 1) & 0xFF`, then `table[index]`; index at `0x030064C8`) |
| `0x7F4344` | i32 per **wingman** 1..12: opponent 1's paint (0..11; `pick_opponent_cars` stores the low byte). With no wingman (0) the game reads index −1, the word at `0x7F4340` = 11 |
| `0x7EF5A0` / `0x7EF672` | i16 overlay material per `(car·7 + record[1])` and its (x, y) in the atlas, 4 bytes per `(car·0x1C + record[1]·4)`. −1 = none. `record[1]` 1..6 give materials 110–115 for every car; 0 gives the car's default: 106–109 for cars 7, 8, 13 and 14, none for the others |
| `0x7EF816` | **rims**: 0x10 bytes per `(car·15 + record[2])`: `[x0, y0, material, ?, x1, y1, material, ?]` as i16. One material (46–60, all 40×40) drawn at the two wheels, (x0, y0) and (x1, y1); the second material is not read. x0 = −1 means no rim |
| `0x7EEBBC` | decal sets (vinyls) for `record[4]`: 3 i16 vehicle materials per set at `(rec[4] − 1)·6`, −1 = none. Sets use materials 67–93 |
| `0x7EEB70` | i16 (x, y) of decal-set material `m` in `car`'s atlas at `0x7EEB70 + 0x9C·car + 4·m`. Since `m` ≥ 67, the data really starts at `0x7EEC7C`: 39 (x, y) per car, for materials 67–105. The bytes at `0x7EEB70` itself belong to other tables |
| `0x7EEA44` | per car id, 0xC bytes: u16 opponent material (a 128×100 raw atlas, 131–144 for cars 0–13; car 14 has 131, car 0's), u16 model index (entity `+0x36`), u16 car id (entity `+0x89`), u16 `id % 3`, u16 13, u16 0. Entries 15 and up (materials 116..) are the special cars |

## RAM state

| Address | What |
|---|---|
| `0x0300611C` | `cars`: i8 car id per racer. [0] is the player (copied to entity 0 `+0x89` by `race_init`); [1..3] are the opponents |
| `0x03005FEC` | `paints`: i8 paint per racer. `race_init` sets [0] = the player's record `[6]` (also into `0x030000B8`) |
| `0x0300539C` | pointer to the car records: `0x02000901`, 0x11 bytes per car id. The save block `*0x030056EC` = `0x02000808` holds them at `+0xF9` |
| `0x03005700` | the car record being edited in the garage (the `param != 0` / garage path) |
| `0x03005784` | opponent count (3) |
| `0x03006104` | **wingman** (0 = none, 1..12); `pick_opponent_cars` reads `0x7F4344[wingman − 1]`. Written by `race_start_from_table_a` (career agent) |
| `0x030064C8` | `rand_table` index (0..255). `setup_race_cars` reseeds it with `*0x03000044 & 0xFF` (`FUN_0815fd1c`), after `pick_opponent_cars` |
| `0x03000044` | the rand seed that `setup_race_cars` applies (0x2603 in the reference race, 0x1605 and 0x14D5 in the car-atlas races) |
| `0x03005624` | flag: while non-zero no atlas is unpacked, and every racer, the player too, is dressed from `0x7EEA44` with its own car id. Meaning unknown (0 in every capture) |
| `0x030000A0` | flag: when 1, opponents get entry 0's car id instead of the player's (0 in every capture) |
| `0x03005650` | per racer: a copy of entity `+0x89` (`setup_race_cars`) |
| `0x0300565C` | per racer: 0xFF means an empty slot. Its entity gets flags 0, `+0x08` = 0 and `+0x4E` = 0xE (`00 01 02 03` in the captures) |
| `*0x030064CC` / `*0x030064D0` | heap: block descriptor table (8 bytes per block: u16 links and start/end word indices) and arena base (`FUN_08160af8`) |
| entity `+0x8C` → `+0x90` | pointer to the car's state; `+0x90 >> 8` is the wheel angle (0x4000 per turn) that the rim is rotated by. At race start it is 0 |
| entity `+0x36` | model index (opponents: `0x7EEA44` `+2`) |
| `0x030057F8` | index of the entity that `shade_car_paint` takes the heading from (0) |
| `0x03006164` | per racer: pointer to the unpacked atlas (player: `0x0201FB9C`, = entity `+0x84`) |
| `0x03006094` | per entity id: pointer to the unpacked decal (`FUN_0813bf58`) |
| `0x03005780` | race-over flag: `shade_car_paint` does nothing while it is set |
| `0x03005630` | palette fade step (i32; ±0x10, stepped by 2 towards 0 each frame in `FUN_0812ae64`). While it is non-zero, the tint and the shade write the second buffer instead of palette RAM. **Not a pointer** (the address map says it is) |
| `0x0300563C` | dirty flag: when set, `FUN_0815dfd8` copies `*0x0300577C` to palette RAM (only when `0x03005808 != 5`) |
| `0x03005808` | game state; 5 in a race, where `FUN_0812ae64` tints every game frame |
| entity `+0x2C` | heading, 8.8 fixed point; `>> 8` gives 0x4000 per turn |
| entity `+0x48` | material (atlas): the player's = car table `+0x0C` + record `[3]`; the opponents' 140/141/142 in the reference race |
| entity `+0x84` | pointer to the unpacked, remapped atlas (player only; 0 for the opponents) |
| entity `+0x89` | car id: indexes the car table and the car records |

### Car record (0x11 bytes)

| Byte | Use in code | Meaning (hypothesis) |
|---|---|---|
| `[1]` | overlay material (`0x7EF5A0`), drawn `+0xD0` | body part (hood/spoiler?) |
| `[2]` | rim (`0x7EF816`), redrawn rotated as the wheels turn | rims |
| `[3]` | added to car table `+0x0C` for the atlas material | body variant |
| `[4]` | 1-based: decal set `0x7EEBBC`, drawn `+0xF0` over body pixels only, and its colours, extra row `[4] − 1` in slots 240–247 | vinyl |
| `[5]` | glass colour index | window tint |
| `[6]` | paint number. From 20 up, `special[cars[0]]` is used (race path only) | paint |

In the reference race the record for car 2 (Chevy Cobalt SS) is `00 00 00 00 00 00 0b 00 …`: paint 11, glass 0.

## Algorithms

### `FUN_0813b6d0(race)`: load the car ramps

`FUN_0812b1d8(slot, src, n)` copies `n` colours into both base buffers and sets `0x0300563C`.

```
rec   = race ? records + cars[0]·0x11 : 0x03005700
slot 160 ← cars[1] > 14 ? RAMPS + 0x20·special[cars[1] − 15] : RAMPS + 0x20·paints[1]      (16)
slot 176 ← RAMPS + 0x20·paints[2]                                                        (16)
slot 208 ← race && records[cars[0]][6] > 0x13 ? RAMPS + 0x20·special[cars[0]]
           : BLOCK + 2·(0x100·car_table[cars[0]].bank + 0x10·paints[0])                  (16)
if rec[4] > 0: slot 240 ← EXTRA + 0x10·(rec[4] − 1)                                      (8)
slot 248 ← EXTRA + 0x10·(cars[1] − 15)                                                   (8)
```

`cars` and `paints` are read with `ldrsb` (signed), and the special table with `ldrb`.

Callers:
- `race_init` twice: once through `FUN_0813b9b8`, then again after it sets `paints[0]`;
- `FUN_0813bc38` (`race = 1`);
- `FUN_0812bf48`, the garage (`race = 0`);
- `race_menu_palette_setup`.

### `shade_car_paint` (`FUN_081387e4`): glass shades

- **Arguments:** `(world, entity, garage, angle)`.
- **Record:** `garage ? 0x03005700 : records + 0x11·(world+0x3C)[+0x89]`, the record of entity 0.
- **Race-over gate:** nothing happens when `*0x03005780 != 0`.
- **Colour:** `c = block[0x218/2 + 0x10·rec[5]]`, split as `r = (c & 0x1F) << 3`, `g = (c & 0x3E0) >> 2`, `b = (c & 0x7C00) >> 7`.
- **Angle:** `a = garage ? angle : entity[+0x2C] >> 8`.
- **For each of `a` (slot 192) and `a + 0x1000` (slot 208):**
  - `s = clamp(|sin_q14(a) >> 9|, 16, 28)` (arithmetic shifts);
  - the colour is `((s·g) >> 8) << 5 | (s·r) >> 8 | ((s·b) >> 8) << 10`.
- **Writes:** slot `+0x180`/`+0x1A0` of both buffers, then palette RAM through `FUN_0815e02c(0xC0/0xD0, colour)` unless `*0x03005630 != 0`.

Callers:
- race frame `FUN_0813a954`: `shade_car_paint(world, entities + *0x030057F8 · 0xA4, 0, 0)`;
- `FUN_0813accc`: `(.., 1, *angle)`;
- the garage turntable `FUN_0812bfa4`: `(.., 1, *angle)`;
- `FUN_0813adf4`.

`sin_q14` (`FUN_0815f948`): `a &= 0x3FFF`; `a ≤ 0x1FFF ? table[a] : −table[a − 0x2000]`. Examples: sin(0x1000) = 0x4000, so s is clamped to 28; sin(0x2000) = 0, so s is clamped to 16.

### Race start order (traced)

`pick_opponent_cars` → `setup_race_cars`: reseed rand, then `unpack_player_atlas(world, 0, record)`, `load_car_palettes(1)`, then dress the four racer entities. After that `unpack_decal` unpacks the rim and draws it once, at angle 0.

### Material decoding

`decompress_material` (`FUN_08163d30`) allocates a 0x1011-byte scratch buffer and runs ARM `0x08169208` (IWRAM `0x030042F4`, through `FUN_0815e6c8`). This is GBA LZ77 (header `0x10`, size in bits 8–31) decoded through a 0x1000-byte ring. The ring is prefilled with 0xFF, writing starts at `0xFEE`, and a reference reads `ring[(pos − disp − 1) & 0xFFF]`.

Facts that make `lz77` in `lib.rs` equal to it for vehicles:
- no vehicle stream refers back before its start, so the 0xFF prefill is never read;
- all 146 vehicle materials are either flag `0x40` (compressed, 110) or format 5 (raw 8bpp, 36);
- so the nibble format 3 and the format-4 decoder (`FUN_081636d0`) in `blit_material_keyed` are never reached.

**Overrun:** the decoder writes the full header size, `w·h + 8`, into a `w·h` buffer. The 8 extra bytes land in the one-word gap after the buffer and the first word of the next heap block. The gap keeps them: after the rim buffer it holds `41 00 c9 00`, the rim stream's overrun (checked). The next block's own data overwrites the second word.

### Player atlas: `unpack_player_atlas` (`FUN_0813b828(world, racer, rec)`), `atlas::player_atlas`

Nothing happens while `*0x03005624 != 0`.

1. `car = cars[racer]` (signed byte). The player's entity (`*0x03000060`) gets `+0x48 = car_table[car].+0x0C + rec[3]`.
2. A buffer of w·h bytes is allocated (pointer in `0x03006164[racer]`, freed first if set), and the material is decompressed into it.
3. `remap_atlas_pixels(buf, n, 0xD0 + 0x10·(racer % 3), 0xC0)` remaps each pixel p:
   - p < 16 → p + body;
   - p < 32 → p + trim − 16 (8-bit);
   - anything else is left unchanged.

   For player 0 this is `192 + (p ^ 16)`.
4. **Overlay:** if `m = 0x7EF5A0[car·7 + rec[1]] ≥ 0`, `blit_material_keyed(atlas + y·256 + x, 0x100, m, texbase, key 0, add 0xD0)` at `(x, y) = 0x7EF672[car·7 + rec[1]]`. Every texel ≠ 0 is written as texel + 0xD0. The blit is row by row with stride 256 and no clipping.
5. **Decal set:** if `rec[4] > 0`, for each `m ≥ 0` of set `rec[4] − 1`: `FUN_08163bfc(atlas + y·256 + x, 0x100, m, texbase, key 0, add 0xF0, lo 0xD0, hi 0xDF)` at `(x, y) = 0x7EEB70 + 0x9C·car + 4·m`. It writes texel + 0xF0 only where the texel ≠ 0 **and** the atlas pixel under it is in 0xD0..=0xDF (body, not glass or trim).

`FUN_08163990` is the same blit with the opposite test: it writes where the atlas pixel is outside lo..=hi. It is not used here.

### Rims: `unpack_decal` (`FUN_0813bf58`), `draw_decal_on_atlas` (`FUN_0813bd90`), `atlas::rim_pixels` / `draw_rim`

**Unpack.** The record is `0x03005700` in the garage (third argument ≠ 0) or `records + 0x11·entity[+0x89]`, and the entry is `0x7EF816[entity[+0x89]·15 + rec[2]]`.
- Material `entry[2]` is decompressed into a new buffer at `0x03006094[entity[+0]]`.
- `remap_decal_pixels(buf, n, add 0xB0, key 0x10, value 0)` maps texel 0x10 → 0 and every other texel → texel + 0xB0.
- Then it draws.

**Draw.** Nothing happens unless the atlas `0x03006164[entity[+0]]` is set and `entry[0] ≠ −1`.
- **Geometry:** `w, h` = the rim material's size and `a` = the wheel angle (entity `+0x8C` → `+0x90 >> 8`, 0 for a null pointer). The destination box is inset by 1/8: `x0 = e0 + w/8`, `y0 = e1 + h/8`, `x1 = e0 + w − w/8`, `y1 = e1 + h − h/8` (all `>> 3`).
- **Steps (16.16):** `du/dx = cos a << 2`, `dv/dx = sin a << 2`, `du/dy = −sin a << 2`, `dv/dy = cos a << 2`. Here `cos` is `FUN_0815f988` = `sin_q14(a + 0x1000)`.
- **Start:**
  - `dx = (w/8 − (w − w/8))·2` and `dy = (h/8 − (h − h/8))·2`;
  - `u0 = cos(−a)·dx + sin(−a)·dy`;
  - `v0 = −sin(−a)·dx + cos(−a)·dy`.
- **Second placement offset:** `off = (e5 − e1)·256 + (e4 − e0)`.
- **Blit:** ARM `0x08169988` (IWRAM `0x03004A74`, through `FUN_0815e770`) returns −1 when `x0 == x1` or `y0 == y1`. Otherwise, row by row, each pixel reads `src_centre[w·(v >> 16) + (u >> 16)]`, where `src_centre = buf + (h >> 1)·w + (w >> 1)` and the shifts are arithmetic. A texel ≠ 0 is written to `dst[p + off]` and then to `dst[p]`.
- **No source bounds.** For 40×40 rims, some angles read up to 63 bytes before and 62 after the buffer. The texel-0 skip means earlier draws stay visible where the current one is transparent.

**What the out-of-buffer reads see (reference race).** The heap has no inline headers and leaves one free word between blocks, so the neighbourhood is:
- offsets −63..−5: the previous block's data;
- offsets −4..−1: the stale gap word (`11 11 11 11`);
- offsets 1600..1603: the rim stream's decoder overrun (`41 00 c9 00`);
- from 1604: the next block, which in the reference race is opponent 1's car state (entity 1 `+0x8C` = `0x0202D168`). It changes every frame.

These texels do reach the atlas. In the drive trace they were read 47 times, and zeroing them breaks the redraw test. The car-paint driving dumps `d0`–`d39` show such a texel, value 43, at (34, 168) and (168, 168). `draw_rim` therefore takes the RAM around the buffer as `memory`.

**When it is redrawn.** In races `FUN_0814a2a0` and `FUN_0814b168` redraw the player's rim (entity index `*0x03000060`) each game frame in which `FUN_0814f9d0(heading, 0x4000 − *0x03005F9C) != 0`. The heading is `(entity[+0x2C] & 0x3FFFFF) >> 8`. `FUN_0814f9d0` returns 0 when the angle difference (`FUN_0815fc38`, then `abs`) is within (−0x400, 0x400) or (0x1C00, 0x2400), i.e. when the car is seen almost straight on. This rule is read from the code; the trace only confirms a redraw every game frame while driving in a turn. `FUN_0814a390` and `FUN_0814b98c` call `unpack_decal` again (tentative: car reset).

### Opponents: `pick_opponent_cars` (`FUN_0813b634`), `atlas::pick_opponent_cars`

Called from `race_start_from_table_a`.

```
k = rand() % 5                                   (rand: rand_table, u16; __umodsi3)
for i in 0..3:
  if i < *0x03005784:                            (unsigned)
    cars[i+1] = 3k + i
    paints[i+1] = rand() % 15
    if i == 0: paints[1] = low byte of i32 0x7F4344[wingman − 1]    (wingman 0 reads 0x7F4340 = 11)
  else:
    cars[i+1] = paints[i+1] = 0
```

It uses `1 + opponents` rand calls.

| Race | rand index at entry | cars | paints (`[0]` set later) |
|---|---|---|---|
| reference | (not traced) | 2, 9, 10, 11 | 11, 11, 11, 5 |
| `start1` | 0x5A | 0, 12, 13, 14 | 5, 11, 12, 9 |
| `pokev1` / `pokev2` | 0x59 | 0 or 7 (poked), 0, 1, 2 | 5 or 6, 11, 14, 12 |

### Dressing the racers: `setup_race_cars` (`FUN_0813b9b8(world, _, one, param_4)`), `atlas::look`

`race_init` calls it with `(world, *0x08139ab8, 0, 0)`. The steps:
- reseed rand;
- `has = unpack_player_atlas(world, 0, records + entity0[+0x89]·0x11)`;
- `load_car_palettes(1)`;
- for racers `i` = 0..3 (only the player's when `one ≠ 0`):
  - **empty slot** (`0x0300565C[i] == 0xFF`): flags `+0x0A` = 0, `+0x08` = 0, `+0x4E` = 0xE.
  - **otherwise:** `+0x84` = 0, flags `&= ~8`.
    - **Player**, when `*0x03005624 == 0` and `param_4 == 0`:
      - `+0x89 = cars[0]`, `+0x84 = 0x03006164[0]`, flags `|= 8`;
      - `+0x36 = car_table[+0x89].+0x12` if `has`, else `.+0x10`;
      - `+0x64 = max(0, i16 0x7F0636[+0x89·0x10 + rec[0]])`.
    - **Opponent** (or anyone while `*0x03005624`):
      - `+0x48 = 0x7EEA44[cars[i]].+0` and `+0x36 = .+2`, flags `|= 2`;
      - `+0x89 = 0x7EEA44[X].+4`, where `X` is `cars[i]` while `*0x03005624`, 0 when `*0x030000A0 == 1`, and `cars[0]` otherwise. So in an ordinary race every opponent carries the player's car id.
  - then `+0x70` = 0x640, `0x03005650[i] = +0x89`, and `+0x4E` = 0 for the player (or while `*0x03005624`), 0x29 for opponents.
- `FUN_0813ff44` (an empty function).

`FUN_0813bc38` is the same player setup for the racer at `*0x03000060`, followed by `unpack_decal` via `FUN_0813ff48` (tentative: re-dressing the player's car mid-game). It was not traced.

### Opponent atlases

The 36 materials of 128×100 are raw 8bpp (format `+0x22` = 5, flags 0). Their pixels are already final palette slots (160–255), not 5-bit atlas indices.

Each uses one body ramp slot, baked in:

| Materials | Ramp slots used |
|---|---|
| 131, 136, 139, 142, 144 | 160 |
| 132, 134, 137, 140, 143, 145 | 176 |
| 133, 135, 138, 141 | 208 (the player's ramp) |
| 116–130 | 160 and 240 |
| 61–66 | a mix |

All of them also use 192–199 and 208 (trim and glass).

In the reference race the opponents (entities 1–3, `+0x84` = 0) use materials 140, 141 and 142, which is `0x7EEA44[cars[i]]` for cars 9, 10 and 11. So they are drawn with ramps 176 (paints[2]), 208 (the player's paint) and 160 (paints[1]). Which paint shows on which opponent is baked into these textures, not tied to the racer index.

## Frame timing (measured)

The source is the trace script under Provenance: mGBA breakpoints and palette watchpoints over 600 video frames of driving.

- **Main loop `FUN_0812ae64`:**
  1. `FUN_0812acec` runs the race frame `FUN_0813a954` (`shade_car_paint` early, then physics and rendering);
  2. then `apply_sector_light_to_palette`;
  3. and at once the next iteration.
- **Frame length:** one game frame took 4 video frames in the trace.
- **Tint:** writes slot 192 at VCOUNT 1–225.
- **Glass window:** the next `shade_car_paint` overwrites slot 192 with the raw shade 1–7 scanlines later (146 of 151 windows fell within one video frame).
- **Result:** for all but a few scanlines per game frame, palette RAM 192/208 hold the **raw** shade and every other car slot holds the tinted base.
- **Samples:**
  - The 49 dumps all show raw 192/208: the reference dump, `p0–p7` (standing) and `d0–d39` (driving).
  - At the start of VBlank, 595 of the 600 traced frames held the raw value. The other 5 held the tinted value: in those frames the tint ran at lines 154–159 and the shade followed after VBlank.
- **Screenshot:** in the `d39` screenshot the glass pixels (rows 108–120) show the raw colours.
- **Heading lag:** the shade uses the heading from before that game frame's physics step. A dump taken later in the frame shows the new heading with the previous frame's shades (d0 → d3, d3 → d7, d15 → d19 in the test).

`paint::race_palette` models this as `tint_palette(base)` with 192 and 208 raw.

## Not 1:1 / open

- `NOT 1:1` (`race_palette`): the 1–7 scanlines per game frame in which 192/208 show the tinted shade are not reproduced. That would need scanline timing of the whole frame.
- **Rim reads outside the buffer.** `draw_rim` is exact given the RAM around the rim buffer. A rewrite has that RAM only if it lays out the heap as the game does: allocation order, the gap words, the decoder overruns, and the live car state of the next block. Until a caller provides it, the texels read there (up to 63 bytes before and 62 after the buffer) are not reproduced.
- **Rim redraw rule** (`FUN_0814f9d0`, `FUN_0815fc38`, `*0x03005F9C`): read from the code, not yet reproduced or traced angle by angle. It belongs to the race frame (whoever implements the entity update).
- Not traced: the garage path (`unpack_decal(.., 1)`, record `0x03005700`), `FUN_0813bc38`/`FUN_0813ff48`, and the flags `0x03005624` / `0x030000A0` (0 in every capture).
- Not exercised: the lower bound 0xD0 of the decal-set test (no capture has decal texels over atlas pixel 0xD0); the format 3/4 paths (no vehicle material uses them).
- `0x7E6EEC` ("car paint presets" in `vehicle-models.md`, `paint_palettes`) is used only by `FUN_081348e8`, a menu function. It is **not** used by the race car palette. `car_palette` in `lib.rs` is superseded by this module.

## Provenance

- **ROM:** `BN7E` SHA-1 `e5298b24…`.
- **Emulator:** mGBA dev build `ext/mgba-dev`, session `car-paint` (`data/work/e5298b24/car-paint/`), from `race.ss`:
  - `load race`, then `dump p0` … `dump p7`;
  - `load race`, `hold A,LEFT 400`, then `dump d0` … `dump d39`, then `shot d39`;
  - a timing trace (`trace.txt`) from this script, loaded with `mGBA.exe --script cp_trace.lua -C mute=1 -C savestatePath=<session> <rom>` and `NFSGBA_MGBA_DIR=<session>`:

```lua
local dir = os.getenv("NFSGBA_MGBA_DIR")
local out = assert(io.open(dir .. "/trace.txt", "w"))
local started, frames = false, 0
local function log(what)
  if started then out:write(string.format("%d %3d %s\n", emu:currentFrame(), emu:read16(0x04000006), what)) end
end
callbacks:add("frame", function()
  if not started then
    emu:loadStateFile(dir .. "/race.ss")
    emu:setKeys((1 << 0) | (1 << 5)) -- A + LEFT
    started = true
    emu:setBreakpoint(function() log("shade_car_paint") end, 0x081387e4)
    emu:setBreakpoint(function() log("apply_sector_light") end, 0x0813a514)
    emu:setBreakpoint(function() log("frame_fn_0812ae64") end, 0x0812ae64)
    emu:setWatchpoint(function() log(string.format("write pal192 pc=%08x", emu:readRegister("pc"))) end,
      0x05000180, C.WATCHPOINT_TYPE.WRITE)
    emu:setWatchpoint(function() log(string.format("write pal208 pc=%08x", emu:readRegister("pc"))) end,
      0x050001a0, C.WATCHPOINT_TYPE.WRITE)
    return
  end
  frames = frames + 1
  log(string.format("frame-callback pal192=%04x base192=%04x", emu:read16(0x05000180), emu:read16(0x02001188)))
  if frames == 600 then out:close(); started = false end
end)
```
- **Decompilation:** read from `data/work/e5298b24/ghidra/carbon_decomp.c`. The Thumb code was checked with capstone for `FUN_0813b6d0`, `shade_car_paint` and `FUN_0815f948`.
- **Car atlases (car-atlas agent):**
  - Disassembled with capstone: `draw_decal_on_atlas`, `setup_race_cars`, `pick_opponent_cars`, `rand_table`, `FUN_0815f988`, `FUN_0814f9d0`, `FUN_08160af8`, and the ARM routines `0x08169208` and `0x08169988`.
  - Session `car-atlas` (`data/work/e5298b24/car-atlas/`, mGBA dev build) ran the trace script `scripts/ca_trace.lua` through `scripts/run.py` (start mGBA, wait for the plan, stop its own PID). The script logs the car-setup functions and, at each `draw_decal_on_atlas`, writes the atlas plus RAM `[rim − 4096, rim + 8192)` to `<tag>-rimNNN.bin`. The runs:
    - `drive`: `race.ss`, then A+LEFT for 600 frames (76 redraws);
    - `start1`: `mainmenu.ss`, then A four times 90 frames apart and a dump (`start1-d1`);
    - `pokev1` / `pokev2`: the same with RAM pokes at `setup_race_cars` (`CA_POKE`). `pokev1` is car 0 record `[1]=3, [2]=5, [4]=1`. `pokev2` is `cars[0]` and entity 0 `+0x89` = 7, with record 7 `[1]=0, [2]=3, [4]=2`. The ROM is never touched.
  - Python cross-checks, in the same `scripts/` folder:
    - `sim.py`: the whole atlas;
    - `chain.py`: the redraws;
    - `oob.py`: out-of-buffer offsets;
    - `opp.py`: the opponent table;
    - `overrun.py`: the decoder overrun;
    - `formats.py`: the material census.
  - Mutation check: each of these changes makes a test fail (a scratch script edits `atlas.rs`, runs `cargo test`, and restores it):
    - the overlay add;
    - the decal-set upper bound, or no range at all;
    - the sign of `du/dy`;
    - zeroing reads outside the rim buffer;
    - the rim remap;
    - the rand step;
    - the wingman index;
    - the opponents' car-id source.

## Integration notes

For the owners of `address-map.md`, `symbols.csv`, `FIDELITY.md` and `lib.rs`.

**FIDELITY:**
- **R3:** closed. `paint::load_car_palettes`, `glass_shades`, `race_palette` and `remap_atlas` are exact and verified (tests above). One `NOT 1:1` is left: the scanline window.
- **R4:** closed. The trim is the city palette's own slots 193–207, and 192/208 are the glass shades.
- **R13:** partly answered. The 128×100 materials are raw 8bpp in final palette slots (opponent atlases). The new remainder is decal/overlay drawing (`FUN_0813bd90`, `FUN_08163870`, `FUN_08163bfc`).
- **New entry:** the glass scanline window (see above).

**Address map, ROM rows:**
- `0x36C75C` (second palette block): `+0x200` paint ramps (0x20 each); glass = ramp colour 12; `+0x600` extra rows for slots 240/248.
- `0x7C05F0`: sine table, 0x2000 i16.
- `0x7C03F0`: random table, 256 u16.
- `0x7EEA24`: special ramp bytes.
- `0x7EEB70`, `0x7EEBBC`: decal sets.
- `0x7EF5A0`, `0x7EF672`: overlays and their positions.
- `0x7EF816`: decals.
- `0x7F4344`: opponent-1 paint per wingman (1..12; wingman 0 reads `0x7F4340`).
- Car table `+0x0E`: palette bank.

**Address map, RAM rows:**
- The globals: `0x0300611C`, `0x03005FEC`, `0x0300539C` → `0x02000901`, `0x02000808 + 0xF9`, `0x03005700`, `0x03005784`, `0x03006104`, `0x030057F8`, `0x03006164`, `0x03006094`, `0x03005780`, `0x0300563C`, `0x03005808`, `0x030064C8`, `0x030000B8`.
- Entity fields: `+0x2C` heading, `+0x48` material, `+0x84` atlas, `+0x89` car id.

**Address map, corrections:**
- `0x03005630` is the fade step, **not** a pointer. The second buffer is `*0x0300577C` = `0x02000E04`.
- The palette RAM row: 160–175 and 176–191 are ramps `paints[1]`/`paints[2]`; 193–207 are the city trim; 248–255 is the `cars[1] − 15` extra row, not HUD. `light_tint_reproduces_the_race_palette` can now include 248–255 via `race_palette`.

**symbols.csv:**
- `set_base_palette` targets `0x0300577C` and `0x030055F0`.
- New rows:

```
0x0813b6d0,load_car_palettes,function,copies car paint ramps to base palette slots 160/176/208 (16) and 240/248 (8)
0x0812b1d8,copy_to_base_palettes,function,copies n colours to slot k of both base palette buffers; sets dirty 0x0300563C
0x0815f948,sin_q14,function,sine: 0x4000 per turn; table 0x087C05F0 (half wave)
0x0815e02c,set_bg_palette_entry,function,palette RAM[idx] = colour when idx < 0x100
0x0815dfd8,copy_palette_to_ram,function,copies a 0x200-byte buffer to palette RAM
0x0815fcfc,rand_table,function,returns table 0x087C03F0[++*0x030064C8 & 0xFF]
0x0816a9b4,__umodsi3,function,libgcc unsigned modulo
0x08151454,vblank_intr_wait,function,swi 5
0x0813b828,unpack_player_atlas,function,unpack car material, remap into slots (FUN_08163e5c), overlays and decal sets
0x08163e5c,remap_atlas_pixels,function,p<16: p+body; p<32: p+trim-16
0x08163e34,remap_decal_pixels,function,p==key: value; else p+add
0x08163870,blit_material_keyed,function,unpack material (fmt 3/4/5, 0x40 compressed) and blit, skipping key, adding offset
0x08163d30,decompress_material,function,decompress via FUN_0815e6c8
0x0813bf58,unpack_decal,function,unpack decal 0x087EF816 into 0x03006094[id] (0x10->0, +0xB0)
0x0813bd90,draw_decal_on_atlas,function,rotated decal blit into the atlas, twice
0x0813b634,pick_opponent_cars,function,cars[1..3] = 3*(rand%5)+i, paints = rand%15; paints[1] from 0x087F4344
0x0813b9b8,setup_race_cars,function,tentative: sets racer entities (+0x89, +0x48, +0x84) and loads the player's atlas and palette
0x0813a954,race_frame_update,function,race frame: shade_car_paint first, then physics and rendering
0x0812ae64,main_frame,function,runs the state machine (FUN_0812acec) then the light tint in races
0x0812bfa4,garage_draw_car,function,tentative: garage turntable, shade_car_paint(.., 1, angle)
0x0812beec,garage_load_car_atlas,function,tentative: sets the car, unpacks atlas and decal
0x0812bf48,garage_load_car_palette,function,tentative: sets the car and paint, FUN_0813b6d0(0)
```

## Integration notes (car-atlas)

For the owners of `address-map.md`, `symbols.csv` and `FIDELITY.md`.

**FIDELITY:**
- **R13:** close. What closes it:
  - the player's atlas (material, remap, overlay, decal set, rims) is exact in `atlas::player_atlas`, `rim`, `rim_pixels` and `draw_rim`;
  - it is verified pixel for pixel at four race starts (four cars and records, two overlays, two decal sets, three rims) and over 76 in-race rim redraws;
  - the opponents' cars, paints, materials, models and car ids are exact in `atlas::pick_opponent_cars` and `look`, verified at four race starts.
- **New entry, rim reads outside the buffer:**
  - Now: `draw_rim` needs the RAM around the rim buffer (up to 63 bytes before and 62 after), and a caller without the game's heap layout cannot supply it.
  - Game: it reads the previous block's tail, the stale gap word, the decoder overrun and the next block's live car state (opponent 1's in the reference race).
  - Exact source: heap allocator `FUN_08160af8` (descriptor table `*0x030064CC`, arena `*0x030064D0`) and the allocation order at race start.
- **New entry, rim redraw rule:**
  - Game: redrawn every game frame unless `|angle_diff(heading, 0x4000 − *0x03005F9C)|` is within ±0x400 of 0 or of 0x2000.
  - Exact source: `FUN_0814a2a0`, `FUN_0814b168`, `FUN_0814f9d0`, `FUN_0815fc38`. Belongs to the race entity update.
- **R3 (viewer part):** opponents must be drawn with `look(..).material`, in final slots, with no remap; the player with `player_atlas` plus `draw_rim`.
- **Correction:** `0x03006104` is the wingman, not an event number (the car-paint notes said "event").

**Address map, ROM rows:**
- `0x7EEA44`: 0xC bytes per car id (u16 opponent material, model index, car id, `id % 3`, 13, 0).
- `0x7F4340`: the word read for wingman 0 (= 11); `0x7F4344`: i32 opponent-1 paint per wingman 1..12.
- `0x7EEB70`: correct the row. Decal-set positions are at `+ 0x9C·car + 4·material`, effectively `0x7EEC7C` for materials 67–105.
- `0x7EEBBC`: decal sets use materials 67–93.
- `0x7EF816`: these are **rims** (materials 46–60, 40×40, two wheels), not decals.
- Vehicle materials: 110 are flag `0x40` (LZ77) and 36 are raw format 5; formats 3/4 are unused.

**Address map, RAM rows:**
- `0x03000044`: rand seed applied by `setup_race_cars`.
- `0x03005624`: flag, no atlases, and all racers dressed from `0x7EEA44`.
- `0x030000A0`: flag, opponents take entry 0's car id.
- `0x03005650[4]`: per-racer copy of entity `+0x89`.
- `0x0300565C[4]`: per-racer byte, 0xFF = empty slot.
- `0x03005F9C`: angle used by the rim redraw test (camera?).
- `*0x030064CC` / `*0x030064D0`: heap descriptor table / arena.
- Entity `+0x8C` → `+0x90 >> 8`: wheel angle.
- Entity `+0x36`: model index.
- Entity `+0x64`: `i16 0x7F0636[car·0x10 + rec[0]]`, clamped at 0.
- Entity `+0x70`: 0x640 at setup.
- Entity `+0x4E`: 0 for the player, 0x29 for opponents, 0xE for empty slots.

**symbols.csv:**
- Rename `0x03004a74` from `blit_scaled_transparent` to `blit_rotated_twice`, comment "rotated 8bpp blit (16.16 steps, no source bounds) skipping 0, each texel also at +off; rim drawing".
- New rows (none duplicate the current file):

```
0x08169988,blit_rotated_twice_rom,function,ROM copy of IWRAM 0x03004a74 (ARM)
0x030042f4,lz77_decode_ring,function,ARM LZ77: 0x1000 ring prefilled 0xFF from 0xFEE; writes the full header size (w*h+8)
0x08169208,lz77_decode_ring_rom,function,ROM copy of IWRAM 0x030042f4 (ARM)
0x0815e6c8,iwram_call_4,function,calls the IWRAM copy of a ROM ARM function (overlay base 0x03006490) with 3 arguments
0x0815e770,iwram_call_8,function,calls the IWRAM copy of a ROM ARM function (overlay base 0x03006490) with 7 arguments
0x0815f988,cos_q14,function,cosine: sin_q14(a + 0x1000)
0x0815fd1c,rand_seed,function,*0x030064C8 = r0 & 0xFF
0x08160af8,heap_alloc,function,allocates ceil(n/4) words; descriptor table *0x030064CC (8 bytes per block), arena *0x030064D0; one free word between blocks
0x08160de4,heap_free,function,(tentative) frees a heap_alloc block
0x08163bfc,blit_material_keyed_inside,function,blit_material_keyed that writes only over atlas pixels in lo..hi (decal sets)
0x08163990,blit_material_keyed_outside,function,blit_material_keyed that writes only over atlas pixels outside lo..hi (not used by the atlas path)
0x08163e14,add_to_pixels_keyed,function,p != key: p + add
0x081636d0,unpack_material_format4,function,(tentative) format-4 decoder used by blit_material_keyed; no vehicle material uses it
0x08163d68,unpack_rle,function,(tentative) PackBits-like RLE: b&0x80 repeat next byte (b&0x7F)+1 times, else copy b+1 bytes
0x0813bc38,setup_player_car,function,(tentative) player-only dressing: material or look, unpack_player_atlas, load_car_palettes(1)
0x0813ff48,reload_player_car,function,(tentative) frees the decal, setup_player_car, unpack_decal
0x0813ff44,empty_0813ff44,function,empty; called at the end of setup_race_cars
0x0814a2a0,entity_update_rim,function,(tentative) race entity update; redraws the player's rim when rim_side_visible
0x0814b168,entity_update_rim_b,function,(tentative) second race entity update that redraws the player's rim
0x0814a390,entity_reset_rim,function,(tentative) calls unpack_decal for the player
0x0814b98c,entity_reset_rim_b,function,(tentative) calls unpack_decal for the player
0x0814f9d0,rim_side_visible,function,0 when |angle_diff(a, b)| < 0x400 or in (0x1C00, 0x2400), else 1
0x0815fc38,angle_diff,function,(tentative) signed difference of two angles (0x4000 per turn)
0x0815fadc,abs_i32,function,absolute value
```
