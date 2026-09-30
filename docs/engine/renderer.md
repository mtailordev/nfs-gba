# World renderer (Carbon `BN7E`, race IWRAM code)

**Status:**
- **Exact and verified against the reference race frame:** the whole world pass (pass 0). It is reimplemented in `crates/nfsgba-formats/src/render.rs`: projection, visible-sector list, wall transform, setup and near clipping, which walls draw, the wall column rasteriser, the floor/ceiling outline, its clip and the flat rasterisers, the painter's order, and the camera-sector search. The tests reproduce `data/work/e5298b24/mgba/race.ss` value for value:
  - visible list (world `+0x60`), all 9 entries;
  - wall draw buffer (world `+0x68`) of the camera sector;
  - flat outline (world `+0x6C`) and its clipped copy;
  - **the frame itself**: all 33,350 world pixels of the 240×160 frame match VRAM. The only other areas are the sky and row 159.
- **Exact and verified: the entity draw** (cars and spoilers; pass 1 and the entities of `0x80` entries in pass 0), in `crates/nfsgba-formats/src/render/entities.rs` (section "Entities" below). The mid-frame reference dump is reproduced up to the exact span it was taken at, and 16 whole frames captured at frame boundaries (opponents near and far, every LOD branch) match the game pixel for pixel.
- Everything here comes from the ARM code (Ghidra decompilation, checked against the disassembly where it matters) plus the reference dumps. Hypotheses are labelled.

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

Reimplemented as `render::draw_sector`; `render::draw_world` is `FUN_030048c8` (both passes).

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
if entry.flags & 0x80 && world+0x78: collect + draw entities            // may leave E2/E4 at the whole screen
if deferred: draw_sector_walls(mask = deferred)                         // walls with span flag 2, after the flats,
                                                                        // clipped to E2/E4 as the entities left them
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
if flags & 2: swap x0, x1                                            // the rasteriser reads the rest swapped
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
- **Flag 2:**
  - `setup_wall_spans` swaps u0/u1 and the screen x ends;
  - the rasteriser then reads depth, u, top and bottom in swapped order, so the drawn wall is consistent;
  - the v fields are read unswapped.

### `raster_wall_columns` (`FUN_03000304`)

