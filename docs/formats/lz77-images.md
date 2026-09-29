# LZ77 image bank (Carbon `BN7E`)

**Status: the container is verified; the image headers and palettes are not.** Checked by decoding and rendering in a scratch session. The blob scan lives in `tools/first_look.py` (`lz77_blobs()`); there is no image extractor yet.

## Container

- **Format:** standard GBA BIOS LZ77, type `0x10` (GBATEK "LZ77UnComp"): a 4-byte header `10 ss ss ss` (24-bit unpacked size), then flag bytes (MSB first), where 1 means a 2-byte back-reference (length 3–18, distance 1–4096) and 0 means a literal byte.
- **Placement:** blobs are 4-byte aligned and packed back to back in banks. They are **not** referenced by absolute pointers. **Hypothesis:** they're addressed by offset from a bank base, since code references the bank start `0x16C244` 29 times.
- **Size quirk:** each header's size is **8 bytes larger** than the data the stream really encodes. Decoding the full size reads up to 8 bytes into the next blob, so the last 8 output bytes are junk (they often contain the next blob's header, e.g. `10 08 96 00`). The real payload is `size - 8`. **Hypothesis:** the packer counted an 8-byte header it never emitted.
- **Totals:** 294 blobs, 4.6 MiB unpacked from 1.9 MiB. Almost all sit in `0x15CF2C–0x4018C2`; the other eight are 41-byte blobs clustered at `0x23C–0x53C`. Their identical size suggests they're real, but they haven't been checked. See `$NFSGBA_DATA/out/first-look/e5298b24/lz77.txt`.

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

- **Palettes:** where they live and how an image finds its palette. BGR555 palettes were seen at `0x33EF14` and `0x36C75C`, with `0x7C1F` magenta as the transparent key.
- **How the code finds a blob:** an offset table, or a walk over the bank?
- **The 24-byte records with width/height** (e.g. `0x36D010`: `…e8 00 00 00 10 00 10 00…`) near the atlases. They could be image or sprite descriptors with relative offsets.
- **How the 3D car meshes map UVs onto the 256×200 atlases** (the meshes aren't located yet).
