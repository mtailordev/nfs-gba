# Text table (Carbon `BN7E`)

**Status: verified** by decoding in a scratch script (no parser in `tools/` yet). Offsets are ROM offsets; pointers in the ROM are absolute GBA addresses (`0x08000000 + offset`).

## Layout

One array of 32-bit pointers at **`0x7E86A0`**, 5,867 entries plus a terminator:

| Entries | Points to |
|---|---|
| 0–976 | **key names**, NUL-terminated ASCII (`COPYRIGHT_NOTICE_TITLE`, `CREDITS_LN1`, …, `TEXT_ACCESS_EEPROM`) |
| 977 + L×977 + k | **string k in language L**, NUL-terminated. L: 0 English, 1 French, 2 German, 3 Italian, 4 Spanish |
| 5862–5866 | **`GBAMOD30` music modules** (`0x04EA14`, `0x04F134`, `0x04F8DC`, `0x050034`, `0x0507FC`) |
| 5867 | `0x00000004`, not a pointer. Probably a terminator or count (unknown) |

- **Order:** the key pointers descend strictly, so the keys are stored in ROM back to front. The keys are roughly alphabetical but not in strict ASCII order (e.g. `TEXT_ZONE_6` comes before `TEXT_ZONES`).
- **Encoding:** strings are 8-bit **Windows-1252**, except `{` and `|`, which the fonts draw as the A and B buttons (glyph map `0x7F5BC8`, [ui.md](ui.md)). `text()` decodes them that way (`ui::decode_text`).
- **Untranslated strings:** some remain in English in every language (e.g. `TEXT_ACCESS_EEPROM` = "Accessing EEprom").
- **Before the table:** the four words at `0x7E8690` are `32, 31, 208, 113` (unknown).

## Examples

| k | Key | En | Fr | De | It | Es |
|---|---|---|---|---|---|---|
| 500 | `TEXT_SELECT_TRACK` | SELECT TRACK | CHOIX DU PARCOURS | STRECKE WÄHLEN | SCEGLI PISTA | SELECCIONAR PISTA |
| 975 | `TEXT_ZONES` | ZONES | ZONES | ZONEN | ZONE | ZONAS |

## Open

- What the four words before the table and the trailing `4` mean.
- Whether the code indexes by key number k at run time (likely), and where it picks the language.
- ~~The character set for bytes ≥ `0x80`~~ Answered: Windows-1252 (see Encoding).
