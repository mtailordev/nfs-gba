# World renderer (Carbon `BN7E`, race IWRAM code)

**Status:**
- **Exact and verified against the reference race frame:** the projection, the visible-sector list, the wall transform, wall setup and near clipping, the floor/ceiling outline and its column clip, and the camera-sector search. They are reimplemented in `crates/nfsgba-formats/src/render.rs`, whose tests reproduce the RAM of `data/work/e5298b24/mgba/race.ss` value for value:
  - visible list (world `+0x60`), all 9 entries;
  - wall draw buffer (world `+0x68`) of the camera sector;
  - flat outline (world `+0x6C`) and its clipped copy.
- **Decoded from the code but not reimplemented:** the column and span rasterisers and the entity draw. These are marked below; no pixel comparison has been made yet.
- Everything here comes from the ARM code (Ghidra decompilation, checked against the disassembly where it matters) plus the reference dump. Hypotheses are labelled.

The race renderer is ARM code copied from ROM `0x08165134` to IWRAM `0x03000220`, so an IWRAM address is the ROM address minus `0x164F14`. Thumb code reaches it through two dispatchers:
- `FUN_0815e674(index, …)` calls `*0x03006490 + 0xE4 + index·4`, where `*0x03006490 = 0x03000220`. Callers pass `(rom_fn − 0x08165218) >> 2`.
- `FUN_0815e690` is the same dispatcher with a return value; it is used for an atan.

All arithmetic is 32-bit two's complement with wrapping multiplies (`mul`, not `smull`), unless it says 64-bit. Shifts are arithmetic.

## Tables and structures

**Reciprocal table:** ROM `0x7C45F0`, 32,767 × i32, `recip[k] = 2^24 / (k + 1)` truncated. Every projection and divide uses it through literal pools `DAT_03001a1c`, `DAT_03001fa4`, `DAT_03002968`, `DAT_0300077c`, `DAT_03004180`, `DAT_03004cc0`, `DAT_03004c6c`, `DAT_03004d14` and `DAT_03004d3c`. It is indexed without bounds checks. Divide helpers:

| Function | Result |
|---|---|
| `FUN_03004ca4(a, b)` | `(a · recip[b]) >> 24`, 64-bit: `a / (b+1)`, rounded down |
| `FUN_03004ccc(a, t)` | `(a · (u32)t) >> 24`, 64-bit, `a` signed: multiply by a 2.24 fraction |
| `FUN_03004cf8(a, b)` | `(a · recip[b]) >> 16`: `(a << 8) / (b+1)` |
| `FUN_03004d20(a, b)` | `(a · recip[b]) >> 8`: `(a << 16) / (b+1)` |
| `FUN_03004c40(n)` | `n > 0x7FFE ? recip[n >> 1] >> 1 : recip[n]` |
| `FUN_03004c1c(p, n, v)` | stores `v` into `n >> 4` blocks of 16 bytes |

**View struct** (IWRAM `0x03000080`, world `+0x50`). `race_load_palettes` (`FUN_08138a9c`) initialises it:

| Offset | What |
|---|---|
| `+0x00` | frame buffer (`0x06000000`; the race draws into `0x0600A000`) |
| `+0x04` | → screen rectangle (`0x030053D0`: left, top, right, bottom as i32) |
| `+0x08`, `+0x0A` | i16 screen centre `cx`, `cy` (the camera rewrites them each frame) |
| `+0x0C` | pitch, 240 |
| `+0x10` | i32 **near plane = 64** |
| `+0x14` | a 0x1000-byte buffer |
| `+0x1C` | i32 **focal length = 150** (`0x96`) |

**Camera matrix** (world `+0x54`, `0x030057A0` in the race): 12 × i32. `m[0..9]` is a 3×3 rotation in 2.14 fixed point, row-major, and `m[9..12]` is the translation (minus the camera position, in world units). The renderer uses only:
- camera x = `(x+m9)·m0 + (z+m11)·m6 >> 14`;
- depth = `(x+m9)·m2 + (z+m11)·m8 >> 14`;
- height offset `m10`. Heights are wall heights plus `m10`, with **no pitch or roll**.

**Visible-list entry** (world `+0x60`, 16 bytes; `0x02018C8C`, room for 64): `[0]` sector, `[1]`/`[2]` left/right screen column, `[3]`/`[4]` top/bottom row, `[5]` flags, `[6]` portal depth, `[7]` (0 in entry 0, never written in the others). Flags:
- `0x80`: draw the sector's entities with the sector in pass 0, not in pass 1;
- `8`: merged away (`FUN_03004ef0`, never called in Carbon);
- `1`: passed to the flat rasteriser, never set.

