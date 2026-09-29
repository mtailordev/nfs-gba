# City sectors (Carbon `BN7E`)

**Status: the structure is verified; some field meanings are hypotheses.**
- **From the code:** the layout was read from the decompiled IWRAM renderer (`FUN_0300224c` scene, `FUN_03000978` wall transform, `FUN_030013ac` wall drawing).
- **From the data:** sectors tile the wall array exactly, and **2,682 of 2,688 portal walls** have their edge, reversed, in the sector they link to (`tools/test_tools.py::test_city_sectors_are_contiguous_and_portals_match`). Top-down and 3D renders show a coherent street network.
- **Parser:** `tools/city.py` exports `city.svg` (top-down) and `city.obj` (walls and floors) to `$NFSGBA_DATA/out/city/<sha1-8>/`.

## What it is

The city is a **2.5D portal/sector engine**, like Doom or Build:
- **Sectors:** the world is 1,113 convex-ish sectors (road cells), each a closed polygon of walls.
- **Walls:** a wall is **solid** (a building facade, textured and drawn as a vertical quad between its top and bottom heights) or a **portal** into a neighbouring sector.
- **Drawing:** the renderer starts in the camera's sector and recurses through portals.
- **Cars:** vehicles are true 3D polygon models drawn into the same view ([vehicle-models.md](vehicle-models.md)).
- **Layout:** the sectors form about ten separate districts, which are the race areas.

## Where

These come from the level descriptor at `0x7F2B08` (see [vehicle-models.md](vehicle-models.md)). Every descriptor shares them.

| Record | World (`0x030000C0`) | ROM | Array |
|---|---|---|---|
| `+0x18` | `+0x14` | `0x723DD4` | **sectors**: 1,113 × 0x30 bytes |
| `+0x14` | `+0x10` | `0x730E84` | **walls**: 4,423 × 0x44 bytes |
| `+0x1C` | `+0x20` | `0x720DE8` | **materials**: 0x24-byte records (wall `+0x2C` indexes them) |
| `+0x08` | `+0x00` | `0x47BC6C` | **city texel base**: material `+0x08` is an offset from here |
| `+0x00` | | `0x71F1E8` | **city palette**: 256 × BGR555. Colour 0 is magenta `0x7C1F` (transparent) |

## Materials and textures (verified)

The material table (level record `+0x1C`) holds **227 self-indexed records** of 0x24 bytes, filling the space up to the next table at `0x722DD4`. Everything below was checked by rendering; `nfsgba_formats::city_textures` decodes all of it.

