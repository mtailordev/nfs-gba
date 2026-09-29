# Harness: oracle, coverage, attribution, notes merge

Shared tooling so that porting a game function does not start with hand-driving mGBA into a state. Every tool
reads the canonical ROM from the vault and writes only under `$NFSGBA_DATA`. Use the analysis venv:
`.venv/Scripts/python.exe` (the pyenv shim mangles multi-line scripts).

| Tool | What it answers |
|---|---|
| `tools/oracle/oracle.py` | What does game function X return, and which bytes does it change, for these inputs on this RAM snapshot? |
| `tools/oracle/prove.py` + `crates/nfsgba-formats/tests/oracle_cases.rs` | Is the oracle right? It is checked against ported functions, and the saved cases check the Rust ports |
| `tools/trace_oracle.py` | Per-step replay of recorded car traces (vehicle-physics); runs on the oracle |
| `tools/coverage.py` + `tools/coverage.lua` | Which functions run in real play, per scenario, and how often |
| `tools/rom_attribution.py` | Which known structure owns each ROM byte, and what is still unexplained |
| `tools/notes_merge.py` | Folding agents' symbol notes into `symbols.csv` without silent choices |

## Function oracle

```python
sys.path.insert(0, "tools/oracle"); from oracle import Gba
gba = Gba()                                    # ROM + data/work/<sha8>/mgba/race.*.bin (any dump prefix works)
r = gba.call(0x0816A708, r0=7, r1=-2)          # __divsi3, Thumb (ROM addresses default to Thumb, RAM to ARM)
r.regs["r0"], r.stop, r.writes                 # "return" when it came back; [(address, bytes)] changed
r = gba.call(0x0813A514, r0=0x030000C0, mem=[(0x03005614, b"\xf8\x02\0\0")])   # inputs for this call only
r.read(0x05000000, 512)                        # memory after the call
gba.call(fn, stubs={0x08135FDC: record})       # stub a callee: record(uc) runs, then it returns at once
```

CLI: JSON lines in, one result per line out (`.venv/Scripts/python.exe tools/oracle/oracle.py < q.jsonl`), for
example `{"fn": "0x0816a708", "regs": {"r0": 7, "r1": -2}}`. The other keys are listed in the module docstring.
A Rust test either spawns it or, better, reads a saved JSONL of cases, as `crates/nfsgba-formats/tests/oracle_cases.rs` does.

**A snapshot** is any mGBA memory dump prefix under `data/work/<sha8>/` (`Gba("car-paint/d0")`). To make one from a
savestate, run `python tools/mgba_ctl.py "load NAME" "dump NAME"` with mGBA running.

**Model and limits:**
- **Memory:** BIOS, EWRAM, IWRAM (+ the `0x03FF8000` mirror), IO, palette, VRAM and OAM come from the snapshot. The ROM sits at `0x08`/`0x0A`/`0x0C000000`, read-only. Each call starts from the snapshot and is restored afterwards; `keep=True` chains calls instead.
- **CPU:** unicorn ARM946 (ARMv5TE), the closest model to the ARM7TDMI. It differs on unaligned loads and stores: the ARM7TDMI rotates them, unicorn does not. So with `align="stop"` (the default) any unaligned access stops the call with a message; `align="ignore"` is about 10× faster. Other v4T/v5 differences (`pop {pc}` or `ldr pc` with bit 0 clear) do not occur in compiled Thumb/ARM interworking code.
- **BIOS:** calls run in Python per GBATEK: Div, DivArm, Sqrt, CpuSet, CpuFastSet, LZ77 WRAM/VRAM (reading memory as the BIOS does, also before the destination). Halt, IntrWait and VBlankIntrWait stop the call. Div by zero stops with an error (the hardware hangs). Other SWIs stop with "not implemented"; Carbon only uses 2, 5, 6, 7, 0xB, 0x11 and 0x12.
- **Not modelled:** interrupts, timers, DMA and video. IO reads return the snapshot's values. DMA starts are reported in `notes`; in Carbon only the EEPROM and sound code use DMA.
- **`writes` leaves out the call's own stack frames.** A write counts as a frame when it lands below the initial sp and within a push's reach (64 bytes) of the sp of that moment. IWRAM globals are never hidden, whatever their address.
- **Translation cache:** code bytes changed behind the CPU's back (inputs, restores) invalidate unicorn's translation cache, so rewritten IWRAM code runs as written.

