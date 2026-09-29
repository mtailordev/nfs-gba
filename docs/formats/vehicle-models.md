# Vehicle model bank (Carbon `BN7E`)

**Status: verified.**
- **From the code:** the layout was read from the renderer's decompiled code (Ghidra, IWRAM routines at their runtime addresses).
- **From the data:** every count and range is consistent (`tools/test_tools.py::test_vehicle_models_fill_their_arrays_exactly`), and the wireframes render as recognisable cars.
- **Parser:** `nfsgba_formats::models()` (tested in the crate); the viewer shows them. (An early Python OBJ exporter, `tools/models.py`, was removed in the consolidation.)

## How the game finds it

- **Level descriptors:** `0x7F2B08` holds 0x68-byte records. There are 12 environments (palette and sky, see city-sectors.md) plus a variant with its own palette block at `0x7F2FE8`.
- **Loading:** `FUN_081397d8` (race init) passes `table + index * 0x68` to `FUN_08139454`. That function copies record words `+0x34…+0x54` into the world struct at IWRAM `0x030000C0` (fields `+0x7C…+0x9C`).
- **Shared bank:** every descriptor points at the same bank.

| Record | World | ROM (`BN7E`) | Array |
|---|---|---|---|
| `+0x34` | `+0x7C` | `0x460A6C` | **models**: 102 × 40-byte records |
| `+0x38` | `+0x80` | `0x461A5C` | **vertices**: 3,933 × `int16 x, y, z` (6 B) |
| `+0x3C` | `+0x84` | `0x46768C` | **vertex indices**: `u16`, 4 slots reserved per polygon |
| `+0x40` | `+0x88` | `0x46E4DC` | **UV indices**: `u16`, parallel to the vertex indices |
| `+0x44`, `+0x48` | `+0x8C`, `+0x90` | `0x47532C` | **UVs**: 4,651 × `u16 u, u16 v`, 1.15 fixed point (32,768 = whole texture) |
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

## Textures

- **Vehicle materials:** level record `+0x20` → world `+0x24` → `0x45F5C0`. There are 146 self-indexed records of 0x24 bytes, in the city material layout.
- **Texel pointer:** world `+0x04` (record `+0x0C` = `0x370550`) + material `+0x08`. Most materials are **BIOS-LZ77 blobs** (size = w×h + 8), which the game unpacks to RAM (entity `+0x84`). The 36 materials of 128×100 are not LZ77 (format unknown).
- **Material sizes:** 0 is 16×16; **1–45 are the car atlases (256×200)**; 46–60 are 40×40; the rest are odd sizes (decals? UI?).
- **Atlas colours:** atlases use indices 0–31. The game loads pixel `i` into palette slot `192 + (i ^ 16)` (verified against the race RAM). So atlas 0–15 is the **body**: a paint ramp that the game **generates at runtime** from the chosen colour (the reference race's red ramp is in no ROM table). Atlas 16–31 holds glass, lights and trim.
- ~~Paint presets at `0x7E6EEC`~~ **Corrected:** those are 19 portrait palettes (64 colours each) for menu materials 0x40–0x52 ([ui.md](ui.md)). Car colours come from the paint ramps at `0x36C95C` ([car-paint.md](car-paint.md)); the old `paint_palettes`/`car_palette` stand-ins are removed.
- **UVs:** `u16 u, u16 v` in **1.15 fixed point, 32,768 = the whole texture**. **Verified:** the Cobalt's polygon UVs, drawn over its atlas, land on the top, front, rear and side views.
- **Choosing a material:** `FUN_03001cf0` computes it per entity: `world[9] + (entity[+0x48] + optional LOD step + entity[+0x46] + entity[+0x44] >> 8) × 0x24`.

## Car table (verified)

`0x7F0BD8` holds 15 × 0x58-byte records (read by the race init `FUN_081397d8` via `DAT_08139b64`). In the reference race, the player's Chevy Cobalt SS had entity `+0x48` = 8, its first material.

| Offset | Meaning |
|---|---|
| `+0x00` | text key of the car name (`TEXT_CARNAME1…15`, keys 183–197) |
| `+0x04` | car index |
| `+0x0C` | u16 first atlas material. The paint variants follow; a car's run ends at the next car's first material |
| `+0x10` | u16 low-detail model |
| `+0x14` | u16 medium-detail model. The high-detail model is `+0x14 − 1` |
| `+0x48…+0x57` | per-car values (performance tier? `+0x48` = 12–16), unverified |

| # | Car | Models | Atlases |
|---|---|---|---|
| 0 | Mazda RX-7 | 0–2 | 1–3 |
| 1 | VW Golf GTI | 5–7 | 4–7 |
| 2 | Chevy Cobalt SS | 8–10 | 8–11 |
| 3 | Mitsubishi Eclipse GT | 13–15 | 12–14 |
| 4 | Audi TT 3.2 quattro | 16–18 | 15–17 |
| 5 | Ford Mustang GT | 21–23 | 18–20 |
| 6 | Mazda RX-8 | 26–28 | 21–23 |
| 7 | Subaru Impreza WRX STi | 29–31 | 24–27 |
| 8 | Mitsubishi Lancer EVOLUTION IX | 34–36 | 28–31 |
| 9 | Toyota MR2 | 39–41 | 32–34 |
| 10 | Porsche Carrera GT | 44–46 | 35–36 |
| 11 | Toyota Supra | 47–49 | 37–39 |
| 12 | Ford GT | 52–54 | 40–41 |
| 13 | Mercedes-Benz SL65 AMG | 55–57 | 42–43 |
| 14 | Aston Martin DB9 | 58–60 | 44–45 |

Models between and after the car triplets (3–4, 11–12, 19–20, …, 61–82) are spoilers. 83–88 are traffic vehicles, and 89–101 are small markers and effects.

## Scale (verified)

- **Units:** model vertices are in **the same units as the city**. In the reference race, the vehicle matrices in the world `+0xFC` buffer are pure rotations (row lengths 16,391, i.e. 1.0 in 2.14). The player's translation `(0, 134, 343)` equals the camera-to-player offset in city units, and the projection code is the same as for walls.
- **Metres:** all 15 high-detail models measure about 48 units per real-world metre on every axis (checked against manufacturer dimensions), so the world is about 48 units per metre. The chase camera then sits 7.1 m behind and 2.8 m above the car.
- **Consequence:** the city itself is exaggerated compared with the cars.

## Open

- The 36 non-LZ77 128×100 materials, and what the small odd-sized materials are for.
- The runtime paint-ramp generator, and what model `+0x98` and the flag bits other than bit 0 do.
