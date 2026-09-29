# Open questions

Unknowns and unverified hypotheses. Move an item to the relevant `formats/` or `engine/` doc once answered. Offsets refer to Carbon `BN7E` unless stated.

## Data

5. **What are the small odd-sized vehicle materials** (46–146)? The 36 128×100 ones are answered: raw 8bpp opponent atlases already in final palette slots ([formats/car-paint.md](formats/car-paint.md)).
6. **How does code locate the LZ77 image blobs** (bank bases such as `0x16C244`), and why do the size fields claim 8 bytes too many?
8. **What are the 24-byte width/height records near the car atlases** (`0x36D010`, `0x345114`)? (The eight "41-byte LZ77 blobs" at `0x23C–0x53C` were false hits inside the sound-effect table at `0x210`.)
9. **Text table details:** the four words before `0x7E86A0` (`32, 31, 208, 113`), the trailing `4`, and the character set for bytes ≥ `0x80`.

## Code

10. **What is `0x78E714–0x799B88`** (46 KiB right after the last racing line; 49 code references into its `0x794000` part), **the 48-byte byte map at `0x7F5CC8`, and the 4 KiB table at `0x722DD4`** (level descriptor `+0x28`)? ROM attribution: 98.907% of the ROM is claimed ([engine/harness.md](engine/harness.md)).

18. **Menu materials 6, 16–152 and most overlays:** what are they, and which palette does each screen use (FIDELITY U4)?
19. **HUD details:** material kind bit 4 (materials 5–33), element bytes `+0x0F`/`+0x10`, and which race mode HUD screen 3 serves.
20. **Menu sprite screens (`0x347778`):** all elements use material 0. Hit boxes or cursor anchors?
21. **The 128 bytes at `0x71F168`**, zero apart from one word, just before the city palettes.
22. **Who calls `FUN_0814279c` (countdown timer), `FUN_08142aac` (best lap) and `FUN_08143094` (units panel)?** The HUD modes don't.
23. **Keys and screens:** what `FUN_0812B084` does with the keys (hardware read, repeat?) and how `0x030064C0` is filled; which screen id is which menu for the seven non-intro kinds; where screen 0x25 (an intro-kind page with heading 960 and prompts 498/146 but no content) is reached.
24. **What reads `0x03005724` and `0x03005728`** (counted and flagged by every VBlank IRQ)? **What are the handler-0x34 entities** spawned by the player's contacts (sparks?), and what does their handler do (D18)?
25. **Menus:** can the profile's cash exceed 999,999 (the thousands separator's stale-register path, U9)? What does the per-frame `rand_table` draw of four menu kinds decorate? Where is screen 0x25 reached? What are the Kind18 screens 0x12–0x14 exactly (garage car pages)?
16. **Audio module header bytes `+0x038` and `+0x138`** are never read by the player. What did they hold for the converter?
17. **What calls the sound re-init/shutdown pair at `0x08149dbe`–`0x0814a018`?** Hypothesis: link play.

## Siblings

12. **What audio engine does Underground use?** It has no Logik State credit and no `GBAMOD` tag, yet shares 41% of its ARM code with Porsche Unleashed.
13. **What does Porsche Unleashed's `0x9CEE6` (`movs r0,#2` EU vs `#3` USA) set?** Low priority.
14. **What is the similarity baseline for unrelated GBA games?** No control ROM is available, and we never download one.

## Gameplay (from the brief, unverified)

15. **Does the GBA Carbon have free roam, or can its city data support it?** The city is one connected sector world shared by all 12 environments, which is promising.

## From the game's booklet (2026-09-30; notes in `data/reference/guidebook.md`, not in git)

