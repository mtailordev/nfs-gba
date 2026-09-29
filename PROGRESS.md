# Progress log

Session state for whoever picks this up next. Read `AGENTS.md` first, then this file, then `docs/DECISIONS.md` (the newest entries set the contract) and `docs/FIDELITY.md` (every open deviation).

## Status (2026-09-29, after the project review)

**Phase: consolidation.** No new subsystems until it is done (user decision, `docs/DECISIONS.md`).

What exists and is exact (details and evidence in `docs/FIDELITY.md` "Closed"):
- **Data:** every asset family decoded from the ROM (text, images, models, city, routes, audio, career tables, fonts, HUD and menu layouts); 98.9% of the ROM's bytes attributed (`tools/rom_attribution.py`).
- **Crates:**
  - `nfsgba-formats`: parsers, plus rendering (`render`, `sky`, `paint`, `atlas`), HUD (`hud`, `ui`), menus (`menu`), career and race rules (`career`);
  - `nfsgba-audio`: the LS_Play engine, bit-exact;
  - `nfsgba-sim`: the player car, the opponents, the wingman and traffic, with every car-step path ported;
  - `nfsgba-game`: the race start (`race_init`) and the race frame loop (`Game::frame`), exact over five recorded runs (2,445 frames) with nothing stood in;
  - `nfsgba-viewer` (Bevy 0.19.1): high resolution, a 240×160 reference mode, and play mode (`NFSGBA_PLAY=1 NFSGBA_DUMP=game-loop/s18`).
- **Not yet:** boot → menus → race in our code (the menus port has logic but no drawing: U3, U7); the garage screens (Kind18); coverage of the 415 functions never reached; the high-resolution view agrees with the reference frame only 49% exactly (R27).
- **Known structural debt (the independent audit, 2026-09-29):**
  - 52% of the Rust keeps state in a GBA-layout RAM image;
  - duplicated helpers and subsystems (camera ×2, racing line ×2, decoder ×2, `rand_table` ×3, maths helpers ×3);
  - 71 tests pass silently without their data;
  - the 1.4 GB of fixtures have no provenance and sit in per-agent folders;
  - ROM data is read by hard-coded BN7E offsets;
  - several Closed claims have no test.

Run everything: `cargo test --release --workspace` (about 3 minutes; the AI trace test alone takes about 70 s), `cargo clippy --release --workspace --all-targets`, `cargo fmt --all --check`, and the Python tools' tests (`cd tools && ../.venv/Scripts/python.exe -m unittest discover -p "test_*.py"`).

## Working rules (from the user)

- **The contract (binding, `docs/DECISIONS.md`):** the core (assets, mechanics, rules, AI, physics, audio, save, the 240×160 reference frame) is exact; presentation adapts (any resolution; any framerate by interpolating between simulation steps). Byte-equal RAM is a test oracle, not the contract. Frame timing is a deterministic input.
- **No game code at runtime, no emulator.** mGBA and unicorn are test oracles only. New code uses typed state; RAM images live only in tests.
- **Every deviation is tracked.** `NOT 1:1 (ID)` in code, the matching row in `docs/FIDELITY.md`; an entry is closed only with a test in the repo that checks it.
- **Never find anything twice.** Every ROM offset, RAM address and function goes into `docs/engine/address-map.md` and `docs/engine/symbols.csv` (applied to Ghidra by `tools/ghidra/ApplySymbols.java`).
- **Oracle first:** port a function against `tools/oracle` on generated inputs over snapshots; use mGBA only for new snapshots and whole-frame traces.

## Next steps: the consolidation phase

1. **Test kit.** Build one shared test crate:
   - one `rom()` and data loader, and `NFSGBA_REQUIRE_DATA=1` so missing data fails instead of skipping;
   - a fixture manifest with provenance (sha1, ROM hash, recorder, command, savestate);
   - a scenario library replacing the per-agent folders;
   - exact stop lists and frame counts in the replay tests;
   - the out-of-workspace oracle checker folded in.
2. **One copy of everything:**
   - a fixed-point maths crate (`div`, `recip`, sine/atan, `isqrt`, `rand_table`, `angle_diff`);
   - one camera (drop the viewer's `Chase`), one racing line, one decoder;
   - one mGBA recorder with probe modules, and one oracle case CLI;
   - one way to load extra emulator scripts.
3. **Typed `World`:** move each subsystem to typed state behind the replay tests (car, AI and traffic, camera and slots, HUD, `race_init`, menus); ROM data parsed once into `GameData`, with the BN7E offsets in one layout table.
4. **Ledgers gated:**
   - every Closed line names its test;
   - a checker for `NOT 1:1 (ID)` markers against open IDs;
   - "Integration notes" sections and merged notes CSVs removed;
   - a merge gate script (tests with required data, `notes_merge`, fmt, clippy).

Then: boot → menus → race at 240×160 (a playable reference), the timing model, broader coverage (every car, route and mode, the garage, a career win, cops if any), the high-resolution renderer on typed state with automated comparisons, and link play and extras.

## How agents are run (lessons from the first 25)

- Infrastructure first, one at a time; fan out only on top of it.
- Size tasks to about 100 turns, each against a named acceptance test. Agents that ran into the 200-turn limit lost time.
- Agents edit the ledger rows they own directly, and `tools/notes_merge.py --write` merges their `docs/engine/notes/*.csv`. Don't leave "Integration notes" sections behind.
- Worktrees share one build directory (`CARGO_TARGET_DIR`) and are removed after their merge.
- Every emulator session uses `NFSGBA_MGBA_SESSION=<agent>` and is stopped by its own PID, never by image name.

## Environment notes

- Toolchains: Rust 1.98.1 (clippy, rustfmt), Git 2.55.0, uv 0.12.20, Python 3.14.7 pinned by `.python-version` (the global pyenv stays 3.12.10); the analysis venv is `.venv` (unicorn, capstone, numpy, pillow).
- `~/.cargo/bin` may be missing from PATH in the tool shells: prefix `export PATH="/c/Users/cyntrex/.cargo/bin:$PATH"`.
- In the Bash tool, `python` is a pyenv-win batch shim. **Never run `python -` or multi-line `python -c`**: they hang, and stopping the shell task leaves the `python.exe` running (three strays were found and ended by PID in the review). Write a script file and run it with `.venv/Scripts/python.exe`.
- `tools/notes_merge.py` prints non-ASCII: run it with `PYTHONIOENCODING=utf-8`.
- In PowerShell, `@(118039/48, -30/48)` fails to parse; pass precomputed numbers.
- The mGBA stable build creates `cheats/ patch/ savegame/ savestate/ screenshot/` in its working folder; `tools/mgba_ctl.py` points them at the session folder. The strays in the repo root were removed in the review (the one save moved to `data/work/e5298b24/_archive/`).
