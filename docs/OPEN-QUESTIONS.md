# Open questions

Unknowns and unverified hypotheses. Move an item to the relevant `formats/` or `engine/` doc once answered. Offsets refer to Carbon `BN7E` unless stated.

## Data

1. **Where is the 3D geometry** (car meshes, city)? Not found in the static pass. Candidates:
   - the "other data" blocks: `0x000000–0x02C000`, `0x1BC000–0x1C8000`, `0x460000–0x478000`;
   - somewhere in the raw pixel region `0x404000–0x794000`.

   Ruled out: `0x3E51A0–0x3F7D9C`, right after the car atlases, is 8bpp pixels, not meshes. Plan: trace the renderer's ROM reads in mGBA, or follow the ARM code at `0x350000` in Ghidra.
2. **How does the raw 8bpp region `0x404000–0x794000` split into images** (widths, headers)? Car sprites seem to be at `0x420000` and street/building textures at `0x500000` (both seen at a guessed width).
3. **Palettes:** where they are and how images reference them. BGR555 palettes were seen at `0x33EF14` and `0x36C75C`.
4. **How does code locate LZ77 blobs?** Hypothesis: offsets from bank bases such as `0x16C244` (referenced 29 times). There is also the "size is 8 too large" quirk.
5. **What are the 24-byte width/height records near the car atlases** (e.g. `0x36D010`, `0x345114`)?
6. **Is `0x02C000–0x128000` PCM samples for the `GBAMOD30` modules?** How is a module laid out?
7. **What are the eight 41-byte LZ77 blobs at `0x23C–0x53C`?**
8. **Text table details:** the four words before `0x7E86A0` (`32, 31, 208, 113`), the trailing `4`, and the character set for bytes ≥ `0x80`.

## Code

9. **What does the 96 KiB of ARM code at `0x350000–0x368000` do, and how does it reach IWRAM (32 KiB)?** Hypothesis: renderer and mixer.
10. **What does the 65-entry Thumb function table at `0x7F38B8` dispatch?** Hypothesis: game states or menus.

## Siblings

11. **What audio engine does Underground use?** It has no Logik State credit and no `GBAMOD` tag, yet shares 41% of its ARM code with Porsche Unleashed. That shared code may be the renderer, not the mixer.
12. **What does Porsche Unleashed's `0x9CEE6` (`movs r0,#2` EU vs `#3` USA) set?** Hypothesis: a regional default. Low priority.
13. **What is the similarity baseline for unrelated GBA games** (shared compiler libraries)? No control ROM is available, and we never download one.

## Gameplay (from the brief, unverified)

14. **Does the GBA Carbon have free roam, or can its city data support it?**

## Answered

- ~~Is `0x7E86A0` the master asset directory?~~ **No:** it's the text table plus music list, see [formats/text-table.md](formats/text-table.md).
- ~~What are the high-entropy regions `0x16C000–0x33C000`?~~ **The LZ77 image bank**, see [formats/lz77-images.md](formats/lz77-images.md).
- ~~Is `0x000000–0x0A0000` fixed-point geometry?~~ **No:** it's PCM-like audio (quiet samples look like small negative int16s).
- ~~Is data also referenced by relative offsets?~~ **Yes:** the LZ77 blobs aren't pointer-referenced.