**Wall buffer entry** (world `+0x68`, 0x34 bytes, `0x02017288`, one per wall of the sector being drawn):

| Offset | Field |
|---|---|
| `+0x00`, `+0x02` | x at start and end. Camera x after `transform_walls`, screen x after `setup_wall_spans` |
| `+0x04`, `+0x06` | depth at start and end (near-clipped) |
| `+0x08`, `+0x0A` | top at start and end (screen y once projected) |
| `+0x0C`, `+0x0E` | bottom at start and end |
| `+0x10`, `+0x14` | u at start and end, in 1/128 texel |
| `+0x18`, `+0x1C` | wall `+0x10` and `+0x18` (v of the top, and the v span) |
| `+0x20`, `+0x24` | wall `+0x14` and `+0x1C` |
| `+0x28`, `+0x2C` | not written by the renderer's setup |
| `+0x30` | v offset: wall `+0x42`·128 + material v scroll / 2 |
| `+0x32` | the wall's flags (`+0x2E`) plus span bits: 4 = not drawn, 8 = start clipped, `0x10` = end clipped, `0x20` = wholly behind the near plane |

**Flat vertex** (world `+0x6C`, 0x18 bytes, `0x0201B094`): x, floor y, ceiling y, `recip[depth]`, u/(depth+1) in 16.16, v/(depth+1) in 16.16. The u and v are in 1/128 texel. World `+0xF2` is the outline length, and world `+0xF4` the length of its clipped copy, which follows it in the buffer.

**Runtime tables** (RAM, updated by game code outside the renderer):
- world `+0x18`: moving wall pieces, 0x20 bytes, named by wall `+0x2A`:
  - `+0`/`+2`: dx/dz of the corner;
  - `+4`/`+6`: ceiling/floor dy;
  - `+8`/`+0xA`: top/bottom dy;
  - `+0xC`: material offset;
  - `+0xE`: flags that replace the wall's.
- world `+0x1C`: sector offsets, 0x14 bytes, named by sector `+0x0A`. `+4`/`+6` are the ceiling/floor dy, and `+8` holds flags that replace sector `+0x12` (`0x40`: sector hidden).
- world `+0x48`: per-material runtime entry, 8 bytes, named by material `+0x00`:
  - `+2`: animation frame (material offset);
  - `+4`: u scroll;
  - `+6`: v scroll.

In the Carbon city no sector uses `+0x0A`, `+0x20`, `+0x22` or `+0x24`. That leaves no movers, aliases or containers. 122 portal walls name a moving piece.

## Frame

1. **Camera:** `FUN_08137cb0` (race chase camera; `FUN_08137ac8` is a simpler look-at camera). It writes:
   - the matrix: `FUN_08160624` = rotation by the yaw `*0x03005F9C = −*0x03000214 & 0x3FFF`. Angles are 14-bit, `0x4000` = one turn; the reference frame has `0x3003`;
   - the translation `(−x, −(*0x03005FA4 >> 8) − (y_player>>8) − 16, −z)`. The 16 becomes `0x82` when `*0x03006148 ≠ 0`. The reference frame has `m10 = −30`: 150 − 164 − 16;
   - world `+0x54`, world `+0xF0 = 0`, the screen rectangle `{0, 0, 240, 159}` (world `+0x58..+0x5E`);
   - `cx = 120 + *0x03005390`;
   - `cy = 79 + *0x030056B8` (clamped to ±32) `+ (*0x030053A0 >> 8) + (8 if *0x030055F8 == 0) + *0x03005392`. `0x030055F8` is a camera mode, 2 in the race;
   - list entry 0 `{sector, 0, 240, 0, 159, 0, 0, 0}`.

   **Start sector:** `FUN_03000800` searches from the previous one (`*0x03005614`) at a probe point `camera + vᵀR`, with `v = (0, 0, 72)` from ROM `0x7BFC68` and `FUN_081608fc`. If that fails it tries `FUN_0814dbbc`, then keeps the previous sector. A second probe at `camera − vᵀR` is computed but only used when the first result is `0xFFFF`, which cannot happen after that fallback. Entry 0 uses sector `+0x22` instead when that is not `0xFFFF`. World `+0xC0`/`+0xC8`/`+0xEA` are just `FUN_03000800`'s inputs: other callers (`0x08147EC4`, `0x081484F0`, `0x0814C19C` and others) reuse them for entity lookups, so the dump's values there are not the camera's.

   **Focal:** while the byte at player-vehicle `+0x4D1` is 0, focal rises by 4 per frame up to 150. Otherwise it falls by 4 per frame towards `150 − max(0, (0x800 − g) >> 5)`, where `g = FUN_0815fadc()` after `FUN_0815fc38(…)`. `g` is not decoded; this looks like a speed/boost effect (hypothesis).
