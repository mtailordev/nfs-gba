# Typed state: conventions for the migration

**End state.** `Game` owns `data: Arc<GameData>` (the ROM tables, parsed once) and a typed `World` (entities,
cars, camera, race, HUD, menus). Subsystems are functions on typed state. The GBA RAM image (`Mem`) lives only in
tests, loaded and stored through the layouts to compare with traces. The integer maths stays as it is
(`nfsgba-fixed`). The camera (`nfsgba-game/src/camera.rs`) is the pilot.

## Where things live

| What | Where |
|---|---|
| The mechanism: `layout!`, `Field` (`load(&Mem, at)`, `store(&self, &mut Mem, at)`), `Layout::FIELDS`, `Ptr<T>`, `assert_disjoint` | `nfsgba_sim::layout` |
| Every typed RAM struct and global block, declared once with its offsets | `nfsgba_sim::state` (split into `state/<area>.rs` when it grows) |
| `GameData` and `bn7e`, the table of every ROM offset typed code uses | `nfsgba_sim::data` |
| Subsystem logic on typed state: no `Mem`, no GBA address | its module, e.g. `nfsgba-game/src/camera.rs` |
| Adapters: `<area>_frame(&Mem)` and `store_<area>_frame(&mut Mem, &Frame)` | `nfsgba-game/src/view.rs` |
| Round-trip and overlap tests over every replay-trace state | `nfsgba-game/tests/layout.rs`; `state.rs` tests |

## Declaring state

- `layout! { pub struct Car: 0x4FC { 0x03C revs: i32, ... } }`. Offsets are literals. Field types: `u8`, `i8`,
  `u16`, `i16`, `u32`, `i32`, arrays, other layouts (`body: RigidBody`), `Ptr<T>`.
- Globals: size `0` and absolute addresses (`struct Camera: 0 { 0x0300_5F94 orbit: i32 }`), loaded with base 0.
- A type declared elsewhere gets its layout with `impl nfsgba_formats::render::Piece: 0x20 { ... }`. List every
  field; this reuses the formats type instead of adding a second one.
- Field names follow `physics.md` and `address-map.md`. An offset the code touches without a known meaning is
  `u_<offset>` (`u_4f4`).
- Every byte the ported code reads or writes must be a field. Undeclared bytes are never touched by `store`.
- Each new struct goes into `fields_do_not_overlap` (`state.rs`) and into `round_trip` (`tests/layout.rs`) at every
  base where it lives.

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
2. Define its frame struct: inputs plus in/out state (`CameraFrame`). Add the load and store adapters in `view.rs`.
3. Port the logic line by line onto the frame: the same integer operations (`wrapping_*` exactly where the
   original has them) and the same read/write order. RAM the game leaves behind that the traces see (the sector
   search's query point) becomes an output of the frame.
4. At the call site in `Game::frame`: load, run, store. The replay tests stay unchanged and green.
5. Shared helpers: one typed implementation. RAM-image callers reach it through an adapter (`world::geometry` runs
   the typed `Geometry` on `Mem`). A RAM-image twin may stay only while unmigrated callers need it, and it says so
   (`world::wall_flags`).

**Done** means: no `Mem` and no address in the logic module; replay and layout tests green; `tools/gate.py` 7/7.
When a whole frame is typed, the per-subsystem adapters merge into `World::load` / `World::store` (tests only),
and `Game` holds the `World`.

## Order after the camera

Car step and its helpers (walls, contacts, route), then AI and traffic, then the matrix slots and effects
(`slots.rs`), then `race_init` (it builds the whole `World`), then the rest of the HUD, then the menus.