Check each against the ROM during coverage:
- **Wingman charges:** the booklet says the first wingmen have 3 uses per race, later ones more, with a green/orange/red availability bar and a flashing "use now" icon. Where are the charge count and the bar in the code?
- **Wingman as a ghost:** the booklet says wingmen can act as ghosts. What does that mean in the code?
- **Buttons the booklet and `docs/formats/ui.md` disagree on:** garage rotate (booklet: D-pad up/down; ui.md: L/R) and the zone map (booklet: L; ui.md: SELECT). The race bindings match the ROM table (nitro A+L automatic, A+UP manual).
- **The Summary screen's four car bars** (acceleration, top speed, handling, visual): which values feed them?
- **Fresh-profile defaults** (chase view, MPH, HUD on, automatic): confirm in `profile_reset`.

## From external research (2026-09-30; notes in `data/reference/research.md`, not in git)

Guidance only (blog and review sources, not confirmed): the ROM is the source of truth. Things to look for in the ROM (text table, career tables, route and car tables):
- **66 career events** in five crew sections (Lucky 7's, Eastsiders, Syrens, Corps, Krimson Crew) plus The Gauntlet (races in sequence); two bosses per section.
- **15 player cars** and **4 traffic models**; car prices in points.
- **30 named routes:** 12 circuits and 18 sprints (the ROM has more route records: which are the others?).
- **Garage:** performance and body parts, paint, vinyls, window tint, a Style meter, a Drivetrain screen (clutch, gearing, traction control), a tuning-rate percentage.
- **Link play:** reviews say there is no multiplayer, but the code has link-play paths (`0x03005624`, the key-packet functions in `read_keys`). Is it reachable from the menus?
- **Cops, free roam:** reviews and the booklet say neither exists; none was seen in the code (D4).

## Answered

- ~~The 65-entry function table at `0x7F38B8`~~ **Answered:** the entity handler table (`update_entities`; world `+0x78` by entity `+0x4E`) ([engine/physics.md](engine/physics.md)).
- ~~96 KiB of ARM code at `0x350000–0x368000`~~ **Not code:** 4bpp HUD sprite texels inside `0x347B74–0x36C55C` (runs of `0xEEEEEEEE`).
- ~~Runtime palette~~ **Answered:** the city palette is picked by the environment (`race_load_palettes`) and tinted every frame by the wall light at the player (`apply_sector_light_to_palette`); car colours are ramps from `0x36C95C` (`load_car_palettes`, `shade_car_paint`). See [FIDELITY.md](FIDELITY.md), [formats/car-paint.md](formats/car-paint.md). The event's environment comes from `0x7F2588` ([formats/career.md](formats/career.md)).
- ~~Route table fields~~ **Answered:** `+0x04` is the racing-line section table (lap plus branches). See [formats/race-routes.md](formats/race-routes.md), [formats/career.md](formats/career.md). Descriptor `+0x62`/`+0x64` are sky offsets ([engine/sky.md](engine/sky.md)).
- ~~Portal walls between sectors of different heights~~ **Answered:** portal walls with flag bit 0 clear and a non-zero material are drawn like solid walls over their own top and bottom; there are no separate upper and lower parts ([engine/renderer.md](engine/renderer.md)).
- ~~Raw 8bpp region `0x404000–0x794000`~~ **Answered:** every byte of `0x4018C0–0x71F1E8` belongs to a known material table except 128 bytes at `0x71F168` (question 21) ([formats/ui.md](formats/ui.md)).
- ~~How the code finds LZ77 blobs, and the "size + 8"~~ **Answered:** through material tables; the game's ring decoder writes the header size ([formats/ui.md](formats/ui.md)).
- ~~The 24-byte records near the car atlases~~ **Answered:** 0x24-byte HUD material records (`0x36CF5C`).
- ~~Text character set for bytes ≥ `0x80`~~ **Answered:** Windows-1252, `{`/`|` = A/B buttons.
- ~~Is `0x02C000–0x128000` PCM?~~ **Answered:** sound-effect data `0x0005D4–0x04EA11` and music bank data `0x051FD4–0x12A84B`, both signed 8-bit PCM; the `GBAMOD30` layout is decoded ([formats/audio.md](formats/audio.md)).

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
