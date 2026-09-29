# Viewer rendering: indexed colour and the light tint

How `crates/nfsgba-viewer` puts colour on screen, what of it is exact, and what is not. It covers FIDELITY R1 (indexed colour) and R2 (the in-race light tint).

## How it works

### Indexed material

The GBA draws the 3D scene into a mode 4 bitmap: one palette index per pixel, shown through the 256-colour BG palette. The viewer does the same on the GPU.

- **`Indexed`** (`src/main.rs`) is a custom Bevy 0.19.1 `Material` with two textures:
  - `indices`: `R8Uint`, one palette index per texel;
  - `palette`: 256×1 `Rgba8UnormSrgb`.
  Both are read with `textureLoad`, so no sampler and no filtering is involved.
- **`src/indexed.wgsl`** is embedded in the binary (`embedded_asset!`), so the exe needs no `assets/` folder and runs from any directory. For each pixel it:
  - takes the texel `floor(uv * size)`;
  - wraps it with Euclidean modulo, which equals the game's `u & (w - 1)` for power-of-two sizes and also covers the 240-wide skyline;
  - discards index 0;
  - otherwise outputs `palette[index]`.
- **No culling:** the pipeline is specialised with `cull_mode = None`, because the winding of the ROM data is not normalised.
- **Exact colours on screen:**
  - A palette entry is BGR555, expanded to 8 bits per channel as `c << 3 | c >> 2`. `rom::palette_rgba` does this, and it is also what mGBA outputs.
  - The palette texture is sRGB and the target is sRGB, so the 8-bit value round-trips unchanged.
  - The camera runs `Tonemapping::None`, `DebandDither::Disabled` and `Msaa::Off`, so nothing blends, dithers or grades the palette colours.

### What uses which palette

| Surface | Index texture | Palette |
|---|---|---|
| City walls, floors, ceilings | `rom::city_textures` (row-major, column maps applied) | The shared city palette |
| Skyline band | The environment's 240×64 skyline | The shared city palette (colour 0 = sky, discarded) |
| Cars (showroom and grid) | Atlas pixel `i` stored as `192 + (i ^ 16)` (byte arithmetic) | The car's own palette: the city palette with slots 192..=223 replaced by the car's colours |
| Sky gradient | Not indexed: a 1×64 RGBA strip on a `StandardMaterial` | The game writes it per scanline (R5, sky agent) |
| Other models in the bank | Flat colour, `StandardMaterial` (debug display only) | None |

**Car colour interface.** `CarPalette { slots: [u16; 32], image }` is a component:
- `slots` holds raw BGR555 values for palette slots 192..=223, in slot order.
- Whoever decides a car's paint writes `slots`; change detection makes `tint` rebuild that car's palette.
- `car_slots()` converts any 32-colour palette indexed by atlas pixel into slots (slot `192 + j` = pixel `j ^ 16`). Its inputs are `rom::car_palette` for the grid cars and the paint presets for the showroom.
- This is the hook for the car-paint work (R3/R4).

### Light tint (`tint` system, every frame)

This reproduces `apply_sector_light_to_palette` (`FUN_0813a514`):

1. **Observer position** in city units, the game's `position >> 8`:
   - In race mode (`NFSGBA_ROUTE` set, or R pressed), the player's grid position `route.grid[0]` is used as is: integers from the ROM entity template, with no float round trip.
   - Otherwise the camera is used: `floor(x / SCALE)`, `floor(-z / SCALE)`.
2. **Observer sector:**
   - The current sector is kept while its outline still contains the point.
   - Otherwise every sector is tested with an exact integer even-odd point-in-polygon test. When several contain the point (overlapping levels), the one whose mean floor is nearest the observer's height wins.
3. **Multipliers:** `m = rom::sector_light(rom, sector, px, pz)`. If that is `None`, or no sector contains the point, the last `m` is kept, as the game leaves the palette untouched. Before any hit, the palette is the untinted one, as loaded.
4. **Palette rebuild,** only when `m` changes, the environment changes (K), or a car's `slots` change:
   - city palette: `palette_rgba(tint_palette(raw, m))`;
   - each car's palette: the same with its slots in 192..=223.
   - The bytes are written into the existing palette images. Bevy re-uploads them into the same GPU texture, so materials keep their bind groups.

**K** cycles the 12 environments. It swaps the raw city palette (which re-tints everything with the current `m`), the gradient material, the skyline index texture and the clear colour.

## Verification (reference race, route 23; checked with environment 1, whose palette and gradient equal the race's environment 11)

The scripts are in the session scratchpad `viewer-indexed/` (`compare.py`, `texzero.py`, `pairs.py`). The shot is `NFSGBA_ROUTE=23 NFSGBA_SHOT=route23.png`. The reference is `data/work/e5298b24/mgba/s15.png` with the dumps `race.*.bin`.

- **Sector and light:**
  - The viewer finds the player in **sector 760**. That is the value of the game's current-sector variable `0x03005614` in the reference race.
  - It computes `m = [4068, 4068, 3754]`. Every channel lies inside the range that the race's palette RAM allows (r, g: 3964..4095; b: 3740..3754).
