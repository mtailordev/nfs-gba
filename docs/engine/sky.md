# Sky (race)

The race sky has two layers, both driven by the camera. Both are **exact and verified** against the reference build (mGBA nightly): the Rust code is `nfsgba_formats::sky`, and its tests replay 15 breakpoint captures, 6 screen snaps and 22 VBlank samples byte for byte.

1. **Gradient:** the backdrop colour (BG palette entry 0) shows through every framebuffer pixel with index 0. A VCount interrupt writes the next gradient colour into palette entry 0 on every second line from line 0 to line 78. The screen shows no per-scanline buffer and no HBlank DMA.
2. **Skyline:** a 240×64 panorama, 8bpp through the city palette. `FUN_0813A318` copies it into the top of the mode-4 back buffer every rendered frame. The 3D world (`FUN_030048C8`) is then drawn over it. Skyline texels with index 0 stay 0, so the gradient shows through them.

Addresses are for `BN7E` v0, SHA-1 `e5298b24…`. "desc" is the level descriptor (`0x7F2B08 + 0x68 × env`), "world" the world struct at `0x030000C0`.

## Interrupts

- The IRQ vector (`0x03007FFC`) points to the ARM dispatcher at `0x03005810` in IWRAM.
- The dispatcher reads `IE & IF` and acknowledges the IRQ. It masks `IE` with `0x20A5 & ~irq`, switches to system mode, and calls a handler from the table at `0x030056D0`:

| Slot | IRQ | Handler |
|---|---|---|
| `+0` | mask `0xA0` (timer 2, serial) | `0x0812B1D4`: `bx lr`, a no-op in races |
| `+4` | VBlank | `FUN_0812AC14` |
| `+8` | VCount | `0x030001C0`, ARM, in IWRAM |

An IRQ with bit 13 set (game pak) hangs in a loop. In the race, `IE = 0x2005` (VBlank, VCount, game pak). DISPSTAT is `0x5029` at VBlank, meaning the VCount IRQ is on and LYC = 80.

## Gradient

**Buffer.** The gradient buffer is 0x200 BGR555 entries at EWRAM `0x0200120C`. Its pointer is at `0x030053B8`, right after the base palette buffer `0x02001008`.

`FUN_0812AE64` (race frame logic) handles fades. When the fade counter `0x03005630` is non-zero, it fades the buffer:
- **Fade in** (counter > 0): `FUN_0815E530(buf, rom, 0, 0x200, 4)` moves each 5-bit channel up by 4 towards the ROM value, then caps it at the ROM value. The counter then drops by 2, down to 0.
- **Fade out** (counter < 0): `FUN_0815E4D4(buf, 0, 0x200, 4)` lowers each non-zero channel by 4, with a floor of 0. The counter then rises by 2, up to 0.

The same call also fades BG palette RAM (`FUN_0815E2F0`/`FUN_0815E290`) and the OBJ palette (`FUN_08161838`/`FUN_081617D4`).

The ROM source is `world[0] + material[desc +0x5E].+8`, which is the gradient texels. Only the first 64 entries are the gradient (a 1×128 material). Entries 64 and up are whatever follows in ROM, and the camera can reach entry 76.

Once the fade has finished, the buffer equals the 0x400 ROM bytes (`gradient_buffer`). **Checked:** the EWRAM buffer equals the ROM in all 6 snaps.

**VBlank (`FUN_0812AC14`).** When `*0x03005620` (the desc pointer) is non-zero, the handler computes:

```
start = max(0, desc[+0x64] − (horizon >> 1) − ((shake_y >> 1) + 4))   // arithmetic shifts
*0x030056E8 = *0x030053B8 + 2·start                                      // gradient read pointer
DISPSTAT &= 0x00FF                                                       // LYC = 0
```

`desc +0x64` is 25 in all 12 environments. That gives `start` = 21 at a level horizon, and a range of 5 to 37 over horizon ±32 (`gradient_start`).

**VCount (`0x030001C0`, ARM):**

```
if VCOUNT > 79: return
PAL[0] = *ptr++          // 0x05000000
DISPSTAT += 0x200        // LYC += 2
```

