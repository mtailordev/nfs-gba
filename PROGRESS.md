# Progress log

Session state for whoever picks this up next. Read `AGENTS.md` first, then this file, then `docs/DECISIONS.md` (the newest entries set the contract) and `docs/FIDELITY.md` (every open deviation).

## Status (2026-09-29, after the tooling bake-off)

**Phase: consolidation, then milestones.** The toolchain is decided (`docs/DECISIONS.md`, "tooling bake-off"). Next session: the consolidation ("Start here" below). No new subsystems until it is done (user decision, `docs/DECISIONS.md`).

What exists and is exact (details and evidence in `docs/FIDELITY.md` "Closed"):
- **Data:** every asset family decoded from the ROM (text, images, models, city, routes, audio, career tables, fonts, HUD and menu layouts); 98.9% of the ROM's bytes attributed (`tools/rom_attribution.py`).
- **Crates:**
  - `nfsgba-fixed`: the game's integer maths, once (division, reciprocal table, sine/atan, `isqrt`, `rand_table`, `angle_diff`);
  - `nfsgba-formats`: parsers, plus rendering (`render`, `sky`, `paint`, `atlas`), HUD (`hud`, `ui`), menus (`menu`), career and race rules (`career`);
  - `nfsgba-audio`: the LS_Play engine, bit-exact;
  - `nfsgba-sim`: the player car, the opponents, the wingman and traffic, with every car-step path ported;
  - `nfsgba-game`: the race start (`race_init`) and the race frame loop (`Game::frame`), exact over five recorded runs (2,445 frames) with nothing stood in;
  - `nfsgba-viewer` (Bevy 0.19.1): high resolution, a 240×160 reference mode, and play mode (`NFSGBA_PLAY=1 NFSGBA_DUMP=game-loop/s18`);
  - `nfsgba-testkit` (dev only): data loading, `NFSGBA_REQUIRE_DATA=1`, the fixture log.
- **Checks:** `tools/gate.py` (rustfmt, clippy `-D warnings`, all tests with data required, Python tests, fixture manifest, notes, `NOT 1:1` markers) passes 7/7. The ledger has 36 open entries and 35 closed.
- **Not yet:** boot → menus → race in our code (the menus port has logic but no drawing: U3, U7); the garage screens (Kind18); coverage of the 415 functions never reached; the high-resolution view agrees with the reference frame only 49% exactly (R27).
- **Structural debt still open (from the 2026-09-29 audit):**
  - the race start still runs on the menus' RAM image and loads the `World` at the end (G3); the rim redraw reads a heap arena (R24);
  - ROM data read by hard-coded BN7E offsets (`GameData` exists; each migrated subsystem moves its offsets into `data::bn7e`);
  - several Closed claims have no test in the repo.

  (Fixed since the audit: silent test skips, fixture provenance, duplicated helpers, recorders and oracle drivers.)

## Working rules (from the user)