2. **Visibility:** `FUN_03004828` (below).
3. **Draw:** `FUN_030048c8`:
   - clears 32 bytes at `0x03006920`;
   - **pass 0**: for entries from last to first, skip flag 8 and call `draw_sector(entry)`;
   - **pass 1**: from last to first again, skip flags `0x80` or 8 and call `draw_sector(entry, 1)`, which draws only entities.

   Drawing is painter's order (farther list entries first) with no depth buffer.

## Projection (R11)

```
sx = cx + ((recip[d] · focal >> 7) · (x >> 1) >> 16)      // = cx + focal·x/(d+1), rounded down
sy = cy + ((recip[d] · focal >> 7) · (y >> 1) >> 16)      // same scale: square pixels
```
With focal 150 on a 240×160 screen:
- the horizontal FOV is `2·atan(120/150)` = **77.32°**;
- the vertical FOV is `2·atan(80/150)` = **56.14°**;
- the optical centre is (120, 79), one row above the screen centre, plus the camera offsets above;
- `+y` is down.

The speed effect can lower focal (to 86 when `g = 0`, a 108.7° horizontal FOV).

## Visibility: `FUN_03004828` + `FUN_03002614` (R10)

```
map = world+0x64 (8 bytes per sector); FUN_03004c1c(map, 8, 0)      // clears nothing: 8 >> 4 = 0
count = 1; i = 0
do { if list[i].depth < 5: count = expand(list[i], count, first = i == 0); i++ }
while i < count && count <= 24
world+0xEE = count

expand(parent):                                                         // FUN_03002614
  if sector.ceiling (+0x04) == 0: world+0xF6 = 1                        // sky visible
  world+0xE2..E8 = parent.left, right, top, bottom
  for each wall w of the sector, next = following wall (wrapping):
    if w.link (+0x30) == 0xFFFF: continue
    (x0, d0) = camera(w), (x1, d1) = camera(next)
    if d0 > 0x6000 || d1 > 0x6000: break                                // drops this sector's remaining portals
    if d0 < near && d1 < near: continue
    if d0 < near: x0 += FUN_03004ca4((near−d0)·(x1−x0), d1−d0); d0 = near; clipL
    elif d1 < near: x1 −= FUN_03004ca4((near−d1)·(x1−x0), d0−d1); d1 = near; clipR
    s0 = project(x0, d0); s1 = project(x1, d1)
    if first && s0 > s1: swap                                           // camera sector only
    if s0 > s1 || s1 < (u16)E2 || s0 >= (u16)E4: continue
    append { w.link,
             clipL ? world+0x58 : max(s0, parent.left),                 // a near-clipped side opens to the screen edge
             clipR ? world+0x5A : min(s1, parent.right),
             world+0x5C, world+0x5E,                                    // full screen height
             (parent.flags & 0x80) | (w.flags & 0x800 ? 0x80 : 0),
             parent.depth + 1 }
    map[link·8]++ (u8, wraps); if it is now < 8: map[link·8 + it] = new slot   // dead state, see quirks
```
There is no per-row clipping in the list (top/bottom are always the full screen), no check for sectors already listed (a sector can appear more than once, e.g. 773 twice in the reference frame), and no screen-edge test except against the parent's span. **Verified:** reproduces the reference frame's 9 entries exactly (test `visibility_reproduces_the_race_frame`).

## `draw_sector` (`FUN_0300224c`)

```
flags = sector+0x12 (byte); if sector+0x0A != 0xFFFF: flags = offsets[+0x0A].+8; if flags & 0x40: return
world+0xE2..E8 = entry.left, right, top, bottom; the rasteriser's rectangle likewise
pass 1: if world+0x78 (entity handlers) != 0: collect + draw entities; return
if flags & 8: for child = sector+0x24 chain (via each child's +0x24): draw_sector(entry with sector = child); return
r = transform_walls();  if r == 0: return                               // a corner deeper than 0x5FFF: nothing drawn
if r == 2: entry.flags |= 0x80                                          // every corner ≥ 0x200 deep
setup_wall_spans()
deferred = draw_sector_walls(pass = 0) & 0xFF                           // FUN_03002084
if floor (+0x08) || ceiling (+0x04):
  clip_flat_outline()                                                   // FUN_03000b44
  if floor:   fill = sector+0x0D; material = floor + runtime[floor].frame
              fill ? draw_flat_fill(floor, fill×4) : draw_flat_textured(floor)
  if ceiling: copy each clipped vertex's ceiling y over its floor y
              fill = sector+0x0C; … draw the ceiling the same way
if entry.flags & 0x80 && world+0x78: collect + draw entities
if deferred: draw_sector_walls(mask = deferred)                         // walls with span flag 2, after the flats
```