Line `y` therefore shows `buffer[start + min(y, 79) / 2]` (`backdrop_entry`). Lines 80–159 and the next VBlank keep entry `start + 39`.

**Checked:**
- In 6 screen snaps, every backdrop pixel (frame index 0, not under an OBJ) matched: 18,027 pixels across the chase and bumper views, entries 21 to 65.
- In 22 VBlank samples, `start` matched the formula for horizon values from −15 to +6, in both views and with the pre-race variant descriptor.

NOT 1:1 on real hardware (reference = mGBA): the palette write lands a few dozen cycles into the line. mGBA renders each line at HBlank, so it applies the new colour to the whole line. On hardware, the left edge of every even line up to 78 would probably still show the previous entry. This timing has not been measured.

## Skyline (`FUN_0813A318`, called from the race frame `FUN_0813A954`)

The skyline is drawn only when world `+0xF6` is non-zero. It uses the skyline material `desc +0x60`: 240×64, with texels at `world[0] + material.+8`.

```
column = ((yaw & 0x3FFF) · w · 4 >> 14) % w  − shake_x      // __modsi3; 4 repeats per turn
top    = horizon + 80 − h + desc[+0x62] + shake_y (+4 if view == 0)
if clip_y0 − h < top < clip_y1:
    rows = h − 1, y = top, src = texels
    if top < clip_y0:          src += (clip_y0 − top)·w; rows −= clip_y0 − top; y = clip_y0
    elif top > clip_y1 − h:    rows −= top − (clip_y1 − h)
    if rows > 0: for each row copy 256 bytes from src + (column >> 1)·2 to fb + 240·y   (strides w, 240)
    if top > clip_y0:          fill0(fb + 240·clip_y0, 240·(top − clip_y0))
    if top + h ≤ 63:           fill0(fb + 240·(top + h − 1), 240·(64 − top − h))
else: fill0(fb, 240·64)
```

`desc +0x62` is 4 in every environment, which puts the chase-view top at row 20.

- **Row copy.** The copy is the IWRAM copy of ROM `0x0816820C`, called through the overlay trampoline `FUN_0815E738`. That trampoline calls IWRAM `*0x03006490 + 0xE4 + (fn − 0x08165218)`, so ROM `0x08165218` sits at `0x03000304`. The copy moves `(240 >> 5) + 1 = 8` blocks of 32 bytes per row, so **256 bytes** per row.
  - Reads run straight on with no wrap-around. From column c, the right edge shows the next texture row.
  - The last row spills 16 bytes into the screen row below it.
  - Only `h − 1` = 63 texture rows are drawn.
- **Fill.** `fill0` is `FUN_0816A2A4` (`bx r3`) with `r3 = *0x0300649C` = `0x030002C0`. It stores 32-byte blocks and **drops `bytes % 32`**: with an odd number of rows above the skyline, the last 16 bytes keep the page's old content.
  - If the count is below 32, it returns without popping its saved registers. That path is never reached here.

Inputs (IWRAM):

| Address | What |
|---|---|
| `0x03000210` `+4` | camera yaw, 0x4000 per turn. Camera code sets it from the player heading (entity `+0x2C >> 8`) or from `FUN_0815E690` (atan2) |
| `0x03005390` / `0x03005392` | s16 screen shake x / y; zeroed by the camera init `FUN_081378CC`; no other writer found, by literal scan or a write watchpoint over racing, crashing and both views |
| `0x030056B8` | s32 horizon shift (rows); see below |
| `0x030055F8` | camera view: 0 bumper, 2 chase (race default). SELECT switched 2 → 0 in play. `FUN_0813867C` toggles 0 ↔ 2; no direct caller was found, so it is probably called through a pointer. `FUN_081378CC` sets 4 or 5 |
| `0x030053D0` | view rect x0, y0, x1, y1 = 0, 0, 240, 159 (skyline clip uses `+4` and `+0xC`) |
| `0x03006410` | screen struct: s16 width 240, height 160 |
| `0x03000080` | view struct (world `+0x50`): `+0` draw-page pointer (`0x06000000`/`0x0600A000`), `+8`/`+0xA` projection centre, `+0x1C` focal |
| `0x03005620` | current level descriptor; `0x0300006C` = environment index (11 in the reference race) |

