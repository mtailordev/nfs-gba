# LZ77 image bank (Carbon `BN7E`)

**Status: superseded by [ui.md](ui.md) for how the game finds and decodes these blobs; kept for the bank survey.** Checked by decoding and rendering in a scratch session. The blob scan lives in `tools/first_look.py` (`lz77_blobs()`); there is no image extractor yet.

## Container

- **Format:** standard GBA BIOS LZ77, type `0x10` (GBATEK "LZ77UnComp"): a 4-byte header `10 ss ss ss` (24-bit unpacked size), then flag bytes (MSB first), where 1 means a 2-byte back-reference (length 3–18, distance 1–4096) and 0 means a literal byte.
- **Placement:** blobs are 4-byte aligned and packed back to back in banks. They are **not** referenced by absolute pointers. **Hypothesis:** they're addressed by offset from a bank base, since code references the bank start `0x16C244` 29 times.
- **Size quirk:** each header's size is **8 bytes larger** than the data the stream really encodes. The game's own decoder (`lz77_ring_decode`, ROM `0x169208`) writes the full header size, decoding the tail from the bytes that follow, so the last 8 output bytes are whatever follows the stream (often the next blob's header, e.g. `10 08 96 00`). Images use only the first `size - 8`. The game decodes through a 4 KiB ring pre-filled with 0xFF, and 15 menu streams reference before their start and rely on it (`ui::unpack`; the BIOS-semantics `lz77()` cannot decode those).
- **Totals:** 294 scan hits, 4.6 MiB unpacked from 1.9 MiB. Almost all sit in `0x15CF2C–0x4018C2`. The eight 41-byte hits at `0x23C–0x53C` are false: they are inside the sound-effect table at `0x210` ([audio.md](audio.md)). The material tables address 299 packed streams in all ([ui.md](ui.md)). See `$NFSGBA_DATA/out/first-look/e5298b24/lz77.txt`.

## Contents

The payloads are 8bpp indexed pixels, and `size - 8` factors into plausible image sizes:

| Unpacked size | Count | Payload | What (rendered, grayscale) | Bank |
|---|---|---|---|---|
| 38,408 | 50 | 240×160 | **full-screen bitmaps**. `0x179114` is the language-select screen (UK/IT/FR/DE/ES flags) | `0x16C284`, `0x1733FC–0x18CBB6`, `0x2246DC–0x33E26D` |
| 51,208 | 45 | 256×200 | **car texture atlases**: top, front, rear and side views of one car. `0x375C10` looks like a Mazda RX-7 | `0x370550–0x3E519E` |
| 4,104 | 53 | 64×64 | textures/icons. `0x1A9478` is an "X" button icon | `0x1A9478–0x1BD27F` |
| 2,312 | 60 | 48×48 | small images (not viewed yet) | `0x1C9610–0x1D6E6E` |
| 1,608 | 16 | 40×40 (hypothesis) | alongside the car atlases, maybe car icons | `0x370550–0x3E519E` |
| 648 | 10 | 640 bytes (unknown shape) | | `0x1C9610–0x1D6E6E` |

Several smaller banks (`0x3F7D9C–0x4018C2`) hold assorted sizes (442 B to 7 KiB), not yet examined.

## Open

- ~~Palettes~~ Answered: menu palettes at `0x33EF14`, OBJ palettes at `0x36C75C`, per screen or material ([ui.md](ui.md)).
- ~~How the code finds a blob~~ Answered: through 0x24-byte material tables (menus `0x345114` from texel base `0x16C244`, vehicles `0x45F5C0` from `0x370550`, HUD `0x36CF5C`).
- ~~The 24-byte records at `0x36D010`~~ Answered: 0x24-byte HUD material records (table `0x36CF5C`).
- ~~Car UVs onto the atlases~~ Answered in [vehicle-models.md](vehicle-models.md).
