# Typed state: conventions for the migration

**End state (reached for the race frame, 2026-09-29).** `Game` owns `data: Arc<GameData>`, the typed `World`
(`nfsgba-game/src/world.rs`: every entity with its car or traffic block, the sector lists as index lists, the car
globals, the camera, the matrix slots and effect sprites, the HUD, the loop's globals, the palettes, the sound
engine) and the outputs (palette RAM, VRAM, OAM, samples). `Game::frame` runs every subsystem on `World`: it builds
each subsystem's frame (`CarWorld`, `CameraFrame`, `Slots`, `HudFrame`) from `World` and writes it back; no `Mem`.
`World::load(&Machine)` reads a machine state (a trace, a dump, the race start's result); nothing stores it back.
Each global word lives once in `World` (the car globals own the words `HudVars`, `SlotGlobals` and `Race` share).
The integer maths stays as it is (`nfsgba-fixed`).

**The contract (2026-09-29, `docs/DECISIONS.md`):** the mechanics, physics and calculations are exact, not the bytes.
Typed state holds what the game logic uses; scratch, stale and unused bytes are left out. The replay test compares
the game's `World` with `World::load` of the next traced state (all of it but the heap arena, compared as the
racers' atlases, and the VCount IRQ's gradient pointer) and the outputs (palette RAM but entry 0, VRAM, OAM); so a
byte no typed field declares is not compared. The layout test round-trips every struct instance
(`tests/common/mod.rs`).

## Where things live

| What | Where |
|---|---|
| The mechanism: `layout!`, `Field` (`load(&Mem, at)`, `store(&self, &mut Mem, at)`), `Layout::FIELDS`, `Ptr<T>`, `assert_disjoint` | `nfsgba_sim::layout` |
| Every typed RAM struct and global block, declared once with its offsets | `nfsgba_sim::state` (split into `state/<area>.rs` when it grows) |
| `GameData` and `bn7e`, the table of every ROM offset typed code uses | `nfsgba_sim::data` |
| Subsystem logic on typed state: no `Mem`, no GBA address | its module, e.g. `nfsgba-game/src/camera.rs` |
| `World`, `World::load`, the subsystems' frames built from it (`with_cars`, `with_slots`, `camera_frame`, `hud_frame`) | `nfsgba-game/src/world.rs`; the renderer's and the viewer's readers in `view/` |
| The car world on a RAM image, for the sim's own RAM tests only | `nfsgba_sim::ram` (`carworld.rs` + `ram.rs`) |
| Round-trip and overlap tests over every replay-trace state | `nfsgba-game/tests/layout.rs`; `state.rs` tests |
| Typed replay (`World` against `World::load` of the next traced state, plus the outputs) | `nfsgba-game/tests/replay.rs` |

## Declaring state

- `layout! { pub struct Car: 0x4FC { 0x03C revs: i32, ... } }`. Offsets are literals. Field types: `u8`, `i8`,
  `u16`, `i16`, `u32`, `i32`, arrays, other layouts (`body: RigidBody`), `Ptr<T>`.
- Globals: size `0` and absolute addresses (`struct Camera: 0 { 0x0300_5F94 orbit: i32 }`), loaded with base 0.
- A type declared elsewhere gets its layout with `impl nfsgba_formats::render::Piece: 0x20 { ... }`. List every
  field; this reuses the formats type instead of adding a second one.
- Field names follow `physics.md` and `address-map.md`. An offset the code touches without a known meaning is
  `u_<offset>` (`u_4f4`).
- Declare what the logic reads or writes as game state. Leave out bytes that only hold scratch the game rewrites
  before reading, stale values, or nothing: the replay test then ignores them. Undeclared bytes are never touched by
  `store`.
- Each new struct goes into `fields_do_not_overlap` (`state.rs`) and into `instances` (`tests/common/mod.rs`) at
  every base where it lives; that one list feeds the layout round trip and the replay comparison.

## Pointers and ROM data

- `Ptr<T>` keeps the GBA address, so it round-trips unchanged. Typed code never follows a pointer. The adapter
  resolves it: `ptr.read(m)` to a value, `base.at(i)` or `p.index_from(base)` to an index, `ptr.read_n(m, n)` to a
  `Vec`. Example: `Race::player_entity` becomes the `Racer` the camera follows.
- In the end state a pointer becomes an index into a `World` vector (entity, car) or disappears when it always
  points at the same thing. For example, world `+0x54` always holds the camera matrix, so the adapter writes it.
- ROM addresses held in RAM (world `+0x10` walls, `+0x14` sectors) stay `u32`; `GameData` holds the data.
- `GameData` is parsed with the `nfsgba-formats` parsers. Extend them, as `Sector`/`Wall` were for the sector
  search; never write a second parser. The maths tables stay in the ROM image, so typed code that needs sine,
  atan or reciprocals also takes `rom: &[u8]`.

## Migrating one subsystem

1. List the RAM it reads and writes (its `m.` calls). Add the missing fields to `state.rs`.
2. Define its frame struct: inputs plus in/out state (`CameraFrame`), built from and written back to `World`.
3. Rewrite the logic on the frame with the existing Rust port as the reference: the same integer operations
   (`wrapping_*` exactly where the original has them) and the same results. The structure is free: indices instead
   of pointers, enums, loops over `Vec`s, named fields; drop what only served the GBA (heap bookkeeping, scratch
   copies, the order of invisible writes).
4. At the call site in `Game::frame`: build the frame from `World`, run, write it back. The replay tests stay green.
5. Shared helpers: one typed implementation. RAM-image callers reach it through an adapter (`world::geometry` runs
   the typed `Geometry` on `Mem`). A RAM-image twin may stay only while unmigrated callers need it, and it says so
   (`world::wall_flags`).
6. If a byte stops matching and nothing reads it, remove it from the struct (it is not game state); if something
   reads it, the rewrite is wrong.

**Done** means: no `Mem` and no address in the logic module; replay and layout tests green; `tools/gate.py` 7/7.

## Heap blocks, sector lists, pointers

A traffic car's block is `Slot::block` (allocated: `Some`, freed: `None`), a racer's physics struct `Slot::c`; the
sector lists are `World::heads` plus `Entity::next` (`carworld::{link_entity, unlink_entity}`, shared with the
sparks in `slots.rs`). Pointers to entities are `EntityRef` (an index; the layout converts). The heap allocator
and its node table are not modelled: `ram::with_world` (the sim's RAM tests) allocates or frees the blocks that
changed, and those tests leave heap bookkeeping out (`tests/trace.rs` `bookkeeping`).

## Order

Done:
- the race frame on `World` (camera, car step, AI, traffic, matrix slots and effects, HUD, renderer, sound);
- the menus: the top level and every screen with a handler in Rust (Kind7 map, Event, Career results, List, Setup,
  Kind38 hints, Intro; `nfsgba-game/src/menu/`, `state/menu.rs`); the typed code reaches the rest (scene setup,
  palettes, frame buffers, the text and blit primitives, game functions not ported) through the `Host` trait,
  `menu/adapt.rs`. Only the garage screens (Kind18, U3) and the exit handlers are unported calls.

Also done: the race start (`race_init::race_start` builds the `World` from a typed `Setup`, `race_setup.rs`: no RAM
image, G3 closed).

Still on RAM: the menus' scene/palette/VRAM helpers (`menu/mod.rs`, FIDELITY U7); the atlases' heap arena
`World::heap`, which the start fills as the game's heap does and the rim redraw reads around its buffer (R24; the live
cars are written into it first).
Next: the menus produce the `Setup` (G1c), then the arena.
