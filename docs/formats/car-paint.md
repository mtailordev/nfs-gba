# Car paint and the car palette slots (Carbon `BN7E`)

**Status: exact and verified against the reference build.** Code: `nfsgba_formats::paint`.

The tests rebuild the reference race's palette from the ROM and the race state:
- `car_slots_reproduce_the_race_palette`: base buffer slots 160–255 and palette RAM slots 160–255, entry for entry.
- `glass_shades_follow_the_heading`: glass shades at five more headings, from a driving session.
- `player_atlas_is_remapped_into_the_car_slots`: the player's unpacked atlas.
- `sine_table_quadrants`.

Everything is raw BGR555. ROM offsets are file offsets (GBA address minus `0x08000000`).

## Palette slots in a race

`FUN_0813b6d0` ("load_car_palettes") copies ramps into both base palette buffers (`*0x030055F0` = `0x02001008` and `*0x0300577C` = `0x02000E04`). The light tint then derives palette RAM from the first buffer (`apply_sector_light_to_palette`, slots 1–143 and 149–255).

| Slots | Filled with | Source |
|---|---|---|
| 160–175 | ramp `paints[1]`; when `cars[1] ≥ 15`, ramp `special[cars[1] − 15]` | `FUN_0813b6d0` |
| 176–191 | ramp `paints[2]` | `FUN_0813b6d0` |
| 192 | glass shade at `angle` | `shade_car_paint` (overwrites the city colour) |
| 193–207 | the city palette's own colours (trim: greys, head lights, tail lights). Nothing car-specific writes them | city palette 3 in the reference race |
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
| `0x7C03F0` | 256 u16 random numbers (`FUN_0815fcfc` returns `table[++*0x030064C8 & 0xFF]`) |
| `0x7F4344` | u32 per event: opponent 1's paint (`FUN_0813b634`) |
| `0x7EF5A0` / `0x7EF672` | i16 overlay material per `(car·7 + record[1])` and its (x, y) in the atlas, 4 bytes per `(car·0x1C + record[1]·4)`. −1 = none. Materials 110–115; cars 7, 8, 13 and 14 have a default (106–109) |
| `0x7EF816` | decals: 0x10 bytes per `(car·15 + record[2])`: `[x0, y0, material, ?, x1, y1, material, ?]` as i16. The decal is drawn twice, at (x0, y0) and at (x1, y1) |
| `0x7EEBBC` | record `[4]` decal sets: 3 i16 vehicle-material indices per set at `(rec[4] − 1)·6`, −1 = none (`FUN_0813b828`) |
| `0x7EEB70` | i16 (x, y) of such a material in the atlas at `car·0x9C + 4·index` |

## RAM state

| Address | What |
|---|---|
| `0x0300611C` | `cars`: i8 car id per racer. [0] is the player (copied to entity 0 `+0x89` by `race_init`); [1..3] are the opponents |
| `0x03005FEC` | `paints`: i8 paint per racer. `race_init` sets [0] = the player's record `[6]` (also into `0x030000B8`) |
| `0x0300539C` | pointer to the car records: `0x02000901`, 0x11 bytes per car id. The save block `*0x030056EC` = `0x02000808` holds them at `+0xF9` |
| `0x03005700` | the car record being edited in the garage (the `param != 0` / garage path) |
| `0x03005784` | opponent count (3) |
| `0x03006104` | event number (`FUN_0813b634` reads `0x7F4344[n − 1]`) |
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
| `[1]` | overlay material (`0x7EF5A0`) | body part (spoiler/rims?) |
| `[2]` | decal (`0x7EF816`) | vinyl |
| `[3]` | added to car table `+0x0C` for the atlas material | body variant |
| `[4]` | 1-based extra row for slots 240–247 and decal set `0x7EEBBC` | decal colour set |
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

### Player atlas: `FUN_0813b828(world, racer, rec)`

1. `entity[+0x48] = car_table[car].+0x0C + rec[3]`.
2. A buffer of w·h bytes is allocated (pointer in `0x03006164[racer]`), and `FUN_08163d30` decompresses the material into it.
3. `FUN_08163e5c(buf, n, 0xD0 + 0x10·(racer % 3), 0xC0)` remaps each pixel p:
   - p < 16 → p + body;
   - p < 32 → p + trim − 16 (8-bit);
   - anything else is left unchanged.

   For player 0 this is `192 + (p ^ 16)`.
4. The overlay `0x7EF5A0[car·7 + rec[1]]` is blitted with `FUN_08163870` (key 0, `+0xD0`).
5. The `rec[4]` decals are blitted with `FUN_08163bfc` (`0xF0`, range `0xD0..0xDF`).

After that, `FUN_0813bf58` unpacks decal `0x7EF816[car·15 + rec[2]]` (pixels: `0x10 → 0`, else `+0xB0`, via `FUN_08163e34`). `FUN_0813bd90` then draws the decal rotated into the atlas twice, clipped to 1/8 inside its w×h box. In the reference race this changed 744 pixels, all inside the two boxes: decal material 46 at (28, 163) and (162, 163).

### Opponents: `FUN_0813b634` (from `race_start_from_table_a`)

```
k = rand() % 5
for i in 0..3:
  if i < *0x03005784:
    cars[i+1] = 3k + i
    paints[i+1] = rand() % 15
    for i = 0, then paints[1] = 0x7F4344[*0x03006104 − 1]
  else:
    cars[i+1] = paints[i+1] = 0
```

In the reference race `cars = [2, 9, 10, 11]` and `paints = [11, 11, 11, 5]`.

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

In the reference race the opponents (entities 1–3, `+0x84` = 0) use materials 140, 141 and 142. So they are drawn with ramps 176 (paints[2]), 208 (the player's paint) and 160 (paints[1]).

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
- The decal draw (`FUN_0813bd90`, rotated blit `FUN_0815e770`), the overlay blit (`FUN_08163870`, formats 3/4/5 and the compressed flag `0x40`) and the `rec[4]` decals (`FUN_08163bfc`) are decoded above but **not implemented**. The atlas test excludes the decal boxes.
- How `FUN_0813b9b8` picks an opponent's material (140–142 here), and why material 141 uses the player's ramp.
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
- `0x7F4344`: opponent-1 paint per event.
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