### `transform_walls` (`FUN_03000978`)

For each wall (moving piece `p` if `+0x2A != 0xFFFF`):
```
x = w.x + m9 (+ p.dx);  z = w.z + m11 (+ p.dz)
buf.x0 = x·m0 + z·m6 >> 14                                    (i16)
buf.top    = (w+0x08, w+0x0C) + m10 (+ p.top)                  (i16 each)
buf.bottom = (w+0x0A, w+0x0E) + m10 (+ p.bottom)
d = x·m2 + z·m8 >> 14;  if d > 0x5FFF: return 0;  if d < 0x200: near = true
buf.d0 = d
```
Then every entry's end (`x1`, `d1`) is the next entry's start, wrapping. The function returns 1 when some corner is nearer than 0x200, otherwise 2.

### `setup_wall_spans` (`FUN_030013ac`)

`wall` is `w`, the next wall is `n`, and `flat` is the floor material, or the ceiling material if there is no floor.
```
floor0/1 = w/n+0x38 + m10 (+ offsets.floor) (+ piece.floor);  ceil0/1 = w/n+0x3A + m10 (+ offsets.ceiling) (+ piece.ceiling)
fu0/fv0 = w+0x20/+0x24;  fu1/fv1 = n+0x20/+0x24
flags = w+0x2E
u0 = w.tex_u (+0x28)·128 + (i16 runtime[mat].u >> 1);  u1 = u0 + (w+0x40 << (mat.log2w − 1))
(u0, u1) swapped if flags & 2;  voff = w+0x42·128 + (runtime[mat].v >> 1)
if d0 < near:
   if d1 < near: flags |= 0x24                                       // wholly behind: nothing else, no flat vertex
   else: t = (near−d0)·recip[d1−d0]; clip start by t (FUN_0300121c: x, u, top, bottom, and v if flags & 0x80;
         flags |= 8); floor0, ceil0, fu0, fv0 += FUN_03004ccc(other − this, t); d0 = near
elif d1 < near: symmetric for the end (flags |= 0x10)
r0 = recip[d0]·focal >> 7;  r1 = recip[d1]·focal >> 7
x0 = cx + (i16)(r0·(x0>>1) >> 16);  x1 likewise                     // 16-bit wrapping
if the sector has a floor or ceiling:
   if flags & 8: emit {x0, cy + (r0·(floor0>>1) >> 16), cy + ((ceil0>>1)·r0 >> 16), recip[d0],
                       FUN_03004d20((fu0 << flat.log2w) >> 7, d0), FUN_03004d20((fv0 << flat.log2h) >> 7, d0)}
   emit the same for the end with r1, floor1, ceil1, d1, fu1, fv1
if flags & 0x2000 && x1 < x0: flags |= 2
if flags & 2: swap x0, x1                                            // heights are not swapped
if x0 < x1 && (u16)E2 <= x1 && x0 < (u16)E4: project top/bottom (start with r0, end with r1)
else flags |= 4
world+0xF2 = vertices emitted
```
**Verified:** the reference frame's wall buffer and flat outline for camera sector 760 are reproduced exactly (test `wall_setup_reproduces_the_race_wall_buffer`). This includes a left clip, a right clip and a wall wholly behind the camera.

## Walls: which are drawn and how (R7, R8)

`draw_sector_walls` (`FUN_03002084`, called `draw_sector_overlay` before) visits the walls in order, with `k` counting down from the wall count to 1:
```
rec = material[w+0x2C];  if piece: rec += piece.+0xC records;  f = piece ? piece.+0xE : w+0x2E
if rec.+0x00 == 0: skip                          // material 0: never drawn (every other material has +0 = its own index)
if f & 1: skip                                   // open portal
pass 0: if span.flags & 4: skip; if span.flags & 2: deferred |= 1 << k; skip
second call: draw only if deferred bit k is set
if !(top0 < E8 || top1 < E8 || E6 <= bottom0 || E6 <= bottom1): skip
rec += runtime[rec.+0].frame records             // animated textures
raster_wall_columns(span, log2w = rec+0x1E, log2h = rec+0x1F, texels = city texel base + rec+0x08,
                    column map = city texel base + rec+0x04, flags = w+0x2E)
```
- **Material 0 (R7):** exact. Walls with material 0 are never drawn: 1,023 walls, portals and solids alike.
- **Portal walls (R8):** exact. A wall with a link is drawn exactly like a solid wall when flag bit 0 is clear and the material is not 0. 661 portal walls qualify: steps, kerbs, fences, railings.
  - Each is one quad between the wall's own top and bottom heights. **There are no separate upper and lower parts**; the data models a step as the portal wall's own height range.
  - Portals with bit 0 set (1,951 walls, flags `0x601` typically) are open.
