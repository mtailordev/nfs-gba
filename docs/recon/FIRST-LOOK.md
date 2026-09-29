# First look

Shallow recon, no disassembly yet. All numbers come from `tools/first_look.py` and are listed in full in [FIRST-LOOK-DATA.md](FIRST-LOOK-DATA.md) (block maps, entropy map, tables). ROM identities are in [ROM-INVENTORY.md](ROM-INVENTORY.md). Offsets are ROM offsets (address minus `0x08000000`). **Hypothesis** marks anything not yet verified. Anything marked **verified** was rendered or decoded in a scratch session and checked by eye; there are no parsers for it in `tools/` yet.

## Canonical ROM: Carbon `BN7E` `e5298b24` (8 MiB)

### Layout

Rough split: about 5–6% code, 23% LZ77-packed images, 12% PCM-like audio, 47% raw pixel-like data, 11% other.

| Range | What | Evidence |
|---|---|---|
| `0x000000` | ARM entry: `b 0x080000C0` | header entry word `ea00002e` |
| `0x02C000–0x128000` | **audio bank** (about 1 MiB) | Signed bytes centred on 0, and neighbour differences more predictable than the bytes (PCM-like). It contains the **`GBAMOD30` music modules** at `0x04EA14–0x0507FC` (verified: pointed to by the text table, see below). The PCM reading is a hypothesis |
| `0x128000–0x16C244` | **Thumb code** (about 250 KiB, including 32 KiB of ARM at `0x164000`) | Return and `push {…, lr}` density. The code ends in a `memset` just before `0x16C244` |
| `0x16C244–0x402000` | **LZ77 image bank** (with raw pixel blocks between the banks) | 294 BIOS-LZ77 blobs, 4.6 MiB unpacked from 1.9 MiB. **Verified** by rendering, see [formats/lz77-images.md](../formats/lz77-images.md) |
| `0x350000–0x368000` | ~~ARM code~~ **4bpp HUD sprite texels** (corrected 2026-09-29) | The "AL condition" heuristic misfired: blank 4bpp pixels `0xEEEEEEEE` have condition nibble `E` (always). See [formats/ui.md](../formats/ui.md) |
| `0x404000–0x794000` | **raw 8bpp pixel data** (about 3.5 MiB) | Many equal neighbouring bytes. Rendered at a guessed 256-pixel width: top-down car sprites at `0x420000` and street/building textures at `0x500000`. Exact layout unknown |
| `0x797D10–0x7E53EC` | localized text | 977 keys × 5 languages, **verified**, see [formats/text-table.md](../formats/text-table.md) |
| `0x7E4000–0x7F4000` | tables | see below |
| `0x7F5CF8–0x7FFFFF` | zero padding (41,736 B) | |

**Not found yet: the 3D geometry** (car meshes, city). It's probably small and low-poly, so it could sit in the "other data" blocks (e.g. `0x000000–0x02C000`, `0x1BC000–0x1C8000`, `0x460000–0x478000`) or inside the raw pixel region. The first-look guess that `0x000000–0x0A0000` held geometry was **wrong**: the dense small negative int16s there are quiet PCM samples. Finding the geometry by tracing the renderer is the next step.

### Strings

- 88,282 printable ASCII runs of 6+ bytes. The full list is in `$NFSGBA_DATA/out/first-look/e5298b24/strings.txt` (not in git).
- **Credits:**
  - `Pocketeers` at `0x7988B9`;
  - `Developed at EA CANADA` in the credits text (EA-side credit; Pocketeers is credited separately);
  - licence lines naming Electronic Arts, plus a trademark notice for each car maker.
- **Audio credit:** `LS_Play (C) Logik State 2003 www.LogikState.com` at `0x7C0360`.
- **Text keys survived in the ROM:** 977 `UPPER_SNAKE` IDs such as `TEXT_SELECT_TRACK` and `TEXT_ZONE_1_NAME`, stored alongside the text.

### Tables

- **`0x7E86A0`: text table plus music list** (**verified**). It holds 977 key pointers, then 5 × 977 string pointers (En, Fr, De, It, Es), then 5 pointers to `GBAMOD30` music modules. It is *not* an asset directory; the first-look hypothesis was **wrong**.
- **`0x7F38B8`: 65 odd addresses** into the Thumb code, so a table of Thumb function pointers (state or menu handlers, hypothesis).
- There are several tables of about 50 entries inside the code segment (`0x12B000–0x131000`) whose targets sit right after them. They are probably switch tables or string lists.
- **Pointer scanning over data is noisy.** Audio and pixel bytes often look like `0x08xxxxxx`, so the "12,117 referenced addresses" count is inflated. Code literal pools are reliable: 488 unique targets, 197 of them in data.
- **False positive:** the "table" at `0x7E466C` is 32 copies of `0x08080808`, i.e. a byte fill.

