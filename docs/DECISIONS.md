# Decisions

Newest first. Each entry: what, why, alternatives.

## 2026-09-30 (web build)

- **The viewer itself builds for the web** (no separate crate): a `cfg(target_arch = "wasm32")` layer in `nfsgba-viewer` takes the ROM and save from the page and hands saves back; the page (`web/index.html`, no framework) does the file pick, the SHA-1 check and browser storage. GitHub Pages deploys it (`.github/workflows/pages.yml` runs `tools/web_build.py`).
- **WebGL2, not WebGPU:** our shaders work there unchanged and it runs in every current browser; WebGPU would add nothing the viewer uses.
- **A `web` cargo profile** (`opt-level = "s"`, LTO, one codegen unit) for the web build only: the module after `wasm-opt -Oz` is 27.8 MB (9.0 MB gzipped) instead of 44.3 MB with the release profile; the game still runs at the display rate. The desktop release profile is unchanged.
- **New dependencies (wasm32 only):** `js-sys` (read `window.nfsgba`, call the save callback), `wasm-bindgen` (its `JsValue`; the CLI version must match the lock file), and Bevy's `web` feature. `nfsgba-audio` is now a direct dependency of the viewer (the spare race's sound engine). Time uses `bevy::platform::time::Instant` (works on the web), so no `web-time`.
- **The full game's spare race comes from `Setup::menus`**, not the `race-init/circuit_pre` capture, so the full game needs no data folder on any platform.

## 2026-09-29 (the contract, simplified; the user's decision)