- **Deferred walls:** walls with span flag 2 are drawn after the floor and ceiling. That covers ROM flag 2 (10 walls) and back-facing walls with flag `0x2000` (2 walls). The mask is truncated to 8 bits, so such a wall with a countdown index of 8 or more (a sector with 8+ walls) is never drawn. This is a quirk to keep.
- **Flag 2** also swaps u0/u1 and the screen x ends, but not the heights.

### `raster_wall_columns` (`FUN_03000304`), not reimplemented

- **Columns:** walls are drawn in **2-pixel columns**, from `x0 >> 1` to `(x1 + 1) >> 1`. A wall of fewer than 2 columns is not drawn.
- **Horizontal clip:** the columns are clipped to `E2 >> 1 .. E4 >> 1`, and the wall is dropped entirely when it lies outside.
- **Per-column interpolation:** all steps divide by `columns + 1` (`FUN_03004ca4`), not `columns`:
  - `u/z = FUN_03004d20(u, d)`, linear across columns;
  - `1/z = recip[d]·256`;
  - top and bottom in 18.14.
- **Per column:**
  - **Stop:** stop the wall once `(1/z) >> 12 > 0x5FFF`.
  - **u (left pixel):** `recip[(1/z) >> 12] · ((u/z) >> 4) >> 23` = texels (`u >> 7`). The right pixel uses the half-step value. **Each pixel of a pair has its own texture column; the rows are shared.**
  - **Height:** `h = ((bottom + 0x3FFF) >> 14) − ((top − 0x3FFF) >> 14)`.
  - **v** (wall flag `0x80` clear, which is every wall): starts at `(wall+0x10) << 8` and steps by `FUN_03004cf8(wall+0x18, h)` per row, plus `voff·256`. The texel row is `v >> 15` masked to the height. So **v is in 1/128 texel rows: 16,384 = 128 rows**, not one texture.
    - The data agrees: 64-row textures use a v span of 8,192.
    - The end values `+0x14`/`+0x1C` are ignored unless flag `0x80` is set (perspective-correct v). No wall has it.
  - **Vertical clip:** clip rows to `E6..E8` (world `+0xE6`/`+0xE8`).
  - **Texel:** `texels + (colmap[u & (W−1)] << log2h) + row`.
- **Writes:** halfwords, with the two pixels from the two columns. When the texture's first texel is 0, `FUN_03004d48` is used: it skips the halfword if either pixel is 0 (colour-0 transparency, per pair). Otherwise `FUN_03004db0` writes opaquely.

## Floors and ceilings (R9)

`clip_flat_outline` (`FUN_03000b44`, called `draw_sector_pass_b` before) clips the outline to columns `E2..E4` (Sutherland–Hodgman against both lines, one edge at a time):
- it interpolates floor y, ceiling y, recip, u and v with `(Δ · t) >> 24`, where `t = distance · FUN_03004c40(dx)`;
- the result follows the outline in the buffer, and world `+0xF4` holds its length.

**Verified:** reproduces the reference frame's 6 clipped vertices exactly.

The rasterisers (`draw_flat_textured` `FUN_03002da0`, `draw_flat_fill` `FUN_03003180`) are not reimplemented:
- **Scanlines:** they walk the left and right edges (`FUN_03002a0c`) from the top vertex (`FUN_03005008`), over rows `E6..E8`. The polygon is skipped when its min y ≥ E8, its max y ≤ E6, or it is flat.
- **Textured spans:** written as **bytes to every other address** (`FUN_03004fa8`). Mode-4 VRAM stores a byte write into both pixels of the halfword, so **textured floors and ceilings are 120 pixels wide** (pairs of equal pixels).
  - Spans of up to 32 pixel pairs are affine between perspective-correct ends; longer spans are split into 16-pair perspective segments (`FUN_03002c40`).
  - Texel = `texels[((v >> (15 − log2w)) & ((H−1) << log2w)) + ((u & ((1 << (log2w+15)) − 1)) >> 15)]`.
  - From the vertex values the span u is `fu·W/16384` texels. **16,384 = one texture in u and in v: exact** (floors are normalised, unlike wall v).
  - Scroll adds `runtime.u << 7` and `runtime.v << 7` to the span values (1/256 texel).
