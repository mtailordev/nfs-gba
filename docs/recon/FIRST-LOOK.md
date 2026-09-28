# First look

Shallow recon, no disassembly yet. All numbers come from `tools/first_look.py` and are listed in full in [FIRST-LOOK-DATA.md](FIRST-LOOK-DATA.md) (block maps, entropy map, tables). ROM identities are in [ROM-INVENTORY.md](ROM-INVENTORY.md). Offsets are ROM offsets (address minus `0x08000000`). **Hypothesis** marks anything not yet verified.

## Canonical ROM: Carbon `BN7E` `e5298b24` (8 MiB)

### Layout

| Range | What | Evidence |
|---|---|---|
| `0x000000` | ARM entry: `b 0x080000C0` | header entry word `ea00002e` |
| `0x000000–0x0A0000` | structured data, entropy 5–6 | Lots of halfwords in `0xF000–0xFFFF`, i.e. small negative int16s. **Hypothesis:** fixed-point geometry or lookup tables |
| `0x004000–0x034000` (patchy) | high entropy but "smooth" | Neighbouring-byte differences have lower entropy than the bytes themselves. **Hypothesis:** PCM audio samples |
| `0x128000–0x154000`, `0x15C000–0x164000` | **Thumb code** (~224 KiB) | returns and `push {…, lr}` density |
| `0x164000–0x16C000` | **ARM code** (32 KiB) | >35% of words carry the AL condition |
| `0x16C000–0x184000`, `0x224000–0x33C000` | high entropy (≈7.5 bits/byte), **not** smooth | One byte value dominates at 3–7%. **Hypothesis:** custom-compressed or packed data (not BIOS formats, see below) |
| `0x350000–0x368000` | **ARM code** (96 KiB) | as above. Large for ARM, since IWRAM is only 32 KiB. **Hypothesis:** renderer and mixer routines, copied to IWRAM piecemeal or run from ROM |
| `0x368000–0x7F0000` | structured data, entropy 3–6 | Graphics, tables, text |
| `0x797D10–0x7E53EC` | localized text | credits, licences and game text in En/Fr/De/Es/It |
| `0x7E4000–0x7F4000` | pointer tables | see below |
| `0x7F5CF8–0x7FFFFF` | zero padding (41,736 B) | |

**Rough code vs data split:** about 5–6% code (192 KiB Thumb, 144 KiB weak-Thumb blocks, 128 KiB ARM) and 94% data. The classifier works on 16 KiB blocks, so the edges are approximate.

### Strings

- 88,282 printable ASCII runs of 6+ bytes. The full list is in `$NFSGBA_DATA/out/first-look/e5298b24/strings.txt` (not in git).
- **Developer credit:** `Pocketeers` (`0x7988B9`). The licence lines name Electronic Arts, and the car makers each have a trademark notice.
- **Audio credit:** `LS_Play (C) Logik State 2003 www.LogikState.com` (`0x7C0360`).
- **Text keys survived in the ROM:** about 970 `UPPER_SNAKE` strings such as `TEXT_ZONE_1_NAME`, `TEXT_WINGMAN_SELECT` and `COPYRIGHT_NOTICE_TITLE`. These look like text-table IDs and should make the text format easy to label.

### Pointer tables

- 12,117 distinct ROM addresses are referenced by aligned words, and there are 47 runs of 8+ consecutive ROM pointers.
- **`0x7E86A0`: 5,867 pointers** into `0x04EA14–0x7BFC18`, which covers almost the whole data area. **Hypothesis:** the master asset directory. This is the obvious first target for the model viewer.
- **`0x7E559C`: 477 pointers** into the text region. Probably the string table for one language, or the key table.
- **`0x7F38B8`: 65 odd addresses** into the Thumb code, so a table of Thumb function pointers (state or menu handlers, hypothesis).
- There are several tables of about 50 entries inside the code segment (`0x12B000–0x131000`) whose targets sit right after them. They are probably switch tables or string lists.
- **False positive:** the "table" at `0x7E466C` is 32 copies of `0x08080808`, i.e. a byte fill.

