# nfs_gba

A from-scratch Rust reimplementation of the Pocketeers Game Boy Advance engine, starting with *Need for Speed Carbon: Own the City* (GBA, 2006). It loads all game data from **your own ROM dump**. No game code or assets ship with this project.

**Unofficial fan project. Not affiliated with or endorsed by Electronic Arts, Pocketeers or Nintendo.**

## Setup

1. Put your zipped dumps in `dumps/`.
2. `copy .env.example .env` and adjust `NFSGBA_DATA` if the data should live elsewhere.
3. `git config core.hooksPath tools/git-hooks` (the pre-commit hook refuses ROMs, zips, saves, `data/` and large files).
4. `python tools/vault.py`: verifies and hashes the ROMs and builds the read-only vault and manifest.
5. `python tools/first_look.py`: runs the recon reports.
6. `python -m unittest discover -s tools`: runs the smoke tests.

7. `cargo run --release -p nfsgba-viewer`: fly through the city with the vehicle bank beside it, all read from your ROM. Hold right mouse to look around, WASD/QE to move, Shift to run, and the scroll wheel to change speed.

Tools: Python 3.14 (stdlib only; the analysis extras are in `tools/requirements.txt`) and Rust stable (built with 1.98). Docs start at [docs/INDEX.md](docs/INDEX.md); agents start at [AGENTS.md](AGENTS.md).