- **The contract (binding, `docs/DECISIONS.md`, simplified 2026-09-29):** the mechanics, physics, rules, AI, audio and calculations are exact (the game's integer maths, function for function), and the assets are used the same way for the same look (the 240×160 reference frame); presentation adapts (any resolution; any framerate by interpolating between simulation steps). Not required: bit-exact replication (the GBA memory layout, pointers, scratch bytes, where interrupts land). Frame timing is a deterministic model. The engine is typed Rust in the IW4L / Skate 3 style.
- **No game code at runtime, no emulator.** mGBA and unicorn are test oracles only. New code uses typed state; RAM images live only in tests.
- **Every deviation is tracked.** `NOT 1:1 (ID)` in code, the matching row in `docs/FIDELITY.md`; an entry is closed only with a test in the repo that checks it.
- **Never find anything twice.** Every ROM offset, RAM address and function goes into `docs/engine/address-map.md` and `docs/engine/symbols.csv` (applied to Ghidra by `tools/ghidra/ApplySymbols.java`).
- **Oracle first:** port a function against `tools/oracle` on generated inputs over snapshots; use mGBA only for new snapshots and whole-frame traces.

## Start here (next session)

**State (2026-09-30):** everything is merged except the viewer branch `worktree-agent-a17182f905c90bd65` (race from the grid, smooth motion; being extended with the fixes from the user's one playtest). Gate and merge it when memory allows. Then, in order:
1. **Playable story mode at 240×160 in our code:** one `Game` loop (boot → title → menus → race → results → menus, passing `Setup`/`Handover`; the menus drawn by the typed `Screen`), the garage (Kind18, U3), `map_draw`. Check: a headless power-on-to-results run compared with the original at every screen change.
2. **Coverage:** a recorded race for every mode (circuit, sprint, elimination, hunter, wingman, career) and a career event win; the six `Unported` stops in the AI and traffic (D4, AI hunter mode); every route started once.
3. **Automated visual check (R27) before more high-resolution work:** the high-resolution view compared with the exact 240×160 frame on every recorded state (traffic drawn, lights on their cars, no seams, the shortcut walls), so rendering bugs fail a test instead of needing a playtester.

### 1. Toolchain (decided 2026-09-29; results in `docs/engine/harness.md` "Tool bake-off")

- **Porting a function:** the oracle, `tools/oracle/cases.py` (a case set module, JSONL, a Rust replay test; example: `cases.py unlock` → `crates/nfsgba-formats/src/unlock.rs`, about 3 minutes for a small function).
- **A race state for any setup:** `cases.py synth make NAME ENV ROUTE MODE CAR` → `cargo run -p nfsgba-game --example synth_race -- NAME` → `cases.py synth check NAME` (our `race_start` against the game's code), then `Game::frame` for later frames.
- **Any other state, long runs, screenshots:** the headless mGBA core, `tools/retro.py` (loads `.ss`, key scripts from power-on, 22× real time, `Retro.dump` → oracle snapshot). Never drive the mGBA window by hand.
- **Breakpoint recordings** (car-step traces, IRQ timing, coverage): the existing `tools/record.py` recorders in mGBA, only when a new trace is really needed. If that becomes frequent, gba-recomp's `gba-core` (Rust, hooks at any PC; its timing differs from mGBA) is the candidate.

### 2. Consolidation (next)

Done: the test kit and merge gate (`tools/gate.py`, `docs/engine/testkit.md`, `docs/engine/fixtures.csv`); one recorder, one oracle CLI, one script loader (`tools/record.py`, `tools/oracle/cases.py`); one copy of the game's maths (`crates/nfsgba-fixed`), one decoder, one dump reader, one copy of each shared helper; ledgers gated (every `NOT 1:1` names an open ID).

Left, in order:
1. ~~**One viewer path**~~ (done 2026-09-29, R28 closed): every race mode runs through `nfsgba-game`; routes are built in Rust (`race_init::apply_setup` + `race_start`).
2. **Typed `World`** (the IW4L / Skate 3 style the user wants): each subsystem rewritten on typed `World` state, with the existing exact Rust port as its reference (differential tests on every trace state; the contract no longer asks for bit-exact RAM). Conventions and the order: `docs/engine/typed-state.md` (`layout!`, `state/`, `GameData`, adapters in `view/`). Done (2026-09-29): `Game` owns a typed `World` and the race frame runs on it (camera, car step, AI, traffic, slots, effects, HUD, renderer, sound; D16, D19 closed); every menu screen with a Rust handler is typed (`nfsgba-game/src/menu/`); the replay and race-start tests compare typed state plus screen and sound. Left: the race start on typed state (G3), the atlas heap arena (R24), the menus' drawing helpers (U7, milestone 1).

### 3. Milestones after that

1. **Playable reference:** boot → menus → race entirely in our code at 240×160 (menus drawing: U7; garage screens: U3; the race-start handover, intro, countdown, fades, race end, pause: G1). Surveyed 2026-09-29; batches of ~100 agent turns, oracle first, on typed state. **Done 2026-09-29: batches 1–4** (fades and sound stops; the race start, intro and countdown to GO; the race end, results, pause and resume with a typed `Flow`/`Handover` outcome; the menu drawing primitives on a typed `Screen`), each with recorded traces or oracle cases. Left: 5, 6, then one `Game` loop that runs the menus and the race and passes `Setup`/`Handover` between them:
   1. G1a, frame edges: `main_frame`'s fade block and the four fade steps (`menus/fades.jsonl`), keys, page flip, timers, shadow-OAM copy, `snd_stop_all`/`music_stop`, `restart_engine_sound`, the state-4 tail (the debug console `debug_print_heap` is invisible: skip it).
   2. G1b, countdown and start: `race_start_from_table_b` (`0x0813af7c`), `effect_sprite_alloc`, `countdown_tiles_a..d`; `Game::frame` for phases 1/9 and `0x03005714 == 3`. Record the intro to GO headless first (`tools/retro.py` from power-on through Quick Play).
   3. G1c, race end and pause: phases 6–8 and 3, `fill_results` + `results_tiebreak`, `race_cleanup` (frees become typed drops), the state-5 exit, the pause block, `race_menu_palette_setup`; the menu ↔ race handover on typed state. A career finish needs a headless capture (laps set to 1).
   4. U7a, drawing: `fill_rect8` (odd-byte BG VRAM stores: check against a headless mGBA frame), `menu_blit_material`, `text_menu`/`text_menu_7`/`text_box`/`text_menu_wrapped_colour`, `menu_button_prompts`, `message_box_draw`, drawing into `Game`'s typed page buffers with the `ui.rs` primitives.
   5. U7b, scenes and sprites: `load_menu_descriptor`, `unpack_to_buffer`, `sprite_screen_select`/`update`, `intro_page_setup`, `health_screen_image`, `menu_scene_free`, `copy_palette_to_ram`; check a headless power-on-to-title run frame by frame.
   6. U7c/U3: boot and save (`profile_reset`, `save_load_profile`, `save_write_profile`), `map_zone_palettes`/`map_draw`, Kind18 (garage) with a new headless key-driven capture.
2. **Deterministic timing model** for live play (T1, T2).
3. **Coverage:** every car, route and mode, the garage, a career win, AI hunter mode, cops if they exist (D4); the 415 functions never reached.
4. **High-resolution engine** on typed state: painter's order (R10), the speed effect (R11), model index 0 (R14), framerate interpolation, an HUD blend (G2), automated comparison against the 240×160 reference (R27).
5. **Extras:** link play over the network, then the sibling Pocketeers titles.

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
- **No playtesting by the user** (2026-09-30: "i dont wanna be your visual debugger"): visual and gameplay bugs are caught by tests (R27 comparison, traces, oracle cases); a launch command only when the user asks to see progress.
- **Machine load (the user plays on this PC):** at most one cargo build or gate at a time across all worktrees (the test kit's fixture log and the `menus3` case files are shared, so parallel gates also give false failures); agents build but leave the gate to the coordinator, who runs gates one after another.
- **Gate before every merge** (`tools/gate.py`); rebase branches made before a history rewrite; no attribution trailers in commits; each worktree builds into its own `target/` (never a shared `CARGO_TARGET_DIR`: cargo then reuses another worktree's build of a crate and tests the wrong code) and is removed after merging; an emulator session is stopped by its own PID.

## Environment notes

- **Commit messages carry no attribution trailers** (no `Co-Authored-By`/session lines; the user's rule). The history was cleaned of them on 2026-09-29 (backup: `data/work/_archive/pre-trailer-rewrite.bundle`); every agent prompt must say so, and rebased agent branches get them stripped.
- **GitHub:** private repo `mtailordev/nfs-gba` (`origin`); push `main` only. The account blocks pushes that expose its private email, so this repo commits as `18623619+mtailordev@users.noreply.github.com` (local git config). On 2026-09-29 the whole history was rewritten to that address (a backup bundle of the old refs is in `data/work/_archive/pre-email-rewrite.bundle`). Branches made before the rewrite (the old `worktree-agent-*` ones, and the dedup agent's) must be **rebased** onto `main`, never merged, or the old addresses come back and the push is refused.

- Toolchains: Rust 1.98.1 (clippy, rustfmt), Git 2.55.0, uv 0.12.20, Python 3.14.7 pinned by `.python-version` (the global pyenv stays 3.12.10); the analysis venv is `.venv` (unicorn, capstone, numpy, pillow).
- `~/.cargo/bin` may be missing from PATH in the tool shells: prefix `export PATH="/c/Users/cyntrex/.cargo/bin:$PATH"`.
- In the Bash tool, `python` is a pyenv-win batch shim. **Never run `python -` or multi-line `python -c`**: they hang, and stopping the shell task leaves the `python.exe` running (three strays were found and ended by PID in the review). Write a script file and run it with `.venv/Scripts/python.exe`.
- `tools/notes_merge.py` prints non-ASCII: run it with `PYTHONIOENCODING=utf-8`.
- In PowerShell, `@(118039/48, -30/48)` fails to parse; pass precomputed numbers.
- The mGBA stable build creates `cheats/ patch/ savegame/ savestate/ screenshot/` in its working folder; `tools/mgba_ctl.py` points them at the session folder. The strays in the repo root were removed in the review (the one save moved to `data/work/e5298b24/_archive/`).
