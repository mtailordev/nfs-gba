# Audio: LS_Play music, sound effects and mixer (Carbon `BN7E`)

**Status: exact and verified.** The whole sound engine is rewritten in `crates/nfsgba-audio`. It covers the module
player, the sound effects, the software mixer and the game's sound API. It reproduces the reference build **bit
for bit**:

- every mix buffer;
- every field of the engine's 0x160C-byte work area;
- 13,800 recorded frames (19,200 frame checks).

The engine was checked in two ways: frame by frame from the game's own state, and running free from one recorded
state, fed only the API calls the game made (see [Verification](#verification)).

What is not rewritten is either unreachable in Carbon or hardware below the 8-bit sample stream. The list is in
[Not 1:1](#not-11).

The engine is Logik State's **LS_Play**, "GBAModPlay 3.0":
- The ROM string at `0x7C0360` reads "LS_Play (C) Logik State 2003".
- Modules carry the tag `GBAMOD30`.

It is a MOD-style tracker player driving 8 software-mixed channels into one 8-bit Direct Sound stream. There is
no panning: the output is mono.

## Hardware and timing

| What | Value | Where |
|---|---|---|
| Mixing rate | index 0: **10512 Hz** (Timer 0 reload `0xF9C4`, 1596 cycles a sample; 2^24/1596 = 10512.04 Hz) | reload table `0x087C0390` (u16 ×8), Hz table `0x087C03B4` (u32 ×8) |
| Samples per frame | **176** (280,896 cycles / 1596, exact) | `0x087C03A2` (u16 ×8: 176, 224, 256, 304, 352, 446, 526, 606) |
| Rates available | 10512, 13379, 15282, 18157, 21024, 26758, 31536, 36314 Hz | Carbon only uses index 0 (the rate-4 set-up at `0x081360E8` has no callers) |
| Buffers | two 176-byte buffers in IWRAM, `0x03005DEC` and `0x03005E9C` (double buffered, swapped every VBlank) | engine `+0x00`/`+0x04` |
| Output | `SOUNDCNT_H = 0xBB0E`: Direct Sound A **and** B, both 100 %, both on L+R, both on timer 0. DMA1 feeds FIFO A and DMA2 feeds FIFO B from the **same** buffer (`DMAxCNT_H = 0xB600`). `SOUNDCNT_X = 0x80` | `FUN_08151554` (mode 2), `FUN_081518c4` |
| Per frame | The VBlank handler `0x0812AC14` runs three steps. `FUN_08151928` → ARM `0x0815CCC4` restarts DMA1/DMA2 on the current buffer. `FUN_08151b10` mixes the next frame into the other buffer. `FUN_08151c48` swaps the two | |

The rewrite's output is the signed 8-bit stream the FIFOs receive: `Engine::vblank` returns 176 samples per frame. The
DAC, `SOUNDBIAS` and the sum of the two identical FIFOs are analogue-side and not modelled.

## Where things are

### ROM

| Address | What |
|---|---|
| `0x08000210` | sound-effect table: u32 count (40), then 40 × 24-byte entries |
| `0x080005D4–0x0804EA11` | sound-effect sample data (signed 8-bit PCM) |
| `0x0804EA14`, `0x0804F134`, `0x0804F8DC`, `0x08050034`, `0x080507FC` | the five `GBAMOD30` modules (music ids 0–4) |
| `0x08050FD4` | sample bank: 256 × 16-byte headers (26 used) |
| `0x08051FD4–0x0812A84B` | sample bank data (signed 8-bit PCM), in header order |
| `0x08151E34` | the loader's `"GBAMOD30"` literal (the sixth `GBAMOD` hit in the ROM; not a module) |
| `0x08153BB4` | note → period table of the linear pitch mode (unused by Carbon's modules) |
| `0x08154354` + … | per-rate note → step tables (linear mode), pointers at `0x087F5BA4` |
| `0x0815CF2C` / `0x0815CFD4` | LZ77-packed ARM mixer for mode 0 / **mode 1** (unpacked to IWRAM `0x03005A00`, 0x3EC bytes) |
| `0x087BFD40` | vibrato half sine, 32 bytes |
| `0x087BFD60` | 768 × u16 frequency table (one octave, period mode) |
| `0x087EE238` | music table: 5 module pointers (then `0x4`) |
| `0x087EE24C` | Carbon sound id → sound-effect slot, 40 bytes |

### RAM

| Address | What |
|---|---|
| `0x03006370` | engine pointer (the work area; `0x0200EE28` in the reference runs) |
| `0x03006378` | mixer-driver call counter |
| `0x0300637C` | current module state (engine `+0x5C`; `+0x83C` only while a jingle plays, never in Carbon) |
| `0x03005A00` | IWRAM block (0x54C bytes): mixer code, then the two buffers |
| `0x03005F4C` | work-area pointer (0x26AC bytes allocated); `0x03005F50` the 28-byte engine config |
| `0x0300578C` / `0x030053A4` | music / sound option (volume = option × 4, at most 63) |
| `0x0300003C` | current music id (−1 none) |

## Module format (`GBAMOD30`)

The player reads only the fields below. Everything else in the 0x650-byte header is ignored by the code.

| Offset | Meaning |
|---|---|
| `+0x000` | `"GBAMOD30"` |
| `+0x028` | music channels (module 0: 2; modules 1–4: 1). The other channels, up to 4, are for sound effects |
| `+0x034` | order count the player wraps at: **255** in every module. Songs loop through a `B00` in their last pattern |
| `+0x13C` | rows per pattern (64) |
| `+0x140` | pitch mode: 0 = periods (all Carbon modules), nonzero = linear note tables |
| `+0x144` | initial tempo: 125 (module 0), 98 (modules 1–4) |
| `+0x148` | initial speed (6) |
| `+0x14C` | order list: 256 pattern numbers |
| `+0x24C` | 256 × u32 pattern offsets, relative to `+0x650` |
| `+0x650` | pattern data |

Not read by the player (their meaning is a guess):
- `+0x038`: 256 bytes; looks like the source MOD's sample → bank mapping.
- `+0x138`: a u32.

**Pattern:** one block per music channel, one after another. Each block has a 0x14-byte header and three streams.

| Block offset | Meaning |
|---|---|
| `+0x04` | stream B (volume column) start, relative to `+0x14` |
| `+0x08` | stream C (effect) start, relative to `+0x14` |
| `+0x10` | block size from `+0x14`; the next channel's block follows |
| `+0x00`, `+0x0C` | not read |
| `+0x14` | stream A: note and instrument |

**Streams** are run-length records `(count, value)`: the value lasts `count` rows. Values are big-endian: 2 bytes in
streams A and C, 1 byte in B.
- The decoder keeps a pointer, the rows left and the value.
- On row 0 all streams restart at the block of the order's pattern.
- A record read with count < 1 is replaced by the next one, once.

Functions: `FUN_08151d8c` (2-byte streams), `FUN_08151d60` (1-byte), `FUN_0815248c` (per channel).

**Row values:**

| Stream | Value |
|---|---|
| A | bits 0–6: instrument, which is the bank sample + 1 (0 = none). Bits 7–15: note (`0x1FF` = none) |
| B | volume + 1 (0 = none). It overrides the channel volume on every row **and every tick** (`FUN_08153344`) |
| C | high byte: effect; low byte: parameter |

What the modules contain:
- Carbon's music is sample-based: note 49 of a 0.6–28 s phrase every 16 rows.
- The only effects are none (`000`) and `B00`.
- Module 0 (menus) uses the volume column (41) on its second channel.

## Sample bank and sound effects

**Bank header** (16 bytes, `FUN_08151ec4` builds the address table at engine `+0x1020`):

| Offset | Meaning |
|---|---|
| `+0x0` | length in bytes (0 = empty slot, no data) |
| `+0x4` | fine tune (halved into the period) |
| `+0x6` | default volume (64) |
| `+0x8` | loop start |
| `+0xA` | loop length (0 = no loop) |
| `+0xC` | relative note (3 for sample 0, 4 for the others) |
| `+0xE` | 0 |

Sample 0 is a 295,256-byte menu song, played by module 0. Samples 1–25 are the race phrases.

**Sound-effect entry** (24 bytes):

| Offset | Meaning |
|---|---|
| `+0x00` | data offset from `0x080005D4` |
| `+0x04` | length in bytes |
| `+0x08` | u32 `0x00400000`: its low half is written to the channel's fine tune, then cleared |
| `+0x0C` | u16 loop start |
| `+0x0E` | u16 loop length |
| `+0x10` | default rate in Hz: 10512, or 13378 for the 2-byte silent entries |
| `+0x14` | format flag; 0 in all 40 entries, so the flagged path is only used by the mode-0 mixer |

Example: sound 12 (the engine loop in the race) is at `0x08003F2C`, `0x4A57` bytes, looping.

## Engine state

**Work area** (0x160C bytes). `crates/nfsgba-audio/src/ram.rs` maps every field the rewrite keeps.

| Offset | Meaning |
|---|---|
| `+0x00` / `+0x04` | buffer addresses |
| `+0x08` | buffer the DMA plays |
| `+0x0C` | previous buffer |
| `+0x10` | −1001 when a frame starts unswapped |
| `+0x14..+0x1C` | test tone (never enabled) |
| `+0x20` / `+0x24` | active / requested rate index |
| `+0x28` | step table for the rate |
| `+0x2C` | linear period table (`0x08153BB4` once a module starts) |
| `+0x30` | IWRAM allocation cursor |
| `+0x34` | running |
| `+0x38` | mixer routine `0x03005A00` |
| `+0x44` | hold: skips mixing (never set) |
| `+0x48` | mixing rate (10512) |
| `+0x50` | music fade-in, 0–64 (+3 a frame; 0x40 when a module starts) |
| `+0x54` | jingle active (u8) |
| `+0x5C` | module state (0x7E0 bytes) |
| `+0x83C` | second module state (jingles) |
| `+0x101C` | sample bank |
| `+0x1020` | 256 sample addresses |
| `+0x1420` / `+0x1424` | sound-effect entries / data |
| `+0x1428` / `+0x142C` / `+0x1430` | master / music / sound volume (64 / 63 / 63 at option 16) |
| `+0x1438` | sound-effect count (40) |
| `+0x143C` | music playing (u16) |
| `+0x1440` | loop music (1) |
| `+0x1444` | most sound-effect channels (4) |
| `+0x1448` | rate index at init |
| `+0x144C` | buffer allocation |
| `+0x1450` | samples per frame |
| `+0x1474` | mixer mode (1) |
| `+0x1478` | module to start at the next update |
| `+0x147C` / `+0x1480` | jingle request / return |
| `+0x1484` / `+0x1488` | last mixer call's output address and length |

**Module state** (engine `+0x5C`):

| Offset | Meaning |
|---|---|
| `+0x00` | module |
| `+0x04` | linear mode |
| `+0x08` | rows |
| `+0x0C` | "B ends pattern" (never set) |
| `+0x14` | speed |
| `+0x18` | tempo lock (never set) |
| `+0x1C` | ticks per second = `tempo·50·65536/125 >> 16` |
| `+0x20` / `+0x24` | samples to next row / per row = `speed·rate/tick_hz` |
| `+0x2C` / `+0x30` | samples to next tick / per tick = `rate/tick_hz` |
| `+0x38` | tick in row |
| `+0x3A` | arpeggio tick |
| `+0x3C` | tempo |
| `+0x40` | row |
| `+0x44` | order position |
| `+0x48` | song loops |
| `+0x4C` | pattern break done (u8) |
| `+0x50` | pattern data |
| `+0x54` | 8 channels × 0x98 |
| `+0x514` | 8 pattern decoders × 0x58 |
| `+0x7D4` / `+0x7D8` / `+0x7DC` | music / sound / total channels |

The pattern decoders hold three streams, at `+0x00`, `+0x20` and `+0x30`, and the decoded cell at `+0x50..+0x55`.

**Channel** (0x98 bytes). The first `+0x7D4` channels belong to the module; the next `+0x7D8` are sound slots 0–3.

| Offset | Meaning |
|---|---|
| `+0x00` | sample address; it moves to the loop start at the first wrap |
| `+0x04` | position (20.12) |
| `+0x08` | step (20.12) |
| `+0x0C` | end (20.12); the loop length after the first wrap; 0 = stopped |
| `+0x10` | loop start in bytes |
| `+0x14` | loop length (20.12) |
| `+0x18` | volume |
| `+0x1C` | sound's set volume |
| `+0x3C` | relative note |
| `+0x40` | sample index / sound id |
| `+0x44` | fine tune |
| `+0x46` | note, then the period (period mode); `0xFFFF` for sounds |
| `+0x48` | running effect (−1 none) |
| `+0x4C`/`+0x4E` | volume slide up/down |
| `+0x50`/`+0x52` | arpeggio |
| `+0x54`..`+0x5A` | vibrato: offset, depth, speed, position |
| `+0x5E` | pitch offset (linear mode) |
| `+0x60` | portamento speed |
| `+0x62` | slide |
| `+0x64` | portamento note |
| `+0x78`/`+0x7A` | pattern loop row/count |
| `+0x7C` | sound format flag |
| `+0x80` | period slide |
| `+0x84` | vibrato period offset |
| `+0x88` | last slide parameter |
| `+0x8C` | volume column |
| `+0x94` | portamento target period |

## Player

### Per frame (`FUN_08151b10`)

1. **Rate change** (`FUN_081519d0`), if one was requested. This never happens in Carbon. Then the music fade-in steps by +3, up to 64.
2. **Pending music.** `FUN_0815176c` loads a requested module (`FUN_08151e04`) and starts it (`FUN_081521b4`). The start zeroes the countdowns, so the first mix runs row 0 and tick 0 at once.
3. **Clear.** With no music playing, the back buffer is cleared. With music playing it is not: segments where no channel has a sample would keep old data.
4. **Mix** 176 samples (`FUN_08152660`).

### Mixer driver (`FUN_08152660`, mode 1)

The buffer is filled in segments. Each segment runs through these steps:

1. **Row and tick.** If the row countdown is 0, run a row. If the tick countdown is 0, run a tick.
2. **Segment length.** A segment lasts until the next row or tick, clipped to the buffer.
3. **Voice list.** Build a voice for each channel with a sample address:
   - The address is `sample + pos>>12`. The fraction is `pos<<20`. The step splits into `step<<20` and `step>>12`.
   - An end below 2 zeroes the volume.
   - Music volume is `(((vol·music>>6)·master>>6)·fade)>>6`. Sound volume is `((vol·sound>>6)·master)>>6`.
4. **Cut at sample ends.** If a channel would pass `end − 0x1000`, the segment is shortened by `Div(pos + n·step − end, step)`.
5. **Mix.** Call the ARM mixer with the voice list.
6. **Advance.** Each channel moves by `n·step`. On passing `end`, the channel jumps to the loop:
   - `sample += loop_start`
   - `pos −= end`
   - `end = loop_len`
   - `loop_start = 0`

   A sample without a loop stops there.

Rows and ticks are **independent sample countdowns**. At tempo 98 a row is `6·10512/39 = 1617` samples and a tick is
`10512/39 = 269`. So six ticks (1614 samples) are not a row, and tick phase drifts against rows. The tick-in-row
counter resets on each row. Module 0 (tempo 125): rows of 1261 samples, ticks of 210.

### Row (`FUN_08152550`)

1. Decode a cell per music channel.
2. `row += 1`.
3. Trigger each channel (`FUN_08153654`).
4. At `row == rows`:
   - `order += 1`.
   - At the order count, go back to 0 and count a loop; the music stops if looping is off.
   - `row = 0`.

### Note and instrument (`FUN_08153654`)

**Instrument only:** sets the sample address and the default volume. It does **not** restart the sample.

**Note:**
1. Loads length, loop, fine tune and relative note, and resets the position.
2. Period mode:
   - `period = (0x1DC0 − ((rel_note + note − 2) & 0xFFFF)·64 − finetune/2) & 0xFFFF`
   - `freq = FREQ[(0x1E00 − p) mod 0x300]·4 >> (7 − (0x1E00 − p) div 0x300)`
   - Here `p = period + period_slide + vibrato`, and both divisions are BIOS `Div`.
   - `step = Div(freq << 12, 10512)`.
   - Carbon's note 49 with relative note 4 gives period 4352, 10536 Hz, step `0x1009`.
3. Linear mode: step from the rate's note table, or `Div(0x6C3E1D, 2·period)` rescaled.

**Effects**, applied on the row and then per tick in `FUN_08153354`:

| Effect | What it does |
|---|---|
| `0xy` | arpeggio. It is stored, but never changes the pitch. In period mode the tick indexes the note table with the period (a bug, reproduced) |
| `1xx` / `2xx` | portamento up / down: ×4 per tick; `Fx` fine ×4 and `Ex` extra fine on tick 0 |
| `3xx` | tone portamento |
| `4xy` | vibrato: half-sine table, depth·sine>>5, speed `(x+4)>>1` per tick |
| `5xy` / `6xy` | tone portamento / vibrato plus volume slide |
| `Axy` | volume slide (not on tick 0, clamped 0–64) |
| `Bxx` | jump to order `xx` at the end of the row |
| `Cxx` | volume |
| `Dxx` | jump to row `xx` of the next order. The pattern streams are not restarted (a quirk, reproduced) |
| `E6x` | pattern loop (no other `E` sub-effect) |
| `Fxx` | speed (`< 0x20`) or tempo |

`7`, `8` and `9` do nothing.

### Mixer (ARM, `0x03005A00`)

The input is a list of 5-word voices, ended by one with volume `0xFF`. The mixer writes whole bytes; it does not
add to what is there. Output sample = `(Σ sample·volume) >> 8`, cut to 8 bits with no clipping.

Quirks, all reproduced by `mixer::mix`:
- **Packed accumulators.** Each group of four samples uses two registers: samples 0 and 2 share one, 1 and 3 the other, as `low + high<<16`. A negative low sum therefore borrows from the high sample's byte.
- **Carry chains.** The position is a 32-bit fraction plus an integer step, chained through the carry flag. The first voice of each 16-sample block starts with carry set.
- **Ragged edges.** Up to 3 bytes before a word boundary, and the last `n & 3` bytes, are mixed one at a time. The trailing loop moves every voice one byte more per sample (`ldrsb [r3], #1`).
- **Partial blocks.** 4–12-sample blocks patch a branch into the unrolled loop, and patch `stm` to store fewer words.
- **Skipped voices.** A voice with volume 0 is skipped and does not advance.

### API (engine) and Carbon's wrappers

| Engine | Rust | Carbon wrapper (`0x08135xxx`) |
|---|---|---|
| `FUN_08151f78` init | `Engine::new` | `0x08135df8` / `0x08135ea4`. The config is IWRAM `0x03005A00`/0x54C, work 0x2000, bank `0x08050FD4`, sounds `0x08000210`, mode 1; rate 0, 4 sound slots. Master 64, music and sound volume `option·4` (≤ 63), loop on, `SOUNDCNT_X = 0x80` |
| `FUN_08151758` play module | `play_module` | `0x08136054`: music id → `0x087EE238`, skipped if already current |
| `FUN_0815240c` stop music | `stop_music` | `0x0813609c` |
| `FUN_08152e40` play sound | `play_sfx` | `0x08135fdc`. The slot comes from `0x087EE24C`. The volume is `option·4`; ids `0x20`–`0x21` use `option`, ids `0x0C`–`0x10` use `option·5`; all capped at 63. One direct call is at `0x08145d6e` |
| `FUN_08152f44` / `FUN_08152f88` | `set_sfx` / `stop_sfx` | `0x08136028` stop sound; `0x08135f38` / `0x08136178` stop all 4 slots |
| `FUN_08152fb8` / `FUN_08152fec` | `set_sfx_channel_volume` / `set_sfx_rate` | `0x08135fb0`; `0x081360b4` (rate = `x·8` Hz: the engine pitch) |
| `FUN_08153034` playing? | `sfx_playing` | `0x081360dc` |
| `FUN_081518d0`/`e4`/`f8` | volumes | `0x08135f94`, `0x08135fc4` |
| `FUN_081516bc` shutdown | `shutdown` | `0x08135f68` |

In a race the game calls `set_sfx_rate(1, engine Hz)` (slot 1 plays sound 12, the engine loop) every 4 frames. It
also calls `set_sfx(0, 0, 0)` + `stop_sfx(0)` on slot 0 every 4 frames. Pausing calls `stop_sfx` on slots 0–3,
then `stop_music`, then `play_module(module 0)`.

## Verification

Tools:
- `tools/audio_trace.py STATE FRAMES [NAME [KEYS]]` runs mGBA with `tools/mgba_audio_trace.lua`.
- The script loads a savestate and breaks at `FUN_08151b10` (tag 0) and at its return `0x0812AC4E` (tag 1).
- At each break it records the 16 bytes of globals at `0x03006370`, the whole work area and both buffers.
- Between updates it logs every call into the engine API (tag 2: function and r0–r3).
- Traces go to `$NFSGBA_DATA/work/e5298b24/audio/*.trace`.

Tests (`cargo test -p nfsgba-audio`; about 2 s):

| Test | What it checks |
|---|---|
| `race_frames`, `main_menu_frames` | 5,400 and 1,800 frames loaded from the game, one update each. Buffers and the whole work area are equal after the call |
| `race_replay` | 5,400 frames of free running from one state, with the game's 4,008 API calls: sound rate, volume and stop |
| `race_drive_replay` | 3,600 frames with throttle and steering: 16 `play_sfx` calls, 2,638 others |
| `main_menu_replay` | 2,400 frames of menu navigation: 4 menu sounds |
| `race_pause_replay` | 600 frames: pausing stops the race sounds and music and starts module 0 from its first row |
| `parses_all_modules` | the 5 modules, their orders and patterns, the sixth `GBAMOD` tag, and that every song loops with `B00` |
| `sample_bank_and_sfx_table` | 26 bank samples ending at `0x0812A84B`, 40 sounds |
| `new_matches_the_game_setup` | `Engine::new` against the game's set-up fields |

Across the traces:
- rows, ticks and note triggers (one every 16 rows);
- the order wrap through `B00`;
- a module load and start;
- the volume column;
- sample loops and ends;
- misaligned segment starts and ends (head and tail loops).

Changing any of the following makes the tests fail: the packed-accumulator borrow, the tail loop's extra byte,
or the cut at sample ends.

Renderer:
- `cargo run -p nfsgba-audio --release --bin nfsgba-audio-render -- music 4 30 OUT.wav` renders music 4.
- `sfx ID OUT.wav` renders a sound; `samples DIR` extracts the raw samples.
- The output is 8-bit WAV at 10512 Hz. Keep it under `$NFSGBA_DATA/out/`; never commit it.

## Not 1:1

These are all unreachable with Carbon's data or code, or below the sample stream:

- **Implemented from the disassembly, never exercised by Carbon's data:**
  - effects 0–6, A, C, D, E6x, F;
  - linear pitch mode;
  - rate change;
  - the first-voice carry;
  - the mixer's zero-address checks;
  - volumes of 0xFF and above.

  They are exact by reading, not by trace.
- **Jingle system not rewritten.** This is the second module state at `+0x83C`: `FUN_081517e8`, `FUN_081522b8`, `FUN_08152310` and `FUN_0815236c`. Nothing in Carbon calls the entry points.
- **Mode-0 mixer not rewritten** (`0x0815CF2C`, `FUN_08152ab8`, `FUN_08152b7c`), nor the flagged sound format. Carbon uses mode 1.
- **Test tone not rewritten** (`FUN_08151aa8`): engine `+0x14` is never set.
- **Division by zero** returns mGBA's HLE result (the BIOS would hang). It cannot happen with Carbon's data.
- **Analogue output not modelled.** Two identical FIFOs, DAC and `SOUNDBIAS`. WAV files say 10512 Hz; the hardware runs at 10512.04 Hz.
