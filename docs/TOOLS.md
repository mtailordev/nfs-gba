# Tools

Installed 2026-09-29. `~` = `C:\Users\cyntrex`. Everything under `ext\` is gitignored and never vendored.
`ext\_selftest\` holds a synthetic 256 KiB test image (`test.gba`, made-up header + 6 instructions, no game data) and `luatest.lua`, used for the checks below.

| Tool | Version | Install / location | License | Verified with | Notes |
|---|---|---|---|---|---|
| 7-Zip | 26.03 | scoop `7zip`: `~\scoop\apps\7zip\current`, shim `7z` | LGPL-2.1+ / BSD (unRAR restriction) | `7z i` | |
| mGBA (stable) | 0.10.5 | scoop `mgba` (games bucket): `~\scoop\apps\mgba\current` | MPL-2.0 | `mgba-sdl.exe --version`; Lua 5.4 embedded; GDB stub: `mGBA.exe -g test.gba` + `arm-none-eabi-gdb -ex "target remote 127.0.0.1:2345"` read regs and disassembled 0x080000C0 | 0.10.5 has no `--script` flag (load scripts via Tools > Scripting). Its `config.ini` uses **relative** save/cheat/screenshot paths, so it creates `cheats\ patch\ savegame\ savestate\ screenshot\` in the current dir: launch it from its own dir, not the repo. `mgba-sdl -g` did not open a GDB port; use Qt `mGBA.exe -g` |
| mGBA (nightly) | 0.11-9146-c3c8e5e81 (2026-09-28) | official dev build unpacked to `ext\mgba-dev\` (portable) | MPL-2.0 | `mGBA.exe --script ext\_selftest\luatest.lua ext\_selftest\test.gba` wrote `loaded Lua 5.4` / `frame30 r0=1 pc=080000CC` | **Use this one for scripted tracing** (`--script`, can pass several). Don't load ROMs from `%TEMP%`: it pops a modal "Temporary file loaded" dialog |
| Java JDK | Temurin 25.0.1 (already installed) | `C:\Program Files\Eclipse Adoptium\jdk-25.0.1.8-hotspot` (on PATH) | GPL-2.0 + CE | Ghidra headless runs on it | Ghidra 12.1.4 needs JDK >= 21, no max. Machine `JAVA_HOME` still points to JDK 17: set `$env:JAVA_HOME` to the JDK 25 path before running Gradle. No extra JDK installed |
| Ghidra | 12.1.4 (2026-09-21) | scoop `ghidra` (extras): `~\scoop\apps\ghidra\current`; user env `GHIDRA_INSTALL_DIR` set | Apache-2.0 | `support\analyzeHeadless.bat` (usage) and a full headless import of `test.gba` | |
| GBA loader for Ghidra | pudii/gba-ghidra-loader 1.1.0+ (9bfb2d1), built for 12.1.4 | clone `ext\gba-ghidra-loader`; built zip in its `dist\`; installed to `%APPDATA%\ghidra\ghidra_12.1.4_PUBLIC\Extensions\gba-ghidra-loader` | Apache-2.0 | headless `-import test.gba -loader GBALoader` -> "Using Loader: GBA Loader", `ARM:LE:32:v4t:default`, import + analysis succeeded | Local patch: removed an unused `org.python.bouncycastle` import (Jython is gone in 12.1). Rebuild after each Ghidra upgrade (command below). Fallback if it breaks: raw binary, `ARM:LE:32:v4t`, base `0x08000000` |
| Arm GNU toolchain | 15.2.Rel1 (gcc 15.2.1, binutils 2.45.1, gdb 16.3.90) | scoop `gcc-arm-none-eabi` (extras); `bin` added to user PATH | GPL-3.0 | `arm-none-eabi-objdump/as/gcc/gdb --version` | |
| no$gba (debug version) | 3.06 (14 Apr 2025) | `ext\nogba\NO$GBA.EXE` from problemkaputt.de/no$gba.zip | Freeware (closed source) | exe present (GUI only) | |
| GBATEK | fetched 2026-09-29 | `ext\docs\gbatek.htm` (+ `gbatek.txt`) | (c) Martin Korth, free to read | file contains "GBA Memory Map" | |
| gba-recomp (Rust) | v0.4 (de4edf5, crate 0.2.0) | clone `ext\gba-recomp`; binaries in `target\release\` (`recomp`, `gba-launcher`, `gba-pack`) | MIT OR Apache-2.0; `gamedb.sqlite` CC0 | `cargo build --release` ok (4 min, warnings only); `recomp --help`, `recomp --version` | Not run on any ROM. Native translation wants `clang`/`gcc`/`cc` (or `GBA_RECOMP_CC`), else it uses its bundled TinyCC. clang only links inside a VS dev env (`vcvars64.bat`, see below). Useful subcommands: `dis`, `engine-scan`, `mp2k-scan`, `run --trace` |
| gbarecomp (C++) | 3dc2379 (2026-09-27) + submodule arm-recomp-core 14be3cf | clone `ext\gbarecomp` (`--recurse-submodules`) | **PolyForm Noncommercial 1.0.0** (read-only reference; don't copy code into our repo) | `cmake -S . -B build; cmake --build build --config Release` with MSVC | `build\Release\gba_recompile.exe` + test exes built. `gbarecomp_runtime` fails on MSVC (`__builtin_clz`); upstream builds the runtime with MSYS2 MinGW64 (gcc, cmake, ninja, SDL2), see MSYS2 row |
| MSYS2 | installer 2026-06-11 | winget `MSYS2.MSYS2` -> `C:\msys64` | mixed (GPL etc.) | **broken**: `usr\bin\pacman.exe` and `pacman-conf.exe` missing after install | Needs a clean reinstall by the user, then in the MINGW64 shell: `pacman -Syu`, `pacman -S --needed mingw-w64-x86_64-toolchain mingw-w64-x86_64-cmake mingw-w64-x86_64-ninja mingw-w64-x86_64-SDL2` |
| MSVC + Windows SDK | VS 2022 Community (MSVC 14.44.35207), VS 2022/2019 Build Tools; SDK 10.0.19041/22621/26100 | already installed | Microsoft EULA | `cargo new` + `cargo build` hello links and runs | VC tools live in the **Community** instance (2022 Build Tools has none) |
| CMake | 3.31.8 | already installed, `C:\Program Files\CMake` | BSD-3-Clause | `cmake --version` | |
| LLVM / clang | 23.1.2 | scoop `llvm`; `bin` on user PATH; user env `LIBCLANG_PATH`, `LLVM_LIB_DIR` | Apache-2.0 WITH LLVM-exception | `clang -shared t.c` inside `vcvars64.bat` -> dll | Also gives `llvm-objdump` (ARM-capable) |
| Rust | 1.98.1 stable msvc (already installed) | rustup, `~\.cargo\bin` | MIT OR Apache-2.0 | see MSVC row | |
| function oracle | — | `tools/oracle/oracle.py` (unicorn in `.venv`); checker `tools/oracle/rust-check` | ours (unicorn GPL-2.0) | `tools/test_oracle.py`; 93,169 cases vs the Rust ports, 0 mismatches | Calls any ROM/IWRAM function on an mGBA dump; returns registers and every changed byte; stops on unaligned access (unicorn does not rotate like the ARM7TDMI). `docs/engine/harness.md` |
| function coverage | — | `tools/coverage.py` + `tools/coverage.lua` | ours | 4 scenarios, about 3 min | mGBA breakpoints on every function entry; `data/out/coverage/e5298b24/` |
| ROM attribution | — | `tools/rom_attribution.py` | ours | `tools/test_rom_attribution.py` | Who owns each ROM byte |
| notes merge | — | `tools/notes_merge.py` | ours | `tools/test_notes_merge.py` | Merges agents' `docs/engine/notes/symbols.<agent>.csv` / `addresses.<agent>.csv`; dry run unless `--write` |
| Python venv | CPython 3.14.7; capstone 5.0.9, unicorn 2.1.4, numpy 2.5.3, pillow 12.3.0 | `uv venv .venv` in repo (gitignored); pins in `tools\requirements.txt` | BSD-3 / GPL-2.0 (unicorn) / BSD-3 / MIT-CMU | script: capstone Thumb `0x4770` -> `bx lr`; unicorn ARM `mov r0,#1` -> r0 == 1 | `capstone.__version__` reports 5.0.7 (upstream string lag); the wheel is 5.0.9 |
| ImHex | 1.38.1 | scoop `imhex` (extras): `~\scoop\apps\imhex\current\imhex.exe` | GPL-2.0 | `imhex --version` | Hex editor with pattern language, for format RE |