- **Palette:** the viewer's tinted city palette equals the race's palette RAM in **178/178** checked entries (1..143, 149..159, 224..247; the other slots are rewritten at runtime by cars and the HUD).
- **Pixels:**
  - Every pixel of the viewer shot outside the two car boxes is either an exact tinted-palette colour or an exact sky-gradient colour.
  - The exceptions are 502 pixels of the debug racing line (gizmo).
  - Of 516,200 pixels in city-only boxes, 499,641 are palette colours; the remaining 16,559 are sky seen through the boxes.
- **Regions against s15**, measured as the share of s15 pixels whose colour the viewer also shows in the matching region:

  | Region | Share | Note |
  |---|---|---|
  | Kerb | 1282/1288 | Same entries (85, 149, 153, 154 on top) |
  | Left facade | 2750/2750 | |
  | Right facade | 3403/3650 | Top entries identical: 97, 100, 73, 77, 94, 76 |
  | Road | 560/768 | The s15 box also holds lane markings and colours near the car that the viewer's box (closer to the car, different framing) does not |

  A per-pixel comparison is not possible yet: projection, portal clipping and the raster resolution are still approximations (R10, R11 and the notes below).
- **Cars are not exact:** only 632 of 35,399 player-car pixels equal a race RAM colour of slots 192..223. The paint ramp is `rom::car_palette`'s reconstruction and the trim is paint preset 0, which matches no race slot. This is R3/R4, owned by the car-paint work; the colours plug in through `CarPalette::slots`.

## What is exact and what is not

**Exact (verified as above):**
- palette lookup: index → BGR555 → RGB8;
- the light multipliers and the tint;
- the tint range (1..=143, 149..=255);
- keeping the last palette when no light is found;
- car pixel → slot mapping `192 + (i ^ 16)`;
- nearest texel choice with wrap on power-of-two textures;
- no filtering, no MSAA, no tonemapping.

**NOT 1:1 (marked in code or listed here):**

1. **Sector lookup.** The game follows the player's sector through the portals it crosses (`0x03005614`). The viewer searches by position, which gives the same sector wherever sectors do not overlap. The camera observer exists only in the viewer: the game always uses the player's entity.
2. **Index 0.** The viewer discards index 0 on every indexed surface. The game does this:
   - A wall texture whose first stored texel is 0 uses the transparent column drawer `FUN_03004d48`. That drawer writes a horizontal pixel **pair** only when **both** texels are non-zero. All 227 city textures have first stored texel == pixel (0,0), so `pixels[0] == 0` identifies these textures.
   - Other walls use `FUN_03004db0`, which writes index 0 like any other index. That index then shows the backdrop (palette entry 0: `0x4A2E` in the race dump, probably the per-scanline sky colour). Only wall texture 92 is opaque and holds index 0.
   - Floors and ceilings (`FUN_03004fa8`) are always opaque, but no floor or ceiling texture contains index 0.
   - The difference is therefore limited to the pixel-pair rule at the edges of transparent walls, and texture 92's index-0 texels showing geometry behind them instead of the backdrop.
3. **Car slots.** Each car has its own 256-colour palette. The game has one palette, so four cars on screen must use different slots (160..=191 hold two more body ramps in the race dump). Which car uses which slots is R3/R4. Tinting a car's slots together with the city follows from the tint pass rewriting 149..=255 every frame.
4. **Non-power-of-two textures** (240-wide skyline, 128×100 vehicle materials) wrap by modulo. How the game treats them is not known.
5. **Raster resolution** (found here, belongs to the sector renderer):
   - Floors and ceilings are drawn at **half horizontal resolution**. `FUN_03004fa8` stores one byte per two pixels (8-bit VRAM writes fill both bytes of the halfword). In s15, 384/384 horizontally adjacent road pixel pairs that start at an even x are equal, against 129/360 for pairs starting at odd x.
   - Walls are drawn at full resolution: `FUN_03004d48`/`db0` write two texels from two columns.
   - The viewer renders everything at window resolution.

## Integration notes

### address-map.md additions

| Address | What |
|---|---|
| `0x03004d48` | Wall column drawer, **transparent**: writes a 2-pixel halfword (texels from two columns, same v) only when both texels are non-zero. Chosen by `raster_wall_columns` when the texture's first stored texel (runtime material `+0x28` pointer) is 0. |
| `0x03004db0` | Wall column drawer, **opaque**: the same without the test. |
| `0x03004fa8` | Floor/ceiling span drawer: one byte store every 2 bytes, so floors are drawn at half horizontal resolution (confirmed on s15 road pixels). No index-0 test. Called by `draw_flat_textured` directly (spans up to 32 pixels) and through `0x03002c40`. |
| `0x03002c40` | Splits long floor spans into 16-pixel pieces with perspective re-division, calling `0x03004fa8`. |
| `0x03004a74` | Scaled 8bpp blit that skips index 0 and writes each byte to two buffers (`p[0]`, `p[param_7]`). Probably sprites or the skyline; user not checked. |
| palette RAM `0x05000000` entry 0 | `0x4A2E` in the race dump. It is the backdrop that opaque index-0 texels show; the per-scanline sky gradient is suspected. |
| palette RAM 160..=191 | In the race dump, two copies of a 16-colour body ramp (`0015 3E1E …`). This supports the "other cars" hypothesis already in the map. |

