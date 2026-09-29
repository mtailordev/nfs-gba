# Progress log

Session state for whoever picks this up next. Read `AGENTS.md` first, then this file.

## Status (2026-09-29)

- **Task 1 (recon):** done and verified. Committed in `bca0b39`.
- **Redundant dumps deleted** at the user's request: the second Carbon zip, and the Porsche Unleashed USA zip plus its vault ROM. Hashes are in `docs/DECISIONS.md`. Five zips and five vault ROMs remain (`data\vault\roms\`); the ROMs exist extracted only in the vault.
- **Static pass:** done and committed (`b4c5768`).
  - `0x7E86A0` is the text table (977 keys × En/Fr/De/It/Es) plus the music-module list, not an asset directory.
  - BIOS LZ77 packs 294 images: full-screen bitmaps, 45 car texture atlases (256×200) and textures.
  - The early "geometry at `0x000000`" guess was PCM audio.
  - **The 3D geometry is not found yet.**
  - Write-ups: `docs/formats/text-table.md`, `docs/formats/lz77-images.md`, and the corrected `docs/recon/FIRST-LOOK.md`.
- **Tool install:** a background agent is installing the toolchain (mGBA, Ghidra plus a GBA loader, a JDK, the Arm GNU toolchain, no$gba, GBATEK, gba-recomp, gbarecomp, the MSVC build tools, and a Python `.venv` with capstone/unicorn/numpy/pillow). It writes `docs/TOOLS.md`. **Check that file.** If it is missing or incomplete, the install didn't finish.
- `python -m unittest discover -s tools` gives 10 tests, all passing. Both tools are rerun-safe.

## Next step

Find the geometry through the renderer:
- run Carbon in mGBA with a Lua script that logs ROM reads during a race;
- and/or load the ROM in Ghidra (ARM:LE:32:v4t, base `0x08000000`) and follow the ARM code at `0x350000–0x368000` plus its callers.

Then write the car-mesh format doc and a minimal Bevy viewer (roadmap step 2).

## Environment notes

- Toolchains: Rust 1.98.1 (`~\.cargo\bin`, on the user PATH in new terminals), Git 2.55.0, uv 0.12.20, and Python 3.14.7 pinned by `.python-version`. The global pyenv Python is 3.12.10 on purpose.
- In the Bash tool, `python` is a pyenv-win batch shim. **Multi-line `python -c "..."` gets mangled**, so write a script file (heredoc into the scratchpad) instead.
- The user approved killing old `sh.exe` trees from the `wardogs-re-pipeline-V6-REWORK` session earlier (ports 8041, 8043, 8097). They are no longer running.
