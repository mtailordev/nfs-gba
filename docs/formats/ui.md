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

**Verification.** `python tools/ui_hud_trace.py STATE FRAMES NAME KEYS POKES` (mGBA with `tools/ui_hud_trace.lua`; traces in `$NFSGBA_DATA/work/e5298b24/hud-logic/`, about 30 KB per frame, not in git). Tag 0 at the `bl hud_update` in `race_frame_update` (`0x0813aa9c`) and tag 1 after `sprite_screen_update` (`0x0813aaa8`) each hold the IWRAM windows `0x03000000–0x03000200` and `0x03005300–0x03006900`, the 0x37 objects, entities 0–3 and their driver structs, profile `+0x402`, OBJ VRAM tiles 0x200–0x3FF, the OBJ palette and DISPCNT. Tags 2 and 3 hold the entry and exit of `hud_message_show`, `FUN_08142e44`, `hud_reset` and `FUN_08143010` (they nest). Pokes set inputs just before `hud_update`; temporary ones are restored after tag 1, so the game only sees them in the HUD. The tests in `hud.rs` run `hud::update` + `ui::update_sprites` from every tag-0 state and require tag 1's objects, globals, message slots, whole shadow OAM, OBJ palette and OBJ VRAM; call records must give the exit's objects and slots. Since the IRQ can tick the race time mid-update, each object, OAM entry and tile may match the run with either value (see *Not 1:1*).

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

## Integration notes

### FIDELITY.md

