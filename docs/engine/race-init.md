# Race start (`race_start_from_table_a`)

`game_state_step` state 4 (`0x0812acec`) sets state 5, calls `race_start_from_table_a(world)` (`0x08139e34`), sets
up the palette fade (0x10) and then runs the first `race_frame_update` in the same `main_frame`. The port is
`nfsgba_game::race_init::race_start` (`crates/nfsgba-game/src/race_init.rs`): it turns the machine the menus left
into the race's first state, byte for byte. It runs no game code.

## What it does, in the game's order

1. **Racers:** the wingman (profile `+0x200` → `0x03006104`); the AI car count `0x030057EC` = opponents (+1 with a
   wingman 1..12; above 3: opponents 2, AI cars 3); `pick_opponent_cars` (`atlas`); the racer slot bytes
   `0x0300565C[0..4]` (`racer_slots_init`: `i` while `i < AI cars + 1`, else 0xFF).
2. **`race_init(world, 0x7F2B08 + 0x68·environment)`** (`0x081397d8`):
   - globals (phase 0, `0x0300610C` = 1, the level descriptor `0x03005620`, `0x030053E8` = 1, profile `+0x400/+0x401`);
   - the race's ARM overlay into IWRAM: `overlay_load(0x03000220, 0x5164)` (0xE4 bytes from ROM `0x08165134` and
     the overlay pointers `0x03006490…0x030064A0`), `overlay_copy(0, 0x1420, 0x08165218)` (to `0x03000304`);
   - both mode-4 pages cleared; `oam_reset` (every OAM entry hidden and copied to OAM, then the shadow reset to
     `A0 00 3C C0 00 02 00 00`; OBJ on, 1-D mapping, OBJ tile base 0x200); BLDCNT 0x3F3F, BLDALPHA 0x0D0F;
   - **`race_load_level`** (`0x08139454`): the route's counts (`+0x0C`: sectors, walls, entities, materials) and
     the level's tables into the world struct; the runtime buffers on the heap, **in this order**: sector
     offsets (`0x14 ·` sectors with an offsets record), moving pieces (`0x20 ·` walls with a piece), material
     runtime (`8 ·` materials), sector entity lists (`2 ·` sectors), entities (`0xA4 · (entities + 0x20)`), the
     route's section table (0x50, copied) and racing line (0x1800, copied), wall draw buffer 0x1A00, visible
     list 0x400, sector map 0x2000 (**not** cleared), flat vertices 0x780, plane table 0x2000, back table 0x400
     (all others cleared); `sprint_line` (sprints only: lapped flag 0, the line one slot up with extrapolated
     ends) and `rebuild_line_links`, which leaves its waypoint total in `0x03006160`; `build_line_planes`;
   - globals, profile `+0x2E8…+0x314` cleared, the material runtime table cleared;
   - **`race_load_palettes`**: the environment's city palette into both base buffers (dirty flag), the OBJ palette
     pointer, the view struct (page 0, centre, near 64, focal 150), a heap copy of the level's 4 KiB table
     (`+0x28`, world `+0x2C` → view `+0x14`), the 0xC00 vehicle matrix slots;
   - modes 0..=3: the HUD sprite screen (world `+0xA4`, screen 0 / 0 / 2 / 1 by mode), `sprite_screen_alloc`
     (0x370 on the heap), `sprite_screen_update(…, 1)` with its OBJ tile uploads, `hud_reset`;
   - `race_spawn_template_entities`, `link_template_entities`, entity 0's car id;
   - **`setup_race_cars`**: rand seed = the tick counter `0x03000044` (see Timing), `unpack_player_atlas` (the old
     atlas block freed; the new one allocated; the car material through the ring decoder, whose 0x1011-byte ring
     is allocated and freed; the overlay and decal-set materials each through a temporary heap buffer, and a
     ring when packed), `load_car_palettes`, then the four racer entities (player from its record, opponents
     from `0x7EEA44`, empty slots handler 0xE);
   - `spawn_wingman_marker`; `camera_init` (view 4 at the route's camera waypoint, level descriptor `+0x66`; floor
     height below it); phase 9 (intro), view 2; `camera_place` (OAM hidden and copied; the chase orbit position);
     the effect-sprite list (0x20 objects on the heap); `load_car_palettes` again after paints[0] = record `[6]`;
     `sky_gradient_start` (DISPSTAT's VCount IRQ enable); HUD material 23's tiles to OBJ slot 0x1C0; the race
     music (`profile +0x2EE` = `rand & 3`, music id = that + 1, `snd_play_module` → engine `+0x1478`); the engine
     sound id (`profile +0x2EF`); `hud_toggle(0)` when the HUD option is off; `traffic_init`; with a route number,
     `moving_pieces_init` (the route's list at `0x7F3A24` starts closed).
3. `0x03006098` = 0.

**Every freed heap block keeps what was written into it** (the decoder rings, the decoded overlay and decal
materials, the decoder's 8-byte overrun past each destination). The port writes them all: they are part of the
race's RAM (R24: the rim draw later reads next to its buffer).

## Inputs

What `race_start_from_table_a` reads before writing it (`tools/race_init_inputs.py`, read hook in the oracle; the
career capture: 259 bytes):

| Where | What |
|---|---|
| EWRAM `0x02000000…0x02000107` | the heap's node table: which blocks the boot and the menus left allocated (the new blocks go into its gaps) |
| car record (`0x02000901 + 0x11·car`) `[0] [1] [3] [4] [6]` | spoiler index, overlay, material, decal set, paint |
| profile `+0x200` | wingman |
| IWRAM | current music id `0x0300003C`, units, tick counter `0x03000044`, player `0x03000060`, environment `0x0300006C`, career flag `0x030000A0`, route number `0x03005388`, car records `0x0300539C`, gradient buffer `0x030053B8`, base palette pointers `0x030055F0`/`0x0300577C`, language and traffic `0x03005600…07`, link flag `0x03005624`, HUD option `0x03005698`, mode `0x030056E0`, profile `0x030056EC`, route index `0x03005720`, opponents `0x03005784`, `paints[0]`, `cars[0]`, **lapped flag `0x0300608C` (the previous race's)**, old atlas `0x03006164`, sound engine `0x03006370`, frame buffer struct `0x03006410`, rand index and heap pointers `0x030064C8…0x030064D3` |
| I/O | DISPCNT, DISPSTAT |

It reads nothing of the freed memory it writes over; every byte it does not write keeps the previous scene's
value. Laps, difficulty and catch-up are not read here (the car init reads them in the first race frame).

A race from the ROM alone therefore needs the boot and menu code that produce these bytes, above all the heap's
block layout (the sound init and the menus' live allocations fix every heap address the race uses).

## Timing

`setup_race_cars` seeds the RNG with the tick counter, which the VBlank IRQ advances while the race loads. The
port takes the number of VBlanks before that point (`seed_vblanks`) as an input and leaves the counter in RAM to
the IRQs. In the 14 recorded starts it is 6 or 7, found as the one value for which the game's code (oracle)
equals mGBA (`NAME_seed.txt`). The race music (`rand & 3`) and every later random draw depend on it.

## Verification

- **Captures** (`tools/race_init_capture.py` + `tools/race_init_capture.lua`, session `race-init`): the machine at
  the entry and return of `race_start_from_table_a`, from race-info savestates of other sessions and the main
  menu. 14 starts: all four modes (circuit, elimination ×2, hunter ×2, sprint ×3), routes 1, 3, 19, 25, 40,
  environments 0, 1, 9, 10, traffic 0–2, difficulty 0–2, a Quick Play wingman, a career event (wingman 2, career
  flag, the previous race's lapped flag 1 and atlas block), and each Quick Play setup twice from different menu
  histories (other RNG index and tick).
- **Against the game's code** (`tools/race_init_oracle.py`): the oracle runs `race_start_from_table_a` on each
  entry state; `tests/race_init.rs` requires the port's EWRAM, IWRAM, I/O, palette, VRAM and OAM to equal it byte
  for byte. 14/14.
- **Against mGBA:** with the recorded seed timing, the port equals the emulator's state at the return in every
  byte outside the IRQs' own writes (sound engine block, counters, mix buffers, IRQ stack, DISPSTAT/VCOUNT, the
  sound FIFOs and DMA 1/2, and the rand index the seed feeds). 14/14.
- **Mutations** that each fail the test: the ring keeping the final literal, OAM tile 0x201, the old atlas not
  freed, the sector-list link, the rebuild's scratch total, the plane table not built, the seed ignoring the
  timing.

## Not 1:1 / open

- **The handover:** the rest of the state-4 `main_frame` (the fade set-up, `FUN_0815e9e8`, the palette copy and
  light tint with `0x03005630`) and the first inline `race_frame_update`, and then the intro frames (phase 9,
  fade 0x10), are not in the game loop yet (G1: `Game::frame` stops at the fades and the countdown), so the
  replay cannot continue from this state yet.
- **Races from the ROM alone** need the boot and menu ports that produce the inputs above (the heap layout first).
- Not exercised by a capture: link play (`0x03005624`), a route index 0, `unpack_material_format4` (stops with
  `Unported`), a heap that runs full.