**Checked against ported functions** (`prove.py`, cases saved to `data/work/<sha8>/harness/oracle/*.jsonl`; `oracle_cases.rs` compares the Rust ports):

| Function | Cases | Result |
|---|---|---|
| `__divsi3` `0x0816A708` | 20,121 (edges, b = 0, INT_MIN / −1) | 0 mismatches against `div`; also equal to truncating division |
| `sin_q14` `0x0815F948` | 71,048 (every angle −0x100…0x100FF, random words) | 0 mismatches against `paint::sin_q14` |
| `apply_sector_light_to_palette` `0x0813A514` | 2,000 random positions in random sectors (1,889 changed the palette) | 0 mismatches against `sector_light` + `tint_palette` |
| same, at the reference state | 1 | palette RAM equals the dump in 254/256 entries; 192 and 208 differ, exactly the glass slots of FIDELITY R17 |
| `recip_div` `0x03004CA4` (IWRAM, ARM) | 5,000 | equal to `(a · recip[b]) >> 24` |
| car handler `0x0814BD4C` (`trace_oracle.py`) | 9 traces, 1,149 steps | every step reproduces the next traced state; `<name>.oracle.txt` byte-identical to the pre-refactor output; `nfsgba-sim` trace tests pass; no unaligned access in any step |

**Speed:** about 7,000–10,000 calls/s for small functions with `align="ignore"`, and about 700–900/s for
`apply_sector_light_to_palette` with the alignment check on. The 1,149 car steps, each with a full RAM load, take 7 s.
Per call, the cost is the memory diff (numpy over 400 KB) plus the Python hooks. A unicorn `timeout` would add a
thread per call (1,000 calls/s), so the instruction limit `max_insns` guards runaways instead.

**How to port with it:** find the function (`carbon_decomp.c`, `symbols.csv`) and pick a snapshot where its inputs
are live. Run it through the oracle on hundreds or thousands of generated inputs (random plus edge cases) and save
the cases as JSONL. Then write the Rust port with a test that replays the cases. That replaces the one-off Lua
capture of each agent. mGBA is still needed for new snapshots and for whole-frame traces (below).

## Coverage map

`tools/coverage.py [SCENARIO ...]` launches mGBA with `coverage.lua`. It sets a breakpoint on each of the 880
function entries in `carbon_decomp.c`, and stops its own mGBA by PID when the key plan ends. It writes
`data/out/coverage/<sha8>/coverage.csv` (address, name, kind, hits per scenario) and `summary.txt`.
Screenshots of each scenario's end are in `data/work/<sha8>/harness/cov-*.png`.

| Scenario | Plan | Frames | Wall time | Functions run (named) |
|---|---|---|---|---|
| `boot` | power-on with a fresh save, intro screens, title (START), new profile "A", main menu | ~2,200 | 59 s | 200 (94) |
| `menu_to_race` | `mainmenu.ss`: Quick Play, Random, confirm, race start and countdown | ~1,100 | 61 s | 384 (237) |
| `drive` | `race.ss`: accelerate, steer both ways, brake | 600 | 35 s | 238 (171) |
| `pause_quit` | `race.ss`: pause, X, "QUIT: ARE YOU SURE?" yes, back to Quick Play | ~580 | 25 s | 305 (192) |
| any | | | | **465 of 880** (265 named, 200 unnamed); 415 never ran |

**Most-called unnamed functions** (the best naming candidates):
- **All scenarios:** `0x08161000` and `0x081610C8`, up to 61,000 hits in boot.
- **Racing:** `0x03005824` and `0x03005894` (IWRAM, next to the IRQ dispatcher `0x03005810`; about 40 per frame, so probably pieces of the IRQ path), and `0x08161204`, `0x08160EBC`, `0x081611C4`, `0x0816A2A0` and `0x0815CD00`.
- **Driving only:** `0x08146754`.

The per-scenario lists are in `summary.txt`.