**Horizon shift.** In views below 2, the vehicle code `FUN_0814BC30` sets `horizon = (M[+0x1C] · −0x80) >> 14` for the player entity (`0x03000060`). Here M is the player's vehicle matrix (world `+0xFC + slot·0x30`, 2.14 fixed point), so the value follows the car's pitch. The camera update `FUN_08137CB0` then:
- clamps it to ±32;
- sets it to 0 when the view is not 0.

The same camera update sets the projection centre from the same inputs: `centre_y = (y1 − y0)/2 + horizon + (*0x030053A0 >> 8) (+8 if view == 0) + shake_y` and `centre_x = width/2 + shake_x`. The sky ignores `0x030053A0`. This paragraph (the formula, the clamp and the reset) comes from the decompile only; the rewrite takes `horizon` from its camera. Captured horizons were −7 to +2 at the skyline breakpoint, and −15 to +6 in the VBlank samples.

**Checked:** in 15 breakpoint captures of rows 0–63 at the end of `FUN_0813A318`, every byte matched and the untouched bytes numbered exactly `bytes % 32`. The captures covered:
- 13 headings, including negative yaw;
- chase and bumper views;
- horizon −7 to +2.

## Timing notes

- The skyline is drawn once per rendered frame, about every 3–4 VBlanks in the reference race. The gradient start is recomputed every VBlank, from the horizon at that moment.
- The display flips pages mid-frame, so the top of the screen can come from the other page. The backdrop test picks the matching page row by row.
- The frame order is `FUN_0813A954` → skyline → world `FUN_030048C8` (IWRAM, via `FUN_0815E674`) → HUD and sprites.

## Verification

- **Captures.** The captures and their scripts are in `$NFSGBA_DATA/work/e5298b24/sky/`:
  - `scripts/sky_probe.lua` is loaded next to `tools/mgba_remote.lua` by `scripts/launch.py start`.
  - Breakpoint `0x0813A4AA` → `NAME.sky.bin` + `.sky.txt` (on `launch.py probe NAME`).
  - Breakpoint `0x0812AC86` → `vblank.txt`.
  - `launch.py snap NAME` → screen, both pages, palette, OAM and the gradient buffer from a single frame.
- **Drive.** Load `race.ss`, hold A, then turn with LEFT/RIGHT to change heading, and press SELECT for the bumper view.
- **Tests.** The tests in `crates/nfsgba-formats/src/sky.rs` skip when the captures are missing.

## Not covered

- Screen shake is never non-zero in play, so its terms are exact to the code but were not exercised.
- The top-clip path (and with it the fill below the skyline) was not captured. It runs in the bumper view when horizon < −24, because `top = horizon + 24` there; the chase view always has top 20.
- The bottom-clip path and the whole-sky fill cannot be reached with clip rows 0/159 and horizon ±32.
- The fade-in/fade-out steps and counter are documented above but not implemented or traced.
- Hardware line timing is unmeasured (see Gradient).

## Integration notes

For the owner of `address-map.md`, `symbols.csv`, `FIDELITY.md` and the viewer.

**address-map.md (RAM):**