- **Exact:** the mechanics, physics, rules, AI, audio and every calculation (the game's integer maths, function for function, checked against the original in the oracle); the assets decoded and used as the game uses them, so the look is the same (the 240×160 reference frame); save data in the game's format. In the user's words: exact same mechanics, physics and calculations, the assets used the same way for the same look, but "we don't need an absolutely perfect bit-exact replication".
- **Not required:** the GBA memory layout, heap addresses and pointers, scratch and unused bytes, and where the interrupts land inside a frame. Frame timing is a deterministic model; recorded timing is only a test input.
- **Architecture: the IW4L / Skate 3 approach.** The ROM is the asset source, parsed once into `GameData`. The engine is typed Rust designed for reading: a `World` of cars, drivers, traffic, camera, race, HUD and menus, with vectors and indices, enums and named fields. There is no RAM image at runtime.
- **Verification:** per function, the oracle (exact on the values the function produces); per frame, the replay traces loaded into typed state and compared on the gameplay state (positions, speeds, race, AI, HUD values, sound commands) and the reference frame, not on raw RAM.
- **For the migration:** each remaining subsystem is rewritten on typed `World` state with the existing exact Rust port as its reference (differential tests on every trace state), instead of a byte-preserving refactor. `layout!` loads test states. `store`, `u_<offset>` fields and the RAM round trip are only needed while a subsystem still hands state to RAM-image code. The car step already in progress finishes under the old, stricter rules.
- Why: the byte-level check doubled the work (a port in the RAM layout, then a byte-preserving conversion) and forced every internal byte into the typed state, none of which the player can see.
- This supersedes the "What 1:1 means" entry of the project review below where they differ.

## 2026-09-29 (tooling bake-off)

- **The toolchain** (measurements in `docs/engine/harness.md` "Tool bake-off"):
  - **Function porting: the oracle** (`tools/oracle`, `cases.py`) on snapshots, with **synthesized race states** (`cases.py synth`: the setup poked into a capture, our exact `race_start`, checked against the game's code). 9,000 cases green on the first run, about 3 minutes per small function.
  - **States and long runs: the headless mGBA libretro core** (`tools/retro.py`, `ext/libretro/mgba_libretro.dll`, MPL-2.0). It loads our `.ss` files, reaches menus from power-on by scripted keys, and is byte-exact against the `mgba/race` fixture in RAM, VRAM, palette and OAM. It runs at 22× real time in a race and is deterministic. `Retro.dump` writes a snapshot the oracle and `Dump::load` read.
  - **Race states after the start: `nfsgba-game`** (2,867 game frames/s, deterministic).
  - **Breakpoint recordings** (per-car-step traces, IRQ timing points, coverage) stay on the existing mGBA Lua recorders (`tools/record.py`) until one is needed often. Nobody drives the mGBA window by hand to reach a state: states come from `retro.py` key scripts, `.ss` files or synthesis.
  - Why: exact against the existing fixtures, fast, deterministic, no build and no window; together they cover every need except breakpoints, which the recorders already have.
  - Alternatives: **gba-recomp** runs Carbon correctly and allows a hook at any PC from Rust, but its cycle model moves the timing bytes (frame counter, timer 3, rand index) within 300 frames, and it has no savestates. It stays in `ext/` as the candidate if a headless hook recorder becomes necessary. **The GDB stub** was not tried: gba-recomp answers whether headless breakpoints are possible, and they are not needed yet.

## 2026-09-29 (efficiency; the user's request)

- **How we work from now on** (binding; details in `PROGRESS.md` "How we work"): fresh session per milestone with `PROGRESS.md` as the handoff; no forked agents, only fresh agents with a one-page brief; a cheaper model for mechanical porting, tests and tools, the top model for hard reverse engineering, design and review; at most 2–3 agents at once; oracle first and no playing the game in the emulator to reach states; exact where observable, invisible internals documented once and not chased; minimal docs; typed state and one copy of each helper in new code.
  - Why: the first session spent about 15–20 M tokens in agents, mostly because forked agents carried the coordinator's whole conversation, everything ran on the top model, and agents drove the emulator window by hand.
- **Tooling bake-off before any more porting.** The next session tests, each in one small timeboxed test: the function oracle with synthesized states (no emulator), a headless mGBA libretro core driven from Python, gba-recomp as a native reference, mGBA's GDB stub, and our own `nfsgba-game` as a state generator. It records the results in `docs/engine/harness.md` and the chosen toolchain here, and retires the rest (likely including the mGBA window plus Lua file-polling remote).

## 2026-09-29 (project review; the user's decisions)

- **What "1:1" means (the contract every test checks).** In the user's words: rebuild everything from the game (assets, scripts, all mechanics, all graphics) 1:1, but as an adaptable engine that runs at pretty much any resolution and framerate while every core system stays exactly the game's, like the Skate 3 and IW4L Rust rewrites. So:
  - **Core = exact.** Simulation, rules, AI, physics, audio, save data and asset decoding give the game's results: same inputs and same frame times, same state evolution, same audio samples, same save bytes, same 240×160 reference frame and sprite table. Byte-equal *RAM* is no longer the contract; RAM images are a debugging and test oracle only.
  - **Presentation = adaptable.** The simulation steps at the game's own cadence (its frame time, `0x03005640`, is a deterministic input). The renderer draws that state at any resolution, and at any framerate by interpolating between steps. The 240×160 mode stays as the exact reference view.
- **Timing: a deterministic model.** Frame timing (timer 3's ticks, where the VBlank IRQs land: T1, T2, D17, U5, R17) stays an explicit input: exact from recordings, a documented deterministic model in live play. No cycle-accurate CPU model unless live play must match real hardware frame for frame later.
- **Consolidate before adding features** (the independent audit, 2026-09-29): a shared test kit with a data-required mode and fixture provenance; one copy of every duplicated helper and subsystem; the typed `World` migration behind the existing replay tests; the ledgers brought up to date and gated. Then boot → menus → race in our code, then broader coverage, then the high-resolution renderer on typed state.
- **Merged agent worktrees are removed** after each merge (the user agreed; 23 worktrees, 29 GB, removed 2026-09-29 after archiving their untracked files).

## 2026-09-29

- **The rewrite never runs game code; there is no emulator in it.** Every runtime crate is our own Rust (their only dependencies are `serde_json` and Bevy; no ARM decoding or CPU emulation anywhere in `crates/`). mGBA and unicorn (`tools/oracle`) run the *original* code only as **test oracles**, to prove our code gives identical results. This is the IW4L / Skate 3 approach (own engine, the user's original data), with one addition forced by the 1:1 goal: game logic is transcribed from the decompile with identical integer maths, not approximated.
  - **Known drift, to be removed:** `nfsgba-sim`, `menu.rs`, part of `hud.rs` (and the game loop being built) keep game state in a byte image laid out like GBA RAM and read fields at the game's addresses (52% of the Rust lines by the 2026-09-29 audit: 14,010 of 27,009 lines in files built on `Mem`/`menu::Gba`, 1,124 hard-coded RAM addresses; an earlier "about 350" was an undercount). That is not emulation (no CPU), but it is a decomp-style port: fastest to prove byte-exact against traces, but not the "readable, idiomatic Rust" the brief asks for, and it drags GBA details (heap layout: R24, U7) into the engine. Likewise the HUD emits GBA sprite-table entries (OAM).
  - **Plan:** once the game loop's frame-by-frame tests exist (they are the safety net), move each subsystem to typed state as the source of truth (car, driver, race, world, HUD, menus), with `from_ram`/`to_ram` adapters only in the tests; keep the integer maths unchanged. GBA-shaped outputs (the 240×160 index frame, OAM, the sound FIFO) stay only for the original-resolution/reference mode; the high-resolution engine reads typed state directly. Already typed: `career::{Racer, Race}`, `hud::{Globals, Racer, Driver}` inputs, `render::Scene`, `nfsgba_audio::Engine`.
  - Alternative rejected: keeping the RAM image as the core. It is a recompilation in all but name and blocks the roadmap's extras (free roam, widescreen, network play).
- **Rust workspace:** two crates.
  - `crates/nfsgba-formats` holds the ROM parsers. It has no Bevy dependency, so its real-data tests build in seconds.
  - `crates/nfsgba-viewer` is the Bevy app.
  - **Bevy is pinned to `=0.19.1`**, the latest stable. We skipped 0.20.0-rc.2 because it's a release candidate. The camera uses Bevy's built-in `FreeCamera` rather than our own controller.
- **Viewer units and axes:**
  - **`SCALE = 1/48`, one unit for cars and city.** This replaces 1/256 (a road-width guess) and then 1/192 (which assumed a ×4 model factor).
  - The race dump shows the vehicle matrices are pure rotations, with translations in city units, so there is no model factor.
  - The cars measure about 48 units per metre, so the city is exaggerated (streets about 40 m).
  - Rendering from the game's chase camera reproduces the game's screenshot layout.
  - Raw space (x right, y down, z forward) maps to Bevy as `(x, -y, -z)`: a rotation, not a mirror.
- **Car paint in the viewer uses the 20 ROM paint presets.** The game generates the real paint ramp at runtime from the player's colour choice.

- **Redundant dumps deleted, at the user's request** (they chose "only redundant copies"). The kept Carbon zip was re-verified to contain `e5298b24…` before anything was deleted. Deleted (SHA-1 of each file):
  - `Need for Speed - Carbon - Own the City (USA, Europe) (En,Fr,De,Es,It).zip`, `71aa106e4bbbdc31524570523cd9472c6b33dab5`: the same ROM as the kept `Need for Speed Carbon - …` zip, which has the TorrentZip timestamp.
  - `Need for Speed - Porsche Unleashed (USA).zip`, `8eb540eb138274bb433461a05cd95062f87c4295`, and its vault ROM `AZFE_v0_c57a0652.gba`, `c57a0652017c47ac2d25a51bf351738cd634272b`: the same build as EU `AZFP`, with 3 bytes different (see FIRST-LOOK.md).
  - Five zips and five vault ROMs remain, one per game.

- **Canonical target is `BN7E` v0 `e5298b24`.** Both Carbon zips contain this same ROM byte for byte, and its game code's region letter `E` covers the USA and Europe (No-Intro: "(USA, Europe)"). There is no separate European build to choose, so the user's "Carbon, European release" is this ROM.
- **Porsche Unleashed representative for comparisons: `AZFP` (Europe).** The EU and USA ROMs are the same build (3 bytes differ); both are vaulted.
- **No No-Intro DAT check.** The user has no DAT, and downloading one was not an option we pursued. Hashes are recorded so the check can happen later.
- **ROMs are read straight out of the zips into the vault; nothing is extracted into `dumps/`.** This means fewer loose copies of ROM bytes. The alternative, extracting to `dumps/<zip-name>/`, is allowed by the brief but adds nothing.
- **Generated docs are separate from hand-written ones.** `recon/ROM-INVENTORY.md` and `recon/FIRST-LOOK-DATA.md` are rewritten by the tools (only when their content changes); `recon/FIRST-LOOK.md` is interpretation. This keeps tool reruns from clobbering prose.
- **The pre-commit hook lives in `tools/git-hooks/` and is enabled with `git config core.hooksPath tools/git-hooks`**, so it is versioned. `.git/hooks/` is not tracked. **A fresh clone must run that config command.**
- **`.gitattributes` forces LF.** The global `core.autocrlf=true` would otherwise check the hook out with CRLF line endings, and `sh` fails on those.
- **The local agent instruction file imports `AGENTS.md` and `PROGRESS.md` (`@file`) instead of copying AGENTS.md**, so the two can't drift. The brief suggested copying. That file is local only (not in git).
- **Toolchains:** Rust 1.98.1, Git 2.55.0, uv 0.12.20, Python 3.14.7 pinned per project via pyenv-win `.python-version`. The global Python stays 3.12.10 so the user's other projects are unaffected.
- **Deferred:** `crates/`, `ext/`, `docs/formats/` and `docs/engine/` get created when first used. Empty folders aren't tracked by git anyway.