- **Fill spans** (`FUN_03005094`) are full resolution. The colour is sector `+0x0D` for the floor and `+0x0C` for the ceiling, a palette index repeated four times. The city has 18 sectors with floor fill 112 and 7 with ceiling fill 235.
- **Flat heights come from wall `+0x38` (floor) and `+0x3A` (ceiling) at each corner, not from the wall's bottom/top.** They differ at 384 floor corners (kerbs, steps).
- A sector with neither floor nor ceiling (117 sectors) gets no flat vertices at all.

## Entities (partly decoded, not reimplemented)

`collect_sector_entities` (`FUN_03004e24`) → `sort_sector_entities` (`FUN_03001ae4`):
- It walks the sector's entity list (world `+0x0C`[sector], then entity `+0x02`) once per frame, using a visited bitmap at `DAT_03001c6c`.
- It runs the entity handler `world+0x78[entity+0x4E]` when entity `+0x08` has bit 1 set and bit 2 clear.
- When bit 4 of `+0x08` is set, it stores `(x'^2 + d^2) >> 8` in `+0x28` and, if `d > 0`, inserts the entity into a list sorted farthest first (links in `+0x04`).

`draw_sector_entities` (`FUN_03001cf0`), per entity with depth `d` (unsigned):
- **Clip span:** entity `+0x0A` bit 0 uses the whole screen (world `+0x58/+0x5A`); otherwise the portal's span.
- **Bit 4 (a "sector" entity):** if `d < 10000`, draw sector `+0x36` through a full-width entry.
- **Otherwise (a model):**
  - **Culls:** drawn only if `d < 0x2000` and the projected y is within `E6 − r .. E8 + r`, where `r = (recip[d]·focal >> 8 << 9) >> 16`.
  - **Forced LOD:** bit 1 forces `d = 0`.
  - **LOD with `+0x36 ≠ 0`:**
    - `d > 0x1000`: if bit `0x40` is set, `d = 0x200`; otherwise nothing is drawn. The code does select the next model record first.
    - `d ≥ 0x200`: model `+0x36`;
    - otherwise: model `+0x36 − 1`.
  - **Second model:** entity `+0x64` adds one (sign selects the order), with mesh slots `+0x88` / `+0x88 + 1`.
  - Entities with `+0x36 == 0` are not drawn by this path.
  - **Hypothesis:** `+0x36` is the model index for the car's LOD pair. Cars beyond depth 4,096 (about 85 m) are not drawn unless flag `0x40` is set. Not checked against a frame.

## Camera sector (`FUN_03000800`, `FUN_030047a8`)

`point_in_sector`: for every wall edge `prev → cur`, starting with `prev` = the last wall, the point must satisfy `(cur.z−prev.z)·(px−prev.x) − (pz−prev.z)·(cur.x−prev.x) ≥ 0`.

`FUN_03000800` looks for world (`+0xC0`, `+0xC8`), in this order:
1. sector world `+0xEA` itself;
2. each wall's `+0x32` neighbour, skipping walls whose flags have `0x1000`. The flags come from the moving piece's `+0xE` when the wall names one.

A neighbour with floor 0 is replaced by its `+0x20` alias when that is set. Otherwise the result is `0xFFFF`. **Verified** on the reference frame (`camera_sector_search`).

## Quirks to keep (all exact behaviour)

- The sector map at world `+0x64` is never cleared: `FUN_03004c1c(map, 8, 0)` clears 0 blocks. Its counts grow every frame, and its only reader (`FUN_03004ef0`, the span merge) is never called. It is harmless, but for sectors ≥ 1024 its writes land past `0x0201B090`, in the flat vertex buffer (world `+0x6C` = `0x0201B094`), which is rebuilt per sector.
- A portal wall deeper than `0x6000` stops the expansion of the rest of that sector's walls.
- Near clipping of a portal opens the child's span to the screen edge on that side.
- All interpolation steps divide by `n + 1`, and all reciprocals round down.
- The 8-bit deferred-wall mask.

## Verification and provenance

- **Reference:** the mGBA savestate `data/work/e5298b24/mgba/race.ss`, with RAM dumps `race.iwram.bin` / `race.wram.bin` / `race.vram.bin`. The pc was `0x03003A7C`, inside `raster_polygon`, during pass 1: walls and floors of every sector were already drawn.
- **Buffers used:** the visible list at `0x02018C8C`, the wall buffer at `0x02017288`, the flat buffer at `0x0201B094`, the view at `0x03000080` and the camera at `0x030057A0`. At dump time the buffers hold camera sector 760, the last sector drawn in pass 0.
- **Scripts** (not in git): `data/scratch/frame.py`, `flat.py`, `map.py` and `walls.py` in the renderer worktree.
- **Tests:** `cargo test -p nfsgba-formats render` (needs the ROM vault).

## Not done / NOT 1:1 if used as is