| Offset | Meaning |
|---|---|
| `+0x00` | u16 runtime slot = its own index (records are self-indexed); `draw_sector_walls` never draws a material whose slot is 0, and only material 0 has that |
| `+0x02` | u16 kind. **0 = plain:** row-major texels. **2 = column-mapped:** the building facades. **1:** a sky gradient (see below) |
| `+0x04` | kind 2: offset (from the texel base) of the **column map**, one byte per u |
| `+0x08` | offset of the texels, from the texel base (level record `+0x08`) |
| `+0x0C`, `+0x0E` | u16 width, height |
| `+0x1E`, `+0x1F` | log2 width, log2 height (the rasteriser's masks and shifts) |

- **Column-mapped textures:** wall texture u picks a column through `map[u]`. That column's `height` texels are stored contiguously at `texels + column × height` (column-major), and repeated columns are stored only once. For example, material 5 (512×128) has 51 unique columns. `FUN_03000304` (the wall rasteriser) reads textures exactly this way.
- **Kinds in use:** 107 facades of 512×128 and 30 of 512×64 (kind 2); floor textures of 64×64, 128×128, 512×128 and 512×512 (kind 0); 8×8 flat textures.
- **Palette:** level record `+0x00` (`0x71F1E8`, 256 × BGR555), picked by the environment. Index 0 (magenta `0x7C1F`) is the backdrop, not a colour: transparent wall textures (first texel 0) skip a 2-pixel pair unless both texels are non-zero, and opaque ones let the backdrop (sky gradient) show through (FIDELITY R14).
- **Runtime palette:** in a race the palette is tinted every frame by the wall light at the player's position (`apply_sector_light_to_palette`, exact in `nfsgba_formats::tint_palette`); car slots come from `load_car_palettes` (`docs/formats/car-paint.md`).
- **Material 0 means "not drawn"** (confirmed from code and in the reference frame). `draw_sector` runs a sector's floor/ceiling passes only if `+0x08` or `+0x04` is non-zero, and `draw_sector_walls` skips material 0 walls (the 91 invisible collision walls).

## Skies (verified)

There are 12 skies, each a pair of consecutive materials (`nfsgba_formats::skies`):
- a **240×64 skyline panorama** (kind 0, row-major, city palette, colour 0 = sky): night city skylines, an industrial skyline, a quarry, snowy mountains, rocky hills;
- a **1×128 BGR555 gradient** (kind 1, 256 bytes), top to bottom. The game writes it into palette entry 0 every 2 scanlines; the chase view uses the first 64 colours and the bumper view reaches entry 76 ([engine/sky.md](../engine/sky.md)).

Each environment (level descriptor `+0x5E`/`+0x60`) picks its sky, and each event picks its environment (`0x7F2588`, [career.md](career.md)); the skyline scrolls by yaw only, repeating 4 times per turn ([engine/sky.md](../engine/sky.md)).

## Sector (0x30 bytes)

| Offset | Type | Meaning |
|---|---|---|
| `+0x00` | u16 | first wall. Sectors tile the wall array in order |
| `+0x02` | u16 | wall count |
| `+0x04` | u16 | **ceiling** material (tunnels and overpasses), 0 = none. Drawn by `FUN_03002da0` pass 1, or filled with colour `+0x0C` by `FUN_03003180` |
| `+0x08` | u16 | **floor** material, 0 = none. Pass 0, or filled with colour `+0x0D` |
| `+0x0A` | u16 | index into a 0x14-byte table (world `+0x1C`); `0xFFFF` = none. Flag `0x40` there hides the sector |
| `+0x0C`, `+0x0D` | u8 | solid fill colour (palette index) for the **ceiling / floor** pass; 0 = textured (from the renderer code; no frame has exercised a fill yet) |
| `+0x12` | u8 | flags (bit 3: container, draw the linked list at `+0x24` instead) |
| `+0x20` | u16 | alias (renderer.md) |
| `+0x22` | u16 | start-sector alias (renderer.md) |
| `+0x24` | u16 | next sector in a list, `0xFFFF` ends it |
| rest | | unknown |

## Wall (0x44 bytes)

A wall is the edge from its point to the next wall's point in the same sector; the last wall wraps to the first.

| Offset | Type | Meaning |
|---|---|---|
| `+0x00` | i32 | x of the start point (world units) |
| `+0x04` | i32 | z of the start point |
| `+0x08` | i16, i16 | top, bottom height at the start (`-y` is up) |
| `+0x0C` | i16, i16 | top, bottom height at the end |
| `+0x10`, `+0x14` | i32 | texture v at the top, start and end, in 1/128 texel rows (16,384 = 128 rows) |
| `+0x18`, `+0x1C` | i32 | texture v **span** (top to bottom), start and end. The end values `+0x14`/`+0x1C` only count with flag `0x80` (perspective v), which no wall has |
| `+0x20`, `+0x24` | i32 | **floor** texture u, v at this corner (16,384 = one texture) |
| `+0x28` | u16 | wall texture u start, in texels (`u0 = +0x28 << 7`) |
| `+0x2A` | i16 | moving piece: index into world `+0x18` (0x20 bytes: dx, dz, ceiling dy, floor dy, top dy, bottom dy, material offset, flags), or `-1` |
| `+0x2C` | u16 | material |
| `+0x2E` | u16 | flags: 1 = open portal, not drawn; 2 = u runs the other way; `0x80` = perspective v; `0x800` = portal forces entities into pass 0; `0x1000` = blocks the camera-sector search; `0x2000` = two-sided |
| `+0x30` | i16 | **portal link**: neighbouring sector, or `-1` for a solid wall |
| `+0x32` | i16 | neighbour for the camera-sector search (usually the same as `+0x30`) |
| `+0x38`, `+0x3A` | i16 | **floor and ceiling height** at this corner; the flats use these, not `+0x08`/`+0x0A` (they differ at 384 floor corners) |
| `+0x40` | u16 | wall texture u span, in 1/256 textures (`u1 = u0 + (+0x40 << (log2w − 1))`, texel = `u >> 7`) |
| `+0x42` | i16 | v offset in texel rows (the renderer adds `+0x42 · 128`), usually 0 |

**Texture mapping (verified).**
- **Walls:** u in texels is `u >> 7`. v is in 1/128 texel rows: the top is `+0x10 + +0x42·128`, the bottom adds the span `+0x18`, and both ends of a wall share them (`docs/engine/renderer.md`). 2,119 of the 2,305 drawn walls span exactly one texture height. (Until 2026-09-29 this doc said 16,384 = one texture, which showed 64-row textures at half height.)
- **Floors:** the corner u and v divided by 16,384. Every 512×512 floor gives 8.53 units per world unit on both axes, and straight roads land their kerbs exactly on the road edges. Confirmed in the renderer (`flat_span_affine` masks). Walls are implemented in `Wall::uv`, floors in the viewer.

Screen projection (`FUN_03000978`): rotate (x, z) by the camera matrix (2.14 fixed point), give up past depth `0x5FFF`, then map both ends and both heights through the same reciprocal table the vehicles use.

## Scale (verified in the reference race)

**Cars and city share one unit:** vehicle matrices are pure rotations, and their translations are city positions. All cars measure about 48 units per metre, so the city is exaggerated (streets about 40 m wide, blocks about 50 m tall).

Check: rendering from the race's chase-camera position (world `+0x54` translation) reproduces the game's screenshot layout, with the kerb and wall on the left, the road ahead, and facades on the right. That also confirms the axis conversion (x right, y down, z forward → `(x, −y, −z)`, not mirrored).

## Open

- ~~The runtime palette transform, and which sky and palette each event uses~~ Answered: the light tint (FIDELITY R2), environments and `0x7F2588`.
- ~~Portal walls between sectors of different heights~~ Answered in [engine/renderer.md](../engine/renderer.md) (FIDELITY R8).
- **Mapping to races:** which districts belong to which races, and how the per-event descriptor fields `+0x58…+0x64` (start sector?) and the route table at `0x7F2798` (44 × 0x14 bytes, read by `FUN_08139454`) are used.
- The six portals without a matching reversed edge (T-junctions?).
