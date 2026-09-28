# Open questions

Unknowns and unverified hypotheses. Move an item to the relevant `formats/` or `engine/` doc once answered. Offsets refer to Carbon `BN7E` unless stated.

## Data

1. **What is the 5,867-entry pointer table at `0x7E86A0`?** Hypothesis: the master asset directory (it spans almost all data). This is the first target for the model viewer.
2. **What are the high-entropy, non-smooth regions `0x16C000–0x184000` and `0x224000–0x33C000`?** Custom compression, packed textures or something else? The BIOS formats are ruled out for pointer-referenced blocks.
3. **Is `0x004000–0x034000` PCM audio?** Hypothesis based on its low neighbour-difference entropy.
4. **Is `0x000000–0x0A0000` fixed-point geometry?** Hypothesis based on its dense small negative int16s.
5. **How does the text table at `0x7E559C` map to the `TEXT_*` keys and the five languages?**
6. **Is data also referenced by relative offsets?** The compression scan only followed absolute pointers.

## Code

7. **What does the 96 KiB of ARM code at `0x350000–0x368000` do, and how does it reach IWRAM (32 KiB)?** Hypothesis: renderer and mixer, copied in pieces or run from ROM.
8. **What does the 65-entry Thumb function table at `0x7F38B8` dispatch?** Hypothesis: game states or menus.

## Siblings

9. **Does Underground use GBAModPlay?** It has no credit string, but 84% of Porsche Unleashed's ARM code occurs in it. That's a hypothesis until the player code is located.
10. **What does Porsche Unleashed's `0x9CEE6` (`movs r0,#2` EU vs `#3` USA) set?** Hypothesis: a regional default. Low priority.
11. **What is the similarity baseline for unrelated GBA games** (shared compiler libraries)? No control ROM is available, and we never download one.

## Gameplay (from the brief, unverified)

12. **Does the GBA Carbon have free roam, or can its city data support it?**
