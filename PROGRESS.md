# Progress log

Session state for whoever picks this up next. Read `AGENTS.md` first, then this file.

## Status (2026-09-29)

- **Task 1 (recon):** done. Redundant dumps deleted at the user's request, with hashes in `docs/DECISIONS.md`. Five zips and five vault ROMs remain.
- **Static pass:** done. Covered the text table, the LZ77 image bank (car atlases, bitmaps) and the corrected first look.
- **Tools:** all installed (see `docs/TOOLS.md`), except MSYS2, which is broken and only needed to build gbarecomp's runtime. That's not required; gbarecomp is reference-only.
- **Reference run works:**
  - `tools/mgba_ctl.py` drives the mGBA nightly (keys, screenshots, RAM dumps, savestates);
  - a Quick Play race is reached, with savestates `race.ss` and `mainmenu.ss` in `data/work/e5298b24/mgba/`;
  - the race uses video mode 4 plus an ARM software renderer in IWRAM.
- **Ghidra:** a headless analysis with the race IWRAM dump is done. The decompiled C of 874 functions is in `data/work/e5298b24/ghidra/carbon_decomp.c` (not in git). Rebuild it with the command in TOOLS.md.
- **First 3D geometry decoded:** the vehicle model bank has 102 models (cars in 3 LODs, spoilers, traffic). Details are in `docs/formats/vehicle-models.md`; `tools/models.py` exports OBJ.
- **City decoded:** a 2.5D portal/sector engine (Doom/Build style) with 1,113 sectors and 4,423 walls. Details are in `docs/formats/city-sectors.md`; `tools/city.py` exports SVG and OBJ. 12 tests pass.
- **Preview renders** (not in git): `data/out/previews/` holds the city top-down map, a 3D district, all vehicle models, and a car atlas.

## Next step

1. **Textures:** city materials (texels at `0x47BC6C`) and palettes; pair car models with their atlases and apply the UVs.
2. **Road surface:** the floor renderer (`FUN_03000b44`, `FUN_03002da0`).
3. **Roadmap step 2:** a minimal Bevy viewer that loads the city and cars straight from the ROM (untextured first).

## Environment notes

- Toolchains: Rust 1.98.1, Git 2.55.0, uv 0.12.20, Python 3.14.7 (pinned by `.python-version`; the global pyenv stays 3.12.10). The analysis venv is `.venv` (capstone, unicorn, numpy, pillow).
- In the Bash tool, `python` is a pyenv-win batch shim. **Multi-line `python -c` and `python - <<EOF` get mangled**, so write a script file into the scratchpad instead.
- The mGBA stable build from scoop creates `cheats/ patch/ savegame/ savestate/ screenshot/` in its current folder. The tools agent left such folders in the repo root. They are untracked, and the user was asked to delete them.
