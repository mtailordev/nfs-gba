# Fidelity ledger

The goal is an **absolute 1:1 rewrite**: same data, same maths, same behaviour. This file lists every place where our code is *not yet* exact. Each entry names what we do now, what the game does, and where the exact behaviour lives. Code that deviates carries a `NOT 1:1` comment pointing here.

An entry is closed only when the exact behaviour is implemented **and** checked against the reference build (mGBA RAM dumps, screenshots, or traces).

## Rendering

| # | Now | Game | Exact source |
|---|---|---|---|
| R6 | Skyline exact in the viewer except the fill leftovers: the viewer clears them | `bytes % 32` above the skyline (and nothing below) keep the page's previous frame; none in the chase view | `draw_skyline` (`nfsgba_formats::sky`) |
| R8 | **Exact rule found** (`render.rs`); **the viewer still skips every portal wall** | Portal walls with flag bit 0 clear and material ≠ 0 are drawn exactly like solid walls, over their own top..bottom; there are no separate upper/lower parts (661 walls) | Viewer: draw them; `docs/engine/renderer.md` |
| R10 | **Exact and verified in `nfsgba_formats::render`** (`visible_sectors` gives the reference frame's 9-entry list, `draw_world` reproduces all 33,350 world pixels in VRAM except the 536 of the car being drawn). **The viewer still draws the whole city** | At most about 25 list entries, 5 portals deep; sectors with a corner deeper than `0x5FFF` dropped; wall columns stop at `1/z > 0x5FFF`; list flag `0x80` draws a sector's cars with it. Cars: dropped at depth ≥ 0x2000 or outside the rows `E6 − margin … E8 + margin`; beyond 0x1000 only with `+0x0A` bit 6 (far model) or bit 1 (depth taken as 0: opponents, markers); a model is dropped whole when a vertex is deeper than 0x6000 | Viewer: portal traversal and limits from `render` |
| R11 | **Partly done in the viewer:** vertical FOV `2·atan(80/150)`, a 960×640 window (4× the GBA screen), a level chase camera with the car at (0, 134, 343) in camera space. **Not done:** principal point (120, 79), the integer projection, the game camera's lag behind the car, the speed effect on the focal length; the free camera's horizon comes from its pitch, which the game never does | Focal 150 → 77.32° × 56.14°, principal point (120, 79), near 64, pure-yaw matrix, projection through `recip` (`0x7C45F0`) | `camera_update`, `render.rs` |
| R14 | Opaque city textures show the backdrop for index 0 (done). **Open:** transparent walls still discard index 0 per pixel, and models are not handled | Transparent wall textures (first texel 0, `wall_column_transparent`) skip a pixel **pair** unless both texels are non-zero; opaque walls write index 0 = backdrop (only texture 92 has any) | The wall column drawers; the model rasteriser |
| R15 | The observer's sector is found by point-in-polygon | The game tracks the player's sector through the portals it crosses (`0x03005614`) | The sector update in the player/camera code |
| R16 | Floors and ceilings are drawn at full window resolution | 120×160 (`flat_span_affine` `0x03004fa8` writes one byte per two pixels; confirmed on s15); wall edges at 120 columns with 240-column texturing. Exact in `render::draw_world` | Only relevant to a 240×160 original-resolution mode |
| R17 | `paint::race_palette` gives the raw glass shade in slots 192/208 | For 1–7 scanlines per game frame, 192/208 show the tinted shade (the tint runs at the end of a frame; the next frame's `shade_car_paint` restores the raw value) | Needs scanline timing of the whole frame |
| R18 | The palette-0 write is applied to the whole scanline (as the reference emulator does) | On hardware the left edge of each even line may still show the previous colour | Unmeasured; needs hardware timing |
| R19 | The viewer puts floors and ceilings at the wall's `bottom`/`top` | Flats use wall `+0x38` (floor) and `+0x3A` (ceiling) per corner (`Wall::floor_y`/`ceiling_y`); they differ at 384 floor corners | Viewer |
| R20 | Wall v fixed in `Wall::uv` from the code: 1/128 texel rows, top `+0x10 + +0x42·128`, span `+0x18`, same at both ends (2,119 of 2,305 drawn walls span exactly one texture height). **No pixel check yet** (the one 64-row wall in the reference frame is hidden); the material's runtime v scroll (world `+0x48`) is not applied | `setup_wall_spans`, `raster_wall_columns` | A frame showing a 64-row wall; the writers of the runtime tables |
| R21 | Moving wall pieces, sector offsets, animated and scrolled materials (world `+0x18`/`+0x1C`/`+0x48`) are read as static | Runtime tables written by door/animation code, not decoded | Their writers |
| R22 | The viewer draws the world on all 160 rows | The race clip rect is rows 0..158 (world `+0xE2…+0xE8` = 0, 240, 0, 159): in the chase view row 159 shows the backdrop; in the bumper view part of it is drawn by something else | `render.rs`; the bumper-view HUD |
| R23 | The viewer uses the reference race's racers as constants (cars [2, 9, 10, 11], paints [11, 11, 11, 5], opponent materials 140–142) and draws the player atlas without overlay, decal set and rims | Exact in `nfsgba_formats::atlas` (`pick_opponent_cars`, `look`, `player_atlas`, `draw_rim`) from the RNG state, the save's car records and the event | Viewer: use `atlas`; the RNG state at race start. Also the race LOD: draw model `+0x36 − 1` near (depth < 0x200 or `+0x0A` bit 1) and `+0x36` far, i.e. medium and low, never high; plus the `+0x64` spoiler |
| R25 | Matrix slots (world `+0xFC`) are inputs to `render`; the code that builds them is not reimplemented | `build_player_matrices` (`0x0814bc30`), `assign_entity_slot` → `build_entity_matrix`, `assign_billboard_slot`, with a one-frame delay before a car appears and a stale matrix beyond depth 0xDAC; the axes of the two rotations are not checked | `0x0814eba0`, `0x0814da68`, `0x0814ec0c` |
| R26 | Implemented from the code but in no capture: bit-4 sector entities, material steps `+0x44`/`+0x46`, negative `+0x64`; effect sprites (`opponent_effects`, `spawn_effect_sprite`) not implemented | — | New captures |
| R24 | `atlas::draw_rim` needs the RAM around the rim buffer (up to 63 bytes before, 62 after); without the game's heap layout the rewrite cannot supply it | The rotated blit reads the previous block's tail, the gap word, the decoder overrun and the next block's live car state (opponent 1's in the reference race); those pixels reach the texture | `heap_alloc` (`0x08160af8`, table `*0x030064CC`, arena `*0x030064D0`) and the allocation order at race start |

## Data and gameplay

| # | Now | Game | Exact source |
|---|---|---|---|
| D2 | Grid cars are placed on the sector's mean floor height | The game's ground height and suspension | Entity physics |
| D3 | Career events, race setup (route number → environment and route via `0x7F2588`), unlocks and the save are exact and checked against RAM (`nfsgba_formats::career`). **Open:** where career races set the opponent count, and how the event's AI skill value is used | — | `career_event_to_globals`, the AI code |
| D4 | **The player car's per-frame update is exact** (`nfsgba-sim`: car init, dynamics, ground contact, walls, integration, route tracking, the traffic spawn from the player's step, sound commands; 1,149 traced steps over 9 scenarios, per step on full RAM and as a free replay). **Not ported:** opponent AI (handler 0x29) and traffic AI (0x36); the replay takes their state from the reference RAM | — | The entity handlers (`0x7F38B8`) |
| D9 | Unported car paths stop with `Unported` instead of guessing: suspension step `FUN_0814de40` (race phases 1 and 4), tipped-over dynamics `FUN_081484f0`, stuck reset `FUN_0814efa8` | — | `docs/engine/physics.md` |
| D10 | Car-to-car collision response not ported (the proximity test is) | `car_car_response` `FUN_08144fa4`, `hunter_hit` | `docs/engine/physics.md` |
| D11 | Sector search two portals away and the push-back when the car leaves every sector not ported | `find_sector_far` `FUN_0814dbbc` | `docs/engine/physics.md` |
| D12 | Traffic: spawns at the route start (kinds 0 and 2) not ported | `traffic_spawn`, `traffic_countdown` | `docs/engine/physics.md` |
| D13 | Ported but not exercised by any trace: manual gearbox, active nitro, breakable walls, hunter wall hits, side route segments, the career and wingman branches of the car init; the wingman command is not ported | — | New trace scenarios |
| D5 | (`hunter_life_tick` and the lap/finish tail of `lap_crossing` are also unported in `nfsgba-sim`.) Transcribed from code, not yet trace-checked: career payout and style-rating reward, race progress, lap crossing and elimination, hunter life per frame and hits, the unlock rebuild with events completed | — | `career_race_payout`, `style_rating`, `race_progress`, `lap_crossing`, `hunter_life_tick`, `hunter_hit`, `rebuild_unlocks` |
| D6 | Not located: the code that arms a lap (driver `+0x4D8` bit 1), and what happens when hunter life reaches zero | — | Race rule code (`docs/formats/career.md`) |
| D7 | No save encoder (the decoder is exact) | `save_encode` (`FUN_081492c0`) | `docs/formats/career.md` |
| D8 | Rim redraw rule decoded, not traced angle by angle | Redrawn every game frame unless `|angle_diff(heading, 0x4000 − *0x03005F9C)|` is within ±0x400 of 0 or 0x2000 | `entity_update_rim`, `entity_update_rim_b`, `rim_side_visible`, `angle_diff`; the race entity update |

