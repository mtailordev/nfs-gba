# Progress log

Session state for whoever picks this up next. Read `AGENTS.md` first, then this file.

## Status (2026-09-29)

Done and committed:
- **Task 1 (recon):** the vault holds five ROMs, one per game, and the redundant dumps are deleted (hashes in `docs/DECISIONS.md`). The first-look reports are in `docs/recon/`.
- **Tools:** all installed (`docs/TOOLS.md`). MSYS2 is broken but not needed.
- **Reference run:** `tools/mgba_ctl.py` and `tools/mgba_remote.lua` drive the mGBA nightly (keys, screenshots, RAM dumps, savestates). Savestates `mainmenu.ss` and `race.ss` are in `data/work/e5298b24/mgba/`. The route into a race is in TOOLS.md.
- **Ghidra:** headless analysis with the race's IWRAM loaded (`tools/ghidra/*.java`). Decompiled C of 874 functions is in `data/work/e5298b24/ghidra/carbon_decomp.c` (not in git).
- **Formats decoded and verified** (details in `docs/formats/`):
  - text table: 977 keys × 5 languages;
  - LZ77 image bank;
  - vehicle model bank: 102 models, the car table (15 cars, names, LOD triplets, atlases), vehicle textures and UVs;
  - city: portal/sector world; column-mapped wall textures; exact wall and floor UVs; floors, ceilings and material 0; 12 skies;
  - world scale: one unit for cars and city, about 48 per metre.
- **Rust workspace:**
  - `crates/nfsgba-formats` has 21 real-data tests (modules `paint`, `sky` and `career` from the agents);
  - `crates/nfsgba-viewer` (Bevy 0.19.1) renders GBA-style indexed colour with the exact per-frame light tint, the textured city, the skies (K cycles them; default environment 11 = the reference race) and a showroom of all cars in every paint variant.
  - Run it with `cargo run --release -p nfsgba-viewer`. `NFSGBA_CAM` and `NFSGBA_SHOT` give scripted screenshots.
  - clippy and rustfmt are clean (`rustfmt.toml`: max width 120).
- **Tests:** the Python tools have 12 tests.
- **Previews** (not in git): `data/out/previews/`. `viewer-race-camera.png` renders from the game's own chase camera and matches the in-game screenshot `data/work/e5298b24/mgba/s15.png`.

## Working rules (from the user)

- **Absolute 1:1 rewrite, no compromise.** Every approximation is marked `NOT 1:1` in code and listed in `docs/FIDELITY.md`, with the game function that holds the exact behaviour. Close entries only after checking them against the reference build.
- **Never find anything twice.** Every ROM offset, RAM address or function goes into `docs/engine/address-map.md` and `docs/engine/symbols.csv` (applied to Ghidra by `tools/ghidra/ApplySymbols.java`).

## Next steps (roadmap)

Done since the last update: race routes (grid plus racing line, `docs/formats/race-routes.md`); environments (the 12 level descriptors pick palette and sky; K cycles them in the viewer); the in-race light tint mechanism (`FUN_0813a514`).

**Parallel agents (started 2026-09-29, each in its own git worktree/branch; the parent merges):**
- **viewer-indexed:** done and merged. R1/R2 closed (`docs/engine/viewer-rendering.md`).
- **car-paint:** done and merged. Car palette slots 160–255 exact in `paint.rs`; R4 closed; the viewer part of R3 is open (`docs/formats/car-paint.md`).
- **sky:** done and merged. Gradient and skyline exact in `sky.rs`; the viewer parts of R5/R6 are open (`docs/engine/sky.md`). It found that the reference race is **environment 11**, not 1.
- **sector-renderer:** R7–R11. Owns `crates/nfsgba-formats/src/render.rs`, `docs/engine/renderer.md`.
- **vehicle-physics:** car simulation plus trace harness. Owns `crates/nfsgba-sim`, `docs/engine/physics.md`, `tools/trace_*`.
- **audio:** done and merged. `crates/nfsgba-audio` reproduces LS_Play bit for bit (13,800 traced frames); `nfsgba-audio-render` writes WAVs to `data/out/audio` (`docs/formats/audio.md`). Hook: `Engine::vblank` once per frame, gameplay calls the `carbon_*` functions.
- **ui-2d:** HUD, fonts, menus, sprites and the raw 8bpp region. Owns `crates/nfsgba-formats/src/ui.rs`, `docs/formats/ui.md`, `tools/ui_*`.
- **career-events:** done and merged. Save format, career tables, unlocks and Quick Play setup exact (`career.rs`, `docs/formats/career.md`); race rules transcribed (D5). Its racing-line sections closed D1 (`routes()` now returns the exact lap and branches).
- **viewer-sky-paint** (started after the first merges): viewer parts of R3/R5/R6. Owns `crates/nfsgba-viewer`, a new section of `docs/engine/viewer-rendering.md`.
- **car-atlas:** R13, the player's decals and overlays, and how opponents' cars, paints and materials are chosen. Owns `paint.rs` (+ `atlas.rs`), `docs/formats/car-paint.md`.
- **race-rules:** D5–D7: trace-check the race rules, find the lap-arming code and hunter life at zero, write the exact save encoder. Owns `career.rs`, `docs/formats/career.md`.

Each writes "Integration notes" (address-map rows, symbols rows, FIDELITY changes) for the parent to merge into the central docs. Each emulator session uses `NFSGBA_MGBA_SESSION=<agent>`. Ghidra: the agents read `carbon_decomp.c`, or work on a private copy of the project.

Next, driven by `docs/FIDELITY.md`:
1. **Viewer integration of R3/R5/R6:** one race palette from `paint::race_palette`; the backdrop per screen line and the skyline layer from `sky`.
2. **R7–R11, R14–R16:** from the sector-renderer agent (portal traversal, step walls, draw limits, projection, index-0 pairs, half-resolution floors).
3. **R13:** decals and overlays on the player's atlas; opponent material choice.
4. **Gameplay parity (roadmap step 4):** handling, AI and cops, traced against the reference build.

## Environment notes

- Toolchains: Rust 1.98.1 (with clippy and rustfmt), Git 2.55.0, uv 0.12.20, Python 3.14.7 pinned by `.python-version` (the global pyenv stays 3.12.10). The analysis venv is `.venv`.
- `~/.cargo/bin` may be missing from PATH in the tool shells: prefix `export PATH="/c/Users/cyntrex/.cargo/bin:$PATH"`.
- Agents: never kill `mGBA.exe` by image name (one agent did, ending the others' sessions); stop your own PID with `mgba_ctl.py stop`.
- In the Bash tool, `python` is a pyenv-win batch shim. **Multi-line `python -c` and `python - <<EOF` get mangled or hang**, so write a script file into the scratchpad instead.
- In PowerShell, `@(118039/48, -30/48)` fails to parse; pass precomputed numbers.
- The mGBA stable build from scoop creates `cheats/ patch/ savegame/ savestate/ screenshot/` in its current folder. The tools agent left such folders in the repo root. They are untracked, and the user was asked to delete them.