| Address | What |
|---|---|
| `0x03007FFC` → `0x03005810` | IRQ dispatcher (ARM); handler table `0x030056D0` (`+0` mask 0xA0, `+4` VBlank, `+8` VCount) |
| `0x030001C0` | VCount handler: palette 0 = next gradient entry, LYC += 2, lines 0–78 |
| `0x0200120C` | gradient buffer, 0x200 BGR555 (pointer `0x030053B8`) |
| `0x030056E8` | gradient read pointer (VBlank sets `base + 2·start`) |
| `0x03005630` | palette/gradient fade counter (±2 per frame) |
| `0x03000210` `+4` | camera yaw (0x4000 per turn) |
| `0x03005390` / `0x03005392` | s16 screen shake x / y (always 0 in play) |
| `0x030056B8` | horizon shift, rows (bumper view: car pitch, ±32) |
| `0x030055F8` | camera view (0 bumper, 2 chase; tables `0x7F39BC`/`0x7F39D4`/`0x7F39EC` per view) |
| `0x030053A0` | extra projection-centre y offset (8.8) |
| `0x030053D0` | view rect x0, y0, x1, y1 |
| `0x03006410` | screen struct (width, height) |
| `0x03006490` | IWRAM overlay base − 0xE4 (`0x03000220`): ROM `0x08165218` ↔ IWRAM `0x03000304` |
| `0x0300649C` | pointer to the 32-byte-block fill `0x030002C0` |
| `0x03005620` | current level descriptor; `0x0300006C` environment index |
| `0x03000080` | view struct (world `+0x50`): `+0` draw page, `+8`/`+0xA` centre, `+0x1C` focal (0x96) |
| world `+0xF6` | sky enabled |

**address-map.md, descriptor:** `+0x62` = s16 skyline row offset (4); `+0x64` = s16 gradient start at a level horizon, plus 4 (25).

**symbols.csv:**

```
0x0812ac14,vblank_irq,function,VBlank IRQ: counters; gradient pointer = base + 2*start; LYC = 0
0x030001c0,vcount_irq_sky_gradient,function,VCount IRQ (ARM): PAL[0] = *grad++; LYC += 2; lines 0-78
0x03005810,irq_dispatch,function,ARM IRQ dispatcher; handler table 0x030056D0
0x0813a318,draw_skyline,function,skyline panorama blit into the mode-4 back buffer (256-byte rows; 32-byte fill)
0x0813a954,race_render_frame,function,race frame (partly read): entities, skyline, world (FUN_030048C8), HUD
0x0812ae64,race_frame_fades,function,light tint + gradient/palette fades (counter 0x03005630)
0x0815e530,palette_fade_in_step,function,channels += step towards target, capped
0x0815e4d4,palette_fade_out_step,function,non-zero channels -= step, floor 0
0x0815e738,iwram_call_7,function,calls the IWRAM copy of a ROM ARM function (overlay base 0x03006490)
0x0816820c,blit_rows_256,function,ARM row copy: (w>>5)+1 blocks of 32 bytes per row
0x030002c0,fill32,function,ARM fill of n>>5 32-byte blocks (n<32 returns without pop)
0x0816a2a4,call_via_r3,function,bx r3
0x0813867c,camera_toggle_view,function,camera view 0 <-> 2 (SELECT)
0x081378cc,camera_init,function,camera init: view 4/5, shake = 0, yaw from player
0x08137ac8,camera_look_at_player,function,(name tentative) yaw = atan2 to the player; matrix; focal 0x96; projection centre
0x08137cb0,camera_update,function,chase/bumper camera; horizon clamp +-32; projection centre
0x0814bc30,draw_vehicle,function,(name tentative) vehicle draw; sets the horizon shift from the player's pitch in views < 2
```

**FIDELITY.md:**
- R5 and R6 are exact and verified in `nfsgba_formats::sky`. What is left is **viewer integration**: draw the backdrop per screen line, scaling y from 160 lines, with `buffer[backdrop_entry(gradient_start, y)]`, and composite the skyline from `draw_skyline` into a 240×64 index layer (or equivalent) under the world. The skyline scrolls with yaw only (column = yaw·960/16384 mod 240), not with the 3D projection. The viewer's sky cylinder should be replaced.
- New NOT 1:1 entry: hardware line timing of the palette-0 write, which is unmeasured (the reference emulator applies it to the whole line).

**Other finds for the ledger:**
- The reference race is **environment 11** (`0x0300006C` = 11, `0x03005620` = `0x087F2F80`), not 1.
- Environments 1 and 11 share palette (city palettes 3 and 13 are byte-identical) and gradient (materials 81 and 226 are identical), but their skylines differ (80 ≠ 225). The palette test's `environments(&rom)[1]` passes for that reason.
