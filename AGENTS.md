# Project brief: NFS GBA engine rewrite

Handoff document for any agent picking up this project. Read it fully before doing anything.

## Goal

Build a **from-scratch Rust reimplementation of the Game Boy Advance engine used by Pocketeers**. The first target is **Need for Speed Carbon: Own the City (GBA, 2006)**. The engine loads all game data from the user's **own ROM dumps**. No game code or assets ever ship with the project.

This is a **rewrite**, not an emulator and not a static recompilation. The end state is readable, idiomatic Rust that:

1. Loads the original ROM's data (car models, city geometry, textures, audio, tables).
2. Re-renders the original polygon data with **wgpu (via Bevy)** at modern resolution and framerate. The original game rendered its 3D in software at 240×160, and an emulator or recompilation can only upscale that image. A rewrite can redraw the actual geometry.
3. Reproduces the original gameplay (handling, AI, cops, races), checked against a reference build of the original.
4. Later loads the **other Pocketeers GBA NFS titles** in the same engine, and adds things the original never had (e.g. free roam, if the city data allows it).

Inspiration and prior art for this approach:
- IW4L: https://github.com/vladtrc/iw4L, an LLM-written Rust/Bevy/wgpu Call of Duty runtime that loads the user's own MW2 data.
- The mashup built on it: https://github.com/chasmlol/2010-rust-rewrite-mashup
- Skate 3 Rust engine: https://github.com/SK8-ENGINE/skate-3-rust-engine

Copy their habits: `docs/` with one short file per area, an agent brief in the repo, and game data kept out of git.

## Current state (as of this handoff)

> Superseded: this section describes the project before task 1. The live state is in `PROGRESS.md`.

- **Machine:** Windows, PowerShell. Project root is `E:\Games\rewrites\nfs_gba`.
- The root contains only this file and a `dumps/` folder. Git status is unknown: check with `git status`, and if there's no repo, run `git init` only **after** `.gitignore` exists.
- `dumps/` holds **7 zip archives**, named in the No-Intro style:

```
Need for Speed - Carbon - Own the City (USA, Europe) (En,Fr,De,Es,It).zip
Need for Speed Carbon - Own the City (USA, Europe) (En,Fr,De,Es,It).zip
Need for Speed - Most Wanted (USA, Europe) (En,Fr,De,It).zip
Need for Speed - Porsche Unleashed (Europe) (En,Fr,De,Es,It).zip
Need for Speed - Porsche Unleashed (USA).zip
Need for Speed - Underground (USA, Europe) (En,Fr,De,It).zip
Need for Speed - Underground 2 (USA, Europe) (En,Fr,De,It).zip
```

**Scope decision (from the user):** we work on **Carbon, European release**, only. The other four games are **reference material**: for back-checking hypotheses (e.g. "is this format or function shared?") and as a second opinion when Carbon's data is ambiguous. They are not targets yet, so don't spend effort on them beyond that.

Notes:
- **Carbon appears twice**, under two naming conventions. Both zips are labelled "(USA, Europe)", meaning one cartridge ROM was sold in both regions. It could be the same ROM or two different dumps; only the hashes will tell. If the two differ, **ask the user** which one counts as "Carbon Europe" before picking the canonical target.
- **Porsche Unleashed has two regional releases** (Europe multi-language and USA), which may be different builds. Vault both as reference.
- All five Pocketeers GBA titles are present, so shared-engine hypotheses can be checked against the siblings at any time.

## What we know about the game

| Fact | Detail |
|---|---|
| Title | Need for Speed Carbon: Own the City (Game Boy Advance) |
| Developer | **Pocketeers** (Doncaster, UK). Some databases credit EA Canada; TCRF notes hidden developer credits |
| Release | NA 31 Oct 2006, EU 3 Nov 2006, AU 9 Nov 2006 |
| ROM size | 8 MiB (64 Mbit), game code `BN7E` (verified, see `docs/recon/ROM-INVENTORY.md`) |
| Rendering | Full **software 3D engine**: cars, city and traffic are polygons. The GBA has no 3D hardware |
| Modes | 1–2 players (link cable). The NFS wiki says the GBA version lacks free roam (unverified) |
| CPU | ARM7TDMI, ARM + Thumb code |

**Sibling Pocketeers GBA titles,** all present in `dumps/` and probably sharing one engine (unverified):
- NFS Porsche Unleashed (GBA, 2004)
- NFS Underground (GBA, 2003)
- NFS Underground 2 (GBA, 2004)
- NFS Most Wanted (GBA, 2005)