### Compression

The BIOS decompressors are **almost unused**:
- Of all pointer targets, only 5 start with a plausible BIOS header, all LZ77.
- 3 of them decode, and all 3 are ≤1 KiB (`0x15CF2C`, `0x15CFD4`, `0x370550`).
- There are no Huffman or RLE candidates.

So the data is either stored raw or packed with a custom scheme; the high-entropy regions above are the place to look. Caveat: data referenced by relative offsets (not absolute pointers) was not scanned.

### Audio

**Logik State's `LS_Play`**, a MOD-style player (the name comes from the credit string). There is no MP2K/M4A: the SelectSong signature is absent, and nothing references the BIOS sound area `0x03007FF0`.

## Engine sharing across the five games

| Game | Code | Thumb / ARM KiB | Thumb shared with Carbon (→ / ←) | ARM shared with Carbon (→ / ←) | Audio | BIOS compression | Code layout |
|---|---|---|---|---|---|---|---|
| Carbon (canonical) | `BN7E` | 336 / 128 | — | — | LS_Play | ~none | yes |
| Most Wanted | `BNWE` | 256 / 16 | 32% / 40% | 37% / **79%** | LS_Play | ~none | yes |
| Underground 2 | `BNFE` | 288 / 80 | 21% / 21% | **70%** / 48% | GBAModPlay | ~none | yes |
| Underground | `BNSE` | 384 / 32 | 13% / 14% | 40% / 42% | no credit string (see below) | ~none | yes |
| Porsche Unleashed | `AZFP` | 176 / 16 | 10% / 21% | 11% / 24% | GBAModPlay | ~none | yes |

How to read it:
- "→" is the share of Carbon's sampled code windows found in the sibling; "←" is the reverse.
- Code is compared after masking addresses and branch offsets, so relinked copies of the same function still match.
- The windows are 32 bytes, so these numbers are a **lower bound** on shared source: any edit inside a window breaks the match.
- There is no unrelated GBA game as a control, so the baseline from shared compiler libraries is unknown. It is probably a few percent (hypothesis).

**Verdict: yes, one engine lineage, in two generations.**
- **Credits:** all five name Pocketeers (Underground says "Pocketeers Development").
- **Layout:** all five have the same code layout: a long Thumb run, a data gap, then a short Thumb run (2–3 blocks) directly followed by ARM. The gap is long only in Underground. That points to the same link order.
- **Other shared traits:** all five skip the BIOS decompressors and MP2K. Four carry a Logik State credit.
- **Generation A (Porsche Unleashed, Underground):** 84% of Porsche's ARM code is found in Underground. Underground has no Logik State string, but this overlap makes GBAModPlay likely there too (hypothesis).
- **Generation B (Underground 2, Most Wanted, Carbon):** 79% of Most Wanted's ARM code is in Carbon and 77% is in Underground 2. Most Wanted also shares the most Thumb code with Carbon.
- **Assets are mostly not shared:** the data-window similarity is at most about 4% (Carbon ↔ Most Wanted).

**For back-checking Carbon:** Most Wanted is the most useful sibling, then Underground 2. The Porsche/Underground generation helps only for the older shared core.

## Sibling notes

- **Porsche Unleashed, EU `AZFP` vs USA `AZFE`:** the same build. Only 3 bytes differ: the region letter, the header checksum, and one Thumb immediate at `0x9CEE6` (`movs r0,#2` in the EU build, `#3` in the USA build, then stored into a struct). **Hypothesis:** a regional default such as language or units.
- **Carbon's two zips:** the same ROM byte for byte. The zips differ only in compression. One has a TorrentZip-style timestamp (1996-12-24 23:32) and the other doesn't (09:32), so it was re-zipped by another tool.
- Both redundant copies (the second Carbon zip, and the Porsche Unleashed USA zip plus its ROM) were deleted afterwards, at the user's request. Their hashes are in [DECISIONS.md](../DECISIONS.md).