### symbols.csv rows

```
0x03004d48,draw_wall_column_pair_transparent,function,wall column pair; writes only when both texels are non-zero (texture first texel 0)
0x03004db0,draw_wall_column_pair_opaque,function,wall column pair; always writes
0x03004fa8,draw_floor_span,function,floor/ceiling span; byte store every 2 bytes (half horizontal resolution)
0x03002c40,draw_floor_span_long,function,splits floor spans into 16-pixel pieces for draw_floor_span
0x03004a74,blit_scaled_transparent,function,scaled 8bpp blit skipping index 0; writes two buffers
```

### FIDELITY.md changes

- **R1 → Closed:** "Indexed colour: exact. Index textures plus a 256-entry palette texture (`Indexed` material, `indexed.wgsl`), nearest texel with mask-equivalent wrap, BGR555 expansion `c << 3 | c >> 2`, no filtering, MSAA, tonemapping or dither. Checked: every city pixel of the route-23 chase shot is an exact palette colour; region colour sets match s15 (`docs/engine/viewer-rendering.md`)."
- **R2 → Closed:** "Light tint in the viewer: exact. `sector_light` + `tint_palette` every frame at the observer (the player's car in race mode), palette kept on no light. Checked: route 23 gives sector 760 (as `0x03005614`) and multipliers inside the range the race palette RAM allows; tinted palette = palette RAM 178/178."
- **New R14:** "Index 0: the viewer discards it per pixel. Game: transparent wall textures (first texel 0, `0x03004d48`) skip a pixel pair unless both texels are non-zero; opaque walls (`0x03004db0`) write index 0 as the backdrop (only texture 92 has any); floors never hold index 0. Exact source: the column drawers."
- **New R15:** "Observer sector found by point-in-polygon; the game tracks the player's sector through portals (`0x03005614`). Exact source: the sector update in the player/camera code."
- **New R16:** "Floors and ceilings are drawn at half horizontal resolution (`0x03004fa8`, one byte per two pixels; confirmed on s15). The viewer draws at full window resolution. Belongs with R10/R11."
- **R3/R4:** add "Viewer hook: `CarPalette::slots` (raw BGR555 for slots 192..=223); one palette per car, which R3 must replace with the game's slot assignment (160..=191 for other cars)."

## Race palette and sky layer (R3, R5, R6)

This section supersedes the car and sky rows of "What uses which palette", the `CarPalette::slots` hook, and items 3 and 4 of the NOT 1:1 list above.

### How it works

**One race palette** (`Tint`, `tint` system), shared by the city, the four race cars and the skyline, built every frame in the game's order (`docs/formats/car-paint.md`, "Frame timing"):
1. `race_base`: the environment's city palette plus `paint::load_car_palettes` with the reference race's racers (`RACE_CARS` = [2, 9, 10, 11], `RACE_PAINTS` = [11, 11, 11, 5], the player's record with paint 11 and glass 0). These are inputs taken from the race dump; the game draws them from the RNG and the save.
2. Glass: slots 192 and 208 = `paint::glass_shades` for the player's heading.
3. `paint::race_palette(base, m)`: the light tint (R2), with 192 and 208 left raw. Before the first light the palette is `base`.

**Race cars.** They stand on the route's grid, facing the template entity heading (`+0x2C >> 8`, read by `grid_headings`).
- The player uses car 2, material 8 (car table `+0x0C` + record `[3]`), remapped by `paint::remap_atlas(.., 0xD0, 0xC0)`.
- The opponents use materials 140, 141 and 142 (entity `+0x48` in the reference race), drawn raw because their pixels are already final slots.
- All four draw model car table `+0x14` (our `models[1]`). This is `draw_sector_entities`' close model `+0x36 − 1`, the same for all four entities in the dump.

**Showroom.** Each car gets its own palette, as the garage builds it: `load_car_palettes` with `race = false`, paint number = car row, and glass at turntable angle 0. It is tinted like the city. This is a display only.

**Sky layer.** A quad follows the camera 20 km out, behind all geometry. It uses the `Indexed` material with `SCREEN | OPAQUE`:
- Its index texture is the 240×160 GBA screen as it is before the world is drawn: `sky::draw_skyline` on a cleared screen.
- Each window pixel reads the GBA pixel `floor((p − viewport origin) / viewport size · (240, 160))`.
- Index 0 shows the backdrop.

**Backdrop.** A 160×1 texture: line `y` = `bgr555(gradient_buffer[backdrop_entry(gradient_start, y)])`. Palette entry 0 is not tinted.

**Camera inputs** (`sky` system; redrawn when the environment, yaw or horizon changes):
- **Yaw:** the view direction in the game's convention, rounded. Heading 0 faces raw +z and 0x1000 faces +x; checked against the racing lines of all 44 routes.
- **Horizon:** `150 · tan(pitch)`, clamped to ±32 as `camera_update` does.
- **View:** 2 (chase). Shake is 0.

**Window.** 960×640, so every GBA pixel is exactly 4×4 window pixels. Other sizes scale each axis on its own; a window that is not 3:2 stretches the sky layer.

**Index 0 on world surfaces (R14, partly).** City textures whose first stored texel is non-zero (the opaque wall drawer) now show the backdrop line colour for index 0. Transparent textures still discard per pixel. Cars discard index 0, because the game's rule for models is not known.

**Chase camera.** Level, with yaw = the player's heading, and the car at (0, 134, 343) in camera space (the reference race's vehicle matrix). Projection: vertical FOV `2·atan(80/150)` = 56.14°. This touches R11; see the integration notes.

