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
  - `crates/nfsgba-formats` has 4 real-data tests;
  - `crates/nfsgba-viewer` (Bevy 0.19.1) shows the textured city, the skies (K cycles them) and a showroom of all cars in every paint variant.
  - Run it with `cargo run --release -p nfsgba-viewer`. `NFSGBA_CAM` and `NFSGBA_SHOT` give scripted screenshots.
  - clippy and rustfmt are clean (`rustfmt.toml`: max width 120).
- **Tests:** the Python tools have 12 tests.
- **Previews** (not in git): `data/out/previews/`. `viewer-race-camera.png` renders from the game's own chase camera and matches the in-game screenshot `data/work/e5298b24/mgba/s15.png`.

## Next steps (roadmap)

1. **Race data:** decode the route table at `0x7F2798` (44 × 0x14) and the level-descriptor event fields, to get start positions, checkpoints and AI lines. Put a car on the start line in the viewer.
2. **Runtime palette:** find the time-of-day/fog palette transform and which sky each event uses, to match in-game colours.
3. **Portal step walls:** the upper and lower wall parts between sectors of different heights.
4. **Gameplay parity (roadmap step 4):** handling, AI and cops, checked against traces from the reference build (mGBA plus Ghidra).

## Environment notes

- Toolchains: Rust 1.98.1 (with clippy and rustfmt), Git 2.55.0, uv 0.12.20, Python 3.14.7 pinned by `.python-version` (the global pyenv stays 3.12.10). The analysis venv is `.venv`.
- In the Bash tool, `python` is a pyenv-win batch shim. **Multi-line `python -c` and `python - <<EOF` get mangled or hang**, so write a script file into the scratchpad instead.
- In PowerShell, `@(118039/48, -30/48)` fails to parse; pass precomputed numbers.
- The mGBA stable build from scoop creates `cheats/ patch/ savegame/ savestate/ screenshot/` in its current folder. The tools agent left such folders in the repo root. They are untracked, and the user was asked to delete them.