- The column and span rasterisers are specified above but not reimplemented; no pixel comparison has been made against `race.vram.bin`.
- The entity LOD path is decoded but not checked against a frame. Entity `+0x36`/`+0x64`/`+0x88` meanings are hypotheses.
- The focal speed effect's input `g` (`FUN_0815fc38`, `FUN_0815fadc`) is not decoded.
- The camera offsets `0x030056B8`, `0x030053A0`, `0x030055F8`, `0x03005390` and `0x03005FA4` are not traced to their writers.
- The runtime tables (world `+0x18`, `+0x1C`, `+0x48`) are inputs. Their writers (door/animation code) are not decoded.

## Integration notes

For whoever merges this. These go into files this worktree may not edit.

### address-map.md

- **ROM table:**
  - `0x7C45F0`: "reciprocal table `recip[k] = 2^24/(k+1)`; every projection and divide (renderer.md)". Replaces "(light interpolation, likely more)".
  - `0x7BFC68`: "camera probe vector (0, 0, 72) for the start-sector search".
  - In the RAM table, add the camera variables:
    - `0x03005F9C`: yaw, 14-bit;
    - `0x03005FA4`: camera height offset (8.8);
    - `0x030055F8`: camera mode;
    - `0x03005390`/`0x03005392`: screen-centre offsets;
    - `0x030056B8`: vertical offset, clamped ±32;
    - `0x03006148`: when set, the camera sits `0x82` above the player instead of 16.
- **RAM table:**
  - `0x03000080`: view struct (renderer.md);
  - `0x030057A0`: camera matrix in the race;
  - `0x02018C8C`: visible list (64 × 16 bytes);
  - `0x02019090`: sector map (8 bytes/sector, never cleared, unused);
  - `0x02017288`: wall buffer;
  - `0x0201B094`: flat vertex buffer;
  - `0x02012404`: moving pieces;
  - `0x020123F8`: sector offsets;
  - `0x02013348`: material runtime table;
  - `0x03006920`: 32 bytes cleared per frame by `FUN_030048c8`;
  - `0x03006490`: → IWRAM image base (`0x03000220`) for the dispatcher.
- **World struct:**
  - `+0x18`: moving wall pieces (0x20: dx, dz, ceiling dy, floor dy, top dy, bottom dy, material offset, flags);
  - `+0x1C`: sector offsets (0x14: `+4` ceiling dy, `+6` floor dy, `+8` flags replacing sector `+0x12`, `0x40` = hidden);
  - `+0x48`: runtime entry `+2` animation frame, `+4`/`+6` u/v scroll;
  - `+0x50`: view `+0x0C` pitch 240, `+0x10` near = 64, `+0x1C` focal = 150;
  - `+0x58..+0x5E`: screen rectangle (0, 240, 0, 159);
  - `+0x60`: visible list;
  - `+0x64`: sector map;
  - `+0x6C`: flat vertex buffer (0x18 per vertex: x, floor y, ceiling y, recip, u/z, v/z);
  - `+0xC0`/`+0xC8`: point searched by `FUN_03000800`, scratch shared by the camera and entity code;
  - `+0xE2..+0xE8`: current portal span (left, right, top, bottom);
  - `+0xEA`: camera sector;
  - `+0xEE`: visible count;
  - `+0xF0`: 0 per frame;
  - `+0xF2`/`+0xF4`: flat outline / clipped outline lengths;
  - `+0xF6`: sky visible.
- **Wall record:**
  - `+0x2A`: moving piece (`0xFFFF` = none);
  - `+0x2E` flags: 1 = open portal / not drawn, 2 = flipped, `0x80` = perspective v, `0x800` = portal forces entities into pass 0, `0x1000` = blocks the camera-sector search, `0x2000` = two-sided;
  - `+0x32`: neighbour for the camera-sector search;
  - `+0x38`/`+0x3A`: floor/ceiling height at this corner;
  - `+0x42`: v offset.
- **Sector record:**
  - `+0x04` ceiling / `+0x08` floor material (0 = none);
  - `+0x0A` offsets record;
  - `+0x0C`/`+0x0D` ceiling/floor fill colour;
  - `+0x12` flags (8 = container, drawn through the `+0x24` chain);
  - `+0x20` alias; `+0x22` start-sector alias.
- **Material record:**
  - `+0x00` runtime slot (= own index; 0 = never drawn);
  - `+0x04` column map offset;
  - `+0x08` texel offset;
  - `+0x1E`/`+0x1F` log2 width/height.
- **Entity:**
  - `+0x02`: next in the sector's list;
  - `+0x04`: next in the frame's sorted draw list;
  - `+0x0A` bit 2: drawn this frame.

### symbols.csv (new rows)

