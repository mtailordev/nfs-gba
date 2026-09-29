# Fidelity ledger

The goal is an **absolute 1:1 rewrite**: same data, same maths, same behaviour. This file lists every place where our code is *not yet* exact. Each entry names what we do now, what the game does, and where the exact behaviour lives. Code that deviates carries a `NOT 1:1` comment pointing here.

An entry is closed only when the exact behaviour is implemented **and** checked against the reference build (mGBA RAM dumps, screenshots, or traces).

## Rendering

| # | Now | Game | Exact source |
|---|---|---|---|
| R3 | **Exact and verified in `nfsgba_formats::paint`** (`load_car_palettes`, `glass_shades`, `race_palette`, `remap_atlas`: slots 160–255 match the race's base buffer and palette RAM). **The viewer still gives each car its own reconstructed palette** through the hook `CarPalette::slots` (raw BGR555 for slots 192..=223) | One palette: player ramp at 208, opponents at 160/176 (their 128×100 materials are already final slot numbers), glass shades at 192/208 | Viewer: feed `paint::race_palette` into the one palette; draw opponents with their raw materials |
| R5 | **Exact and verified in `nfsgba_formats::sky`** (`gradient_buffer`, `gradient_start`, `backdrop_entry`). **The viewer still maps the gradient on a cylinder** | Palette entry 0 is the backdrop; a VCount IRQ (`0x030001C0`) writes the next gradient colour every 2 lines on lines 0–78; the start entry comes from the horizon (`vblank_irq`) | Viewer: draw the backdrop per screen line, `buffer[backdrop_entry(gradient_start, y)]`, y scaled from 160 lines |
| R6 | **Exact and verified in `nfsgba_formats::sky`** (`skyline_column`, `skyline_top`, `draw_skyline`, including its row-copy quirks). **The viewer still draws the skyline on the cylinder** | The panorama is copied into the back buffer before the world, scrolled by yaw only (repeats 4× per turn) | Viewer: composite `draw_skyline` into a 240×64 index layer under the world |
| R8 | **Exact rule found** (`render.rs`); **the viewer still skips every portal wall** | Portal walls with flag bit 0 clear and material ≠ 0 are drawn exactly like solid walls, over their own top..bottom; there are no separate upper/lower parts (661 walls) | Viewer: draw them; `docs/engine/renderer.md` |
| R10 | **Exact and verified in `nfsgba_formats::render`** (`visible_sectors` gives the reference frame's 9-entry list, `draw_world` reproduces all 33,350 world pixels in VRAM except the 536 of the car being drawn). **The viewer still draws the whole city** | At most about 25 list entries, 5 portals deep; sectors with a corner deeper than `0x5FFF` dropped; wall columns stop at `1/z > 0x5FFF`; list flag `0x80` draws a sector's cars with it | Viewer: portal traversal and limits from `render` |
| R11 | **Exact parameters found; the viewer still uses the Bevy default FOV** | Focal 150 → 77.32° × 56.14°, principal point (120, 79), near 64, no pitch or roll, projection through `recip` (`0x7C45F0`). A speed effect can lower the focal length; its input is not decoded | Viewer camera; `camera_update` for the speed effect |
| R12 | Cars show no wheels or shadows; the entity draw is not reimplemented | Entity drawing (`draw_sector_entities`, `raster_polygon`) and LOD rules are decoded in renderer.md but unchecked; entity field meanings are hypotheses; cars beyond depth `0x1000` are not drawn (hypothesis) | `render.rs` next step; entity draw passes |
| R13 | The 36 128×100 vehicle materials are raw 8bpp in final palette slots (opponent atlases; answered). **Open:** decals and overlays drawn onto the player's atlas are decoded but not implemented; how opponents' materials are picked is unknown | `blit_material_keyed`, `draw_decal_on_atlas` (744 pixels in the reference race); `setup_race_cars` picks opponent materials (140–142 in the race) | `FUN_08163870`, `FUN_0813bd90`, `FUN_08163bfc`, `FUN_0813b9b8` |
| R14 | The viewer discards index 0 per pixel | Transparent wall textures (first texel 0, `0x03004d48`) skip a pixel **pair** unless both texels are non-zero; opaque walls (`0x03004db0`) write index 0, which shows the backdrop (only texture 92 has any); floors never hold index 0 | The wall column drawers |
| R15 | The observer's sector is found by point-in-polygon | The game tracks the player's sector through the portals it crosses (`0x03005614`) | The sector update in the player/camera code |
| R16 | Floors and ceilings are drawn at full window resolution | 120×160 (`flat_span_affine` `0x03004fa8` writes one byte per two pixels; confirmed on s15); wall edges at 120 columns with 240-column texturing. Exact in `render::draw_world` | Only relevant to a 240×160 original-resolution mode |
| R17 | `paint::race_palette` gives the raw glass shade in slots 192/208 | For 1–7 scanlines per game frame, 192/208 show the tinted shade (the tint runs at the end of a frame; the next frame's `shade_car_paint` restores the raw value) | Needs scanline timing of the whole frame |
| R18 | The palette-0 write is applied to the whole scanline (as the reference emulator does) | On hardware the left edge of each even line may still show the previous colour | Unmeasured; needs hardware timing |
| R19 | The viewer puts floors and ceilings at the wall's `bottom`/`top` | Flats use wall `+0x38` (floor) and `+0x3A` (ceiling) per corner (`Wall::floor_y`/`ceiling_y`); they differ at 384 floor corners | Viewer |
| R20 | Wall v fixed in `Wall::uv` from the code: 1/128 texel rows, top `+0x10 + +0x42·128`, span `+0x18`, same at both ends (2,119 of 2,305 drawn walls span exactly one texture height). **No pixel check yet** (the one 64-row wall in the reference frame is hidden); the material's runtime v scroll (world `+0x48`) is not applied | `setup_wall_spans`, `raster_wall_columns` | A frame showing a 64-row wall; the writers of the runtime tables |
| R21 | Moving wall pieces, sector offsets, animated and scrolled materials (world `+0x18`/`+0x1C`/`+0x48`) are read as static | Runtime tables written by door/animation code, not decoded | Their writers |

## Data and gameplay

| # | Now | Game | Exact source |
|---|---|---|---|
| D2 | Grid cars are placed on the sector's mean floor height | The game's ground height and suspension | Entity physics |
| D3 | Career events, race setup (route number → environment and route via `0x7F2588`), unlocks and the save are exact and checked against RAM (`nfsgba_formats::career`). **Open:** where career races set the opponent count, and how the event's AI skill value is used | — | `career_event_to_globals`, the AI code |
| D4 | No gameplay yet (handling, AI, cops) | — | Roadmap step 4, traced against the reference build |
| D5 | Transcribed from code, not yet trace-checked: career payout and style-rating reward, race progress, lap crossing and elimination, hunter life per frame and hits, the unlock rebuild with events completed | — | `career_race_payout`, `style_rating`, `race_progress`, `lap_crossing`, `hunter_life_tick`, `hunter_hit`, `rebuild_unlocks` |
| D6 | Not located: the code that arms a lap (driver `+0x4D8` bit 1), and what happens when hunter life reaches zero | — | Race rule code (`docs/formats/career.md`) |
| D7 | No save encoder (the decoder is exact) | `save_encode` (`FUN_081492c0`) | `docs/formats/career.md` |

## Audio

| # | Now | Game | Exact source |
|---|---|---|---|
| A1 | Implemented from the disassembly but never exercised by Carbon's data, so exact by reading only: effects 0–6, A, C, D, E6x, F; linear pitch mode; rate change; the first-voice carry; the mixer's zero-address checks; volumes ≥ 0xFF | — | `docs/formats/audio.md` |
| A2 | Jingle system (second module state at engine `+0x83C`) not rewritten | Nothing in Carbon calls it | `FUN_081517e8`, `FUN_081522b8`, `FUN_08152310`, `FUN_0815236c` |
| A3 | Mode-0 mixer and the flagged sound format not rewritten | Carbon uses mode 1 | `0x0815CF2C`, `FUN_08152ab8`, `FUN_08152b7c` |
| A4 | Test tone not rewritten | Engine `+0x14` is never set | `FUN_08151aa8` |
| A5 | Division by zero returns mGBA's HLE result | The BIOS would hang; cannot happen with Carbon's data | `bios_div` |
| A6 | Analogue output not modelled: two identical FIFOs, DAC, `SOUNDBIAS`; WAV files say 10512 Hz | Hardware runs at 10512.04 Hz | Hardware |

## Closed

- **R1, indexed colour:** exact. Index textures plus a 256-entry palette texture (`Indexed` material, `indexed.wgsl`), nearest texel with mask-equivalent wrap, BGR555 expansion `c << 3 | c >> 2`, no filtering, MSAA, tonemapping or dither. Checked: every city pixel of the route-23 chase shot is an exact palette colour; region colour sets match s15 (`docs/engine/viewer-rendering.md`).
- **R2, light tint:** exact in formats and viewer. `sector_light` + `tint_palette` every frame at the observer (the player's car in race mode); the palette is kept when there is no light. Checked: route 23 gives sector 760 (as `0x03005614`) and the tinted palette equals palette RAM 178/178 (test `light_tint_reproduces_the_race_palette`).
- **R4, car trim:** slots 193–207 are the city palette's own colours; nothing car-specific writes them. 192 and 208 are the glass shades (R3).
- **Environment palette and sky selection:** exact (`race_load_palettes`: palette `+0x00 + (+0x5A)·2`, sky from `+0x5E`/`+0x60`). Checked: the race's base palette equals city palette 13 (environment 11, the reference race; byte-identical to palette 3) in every non-runtime slot.
- **D1, racing line length:** exact. Section 0 of the route `+0x04` table is the lap (its last waypoint repeats the first); sections 1.. are branches, joined by waypoint links (`routes()`; route 23: 36 waypoints, 108,219 units, one branch from 19 to 27).
- **LS_Play music, sound effects and mixer:** exact (`nfsgba-audio`: 13,800 traced frames bit-exact in mix buffers and the whole engine work area, including 12,000 free-running frames driven only by the game's API calls).
- **Wall textures and wall u:** exact (column maps, `u >> 7`), from `raster_wall_columns` and `setup_wall_spans`. (Wall v was reopened as R20.)
- **R7, material 0:** never drawn (`draw_sector_walls` skips materials whose slot `+0x00` is 0; only material 0), and walls with flag bit 0 are open portals. Confirmed from code and in the reference frame.
- **R9, floor UVs:** 16,384 = one texture in u and v (`setup_wall_spans` and the `flat_span_affine` masks). Floor fill colour is sector `+0x0D`, ceiling `+0x0C`.
- **World renderer at 240×160:** `render::draw_world` plus the race palette (R2) is a pixel-exact software path of the game's world image; it can back an original-resolution mode or a reference view.
- **World scale and axes:** exact (one unit for cars and city; the chase-camera view matches the game's screenshot).
- **Vehicle UVs:** exact (1.15 fixed point, overlaid on the atlas).
