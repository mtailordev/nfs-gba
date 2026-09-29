# 2D layer: menus, HUD, fonts, text (Carbon `BN7E`)

**Status: formats, drawing primitives and the race HUD logic are exact and verified**; per-screen menu logic is located but not ported (see *Not 1:1*). Code: `crates/nfsgba-formats/src/ui.rs` (6 tests) and `hud.rs` (HUD logic, 13 tests replaying 11 mGBA traces). Export: `python tools/ui_export.py` → `$NFSGBA_DATA/out/ui/<sha1-8>/` (`--check` runs a self-check).

The whole 2D layer is software-composited into the mode-4 frame buffer (240×160, 8bpp, two pages), plus OBJ sprites for the race HUD. No BG tile layers are used. ROM offsets are file offsets; functions are Ghidra `FUN_` addresses.

## How the game reaches it

The game loads a *level descriptor* into the world struct (`0x030000C0`). Menus and races use the same layout (`docs/engine/address-map.md`):

| Descriptor field | Race levels (`0x7F2B08` + 0x68·k, all 12 identical here) | Menu descriptor `0x7F2FE8` |
|---|---|---|
| `+0x04` → world `+0x34` | `0x36C75C` 4 OBJ palettes (256 colours); level `+0x58` picks one (0 for all) | `0x33EF14` menu palettes |
| `+0x10` → world `+0x08` | `0x347B74` HUD sprite texels (4bpp) | `0x16C244` menu texels (the LZ77 bank start) |
| `+0x24` → world `+0x28` | `0x36CF5C` HUD materials (280) | `0x345114` menu materials (273) |
| `+0x2C` → sprite screen `+0x10` | `0x36F6BC` 4 sprite screens | `0x347778` 10 sprite screens |
| `+0x30` → sprite screen `+0x0C` | `0x36F6DC` 185 sprite elements | `0x3477C8` 47 sprite elements |

`menu_scene_setup_a/b` then overrides world `+0x00` (texels = `0x16C244`), `+0x20` (materials = `0x345114`), `+0x30` (palette = `0x33EF14 + p × 0x200`) and `+0x34` (`0x33F114`, palette 1).

## Materials (0x24 bytes, self-indexed)

Same record as the city and vehicle materials: `+0x00` index, `+0x02` kind, `+0x04` aux (city column map; stale bytes in the sprite tables), `+0x08` offset from the texel base, `+0x0C/+0x0E` width, height, `+0x1E/+0x1F` log2 w/h, `+0x20` palette bank (used when a sprite element uploads the material).

Kind bits: **6 (0x40)** packed (see below; menus, vehicles); **2 (0x04)** 4bpp in GBA OBJ tile order (HUD); **4 (0x10)** set on HUD materials 5–33 (meaning unknown). Kind 0 = raw, row-major (8bpp in the menu and vehicle banks, 4bpp in the HUD bank, e.g. HUD material 135, the 512×384 minimap, read linearly by `FUN_08142440`).

The tables tile the ROM exactly: menu texels end at `0x33EF14` (menu palettes, 49 × 0x200), which end at `0x345114` (menu materials); HUD texels end at `0x36C75C` (OBJ palettes, 4 × 0x200), which end at `0x36CF5C` (HUD materials, to `0x36F6BC`), then HUD screens and elements to `0x370550` (vehicle texel base).

## Packed images: the game's decompressor

`FUN_08163d30(src, dst)` allocates 0x1011 bytes and runs an ARM routine (ROM `0x169208`, IWRAM copy, called through `FUN_0815e6c8`). It reads the **BIOS LZ77 bit stream** (`10 ss ss ss`, MSB-first flags, references of 3–18 bytes back 1–4096) but decodes through a **4 KiB ring** that starts at 0xFEE with bytes 0..0xFED set to **0xFF**. So:

