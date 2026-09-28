# Progress log

Session state for whoever picks this up next. Read `AGENTS.md` first, then this file.

## Status (2026-09-29)

- **Task 1 (recon) done and verified.** Waiting on the user to pick the next step.
- Deliverables: `docs/recon/ROM-INVENTORY.md`, `$NFSGBA_DATA/vault/manifest.json`, `docs/recon/FIRST-LOOK.md` (plus the generated `FIRST-LOOK-DATA.md`).
- The vault is at `E:\Games\rewrites\nfs_gba\data\vault\roms\`: 6 ROMs, all read-only, all SHA-1-verified against the manifest.
- The zips in `dumps/` were SHA-1-checked before and after: unchanged.
- Tools: `tools/vault.py` and `tools/first_look.py` are rerun-safe (verified: a second run touches no file). `python -m unittest discover -s tools` gives 9 tests, all passing.
- Git: repo initialized on `main`, hook enabled via `core.hooksPath`. **Nothing committed yet** (waiting for the user's go).

## Key results

- **Canonical target:** `BN7E_v0_e5298b24.gba` (SHA-1 `e5298b2482a769aa6458955cd6b18db3ac3f3a20`). Both Carbon zips hold this one ROM; region `E` = USA+Europe.
- **Porsche Unleashed EU/USA:** the same build, with 3 bytes different (header plus one Thumb immediate).
- **Engine:** one Pocketeers lineage in two generations: (Porsche Unleashed, Underground) and (Underground 2, Most Wanted, Carbon). Most Wanted is the best back-check sibling.
- **Carbon:** about 5–6% code (Thumb `0x128000–0x164000`, ARM `0x164000–0x16C000` and `0x350000–0x368000`). Almost no BIOS compression. Audio is Logik State `LS_Play`, not MP2K.
- **Best lead:** the 5,867-entry pointer table at `0x7E86A0` (probable asset directory).

## Recommended next step (not started)

1. Commit the scaffold, docs and tools.
2. Roadmap step 1, the reference build: fetch mGBA (Lua scripting, GDB stub) and Ghidra with a GBA loader into `ext/` after a licence check.
3. In parallel, a static pass on the `0x7E86A0` table: entry sizes, what the targets look like, and whether the geometry hypothesis at `0x000000–0x0A0000` fits. That is the path to the model viewer.

## Environment notes

- Toolchains: Rust 1.98.1 (`~\.cargo\bin`, now on the user PATH in new terminals), Git 2.55.0, uv 0.12.20, and Python 3.14.7 pinned by `.python-version`. The global pyenv Python is 3.12.10 on purpose.
- In the Bash tool, `python` is a pyenv-win batch shim. **Multi-line `python -c "..."` gets mangled**, so write a script file (heredoc into the scratchpad) instead.
- To unlock the Git upgrade, the user approved killing 8 old `sh.exe` trees from the `wardogs-re-pipeline-V6-REWORK` session (servers on ports 8041, 8043 and 8097). They are no longer running.
