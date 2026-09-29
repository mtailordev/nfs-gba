# Fidelity ledger

The goal is an **absolute 1:1 rewrite**: same data, same maths, same behaviour. This file lists every place where our code is *not yet* exact. Each entry names what we do now, what the game does, and where the exact behaviour lives. Code that deviates carries a `NOT 1:1` comment pointing here.

An entry is closed only when the exact behaviour is implemented **and** checked against the reference build (mGBA RAM dumps, screenshots, or traces).

## Rendering

| # | Now | Game | Exact source |
|---|---|---|---|
| R1 | Textures are expanded to RGBA and filtered by the GPU as true colour | 8bpp indexed texels through a 256-entry BGR555 palette with integer palette maths | Render with index textures plus a palette texture (custom material), doing the palette maths in integers as the game does |
| R2 | The algorithm is **exact and verified**: `nfsgba_formats::sector_light` + `tint_palette` reproduce the reference race's palette RAM entry for entry (test `light_tint_reproduces_the_race_palette`). **The viewer still shows per-vertex wall light** | The whole palette is tinted by the light at the player's position | Viewer integration needs R1 (a palette texture) |
| R3 | The car body ramp is a reconstruction fitted to one red ramp; opponents use presets | Paint colour from a table (`DAT_08138860 + 0x218 + paint·0x20`), shaded by `sin(heading)` clamped to 16..28/32, into palette slot 192+ | `shade_car_paint` (`FUN_081387e4`); the paint table; how slots 160–223 are filled for opponents |
| R4 | Car trim colours (glass, lights) come from paint preset 0 | Unknown | Palette slots 192–207 at runtime; find the writer |
| R5 | Sky gradient mapped on a cylinder around the camera | 64 BGR555 colours, probably one per scanline (HBlank), placed against the horizon | The sky renderer (not yet found); DMA/HBlank setup (`FUN_08151114`, `FUN_08151554` use DMA registers) |
| R6 | Skyline repeats 4× around the horizon over about 10° | Unknown scroll factor and vertical placement | Sky renderer |
| R7 | Walls with material 0 are skipped (assumed invisible) | Not confirmed from code | Wall draw path: `setup_wall_spans` / `raster_wall_columns` |
| R8 | Portal walls between sectors of different heights are not drawn | Probably upper and lower wall parts | `setup_wall_spans`, `draw_sector` |
| R9 | The floor UV scale (16,384 = one texture) was derived from data | Needs confirming in `draw_flat_textured` | `FUN_03002da0` |
| R10 | Everything is drawn: no portal traversal, no depth limit | Portal recursion from the camera sector; walls cut at depth `0x5FFF`, models at `0x6000`; LOD by distance | `draw_sector`, `transform_walls`, `transform_model_vertices`, `draw_sector_entities` |
| R11 | Free perspective camera (Bevy default FOV) | Projection through a reciprocal table, focal length in the view struct (`+0x1C`), 240×160 screen | View struct `world +0x50`; reciprocal tables |
| R12 | Cars show no wheels or shadows | Unknown (separate models 89–101? sprites?) | Entity draw passes |
| R13 | The 36 128×100 vehicle materials are read raw | Format unknown | Users of vehicle materials 61–66 and 116+ |

## Data and gameplay

| # | Now | Game | Exact source |
|---|---|---|---|
| D1 | The racing line stops at the first waypoint that breaks the pattern | The real length or terminator is unknown | Code reading world `+0x44` |
| D2 | Grid cars are placed on the sector's mean floor height | The game's ground height and suspension | Entity physics |
| D3 | Route names and modes are not mapped | Events select route, mode and environment | Event tables (text keys `TEXT_TRACK*`, `TEXT_ROUTE*`) |
| D4 | No gameplay yet (handling, AI, cops) | — | Roadmap step 4, traced against the reference build |

## Closed

- **Environment palette and sky selection:** exact (`race_load_palettes`: palette `+0x00 + (+0x5A)·2`, sky from `+0x5E`/`+0x60`). Checked: the race's base palette equals city palette 3 in every non-runtime slot.
- **Wall textures and wall UVs:** exact (column maps, `u >> 7`, v 16,384 = one texture), from `raster_wall_columns` and `setup_wall_spans`.
- **World scale and axes:** exact (one unit for cars and city; the chase-camera view matches the game's screenshot).
- **Vehicle UVs:** exact (1.15 fixed point, overlaid on the atlas).