Reimplemented as `render::raster_wall_columns`, with the draw loop as `render::draw_sector_walls`.

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
    - Pixel-checked on 128-row textures (the reference frame) and 64-row ones (`render2/w64-*`: three views facing 64-row walls, drawn by the game's code in the function oracle, `cases.py render2`).
    - The end values `+0x14`/`+0x1C` are ignored unless flag `0x80` is set (perspective-correct v). No wall has it.
  - **Vertical clip:** clip rows to `E6..E8` (world `+0xE6`/`+0xE8`).
  - **Texel:** `texels + (colmap[u & (W−1)] << log2h) + row`.
- **Writes:** halfwords, with the two pixels from the two columns. When the texture's first texel is 0, `FUN_03004d48` is used: it skips the halfword if either pixel is 0 (colour-0 transparency, per pair). Otherwise `FUN_03004db0` writes opaquely.

## Floors and ceilings (R9)

`clip_flat_outline` (`FUN_03000b44`, called `draw_sector_pass_b` before) clips the outline to columns `E2..E4` (Sutherland–Hodgman against both lines, one edge at a time):
- it interpolates floor y, ceiling y, recip, u and v with `(Δ · t) >> 24`, where `t = distance · FUN_03004c40(dx)`;
- the result follows the outline in the buffer, and world `+0xF4` holds its length.

**Verified:** reproduces the reference frame's 6 clipped vertices exactly.

The rasterisers (`draw_flat_textured` `FUN_03002da0`, `draw_flat_fill` `FUN_03003180`) are reimplemented as `render::draw_sector_flats`:
- **Scanlines:** they walk the left and right edges (`FUN_03002a0c`) from the top vertex (`FUN_03005008`), over rows `E6` to `E8` exclusive. So with the race rectangle **row 159 is never drawn** by the world renderer.
  - The polygon is skipped when its min y ≥ E8, its max y ≤ E6, or it is flat.
  - The two edges share one count of outline vertices.
  - For the floor, the left edge walks the outline backwards; for the ceiling, forwards.
- **Textured spans:** written as **bytes to every other address** (`FUN_03004fa8`). Mode-4 VRAM stores a byte write into both pixels of the halfword, so **textured floors and ceilings are 120 pixels wide** (pairs of equal pixels).
  - Spans of up to 32 pixel pairs are affine between perspective-correct ends; longer spans are split into 16-pair perspective segments (`FUN_03002c40`).
  - Texel = `texels[((v >> (15 − log2w)) & ((H−1) << log2w)) + ((u & ((1 << (log2w+15)) − 1)) >> 15)]`.
  - From the vertex values the span u is `fu·W/16384` texels. **16,384 = one texture in u and in v: exact** (floors are normalised, unlike wall v).
  - Scroll adds `runtime.u << 7` and `runtime.v << 7` to the span values (1/256 texel).
- **Fill spans** (`FUN_03005094`) are full resolution. The colour is sector `+0x0D` for the floor and `+0x0C` for the ceiling, a palette index repeated four times. The city has 18 sectors with floor fill 112 and 7 with ceiling fill 235.
- **Flat heights come from wall `+0x38` (floor) and `+0x3A` (ceiling) at each corner, not from the wall's bottom/top.** They differ at 384 floor corners (kerbs, steps).
- A sector with neither floor nor ceiling (117 sectors) gets no flat vertices at all.

## Entities (R12)

Reimplemented exactly in `render/entities.rs` (`draw_entities`, `project_model`, `Scene`, `Entity`); `draw_sector` calls it in pass 1 and for `0x80` entries in pass 0. The entity array is world `+0x3C` (0xA4 bytes each, `0x0201431C` in races; count world `+0xF8` + `+0xFA`).

### Entity fields the renderer uses (confirmed from the code and the captures)

| Offset | Meaning |
|---|---|
| `+0x00` | own index (the draw order links through it) |
| `+0x02` | next entity in its sector's list; the list heads are world `+0x0C` (u16 per sector) |
| `+0x04` | next entity in the sector's draw order (written by the sort) |
| `+0x08` | state: bit 0 runs the handler `world+0x78[+0x4E]` during the sort unless bit 1 is set; bit 2 takes part in the sort |
| `+0x0A` | flags: **bit 0** clip to the whole screen (world `+0x58/+0x5A`) instead of the portal span; **bit 1** depth taken as 0 for LOD and the far cut (always the near model; opponents and markers have it, `0x22`/`0x02`); **bit 2** passed the screen cull this frame (written; the next frame's slot assignment reads and clears it); **bit 3** texture in RAM (`+0x84`) instead of the ROM; **bit 4** draw sector `+0x36` instead of a model; **bit 5** matrix from the driver's physics orientation (`FUN_0814da68`); **bit 6** beyond depth 0x1000, draw the far model instead of nothing. The player has `0x0D`, opponents `0x22` |
| `+0x0C/+0x10/+0x14` | position, 8.8 fixed point |
| `+0x28` | sort key `(x'² + d²) >> 8`, camera space (written) |
| `+0x36` | **far model** (drawn at depth ≥ 0x200); the near model is `+0x36 − 1`. For cars it is the car's low-detail model, so the race draws the medium model near and the low one far; the high-detail model is never drawn in a race. Bit 4 entities: a sector index |
| `+0x44` (high byte), `+0x46` | material steps added to `+0x48` (0 in every capture) |
| `+0x48` | vehicle material (the atlas: its size, log2 width `+0x1E`, height `+0x0E` and, for ROM textures, texels `+0x08`); 0 = not drawn |
| `+0x4E` | handler index (world `+0x78` = the Thumb table `0x7F38B8`, which answers OPEN-QUESTIONS 11) |
| `+0x64` | second model on matrix slot `+0x88 + 1`: after the first model when positive (the player's spoiler: 12 for the Cobalt, 4 for car 0), before it as model `−n` when negative |
| `+0x84` | RAM address of the unpacked atlas (bit 3) |
| `+0x88` | matrix slot (world `+0xFC`, 0x30 bytes each); `0xFF`: not drawn |

**Matrix slots** are game-side inputs, built each frame before the draw: `race_frame_update` (`FUN_0813a954`) calls `draw_vehicle` (`0x0814bc30`) for the player and `FUN_0814eba0` for every other entity. `FUN_0814eba0` gives the entity the next slot (counter `0x03005394`; `0xFF` from 0x40 on, or when `+0x36 == 0` or `+0x0A` bit 2 was not set by the last frame's draw, so a car appears one frame after it first passes the screen cull), builds the matrix with `FUN_0814da68`, then clears `+0x0A` bit 2 (`& 0xFFFB`). `FUN_0814da68` (and `FUN_0814ec0c`) only write the matrix when bit 1 is set or the depth is at most `0xDAC`: beyond that the slot keeps whatever it held. The matrix is `A(+0x30) · camera · B(+0x32)`: two rotation matrices built by IWRAM helpers from the angles `+0x30` and `+0x32` (axes not checked), multiplied through `FUN_0815e6c8`; with bit 5 (and `0x0300610C` non-zero) it is the driver's physics matrix (driver `+0x128…+0x148`, `<< 2`) times the camera (`FUN_08160434`). The translation is the camera-space position.

### Per frame

1. `draw_visible_sectors` clears the visited bitmap `0x03006920` (32 bytes, one bit per entity), so an entity is drawn at most once per frame even when its sector is listed twice.
2. **`collect_sector_entities`** (`FUN_03004e24`), per `draw_sector` call: resets the draw list (head `0x03006940` = `0xFFFF`, greatest key `0x03006900` = −1) and sorts the sector's list, or for a container (sector `+0x12` bit 3, from the ROM) the list of each child along the `+0x24` chain.
3. **`sort_sector_entities`** (`FUN_03001ae4`), along `+0x02`: skip visited entities (then mark them) and those with material 0; run the handler if `+0x08` has bit 0 and not bit 1; if `+0x08` bit 2: `x' = m9 + (x >> 8)`, `z' = m11 + (z >> 8)`, `d = (m2·x' + m8·z') >> 14`, `c = (m0·x' + m6·z') >> 14`, key = `(c² + d²) >> 8` → `+0x28`; if `d > 0` insert: a key ≥ the greatest so far becomes the head, otherwise it goes after the last entity with a greater key (so equal keys go first). The result is farthest first.
4. **`draw_sector_entities`** (`FUN_03001cf0`), along `+0x04`:
   ```
   E2/E4 and rect[0]/rect[2] = bit 0 ? world+0x58/+0x5A : the entry's span   // stays set after the loop
   d = (m2·(m9 + x>>8) + m8·(m11 + z>>8)) >> 14;  y' = m10 + (y >> 8)
   bit 4: if (u32)d ≤ 9999: draw_sector(pass 0) of sector +0x36 through {E2, E4, E6, E8}
   else if (u32)d < 0x2000:
     r = recip[d]·focal >> 8;  sy = cy + (r·y' >> 16);  margin = (r << 9) >> 16
     if E6 − margin < sy < E8 + margin:                                  // E6/E8 unsigned
       material = +0x48;  if bit 1: d = 0;  +0x0A |= 4
       if +0x36 ≠ 0 && d > 0x1000: bit 6 ? d = 0x200 : material += 1       // (the step is dead: see below)
       material += +0x46 + (+0x44 >> 8)
       texture = bit 3 ? +0x84 : vehicle texel base (world +0x04) + material+0x08
       if d > 0x1000 || +0x36 == 0 || +0x88 == 0xFF: next
       model = +0x36 + (d ≥ 0x200) − 1
       draw_model_polygons(model, slot +0x88), and the +0x64 model on slot +0x88 + 1 (order by its sign)
   ```
   Beyond 0x1000 without bit 6 the code also offsets a RAM texture by the previous material's width × height, but then draws nothing, so that path has no effect.
5. **`draw_model_polygons`** (`FUN_03004190`): `transform_model_vertices` (`FUN_03004018`) projects the model's vertices into world `+0xA0` (`0x030053F0`): camera = `M·v >> 14` plus the translation (columns `m0,m3,m6` / `m1,m4,m7` / `m2,m5,m8`), `r = recip[d]·focal >> 8`, screen = centre + `(r·c) >> 16` stored as halfwords. **If any vertex's depth, unsigned, is beyond 0x6000 the whole model is dropped.** Then per polygon: size byte `n`, the first three projected corners give `cross = (y1−y0)(x2−x0) − (y2−y0)(x1−x0)`; `cross < 0` is culled. UVs come from world `+0x90` plus model `+0x08`·4 when model flag bit 0 is set.
6. **`raster_polygon`** (`FUN_03003808`), with the rectangle `rect = {E2, E6, E4, E8}` (IWRAM `0x030053D0`):
   - **Outcode** (`FUN_0300514c`): 4 (dropped) when no corner has `left ≤ x < right` or none has `top ≤ y < bottom`, **even if the polygon covers the screen**; otherwise bit 0 when a corner is outside in x, bit 1 in y.
   - **Clip** (`FUN_03003d70` / `FUN_03003acc`): Sutherland–Hodgman in x (if bit 0) then y (if bit 1), against `lo ≤ c ≤ hi` (**inclusive**, unlike the outcode). An edge end is moved by `t = (recip[den] >> 8)·num` (16.16), the other coordinate and u, v by `p + ((q−p)·t >> 16)`. Results are stored as halfwords (scratch `0x03006440`/`0x03006390`, then `0x03006460`/`0x030063B0`, identity indices `0x03006430`). An empty result draws nothing.
   - **Top corner** (`FUN_03005320`): the first with the least y.
   - **Edges:** `a` walks the corners backwards (`FUN_03003464`), `b` forwards (`FUN_03003634`), sharing one count of edges left (`0x03006904`, starting at `n`); horizontal edges are skipped; the polygon ends when the count runs out or the next corner is higher. Each edge: `r = recip[dy] >> 8`, x in 16.16 (`dx = r·Δx`), u = `(uv.u << log2w) >> 7` and v = `(uv.v · height) >> 7` in 1/256 texels, `du = (r·Δu) >> 16`, `dv = (r·Δv) >> 16`, drawing `dy` rows.
   - **Rows:** a span only when `a.x > b.x`, from `b.x >> 16` for `((a.x + 0xFFFF) >> 16) − (b.x >> 16)` pixels, starting at `b`'s u, v with `du = (recip[w] >> 8)·(a.u − b.u) >> 16` (likewise dv).
   - **Span** (`FUN_030051f4`): texel pointer `texture + (v >> 8 << log2w) + (u >> 8)`, stepped by the integer parts of du, dv plus the carries of their 8-bit fractions: **no wrap in u or v**, full resolution (halfword read-modify-write at odd ends). Row 159 is never drawn.
   - Globals: `0x030068F0` log2w, `0x030068F4` edge x, `0x030068F8` edge v, `0x030068FC` corner index, `0x03006908` edge du, `0x0300690C` edge dx, `0x03006910` edge u, `0x03006914` span dv, `0x03006944` edge rows, `0x03006948` edge dv, `0x0300694C` span du.

**Wheels and shadows** (R12) are not a separate pass: they are polygons of the car models (the medium/low models and their atlases; the opponents' 128×100 atlases include the wheels). After `draw_visible_sectors` the game writes nothing more into the page (checked on 3 captures), and no OBJ sprite covers the cars in the reference frame. Car effects such as `FUN_0814e628`/`FUN_0814e7b0` are sprites (the 2D layer).

### Quirks to keep

- The outcode drops a polygon with no corner inside in x (or in y), even one covering the screen.
- The outcode excludes `right`/`bottom`, the clipper includes them.
- The model drop at vertex depth > 0x6000, and the unsigned depth tests (negative depths are culled).
- An entity drawn with bit 0 leaves E2/E4 at the whole screen for the sector's deferred walls.
- A car appears one frame after it first passes the screen cull (slot assignment); beyond depth 0xDAC without bit 1 its slot keeps a stale matrix.

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
  - `world_pixels_match_the_race_frame` also needs `race.vram.bin`. It renders pass 0 of the frame with `visible_sectors` + `draw_world` (no entities) and compares against the VRAM page being drawn (`0x0600A000`).
  - It draws on two backgrounds to tell written pixels from unwritten ones.
  - `race_frame_matches_up_to_the_car_span_being_drawn` (entities): the dump was taken inside pass 1 (`mgba/log.txt`: pc `0x03003A7C`, just after a span call in `raster_polygon`; its row pointer `0x06011080` is row 120; the stack holds `draw_model_polygons` with 24 of model 9's 68 polygons left, i.e. polygon 44). Of the frame's 346 entity spans, cutting at exactly one count (184) reproduces every written pixel of the page, and that span is on row 120 of polygon 44.
  - `probe_frames_match`: every capture in `data/work/e5298b24/entity-draw/` (below) must match on every pixel the renderer writes.

### Frame captures (`data/work/e5298b24/entity-draw/`)

`tools/recorders/frame.lua` (loaded with `NFSGBA_MGBA_EXTRA`, armed by writing `NAME [ADDR=VALUE …]` to `probe.txt`, atomically) keeps IWRAM and EWRAM at the start of `draw_visible_sectors` and VRAM at its end, the page again one frame later (`NAME.final.bin`), and can apply 16-bit writes first to steer the renderer into paths the data does not reach. States: `race.ss` (the reference race) and `g0.ss` (a Quick Play from `mainmenu.ss`, A ×4, saved during the countdown). All 16 match on every written pixel:

| Capture | What | Entities shown (depth) |
|---|---|---|
| `r0` | reference race, frame boundary | player (0x157), 1679 px |
| `c1`…`c7` | race start, `g0` + 60…470 frames | opponents near model at every depth (bit 1): 0x144 (1829 px) … 0x194F (6 px), none at 0x1C54 |
| `e1`…`e5` | 12 s into another Quick Play | a marker entity (model 92, 30–318 px) |
| `p1`, `p2` | opponents' bit 1 cleared | near model at 0x16F / 0x1F9 |
| `p3`, `p5` | bit 1 cleared | **far model** at 0x4DE…0xB54 |
| `p4` | bits 1 cleared, 6 set, below 0x1000 | far model, same pixels as `p3` |
| `p6` | bit 1 cleared, 0x1328…0x1481 | **nothing drawn** (the 0x1000 cut) |
| `p7` | bit 6 set, 0x136C…0x14CD | **far model drawn** beyond the cut |

`p*` write `+0x0A` of entities 1–3 (`0x020143CA`, `0x0201446E`, `0x02014512`). `p1`, `p3`, `p7`: `NAME.final.bin` equals the page at the end of the world draw.

## Not done / NOT 1:1 if used as is

- **Entities:**
  - the entity handlers (world `+0x78`, Thumb game code) are not reimplemented: `Scene::handler` defaults to doing nothing. No entity in any capture calls one (all have `+0x08` bit 1).
  - bit 4 (sector) entities, material steps `+0x44`/`+0x46`, negative `+0x64` and textures outside ROM/EWRAM follow the code but appear in no capture.
  - the matrix slots are inputs: their builders (`draw_vehicle`, `FUN_0814eba0` → `FUN_0814da68`, `FUN_0814ec0c`) are game code, not reimplemented here.
- The pixel check covers 17 frames from two races: 128-row wall textures, textured floors. Fill-colour flats, ceilings, deferred walls, moving pieces, animated or scrolled materials and transparent textures are reimplemented from the code but not yet exercised against a frame.
- The focal speed effect's input `g` (`FUN_0815fc38`, `FUN_0815fadc`) is not decoded.
- The camera offsets `0x030056B8`, `0x030053A0`, `0x030055F8`, `0x03005390` and `0x03005FA4` are not traced to their writers.
- The runtime tables (world `+0x18`, `+0x1C`, `+0x48`) are inputs. The moving pieces are written by `moving_pieces_init` and the wall breaks (`nfsgba-sim` `walls.rs`); the other two never change in Carbon: no sector names an offsets record, and the material table is only allocated zeroed and cleared (`race_init`, `load_menu_descriptor`), its level-animation step `race_frame_nop_a` (`0x0813B62C`) being empty (`reach::renderer_runtime_tables_never_change`).
