# The race game loop (`crates/nfsgba-game`)

One game frame of a race, in the game's exact order, composed from the exact subsystems: the car steps
(`nfsgba-sim`), the world and cars (`render`), the sky, the car paint and the light tint (`paint`,
`tint_palette`), the HUD (`hud`, `ui`) and the sound engine (`nfsgba-audio`). `Game` owns the whole machine
state (EWRAM, IWRAM, palette RAM, VRAM, OAM) in the game's own layout. ROM `BN7E` v0, SHA-1 `e5298b24…`.

**Status:** every traced frame is exact, byte for byte in RAM, VRAM, palette, OAM and the sound engine, from a
reference state (149 of 149 frames) and as a free run carrying its own state (149 of 149). Three pieces are not
ported yet and are stood in for from the reference build in the replays (listed under [Not ported](#not-ported)).
The viewer drives it live (`NFSGBA_PLAY=1`), with stand-ins for those pieces.

## No frame pacing: timing is an input

- `game_main` (`0x0812a950`) ends in `run_frame_loop` (`0x081621c0`), which calls `main_frame` (`0x0812ae64`)
  back to back forever. There is no VBlank wait: a race frame lasts as long as the CPU needs, about 3.9 video
  frames in the reference race (581 video frames for 150 game frames).
- **Timer 3** measures it. `main_frame` stops timer 3, reads it (1,024-cycle ticks, stored at `0x03005934`; 0
  counts as 0x200), restarts it, and sets the physics' frame time `0x03005640 = clamp(25,500 / ticks, 10, 100)`
  (15 in link play, `0x03005624 == 2`).
- **The VBlank IRQ** (`vblank_irq` `0x0812ac14`) runs wherever the frame happens to be: it mixes the next sound
  buffer, counts `0x030053B4`, the race time `0x03005800` (while `0x03005398 == 0` and `0x03000048 == 2`),
  `0x03000044` and `0x03005724`, and sets the sky gradient's start (`0x030056E8`). What the game reads between two
  IRQs therefore depends on where they land: the race time in `route_gap` (inside the player's car step, which
  writes the split time `0x0300615C`) and in `hud_timer`, and the sound commands relative to the mixes.
- So besides the keys, a frame takes a [`Timing`]: timer 3's ticks, and the IRQ count at `update_entities`, at
  each sound call of the car steps, at `route_gap`, at `hud_update` and `hud_timer`, and at the end. A replay takes
  them from the trace; live play uses `Timing::steady()` (1,098 ticks, four IRQs while the world is drawn), NOT 1:1.
- The sim runs a car step whole. The IRQs that fell inside it are handled exactly anyway: their race-time ticks
  are lent to the step (so `route_gap` and the lap crossing read what the game read), and the IRQs themselves run
  after it, between the recorded sound commands they fell between (the sim records commands instead of running
  them).
- The VCount IRQ (`0x030001C0`) rewrites palette entry 0 every two lines from the gradient; `Game::backdrop()`
  gives the colour of every line.

## One frame (`Game::frame`)

`main_frame` in a race (game state `0x03005808 == 5`):

1. Timer 3 → frame time (above).
2. `flip_page` (`0x0812b084`): frame counter `0x03005628` even → show page 0 and draw into the frame buffer
   struct's `+0x10` (`0x0600A000`); odd → show page 1 and draw into `+0x0C` (`0x06000000`); view `+0x00` (`0x03000080`)
   = the page to draw.
3. `game_state_machine` (`0x0812acec`) → `race_frame_update` (`0x0813a954`):
   1. `0x030056F0` += 1; the engine loop is restarted when sound slot 1 fell silent (`carbon_sound_playing`,
      `restart_engine_sound`).
   2. `shade_car_paint`: the glass shades into both base palette buffers and palette RAM 192/208.
   3. `0x03005394` = 0 (matrix slots); profile `+0x2E0`/`+0x2E4` = 0; world `+0xF6` = 0.
   4. `update_entities`: every entity with state bits 0 and 1 set: the car handler (`nfsgba_sim::car::handler`) for
      handlers 0..3; others are not ported (opponents 0x29, traffic 0x36, sparks 0x34: [Not ported](#not-ported)).
   5. `camera_dispatch` (`0x081389a0`): the view's camera function from `0x087F399C` (chase view: `camera_update`,
      which ends by building the visible-sector list; `render::visible_sectors` here, which sets world `+0xF6`
      when a listed sector has no ceiling).
   6. The matrix slots: `build_player_matrices` for the player, `assign_entity_slot` (with `opponent_effects`) for
      the other racers, `FUN_08144c98` for traffic.
   7. `draw_skyline` (when world `+0xF6`), then `draw_visible_sectors` (`render::draw_world`, cars included; the
      entity fields it writes, `+0x04`, `+0x0A`, `+0x28`, go back to RAM).
   8. `race_start_from_table_b` (nothing once the countdown is over), `hud_update` and `sprite_screen_update`
      (`hud::update`, `ui::update_sprites`, the minimap tiles and the uploads into OBJ VRAM). The HUD's digits
      divide through the IWRAM routine, whose remainder (`0x03006480`) ends as the speed's ones digit.
   9. The race positions (`FUN_0813ea04`, driver `+0xA8`), the car-contact sound (sound 0x20 at rate 0x4B0 when
      profile `+0x2E0` and `+0x2E4` are set and driver `+0x4D1` is 0), and the wrong-way message (2): shown for
      0x3C frames while `0x03005384`, else cancelled.
4. `draw_effect_sprites` (`0x08161f38`): the effect-sprite list (header `0x03000058`: 32 objects of 20 bytes at
   `0x0202C3A0`, OAM entries 127 down to 96) into the shadow OAM, through the game's OAM setters; one-shot objects are
   cleared. `copy_shadow_oam` (`0x0816102c`): shadow OAM `0x030064F0` → OAM.
5. `0x03005628` += 1; the light tint (`apply_sector_light_to_palette`: the base buffer `*0x030055F0` tinted by the
   light at the player's position in the camera sector `0x03005614`, into palette RAM 1..143 and 149..255).
6. `read_keys` (`0x0812b040`): `0x030064C0` newly pressed, `0x030064C4` held (`0xFC00 | keys`), the player's control
   word `0x030057D8 + 2·player`. The keys a frame samples at its end steer the next one.

## Verification

- **Traces:** `tools/mgba_game_trace.lua` (loaded next to the remote through `NFSGBA_MGBA_SCRIPTS`) records the
  whole machine state at every `main_frame` entry, and per frame the IRQ counter at the points above, timer 3's
  ticks, and the effect-sprite list as `draw_effect_sprites` finds it. `tools/game_trace.py record drive` plays
  the scenario from `race.ss` (accelerate, steer both ways, brake, accelerate; 150 game frames, 581 video frames)
  and `pack` stores it as a base state plus per-frame byte runs (5.5 MB), in `data/work/e5298b24/game-loop/`.
- **`tests/replay.rs`:**
  - `frames_match_the_trace`: from each traced state, one `Game` frame with that frame's keys and timing gives the
    next traced state byte for byte: 149 of 149 frames. Left out only: the IWRAM stack, palette entry 0 and the
    VCount read pointer (the display's current line), and the renderer's scratch buffers (visible list, sector
    map, wall and flat buffers, rasteriser state), which no later frame reads before rewriting them.
  - `free_run_matches_the_trace`: the same from the first state on, the game carrying its own state (the player's
    car, sound engine, HUD, palette, frame buffers): 149 of 149 frames exact.
  - `live_play_divergence` (ignored; run with `--ignored --nocapture`): with the opponents frozen the run leaves the
    reference in frame 0 (opponent 1's driver struct, `0x0202D168`: their AI runs every frame); with
    `standin::slots` for the matrix slots it stays exact through frame 98 and leaves it in frame 99 at the effect
    list (`0x0202C3A0`: the brake smoke sprites the unported effect code spawns).
- **Stand-in check:** `standin::slot_matrix` gives the player's slot matrix exactly in 149 of 149 frames
  (braking, steering and wall contacts included).
- **Sensitivity:** leaving the light tint out makes all 149 frames fail.

## Play mode (viewer)

`NFSGBA_PLAY=1 NFSGBA_DUMP=game-loop/s18 cargo run --release -p nfsgba-viewer`: a machine state taken at
`main_frame`'s entry with palette, VRAM and OAM (any trace state; the `mgba/race` dump was taken inside the
renderer). One game frame every four video frames (59.7275 Hz); keys: arrows, X = A, Z = B, A = L, S = R, Enter =
START, Backspace = SELECT, or `NFSGBA_PLAY_KEYS=A*80,A+LEFT*15,...` (game frames). The racers, camera, visible list
and palette are read back from the game's RAM each game frame; O shows the GBA screen as the game composes it: the
page on display, each line's backdrop, and the OAM sprites with the race's blend (`BLDALPHA 0x0D0F`: semi-transparent
HUD sprites brighten what is under them). Stand-ins, all NOT 1:1: the viewer's `Chase` for `camera_update`,
`standin::slots` for the matrix slots, `Timing::steady()`, the opponents frozen, no sound output yet. Play stops at
the first unported code path and logs it (the pause menu, the race end, …).

## Not ported

`Game::frame` returns `nfsgba_sim::Unported` rather than guess:
- `camera_update` and the matrix-slot code (`build_player_matrices` with its effect sprites and sparks,
  `assign_entity_slot`/`build_entity_matrix`, `FUN_08144c98`) must be provided at their checkpoints: by the trace in
  replays, by stand-ins in live play;
- game states other than the race (the state machine's menus and race start), the race countdown
  (`race_start_from_table_b` in phases 1 and 9), the race-start set-up (`0x03005714 == 3`), the race-end countdown
  and the race end, the pause menu (START), palette fades (`0x03005630`), other camera views, link play;
- the opponents' and traffic handlers (0x29, 0x36) and the sparks (0x34) are skipped (`Game::skipped`), their
  entities keep their state (D4).

## Integration notes (game-loop)

**FIDELITY.md:**
- **New T1 (timing):** Now: a frame's timing (timer 3's ticks, where the VBlank IRQs land) is an input: exact from a
  trace (`Timing`), `Timing::steady()` in live play. Game: the frame runs back to back with no pacing, its length and
  the IRQ positions follow the CPU's cycle count. Exact source: ARM7TDMI and bus cycle timing (not modelled).
- **U5 (HUD timing):** narrowed: the game loop reads the race time for the HUD at the recorded `hud_timer` IRQ point,
  exact on every traced frame; the portrait and arrow still take the same single value.
- **R25:** add: "`nfsgba_game::standin::slots` (live play) builds the slots the way `build_entity_matrix` does for
  flag-bit-5 entities (physics rotation × camera); it gives the player's matrix exactly on 149 of 149 traced frames.
  Missing: the effect sprites and sparks the player's matrix code spawns (first divergence frame 99, brake smoke),
  the `+0x30`/`+0x32` rotation path of entities without a driver."
- **R11:** add: "`camera_update` is not ported in `nfsgba-game`; live play stands in the viewer's `Chase`."
- **New entry (game loop):** the unported frame paths above (other game states, countdown, race end, pause, fades,
  other views, link play), each an `Unported` stop.
- **New entry (viewer):** play mode draws the HUD from OAM: exact in the original-resolution frame (the game's
  alpha blend), 50% alpha over the high-resolution view (NOT 1:1); OBJ priority against BG2 is not modelled; no
  sound output.
- **Closed:** "The race frame loop: `nfsgba_game::Game::frame` reproduces `main_frame`/`race_frame_update` with the
  IRQs at their recorded points, byte for byte on 149 of 149 traced frames, per frame and free-running (with the
  opponents' AI, `camera_update` and the matrix-slot code stood in from the reference)."

**address-map.md / symbols.csv:** `docs/engine/notes/addresses.game-loop.csv` and `symbols.game-loop.csv` (29
symbols; `tools/notes_merge.py` finds no conflict).

**Other notes:**
- `hud::update` does not write the IWRAM divider's remainder (`0x03006480`); the game loop recomputes it (the
  speed's ones digit). The `hud.rs` owner could return it instead.
- `career::RacingLine::update_places` is the same rule as `FUN_0813ea04`; the game loop runs it on the game's live
  tables (world `+0x40`/`+0x44`), as the game does.
- OPEN-QUESTIONS: what reads `0x03005724` and `0x03005728` (counted and flagged by every VBlank IRQ); what the
  handler-0x34 entities spawned by the player's contacts are (sparks?) and what their handler does.
- TOOLS.md: `tools/mgba_game_trace.lua` + `tools/game_trace.py` (game-frame traces, `test_game_trace.py`);
  `tools/decomp_show.py` (a decompiled function with every literal-pool `DAT_` resolved to its value and symbol).
- INDEX.md: `engine/game-loop.md`. PROGRESS.md: the game-loop line (done).