- References before the start of the output read 0xFF. **15 menu blobs rely on this** (e.g. materials 2, 66–82); the BIOS decoder (and `nfsgba_formats::lz77`) cannot decode them. All other packed streams decode identically either way (test).
- It writes the **header size**, which is 8 more than the stream encodes; the last 8 bytes come from decoding whatever follows (usually the next blob's header). If the size runs out inside a reference, the routine does not stop until the next literal (ported as is). Images use only the first `w × h` bytes, which the stream fully encodes.
- Ring bytes 0xFEE..0xFFF are uninitialised heap; no stream in the ROM can reach them (test).

`ui::unpack` is an exact port; `ui::unpack_lowest_reference` finds streams that need the ring fill.

## Menu bank (materials at `0x345114`, texels `0x16C244`)

| Materials | What | Palette (index into `0x33EF14`) |
|---|---|---|
| 1–5 | 240×160 backgrounds: garage (menus), EA logo, name-entry keyboard, language flags, title art | = material (menu page table; 1, 2, 3, 4, 5 checked against dumps) |
| 6 | 180×84, packed (unidentified) | unknown |
| 7–11 | health and safety screens, En/Fr/De/It/Es | 1, with colours 0–3 replaced from `0x7E5E48` by `FUN_081315a0` (state 0x2F); colour 4 animated (`FUN_081318e4`) |
| 12, 13, 14 | font glyphs: 13×(224·17), 13×(224·13), 11×(224·12), raw 8bpp | any (values 0–7 plus the draw colour) |
| 16–82 | 64×64 packed (unidentified) | unknown |
| 83–142, 143–152 | 48×48 and 32×20 packed (unidentified) | unknown |
| 156–161 | button icons A, A′, B, B′… (16×16) for the prompt bar | shared colours |
| 181–185 | highlighted flags (70×62 … 72×60), one per language (181 = UK, verified) | 4 |
| 191–195 | PRESS START (191 = English, verified; the others per language, hypothesis) | 5 |
| 196–200 | copyright line, 240×10 (196 = English, verified; others per language, hypothesis) | 5 |
| 201, 202 | title logo (146×38; 202 is drawn) | 5 |
| 203 | LICENSED BY NINTENDO (144×8) | 5 |
| 218 | 512×384 city map | 7 (`menu_scene_setup(0xDA, 7)`) |
| 226–266 | 41 story screens | material − 218 (story table `0x7E78B8`) |
| others | small overlays and bars | unknown (exported with palette 1) |

**Menu page records** (0x14 bytes; tables at `0x7E5090` ×5, `0x7E553C`, `0x7E5DA8` ×8): `+0x00` title text key, `+0x02` accept prompt key, `+0x04` back prompt key (0xFFFF = none), `+0x06` background material, `+0x08` palette, `+0x0A` menu sprite screen, `+0x0C` 0xFFFF, `+0x0E` 0, `+0x10` item list pointer. Example: `0x7E5DF8` = language screen (material 4, palette 4, prompt TEXT_SELECT). The rest of the menu data (`0x7E4A00–0x7E6EEC`) is not decoded.

**Blitting.** `FUN_08136d74(world, material, x, y)` unpacks a kind-0x40 material into `*0x030057F0 + 0x9608`, then `FUN_08164d90(dst, stride, src, w, h, key=0)` copies it, skipping key bytes, through 16-bit read-modify-write, **without clipping** (the title logo at x 96, 146 wide, wraps two columns into the next row; verified). Port: `ui::blit`.

**Prompt bar** `FUN_0812bd60(accept, back, extra)`: accept text in font 0xE right-aligned at (238, 147), then icon 156 (157 if profile `+0x340` ≥ 1) at (221 − text width, 143); back/extra prompts use icons 158–161 at x 1 and text at x 18 (rows 0x84/0x87 or 0x8F/0x91/0x93 depending on which prompts are present).

## Fonts and text

Four descriptors of 0x18 bytes at `0x7EE974` (font ids 0xC..0xF):

| Offset | Meaning |
|---|---|
| `+0x01` | bit 0: skip glyph pixels equal to `+0x02` |
| `+0x02` | transparent key (0) |
| `+0x03` | s8 spacing after every character (−1, −1, −1, 0) |
| `+0x04/+0x06` | glyph cell w × h (13×17, 13×13, 11×12, 11×12) |
| `+0x08` | line height of the wrapping renderer (15, 12, 11, 9) |
| `+0x0C` | glyph data, filled at run time with a menu material |
| `+0x10/+0x14` | pointers to 224 widths and 224 signed y offsets |

Glyph material: ids 0xC..0xE → materials 12..14; id 0xF → material 14 (offset 0x1F8 hard-coded in `FUN_08141578`, `FUN_08141840`, `FUN_081419c0`, `FUN_08141b40`, `FUN_08141c88`) but **material 12** in `FUN_081416d0` (offset 0x1B0). `FUN_08141dd0`/`FUN_08141f00` read city materials 12–15 instead and have no callers.

**Character set.** Bytes map through `0x7F5BC8` (256 bytes): 0x00–0x1F → 0xFF (no glyph, advance = spacing only); c → glyph c − 0x20 otherwise, except 0x8E (Ž) → glyph 0x64 (the empty glyph of 0x84). The glyphs are **Windows-1252**, with `{` (0x7B) and `|` (0x7C) drawn as the **A and B buttons**; 0x7D–0x7F and most of 0x80–0xBF have width 0. `~` (0x7E) advances by glyph 0's width without drawing. `ui::decode_text` decodes accordingly (`nfsgba_formats::text` decodes Latin-1).

**Drawing** (`ui::Font::draw`, exact): `FUN_08162860(font, string, fb, stride, x, y, align, colour)`: align 1 centres (x − width/2), −1 right-aligns (x − width); draws only if 0 ≤ y ≤ 159 − h and x < stride. Per glyph `FUN_08162720` writes `pixel + colour` for `widths[g]` columns × h rows at y + `y_offsets[g]`, through 16-bit read-modify-write (a carry past 0xFF on an even byte leaks into the odd byte; ported). Width: `FUN_08163080` (the same sum).

**Word wrap** (`ui::Font::draw_wrapped`, exact): `FUN_08163278` (range check) → `FUN_08162a1c(font, string, dst, stride, max_width, max_lines, colour)`. Words end after a space (measured without it); a word that would pass `max_width` starts a new line (`+0x08` rows down, x back to the start); `\n` forces one; returns lines advanced.

**Front ends.** `FUN_08141578(font_id, key_or_ptr, x, y, align, colour)` draws into the current page and returns the width; `FUN_081416d0` draws into both pages (`*0x03006410` `+0x0C`/`+0x10`); `FUN_08141840` (and `…19c0/1b40/1c88`) wrap into both pages. A second argument ≤ 0xFFFF is a text key, resolved in the language `*0x03005600` through a jump table.

## HUD (race)

**Graphics.** 280 materials at `0x36CF5C`, 4bpp texels at `0x347B74`. OBJ palette 0 at `0x36C75C` (16 banks); every HUD frame `hud_minimap_palette` (`FUN_081430c4`) writes bank 13 (OBJ colours 0xD0–0xDF) straight to OBJ palette RAM with minimap palette `0x7F44F8 + 0x20 × byte[0x7F4598 + route − 1]` (5 palettes). Material `+0x20` is the bank.

**Sprite screens.** `0x36F6BC`: 4 × {s16 x, s16 y, u16 first element, u16 count} = elements 0–46, 47–93, 94–148, 149–184. Race init selects screen 0 for race modes 0/1, 2 for mode 2, 1 for mode 3 (`*0x030056E0`); screen 3's use is unknown. **Elements** (0x14): `+0x00/+0x02` s16 x, y; `+0x04` first material; `+0x06` OBJ tile slot; `+0x0C` index in screen; `+0x0D` shape/size code 0–11 (`FUN_08160f10`); `+0x11` s8 palette (−1 = 256 colours); other bytes unknown (`+0x0F`, `+0x10` vary).

**Objects** (0x10, array at sprite screen `+0x14`, world `+0xB8`; `0x0201F828` in the reference race): `+0x00` flags (bit 0 visible, bit 1 semi-transparent), `+0x02/+0x04` affine scale, `+0x06` frame (added to the material), `+0x08` frame in VRAM, `+0x0A/+0x0C` y/x offset, `+0x0E` angle (0x4000 = turn).

**`FUN_08161c24(screen, init)`** writes the shadow OAM (`0x030064F0`, copied each frame); element k → OAM entry 55 − k. Visible: x = elem x + screen x + dx (attr1 bits 0–8), y likewise (attr0 low byte); hidden: y = 160, affine off. Init: tile = slot + `*0x030064E0` (0x200 in mode 4), shape/size, 256-colour bit, palette `+0x11`, uploads the first material. Frame: if frame changed, set palette bank from material `+0x20` and upload material + frame (`FUN_08161a98`: n × 32 bytes to `0x06010000 + 32 × (slot + base)`); mode 1 if flag bit 1; affine if angle or scale set: matrices 1, 2, … in element order, `FUN_0816144c`: pa = sx·cos, pb = sx·sin, pc = −sy·sin, pd = sy·cos (>> 14; scale 0x100 when angle is set and a scale is 0), sin from the half-wave table `0x7C05F0` (`FUN_0815f948`, cos = sin(a + 0x1000)). Port: `ui::update_sprites`.

**Screen 0 elements** (update functions called by `FUN_081421b8`/`FUN_0814228c`/`FUN_08142360` via `FUN_08142f84`):

| Elements | OAM | Material(s) | Shows | Driven by |
|---|---|---|---|---|
| 0–2 | 55–53 | 120+lang, 125, 126 | timer panel | `FUN_081431f0` (frame = language) |
| 3, 4 | 52, 51 | 104, 105 | separators (4: frame 3 for Italian) | `FUN_081431f0` |
| 5–10 | 50–45 | 109+digit | race time mm:ss.cc = frames·100/60; blinks from 59:49.98; past 59:59.98 it sets race state 8 (time limit) | `FUN_081428c0` (`*0x03005800` frames) |
| 11–13 | 44–42 | 109+, 119 | lap n / total (`*0x030056E4`), shifted by `0x7F4378[lang]` | `FUN_08142c4c` |
| 14–16 | 41–39 | 127+lang, 132, 133 | split panel | `FUN_08143210` |
| 19, 20 | 36, 35 | 43+n | position / racers (`*0x03005784` + 1) | `FUN_0814306c` |
| 21–27 | 34–28 | 102+sign, 109+digit | split time and sign | `FUN_081429c4` (`*0x0300615C`) |
| 28–32 | 27–23 | 71, 72, 75, 79, 85 | dial; 29–31 take frames from driver `+0x4C8`·9/0x50000 | `FUN_08142ca8` |
| 33 | 22 | 34+gear | gear (driver `+0x40`) | `FUN_0814308c` |
| 34–36 | 21–19 | 92+digit | speed: driver `+0x44`/0x163C, × 256/411 (mph) when `*0x03000040` = 0 | `FUN_08142724` |
| 37 | 18 | 91 (rotated) | needle: angle = \|driver `+0x3C`\|·0x1C00 (or 0x2000 per profile `+0x402`)/driver `+0x454` − 0x1770 | `FUN_08142bd8` |
| 38–42 | 17–13 | 134, 66–63 | minimap (a 64×64 window of the 512×384 map material 135 copied into 134's tiles and rotated by the OBJ affine matrix) and the dots of racers 0–3 (objects 42–39) | `FUN_08142440` |
| 43, 44 | 12, 11 | 136+, 265+ | portrait (frame `*0x030061DC`) and 8-step bar | `FUN_08142b44` |
| 45 | 10 | 273+ | animated arrows, frames 0–2 or 3–5 by sign | `FUN_081431a8` |
| 46 | 9 | 33 | message (big icon) | message table below |

**Messages.** `FUN_08142ec0(msg, time, …)` looks up a 16 × 4-byte table per race mode (`0x7F437D` modes 0/1, `0x7F43FD` mode 2, `0x7F43BD` mode 3: slot, element, frame), stores it in 6 slots at `0x03006210`; `FUN_08142dec` counts down and blinks them; only message 2 is defined (element 46, or 54 in mode 2). Details in *HUD logic* below.

## HUD logic (exact: `hud.rs`)

Every race frame, `race_frame_update` calls `hud_update(S)` (`0x08142f84`; S = the race sprite screen struct `0x03000164`) and then `sprite_screen_update(S, 0)`. `hud::update` ports the first, `ui::update_sprites` the second.

**`hud_update`:**
1. `hud_minimap_palette`: OBJ palette 0xD0–0xDF = the minimap palette of route `*0x03005388`, every frame.
2. HUD on (`*0x03005698` ≠ 0): mode 0 or 1 → `hud_update_mode0`, 2 → `hud_update_mode2`, 3 → `hud_update_mode3` (identical to mode 0). HUD off: only `hud_timer` (modes 0–3), for its time-limit side effect.
3. `hud_message_tick`.

The mode updates call, in this order, with the driver `*(entity[*0x030057F8] + 0x8C)`: language panel (objects 0, 4), split panel (14, 17, 18), timer (5–10), split (21–27), position (19, 20), lap (11–13), needle (37), speed (34–36), gear (33), minimap (38–42), portrait (43, 44; mode 2: 51, 52), dial (29–31), mode 2 only the hunter bars (`FUN_08142674`, 43–50), arrow (45; mode 2: 53).

**Inputs** (`hud::Globals`, `Racer`, `Driver`):

| Input | Where |
|---|---|
| HUD setting, race mode, language, units | `0x03005698`, `0x030056E0`, `0x03005600`, `0x03000040` (0 = mph) |
| race time (frames), split (frames) | `0x03005800` (counted by the VBlank IRQ), `0x0300615C` (`route_gap` `0x0813ebac`, `race_start_setup` `0x0813e430`) |
| opponents, AI cars, laps | `0x03005784`, `0x030057EC`, `0x030056E4` |
| wingman; portrait frame, blink; bar value, full scale | `0x03006104`; `0x030061DC`, `0x030061D4`; `0x030061E4`, `0x03006188` (written by `FUN_0814078c`…`FUN_081413b0`) |
| arrow direction | `0x0300601C` (written by the car update `FUN_0813d1f0`) |
| route (minimap palette), shown entity | `0x03005388`, `0x030057F8` |
| needle scale | profile `+0x402` (`*0x030056EC`) |
| racers 0–3 | entity `+0x0C` x, `+0x14` z, `+0x2C` heading, `+0x8C` driver |
| driver | `+0x3C` revs, `+0x40` gear, `+0x44` speed, `+0xA8` position, `+0xC5` laps left (s8), `+0x454` rev scale, `+0x4C8` dial, `+0x4D8` flags (bit 3 eliminated), `+0x4E8` hunter life |
| written | race state `0x03000048` := 8 and `0x030000AC` := 1 past 59:59.98; split clamped to 0 |

**Divide.** The digits go through the IWRAM routine `0x03000220` (ROM copy `0x08165134`), called through `*0x03006494` with the remainder stored at `0x03006480`: shift-and-subtract on the magnitudes, the quotient signed by `n ^ d`, the remainder `n − |q|·d` (off for negative `n`; a negative `d` is mishandled but never passed). Port: `hud::divmod`. Elsewhere `__divsi3` (truncating).

**Elements:**
- *Language panels:* object 0 = language; object 4 = 3 in Italian, else 0. Object 14 = language; objects 17 and 18 dx = `0x7F4378[language]`; object 18 = 3 in Italian, else 0.
- *Timer:* cs = frames·100/60. Past 0x57E3E (59:59.98) the race state becomes 8 (once). With the HUD on: divmod(cs, 100) → divmod(seconds, 60) → tens and ones of minutes, seconds and hundredths into 5–10; past 0x57A56 (59:49.98) objects 5–10 are visible only while `(frames >> 3) & 1`.
- *Split:* converted to cs **before** the split is clamped to 0; object 21 = 0 when in first place, else 1 (sign); digits in 22–27; all 7 objects dx = `0x7F4378[language]`.
- *Position:* objects 19 and 20 first copy their frame into `loaded`, then take the position and opponents + 1.
- *Lap:* laps − laps left + 1, capped at laps with an **unsigned** compare; sprints show 1 / 1; objects 11–13 dx = the digit shift.
- *Needle:* dx = dy = 0; angle = divmod(|revs|·(0x2000 when profile `+0x402`, else 0x1C00), rev scale).q − 0x1770, and 0 becomes 1.
- *Speed:* speed / 0x163C; in mph divmod(v·256, 411).q; three digits; angles 0.
- *Dial:* v = clamp(dial·9 / 0x50000, 0, 8); object 29 = 2 at 8, 1 at 7, else 0; object 30 = min(v, 3); object 31 = clamp(v − 2, 0, 5).
- *Portrait* (wingman ≠ 0): frame `0x030061DC`, hidden while `0x030061D4` ≠ 0 and `(frames & 63) > 44`; next object = clamp(bar·8 / full scale, 0, 7). No wingman: frame 0, both hidden.
- *Hunter bars* (mode 2): racer i (entities 0–3) → objects 43 + 2i and 44 + 2i, hidden when i > AI cars (with a wingman: i ≥ AI cars); v = clamp(life·28 / 2^19, 0, 28) → (v, 0) up to 12, else (12, v − 12).
- *Arrow:* 0 → hidden, frame 0; else visible with frame `(frames >> 4) % 3`, plus 3 when negative.

**Minimap** (`hud_minimap`; always centred on entity 0, not on `*0x030057F8`):
- Object 38: frame 0, angle = (−heading) >> 8, dx = dy = −2. The rotation is the OBJ affine matrix; only the tiles are written.
- Window origin (cx + 0x86, cz + 0x79) with cx = (x >> 8) / 499, cz = ((−z) >> 8) / 499, clamped to 0..0x1C0. The clamped-off amount offsets the dots: x < 0 → off_x = x, x > 448 → off_x = 448 − x, **y < 0 → off_x = y** (a game bug), y > 448 → off_y = 448 − y.
- Copy: `iwram_call_4` index (0x08169AAC − 0x08165218) / 4 → IWRAM `0x03004B98` (ARM, ROM copy `0x08169AAC`). Destination: the element's OBJ tiles (`FUN_08161a34`: `0x06010000 + 32·(slot + *0x030064E0)` when DISPCNT bit 6, else 0). Source: HUD texels + material 135's offset + y·256 + (x & ~7) / 2, byte shift s = (x >> 1) & 3. For 64 rows it reads 9 words and writes `(w[k] >> 8s) | (w[k+1] << (32 − 8s)) & mask[s]` (masks `0x7F5CE8`: 0, 0xFF000000, 0xFFFF0000, 0xFFFFFF00; a shift by 32 gives 0) into 8 × 8 tiles in 1-D order. Horizontal steps are 2 pixels. Nothing is clamped: rows past the map's 384 read the ROM that follows. Port: `hud::minimap_window`.
- Dots: racer i → object 42 − i; hidden (dx −16) when i > AI cars, or in elimination when its driver is null or flagged (bit 3). With a = −angle and (dx, dy) = the racer's (x / 499, −z / 499) − (cx, cz) + offsets: px = (dx·cos a >> 14) + (dy·sin a >> 14) + 28, py = (−dx·sin a >> 14) + (dy·cos a >> 14) + 28; shown at (px, py) when px ≤ 55 and 0 < py ≤ 55 (no lower bound on px), else at (−100, 100); frame 4 when i > opponents.

**Messages** (slots `0x03006210`: element, frame, timer, unused):
- `hud_message_show(msg, time, force)`: msg ≤ 15 and a table entry with slot ≠ 0xFF; timer = min(time, 0xF4) | 6; a running slot is only replaced with `force`. Wrappers: `FUN_0814305c` (no force), `FUN_081430f0` / `FUN_0814310c` (only for the player entity `*0x03000060`; force 0 / 1), `FUN_08143128` (message 2 for the player). After the HUD, every race frame calls `FUN_08143128(…, 0x3C, …)` or `FUN_08142e44(2)`.
- `FUN_08142e44(msg)` (cancel): timer 0, and the element hidden when the slot holds one.
- `hud_message_tick`: a running slot shows its element with its frame while timer bit 2 is set, hides it otherwise, and counts down.
- `hud_message_hide`: all timers 0; each element of the mode's table hidden with position, angle, scale and frame cleared.

**Reset and toggle.** `hud_reset(S)` (race init, and after the race on the results screen's menu sprite screen `0x08347778`): every object of the current screen gets flags |= 3 (visible, semi-transparent) and angle, frame and scale 0; then, in modes 0–3 (`FUN_08142094`, `FUN_081420d0`, `FUN_0814210c`): objects 0 and 14 frame = language, object 32 = language + 1 when `*0x03000040` ≠ 0, the message element (46; 54 in mode 2) hidden; then `hud_message_hide`. `FUN_08143010(on)` (pause: 0, continue: 1): 1 → `hud_reset`, else every object of the screen hidden; then `hud_message_hide`. Ports: `hud::reset`, `hud::toggle`, `hud::message_*`.

**Located, not called by the HUD, not ported:** `FUN_0814279c` (a countdown timer: `*0x03006154` − race time, race state 7 at zero, blinking under 10 s), `FUN_08142aac` (best lap driver `+0xB4`, else the profile's record time `+0x218` of track slot `0x7E49C4[route]`), `FUN_08143094` (a units panel frame).

**Verification.** `python tools/record.py hud STATE FRAMES NAME KEYS POKES` (mGBA with `tools/record.py hud`; traces in `$NFSGBA_DATA/work/e5298b24/hud-logic/`, about 30 KB per frame, not in git). Tag 0 at the `bl hud_update` in `race_frame_update` (`0x0813aa9c`) and tag 1 after `sprite_screen_update` (`0x0813aaa8`) each hold the IWRAM windows `0x03000000–0x03000200` and `0x03005300–0x03006900`, the 0x37 objects, entities 0–3 and their driver structs, profile `+0x402`, OBJ VRAM tiles 0x200–0x3FF, the OBJ palette and DISPCNT. Tags 2 and 3 hold the entry and exit of `hud_message_show`, `FUN_08142e44`, `hud_reset` and `FUN_08143010` (they nest). Pokes set inputs just before `hud_update`; temporary ones are restored after tag 1, so the game only sees them in the HUD. The tests in `hud.rs` run `hud::update` + `ui::update_sprites` from every tag-0 state and require tag 1's objects, globals, message slots, whole shadow OAM, OBJ palette and OBJ VRAM; call records must give the exit's objects and slots. Since the IRQ can tick the race time mid-update, each object, OAM entry and tile may match the run with either value (see *Not 1:1*).

| Trace | From | Frames / calls | Covers |
|---|---|---|---|
| circuit | `race.ss` (the reference race, mode 0) | 600 / 0 | mph, gears |
| hunter | Quick Play custom hunter (mode 2) | 900 / 0 | hunter bars, messages |
| elimination | custom elimination (mode 1) | 900 / 0 | positions 1–4; opponents 1 and 2 flagged eliminated (temporary pokes) |
| sprint | custom sprint (mode 3) | 900 / 0 | lap 1 / 1 |
| inputs | `race.ss` + pokes | 900 / 0 | km/h, languages 1–4, wingman portrait, blink and bar, arrow both ways, first place, needle scale, HUD off |
| hunter-wingman | hunter + pokes | 600 / 599 | the bars' wingman rule, bar over and under range; message show and cancel |
| edges | `race.ss` + temporary teleports | 600 / 599 | minimap x > 448, x < 0 at a real city position; a split past an hour |
| edges-y | `race.ss` + temporary teleports | 300 / 299 | minimap y < 0 (the offset bug), y > 448, both negative |
| racestart | race-info screen → race | 400 / 400 | the race init's `hud_reset`, countdown, positions 0–4, arrow |
| timeout | race time set to 59:48.33 | 196 / 196 | blinking, race state 8, the results screen's `hud_reset` |
| pause | pause and continue | 300 / 302 | the toggle both ways (nested `hud_reset`) |

All 7,096 frames and 2,395 calls replay exactly. Changing the needle offset, dropping the minimap's word merge or the dots' frame rule each makes the replay fail.

## Menus (top level and intro screens exact: `menu.rs`)

The game's frame (`main_frame` `0x0812AE64`) runs a small state machine, and in state 1 a menu system of 49
screens that share eight kinds of handler. Everything below is ported from the disassembly (Ghidra lost the jump
tables) and checked against the game's own code through the function oracle.

### Frame and game state

`main_frame`, once per VBlank:
1. Timer 3's count for the last frame (`timer_read(3)`; 0 reads as 0x200) → frame time `0x03005640` = 25,500 / count,
   clamped to 10..=100, or 15 when `0x03005624` is 2 (then the 0 → 0x200 fix is skipped).
2. `FUN_0812B084` (input), then `game_state_step`, the sprite screen `0x03000058`, `FUN_0816102C`; frame counter
   `0x03005628` + 1; in a race the light tint.
3. **Palette fade** (`FADE` `0x03005630`): above 0 it fades in, 4 per channel and frame, the sky gradient buffer
   (`*0x030053B8`, 0x200 colours, towards the level's gradient material: world `+0x00` + material `+0x5E`'s offset),
   BG palette RAM (towards the second base palette `*0x0300577C`) and OBJ palette RAM (towards world `+0x34`), then
   the counter moves 2 towards 0. Below 0 the same three fade out towards black. At 0, outside races, a dirty
   palette (`0x0300563C`) is copied to palette RAM. The four RAM fade routines share two algorithms
   (`fade_in_step`, `fade_out_step`); the OBJ fade-in reads its target from `first`, the others from index 0.
4. `FUN_0812B040`, `FUN_08142090`.

`game_state_step` (`0x03005808`):

| State | Does |
|---|---|
| 0 boot | `MENU_EXIT` = 0; once the fade is 0: `enter_screen`, state 1 |
| 1 menus | `menu_frame`; 0 → state 4; 1 and fade 0 → `draw_screen(0)` |
| 4 race start | state 5, `race_start_from_table_a(world)`, fade 0x10, `0x030057E0` 0x10, frame counter 0; with a route: base palette copy and the light tint; then state 5 at once |
| 5 race | `race_frame_update(world)`; when it returns 0: results, profile `+0x32C` = player, back-stack top restored, then `menu_back` (race phase 5) or `FUN_0812BB5C(0xB)`; state 1; `carbon_play_music(0)` |

### Screens

A screen (`0x03005944`) has four handlers, found through four jump tables of Thumb stubs (`bl handler; b join`):
enter `0x0812B454`, update `0x0812B980`, draw `0x0812D370`, exit `0x0812B640`. The stubs group the 49 screens
into eight kinds (`menu::Kind`; the test `screen_tables_match_the_rom_jump_tables` decodes the ROM stubs):

| Kind | Screens | Enter | Update | Draw | Exit |
|---|---|---|---|---|---|
| List | 0–6, 9, 27–30, 35, 36, 45, 46 | `0x0812FE38` (28: a random unlocked car first) | `0x081303E8` | rand + `0x08130D8C` | `0x081314A4` |
| Kind7 | 7, 8, 14, 17 | `0x0812E80C` (7: profile `+0x404` = 0; 8: also reverse = 0) | `0x0812E3D4` | `0x0812E5AC` | `0x0812E850` |
| Career zone | 11, 12 | `0x0812F204` | `0x0812F364` | `0x0812F450` | `0x0812FC6C` |
| Career event | 13 | `0x0812E308` | `0x0812DAFC` | `0x0812DD80` | `0x0812E380` |
| Setup | 10, 15, 16 | `0x081328F4` (15: `FUN_0812B320` first) | `0x08132AB8` | rand + `0x08133074` | `0x081336BC` |
| Kind18 | 18–20 | `0x08133708` | `0x081338E0` | `0x08133F2C` | `0x081348D8` |
| Intro | 21–26, 37, 47, 48 | `0x081315A0` (not 48) | `0x081318E4` | rand + `0x08131FE0` | `0x08132780` |
| Kind38 | 38–43 | `0x08134DF0` (38, 39, 43; 40 starts a fade-out) | `0x08134EB8` | rand + `0x08135340` (not 40, 42) | `0x081354FC` (38, 39, 43) |
| — | 31–34, 44 | none | none | none | none |

Screens seen in snapshots: 0x19 language select, 0x30 health and safety, 0x1A EA logo, 0x18 public service
announcement, 0x17 title, 0x16 profile, 0x0B/0x0C career zones.

**The draw handlers of four kinds draw a `rand_table` number first** (`draw_screen`), so every menu frame on those
screens advances the random sequence that later picks the opponents (`atlas::pick_opponent_cars`): race setups
depend on how long the player stayed in the menus.

`menu_frame` (`0x0812B5F0`; earlier notes called it `race_setup_route`), one menu frame:
1. **Leaving:** while `MENU_EXIT` (`0x03005780`) is 7 and the fade has finished, run the exit handler of
   `0x0300594C` and return 0, which moves the game to state 4. Other non-zero values return 1.
2. **Race chosen** (screen above 0x7F): 0x81 is Quick Play: track slot `0x7E49C4[route]`, direction fix-up (reverse
   slots are 12 above; reverse is cleared past slot 23), the player's car (profile `+0x10` in career, else `+0x11`)
   into `0x03005718` and `0x0300611C`, route number from `0x7E4A70[slot]`, back-stack top saved, racer slots
   cleared, environment and route index from `0x7F2588`. Any such screen then sets `MENU_EXIT` 7 and a fade-out.
3. **Message box** open (`0x030059F0` ≥ 0): A (or B on a type-2 box) closes it and swallows the keys; nothing else.
4. **Key repeat:** each newly pressed key sets its delay in profile `+0x33C…+0x343` to 3.
5. The screen's **update** handler; its result is `menu_frame`'s.
6. **B** (keys exactly 2) goes back (`menu_back`, with sound 3 when the stack is not empty), except on screens 5,
   6, 0xB, 0xC, 0x16–0x1A, 0x26, 0x27, 0x2A, 0x2F, 0x30, and 9 in career when profile `+0x12` is 0. On screen 0x11
   with profile `+0x404` = 3 it calls `FUN_081439C0(0)` instead. The screen is read again after the update.
7. Every delay above 0 counts down.

`enter_screen` marks the screen changed, runs the enter handler, sets `0x03005938` (except screen 5), clears profile
`+0x2F6`/`+0x2F8`, the exit screen, and draws in full. `menu_back` pops profile `+0x344 + top`.

### Intro screens (the whole kind exact: `intro_enter`, `intro_update`, `intro_draw`, exit)

Boot flow: **0x19 language** (A picks; the cursor moves over 5 languages, `0x7E5D10` maps it) → **0x2F health and
safety** (0x5A ticks) → **0x30** the same, colour 4 blinking (any key or 0xDB6 ticks) → **0x1A EA logo** (0xF0 ticks)
→ **0x18 public service announcement** (0x1E0 ticks) → **0x17 title** (START after 0xF0 ticks: with a saved profile
(`+0x490`) load it and go to screen 0, else **0x16 name entry**). 0x15 is the credits. Deadlines are profile `+0x3B0`
against the tick counter `0x03000044`. The next screen of a timed page is its item list `+8`.

Page records at `0x7E5DA8` (0x14 bytes: `+0` heading text, `+2`/`+4` button prompt texts, `+6` background menu
material, `+8` menu palette, `+0xA` item count, `+0x10` item list). The enter handler hands `+6`/`+8` to
`menu_scene_setup_a`/`_b` (`0x081370D4`/`0x081371A4`), which unpacks the material and sets world `+0x30` to menu palette
`0x33EF14 + palette·0x200` (and `+0x34` to menu palette 1): **this is each intro screen's palette** (FIDELITY U4).

| Screen | Heading | Prompts | Material | Palette | Items |
|---|---|---|---|---|---|
| 0x15 credits | 269 | −1, 146 | 227 | 9 | list `0x08799882` set by enter |
| 0x16 name entry | 268 (0x122/0x10C drawn) | 498, 147 | 3 | 3 | — |
| 0x17 title | — | — | 5 | 5 | — |
| 0x18 PSA | — | — | 1 | 1 | next 0x17 |
| 0x19 language | — | 498 | 4 | 4 | 5 cursor positions (material, x, y) at `0x087E5D30` |
| 0x1A EA logo | — | — | 2 | 2 | next 0x18 |
| 0x25 | 960 | 498, 146 | 1 | 1 | — |
| 0x2F/0x30 health | — | — | 1 (per language 7, 8, 0xA, 9, 0xB via `FUN_0813644C`) | 1 | — |

- **Name entry (0x16):** 4 rows of 10 characters (`1`–`9`, `0`, `A`–`Z`, `.`, `,`, `!`, `:`, then a space) and a row
  of DEL (columns 0–2), SPACE (3–6), OK (7–9); cursor `0x0300597C` row, `0x03005990` column, name `0x03005970` (9
  bytes), length `0x0300598C`. B deletes, START is OK. OK needs a name whose bytes OR to neither 0 nor 0x20, copies it
  to the profile, and calls `save_write_profile`; a failure goes to screen 0, else back (profile `+0x494` = 2). Entering
  the screen resets the cursor, copies the profile's name in and, unless `+0x494` is 2, clears `+0x478…+0x48C`.
- **Title (0x17):** logo line `0xC4 + language`, `0xC9` (Spanish) or `0xCA`, and PRESS START `0xBF + language` blinking on
  bit 4 of `0x030053B4` once the deadline has passed. START plays sound 2, sets profile `+0x494` = 0, and with a
  profile draws text 0x159, loads the save, sets units from the language when it differs from the save's (`+0x4E8`).
- **Health (0x30):** colour 4 of the second base palette steps by ±0x421 between 0 and 0x7FFF each frame (direction
  `0x03000000`), the palette is marked dirty, and the frame waits one extra VBlank.
- **Credits (0x15):** a list of pages (u16 line count, then (flags, text key) per line); each 0xB4 ticks the next
  page; a count of 0 presses B. Lines are centred in 0x90 rows (12 per line plus 4/6/12 for flags 0x2000/0x1000/
  0x800); flag bits 0–1 give the colour, 0x2000 the smaller font, 0x8000/0x4000 on the first line start at row 0x28/0;
  some keys sit lower in French (0x1B, 0x1D), Italian (0x19A) and Spanish (0x1F).
- **Language (0x19):** left/right step through 0–4 with wrap; up/down move between the rows of 3 and 2 (0→3, 1→4,
  2→4, 3→0, 4→1 down; 3→0, 4→1, 0→3, 1/2→4 up); the language follows the cursor every frame.

### Verification

- `menu::tests::top_level_matches_the_game`: 2,400 oracle cases (`tools/oracle/cases.py menus toplevel`) of
  `menu_frame`, `game_state_step` and `main_frame` over six snapshots (menus, career, race) with random screens, exit
  requests, fades, message boxes, keys, back stacks, key delays, profile flags, routes, reverse, random index,
  palettes and handler results. The unported callees (the kind handlers, sound, timers, race functions) are stubbed on
  both sides and must be called in the same order with the same arguments; every changed RAM byte and the result
  must match. 0 mismatches; breaking the rand-before-draw rule or one key-repeat slot fails it.
- `menu::tests::intro_screens_match_the_game`: 2,400 oracle cases (`tools/oracle/cases.py menus intro`) of `menu_frame`,
  `goto_screen` (entering each intro screen and 0x80/0x81/0x82) and `draw_screen` on the intro screens with random
  keys, deadlines, keyboard and name states, languages, credits lists, blink colours, profile flags and save results.
  The drawing primitives are stubbed on both sides, so every blit and text call must have the same arguments. Six
  mutations (keyboard remap, blink step, language image, a cleared profile field, the row-4 cursor, the credits
  height) each fail it.
- `menu::tests::fades_match_the_game`: 600 cases of the six fade routines (buffer, BG and OBJ, in and out).
- `menu::tests::screen_tables_match_the_rom_jump_tables`: the four tables for all 49 screens.

Oracle gotcha: `Result.read` rebuilds memory from the snapshot plus the call's writes, **without** the call's `mem`
inputs, so a byte an input set and the call left alone reads back as the snapshot's. Apply the writes to your own
inputs (`after()` in `tools/oracle/cases.py menus`).

## Menus, continued (menus-2: `menu.rs`)

The other screen kinds, ported from the disassembly and checked against the game's code through the oracle like the
intro kind (`tools/oracle/cases.py menus`, cases under `$NFSGBA_DATA/work/<sha8>/menus2/`).

### The screens

| Screen | Kind | Heading (English) | Background (material, palette) |
|---|---|---|---|
| 0 | List | MAIN MENU | 1, 1 |
| 1 | List | RACE TYPE | 1, 1 |
| 2 | List | MY PROFILE | 1, 1 |
| 3 | List | CREW HOUSE | 1, 1 |
| 4 | List | GARAGE | 1, 1 |
| 5 | List | PAUSED | 1, 1 |
| 6 | List | END OF RACE | 1, 1 |
| 7, 8 | Kind7 (map) | SELECT TRACK (circuits, sprints) | 218, 7 |
| 9 | List | CAR SELECT | 1, 1 |
| 0xA | Setup | RACE INFO | 1, 1 |
| 0xB, 0xC | Results | RACE RESULTS (record and unlocks; standings) | 1, 1 |
| 0xD | Career event | RACE EVENT | 1, 1 |
| 0xE | Kind7 (map) | SELECT ZONE | 218, 7 |
| 0xF | Setup | SETTING CIRCUIT / ELIMINATION / HUNTER / SPRINT (by race mode) | 1, 1 |
| 0x10 | Setup | OPTIONS | 1, 1 |
| 0x11 | Kind7 (map) | TRACK RECORDS | 218, 7 |
| 0x12–0x14 | Kind18 | the garage's car pages | (their enter handler) |
| 0x1B | List | MY DASH | 1, 1 |
| 0x1C | List | QUICK PLAY | 1, 1 |
| 0x1D | List | PERFORMANCE UPGRADE | 1, 1 |
| 0x1E | List | VISUAL UPGRADE | 1, 1 |
| 0x23 | List | AERODYNAMICS | 1, 1 |
| 0x24 | List | ACCESSORIES | 1, 1 |
| 0x26 | Kind38 | career hints (story pages) | per entry (e.g. 226, 8) |
| 0x27 | Kind38 | the wingman's introduction | per entry (226, 8) |
| 0x28 / 0x29 / 0x2A | Kind38 | save / "hint n" wait / clear, between hints | 5, 5 before the first hint, else 226, 8 |
| 0x2B | Kind38 | a race mode's first-race hint | per entry |
| 0x2D, 0x2E | List | MY CREW | 1, 1 |

The background is what `menu_scene_setup_a/_b` load (`menu_scene_setup` in `menu.rs`): the material (unpacked when
packed) into the menu scene, menu palette `0x33EF14 + 0x200·palette` into world `+0x30` and both base palettes, OBJ
palette 1 scaled to black, and optionally a sprite screen. `_a` (a screen entered from outside the menus) first
waits a VBlank, blacks out BG palette RAM, clears the exit request and loads the menu descriptor `0x7F2FE8`. This
answers FIDELITY U4 for every kind: the page record of each screen names its palette (intro pages `+8`, List pages
`0x7E544C` `+8`, results `0x7E510C` `+10`, event `0x7E5090` `+8`, setup `0x7E6260` `+8`, maps 7, hint entries `+2`).

### Map screens (Kind7: 7, 8, 0xE, 0x11)

State `0x03006230` (`+0`/`+4` view, `+8` cursor, `+9` moved). Left/right move over 12 entries (18 for sprints and
the sprint records, step 2 on the district map). A by profile `+0x404`: 0 picks a track (circuits through
`0x7E472C`, sprints `cursor + 0x18`; the route number from `0x7E4A70`) if its district is unlocked (`0x117 +`
cursor/2 or /3) and goes to 0x2D; 1 picks the district (profile `+0x1FB`); 2 switches the record pages to mode 3; 3
goes back. The draw shows the heading, the arrows lit while their key-repeat delay runs, and A's prompt 0x15A over a
locked district. Not ported: the map's zone palette (`map_zone_palettes` `0x08143284`) and the scrolling map draw
(`map_draw` `0x081435C4`).

### Career event (0xD)

The zone's events in rows of 3 (12; 6 in zone 5), cursor per zone at profile `+0x388`. SELECT opens the district map.
A on an open event (a boss race needs its unlock `0x122 + zone` / `0x11D + zone`; events past 0x3C need the one
before won) clears the result slots, copies the event into the race globals (`career_event_to_globals`, from
`career::events`), then shows a due hint (0x28) or goes on. The draw: the grid with mode icons (`0x7E5078`), boss
lock icons, won/second marks, the cursor; the event's track, mode, reward (the ladder reward, halved once won, times
the car's style percentage: `career::style_rating`, `reward_percent`) and record time.

**Hints** (`hint_due` `0x0812CF48`): in career mode, the hints seen (`+0x1F8 + 0x1F9`) pick the next one per zone and
per screen it precedes (3, 6, 0xD, 0x2D, and 10 for a race mode's first race), some only on boss events or with car
record bits `+0x450…+0x453`.

**Texts built on the stack:** numbers (`number_text` `0x081633CC`, with the thousands separator `0x0812D62C`: a space
in French, a dot in German and Italian, above 999) and times (`time_text` `0x0812FC00`: `mm:ss:cc` with the last
colon a dot in French, German and Spanish, a comma in Italian). Both divide through the IWRAM routine, whose
remainder stores (`0x03006480`) are part of the exact RAM effect.

### Race results (Career kind: 0xB, 0xC)

Entering 0xB after a race copies the results `0x03005650` to the ranked table `0x03005730`, ranks an elimination by
time then knock-out, checks for a new track record (the player's best lap against profile `+0x218`, not in two-player
career) and pays out (`career_race_payout`, still a stub here; `career::race_payout` models it on a `Save`); with no
record it goes straight to the standings (0xC). 0xB shows the record (track, time) or the unlock messages
(`+0x4AA…`, new cars and districts highlighted); 0xC the standings per mode (hunter: life/2^19·100; circuit and
elimination: finish time and best lap, dashes when knocked out; sprint: finish time), then the payout. A on 0xC
returns to the career menu (6), cutting the back stack at its first 0xC. Racer names: the player's (profile), a
boss's (`0x7E4954[id − 0x10]`) or an opponent's (text `id + 0xA4`).

### Settings (Setup: 0xA, 0xF, 0x10)

Records `0x7E6260` (`career::setup_screens`): 0x10 options, 0xA Quick Play, 0xF by race mode. Entering copies the
settings into the profile (`+0x3BC…+0x3F4`, volumes / 8; at most 2 opponents with a wingman); outside 0xA the copies
drive the race globals every frame. Left/right step the item's value within its range (laps and opponents move
together in an elimination; at most 2 with a wingman), up/down the cursor. A runs the item's action: a screen (Quick
Play 0x81 may show a hint first, 0x2B), back when nothing changed, or the save question (0x87, message box 2, text
0x1D8); a "yes" on the options writes the settings back and saves. The draw lists the items (0xA first a summary:
track or mode, car, wingman, blinking on bit 7 of the frame counter) with their value texts.

**The career race's opponents** (`FUN_0812B320`, run when entering screen 15): three different random picks
(`rand & 7`) of the zone's eight opponents (ids `0x40 + 8·zone`; zone 5 `0x60`) into the result slots
`0x03005655..=57`; on the zone's boss events (and event 0x41) the first is the boss (`0x10 + 2·zone`, `+1`); the
wingman (`0x20 + wingman`) takes the last opponent slot; in a two-player career before the first hint the first
opponent is fixed (0x2D). This answers where career races choose their opponents (career.md "Not decoded").

### Hints and story pages (Kind38: 0x26–0x2B)

Page records (0xC bytes: `+0` enter script, `+4` draw script, `+8` entries of 10 bytes: background, line, text,
action): hints `0x7E8570 + 0xC·hint`, the wingman page `0x7E8534`, the race modes' hints `0x7E8540 + 0xC·mode`.
**Page scripts** (`page_script` `0x081348E8`): u16 sections `[id, commands…, 0xFFFF]`, the section run is the page
(hints: profile `+0x1FA`); commands 0xFFF5 stop a sound; 0xFFF6 set a wait and play a sound; 0xFFF7 play a sound;
0xFFF8 stop a sound once the wait is over; 0xFFF9 stop the music; 0xFFFA play music; 0xFFFB a flash (palette
0xC0.. from `0x7E6ED4` and a filled rectangle); 0xFFFC a picture on the map grid (profile `+0x254`); 0xFFFD a
portrait (the wingman's on 0x27, `0x7E78A0`) and its 64 colours (`0x7E6EEC…`). A: 0x40 next page, 0x41 hints done
(next zone when `+0x256`, save, screen 3), 0x42 back, 0x81 a tutorial race (`FUN_08134CC0`: one of three) or a
mode's first race, others a screen (9 also resets every car record and sets the cash to 1000). B: the page before,
or out. Between hints: 0x28 saves the hints seen, 0x29 shows "hint n" until the wait, 0x2A clears both frame buffers
(`fill32`) and returns to 0x26.

### Lists (0–6, 9, 27–30, 35, 36, 45, 46)

Pages `0x7E544C` by `list_slot` (items 8 bytes: text, two pictures, action); cursor per slot at profile `+0x350`.
Three items are drawn (previous, next, current at `0x797CD8`/`0x797CE0`), the crew pages with lock state, portrait
(palette `0x7E786C`), role and level; car select (9) shows the turning car (L/R on `0x030064C4`), its name, stats,
price, cash and buy/pick prompts. Left/right move (2-item pages toggle); on car select the car record is copied
(`garage_copy_car_record`) and the car reloaded. A: the crew pages pick the wingman (`+0x200`, side `+0x204`) and the
opponent count (3, or 2 with a wingman), then return by the back stack; career (3) and events (0xD) may show a hint;
0x1F–0x22 open the upgrade pages (`+0x364`/`+0x365`); RACE TYPE (1) sets up a free race (Quick Play car, 2
opponents, 2 laps or 1 in a sprint, `rand % 3 + 1` and `rand % 20` looks); QUICK PLAY's first item draws a race mode
from `0x797D0A` and a random track; actions from 0x85: pick or buy the car, the profile pages, the options, the
hints-done step, a career race's setup, the upgrades save. A confirmed message box resumes the paused race (5),
starts a new profile (2), buys the car (9), or saves the upgrades (0x8B, `upgrades_changed` into `0x03005954`).
Stubbed below the handler: `unlock_state`, `unlock_owned`, `buy_unlock`, `unlock_price`, `upgrades_changed`,
`list_item_new`, `car_stats_draw`, `quick_race_random`, the garage car loaders and `garage_draw_car`.

### Verification (menus-2)

Each kind's handlers are called directly on random menu states over the five menu snapshots, with every unported
callee stubbed on both sides (same calls, same arguments, same order), plus `goto_screen(15)` for the opponent
picks. Every changed RAM byte (VRAM and palette RAM included) and the update handler's result must match.

| Test | Cases | Mutations caught |
|---|---|---|
| `map_screens_match_the_game` (kind7) | 1,600 | 6 of 6 |
| `career_event_screen_matches_the_game` (event) | 1,600 | 12 of 12 |
| `race_results_match_the_game` (career) | 1,600 | 9 of 10 (the tenth is equivalent) |
| `settings_screens_match_the_game` (setup) | 1,600 | 14 of 14 |
| `hint_pages_match_the_game` (kind38) | 1,600 | 12 of 12 |
| `list_screens_match_the_game` (lists) | 2,400 | 14 of 15 (the fifteenth is equivalent); a coverage gap: a back-stack top of 9 on the crew page |
| `top_level_matches_the_game`, `intro_screens_match_the_game` (regenerated) | 2,400 each | — |

Stub arguments that point into the game's stack (the number and time texts, the rectangle of `fill_rect8`) are
recorded by content and compared with the port's `Gba::texts`. After porting any kind, **every** case set must be
regenerated (`tools/oracle/cases.py menus all`): handlers enter and draw arbitrary screens (`menu_back`,
`goto_screen`), whose handlers were stubs in older sets.

## The "raw 8bpp region" `0x4018C0–0x794000`

Every byte up to the city tables is a material of a known table (script `coverage`, not kept):

| Range | Contents | Referenced by |
|---|---|---|
| `0x4018C0–0x45F4C0` | vehicle materials 116–145: 30 raw 8bpp 128×100 car atlases (half-size views of the player cars, vinyl areas in slots 224–255) | vehicle materials `0x45F5C0` (level `+0x20`), base `0x370550` (`+0x0C`) |
| `0x45F4C0–0x45F5C0` | vehicle material 146, raw 16×16 | same |
| `0x45F5C0–0x47BC6C` | vehicle materials, models, vertices, indices, UVs, sizes | address map |
| `0x47BC6C–0x71F168` | city texels incl. column maps (all 227 city materials) | city materials `0x720DE8`, base `0x47BC6C` |
| `0x71F168–0x71F1E8` | 128 bytes, zero except the first word (unknown) | — |
| `0x71F1E8–0x77A000` | city palettes, materials, sectors, walls | address map |
| `0x77A000–0x794000` | route data (`race-routes.md`); the menu 3D scene uses sectors `0x77A570`, walls `0x77A600` (menu descriptor) | not UI |

Vehicle materials 61–66 (`0x3E519C–0x3F7D9C`) are the same raw 128×100 format for traffic cars. Pixel values are 161–255: direct indices into the race BG palette (car slots), so `vehicle_textures` already reads them exactly.

## Verified

- **Language screen, byte for byte**: material 4 + material 181 at (9, 19) + TEXT_SELECT in font 0xE right-aligned at (238, 147) + icon 156 at (221 − width, 143) = the dumped frame buffer; palette RAM = menu palette 4 (`language_screen_is_recomposed_exactly`).
- **Health and safety, EA logo, public-service screen, title screen, byte for byte**, including the word-wrapped paragraph (font 0xE from (8, 30), width 224, colour 8), the centred title (font 0xD at x 120, rows 6 and 16) and the unclipped logo blit; palettes checked (`intro_screens_are_recomposed_exactly`).
- **Race HUD**: OAM entries 9–55 (all attribute bits the routine owns, starting from inverted bits), both affine matrices, the OBJ palette (level palette + minimap bank for route 23), and every uploaded tile except the redrawn needle and minimap, all equal to the reference race dump (`hud_matches_the_race_dump`).
- **HUD logic** (`hud.rs`): 11 mGBA traces, 7,096 race frames and 2,395 calls around the HUD, replay exactly in objects, globals, message slots, the whole shadow OAM, the OBJ palette and OBJ VRAM, minimap tiles included (*HUD logic* above).
- **Unpacker**: all 299 packed menu and vehicle streams decode; the 284 self-contained ones equal the BIOS decoder; none reads uninitialised ring bytes.
- Table sizes and tiling, character map, font geometry, Windows-1252 decoding of the German text.

Dumps used: `work/e5298b24/mgba/race.*` (reference race) and `work/e5298b24/ui-2d/{lang,n1,n2,n3,n7}.*` (fresh save: language screen, health, EA, PSA, title), made with `tools/mgba_ctl.py` in session `ui-2d`.

## Not 1:1 (yet)

- **HUD timing:** the race time `0x03005800` is counted by the VBlank IRQ, which can land in the middle of `hud_update`; then the timer, the portrait and the arrow of one frame read different values (seen in the traces). `hud::update` reads one value per frame. Exact reproduction needs the frame's CPU timing (like FIDELITY R17/R18).
- **HUD paths the game never reaches**, ported as written but only exercised through forced inputs: the minimap's y clamps (the city spans map rows 1–316 and columns −40–445, so only x < 0 happens; y < 0 stores its offset into the x slot, a game bug) and its window rows past the map's 384 (it copies whatever ROM follows). A **negative split** is never produced (`route_gap` `0x0813ebac` computes `T − x·T/y` only when `x < y`); forced, `hud_split` converts it before clamping, the divide quirk gives digit frames like −100 and −2000, and the game uploads garbage material data until it hangs (the port would index outside the material table and panic).
- **`map_world_to_screen`** (`FUN_08143144`, map screens, not the HUD): `((x >> 8) << 6) / scale[i] + x0[i]` and the same for −z, scales `0x7F4480` (u32), origins `0x7F44A8` (2 × u32 per map). Documented, not ported (part of U3).
- **Menu screens**: the primitives are exact, but each screen's positions and state machine (`FUN_081315a0` and the page tables at `0x7E4A00–0x7E6EEC`) are not ported. Menu sprite screens (`0x347778`) only hold material-0 placeholders; their use (hit boxes or cursor positions, hypothesis) is unknown.
- Palettes of the unidentified menu overlays are **assumed** in `ui_export.py` (recorded per image in `index.json`).
- Blits and glyphs outside the frame buffer are dropped (the game writes outside its buffer).