```
0x03004828,build_visible_sectors,function,visible-sector list from the camera sector (<=25 entries, portal depth <5)
0x03002614,expand_portal,function,appends the sectors seen through one list entry's portal walls
0x03004ef0,merge_portal_spans,function,unions spans of repeated sectors via the sector map (never called in Carbon)
0x030048c8,draw_visible_sectors,function,pass 0 draw_sector back to front then pass 1 entities
0x03000800,find_camera_sector,function,sector of world+0xC0/+0xC8 from world+0xEA or its +0x32 neighbours
0x030047a8,point_in_sector,function,edge cross-product test against a sector's walls
0x0300121c,clip_wall_span,function,moves a wall span's start or end to the near plane by a 2.24 fraction
0x03004ca4,recip_div,function,(a*recip[b])>>24 = a/(b+1)
0x03004ccc,mul_frac24,function,(a*(u32)t)>>24
0x03004cf8,recip_div_8,function,(a*recip[b])>>16 = (a<<8)/(b+1)
0x03004d20,recip_div_16,function,(a*recip[b])>>8 = (a<<16)/(b+1)
0x03004c40,recip_wide,function,recip[n] or recip[n>>1]>>1 above 0x7FFE
0x03004c1c,fill_blocks16,function,stores a word into n>>4 16-byte blocks
0x03002a0c,flat_edge_step,function,steps a flat polygon's left or right edge to its next vertex
0x03005008,flat_top_vertex,function,finds a flat polygon's top vertex and rejects it outside E6..E8
0x03004fa8,flat_span_affine,function,textured flat span: byte writes every 2 bytes (120-wide floors)
0x03002c40,flat_span_perspective,function,long flat span in 16-pair perspective segments
0x03005094,fill_span,function,solid-colour span at full resolution
0x03004d48,wall_column_transparent,function,wall column pair; skips pairs with a colour-0 texel
0x03004db0,wall_column_opaque,function,wall column pair
0x08137cb0,race_camera_update,function,chase camera: matrix, focal effect, screen centre, start sector, list entry 0
0x08137ac8,look_at_camera_update,function,fixed camera looking at the player (focal 150)
0x0815e674,iwram_call,function,calls IWRAM function *0x03006490+0xE4+index*4
0x0815e690,iwram_call_ret,function,iwram_call with a return value
```
Rename existing rows:
- `0x03002084` `draw_sector_overlay` → `draw_sector_walls` ("draws the sector's walls; defers span-flag-2 walls (8-bit mask)");
- `0x03000b44` `draw_sector_pass_b` → `clip_flat_outline` ("clips the floor/ceiling outline to the portal's columns");
- `0x08138a9c` comment: add "also initialises the view struct (near 64, focal 150)".

### FIDELITY.md

- **R7:** close. Material 0 is never drawn (`draw_sector_walls` skips material records whose `+0` is 0; only material 0 has that); walls with flag bit 0 are not drawn either.
- **R8:** exact rule found; the viewer still needs it. Portal walls with flag bit 0 clear and material ≠ 0 are drawn like solid walls over their own top..bottom. There are no upper/lower parts. 661 such walls.
- **R9:** close. Flat UV 16,384 = one texture in u and v (`setup_wall_spans` + `flat_span_affine` masks). Fill colours are sector `+0x0D`/`+0x0C`.
- **R10:** algorithm exact and verified (`render::visible_sectors`, `transform_walls`); the viewer still draws everything.
- **R11:** focal 150 → 77.32° × 56.14° FOV, principal point (120, 79), near 64, no pitch or roll; the viewer still uses the Bevy default.
- **New entries:**
  - **Wall v units (reopen "Wall textures and wall UVs"):** `Wall::uv` normalises v by 16,384. The game's wall v is in 1/128 texel rows (`v >> 7`), so 16,384 = 128 rows. The 560 drawn walls with 64-row textures (v span 8,192) show only half their texture in the viewer. Exact normalised v = `v / (128 · texture_height)`, bottom = `+0x10 + +0x18`.
  - **Floor heights:** the viewer puts floors at the wall's bottom. The game uses wall `+0x38` (floor) and `+0x3A` (ceiling) per corner; they differ at 384 floor corners.
  - **Resolution:** floors and ceilings render at 120×160 (byte writes duplicated by VRAM); wall edges at 120 columns with 240-column texturing. Only relevant for a pixel-exact 240×160 mode.
  - **Draw limits:** the viewer draws the whole city. The game drops sectors with a corner deeper than `0x5FFF`, stops each wall column at `1/z > 0x5FFF`, lists at most about 25 sectors, 5 portals deep, and does not draw cars beyond depth `0x1000` (hypothesis, see Entities).