As of late September 2026, no decompilation, recompilation or port of any of these was found.

## Tools to fetch

Check current versions and licenses before use. Fetch tools into `ext/`, which is gitignored and never vendored into the repo without a license check.

| Tool | Purpose |
|---|---|
| **gba-recomp** (Rust): https://github.com/JRickey/gba-recomp | Builds a native reference binary from the ROM; also a source of hooks and traces. Its runtime identifies the audio middleware (MP2K/M4A, GAX, …) by byte signature |
| **gbarecomp** (C++): https://github.com/smpduong/gbarecomp | Alternative recompiler with an analyzer and a self-healing interpreter tier. See MinishCapRecomp / EmeraldRecomp for usage patterns |
| **mGBA** | Emulator with a debugger, GDB stub and Lua scripting. Used for memory and call tracing and for producing golden reference outputs |
| **Ghidra** plus a GBA loader extension (look for one; verify it works) | Static analysis of the ARM/Thumb code |
| **GBATEK** (problemkaputt.de/gbatek) | Hardware reference: memory map, I/O registers, BIOS calls (SWI), cartridge header |
| Rust stable, Bevy, wgpu | The engine itself. Bevy is 0.x and breaks APIs each release, so pin versions and read the current docs, not memory |
| Python 3 (stdlib only) | Recon scripts: `zipfile`, `hashlib`, `zlib.crc32`. No third-party packages needed for task 1 |
| no$gba (Windows, optional) | Very good GBA debugger |

## Hard rules

1. **The `.gba` ROM files are the user's originals. Their bytes are never modified,** wherever they live. Verify with hashes before and after any operation. The zips in `dumps/` are only containers: extracting them (in `dumps/` or straight into the vault) is fine. Don't delete or overwrite the zips unless the user asks.
2. **Never commit or share ROMs, extracted assets, save files, or recompiled/generated code.** `.gitignore` is created before `git init` or any `git add`. Add a pre-commit hook that rejects `.gba`, `.zip`, `.sav`, large binaries, and anything under `dumps/` or `data/`.
3. **Record provenance** for every derived file: which ROM hash, which tool and version, which command.
4. **Don't guess silently.** Unknowns go in `docs/OPEN-QUESTIONS.md`. Hypotheses are labelled as hypotheses until verified.
5. **Work in small verified steps.** Every format parser gets a test against real data. Every gameplay function gets checked against the reference build: same inputs, same outputs.
6. Don't copy code from other projects without checking their license. Write down what was read and what was reused.
7. **Stop and report to the user** before anything destructive, expensive, or ambiguous.
8. **Windows hygiene:**
   - Quote every path, since the original zip names contain spaces, commas and parentheses.
   - Paths we create use no spaces: lowercase, hyphen-separated.
   - Keep paths short.
   - Use UTF-8 for all text files.

## Folder layout (set up first)

Everything lives under `E:\Games\rewrites\nfs_gba`. Source code is tracked by git; `dumps/` and `data/` never are. The repo finds the data folder through an environment variable, `NFSGBA_DATA`, set in a gitignored `.env` and defaulting to `.\data`, so it can move to another drive later without code changes.

```
E:\Games\rewrites\nfs_gba\
  AGENTS.md                    # this file (copy to CLAUDE.md if using Claude Code)
  README.md
  .env.example                 # NFSGBA_DATA=E:\Games\rewrites\nfs_gba\data
  .gitignore                   # .env, dumps/, data/, ext/, target/, *.gba, *.zip, *.sav, *.bin
  dumps/                       # user's zips; extracting here is fine; ROM bytes never modified; not in git
  data/                        # $NFSGBA_DATA: not in git
    vault/
      roms/<GAMECODE>_v<ver>_<sha1-8>.gba   # verified ROMs taken from the zips, read-only
      manifest.json            # one entry per ROM: hashes, header fields, source zip + member name
    work/<sha1-8>/<tool>/...   # scratch, always grouped by ROM, then by tool
    out/                       # reproducible generated outputs (reports, renders, viewers)
  docs/
    INDEX.md                   # map of all docs
    DECISIONS.md               # dated decision log (what, why, alternatives)
    OPEN-QUESTIONS.md
    recon/                     # task 1 reports
    formats/                   # one file per discovered data format
    engine/                    # one file per engine area (render, physics, AI, audio...)
  tools/                       # our own scripts, with tests
  crates/                      # Rust workspace (later)
  ext/                         # third-party tools, fetched, not in git
```