## UI (2D)

| # | Now | Game | Exact source |
|---|---|---|---|
| U5 | `hud::update` reads the race time `0x03005800` once per frame | The VBlank IRQ counts it and can land inside `hud_update`, so the timer, portrait and arrow of one frame may read different values (seen in the traces; the tests accept either) | The frame's CPU timing (like R17, R18) |
| U6 | A forced negative split makes `hud::update` index outside the material table (panic) | The game uploads garbage until it hangs; negative splits never happen (`route_gap` computes `T − x·T/y` only for `x < y`) | — (unreachable) |
| U3 | Menu primitives exact (`ui::unpack`, `blit`, `Font::draw`, `draw_wrapped`, button prompts); the screen logic and state machine are not ported | Page tables at `0x7E4A00–0x7E6EEC` only partly decoded (three tables) | `menu_screen_setup`, the page records, `menu_button_prompts` |
| U4 | Menu image palettes are verified only for materials 1–5, 7, 156, 181, 191, 196, 202, 203, 218 and 226–266; `tools/ui_export.py` assumes the rest (recorded per image in `index.json`) | Each screen loads its palette | The screen logic (U3) |

## Audio

| # | Now | Game | Exact source |
|---|---|---|---|
| A1 | Implemented from the disassembly but never exercised by Carbon's data, so exact by reading only: effects 0–6, A, C, D, E6x, F; linear pitch mode; rate change; the first-voice carry; the mixer's zero-address checks; volumes ≥ 0xFF | — | `docs/formats/audio.md` |
| A2 | Jingle system (second module state at engine `+0x83C`) not rewritten | Nothing in Carbon calls it | `FUN_081517e8`, `FUN_081522b8`, `FUN_08152310`, `FUN_0815236c` |
| A3 | Mode-0 mixer and the flagged sound format not rewritten | Carbon uses mode 1 | `0x0815CF2C`, `FUN_08152ab8`, `FUN_08152b7c` |
| A4 | Test tone not rewritten | Engine `+0x14` is never set; the function is still entered every frame (coverage) and returns early | `FUN_08151aa8` |
| A5 | Division by zero returns mGBA's HLE result | The BIOS would hang; cannot happen with Carbon's data | `bios_div` |
| A6 | Analogue output not modelled: two identical FIFOs, DAC, `SOUNDBIAS`; WAV files say 10512 Hz | Hardware runs at 10512.04 Hz | Hardware |