### Compression

**BIOS LZ77 is used heavily. Huffman and RLE are not detectably used.**
- **Blobs:** 294 LZ77 blobs, found by brute-force decoding every 4-aligned `0x10` header.
- **Addressing:** they are addressed by offset from a bank base, not by absolute pointers. That's why the first, pointer-based scan found only 3 of them.
- **Size quirk:** each blob's size field is 8 bytes larger than what it encodes, so a decoder reads a few bytes into the next blob.
- **Other formats:** a brute-force RLE/Huffman scan is meaningless (almost any bytes decode), and no pointer-referenced RLE/Huffman header was found.

Details: [formats/lz77-images.md](../formats/lz77-images.md).

### Audio

- **Engine:** Logik State's **`LS_Play`**, a MOD-style player (the name comes from the credit string).
- **Modules:** `GBAMOD30` music modules; 6 `GBAMOD` tags, 5 of them in the text/music table.
- **Samples:** in the PCM-like bank, probably.
- **No MP2K/M4A:** the SelectSong signature is absent, and nothing references the BIOS sound area `0x03007FF0`.

## Engine sharing across the five games

| Game | Code | Thumb / ARM KiB | Thumb shared with Carbon (→ / ←) | ARM shared with Carbon (→ / ←) | Audio | LZ77 images | Code layout |
|---|---|---|---|---|---|---|---|
| Carbon (canonical) | `BN7E` | 336 / 128 | — | — | LS_Play, 6 GBAMOD | 294 blobs, 4.6 MiB | yes |
| Most Wanted | `BNWE` | 256 / 16 | 32% / 40% | 37% / **79%** | LS_Play, 6 GBAMOD | 99 blobs, 3.8 MiB | yes |
| Underground 2 | `BNFE` | 288 / 80 | 21% / 21% | **70%** / 48% | GBAModPlay, 10 GBAMOD | 663 blobs, 2.7 MiB | yes |
| Underground | `BNSE` | 384 / 32 | 13% / 14% | 40% / 42% | unknown (no credit, no GBAMOD) | none | yes |
| Porsche Unleashed | `AZFP` | 176 / 16 | 10% / 21% | 11% / 24% | GBAModPlay, 8 GBAMOD | none | yes |

How to read it:
- "→" is the share of Carbon's sampled code windows found in the sibling; "←" is the reverse.
- Code is compared after masking addresses and branch offsets, so relinked copies of the same function still match.
- The windows are 32 bytes, so these numbers are a **lower bound** on shared source: any edit inside a window breaks the match.
- There is no unrelated GBA game as a control, so the baseline from shared compiler libraries is unknown. It is probably a few percent (hypothesis).
- The "none" LZ77 entries are 6–9 tiny noise hits (a few hundred bytes in total).

**Verdict: yes, one engine lineage, in two generations.**
- **Credits:** all five name Pocketeers (Underground says "Pocketeers Development").
- **Layout:** all five have the same code layout: a long Thumb run, a data gap, then a short Thumb run (2–3 blocks) directly followed by ARM. The gap is long only in Underground. That points to the same link order.
- **Audio:** none uses MP2K. Four carry a Logik State credit and `GBAMOD` modules.
- **Generation A (Porsche Unleashed, Underground):** 84% of Porsche's ARM code is found in Underground. Neither packs images with LZ77. Underground's audio engine is unknown: it has no credit string and no `GBAMOD` tag.
- **Generation B (Underground 2, Most Wanted, Carbon):** 79% of Most Wanted's ARM code is in Carbon and 77% is in Underground 2. Most Wanted also shares the most Thumb code with Carbon. All three pack images with BIOS LZ77.
- **Assets are mostly not shared:** the data-window similarity is at most about 4% (Carbon ↔ Most Wanted).

**For back-checking Carbon:** Most Wanted is the most useful sibling, then Underground 2. The Porsche/Underground generation helps only for the older shared core.

## Sibling notes

- **Porsche Unleashed, EU `AZFP` vs USA `AZFE`:** the same build. Only 3 bytes differ: the region letter, the header checksum, and one Thumb immediate at `0x9CEE6` (`movs r0,#2` in the EU build, `#3` in the USA build, then stored into a struct). **Hypothesis:** a regional default such as language or units.
- **Carbon's two zips:** the same ROM byte for byte. The zips differ only in compression. One has a TorrentZip-style timestamp (1996-12-24 23:32) and the other doesn't (09:32), so it was re-zipped by another tool.
- Both redundant copies (the second Carbon zip, and the Porsche Unleashed USA zip plus its ROM) were deleted afterwards, at the user's request. Their hashes are in [DECISIONS.md](../DECISIONS.md).
