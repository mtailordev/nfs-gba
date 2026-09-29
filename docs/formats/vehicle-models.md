# Vehicle model bank (Carbon `BN7E`)

**Status: verified.**
- **From the code:** the layout was read from the renderer's decompiled code (Ghidra, IWRAM routines at their runtime addresses).
- **From the data:** every count and range is consistent (`tools/test_tools.py::test_vehicle_models_fill_their_arrays_exactly`), and the wireframes render as recognisable cars.
- **Parser:** `tools/models.py` exports all 102 models to OBJ in `$NFSGBA_DATA/out/models/<sha1-8>/`.

## How the game finds it

- **Level descriptors:** `0x7F2B08` holds 0x68-byte records, one per event/track; 13 records plus a variant at `0x7F2FE8`.
- **Loading:** `FUN_081397d8` (race init) passes `table + index * 0x68` to `FUN_08139454`. That function copies record words `+0x34…+0x54` into the world struct at IWRAM `0x030000C0` (fields `+0x7C…+0x9C`).
- **Shared bank:** every descriptor points at the same bank.

| Record | World | ROM (`BN7E`) | Array |
|---|---|---|---|
| `+0x34` | `+0x7C` | `0x460A6C` | **models**: 102 × 40-byte records |
| `+0x38` | `+0x80` | `0x461A5C` | **vertices**: 3,933 × `int16 x, y, z` (6 B) |
| `+0x3C` | `+0x84` | `0x46768C` | **vertex indices**: `u16`, 4 slots reserved per polygon |
| `+0x40` | `+0x88` | `0x46E4DC` | **UV indices**: `u16`, parallel to the vertex indices |
| `+0x44`, `+0x48` | `+0x8C`, `+0x90` | `0x47532C` | **UVs**: 4,651 × `u16 u, u16 v`. **Hypothesis:** 8.8 fixed-point texels |
| `+0x4C`, `+0x54` | `+0x94`, `+0x9C` | `0x479BD8` | **polygon sizes**: 1 byte per polygon (3 or 4) |
| `+0x50` | `+0x98` | `0x47A9A4` | unknown, 276 bytes, possibly shading data |

## Model record (40 bytes)

| Offset | Type | Meaning |
|---|---|---|
| `+0x00` | u32 | first vertex (index into the vertex array) |
| `+0x04` | u32 | first vertex-index slot |
| `+0x08` | u32 | first UV |
| `+0x0C` | u32 | first polygon (repeated at `+0x14` and `+0x1C`) |
| `+0x10` | u32 | first vertex (repeat of `+0x00`) |
| `+0x18` | u32 | first UV-index slot (equals `+0x04`) |
| `+0x1C` | u32 | first polygon-size byte |
| `+0x20` | u16 | flags. Bit 0 = textured (UVs are used). Seen: 3, 6, 7 |
| `+0x22` | u16 | polygon count |
| `+0x24` | u16 | vertex count |
| `+0x26` | u16 | UV count |

## Drawing

- **Transform** (`FUN_03004018`): each vertex is rotated with a 3×3 matrix in 2.14 fixed point, then translated. Depth indexes a reciprocal table (`DAT_03004180`), and the result is projected to screen `short x, y`. It gives up if a vertex's depth exceeds `0x6000`.
- **Polygon loop** (`FUN_03004190`): for each polygon it reads its size byte n, then n vertex indices and n UV indices. It culls back faces with a 2D cross product on the first three projected vertices, then rasterises with `FUN_03003808` (texture = the per-object atlas). Indices advance by n, so triangles leave their 4th reserved slot unused, and those slots hold junk at the end of a model's range.
- **Up direction:** `-y` is up (roofs sit at about `y = -23`).

## Contents

Checked by rendering all 102 models as wireframes:
- **0–52:** player/opponent cars, mostly in LOD groups of three (e.g. 0/1/2 = 89/63/36 vertices), interleaved with their spoilers.
- **53–82:** about 30 aftermarket spoiler variants.
- **83–88:** traffic vehicles (a box truck/bus, a van, sedans).
- **89–101:** small pieces: markers, arrows, flat quads (shadows or effects, unverified).

## Open

- Which texture atlas goes with which car (the 45 × 256×200 LZ77 atlases), and the palettes.
- The exact UV scale and what model `+0x98` does.
- The flag bits other than bit 0.
- **The city geometry is not in this bank.** It is drawn by other IWRAM routines (`FUN_030013ac` and `FUN_03000b44` are the candidates, both called from the scene routine `FUN_0300224c`).