## Closed

- **R1, indexed colour:** exact. Index textures plus a 256-entry palette texture (`Indexed` material, `indexed.wgsl`), nearest texel with mask-equivalent wrap, BGR555 expansion `c << 3 | c >> 2`, no filtering, MSAA, tonemapping or dither. Checked: every city pixel of the route-23 chase shot is an exact palette colour; region colour sets match s15 (`docs/engine/viewer-rendering.md`).
- **R2, light tint:** exact in formats and viewer. `sector_light` + `tint_palette` every frame at the observer (the player's car in race mode); the palette is kept when there is no light. Checked: route 23 gives sector 760 (as `0x03005614`) and the tinted palette equals palette RAM 178/178 (test `light_tint_reproduces_the_race_palette`).
- **R3, car paint:** exact in formats and viewer. One shared palette from `paint::race_palette` (car ramps, glass shades from the player's heading, then the light tint); player atlas remapped into slots, opponents' raw materials. Checked on the route-23 chase shot: every player-car pixel is a race palette colour; 25 of s15's 27 car-slot colours are present (the other two are probably the rims, R23).
- **R5, sky gradient:** exact in formats and viewer. Backdrop per screen line (`sky::backdrop_entry`/`gradient_start`); the window maps onto 240×160 at 4×4. Checked: 107,520/107,520 pixels against 7 skyline captures, and every s15 sky pixel where the viewer shows sky.
- **R12, the entity draw:** exact in `nfsgba_formats::render` (`render/entities.rs`: sort, culls, LOD, the 0x1000 cut, flag 6, projection, back-face cull, clipping and the polygon rasteriser with its quirks). Verified pixel for pixel: the mid-frame reference dump up to its exact stop (span 184 of 346, row 120 of model 9's polygon 44), and 16 frame-boundary captures covering the near and far models, the 0x1000 cut and flag 6. Wheels and shadows are polygons of the car models. With it, `draw_world` reproduces whole world frames exactly. Viewer use: R23.
- **R13, the player's atlas and the opponents' cars:** exact in `nfsgba_formats::atlas` (material, remap, overlay, decal set, rims; opponents' cars, paints, materials, models and car ids from `pick_opponent_cars`, `look`, table `0x7EEA44`). Verified pixel for pixel at four race starts (cars 2, 0, 0, 7; two overlays, two decal sets, three rims), over 76 in-race rim redraws, and for every opponent at the four starts. The 128×100 materials are raw 8bpp in final slots. Viewer use: R23.
- **R4, car trim:** slots 193–207 are the city palette's own colours; nothing car-specific writes them. 192 and 208 are the glass shades (R3).
- **Environment palette and sky selection:** exact (`race_load_palettes`: palette `+0x00 + (+0x5A)·2`, sky from `+0x5E`/`+0x60`). Checked: the race's base palette equals city palette 13 (environment 11, the reference race; byte-identical to palette 3) in every non-runtime slot.
- **D1, racing line length:** exact. Section 0 of the route `+0x04` table is the lap (its last waypoint repeats the first); sections 1.. are branches, joined by waypoint links (`routes()`; route 23: 36 waypoints, 108,219 units, one branch from 19 to 27).
- **U1, HUD element logic:** exact in `nfsgba_formats::hud` (`update` = `hud_update` and every element, `reset`, `toggle`, the message system) on top of `ui::update_sprites`. Checked: 11 mGBA traces (all four race modes, forced inputs, race start, pause, time limit), 7,096 HUD frames and 2,395 calls replay exactly in objects, globals, message slots, shadow OAM, OBJ palette and OBJ VRAM. Inputs: `hud::Globals`, `Racer`, `Driver` (driver `+0x3C/+0x40/+0x44/+0xA8/+0xC5/+0x454/+0x4C8/+0x4D8/+0x4E8`, race time, split, wingman values).
- **U2, minimap:** exact (`hud_minimap`, the IWRAM window copy `0x03004B98`, the dots); every traced frame's minimap tiles match OBJ VRAM, clamp paths included (forced).
- **Menu drawing and HUD sprites:** five menu screens (language select, health and safety, EA logo, PSA, title) rebuilt from ROM with 0 bytes different from the game's frame buffer and palette; the HUD OAM entries 9–55, both affine matrices, the sprite palette and the uploaded tiles match the race dump bit for bit (`ui.rs`).
- **Decompressor:** the game's ring decoder (`lz77_ring_decode`: 4 KiB ring pre-filled with 0xFF, writes the header size) decodes all 299 packed streams; `vehicle_textures` and the menus use it (`ui::unpack`). `lz77()` keeps plain BIOS semantics for comparison.
- **Text encoding:** Windows-1252 with `{`/`|` as the A/B buttons; `text()` decodes this way. Four fonts of 224 glyphs; font 0xF takes material 14, except material 12 in `text_menu_both_pages`.
- **LS_Play music, sound effects and mixer:** exact (`nfsgba-audio`: 13,800 traced frames bit-exact in mix buffers and the whole engine work area, including 12,000 free-running frames driven only by the game's API calls).
- **Wall textures and wall u:** exact (column maps, `u >> 7`), from `raster_wall_columns` and `setup_wall_spans`. (Wall v was reopened as R20.)
- **R7, material 0:** never drawn (`draw_sector_walls` skips materials whose slot `+0x00` is 0; only material 0), and walls with flag bit 0 are open portals. Confirmed from code and in the reference frame.
- **R9, floor UVs:** 16,384 = one texture in u and v (`setup_wall_spans` and the `flat_span_affine` masks). Floor fill colour is sector `+0x0D`, ceiling `+0x0C`.
- **World renderer at 240×160:** `render::draw_world` plus the race palette (R2) is a pixel-exact software path of the game's world image; it can back an original-resolution mode or a reference view.
- **World scale and axes:** exact (one unit for cars and city; the chase-camera view matches the game's screenshot).
- **Vehicle UVs:** exact (1.15 fixed point, overlaid on the atlas).
