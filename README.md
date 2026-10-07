# nfs_gba

A from-scratch Rust rewrite of *Need for Speed Carbon: Own the City* for the Game Boy Advance (Pocketeers / EA, 2006). It plays the whole game, from power-on through the career to the ending, in a modern engine at any resolution and framerate. Every asset is read from **your own ROM** at startup. This repository contains no game code, graphics, sound or text.

**Unofficial fan project, not affiliated with or endorsed by Electronic Arts, Pocketeers or Nintendo.**

## Play in the browser

**https://mtailordev.github.io/nfs-gba/**

Pick your ROM file (`.gba`, or the `.zip` it came in). It is read locally and never uploaded. Only the canonical cartridge dump is accepted:

| | |
|---|---|
| Game code | `BN7E` (the single USA/Europe release) |
| SHA-1 | `e5298b2482a769aa6458955cd6b18db3ac3f3a20` |
| CRC32 | `f4c0d140` |

Saves stay in your browser. They use the cartridge's own EEPROM format, so you can download a save and load it in an emulator, or the other way round.

## What is in it

- The complete game: boot, menus, Quick Play, the career (6 zones, 66 events, bosses and wingmen), the garage (parts, upgrades, paint, the turntable car), race results, pause, and the 20-page story ending.
- All four race types (circuit, sprint, elimination, hunter) plus wingman races, on every route the game can race, with all 15 cars.
- The game's own physics, opponent AI, wingman and traffic, the career rules and payouts, the HUD, and the music and sound engine.
- Two views of a race: a high-resolution 3D view with smooth motion between the game's frames, and the original 240×160 frame (key **O**).

## How it is checked

The rule is "exact where you can observe it". The simulation, rules, AI, physics, audio, save data and the 240×160 frame must give the game's own results. Presentation may differ: resolution, framerate interpolation, the 3D view.

- **Function oracle:** the original machine code runs in a CPU emulator on generated inputs, and our Rust must give the same results. The game's code is only ever run inside tests, never in the product.
- **Recorded play:** races and menu sessions recorded from the original (headless mGBA) are replayed in our engine and compared frame by frame: gameplay state, screen, sprites and sound samples.
- **Reachability proofs:** code the port leaves out, such as link play, is proven unreachable by tests that scan the ROM.
- **A deviation ledger:** [docs/FIDELITY.md](docs/FIDELITY.md) lists every place that is not exact and why. Nothing there is open work. What remains is accepted by design, mainly timing: the original's frame length depends on CPU cycles, which are modelled, not emulated. `tools/gate.py` (format, lint, all tests against real data, ledger markers) runs before every merge.

## Run it natively

Builds for Windows, Linux, macOS and Android come out of every push to `main` (the `builds` workflow's artifacts; engine only). On the desktop, give it your ROM: `nfsgba-viewer path/to/your.gba` (or drop the `.gba` on the program); the save goes next to it as `.sav`. On Android, the app asks for the ROM (a `.gba` or a `.zip`) on its first start and keeps it in its own storage; touch pad on screen, controllers work too. To build the Android app yourself: `tools/android_build.py` (needs the Android SDK and NDK, `cargo-ndk`, JDK 17; see its header).

The game keeps its original speed on slow machines: the simulation is far cheaper than a frame (0.24 ms per race step), its clock follows the audio device's, and the 3D view lowers its internal resolution when frames come late (`NFSGBA_SCALE=<0.25..1>` fixes it). Only the sectors the game itself lists are drawn, and nothing is drawn under the full-screen menus.

From the source:

Requirements: Rust stable, Python 3.14 (the tools are stdlib-only; analysis extras are in `tools/requirements.txt`).

1. Put your zipped dump in `dumps/`, then `copy .env.example .env` and set `NFSGBA_DATA` (where the verified ROM vault and work files live).
2. `git config core.hooksPath tools/git-hooks`. The pre-commit hook refuses ROMs, zips, saves, `data/` and large files.
3. `python tools/vault.py` verifies the hashes and builds the read-only ROM vault.
4. `cargo run --release -p nfsgba-viewer` starts the game from power-on (with the vault's ROM when no ROM is given).

Other modes are chosen with environment variables. `NFSGBA_PLAY=1 NFSGBA_ROUTE=<n>` races one route from the grid; `NFSGBA_CITY=1` is a free fly-through of the city. Details are in the doc comment at the top of `crates/nfsgba-viewer/src/lib.rs`.

## Controls

| GBA | Keyboard |
|---|---|
| D-pad | Arrow keys |
| A | X |
| B | Z |
| L / R | A / S |
| Start | Enter |
| Select | Backspace |

A gamepad works too: South = A, East = B, shoulders or triggers = L/R, D-pad or left stick (on Android: the controller's A/Y = A, B/X = B, shoulders and triggers = L/R, Start, Select, D-pad). On phones the web page shows a touch pad; the Android app has its own, and on a touch-screen computer it appears with the first touch.

Viewer keys in a race: **O** switches to the original 240×160 frame, **G** toggles the game camera and a free camera, **T** shows the racing line.

## Layout

| Path | What |
|---|---|
| `crates/nfsgba-fixed` | The game's integer maths: division, sine and reciprocal tables, atan, square root, random table |
| `crates/nfsgba-formats` | ROM parsers, the pixel-exact software renderer, HUD, menus data, career rules |
| `crates/nfsgba-audio` | The LS_Play sound engine: music modules, sound effects, mixer |
| `crates/nfsgba-sim` | Player car physics, opponents, wingman, traffic |
| `crates/nfsgba-game` | The typed game state, race start and race frame, the menus, and `Session`, the whole game behind one frame API |
| `crates/nfsgba-viewer` | The Bevy app: the full game, the high-resolution view; `platform.rs` is what differs per platform (the ROM, the save, the window) |
| `crates/nfsgba-android`, `android/` | The Android app: the native library and the Gradle project around it |
| `tools/` | ROM vault, headless emulator driver, recorders, function oracle, coverage, the merge gate |
| `docs/` | Formats, engine notes, the address map, decisions and the ledger ([index](docs/INDEX.md)) |

## Licence

The code is dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your choice. This covers our code only, not the game or its data.

## Legal

You need your own copy of the cartridge. Don't open issues or pull requests containing ROMs, extracted assets or save files. *Need for Speed* and *Carbon* are trademarks of Electronic Arts.
