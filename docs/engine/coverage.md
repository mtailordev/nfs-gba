# Coverage of the original (2026-09-30)

`tools/record.py coverage` (mGBA breakpoints on every function entry), `NFSGBA_MGBA_SESSION=coverage3`, the four original
scenarios plus eight career and ending runs replayed from `tools/career/*.script.json`
(`tools/oracle/coverage_plan.py` writes their key plans and race-finish pokes; fixture folder `coverage3`, PROVENANCE in
`tools/fixtures.py`). `tools/oracle/coverage_report.py` counts the Rust counterparts (a function's address as a literal or
`FUN_xxxxxxxx`, or its symbol name, anywhere in `crates/`) and writes `coverage3/unported.csv`.

| set | reached | in the Rust | no counterpart | not reached | of those in the Rust |
|---|---|---|---|---|---|
| original four (boot, menu_to_race, drive, pause_quit) | 466 | 357 | 109 | 419 | 152 |
| career and ending runs (ordinary, boss, boss2, gauntlet, zone, late, ending20, ending) | 493 | 386 | 107 | 392 | 123 |
| all twelve | 510 (of 885) | 400 | 110 | 375 | 109 |

The eight career runs reach 44 functions the original four miss; 43 have counterparts. The 110 without one are the earlier
classes (12 IWRAM renderer copies ported under other names, BIOS/OAM/text/fill libraries, IRQ and sound-hardware thunks, EEPROM
block helpers, no-ops); `hud_update_mode0`/`_mode3` are the mode branches of `nfsgba_formats::hud::hud_update`.
No new function needs porting. Not covered: Quick Play modes other than the circuit, every car, the garage screens.
