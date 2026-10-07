# Progress log

Session state for whoever picks this up next. Read `AGENTS.md` first, then this file, then `docs/DECISIONS.md` (the newest entries set the contract) and `docs/FIDELITY.md` (every open deviation).

## Status (2026-09-30: the milestones are done)

**Phase: finished; extras next.** The whole game runs in our code: `cargo run --release -p nfsgba-viewer` plays from power-on (boot, menus, every race mode, results, pause, garage, career, bosses, story, the 20-page ending; the game's sound; saves in the game's EEPROM format), checked against the original at every step. `tools/gate.py` passes 8/8 on `main`.

What exists (evidence in `docs/FIDELITY.md` "Closed"):
- **Data:** every asset family decoded from the ROM; 98.9% of the ROM's bytes attributed (`tools/rom_attribution.py`).
- **Crates:** `nfsgba-fixed` (the game's integer maths, once), `nfsgba-formats` (parsers, the exact 240×160 renderer, HUD, menus data, career rules), `nfsgba-audio` (the LS_Play engine, bit-exact), `nfsgba-sim` (player car, opponents, wingman, traffic), `nfsgba-game` (typed `World`, race start, race frame, typed menus, `Session` = the whole game with one frame API), `nfsgba-viewer` (Bevy 0.19.1: the full game, high resolution with framerate interpolation and the 240×160 reference mode), `nfsgba-testkit` (dev only).
- **Checks:** per-function oracle sets on the game's own code, whole-frame replay traces, headless power-on runs through the menus, races of every mode to their results, the visual check of the high-resolution view against the exact frame (R27), the exact HUD blend (G2).
- **Reachability:** every `Unported` stop left in the code is proven unreachable with Carbon's data by a test (`nfsgba-game` `src/reach.rs`, FIDELITY N1). Coverage (`docs/engine/coverage.md`): 12 scenarios reach 510 of 885 functions; none still needs porting.
- **The ledger** (`docs/FIDELITY.md`) groups what is not exact by why: **Open** is empty; everything else is **accepted** by the contract: hardware timing (no cycle-accurate CPU), unreachable paths, high-resolution presentation, invisible internals and oracle scaffolding.

## Working rules (from the user)

- **The contract (binding, `docs/DECISIONS.md`, simplified 2026-09-29):** the mechanics, physics, rules, AI, audio and calculations are exact (the game's integer maths, function for function), and the assets are used the same way for the same look (the 240×160 reference frame); presentation adapts (any resolution; any framerate by interpolating between simulation steps). Not required: bit-exact replication (the GBA memory layout, pointers, scratch bytes, where interrupts land). Frame timing is a deterministic model. The engine is typed Rust in the IW4L / Skate 3 style.
- **No game code at runtime, no emulator.** mGBA and unicorn are test oracles only. New code uses typed state; RAM images live only in tests.
- **Every deviation is tracked.** `NOT 1:1 (ID)` in code, the matching row in `docs/FIDELITY.md`; an entry is closed only with a test in the repo that checks it.
- **Never find anything twice.** Every ROM offset, RAM address and function goes into `docs/engine/address-map.md` and `docs/engine/symbols.csv` (applied to Ghidra by `tools/ghidra/ApplySymbols.java`).
- **Oracle first:** port a function against `tools/oracle` on generated inputs over snapshots; use mGBA only for new snapshots and whole-frame traces.

## Start here (next session)

1. `docs/FIDELITY.md` "Open" is empty (2026-09-30); a new deviation found later goes there. **Seam audit (2026-10-07):** the bugs that slipped through were at the boundaries, not in ported functions: words the menus and the race share in the game's RAM that the hand-over did not copy (the results block, the career skill, the reverse flag, the camera option; back: camera, rand, laps, yaw), a RAM address passed as text (the player's name), the high-resolution car (spoiler pose and order, the rims drawn into the atlas during the race), and checks that excluded exactly the broken part. Now checked: `race_init.rs` `the_race_start_takes_every_shared_word_from_the_menus` (every word both sides declare, on all race-start captures and with test patterns), the career standings' racers, the viewer's menu race in chase, turn and look-back views (`shots::a_menu_race_shows_the_spoiler_and_the_rims`), pause-quit, a German run, a lost race (`session2`, `session3`, `race-init2`). When adding a hand-over or a new view, extend these instead of writing a new one-off check. Everything under "Accepted" is settled by the contract; don't re-open it without a new reason (a path becoming reachable, a decision to model CPU timing).
2. **Extras (the roadmap's milestone 5):**
   - **Web: done (2026-09-30).** The repo is public; `.github/workflows/pages.yml` deploys the WebGL2 build to https://mtailordev.github.io/nfs-gba/ on every push to `main` (the player picks their own ROM; SHA-1 checked; saves in `localStorage`, downloadable in the EEPROM format; touch pad on phones; gamepad). Local build: `tools/web_build.py` (`docs/engine/viewer-rendering.md` "Web build"). Untested: a real phone and a real gamepad; loading a save mid-game needs a reload. The gate lints the wasm build.
   - **Every platform (2026-10-07):** `platform.rs` (ROM, save, window per platform), the touch pad, Android controllers, the background pause; speed on weak systems (only the game's sectors drawn, render scale, audio-clock sync, nothing under the menus: `docs/engine/viewer-rendering.md` "Speed on any system"). Desktop builds for Windows/Linux/macOS and the Android APK come out of `.github/workflows/builds.yml`. **Android works** (`docs/engine/android.md`: power-on to a race on the emulator at real speed); untested on a real phone. iOS later on the same pieces.
   - A high-rate mode (read how `mstan/MarioKartSuperCircuitRecomp` does 60 fps; licence unclear, read only), widescreen, free roam.
   - Link play over the network (Carbon's link code is dead: nothing sets the flag, `reach::link_play_is_unreachable`), then the sibling Pocketeers titles.

### Toolchain (decided 2026-09-29; results in `docs/engine/harness.md` "Tool bake-off")

- **Porting a function:** the oracle, `tools/oracle/cases.py` (a case set module, JSONL, a Rust replay test; example: `cases.py unlock` → `crates/nfsgba-formats/src/unlock.rs`).
- **A race state for any setup:** `cases.py synth make NAME ENV ROUTE MODE CAR` → `cargo run -p nfsgba-game --example synth_race -- NAME` → `cases.py synth check NAME`, then `Game::frame` for later frames.
- **Any other state, long runs, screenshots:** the headless mGBA core, `tools/retro.py` (loads `.ss`, key scripts from power-on, 22× real time, `Retro.dump` → oracle snapshot). Never drive the mGBA window by hand.
- **Breakpoint recordings** (per-call logs, IRQ timing, coverage): mGBA nightly with Lua, scripted through `tools/mgba_ctl.py` / `tools/record.py` (examples: `tools/traces2.py`, `tools/recorders/coverage.py`). Known bug: `tools/record.py run` calls `probe()` twice on the same paths (work around with `lua FILE` commands).
- **Proving a path unreachable:** a ROM scan in `nfsgba-game` `src/reach.rs` (literal-pool words, Thumb BL call sites, `thumb_uses` register follow); the Ghidra decompile misses some functions, the scan does not.

## How we work (efficiency rules, 2026-09-29; binding)

Why the first session was expensive: agents were spawned as *forks* (each copied the coordinator's whole, growing conversation and re-read it every turn); every agent ran on the most expensive model; agents played the game in the emulator to reach states and built their own recorders and oracles; invisible internals were chased to the byte; long prose docs, integration notes and hand-merging; one ever-growing coordinator session.

Rules:
- **Fresh sessions.** One milestone per session; `PROGRESS.md` is the handoff. Keep coordinator messages short.
- **No forks.** Spawn fresh agents with a brief of at most one page: the files to read (only the ones needed), the files they own, the acceptance test, the budget (50–100 turns), "commit early".
- **Right model for the job.** A cheaper model (Sonnet-class) for mechanical porting against the oracle, tests, tools and doc tidying; the top model only for hard reverse engineering, design and review.
- **At most 2–3 agents at once**, with disjoint files. Infrastructure first, one at a time.
- **Oracle first, emulator last.** Port function by function against `tools/oracle/cases.py` on real or synthesized states. Whole-frame checks use the tool the bake-off picks. Nobody navigates menus by hand to reach a state.
- **Exact where it is observable** (the contract in `docs/DECISIONS.md`): gameplay state, rules, AI, physics, audio, save data, the 240×160 frame. Traces are compared on typed gameplay state, not raw RAM. Invisible internals (heap bytes, stale registers, mid-frame IRQ timing) are not reproduced and not chased.
- **Minimal docs.** Code comments, one `docs/FIDELITY.md` row and address/symbol rows per finding (`docs/engine/notes/*.<agent>.csv` → `tools/notes_merge.py --write`). No prose write-ups, no "Integration notes" sections, no long reports.
- **Better code, not just more.** New code on typed state, one copy of each helper (`nfsgba-fixed`, `nfsgba-testkit`), small modules; no new RAM-image code.
- The user looks at progress now and then; they are not part of the test loop. Ask them only when stuck on something only they can do.
- Before any piece of work, ask "can we do this way smarter and still get the same or better result?" (the user's rule, 2026-09-30).
- **Fixture folders are shared between worktrees** (`data/` is one folder): an agent records new traces into a new folder with its own `PROVENANCE` row, never into an existing one, so work in progress can't break another branch's gate.
- **Low memory (WSL/Docker can hold ~34 GB):** when Claude Code's memory guard stops a gate, run it in pieces at `CARGO_BUILD_JOBS=1`: fmt; Python tests; markers; clippy on the engine crates, then the viewer; `cargo test --release -p <crate>` one crate at a time with `RUST_TEST_THREADS=1` (the whole-game tests hold a full simulation each; in parallel they exhaust memory), all with `NFSGBA_REQUIRE_DATA=1 NFSGBA_FIXTURE_LOG=<one log>`; then `tools/fixtures.py build --log <log>` and `check --log <log>`.
- **Machine load (the user plays on this PC):** at most one cargo build or gate at a time across all worktrees (the test kit's fixture log and the `menus3` case files are shared, so parallel gates also give false failures); agents build but leave the gate to the coordinator, who runs gates one after another.
- **Gate before every merge** (`tools/gate.py`); rebase branches made before a history rewrite; no attribution trailers in commits; each worktree builds into its own `target/`, seeded with `cp -r` of the main checkout's `target/` (dependencies stay fresh; only crates with build scripts and ours rebuild; never a shared `CARGO_TARGET_DIR`, which makes cargo test another worktree's code; sccache gives no hits across worktree paths) and is removed after merging; the coordinator gates in the main checkout (warm: ~7 min instead of 40): rebase the branch onto `main`, `git checkout --detach` it, `tools/gate.py --update-fixtures` (one test run: the manifest is rebuilt from the gate's own log; `CARGO_BUILD_JOBS=4`, more runs out of memory on this PC), then fast-forward `main` at once (commit nothing to `main` in between); an emulator session is stopped by its own PID.

## Environment notes

- **Commit messages carry no attribution trailers** (no `Co-Authored-By`/session lines; the user's rule). The history was cleaned of them on 2026-09-29 (backup: `data/work/_archive/pre-trailer-rewrite.bundle`); every agent prompt must say so, and rebased agent branches get them stripped.
- **GitHub:** private repo `mtailordev/nfs-gba` (`origin`); push `main` only. The account blocks pushes that expose its private email, so this repo commits as `18623619+mtailordev@users.noreply.github.com` (local git config). On 2026-09-29 the whole history was rewritten to that address (a backup bundle of the old refs is in `data/work/_archive/pre-email-rewrite.bundle`). Branches made before the rewrite (the old `worktree-agent-*` ones, and the dedup agent's) must be **rebased** onto `main`, never merged, or the old addresses come back and the push is refused.

- Toolchains: Rust 1.98.1 (clippy, rustfmt), Git 2.55.0, uv 0.12.20, Python 3.14.7 pinned by `.python-version` (the global pyenv stays 3.12.10); the analysis venv is `.venv` (unicorn, capstone, numpy, pillow).
- `~/.cargo/bin` may be missing from PATH in the tool shells: prefix `export PATH="$HOME/.cargo/bin:$PATH"`.
- In the Bash tool, `python` is a pyenv-win batch shim. **Never run `python -` or multi-line `python -c`**: they hang, and stopping the shell task leaves the `python.exe` running (three strays were found and ended by PID in the review). Write a script file and run it with `.venv/Scripts/python.exe`.
- `tools/notes_merge.py` prints non-ASCII: run it with `PYTHONIOENCODING=utf-8`.
- In PowerShell, `@(118039/48, -30/48)` fails to parse; pass precomputed numbers.
- The mGBA stable build creates `cheats/ patch/ savegame/ savestate/ screenshot/` in its working folder; `tools/mgba_ctl.py` points them at the session folder. The strays in the repo root were removed in the review (the one save moved to `data/work/e5298b24/_archive/`).
