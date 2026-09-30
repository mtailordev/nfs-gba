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

## From the game's booklet (answered from the ROM 2026-09-30; test `career_facts_in_the_rom`, `tests/coverage.rs`)

- **Wingman charges and bar.** Commands per race: the `i32` table at `0x7F4284`, indexed by wingman − 1 (KITA … CLUTCH) = 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8 (`wingman_setup` loads it into `0x030061DC`; one is spent per command). The role follows the parity: odd wingmen attack, even ones draft (`wingman_attacker = (w − 1) & 1`). The 8-step bar next to the portrait is `0x030061E4` over `0x03006188` (0x78000 timer ticks): the time left of the running command (full when it starts). The portrait blinks (`0x030061D4`) when a command can be given now (charges left, no cool-down, target close). The bar's colours are art in the HUD frames (not checked).
- **Wingman as a ghost:** still open. The wingman is a non-racer car (handler 0x29, no results slot); no ghost mode was found in `wingman_throttle`, `non_racer_car_hit` or the collision code.
- **Buttons.** The ROM agrees with the booklet: the garage upgrade pages rotate the car with UP/DOWN held (`garage.rs`, angle ± 0x400), and L (`0x200`) opens the district map on the career event screen (`event.rs`). `docs/formats/ui.md` said SELECT for the map (corrected there); its garage rotate note (L/R) was not found in the file.
- **The Summary screen's four bars** (`car_stats_draw`, `FUN_08133D30`) are ACCELERATION (text key 0x8B), TOP SPEED (0x2E2), HANDLING (0x141) and VISUAL (0x39A). The first three are the car table's bytes `+0x52..+0x54` plus the installed part levels of the 10 part groups times the weights at `0x7E46EC` (`i8` × 3 per group) / 100, capped at 100 (the lighter part of the bar is the upgrade); VISUAL is the career car's style rating − 100 (`career::style_rating`).
- **Fresh-profile defaults** (`profile_reset`): chase view (`0x030053E4` = 0), HUD on, transmission variable `0x03005798` = 1 (automatic per the booklet), units = MPH for English and km/h for every other language.

## From external research (answered from the ROM 2026-09-30 where marked)

- **66 career events in five crews plus The Gauntlet: confirmed.** The zone names (text keys 963–968) are Lucky 7's, The Eastsiders, Syrens, The Corps, Krimson Crew and The Gauntlet; events 0–59 are five crews of 12, events 60–65 the Gauntlet's six (hunter, elimination, circuit, elimination, circuit, sprint, in slot order). Two bosses per crew, the events 7 and 8 of it (indices 7/8, 19/20, 31/32, 43/44, 55/56); the Gauntlet has none. Modes over all 66 events: 17 circuit, 17 elimination, 16 hunter, 16 sprint.
- **15 player cars:** the car table `0x7F0BD8` has 15 records. **4 traffic models:** not checked.
- **30 named routes: confirmed, and the other records are named.** The name table `0x7E4A70` holds 12 circuits (each forward and reversed: 24 route numbers) and 18 sprints = 42 route numbers, 1..=42; the race-slot table `0x7F2588` has 44 records: record 0 is the menu scene (environment 12) and record 43 is a spare (the same tail bytes as 0, environment 0, no name, no route). Circuits use the 12 racing environments 0..11, two route numbers each; sprints reuse six of them (1, 7, 4, 0, 8, 10). All 42 route numbers race (circuit, elimination and hunter on the circuits; sprints as sprints; easy and hard, with and without traffic) for 400 frames without a stop (`every_route_runs`), as does every car (`every_car_runs`).
- **Garage** (parts, paint, vinyls, tint, style meter, drivetrain), **link play** (`0x03005624`, the key-packet paths in `read_keys`; reachable from the menus?): still open.
- **Cops, free roam:** none seen in the code (D4).

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
