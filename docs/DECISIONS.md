# Decisions

Newest first. Each entry: what, why, alternatives.

## 2026-09-29

- **Rust workspace:** two crates.
  - `crates/nfsgba-formats` holds the ROM parsers. It has no Bevy dependency, so its real-data tests build in seconds.
  - `crates/nfsgba-viewer` is the Bevy app.
  - **Bevy is pinned to `=0.19.1`**, the latest stable. We skipped 0.20.0-rc.2 because it's a release candidate. The camera uses Bevy's built-in `FreeCamera` rather than our own controller.
- **Viewer units and axes:**
  - `SCALE = 1/256` turns raw units into roughly metres (a two-lane street of 1,920 units comes out about 7.5 m wide).
  - Raw space (x right, y down, z forward) maps to Bevy as `(x, -y, -z)`: a rotation, not a mirror.
  - Vehicles are drawn at `CAR_SCALE = 4` × `SCALE`. **This is a guess** (it makes a car about 4 m long), marked `ponytail:` in the code until the vehicle transform gives the real factor.

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
