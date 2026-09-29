# The race game loop (`crates/nfsgba-game`)

One game frame of a race, in the game's exact order, composed from the exact subsystems: the car steps
(`nfsgba-sim`), the world and cars (`render`), the sky, the car paint and the light tint (`paint`,
`tint_palette`), the HUD (`hud`, `ui`) and the sound engine (`nfsgba-audio`). `Game` owns the whole machine
state (EWRAM, IWRAM, palette RAM, VRAM, OAM) in the game's own layout. ROM `BN7E` v0, SHA-1 `e5298b24…`.

**Status:** nothing is stood in any more. The opponents' and traffic AI, `camera_update`, the matrix slots, the
effect sprites and the spark entities all run in `Game::frame`. Five traces hold 2,445 frames. 2,444 of them are exact,
byte for byte in RAM, VRAM, palette, OAM and the sound engine, both from each traced state and as free runs that carry
their own state. The other one stops with `Unported` in a car path the physics-paths work still owns (D9–D11). Only the frame timing (T1) and the keys come from the trace. The viewer drives it live
(`NFSGBA_PLAY=1`) and plays its sound. The race start, countdown, fades, race end and pause are not in the loop yet
([Not ported](#not-ported)).

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
- So besides the keys, a frame takes a [`Timing`], which a replay reads from the trace and live play sets to
  `Timing::steady()` (1,098 ticks, four IRQs while the world is drawn; NOT 1:1). It holds timer 3's ticks and the
  IRQ count at these points:
  - `update_entities`;
  - the entry and the return of each sound call of the entity handlers;
  - `route_gap` and its two race-time reads;
  - each opponent's lane-timer read;
  - `hud_update` and `hud_timer`;
  - the end of the frame.
- The sim runs an entity step whole. The IRQs that fell inside it are still handled exactly:
  - **Race time:** their ticks are lent to the step, so it reads what the game read (`route_gap`, the lap
    crossing, the opponents' lane-change timer at `0x0813C95C`).
  - **`route_gap`:** it reads the race time twice, around `__divsi3` (split = rt₂ − x·rt₁ / y). An IRQ between
    the reads adds `second − first` to the split `0x0300615C` afterwards.
  - **Sound:** the IRQs run after the step, between the recorded sound commands they fell between (the sim records
    commands instead of running them). A rate change counts from the call's return, because
    `carbon_set_sound_rate` divides through the BIOS before it stores. A stop counts from its entry, since it
    zeroes the volume first. A play that an IRQ split is `Unported`; none was seen.
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
   4. `update_entities` runs the handler of every entity with state bits 0 and 1 set, from table `0x087F38B8`:
      - **0..3, the car** (`nfsgba_sim::car::handler`). For the player seen from the side, the rim redraw on the
        atlas comes first (`slots::rim_redraw`).
      - **0x29, opponents and wingman** (`nfsgba_sim::ai::handler`), then `opponent_effects`.
      - **0x34, sparks** (`slots::effect_handler`).
      - **0x36, traffic** (`nfsgba_sim::traffic_ai::handler`).

      The sound commands of each handler are played at their recorded IRQ points.
   5. `camera_dispatch` (`0x081389a0`) calls the view's function from `0x087F399C`: `camera::update` for views
      0–6, nothing for a 0 entry. `camera_update` ends by building the visible-sector list: `render::visible_sectors`
      here, which sets world `+0xF6` when a listed sector has no ceiling.
   6. The matrix slots (`slots::race_slots`):
      - `build_player_matrices` for the player: body and spoiler, lights, exhaust and nitro flames, sparks of a car
        contact, the horizon shift;
      - `assign_entity_slot`/`build_entity_matrix` with `opponent_effects` for the other racers;
      - `FUN_08144c98` for traffic: slots and rear lights.
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
- **More traces (live-race):** in `data/work/e5298b24/live-race/`, recorded with `NFSGBA_MGBA_SESSION=live-race`.
  The mode `racing` arms at the first frame the loop runs whole: game state 5, phase 2, no fade, the start set-up
  done.

  | Trace | Frames | Start | What it covers |
  |---|---|---|---|
  | `live` | 700 | `circuitinfo.ss`, racing | LONGPOINT circuit, hard, three opponents, heavy traffic: bumping the neighbours on the grid, braking (brake smoke), swerves, a car-to-car contact in frame 387 |
  | `trail` | 700 | `sprintinfo.ss`, racing | a sprint behind the opponents through heavy traffic: braking, handbrake turns |
  | `views` | 600 | `race.ss` | SELECT to the bumper view and back, DOWN to look back in both views, L and R |
  | `nitro` | 300 | `race.ss` | nitro poked into the tank (`poke 0x0202CAEE 1`: the Quick Play car has none): the focal-length speed effect and the nitro flames |

- **Recorder columns** (`NAME.csv`), each an IRQ count unless noted:
  - `video_frame`;
  - `keys`;
  - `vblanks_start`, `vblanks_entities`, `vblanks_hud`;
  - `timer3`: ticks, not a count;
  - `vblanks_sounds`: `entry-return` pairs of the entity handlers' sound calls, at breakpoints on
    `carbon_play_sound`/`stop`/`set_sound_rate` and on direct `snd_play_sfx` calls;
  - `vblanks_gap`: `route_gap`;
  - `vblanks_timer`: `hud_timer`;
  - `effects`: the effect-sprite list as `draw_effect_sprites` finds it, in hex;
  - `lanes`: `driver:count` at `0x0813C95C`;
  - `gap_reads`: at `0x0813ECB6`, `0x0813ECC0`, `0x0813EDAC` and `0x0813EDB8`.
- **`tests/replay.rs`**, all five traces, nothing stood in:
  - **`frames_match_the_trace`:** from each traced state, one `Game` frame with that frame's keys and timing gives
    the next traced state byte for byte.
    - Results: `drive` 149/149, `live` 698/699 (frame 387 stops in D10 `FUN_08144fa4`, car to car), `trail`
      699/699, `views` 599/599, `nitro` 299/299.
    - Left out of the comparison: the IWRAM stack, palette entry 0 and the VCount read pointer (the display's
      current line), and the renderer's scratch buffers (visible list, sector map, wall and flat buffers,
      rasteriser state), which no later frame reads before rewriting them.
  - **`free_run_matches_the_trace`:** each trace as one run from its first state. The game carries every car, the
    camera, the slots and effects, the sound engine, the HUD, the palette and the frame buffers. Every frame is
    exact: `drive` 149, `trail` 699, `views` 599, `nitro` 299, and `live` 387 up to the D10 stop.
  - **`one_frame`** (ignored): one frame's differences, e.g.
    `NFSGBA_TRACE=live NFSGBA_FRAME=193 cargo test --release -p nfsgba-game one_frame -- --ignored --nocapture`.
- **Sensitivity:** each of these mutations fails the replays:
  - the light tint left out (every frame);
  - the camera's focal step (`nitro`);
  - one traffic light point (`live`, `trail`);
  - the spark lifetime (`live`).

## Play mode (viewer)

`NFSGBA_PLAY=1 NFSGBA_DUMP=game-loop/s18 cargo run --release -p nfsgba-viewer`: a machine state taken at
`main_frame`'s entry with palette, VRAM and OAM (any trace state; the `mgba/race` dump was taken inside the
renderer). One game frame every four video frames (59.7275 Hz); keys: arrows, X = A, Z = B, A = L, S = R, Enter =
START, Backspace = SELECT, or `NFSGBA_PLAY_KEYS=A*80,A+LEFT*15,...` (game frames). The racers, camera, visible list
and palette are read back from the game's RAM each game frame; O shows the GBA screen as the game composes it: the
page on display, each line's backdrop, and the OAM sprites with the race's blend (`BLDALPHA 0x0D0F`: semi-transparent
HUD sprites brighten what is under them). The camera, slots, AI and effects are the game's own. The sound the
VBlanks mix (`Game::sound`, signed 8-bit at 10,512 Hz) goes to a Bevy audio source (`play::GbaSound`), about a
quarter of a second at most. NOT 1:1:
- `Timing::steady()` for every frame (T1);
- the output rate is 10,512 Hz rather than 10,512.04, and the DAC and `SOUNDBIAS` are not modelled (A6);
- the HUD is blended at 50% over the high-resolution view (G2).

Play stops at the first unported code path and logs it (the pause menu, the race end, …).

## Not ported

`Game::frame` returns `nfsgba_sim::Unported` rather than guess.

**The race's own frame:**
- The car paths of D9–D11 (physics-paths).
- View 7 (`camera_look_at_player`, the dispatch's only other function).
- A camera probe that needs `find_sector_far` (`FUN_0814dbbc`).
- `FUN_0814e050` without `0x0300610C`, or with the raised camera `0x03006148` (`FUN_08140224`).
- A rim redraw whose read window overlaps the atlas it writes.
- A sound play that an IRQ split.
- Entity handlers other than 0..3, 0x29, 0x34 and 0x36.
- Link play.

**Around the race (the race-init handover, open):**
- game state 4: the rest of the frame after `race_start` (the fade set-up, the debug print `FUN_0815e9e8`, the
  palette copy and light tint);
- the intro frames (phase 9), the countdown (`race_start_from_table_b` in phases 1 and 9) and the start set-up
  (`0x03005714 == 3`, `FUN_0813a054`…`FUN_0813a108`);
- the palette fades of `main_frame` (`0x03005630`);
- the race-end countdown (phases 6–8), the race end (phase 3) and the state-5 exit of `game_state_step` (sound
  stops, `fill_results`, `FUN_081396c4`'s frees, the screen change);
- the pause (START) and every menu frame (game state 1).

The plan for this is under [Integration notes (live-race)](#integration-notes-live-race).

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

## Integration notes (live-race)

Branch `worktree-agent-acfa1b330ee6787fd`: commits `e599be1`, `a140b32`, `6f068e8`, merge of main `c523ff0`, and
this doc commit.

**FIDELITY.md:**
- **R25 (matrix slots):** close. `slots.rs` ports the slot code. Exact on all five traces, per frame and
  free-running, with nothing stood in:
  - `build_player_matrices` and `FUN_0814e050` (the physics orientation, contact sparks, the spoiler);
  - `assign_entity_slot`/`build_entity_matrix`, `assign_billboard_slot`;
  - `opponent_effects` with the rear and brake lights;
  - the exhaust and nitro flames (`FUN_0814e8b4`);
  - traffic (`FUN_08144c98`, rear lights);
  - the effect-sprite pool (`FUN_08162048`, `spawn_effect_sprite`, `FUN_0814eca4`).

  Open (`Unported`): `FUN_0814e050` without `0x0300610C` or with the raised camera `0x03006148` (`FUN_08140224`),
  and link play's per-racer `build_player_matrices`.
- **R11 (camera):** `camera.rs` ports `camera_dispatch` (`0x081389a0`, table `0x087F399C`) and `camera_update`
  (`0x08137cb0`, the `param_3 = 0` branch, the only one the dispatch uses). That covers:
  - the chase and bumper views (SELECT, `0x030053E4`; the reset behind the car on switching back);
  - looking back (DOWN);
  - the nitro focal-length effect;
  - the probe sector (`0x7BFC68`), the ceiling flag (profile `+0x400/+0x401`);
  - `camera_push_out_of_walls` (`0x08137744`).

  Exact on all traces. Open: view 7 `camera_look_at_player`, and a probe that needs `find_sector_far`
  (`FUN_0814dbbc`). The viewer's `Chase` stays only for the free camera.
- **D4 (AI in the loop):** the opponents, the wingman (0x29) and traffic (0x36) run from `Game::frame` in
  `update_entities` order, with their sounds. The `live` and `trail` traces (1,400 frames, three opponents, heavy
  traffic) are exact; D9–D11 stops surface as `Unported`.
- **D17 (lane-change timer):** exact in the loop. The race time the AI reads at `0x0813C95C` is recorded per driver
  (the `lanes` column) and lent to the step (T1).
- **D18 (effect handler 0x34):** close. `slots::effect_handler` ports the spark entities (movement scaled by timer 3,
  ageing, the lifetime from `0x0836D010 +0x10`, the billboard slot and sprite), and `FUN_0814c37c` spawns them.
  Exact on `live` (contacts).
- **R24 (rim redraw):** exact in the loop. `slots::rim_redraw` runs `atlas::draw_rim` on the game's real heap
  (`race_init` made the layout exact). It refuses (`Unported`) only when the read window overlaps the atlas it
  writes; that never happened in the traces.
- **U5 (HUD timing):** unchanged; still exact at the recorded `hud_timer` point.
- **T1 (timing):** extend "Now". The recorded IRQ points are:
  - timer 3;
  - `update_entities`;
  - each entity sound call's entry and return (a rate change counts from the return, after the BIOS division; a
    stop from the entry);
  - `route_gap` and its two race-time reads (the split correction);
  - each opponent's lane-timer read;
  - `hud_update`/`hud_timer`;
  - the frame's end.

  A play that an IRQ splits is `Unported` (never seen). Live play uses `Timing::steady()`.
- **G1:** narrow further. The race frame needs no stand-in; open is only the race-init handover (below).
- **G2 / viewer:** the viewer's play mode has no stand-ins any more and plays the game's sound (A6: 10,512 Hz rather
  than 10,512.04, no DAC or `SOUNDBIAS`). The HUD is still blended at 50% over the high-resolution view.
- **Closed (game loop):** remove "the opponents' AI, `camera_update` and the matrix-slot code stood in from the
  reference" from the game-loop entry.

**address-map.md / symbols.csv:** two notes files.
- `docs/engine/notes/addresses.live-race.csv`, 24 rows:
  - the camera globals, the pool layout, the effect materials, the driver's light and flame fields;
  - from the handover recon: the debug console, the countdown accumulator and digit, the pause flag, the
    finishing car, the result places.
- `symbols.live-race.csv`, 39 functions:
  - the slot helpers, the lights, the flames, the sparks, the ROM copies of the IWRAM maths;
  - from the handover recon: the debug print and `itoa`, the sound stops, the race cleanup and its frees, the
    results tie-break, the start set-up's tile uploads. Run
`tools/notes_merge.py docs/engine/notes/*.live-race.csv`.

**Other notes:**
- `nfsgba-sim` records sound commands instead of running them. The loop replays them against the audio engine at
  the recorded IRQ points (`Game::play_commands`).
- The replay test's `EXPECTED_STOPS` lists the D9–D11 functions: when physics-paths ports one, its stop disappears,
  and the frames after it must then match.
- `tools/game_trace.py`:
  - scenarios take optional RAM pokes before arming (`nitro`);
  - the recorder arms at the first whole racing frame (`racing`);
  - the wait loop allows 10 minutes.

**Open: the race-init handover** (the coordinator's added scope, not started, paused for the review). The plan, from
the decompiled code:
- **State 4 in `Game::frame`**, after `race_init::race_start`:
  - `MENU_EXIT` 0, state 5, fade `0x10`, `0x030057E0` `0x10`, the frame counters 0;
  - the debug console print `FUN_0815e9e8`. It writes RAM: the text console `0x030064B0` (via `FUN_0815e850`),
    with the free heap from `FUN_08160e74` (itoa `FUN_081633cc`);
  - with a route, the base palette copied into the second buffer and `apply_sector_light_to_palette`;
  - then the first `race_frame_update`.
- **IRQs in the state-4 frame:** they commute with `race_start` except for two reads.
  - The tick counter is the rand seed: record the IRQ count at `setup_race_cars`' read and pass
    `seed_vblanks = 0` after running them.
  - The music request (engine `+0x1478`): record the count at `carbon_play_music` and apply the request to the
    engine there.

  So the recorder needs a `marks` column: seed, music, `race_start_from_table_b` (phase 2 starts the race time
  mid-frame on the GO frame), the pause block entry and the store to `0x03005398`.
- **`Game` changes:**
  - an `io` field, because `race_start` and `obj_upload_tiles` read DISPCNT's 1-D bit;
  - the sprite bank rebuilt after `race_start`: `Game::new` must not assume a level descriptor at state 4.
- **`race_start_from_table_b`**, with fade 0:
  - in phases 1/9: an effect sprite, and `0x030000AC += 50000 / frame time`;
  - every `0x4000`: `0x03005714` += 1 (phase 2 at 3), or the next countdown digit's tiles uploaded;
  - at 3: the four tile uploads `FUN_0813a054`…`a0e0`, then 4.
- **Race end:**
  - phases 6–8 count `0x030000AC` down by the frame ticks, then `0x03005780 = 1` and fade `−0x10`;
  - phase 3 sets fade `+0x10`;
  - once over with no fade, `race_frame_update` returns 0.
- **The state-5 exit, in order:**
  - `FUN_08135f38` (stop sounds 0–3 and the music, `0x0300003C = −1`);
  - profile `+0x32C`;
  - `fill_results` (`finish_time_estimate`, the tie-break `FUN_0812e9e8`);
  - `FUN_081396c4` (heap frees, `oam_hide_range(0, 0x37)`, `FUN_081374b0`/`FUN_0813ffbc`);
  - `goto_screen(0xB)` or `menu_back` (in `menu.rs`);
  - state 1, `carbon_play_music(0)`.
- **Pause:** the START block writes `0x03005398`, clears the gradient and both pages, turns DISPSTAT's VCount IRQ off
  (`FUN_0813a4e0`), calls `hud_toggle(0)`, then `goto_screen(5)`.
- **The menu boundary:** menu frames and `goto_screen(0x82)` (resume) run in `menu.rs` on its `Gba`, where
  race-side calls (`race_menu_palette_setup`, `hud_toggle`, `restart_engine_sound`, the music, the palette copy and
  tint) go through `Gba::unported`. The loop should run state 1 through `menu.rs` and stop with `Unported` whenever
  it logs a call. `menu.rs` needs a hook so those race-side functions can run in place.
- **Traces to record:**
  - the 14 race-init starts, arming at state 4 (intro, countdown, first racing frames);
  - a circuit finish with laps set to 1 on the info screen (a legal setting, 1..6), driven by the autopilot from
    `tools/trace_race_rules.lua`;
  - a pause.

  `Trace` then has to stream its states: it holds every state in memory, about 400 KB each.
