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

## Materials and textures (verified by rendering)

- **Texels:** 8bpp, row-major, at `texel base + material +0x08`.
- **Size:** material `+0x0C`/`+0x0E` hold width and height (8 … 512), and `+0x1E`/`+0x1F` their log2.
- **Palette:** textures render correctly with the palette at record `+0x00`, and index 0 is transparent (for example above facade cut-outs).
- **Own palettes:** materials whose `+0x02` field is 2 point at their own 512-byte palette through `+0x04`, stored just before their texels (not yet verified).
- **Runtime palette:** during a race the BG palette in RAM is not a raw copy. It's roughly `0.6 × ROM colour + haze` (a fade or fog effect, unverified).
- **Contents:** large road-surface textures (a 512×512 roundabout quarter, straights with lane markings), 512×64 building-facade strips, and 64×64 paving and tiles.

## Sector (0x30 bytes)

| Offset | Type | Meaning |
|---|---|---|
| `+0x00` | u16 | first wall. Sectors tile the wall array in order |
| `+0x02` | u16 | wall count |
| `+0x04`, `+0x08` | u16 | when non-zero, extra draw passes (`FUN_03002da0`/`FUN_03003180`); probably floor/ceiling or road-surface materials |
| `+0x0A` | u16 | index into a 0x14-byte table (world `+0x1C`); `0xFFFF` = none. Flag `0x40` there hides the sector |
| `+0x0C`, `+0x0D` | u8 | per-pass options |
| `+0x12` | u8 | flags (bit 3: container, draw the linked list at `+0x24` instead) |
| `+0x24` | u16 | next sector in a list, `0xFFFF` ends it |
| rest | | unknown |

## Wall (0x44 bytes)

A wall is the edge from its point to the next wall's point in the same sector; the last wall wraps to the first.

| Offset | Type | Meaning |
|---|---|---|
| `+0x00` | i32 | x of the start point (world units) |
| `+0x04` | i32 | z of the start point |
| `+0x08` | i16, i16 | top, bottom height at the start (`-y` is up; roads sit around 0 to -320, buildings reach about -2,560 to -3,136) |
| `+0x0C` | i16, i16 | top, bottom height at the end |
| `+0x10…+0x1C` | 4 × i32 | copied into the draw entry. **Hypothesis:** texture u/v setup |
| `+0x20`, `+0x24` | i32 | texture coordinates at this end, interpolated when the wall is clipped at the near plane |
| `+0x28` | u16 | texture offset (× 0x80) |
| `+0x2A` | i16 | index into a 0x20-byte table (world `+0x18`) of position and height offsets, or `-1`. **Hypothesis:** moving or animated pieces |
| `+0x2C` | u16 | material (0x24-byte record: `+0x08` texel offset, `+0x1E`/`+0x1F` used as shifts, probably log2 texture width and height (hypothesis)) |
| `+0x2E` | u16 | flags. Bit 1 swaps the texture direction; bit `0x2000` is also used |
| `+0x30` | i16 | **portal link**: neighbouring sector, or `-1` for a solid wall |
| `+0x32` | i16 | usually the same as `+0x30` |
| `+0x38`, `+0x3A` | i16 | offsets used when drawing the road surface |
| `+0x40` | u16 | texture repeat length |

Screen projection (`FUN_03000978`): rotate (x, z) by the camera matrix (2.14 fixed point), give up past depth `0x5FFF`, then map both ends and both heights through the same reciprocal table the vehicles use.

## Open

- **Wall texture mapping.** From `FUN_030013ac`: u starts at `+0x28 × 0x80` and spans `+0x40 << (log2w − 1)`, and v uses `+0x42`. The viewer still uses a square-texel guess.
- **Floor texture mapping is known in practice.** The per-corner `+0x20`/`+0x24` pairs are floor UVs, and **16,384 = one texture**. Every 512×512 floor gives 8.53 UV units per world unit on both axes (= 16,384 / 1,920, one road width). Straight roads then render with their kerbs exactly at the road edges. Road textures (512×128) are stretched along the road (texel aspect about 1.8); that may be deliberate. Why the renderer's shifts produce this scale is not yet derived.
- **The road surface:** how floors are drawn (`FUN_03000b44`, `FUN_03002da0`) and what the sector `+0x04`/`+0x08` passes mean.
- **Mapping to races:** which districts belong to which races, and how the per-event descriptor fields `+0x58…+0x64` (start sector?) are used.
- **The six portals without a matching reversed edge** (T-junctions?).