**Caveats:**
- **Coverage counts entries.** A function that returns at once still counts: `0x08151AA8`, the audio test tone of FIDELITY A4, is entered every frame (about 2,200 times in boot), presumably to check the flag that is never set.
- **Scenarios don't reach everything.** The garage, career events, the other race modes and link play are not among these scenarios, so functions that only run there show 0.
- **`0x08224EE0` is not code.** It lies in the LZ77 image bank; Ghidra's `FUN_08224ee0` should be deleted.

## ROM byte attribution

`tools/rom_attribution.py` writes `data/out/coverage/<sha8>/rom-attribution.csv` (every byte, in ranges:
start, end, size, category, owner) and `rom-attribution.txt`. Claims go most specific first:
- code: each function from its start to the next, so literal pools count with their function;
- audio;
- the four material tables with every stream they address (packed streams by the bytes the decoder consumes for the image);
- palettes, and the model-bank arrays (extents from the model records);
- city and menu-scene sectors and walls;
- routes (template entities, section tables, waypoints);
- the text table and every string it points to;
- the level descriptors;
- then every sized ROM row of the address map.

Container ranges and rows the map calls unknown claim nothing.

**Result: 98.907% of the 8,388,608 bytes are attributed** (91,698 bytes in 350 ranges are not).

| Category | Bytes |
|---|---|
| images | 5,805,794 |
| audio | 1,223,057 |
| city | 355,148 |
| code | 268,273 |
| tables (map rows, descriptors) | 202,007 |
| text | 171,798 |
| models | 106,292 |
| **unattributed** | **91,698** |
| routes | 80,724 |
| padding (end-of-ROM zero fill, alignment) | 49,321 |
| palettes | 34,304 |
| header | 192 |

