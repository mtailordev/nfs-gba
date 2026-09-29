# Test kit and merge gate

How the tests reach data, how missing data is handled, where every fixture came from, and the gate a branch
passes before it merges.

## `nfsgba-testkit` (a dev-dependency of every crate)

| Call | What |
|---|---|
| `rom()` | The canonical ROM (`data/vault`, `tools/vault.py`) |
| `fixture(rel)` | A fixture file or folder, `rel` relative to `$NFSGBA_DATA/work/<sha1-8 of the ROM>/` (e.g. `mgba/race.wram.bin`, `vehicle-physics`). The **only** place a fixture path is resolved |
| `read(rel)`, `read_to_string(rel)` | A fixture's contents |
| `dump(prefix)` | An mGBA dump by prefix (`mgba/race` → `race.wram.bin`, `.iwram.bin`, …) |
| `missing(what)` | The one rule for missing data (below) |
| `Expect::new(what, n)` + `tick()` | Counts what a replay loop checked; the test fails if it ends with any other count, so an early `return` or `break` cannot pass silently |

**Missing data:** with `NFSGBA_REQUIRE_DATA=1` a missing ROM or fixture **fails** the test. Otherwise the test
prints one `SKIPPED (no data): …` line (visible with `-- --nocapture`) and passes, so a fresh checkout without
data still builds and tests. The merge gate always requires data.

**Exact replays:** the replay tests assert exact counts and accept no stop:
- game traces: 149, 699, 699, 599, 299 frames, per frame and free-running;
- the 16 car scenarios (steps per scenario);
- the 12 AI scenarios (calls, compared car states, lane timers taken from the trace);
- the 14 race starts;
- the fuzz and direct-call sets (4,500 cases each) and the oracle cases (`crates/nfsgba-formats/tests/oracle_cases.rs`: 20,121, 71,048 and 2,000).

A re-recorded trace changes these numbers on purpose; update the table in the test with it.

## Fixture manifest (`docs/engine/fixtures.csv`, hard rule 3)

Every file the tests read, with SHA-1, size, recorder, command, source state and the ROM hash: 1,414 files,
1,129 MB. Built from what the tests actually resolve: `tools/fixtures.py build` runs the Rust tests with
`NFSGBA_REQUIRE_DATA=1` and `NFSGBA_FIXTURE_LOG=<file>` (every `fixture()` call appends its path; a folder
fixture lists every file under it). `tools/fixtures.py check [--log FILE]` verifies each listed file (about
1 s) and, given a test run's log, that nothing unlisted was read.

Provenance is recorded per recording session (top folder), as the agents documented it: the recorder tool, its
command and the source state, with the doc that has the details (`PROVENANCE` in `tools/fixtures.py`). A new
fixture folder without an entry stops `build`. After re-recording, run `build` and commit the manifest.

## Merge gate (`tools/gate.py`)

`.venv/Scripts/python.exe tools/gate.py` runs, and exits non-zero if any step fails:

1. `cargo fmt --all --check`;
2. `cargo clippy --release --workspace --all-targets -- -D warnings`;
3. `cargo test --release --workspace` with `NFSGBA_REQUIRE_DATA=1` (and the fixture log);
4. the Python tool tests (`unittest discover -p "test_*.py"` in `tools/`; `trace_tests.py` is now
   `test_trace_tools.py`, so discover runs it);
5. `tools/fixtures.py check --log` (every fixture present, unchanged and listed);
6. the branch's **pending** notes CSVs (`docs/engine/notes/`, changed since the merge base with `main`) merge
   without conflict (`tools/notes_merge.py` dry run); notes already merged are not re-checked;
7. every `NOT 1:1 (ID)` marker in `crates/` names an ID that is open in `docs/FIDELITY.md` (a table row).
   Markers without an ID, or citing a closed or unknown ID, fail.

A full run takes about 4 minutes (the Rust tests dominate).

**Demonstrated on this branch (2026-09-29):**
- with `harness/oracle/divsi3.jsonl` renamed, the gate fails twice: step 3 (`required test data missing:
  fixture harness/oracle/divsi3.jsonl`) and step 5 (`FIXTURE harness/oracle/divsi3.jsonl: missing`);
- with `if k == 100 { return; }` in the free-run loop of `crates/nfsgba-game/tests/replay.rs`, step 3 fails
  (`drive free run: items checked`).

Steps 1–6 pass on the branch; step 7 reports 23 markers for the coordinator (below).

## Marker findings (for `docs/FIDELITY.md`; not fixed here)

- Without an ID:
  - `nfsgba-audio/src/lib.rs:4` (a doc sentence mentioning the marker);
  - `nfsgba-formats`: `hud.rs:462`, `lib.rs:311` (runtime v scroll: R21), `menu.rs:1208`, `menu.rs:2489`
    (unreachable: U9), `paint.rs:84` (R17), `render/entities.rs:84` (entity handlers), `render/entities.rs:120`,
    `ui.rs:166`, `ui.rs:562`;
  - `nfsgba-game/src/lib.rs:56` (live-play timing: T1);
  - `nfsgba-sim`: `ai.rs:711` (lane timer, timing: T1 or the closed D17), `car.rs:70` and `init.rs:473`
    (the rim blit `draw_decal_on_atlas`, in no FIDELITY row);
  - `nfsgba-viewer`: `game.rs:228` and `game.rs:316` (R11), `indexed.wgsl:17` (hi-res: R27),
    `main.rs:1042` (free camera only), `play.rs:199` (G2).
- Citing closed IDs: `nfsgba-viewer/src/main.rs:580` (R13), `main.rs:890`, `main.rs:1120`, `main.rs:1183`
  (R12). Note that `main.rs:1120` says the original frame draws no cars, while FIDELITY's Closed section says the
  240×160 mode matches s15 with the car included.
