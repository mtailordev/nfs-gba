# Opponents and traffic (entity handlers 0x29 and 0x36)

The computer-driven cars of a race, ported exactly to `crates/nfsgba-sim` (`ai.rs`, `traffic_ai.rs`) and checked
call by call against the game's own code over twelve traced races. ROM `BN7E` v0, SHA-1 `e5298b24…`.

**Status: exact and verified.**
- **Checked:** 12,376 opponent and traffic handler calls write exactly the RAM bytes the game's code writes, and make the same sound and sprite calls.
- **Replayed:** 12,372 car states reproduce the next traced frame, with the cars running on their own state.
- **Stops:** 96 calls stop at car-physics code that is not ported yet: tipped-over dynamics and the car-to-car response (FIDELITY D9, D10).
- **One input comes from outside the frame:** the race time the VBlank IRQ has counted by the moment the AI runs ([Timing](#timing-not-11)).

## The entity loop

`update_entities` (`FUN_0813765c`, called with start 0) walks entities 0..35 (world `+0xF8` + `+0xFA`) and calls the
handler table `0x087F38B8` by entity `+0x4E` for every entity with flags `+0x08 & 3 == 3`:

| Index | Handler | Entities |
|---|---|---|
| 0..3 | `car_handler` `0x0814BD4C` | the player (`docs/engine/physics.md`) |
| 0x29 | `opponent_handler` `0x0814A2A0` | opponents and the wingman |
| 0x34 | `0x0814C49C` | effect entities (spawned in pairs, e.g. at the start; not ported) |
| 0x36 | `traffic_handler` `0x081443FC` | traffic cars (spawned by `traffic::spawn`) |
| 0x39..0x3F, 0x40 | `camera_update`, `camera_look_at_player` | cameras |

The racers are entities 0..`*0x030057EC`; a racer whose index is above the opponent count `0x03005784` is a
*non-racer* (the wingman).

## Opponents (`ai.rs`)

**Handler** (`FUN_0814A2A0`):
1. Read "visible" before anything else: entity `+0x08` bit 2 and `+0x0A` bit 2.
2. **State 0:** run the setup.
3. **States 1 and 2** (an unsigned `state − 1 < 2`):
   - unlink from the sector list;
   - in race phases other than 0, 1, 4 and 9, drive (`FUN_0814D078`);
   - relink;
   - clear `+0x0A` bit 0 (for the player the game also redraws its rims, which is rendering).
4. **When visible:** call `opponent_effects` (`FUN_0814E628`). It places the car's rear light and exhaust sprites in the 2D layer (sprite pool `*0x03000058`). The port returns it as `ai::Effects` (entity, heading, `0x3FFF − *0x03005F9C`, size 0x20 or 0x40) instead of drawing.

**Setup** (`FUN_0814A390`):
- **Physics struct and fields:** allocate the physics struct. Set the start position from the route's template entity (after `place_on_floor` at the stale position), the heading, and the inertia terms. Set laps, a random lane timer (`rand >> 7 & 0xF`), a random hunter countdown, and grip scale 0x100.
- **Start lane:** 0..4 from the car's offset across the route (`FUN_0814059C`); mode 0xF or 0xE forces 2.
- **Wheel heights:** the floor heights under four points around the car seed the suspension state (`FUN_0814DD24`).
- **Upgrades:**
  - a racer copies the player's upgrades, or `AI_skill / 10 − 1` in career;
  - a non-racer (the wingman) gets **one** word of `0x7F42B4` in all ten slots (the game's copy loop never advances its source).
- **Handling and speed curve:** nitro; handling; the speed curve (below); handling again.
- **Wingman globals** (`FUN_081410C0`).
- **Grips and settling:** grips × 1.25, then `race_start_setup` (20 settling steps).

**Speed curve** (`FUN_0813C394`): for 20 wheel speeds up to `max rpm · 0x180 / top-gear ratio`, the game runs the
automatic gearbox 7 times (race phase set to 1, then left at 9). It stores `(torque · 0x9999 · ratio · torque scale
− rpm · 16) >> 8 · 0x90` in the profile at `+0x27C`, a curve of 0x15 points (`+0x26C` count, `+0x274` last x).

**Steering** (`FUN_0814D078`):
1. **Nitro drain.**
2. **Stuck recovery:**
   - 0x32 stuck steps (`+0x4B0`) re-sync the route segment from the sector (`FUN_0813F530`);
   - over 0x96 (or 0xFA) the car is put back on the road (`FUN_0814EFA8`, the physics port's).
3. **Look-ahead distance:** speed − 200, capped by a per-lane distance `0x7F5A50` in sharp curves, or 0x708 near traffic; at least 0x640.
4. **Advance along the line:** past the next waypoint's crossing plane (`+0x444` > 0) it moves on to the next waypoint, or through a branch link at the section's second-to-last point. It arms the lap on segments 1–7. Before a fork a non-leading racer takes the shortcut when `rand & 0xFF` beats `0x7BFCD4[difficulty]` (255 / 192 / 100) and the shortcut's section was visited (`0x030060C0`). Then `lap_crossing` runs.
5. **Target point:** the point of the current segment nearest the car (`FUN_0815FE5C`), moved ahead by the look-ahead along the segment's direction. Past the segment's end it carries over onto the next segment, or snaps to the waypoint after it. The target is then shifted sideways by `(lane − 2) · 0x100` (0x200 when `*0x030056F0 < 0x32`). Kept quirk: the sideways direction uses the segment the target ended on.
6. **Outputs:** the target heading is `+0x94`; the angle between the next two segments `× 0x14 >> 4` is `+0x98`.
7. **Following instead:** while `+0x4F0` is set and the entity at `+0x4F4` is ahead, the car aims straight at that entity.

**Driving** (`FUN_0813C5A8`), per step:
1. **Grip and heading:** grip scale; the heading from the body matrix; brake if behind the entity it follows.
2. **Steering:** `(angle to +0x94) << 6 >> 8`, clamped to ±0x1000, × 0x15E. A non-racer adds `wingman_steer`.
3. **Curve braking:** momentum is scrubbed and the brake set in sharp curves, depending on the speed and `+0x98`.
4. **Lane changes:**
   - a lane is blocked (`+0x4DA` + 2·lane) by traffic just ahead, by racers close ahead (after 0xB4 race frames), or by the timer passing 300;
   - a blocked current lane moves the car ±1, ±2, ±3 lanes, starting on a random side;
   - the timer then restarts at `race time & 0x1F`.
5. **Throttle:**
   - 0x9999, or 0x2666 when slow and turning hard;
   - **boost** on straights (`0x03006158`, timers `0x7F3DE8`/`0x7F3DF4`);
   - **catch-up** (`0x03000050`): the lead over the player scales the throttle by up to +7.4% when the car is behind the player, or −27% when it is ahead;
   - a non-racer uses `wingman_throttle` instead.
6. **Wheel spin and gravity:** the throttle is scaled by the speed curve at the wheel speed, then drives each wheel's spin by `+0x188 + *0x03006110`, and the brake cuts it. Then the sector and gravity.
7. **Contact and integration:**
   - its **own wheel contact** (`FUN_081489DC`): every wheel stands on the car's own sector, and the front wheels turn by `+0x20 >> 8`;
   - the wall test **without** portal recursion (`walls::walls(.., false)`);
   - **one** integration of `2·dt`;
   - the sector again (the push-back loop needs `find_sector_far`);
   - the car-to-car test and the race progress.

**The wingman** (`FUN_08140CF4`, non-racer throttle):
- **Normal:** it follows the player (`*0x0300619C`) into side segments and paces itself on the player's lead with a PD-like gain (`now·0x2B − prev·0x2A + bias`, clamped). Near the player it keeps out of the player's lane and its neighbours.
- **Commands** (R+L, `0x030061E8`):
  - the **attacker** (`0x030061F8 = 0`) closes on the car at `*0x03006178`, then runs at full throttle for 6 steps;
  - the **drafter** (1) runs just ahead of the player in its lane and refills the player's nitro tank by 0x2AAA per step, up to 0x50000.
- The wingman's globals are listed in `docs/engine/notes/addresses.ai-traffic.csv`.

## Traffic (`traffic_ai.rs`)

`FUN_081443FC`, only while traffic is on (`0x03006298`):
- **State 0:** onto the floor; state 1.
- **State 1 (driving):**
  - **Speed and removal:** speed 3 (4 every `*0x0300624C` steps, up to `*0x03006290`). The car is removed (state 2) when it is far from the camera's car (`*0x030057F8`).
  - **Turning:** a turn towards the next direction runs over 0x20 steps (0x10 in mode 2), interpolating the direction and its `atan2_fast` heading. The shown heading eases towards the travel heading, or spins with the wobble `+0x38`.
  - **Mode 1** (lanes): near the target point it moves to the next waypoint in its direction (a sprint's ends knock it away, state 3), offset into its lane.
  - **Sector, floor and pitch:** its own sector search (`FUN_08144B7C`: dynamic walls never count as solid, solid walls pass with material 0). The floor, and a pitch from the floor one turn-step ahead.
  - **Racer test** (`FUN_08146094`): swept centre tests, then the traffic type's two points against the racer's two axle points.
- **The hit** (`FUN_08145DAC`):
  - an impulse along the line between the centres, with restitution clamped to −0x16..−0x11;
  - it is split by the traffic type's shifts (`0x7F5604` for the traffic car's velocity and wobble, `0x7F5684` for the racer's momentum and spin);
  - side effects: the racer's lane; hunter life; the crash sounds for the player; race phase 7 for some types (`0x7F5924`);
  - the traffic car goes to state 3.
- **State 3 (knocked away):** it slides with friction 1/32 and bounces off solid walls within 100 units (`FUN_0814658C`: restitution 0x13/0x400). It keeps its wobble and is tested against the racers again. It is removed after its timer while unseen, or when far.
- **State 2:** free the block, unlink, one fewer car, free its slot.

## Timing (NOT 1:1)

`vblank_irq` counts the race time `0x03005800` every video frame; a game frame spans about 4. The lane-change timer
restarts at `race time & 0x1F` read **when the AI runs**, so it can be 1–3 more than at the frame's start. It
depends on CPU cycles, which the rewrite does not model.

In the traces this changed the result 38 times in 12,372 car states. The replay test takes the trace's value there
and counts it. The per-call test is exact, because the oracle sees the same RAM.

## Verification

- **Traces:**
  - the nine player-car traces of `docs/engine/physics.md` (session `vehicle-physics`);
  - three recorded here with `tools/record.py ai` (session `ai-traffic`), from race-info savestates made through Quick Play > CUSTOM:
    - `sprint`: JUNKPOINT, normal difficulty, heavy traffic, catch-up on;
    - `circuit`: LONGPOINT, 2 laps, hard difficulty, heavy traffic, catch-up on;
    - `wingman`: JUNKTOWN BLITZ with KITA. Wingmen were made selectable with `poke 0x02000C5A 0xFF`, a new `tools/mgba_remote.lua` command that sets RAM, never the ROM.

  The scenarios cover the race start, launches and overtaking, lane changes, shortcuts, stuck opponents, traffic spawning and removal, a racer hitting a traffic car, knocked-away traffic, catch-up, boost and the wingman.
- **Oracle** (`tools/oracle/cases.py ai`, on the harness's `tools/oracle`): for every traced frame it runs the game's own entity loop on the frame's RAM, chaining the calls. It records each call's RAM writes and stubbed calls (sounds, the rim blit, `opponent_effects` with its arguments) in `<name>.ai-oracle.txt`. It checks every opponent and traffic car against the next traced frame: 12,425 of 12,468 match, and all 43 mismatches are the timing field above.
- **Rust** (`crates/nfsgba-sim/tests/ai_trace.rs`):
  - `each_call_matches_the_game`: each frame replays the loop; the player's and the effect entities' calls apply the game's writes. Every opponent and traffic call runs the port, which must write exactly the same bytes and make the same calls. Only the car-physics stops listed there may stop.
  - `replay_matches_the_trace`: the opponents and traffic carry their own entity and data block from frame to frame over the reference RAM. They must reproduce every next traced frame (external bits masked, as in the physics tests). A car that reaches unported code takes the game's result for that call and re-syncs from the trace.
- **Not traced:**
  - the hunter race mode (the AI stops at `Unported` there);
  - mode 0xE/0xF setups;
  - the push-back loop;
  - traffic without a route.

## Not ported

Each stops with `Unported`:
- `car_put_back_on_road` (`FUN_0814EFA8`) for a stuck opponent, D9;
- tipped-over dynamics, D9;
- the car-to-car response, D10;
- the sector search two portals away and the push-back loop, D11;
- in hunter races: `FUN_0813FDB0` and `hunter_life_tick`;
- a traffic car without a route.

`opponent_effects` is rendering and is reported, not run.

## Relation to `career::RacingLine`

The race-rules port has a model of the same advance (`RacingLine::ai_advance`), built from the ROM and checked on its
own. The simulation instead runs on the game's RAM, where the race-time line already exists as the game built it
(rebuilt links, sprint end points, re-measured distances, planes at `*0x03005FB4`). So the port reads the line there
and writes the same bytes the game writes.

Calling `ai_advance` would mean copying the racer, the race, the planes and the back table out of RAM and back each
call. That is more code than the 40-line transcription, and it could get the write order wrong (the RNG index, flags).
Both versions are checked against the game's code, so they agree.
