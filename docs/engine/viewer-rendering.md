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