After a ROM is written to `vault/`, set it read-only (`Set-ItemProperty -Path <file> -Name IsReadOnly -Value $true`).

## Task 1: full recon of the dumps

**Goal:** know exactly what is in `dumps/` and have a clean vault before any reverse engineering starts. Deliverables are `docs/recon/ROM-INVENTORY.md`, `data/vault/manifest.json`, and `docs/recon/FIRST-LOOK.md`.

0. **Scaffold.** Create the layout above, `.gitignore` first, then `git init` if needed, then the pre-commit hook. Commit only the docs and scripts. Confirm `git status` shows nothing from `dumps/`.
1. **Inventory the zips.** For each zip, read the member list and each member's name, size, and the CRC32 stored in the archive.
2. **Extract and hash each ROM.** Extract the zips (into `dumps/<zip-name-without-extension>/` or straight into the vault). Compute CRC32, MD5 and SHA-1 of each `.gba`, and check that the CRC32 matches the one stored in the zip.
3. **Parse each GBA header** per GBATEK:
   - title (0xA0, 12 bytes)
   - game code (0xAC, 4 bytes; the last character is the region)
   - maker code (0xB0)
   - version (0xBC)
   - header checksum (0xBD), and whether it's valid
4. **Sanity-check each ROM:**
   - Is the size a power of two, and is there trailing padding?
   - **Are the two Carbon zips the same ROM?** Compare SHA-1s. If they differ, diff them (version byte, byte ranges that differ) and explain.
   - Are the two Porsche Unleashed ROMs different builds or just regional variants?
   - If the user has a No-Intro DAT, compare hashes against it. **Never download ROMs.**
5. **Build the vault.** Write each unique, valid ROM once to `data/vault/roms/` with the naming scheme, set it read-only, and record the source zip and member name in `manifest.json`. Duplicates are recorded in the manifest, not stored twice. Write `ROM-INVENTORY.md` as a readable table: game, region, version, hashes, source zip(s), verdict. Flag which Carbon ROM is the **canonical target** and why.
6. **First look.** Keep this shallow, with no deep reverse engineering yet. The full pass is for the **canonical Carbon ROM only**; the four siblings get just the comparison items marked *(siblings)*:
   - entropy map across each ROM, to see where code, compressed data and raw data sit
   - readable strings, including any "Pocketeers" or copyright strings
   - likely pointer tables (runs of values in the 0x08xxxxxx range)
   - candidate BIOS-compressed blocks (LZ77 header byte `0x10`, Huffman `0x2x`, RLE `0x30`), with a test decompression of a few
   - which audio engine it uses, by signature *(siblings)*
   - rough code-vs-data split
   - **engine-sharing evidence** *(siblings)*: identical or near-identical code sequences, shared function prologues, the same compression and audio engine. Produce one small similarity table. That's enough for now; the point is to know which siblings are useful for back-checking later.

   Write everything to `docs/recon/FIRST-LOOK.md`.
7. **Stop and report** to the user in plain language:
   - what's in the dumps
   - the verdict on the duplicate Carbon zip and the two Porsche Unleashed versions
   - whether the five games really share an engine
   - the vault location
   - anything suspicious
   - the recommended next step

All scripts go in `tools/`. They must be rerunnable (running twice changes nothing and never overwrites vault files), read `NFSGBA_DATA` from `.env`, write only into `data/work`, `data/out`, `data/vault` (task 1 only) or `docs/`, and have at least a smoke test.

## Roadmap after task 1 (outline, adjust as we learn)

1. **Reference build.** Get the canonical Carbon ROM running through gba-recomp and/or mGBA with scripting, so memory and function-call traces can be recorded for later comparison.
2. **Model viewer (first visible milestone).** Locate and parse the car and city geometry, then fly around Coast City in a minimal Bevy app at full resolution. This answers how Pocketeers stored their 3D data.
3. **High-resolution renderer.** wgpu rendering of the original geometry, keeping the original look.
4. **Gameplay parity.** Handling, AI, cops and races, each subsystem checked against the reference build.
5. **Sibling titles** loaded through the shared engine, only after Carbon works. Until then they're reference material only. Then extras: free roam, widescreen, higher framerate, link play over the network.

## Legal note

It's EA's game. The user supplies their own dumps, nothing derived from the ROMs is ever redistributed, and the project carries a clear "unofficial, not affiliated" notice. Reverse engineering for interoperability is tolerated in many jurisdictions, but not everywhere, and nothing here is legal advice.