- **Close R13**: the 36 128×100 vehicle materials (61–66, 116–145) are raw 8bpp, kind 0, pixel values 161–255 indexing the race BG palette directly; `vehicle_textures` reads them exactly. (Rendered with the race palette: van, pickup, sedan, SUV and the player cars' half-size atlases.)
- **New U1** — Now: HUD OAM builder exact (`ui::update_sprites`), element logic not ported. Game: `FUN_081421b8`/`FUN_0814228c`/`FUN_08142360` and the functions in the HUD table. Exact source: those functions.
- **New U2** — Minimap: not ported. Game: `FUN_08142440` windows material 135 by player position/499 + (0x86, 0x79), rotates by heading into material 134's tiles, places dots. Source: `FUN_08142440`, `FUN_0815e6c8` index `(0x08169208…)` rotator.
- **New U3** — Menu screens: primitives exact (`unpack`, `blit`, `Font::draw`, `draw_wrapped`), screen logic not ported. Source: `FUN_081315a0`, menu page tables `0x7E5090`/`0x7E553C`/`0x7E5DA8`, `FUN_0812bd60`.
- **New U4** — `nfsgba_formats::text` decodes Latin-1; the game's glyphs are Windows-1252 with `{`/`|` = A/B buttons. Use `ui::text_bytes` + `ui::decode_text` (or switch `text`).
- **New U5** — `nfsgba_formats::lz77` (BIOS semantics) panics on the 15 menu streams that reference before their start; use `ui::unpack` for menu materials (vehicle streams are unaffected).

### address-map.md (ROM)

| Offset | Size | What | Doc |
|---|---|---|---|
| `0x16C244` | | menu texel base (menu descriptor `+0x10`); the LZ77 bank | formats/ui |
| `0x169208` | | ARM ring-buffer LZ77 decoder (IWRAM copy) | formats/ui |
| `0x33EF14` | 49 × 0x200 | menu palettes (menu descriptor `+0x04`) | formats/ui |
| `0x345114` | 273 × 0x24 | menu materials (menu descriptor `+0x24`) | formats/ui |
| `0x347778` | 10 × 8 | menu sprite screens (`+0x2C`) | formats/ui |
| `0x3477C8` | 47 × 0x14 | menu sprite elements (`+0x30`) | formats/ui |
| `0x347B74` | | HUD sprite texels, 4bpp (race `+0x10`; was "?") | formats/ui |
| `0x36C75C` | 4 × 0x200 | OBJ palettes (race `+0x04`; was "second palette block") | formats/ui |
| `0x36CF5C` | 280 × 0x24 | HUD materials (race `+0x24`; was "?") | formats/ui |
| `0x36F6BC` | 4 × 8 | HUD sprite screens (race `+0x2C`) | formats/ui |
| `0x36F6DC` | 185 × 0x14 | HUD sprite elements (race `+0x30`) | formats/ui |
| `0x4018C0–0x45F5C0` | | vehicle materials 116–146, raw 8bpp | formats/ui |
| `0x7C05F0` | 0x2000 × 2 | sine table, 2.14, half wave (0x4000 = turn) | formats/ui |
| `0x7E5090`, `0x7E553C`, `0x7E5DA8` | 0x14 each | menu page records | formats/ui |
| `0x7E5E48` | 5 × 2 | health screen colours 0–4 | formats/ui |
| `0x7E78B8` | 10-byte records | story screens: (material, palette) | formats/ui |
| `0x7EE974` | 4 × 0x18 | font descriptors (widths/y offsets at `0x7EE274…0x7EE894`) | formats/ui |
| `0x7F4378` | 5 | HUD digit x shift per language | formats/ui |
| `0x7F437D`, `0x7F43BD`, `0x7F43FD` | 16 × 4 | HUD message tables (race modes 0/1, 3, 2) | formats/ui |
| `0x7F4480`, `0x7F44A8` | | map scales and offsets (`FUN_08143144`) | formats/ui |
| `0x7F44F8` | 5 × 0x20 | minimap palettes | formats/ui |
| `0x7F4598` | 1 per route | minimap palette per route | formats/ui |
| `0x7F5BC8` | 256 | character → glyph map | formats/ui |

Level descriptor rows to update: `+0x04` = OBJ palettes (menus: menu palettes); `+0x10` = sprite texels; `+0x24` = sprite materials; `+0x2C`/`+0x30` = sprite screens/elements; `+0x58` = OBJ palette index × 0x100.

### address-map.md (RAM)

| Address | What |
|---|---|
| `0x03000040` | speed units: 0 = mph |
| `0x03000164` | race sprite screen (world `+0xA4`): `+0x00` materials, `+0x04` texels, `+0x0C` elements, `+0x10` screens, `+0x14` objects, `+0x18` u16 screen |
| `0x03005388` | u32 current route (1-based, 23 in the reference race) |
| `0x03005600` | u32 language (0 En … 4 Es) |
| `0x030056E0` | u32 race mode (selects HUD screen and messages) |
| `0x030056E4` | u32 laps |
| `0x03005784` | u32 opponents |
| `0x030057F0` | pointer to the unpack buffer |
| `0x03005800` | u32 race time in frames |
| `0x03006210` | 6 × 4 HUD message slots |
| `0x03006410` | frame buffer struct: s16 stride, `+0x0C`/`+0x10` page pointers |
| `0x030064E0` | u16 OBJ tile base (0x200 in mode 4) |
| `0x030064F0` | shadow OAM (128 × 8 bytes) |
| `0x0201F828` | HUD objects in the reference race |

### symbols.csv (new rows)

```
0x08163d30,unpack_to_buffer,function,allocates a 0x1011-byte ring and runs the ring LZ77 decoder at 0x08169208
0x08169208,lz77_ring_decode,function,ARM (IWRAM copy): BIOS LZ77 stream through a 4 KiB ring pre-filled with 0xFF from 0xFEE; writes header-size bytes
0x08164d90,blit_keyed,function,copies w x h bytes skipping a key byte through 16-bit read-modify-write; no clipping
0x08136d74,menu_blit_material,function,blits menu material (unpacked if kind bit 6) at x y into the current frame buffer
0x0812bd60,menu_button_prompts,function,button prompt bar: icons 156..161 and prompt texts in font 0xE
0x081315a0,menu_screen_setup,function,menu state setup; state 0x2F loads the health screen colours from 0x7E5E48
0x08162860,text_draw,function,draws a string: char map 0x7F5BC8; align 1 centre / -1 right; colour offset
0x08162720,text_draw_glyph,function,draws one glyph (width table; key skip; 16-bit read-modify-write)
0x08163080,text_measure,function,string width with a font
0x08163278,text_draw_wrapped_checked,function,range check then text_draw_wrapped
0x08162a1c,text_draw_wrapped,function,word-wrapped text: max width and lines; line height font +0x08
0x08141578,text_menu,function,font id 0xC..0xF with text key or pointer into the current page; returns width
0x081416d0,text_menu_both_pages,function,text_menu into both mode-4 pages; font 0xF uses material 12
0x08141840,text_menu_wrapped,function,word-wrapped text into both mode-4 pages
0x08141dd0,text_city_font_unused,function,uncalled variant reading glyphs from city materials 12..15
0x08161a98,obj_upload_tiles,function,copies n x 32 bytes to OBJ tile slot + *0x030064E0
0x08161c24,sprite_screen_update,function,sprite screen objects to shadow OAM (init or frame): position; hide; uploads; palette; blend; affine
0x08161ea0,sprite_screen_alloc,function,allocates 0x37 sprite objects; flags = visible for the screen's elements
0x08161eec,sprite_screen_select,function,sets the screen index then sprite_screen_alloc
0x0816144c,oam_set_affine,function,affine matrix from scale x/y and angle
0x08160fd0,oam_hide_range,function,hides n OAM entries (y 160; affine off)
0x0815f948,sin14,function,sine 2.14 from the half-wave table 0x7C05F0 (0x4000 = turn)
0x0815f988,cos14,function,sin14(a + 0x1000)
0x08142148,hud_reset,function,hides the HUD objects and resets messages for the race mode
0x08142f84,hud_update,function,minimap palette; per-mode HUD update; message tick
0x081421b8,hud_update_mode0,function,HUD element updates for race modes 0 and 1
0x0814228c,hud_update_mode3,function,HUD element updates for race mode 3
0x08142360,hud_update_mode2,function,HUD element updates for race mode 2
0x081428c0,hud_timer,function,race time digits mm:ss.cc (elements 5..10)
0x081429c4,hud_split,function,split time digits and sign (elements 21..27)
0x08142c4c,hud_lap,function,lap and total laps (elements 11..13)
0x0814306c,hud_position,function,race position and racer count (elements 19..20)
0x0814308c,hud_gear,function,gear digit (element 33)
0x08142724,hud_speed,function,speed digits in km/h or mph (elements 34..36)
0x08142bd8,hud_needle,function,rev needle angle (element 37)
0x08142440,hud_minimap,function,minimap window of the 512x384 map rotated by heading; opponent dots
0x08142ca8,hud_dial,function,dial segment frames from entity +0x4C8 (elements 29..31)
0x08142b44,hud_portrait,function,portrait and 8-step bar (elements 43..44)
0x081431a8,hud_arrow,function,animated arrow (element 45)
0x081431f0,hud_panel_language,function,timer panel frame = language
0x08143210,hud_split_panel_language,function,split panel frame and digit shift by language
0x08142ec0,hud_message_show,function,starts a HUD message from the race mode's message table
0x08142dec,hud_message_tick,function,counts down and blinks HUD messages
0x08142d64,hud_message_hide,function,hides all HUD message elements
0x081430c4,hud_minimap_palette,function,loads the route's minimap palette into OBJ colours 0xD0..0xDF
0x08143144,map_world_to_screen,function,world position to map coordinates (scales 0x7F4480; offsets 0x7F44A8)
```

### OPEN-QUESTIONS.md

- Menu materials 6, 16–152 and most overlays: what they are and which palette each screen uses.
- HUD material kind bit 4 (materials 5–33); element bytes `+0x0F`, `+0x10`; HUD screen 3's race mode.
- Menu sprite screens (`0x347778`): all elements use material 0 — hit boxes or cursor anchors?
- The rest of the menu data `0x7E4A00–0x7E6EEC` (page records found only in three tables).
- `docs/formats/lz77-images.md` answers: blobs are addressed through material tables (menu `0x345114` from `0x16C244`, vehicle `0x45F5C0` from `0x370550`); the "size + 8" is the game's decoder writing the header size; the "24-byte records near the atlases" at `0x36D010` are 0x24-byte HUD material records; palettes are `0x33EF14` (menus) and `0x36C75C` (OBJ).

## Integration notes (hud-logic)

### FIDELITY.md

- **Close U1:** "HUD element logic: exact in `nfsgba_formats::hud` (`update` = `hud_update` and every element function, `reset`, `toggle`, the message system) on top of `ui::update_sprites`. Checked: 11 mGBA traces (all four race modes, forced inputs, race start, pause, time limit), 7,096 HUD frames and 2,395 calls replay exactly in objects, globals, message slots, shadow OAM, OBJ palette and OBJ VRAM (`hud::tests`, `docs/formats/ui.md` *HUD logic*)."
- **Close U2:** "Minimap: exact (`hud_minimap`, the IWRAM window copy `0x03004B98`, the dots); every traced frame's minimap tiles match OBJ VRAM, clamp paths included (forced)."
- **New U5 (timing):** Now: `hud::update` reads the race time `0x03005800` once per frame. Game: the VBlank IRQ counts it and can land inside `hud_update`, so the timer, portrait and arrow of one frame may read different values (seen in the traces). Exact source: the frame's CPU timing (like R17/R18).
- **New U6 (unreachable):** Now: a negative split makes `hud::update` + `ui::update_sprites` index outside the material table (panic). Game: it uploads garbage material data until it hangs. Negative splits never happen (`route_gap` `0x0813ebac` only computes `T − x·T/y` for `x < y`).
- Feeding the HUD: `hud::Globals`, `Racer` and `Driver` list every RAM input; the physics and race-rules work should produce these (driver `+0x3C`, `+0x40`, `+0x44`, `+0xA8`, `+0xC5`, `+0x454`, `+0x4C8`, `+0x4D8`, `+0x4E8`, the race time, the split and the wingman values).

### address-map.md

ROM:

| Offset | Size | What | Doc |
|---|---|---|---|
| `0x165134` | | ARM divide with remainder (IWRAM `0x03000220`, called through `*0x03006494`) | formats/ui |
| `0x1651AC` | | ARM 32-byte block copy (IWRAM `0x03000298`) | formats/ui |
| `0x169AAC` | | ARM minimap window copy (IWRAM `0x03004B98`) | formats/ui |
| `0x7F4480` / `0x7F44A8` | u32 / 2 × u32 per map | `map_world_to_screen` scales / origins | formats/ui |
| `0x7F5CE8` | 4 × u32 | minimap copy masks per byte shift (0, 0xFF000000, 0xFFFF0000, 0xFFFFFF00) | formats/ui |

RAM:

| Address | What |
|---|---|
| `0x03000048` | race state: `hud_timer` sets 8 past 59:59.98 (time limit), the countdown timer 7 |
| `0x030000AC` | race-state changed flag |
| `0x03000220` | IWRAM divide routine (the image's first routine; `*0x03006494` points here) |
| `0x03000298` | IWRAM 32-byte block copy, used by `obj_upload_tiles` |
| `0x03004B98` | IWRAM minimap window copy |
| `0x0300601C` | HUD arrow direction (written by the car update `FUN_0813d1f0`) |
| `0x03006154` | countdown time (`FUN_0814279c`) |
| `0x0300615C` | split time in frames (`route_gap`, `race_start_setup`; `hud_split` clamps it to 0) |
| `0x030061D4` / `0x030061DC` | wingman portrait blink / frame |
| `0x030061E4` / `0x03006188` | wingman bar value / full scale |
| `0x03006480` / `0x03006494` | divide remainder / pointer to the divide routine |
| profile `+0x402` | needle scale (0x2000 instead of 0x1C00) |
| driver `+0x3C` / `+0x40` / `+0x44` | revs / gear / speed |
| driver `+0x454` / `+0x4C8` | needle rev scale / dial value |

Correction for the existing HUD rows: the HUD reads revs, gear, speed, `+0x454` and `+0x4C8` from the **driver** struct, not the entity; `hud_minimap_palette` runs every HUD frame, not once.

### symbols.csv (new rows; none in `docs/engine/symbols.csv` at 508f507)

```
0x03000220,divmod,function,ARM divide: |n|/|d| shift-subtract; quotient signed by n^d; remainder n-|q|*d stored at *r2 (called via *0x03006494)
0x08165134,divmod_rom,function,ROM copy of IWRAM 0x03000220 (divmod)
0x03000298,copy32,function,ARM copy of n>>5 32-byte blocks (ldm/stm); used by obj_upload_tiles
0x081651ac,copy32_rom,function,ROM copy of IWRAM 0x03000298 (copy32)
0x03004b98,minimap_window_copy,function,ARM: 64x64 window of the 4bpp map into 64 OBJ tiles; byte shift with masks 0x087F5CE8
0x08169aac,minimap_window_copy_rom,function,ROM copy of IWRAM 0x03004b98 (minimap_window_copy)
0x08142094,hud_reset_mode0,function,objects 0/14 frame = language; 32 = language+1 in km/h; hides message element 46 (modes 0/1)
0x081420d0,hud_reset_mode3,function,as hud_reset_mode0 (mode 3)
0x0814210c,hud_reset_mode2,function,as hud_reset_mode0 but hides message element 54 (mode 2)
0x08142674,hud_hunter_bars,function,hunter mode: two bar objects per racer from hunter life (driver +0x4E8)
0x0814279c,hud_countdown_timer,function,(not called by the HUD modes) countdown *0x03006154 - race time; race state 7 at zero
0x08142aac,hud_best_lap,function,(tentative; not called by the HUD modes) best lap driver +0xB4 or the profile record time as digits
0x08142e44,hud_message_cancel,function,stops a message and hides its element
0x08143010,hud_toggle,function,1: hud_reset; else hides every object; then hud_message_hide (pause 0 / continue 1)
0x0814305c,hud_message_show_default,function,hud_message_show(msg, time, 0)
0x081430f0,hud_message_show_player,function,hud_message_show(msg, time, 0) when the entity is the player (*0x03000060)
0x0814310c,hud_message_force_player,function,hud_message_show(msg, time, 1) when the entity is the player
0x08143128,hud_message2_player,function,hud_message_show(2, time, 0) when the entity is the player
0x08143094,hud_units_frame,function,(tentative; not called by the HUD modes) a units panel frame by language
0x08143248,hud_message_entry,function,message table entry: 0x087F437D (modes 0/1) / 0x087F43FD (2) / 0x087F43BD (3) + 4*msg
0x08161a34,obj_tile_address,function,0x06010000 + 32*(tile + *0x030064E0) when DISPCNT OBJ 1-D mapping; else 0
0x0816165c,obj_palette_write,function,copies n colours to OBJ palette RAM from index k (-1 when out of range)
```

### OPEN-QUESTIONS.md

- What the HUD arrow (`0x0300601C`, set by the car update `FUN_0813d1f0`) signals.
- Who calls `FUN_0814279c` (countdown timer), `FUN_08142aac` (best lap) and `FUN_08143094`.