### Verification

The scripts are in the session scratchpad `vsp/`: `verify_sky.py`, `verify_s15.py`, `strip.py`, `car.py`. The viewer renders at 960×640 and the centre pixel of each 4×4 block is compared.

- **Sky layer against the game's skyline captures** (`data/work/e5298b24/sky/`: the 7 chase-view breakpoint captures h0–h5 and w1).
  - **Yaws:** 0x0FFD, 0x1018, 0x0DA9, 0x0E21, 0x23E6, 0x39E7, 0x37F6.
  - **Setup:** `NFSGBA_ROUTE=23` (the race palette), with the camera 400 m above the city looking level along the capture's yaw.
  - **Expected colour:** the capture's index through race palette RAM; index 0 through the game's gradient buffer (`c0.gradbuf.bin`) at entry `21 + y/2`.
  - **Result:** 107,520 / 107,520 GBA pixels (rows 0–63) equal.
- **Sky against s15,** from the game's camera (eye raw (118039, 30, −64321), yaw 0xFFD, level).
  - **Pixels checked:** 4,925 s15 pixels that the game left as sky (VRAM index 0, or the h0 skyline index) and that no sprite covers.
  - **Equal:** 4,555.
  - **The other 370:** all are pixels where the viewer draws world geometry instead (224 on row 159, see below; 49 at the far building; 97 at the left roof edge). None shows a wrong sky colour.