## How to launch

**mGBA with a Lua script** (nightly, portable):

```powershell
& E:\Games\rewrites\nfs_gba\ext\mgba-dev\mGBA.exe --script <script.lua> <rom.gba>   # add -g for the GDB stub on :2345
& "$env:USERPROFILE\scoop\apps\gcc-arm-none-eabi\current\bin\arm-none-eabi-gdb.exe" -ex "target remote 127.0.0.1:2345"
```

Lua API: `callbacks:add("frame", fn)`, `emu:currentFrame()`, `emu:readRegister("pc")`, `emu:read32(addr)`, standard `io` for writing trace files.

**Remote control for the canonical ROM:** `python tools/mgba_ctl.py start`, then batches such as `python tools/mgba_ctl.py "hold A 10" "wait 120" "shot x" "dump x" "save x"`, then `stop`. Output goes to `$NFSGBA_DATA\work\<sha1-8>\mgba\`.

**Extra emulator scripts:** `NFSGBA_MGBA_SCRIPTS=<path>` makes `mgba_ctl.py start` load more Lua next to the remote. `tools/mgba_frame_probe.lua`: write `NAME [ADDR=VALUE …]` to `probe.tmp` in the session folder and rename it to `probe.txt`; it saves the renderer's inputs at the start of `draw_visible_sectors` (`NAME.iwram.bin`, `NAME.wram.bin`, `NAME.vram.bin`) and the finished page a frame later (`NAME.final.bin`), optionally after patching RAM. `emu:setBreakpoint` works on IWRAM ARM code. Physics traces: the `trace NAME [SKIP]` command logs full RAM per car step (`docs/engine/physics.md`).
- **Menus** need presses of at least 10 frames.
- **Route to a race from a fresh save:** A (language: English) → A → A → START → A (intro screens) → profile name (DOWN, A, START) → main menu → A (Quick Play) → A (Random) → A → A.
- **Savestates** `mainmenu.ss` and `race.ss` are kept in the work folder.

**Ghidra headless** (GBA loader installed). For the race-time IWRAM code, add `-scriptPath tools\ghidra -preScript LoadIwram.java <iwram dump> -postScript ExportDecomp.java <out.c>` (the output of the current run is `data\work\e5298b24\ghidra\carbon_decomp.c`):

```powershell
$g = "$env:USERPROFILE\scoop\apps\ghidra\current"
& "$g\support\analyzeHeadless.bat" <project_dir> <project_name> -import <rom.gba> -loader GBALoader [-postScript <script>] [-readOnly]
# GUI: ghidraRun
```

**Apply our names** (after adding to `docs/engine/symbols.csv`), then re-export the decompile:

```powershell
& "$g\support\analyzeHeadless.bat" data\work\e5298b24\ghidra carbon -process BN7E_v0_e5298b24.gba -noanalysis -scriptPath tools\ghidra -postScript ApplySymbols.java docs\engine\symbols.csv -postScript ExportDecomp.java data\work\e5298b24\ghidra\carbon_decomp.c
```

Rebuild the loader after a Ghidra upgrade, then re-extract `dist\*.zip` into `%APPDATA%\ghidra\ghidra_<ver>_PUBLIC\Extensions\`:

```powershell
$env:JAVA_HOME = 'C:\Program Files\Eclipse Adoptium\jdk-25.0.1.8-hotspot'
$env:GHIDRA_INSTALL_DIR = (Resolve-Path "$env:USERPROFILE\scoop\apps\ghidra\current").Path
& "$env:GHIDRA_INSTALL_DIR\support\gradle\gradlew.bat" -p E:\Games\rewrites\nfs_gba\ext\gba-ghidra-loader --no-daemon
```

**Python venv:**

```powershell
E:\Games\rewrites\nfs_gba\.venv\Scripts\python.exe tools\<script>.py
# recreate: uv venv .venv --python C:\Users\cyntrex\.pyenv\pyenv-win\versions\3.14.7\python.exe
#           uv pip install --python .venv\Scripts\python.exe -r tools\requirements.txt
```

**clang for gba-recomp native translation:** run from a VS x64 dev shell:
`cmd /k "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat"`.