- **Text strings** span exactly `0x799B88–0x7BFC53` (computed from the table's pointers).
- **Material counts:** menu 273 (189 packed), HUD 280, vehicle 147 (110 packed), city 227.

**The two known gaps:**
- **`0x794000–0x799B88`:** confirmed unattributed, all 23,432 bytes. It is the tail of a larger unexplained block, **`0x78E714–0x799B88` (46,196 bytes)**, which starts right after route 42's racing line (the last route data) and runs up to the text strings. It opens like a u16 table (`00 03 00 80 00 80 01 03 …`).
- **`0x7F5CC8–0x800000`:** only **48 bytes** of data (`e0 e1 e2 …`, a byte map), then zero fill to the end of the ROM (41,736 bytes). The map's "the ROM has no padding" is wrong for this range.

**Largest other unattributed ranges** (full list in the .txt, with the owners on both sides):

| Range | Bytes | Next to | Likely |
|---|---|---|---|
| `0x7EE9D4–0x7F0BD8` | 8,708 | font descriptors / car table | the per-car tables `0x7EEA24`…`0x7F0636`: in the map, but without a byte size |
| `0x47A9A2–0x47BC6C` | 4,810 | model polygon sizes / city texels | rec `+0x50` (276 B, unknown) and the variant descriptor's palette block `0x47A9B8` |
| `0x722DD4–0x723DD4` | 4,096 | city materials / sectors | rec `+0x28` → world `+0x2C`, unknown |
| `0x7F4598–0x7F5494` | 3,836 | minimap palettes / control bindings | minimap palette per route and neighbours (unsized rows) |
| `0x7E5078–0x7E5E48` | 3,536 | race mode names / health colours | menu page records `0x7E5090`… (unsized) |
| `0x7E78EC–0x7E86A0` | 3,508 | paint presets / text table | story screens `0x7E78B8` (10-byte records, count unknown) and more |
| `0x7E62C0–0x7E6EEC` | 3,116 | setup screens / paint presets | setup items (0x18 each); starts with the string "ERROR" |
| `0x7F3050–0x7F38B8` | 2,152 | menu descriptor / entity handler table | `0x7F3050`, `0x7F37D8` rows (unsized) |
| `0x7EE274–0x7EE974` | 1,792 | sound ids / font descriptors | font glyph widths and y offsets (range row) |

Twelve 128-byte blocks after the sky gradients turned out to be part of the gradient materials: kind-1 materials
are 1 × 128 colours (256 bytes). The game's gradient uses the first 64; the rest are `0x0421` and zeros. The
tool now claims the declared size.

**The address map should give a byte size for these rows**, so that they can be claimed and checked:
`0x7E4714`, `0x7E4B18`, `0x7E4DE4`, `0x7E544C`, `0x7E5E54…`, `0x7E5090`/`0x7E553C`/`0x7E5DA8`, `0x7E78B8`,
`0x7EE274…0x7EE894`, `0x7EEA24`, `0x7EEA33`, `0x7EEA44`, `0x7EEB70`, `0x7EEBBC`, `0x7EF5A0`/`0x7EF672`,
`0x7EF816`, `0x7F0626`, `0x7F0636`, `0x7F3050`, `0x7F37D8`, `0x7F3CDA`, `0x7F40D0`, `0x7F4164`/`0x7F41A0`,
`0x7F42E4`, `0x7F4480`/`0x7F44A8`, `0x7F4598`, `0x7F546C`, `0x7F5488`, `0x7F5904`, `0x7F5988`, `0x7F5BA4`,
`0x7BFC68`, `0x7BFD0C`/`0x7BFD18`. Write the size as `count × record` (for example `15 × 0xC`); per-car and
"each" sizes are not machine-readable.

## Notes merge

Agents put machine-readable notes in `docs/engine/notes/`:
- `symbols.<agent>.csv`: the `symbols.csv` format; the comment may hold commas, as `ApplySymbols.java` reads it.
- `addresses.<agent>.csv`: ordinary CSV with the header `region,address,size,what,doc`, the address spelled as the map spells it.

Then run `python tools/notes_merge.py`, which is a dry run. It lists duplicates, the new rows, and **conflicts**: the
same address under another name, or the same name at another address, also between two agents. The exit code is 1
when there are conflicts. `--write` appends only the clean rows to `docs/engine/symbols.csv`. Address rows are
printed as map rows to paste, and flagged when the address is already in the map. Delete the notes files after
merging.

Merging the current `symbols.csv` into itself gives 244 duplicates and 0 conflicts.

## Scenario library (design)

A scenario is **a savestate plus a key script**, so it replays deterministically in mGBA: the emulator is
deterministic from a savestate (`trace_race.py` relies on this).

- **Store:** `data/work/<sha8>/scenarios/<name>.ss` and `<name>.keys`, one mgba_remote command per line (`hold KEYS N`, `wait N`, `trace NAME`, `untrace`, `shot`, `dump`). A `scenarios.md` table lists what each one exercises: mode, route, car, weather, the menus passed. Never commit them (they are save data).
- **Replay:** `python tools/mgba_ctl.py "load NAME" <commands from NAME.keys>`, with the existing remote. `coverage.py` accepts the same plans in its own step syntax, and a scenario file can feed both.
- **Record:** the physics agent's `trace NAME [SKIP]` / `untrace` (mgba_remote.lua) already writes the full EWRAM + IWRAM per car step. `trace_race.py` turns that into `<name>.ramdelta` (the runs that differ from the first step). A second recorder is not needed. Two extensions are worth adding when needed: a `trace` variant that fires every VBlank instead of every car step, and deltas against the previous step instead of the first.
- **Disk budget:** measured deltas are 4–17 KB per step (`accel` 4.1, `drive` 7.5, `long` 7.7, `start` 16.5 KB/step), against 288 KB for raw RAM. At 60 frames/s, ten minutes of play is 36,000 frames. That's about 0.3–0.6 GB against the first frame; zlib should bring it to roughly 0.1–0.2 GB, and deltas against the previous frame much less. A library of 30 one-minute scenarios therefore fits in about 1 GB.
- **Scenarios to add first:**
  - one per race mode (circuit, sprint, elimination, hunter), each with a finish;
  - a career event win (payout, unlocks, save write);
  - the garage (paint, parts);
  - a crash into a wall and a cop chase, if Carbon has one;
  - every environment once (sky, palette);
  - the pause menu options.

## Oracle gotcha (from the menus work)

`Result.read` rebuilds memory from the snapshot plus the call's writes, **not** the call's `mem=` inputs: a byte set as an input that the call leaves alone reads back as the snapshot's value. Generators should apply the writes to their own inputs.
