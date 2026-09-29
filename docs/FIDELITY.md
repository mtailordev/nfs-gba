# Fidelity ledger

The goal is an **absolute 1:1 rewrite**: same data, same maths, same behaviour. This file lists every place where our code is *not yet* exact. Each entry names what we do now, what the game does, and where the exact behaviour lives. Code that deviates carries a `NOT 1:1` comment pointing here.

An entry is closed only when the exact behaviour is implemented **and** checked against the reference build (mGBA RAM dumps, screenshots, or traces).

## Rendering

| # | Now | Game | Exact source |
|---|---|---|---|
| R6 | Skyline exact in the viewer except the fill leftovers: the viewer clears them | `bytes % 32` above the skyline (and nothing below) keep the page's previous frame; none in the chase view | `draw_skyline` (`nfsgba_formats::sky`) |
| R10 | **Exact in `render` and in the viewer's visibility:** the viewer draws the game's per-frame visible list (entries past `transform_walls`, clipped to their spans) and, per entry, the walls `draw_sector_walls`/`raster_wall_columns` draw (279 kept, 56 dropped over all 43 routes' chase frames, equal to `render`). **Open:** hidden surfaces come from the depth buffer, not the game's painter's order | Painter's order back to front | Viewer |
| R11 | **Chase camera exact** (`game::Chase::step` reproduces the dump's camera state, matrix `0x030057A0` and sector 760); projection focal 150, centre (120, 79), `d + 1`, near 64. **Open:** the speed effect on the focal (driver `+0x4D1`, rule documented), the wall-push height limit (`floor_height` smoothing), `find_sector_far`, integer rounding at high resolution | `camera_update`, `camera_push_out_of_walls` | Viewer |
| R14 | Walls exact (the pair rule on the 240-column grid; opaque walls show the backdrop). **Open:** index 0 on car models is dropped per pixel | The model rasteriser | `raster_polygon`, `draw_polygon_span` |
| R16 | Floors and ceilings are drawn at full window resolution | 120×160 (`flat_span_affine` `0x03004fa8` writes one byte per two pixels; confirmed on s15); wall edges at 120 columns with 240-column texturing. Exact in `render::draw_world` | Only relevant to a 240×160 original-resolution mode |
| R17 | `paint::race_palette` gives the raw glass shade in slots 192/208 | For 1–7 scanlines per game frame, 192/208 show the tinted shade (the tint runs at the end of a frame; the next frame's `shade_car_paint` restores the raw value) | Needs scanline timing of the whole frame |
| R18 | The palette-0 write is applied to the whole scanline (as the reference emulator does) | On hardware the left edge of each even line may still show the previous colour | Unmeasured; needs hardware timing |
| R20 | Wall v fixed in `Wall::uv` from the code: 1/128 texel rows, top `+0x10 + +0x42·128`, span `+0x18`, same at both ends (2,119 of 2,305 drawn walls span exactly one texture height). **No pixel check yet** (the one 64-row wall in the reference frame is hidden); the material's runtime v scroll (world `+0x48`) is not applied | `setup_wall_spans`, `raster_wall_columns` | A frame showing a 64-row wall; the writers of the runtime tables |
| R21 | Moving wall pieces, sector offsets, animated and scrolled materials (world `+0x18`/`+0x1C`/`+0x48`) are read as static | Runtime tables written by door/animation code, not decoded. In every captured race (routes 18 and 23, 20 dumps) all 122 moving pieces are at zero offsets with flags 1 (open); the viewer uses that state (`game::race_runtime`) | Their writers |
| R25 | Matrix slots (world `+0xFC`) are inputs to `render`; the code that builds them is not reimplemented | `build_player_matrices` (`0x0814bc30`), `assign_entity_slot` → `build_entity_matrix`, `assign_billboard_slot`, with a one-frame delay before a car appears and a stale matrix beyond depth 0xDAC; the axes of the two rotations are not checked | `0x0814eba0`, `0x0814da68`, `0x0814ec0c` |
| R26 | Implemented from the code but in no capture: bit-4 sector entities, material steps `+0x44`/`+0x46`, negative `+0x64`; effect sprites (`opponent_effects`, `spawn_effect_sprite`) not implemented | — | New captures |
| R27 | High resolution differs from the 240×160 frame by construction: continuous edges (spans kept `left ..= right`), surfaces thinner than a GBA pixel stay visible, textures sampled per window pixel. Agreement with the original frame at the reference camera: 48.8% exact, 82.7% within one pixel. In the viewer, cars are not clipped to their portal span and grid cars' rims stay at angle 0 | The original-resolution frame (`render::draw_world`) is exact: 100% of s15 outside the HUD | Design choice for the high-resolution mode; the original-resolution mode is the reference |
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
| D14 | The grid deal's rand index (0x11 for the reference race) is derived (the only index of 256 giving the reference racers), not traced; the seed `*0x03000044 & 0xFF` = 3 comes 14 draws earlier | — | Trace `rand_table` calls from the seed to `pick_opponent_cars` |
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
- **R8, portal step walls:** portal walls with flag bit 0 clear and material ≠ 0 are drawn as solid walls, and a moving piece's flags replace the wall's (1,644 solid and 661 portal walls by the ROM flags, 539 in races), in `render` and the viewer.
- **R15, the tint's sector:** it is the camera sector `0x03005614` (`apply_sector_light_to_palette` reads it with the player's position), taken from the exact chase camera. The free camera's point-in-polygon search is a non-game mode.
- **R19, flat heights:** floors and ceilings use `Wall::floor_y`/`ceiling_y` (`+0x38`/`+0x3A`).
- **R22, row 159:** shows the backdrop: 240/240 pixels against the original frame, 106/106 against s15 outside the HUD.
- **R23, racers in the viewer:** dealt by `atlas::pick_opponent_cars`/`look` (rand index 0x11, D14); the player atlas from `atlas`; LOD models and spoiler as `draw_sector_entities`; with a dump, poses from the vehicle matrices.
- **Original-resolution frame:** the viewer's 240×160 mode (`render::draw_world` + the race palette) equals s15 on 25,716/25,716 pixels outside the HUD, car included.
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
