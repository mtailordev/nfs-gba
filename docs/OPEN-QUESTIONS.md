# Open questions

Unknowns and unverified hypotheses. Move an item to the relevant `formats/` or `engine/` doc once answered. Offsets refer to Carbon `BN7E` unless stated.

## Data

1. **Runtime palette:** during a race the city palette is computed (a night or fog tint of the ROM palette at level record `+0x00`), and the car paint ramp is generated from the chosen colour. What are the transform and the generator, and which of the 12 skies does each event use?
2. **Race data:** the route table at `0x7F2798` (44 × 0x14 bytes, read by `FUN_08139454`: `+0x00` route data → world `+0x38`, `+0x04` 0x50 bytes, `+0x08` 0x1800 bytes, `+0x0C` counts), and the level-descriptor fields `+0x00…+0x30` and `+0x58…+0x64` (start sector?).
3. **Portal walls between sectors of different heights:** are upper and lower wall parts drawn, and from which fields?
4. **How does the raw 8bpp region `0x404000–0x794000` split into images?** Most city textures sit at `0x47BC6C` + material offsets; is the rest the same, or HUD, menus and sprites?
5. **What are the 36 non-LZ77 128×100 vehicle materials and the small odd-sized ones** (46–146)?
6. **How does code locate the LZ77 image blobs** (bank bases such as `0x16C244`), and why do the size fields claim 8 bytes too many?
7. **Is `0x02C000–0x128000` PCM samples for the `GBAMOD30` modules?** How is a module laid out?
8. **What are the eight 41-byte LZ77 blobs at `0x23C–0x53C`, and the 24-byte width/height records near the car atlases** (`0x36D010`, `0x345114`)?
9. **Text table details:** the four words before `0x7E86A0` (`32, 31, 208, 113`), the trailing `4`, and the character set for bytes ≥ `0x80`.

## Code

10. **What does the 96 KiB of ARM code at `0x350000–0x368000` do?** It is *not* what runs in IWRAM during a race: that code comes from `0x14FC38`, `0x165154` and `0x168264`.
11. **What does the 65-entry Thumb function table at `0x7F38B8` dispatch?** Hypothesis: game states or menus.

## Siblings

12. **What audio engine does Underground use?** It has no Logik State credit and no `GBAMOD` tag, yet shares 41% of its ARM code with Porsche Unleashed.
13. **What does Porsche Unleashed's `0x9CEE6` (`movs r0,#2` EU vs `#3` USA) set?** Low priority.
14. **What is the similarity baseline for unrelated GBA games?** No control ROM is available, and we never download one.

## Gameplay (from the brief, unverified)

15. **Does the GBA Carbon have free roam, or can its city data support it?** The city is one connected sector world shared by all 13 events, which is promising.

## Answered

- ~~Is `0x7E86A0` the master asset directory?~~ **No:** it's the text table plus music list, see [formats/text-table.md](formats/text-table.md).
- ~~What are the high-entropy regions `0x16C000–0x33C000`?~~ **The LZ77 image bank**, see [formats/lz77-images.md](formats/lz77-images.md).
- ~~Is `0x000000–0x0A0000` fixed-point geometry?~~ **No:** it's PCM-like audio.
- ~~Where is the city geometry?~~ **A 2.5D portal/sector world** (1,113 sectors, 4,423 walls), see [formats/city-sectors.md](formats/city-sectors.md).
- ~~City textures, palette and road surface?~~ **Answered:**
  - materials: plain textures, column-mapped facades, and 12 skies;
  - palette: level record `+0x00`;
  - floor and ceiling passes, where material 0 means "not drawn";
  - exact wall and floor UVs.

  See [formats/city-sectors.md](formats/city-sectors.md).
- ~~Where are the car meshes, and which car is which?~~ The **vehicle model bank at `0x460A6C`**, plus the **car table at `0x7F0BD8`** (15 named cars, models and atlases), see [formats/vehicle-models.md](formats/vehicle-models.md).
- ~~How big are things?~~ **One unit for cars and city**, about 48 per metre. Checked with the race's vehicle matrices and chase camera.
- ~~How is the 3D scene drawn?~~ **Video mode 4** (8bpp framebuffer) with an ARM software renderer copied into IWRAM:
  - `FUN_0300224c`: scene;
  - `FUN_03000978` / `FUN_030013ac` / `FUN_03000304`: walls;
  - `FUN_03001cf0` / `FUN_03004018` / `FUN_03004190` / `FUN_03003808`: vehicles.