- **Cars** (chase shot, route 23):
  - Every pixel of the player car's box is an exact colour of the race dump's palette RAM (0 exceptions).
  - 25 of the 27 colours that s15 draws from slots 192–223 appear.
  - The two missing ones are near-whites, (247, 247, 231) and (231, 231, 214), and were not traced. Candidates are model 12 (the player's second piece) or the decals (R12/R13).
- **Unit test `camera_angles`:** yaw round trip, horizon sign and clamp.

### Not 1:1 (remaining)

- **R6 leftover bytes.** The game's fill drops `bytes % 32` above the skyline and never clears below it, so those bytes keep the page's previous frame. The viewer starts each sky layer from a cleared screen. The chase view has none: 240·20 is a multiple of 32.
- **Free-camera horizon.** It comes from the pitch; the game never pitches. At the ±32 clamp, the backdrop reaches gradient entries 64–76 (ROM bytes after the gradient), as in the game's bumper view.
- **Racer state.** The racers are the reference race's constants. The game picks them with `pick_opponent_cars` (RNG) and the save. The rule for opponent materials is open (R13); 131 + car id fits all three reference opponents (hypothesis).
- **Chase-camera dynamics.** The game's camera trails the car: yaw 0xFFD against heading 0x1000 at the start. That moves the skyline source column from 0 to 238, i.e. 2 pixels plus the next texture row, through the row-copy quirk.
- **Unchanged:** R14 (pair rule), R17 (glass scanline window), R18 (hardware line timing), and the model LOD, model 12, decals and overlays (R10/R12/R13).

**Finding: row 159 is never drawn by the world.** The world clip bounds `world +0xE2…+0xE8` are (x0 0, x1 240, y0 0, y1 159) in the race dump.
- In the chase view, row 159 is index 0 across the width in both pages of the race dump and of snaps c0–c2, so the bottom line shows the backdrop. The viewer draws the world there.
- In the bumper view (c3–c5), 148–162 of the 240 pixels of row 159 are index 0, so something else draws part of that row.
- This is viewer geometry (R11), outside this work.

## Integration notes (race palette and sky layer)

**FIDELITY.md:**
- **R3 → Closed:** "Car paint: exact in formats and viewer. One shared palette from `paint::race_palette` (`load_car_palettes` with the race's racers, glass shades from the player's heading); player atlas through `remap_atlas`; opponents' raw materials. Checked on the route-23 chase shot: every player-car pixel is a race palette RAM colour; 25/27 of s15's car-slot colours are present (`docs/engine/viewer-rendering.md`)." The racer choice itself goes to a new D-entry (below).
- **R5 → Closed:** "Backdrop per screen line in the viewer (`sky::backdrop_entry`/`gradient_start` on a 160-line texture; the window maps onto 240×160). Checked: 107,520/107,520 pixels against 7 skyline captures, and 4,555/4,555 s15 sky pixels where the viewer shows sky."
- **R6 → Closed,** except the leftover bytes: "Skyline: `sky::draw_skyline` into a 240×160 index screen behind the world, yaw and horizon from the camera. Checked as R5." Keep R6 open only for "fill leftovers (`bytes % 32` above, nothing below) keep the page's previous frame; the viewer clears. None in the chase view."
- **R14:** partly done. Opaque city textures (first stored texel non-zero) show the backdrop for index 0; the pair rule for transparent walls and the rule for models remain.
- **R11:** note what this work touched (the owner may replace it): FOV `2·atan(80/150)`, a 960×640 window, and a level chase camera with the car at (0, 134, 343) in camera space. The principal point (120, 79) and the integer projection are not done.
- **New R entry (row 159):** "The world's clip rect is rows 0..158 (`world +0xE2…+0xE8` = 0, 240, 0, 159); in the chase view the bottom line shows the backdrop. The viewer draws the world on all rows. In the bumper view part of row 159 is drawn by something else."
- **New D entry (racers):** "The viewer uses the reference race's racers as constants (cars [2, 9, 10, 11], paints [11, 11, 11, 5], record paint 11, opponent materials 140–142). Exact source: `pick_opponent_cars` (RNG `0x7C03F0`, index `0x030064C8`), the save's car records, and `setup_race_cars`."

**address-map.md:**
- World `+0xE2…+0xE8`: clip x0, x1, y0, y1 = 0, 240, 0, 159 in the race.
- Route template entity `+0x2C`: heading, 8.8 (`0x100000` = 0x1000 for route 23). Direction raw `(sin, cos)` in (x, z); checked against all 44 racing lines.
- Entity `+0x36`: close model + 1 (car table `+0x14` + 1). `draw_sector_entities` draws `+0x36 − 1`, and the next model beyond depth 0x1FF.
- Entity `+0x64`: second model piece, drawn with matrix slot `+0x88 + 1`; negative swaps the order. It is 12 for the player.
- Entity `+0x0A` bit 1: LOD depth forced to 0 (the opponents' 0x22).
- Entity `+0x88` = 0xFF: no matrix slot, not drawn by `draw_sector_entities` (all three opponents in the reference race, which were about 20,000 units ahead).
- Car table: `+0x14` = close model (`models[1]`), `+0x10` = far model (`models[2]`). `+0x12` equals `+0x10` and `+0x16` equals `+0x14` for all 15 cars.
- Vehicle materials 131–145: 15 raw 128×100 opponent atlases. The reference opponents (cars 9, 10, 11) use 131 + car (hypothesis).

**symbols.csv:** no new functions.

**Viewer-local ROM reader:** `grid_headings` (route table `0x7F2798` → template entities `+0x2C`) lives in the viewer. It could move to `nfsgba_formats::Route` as `headings`.

## Game camera, portal pass, walls and racers (viewer-geometry)

This part makes the race view the game's view: the camera, the projection, what is drawn and the racers. It covers FIDELITY R8, R10, R11, R14, R15, R19, R22 and R23 in the viewer, and adds the original-resolution frame.

### Modes and inputs

- **Race setups** (`src/game.rs`, `RaceSetup`):
  - `NFSGBA_DUMP=<dir/name>` loads an mGBA race dump from `$NFSGBA_DATA/work/e5298b24/`. It provides the racers' entities (`+0x0C`…`+0x88`), the cars, paints and player record, and the camera state; the reference race is `mgba/race`.
  - `NFSGBA_ROUTE=<n>` (or R) builds a Quick Play race on a route's grid with a new profile. The player drives car 2 with its default record. The opponents are dealt by `atlas::pick_opponent_cars` from rand index `0x11` (the only index of the 256 that deals the reference race's racers; derived, not traced) and dressed by `atlas::look`.
  - `nfsgba_formats::Route` now carries the template entities' 8.8 `positions`, `headings` (`+0x2C >> 8`) and start `sectors` (`+0x78`). `Wall::piece` exposes `+0x2A`.
- **Game camera and free camera:** the game camera is the default in a race. G switches to the free camera (Bevy `FreeCamera`), which is a non-game mode; without a race the viewer starts in it.
- **Original-resolution frame:** O shows the game's own frame, drawn on the CPU with `render::draw_world` into the sky layer's 240×160 index screen, over the skyline. With a dump the frame includes the cars, from the dump's `render::Scene` (entities, sector heads, matrix slots, EWRAM atlases). On a grid it shows the world alone.

### Game camera (R11)

`game::Chase::step` is one frame of `camera_update` (`0x08137cb0`) in the chase view (view 2). Its literals are resolved from the ROM:

1. **Focal:** it eases back up to 150 by 4 per frame. The speed effect is not modelled (driver `+0x4D1`; see NOT 1:1).
2. **Teleport:** a camera more than 1024 units from the player jumps onto it.
3. **Look yaw** `0x03000214 = atan(player − camera)`, from the previous frame's camera position, using the IWRAM atan `0x03004470`. This is the "lag" of the reference race: the camera stands exactly 344 units behind the player (dz = 0), and the game's polynomial atan gives `atan(344, 0) = 0xFFD`, not 0x1000.
4. **Orbit yaw** `0x03005F94` eases towards the driver's heading (`*(entity + 0x8C)`):
   - the step is `yaw −= clamp(angle_diff(heading, yaw), ±0x600) >> 3`;
   - it is skipped while `0x03006148` is set;
   - the result is wrapped to ±0x4000.
5. **Position:** `player + (sin, cos)(orbit yaw) · d` in 8.8.
   - `d = table[view]·256 + (0x80 − focal)·0x200`. Chase: −300·256 − 22·512 = −88064, i.e. 344 units behind.
   - The view tables are `0x7F39BC` (x, all 0), `0x7F39D4` (height, 8.8: −100, −140, −150, −115, −80, −105) and `0x7F39EC` (distance: 0, −290, −300, −150, 200, −120).
6. **Wall push** (`FUN_08137744`): the camera is pushed out of the blocking walls of its sector to 80 units.
7. **Camera sector** `0x03005614`:
   - the camera position must be reachable from the previous sector (else the player's sector is taken);
   - then the sector 72 units ahead along the look yaw (`vᵀR(look)`, `FUN_081608fc`) is taken, when found.
8. **Matrix:** `rotation(−look)` (`FUN_08160624`: `[cos, 0, −sin, 0, 0x4000, 0, sin, 0, cos]`).
   - Translation: `(−x >> 8, −(h >> 8) − (y_player >> 8) − 16, −z >> 8)`.
   - Screen centre (120, 79), rectangle (0, 240, 0, 159), list entry 0 = the camera sector.

**Checked:**
- From the reference race's own state, one step leaves the camera state unchanged (`0x03005F94`, `0x03000214`, `0x030056A0`, `0x030000A4`, `0x03005614`). It gives the dump's matrix `0x030057A0` exactly (`[18, 0, 16383, 0, 16384, 0, −16383, 0, 18, −118039, −30, 64321]`) and camera sector 760.
- A camera settled from scratch behind the dump's player (`Chase::behind`) is the same state.
- Test: `game::tests::chase_camera_reproduces_the_race`.

**Projection** (`game::GbaProjection`, a Bevy custom projection, in free mode too):
- `sx = 120 + focal·x/(d + 1)` and `sy = 79 + focal·y/(d + 1)` on the 240×160 screen, which the window shows whole (960×640 = 4×).
- The optical axis falls on the top-left corner of pixel (120, 79).
- Points nearer than 64 units are clipped; depth is reversed and infinite.
- The viewer camera is placed at the frame's eye (`game::frame_transform`).
- Tests: `projection_matches_the_game`, `frame_transform_matches_the_render_camera`.

### Portal pass and walls (R10, R8, R14, R19, R22)

**Decision: the GPU draws the game's per-frame visibility, not an approximation of it.** Every frame in the game camera:

1. `render::visible_sectors` runs from the camera sector. Entries whose sector `render::transform_walls` rejects (a corner deeper than 0x5FFF) are dropped, as `draw_sector` drops them.
2. For each remaining entry, `render::setup_wall_spans` runs on that entry, and `walls_drawn` (`src/main.rs`) decides which walls of the sector the game draws through it. These are the checks of `draw_sector_walls`:
   - material 0;
   - open portal (flag bit 0, or the moving piece's flags);
   - span flag 4;
   - deferred walls beyond the 8-bit defer mask;
   - rows outside the entry;

   plus the early returns of `raster_wall_columns` (fewer than 2 column pairs, before or after clipping to the entry's pairs).
3. The list and a 128-bit wall mask per entry go to a 32×2 `Rgba32Sint` texture that every city material reads. City vertices carry their sector and wall index (`UV_1`). A fragment is drawn only inside the span of an entry of its sector, and a wall only when that entry's mask has it.
4. Hidden surfaces come from the depth buffer rather than the game's painter's order.

**Checked:**
- The reference frame's list is the dump's 9 entries.
- `walls_drawn_matches_the_rasteriser`: over the chase frames of all 43 routes, 279 walls kept and 56 dropped. Each agrees with `render::raster_wall_columns` drawing that wall alone on a blank screen (opaque textures; the others are checked for "never passed to the rasteriser").

**Moving pieces** (world `+0x18`, 122 walls name one; R21): all 20 captured race dumps (routes 18 and 23) have every piece at zero offsets with flags 1, so these walls are open (not drawn, not blocking).
- `game::race_runtime` builds that state.
- It feeds every `render` call (without it `Runtime::default()` panics on any wall that names a piece), the mesh and the camera's wall push.

**Walls and flats (R8, R19):**
- Every wall with material ≠ 0 and effective flag bit 0 clear is drawn, solid or portal; a portal wall is a step, kerb or fence over its own top..bottom.
- Counts: 1,644 solid, 661 portal by the ROM flags, 539 in races.
- Facing follows `setup_wall_spans`:
  - the front (start left of end on screen) is drawn unless flag 2;
  - the back only with flag 2 or 0x2000, as a deferred wall that the 8-bit mask drops at countdown index 8 and up;
  - the city materials cull back faces in the shader.
  - 12 walls have a back side and none is lost to the mask.
- Flats use `Wall::floor_y` / `ceiling_y` (`+0x38`/`+0x3A`), both faces.

**Transparent walls (R14):** textures whose first texel is 0 draw in the pairs of the 240-column grid. The shader steps u along the screen (`dpdx`) to the left edges of the fragment's pair and drops the fragment unless both texels are non-zero.
- On route 22's start frame the rule changes 215 GBA pixels, all on the railing and the left roadside barrier.
- In the railing region, agreement with the original frame goes from 23.00% to 23.43% exact and from 59.86% to 60.52% within one pixel. The rest of that region is the embankment behind, sampled at 4×.

**Row 159 (R22):** the entries' rows are `top .. bottom` = 0..159, so the world never draws row 159.
- The GPU shot's row 159 equals the original frame on 240/240 pixels.
- It equals s15 on 106/106 pixels outside the HUD.

**Screen spans at high resolution:** the game rounds its projected portal ends down and draws walls in 2-pixel pairs, so its surfaces meet on whole pixels. The viewer's geometry is continuous, so the span test keeps `left ..= right`.
- Clipping walls to pairs opened a one-GBA-pixel seam at an entry edge on route 22.
- The exclusive right edge still left a one-window-pixel hairline.
- Both showed the backdrop.

### Racers (R23)

- **Dealt as the game deals them:** `RaceSetup::grid` deals cars, paints and the new-profile record (`0x7EEA33`), the far model (`+0x36`: car table `+0x14` + 1 for the player, `0x7EEA44` for opponents) and the spoiler (`+0x64`: `i16 0x7F0636[car·0x10 + record[0]]`). For route 23 all of these equal the dump's (test).
- **Player atlas:** `atlas::player_atlas` plus the rim at angle 0 (race start). With a dump, the atlas is taken from EWRAM (entity `+0x84`).
- **Opponents:** their raw `look` material.
- **Models:** each racer has meshes for its near body (`+0x36 − 1`), far body (`+0x36`) and spoiler.
  - `Racer::models_at(depth)` picks per frame as `draw_sector_entities` does:
    - none at depth ≥ 0x2000, without a matrix slot, or beyond 0x1000 without flag bit 6;
    - flag bit 1 forces the near body;
    - the far body from 0x200.
  - A racer shows only when its sector is in the drawn list.
  - The high model is never used in a race.
- **Poses:** with a dump, cars stand exactly as their vehicle matrix slot (world `+0xFC`) puts them relative to the game camera (pitch and roll included). On a grid they stand level on the floor fan (D2).
- **Light tint (R15):** `apply_sector_light_to_palette` reads the player's position with the **camera** sector `0x03005614`, not a player sector. The viewer now takes that sector from the chase camera, so the tint's sector is exact in the game camera.

### Pixel agreement at the reference camera

Setup: `NFSGBA_DUMP=mgba/race`, screenshots at 960×640, reduced to the 240×160 grid by each GBA pixel's centre (`crates/nfsgba-viewer/diff_shots.py`).
- **HUD boxes** (excluded where noted): (0, 0)–(96, 40), (0, 94)–(64, 160), (170, 94)–(240, 160).
- **Original frame vs s15:** 25,716 of 25,716 non-HUD pixels equal (100.00%), the car included. Of the whole frame, 28,745 of 38,400 are equal; the rest is HUD.
- **GPU vs original frame:** 18,745 of 38,400 equal (48.82%); 31,764 (82.72%) equal within one pixel.
- **GPU vs s15 outside the HUD:** 12,779 of 25,716 (49.69%); 21,089 (82.01%) within one pixel.
- **Route 22 grid start, GPU vs original frame** (cars excluded): 49.31% exact, 77.20% within one pixel.
- **What the differences are:**
  - Sampling the GPU shot at other points of each GBA pixel moves the exact agreement only between 47.2% and 49.8%, so there is no systematic offset.
  - The differences are texture sampling: 4× sampling against the game's per-column and per-pair integer sampling, and 120-wide floors.
  - Edges round 1 pixel differently.
  - Sub-pixel surfaces appear at 4× that the 240×160 rasteriser drops, for example route 22's road-edge strip.
  - The sky and backdrop are equal.

### Not 1:1

- **R10:** the depth buffer, not the painter's order, hides surfaces.
- **High resolution (R16):**
  - continuous edges, with spans kept at `left ..= right`;
  - surfaces thinner than a GBA pixel can appear;
  - textures are sampled per window pixel;
  - floors are not 120 wide.

  The original-resolution frame is the exact image.
- **R11:**
  - the speed effect is not modelled: with driver `+0x4D1` set, focal eases by 4 towards `150 − max(0, (0x800 − g) >> 5)`, where `g = |angle_diff(heading, atan(driver +0x11C >> 8, +0x124 >> 8))|`, the angle between heading and travel;
  - the camera wall push takes the height limit `*0x03005778` (a smoothed `floor_height` `0x0814ca84` at the camera) as passed;
  - the sector search's second fallback `find_sector_far` is not applied;
  - the free camera pitches (a non-game mode).
- **R12 in the viewer:**
  - cars are not clipped to their portal span, and the screen-row cull is not applied;
  - on a grid the original frame has no cars (the vehicle matrices come from `draw_vehicle` / `FUN_0814eba0`).
- **R13 in race:** the rim is at angle 0 on a grid (the game redraws it by wheel angle); a dump's atlas is as dumped. R24 is unchanged.
- **R14:** at high resolution a fragment keeps its own texel, and the pair test uses its row. Index 0 on car models is dropped per pixel.
- **R21:** the runtime tables are as every captured race has them; their writers are not decoded.
- **D2:** grid racers stand on the floor fan; the camera then sits a few units off.
- **Grid assumptions:**
  - the rand index `0x11` is derived, not traced (the seed `*0x03000044 & 0xFF` = 3 is 14 draws earlier; open);
  - the draw flags are the reference race's (player 0x0D, opponents 0x22).

## Integration notes (viewer-geometry)

**FIDELITY.md:**
- **R8 → Closed (viewer):** "Portal walls with flag bit 0 clear and material ≠ 0 are drawn as solid walls, and a moving piece's flags replace the wall's. 1,644 solid and 661 portal walls by the ROM flags, 539 in races. Per-entry wall visibility is exact against `render::raster_wall_columns` (279 kept and 56 dropped over all 43 routes' chase frames)."
- **R10 → narrowed:** "The viewer draws the game's per-frame visible list: entries past `transform_walls`, clipped to their spans; per entry, the walls `draw_sector_walls`/`raster_wall_columns` draw. Open: hidden surfaces come from the depth buffer, not the painter's order."
- **R11 → narrowed:** "Chase camera exact (`game::Chase::step` reproduces the dump's camera state and matrix `0x030057A0`); projection focal 150, centre (120, 79), `d + 1`, near 64. Open:
  - the speed effect (driver `+0x4D1`, rule known);
  - the wall-push height limit (`floor_height` smoothing);
  - `find_sector_far`;
  - integer rounding at high resolution."
- **R14 → Closed for walls:** "Transparent walls draw in pairs of the 240-column grid (both texels non-zero). Open: car models' index 0."
- **R15 → Closed:** "The tint's sector is the camera sector `0x03005614` (`apply_sector_light_to_palette` reads it with the player's position), taken from the chase camera. The free camera's point-in-polygon search is a non-game mode."
- **R19 → Closed.**
- **R22 → Closed:** "Row 159 shows the backdrop: 240/240 against the original frame, 106/106 against s15 outside the HUD."
- **R23 → Closed:** "Racers dealt by `pick_opponent_cars`/`look` from rand index `0x11` (derived: the only index giving the reference racers). Player atlas from `atlas`, LOD models and spoiler as `draw_sector_entities`. With a dump, poses come from the vehicle matrices."
- **New:** "High resolution: continuous edges (spans kept `left ..= right`), sub-GBA-pixel surfaces visible, textures sampled per window pixel. The original-resolution frame (`render::draw_world`) is exact: 100% of s15 outside the HUD."
- **R21:** add "In every captured race (routes 18 and 23, 20 dumps) all 122 moving pieces are at zero offsets with flags 1 (open); the viewer uses that state (`game::race_runtime`)."
- **New D (grid deal):** "The rand index at `pick_opponent_cars` (0x11 for the reference race) is derived, not traced; the seed `*0x03000044 & 0xFF` = 3 comes 14 draws earlier."

**address-map.md:**
- `0x03005614`: correct to "camera sector (written by `camera_update`, searched 72 units ahead of the camera; `apply_sector_light_to_palette` reads it with the player's position; 760 in the reference race)".
- `0x03000214`: add "look yaw = `atan(player − camera)` from the previous frame's camera position (`0x03004470`); 0xFFD in the reference race, the atan's value at (344, 0)".
- `0x03005F94`: chase orbit yaw; eases towards the driver's heading by `clamp(diff, ±0x600) >> 3` unless `0x03006148`.
- `0x030056A0` / `0x030000A4`: camera x / z, 8.8.
- `0x03005778`: smoothed floor height at the camera (`floor_height`), the height limit for flag-0x4000 walls in the camera wall push.
- `0x03005FA4`: add "= view height table entry (chase −150·256)".
- `0x7F39BC` / `0x7F39D4` / `0x7F39EC`: per-view camera x offset (all 0), height 8.8 (−100, −140, −150, −115, −80, −105) and distance (0, −290, −300, −150, 200, −120). The orbit distance is `distance·256 + (0x80 − focal)·0x200`.
- Driver struct `+0x00`: the heading the chase camera follows (0x1000 in the reference race); `+0x11C`/`+0x124`: travel vector (speed effect; hypothesis).
- Route template entity `+0x78`: start sector.
- World `+0x18` in races: every piece zero, flags 1.

**symbols.csv** (new rows; `atan2_fast`, `angle_diff`, `floor_height`, `find_sector_far` and `cos_q14` exist already):

```
0x08160624,rotation_y,function,3x4 matrix: rotation about y by an angle (cos 0 -sin / 0 0x4000 0 / sin 0 cos), zero translation
0x08160668,rotation_z,function,3x4 matrix: rotation about z by an angle, zero translation
0x081607e0,transform_point,function,out = (v · M >> 14) + translation (row vector times the 3x3, then m9..m11)
0x081608fc,rotate_vector,function,out = v · M >> 14 (row vector times the 3x3, no translation)
0x08137744,camera_push_out_of_walls,function,pushes the camera out to 80 units from flag-0x1000 (or low 0x4000) walls of the camera sector
```
