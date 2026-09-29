# Decisions

Newest first. Each entry: what, why, alternatives.

## 2026-09-29

- **The rewrite never runs game code; there is no emulator in it.** Every runtime crate is our own Rust (their only dependencies are `serde_json` and Bevy; no ARM decoding or CPU emulation anywhere in `crates/`). mGBA and unicorn (`tools/oracle`) run the *original* code only as **test oracles**, to prove our code gives identical results. This is the IW4L / Skate 3 approach (own engine, the user's original data), with one addition forced by the 1:1 goal: game logic is transcribed from the decompile with identical integer maths, not approximated.
  - **Known drift, to be removed:** `nfsgba-sim`, `menu.rs`, part of `hud.rs` (and the game loop being built) keep game state in a byte image laid out like GBA RAM and read fields at the game's addresses (about 350 accesses). That is not emulation (no CPU), but it is a decomp-style port: fastest to prove byte-exact against traces, but not the "readable, idiomatic Rust" the brief asks for, and it drags GBA details (heap layout: R24, U7) into the engine. Likewise the HUD emits GBA sprite-table entries (OAM).
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
- **`CLAUDE.md` imports `AGENTS.md` and `PROGRESS.md` (`@file`) instead of copying AGENTS.md**, so the two can't drift. The brief suggested copying.
- **Toolchains:** Rust 1.98.1, Git 2.55.0, uv 0.12.20, Python 3.14.7 pinned per project via pyenv-win `.python-version`. The global Python stays 3.12.10 so the user's other projects are unaffected.
- **Deferred:** `crates/`, `ext/`, `docs/formats/` and `docs/engine/` get created when first used. Empty folders aren't tracked by git anyway.
