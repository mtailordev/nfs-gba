//! The 2D layer: menu images, HUD sprites, palettes, fonts and text (`docs/formats/ui.md`).
//!
//! The game reaches all of it through a level descriptor. The 12 race levels (`LEVEL_TABLE`) and the menu
//! descriptor (`MENU_DESCRIPTOR`) share one layout: `+0x04` palette block, `+0x10` sprite texel base, `+0x24`
//! sprite materials, `+0x2C` sprite screens, `+0x30` sprite elements. Race levels point at the HUD tables,
//! the menu descriptor at the menu tables.

use super::{LEVEL_TABLE, ptr, u16_at, u32_at};

/// Level descriptor used by the menus (`FUN_08139c8c(world, 0x7F2FE8)` in `menu_scene_setup_a`).
pub const MENU_DESCRIPTOR: usize = 0x7F_2FE8;
/// Menu texel base (the LZ77 bank start; menu descriptor `+0x10`).
pub const MENU_TEXELS: usize = 0x16_C244;
/// Menu materials, 273 × 0x24 (menu descriptor `+0x24`).
pub const MENU_MATERIALS: usize = 0x34_5114;
/// 49 menu palettes of 256 BGR555 colours (menu descriptor `+0x04`; `menu_scene_setup_a` loads `+ p × 0x200`).
pub const MENU_PALETTES: usize = 0x33_EF14;
pub const MENU_PALETTE_COUNT: usize = 49;
/// HUD sprite texels, 4bpp (race descriptor `+0x10`).
pub const HUD_TEXELS: usize = 0x34_7B74;
/// Four 256-colour OBJ palettes (race descriptor `+0x04`; a level picks `+ (+0x58) × 2`).
pub const OBJ_PALETTES: usize = 0x36_C75C;
/// HUD sprite materials, 280 × 0x24 (race descriptor `+0x24`).
pub const HUD_MATERIALS: usize = 0x36_CF5C;
/// Four font descriptors, 0x18 bytes each (`FUN_08141578` picks them for font ids 0xC..0xF).
pub const FONTS: usize = 0x7E_E974;
/// Menu materials holding the glyphs of fonts 0..3. The text functions map font ids 0xC..0xE to materials
/// 12..14; for id 0xF they use a hard-coded offset: 0x1F8 (material 14) in `FUN_08141578`, `FUN_08141840`,
/// `FUN_081419c0`, `FUN_08141b40` and `FUN_08141c88`, but 0x1B0 (material 12) in `FUN_081416d0`.
pub const FONT_MATERIALS: [usize; 4] = [12, 13, 14, 14];
/// Character → glyph index, 256 bytes; `0xFF` = no glyph (`FUN_08162860`).
pub const CHAR_MAP: usize = 0x7F_5BC8;
/// Five 16-colour minimap palettes (`FUN_081430c4` loads one into OBJ palette bank 13).
pub const MINIMAP_PALETTES: usize = 0x7F_44F8;
/// Minimap palette per route, one byte, indexed by `route - 1` (`FUN_081430c4`).
pub const MINIMAP_PALETTE_OF_ROUTE: usize = 0x7F_4598;

/// A 0x24-byte material record: the same layout as the city and vehicle materials.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Material {
    pub index: usize,
    /// `+0x02`. Bit 6: packed with `unpack` (menus, vehicles). Bit 2: OBJ tile order (HUD sprites). Bit 4 is
    /// set on HUD materials 5..33 (meaning unknown).
    pub kind: u16,
    /// `+0x04`: the column map of city walls; unused (stale bytes) in the sprite tables.
    pub aux: u32,
    /// `+0x08`: offset of the texels from the texel base.
    pub offset: usize,
    pub width: usize,
    pub height: usize,
    /// `+0x1E`, `+0x1F`: log2 of width and height (0 where not a power of two).
    pub log2: [u8; 2],
    /// `+0x20`: OBJ palette bank the sprite code sets when it uploads this material (`FUN_08161c24`).
    pub palette: u16,
}

/// The self-indexed material table at `table` (records until the index stops matching).
pub fn materials(rom: &[u8], table: usize) -> Vec<Material> {
    (0..)
        .map(|i| (i, table + 0x24 * i))
        .take_while(|&(i, m)| u16_at(rom, m) as usize == i)
        .map(|(index, m)| Material {
            index,
            kind: u16_at(rom, m + 2),
            aux: u32_at(rom, m + 4),
            offset: u32_at(rom, m + 8) as usize,
            width: u16_at(rom, m + 0x0C) as usize,
            height: u16_at(rom, m + 0x0E) as usize,
            log2: [rom[m + 0x1E], rom[m + 0x1F]],
            palette: u16_at(rom, m + 0x20),
        })
        .collect()
}

/// Where the game's LZ77 ring decoder reads and writes: the packed stream, the 4 KiB ring and the output. `unpack`
/// keeps them in local buffers; `nfsgba_game::race_init` keeps them in the game's RAM (its ring's side effects
/// are part of the exact state).
pub trait RingIo {
    /// The next byte of the packed stream.
    fn next(&mut self) -> u8;
    fn ring(&self, i: usize) -> u8;
    fn set_ring(&mut self, i: usize, b: u8);
    /// The next output byte.
    fn put(&mut self, b: u8);
}

/// The decode loop of the game's decompressor (`lz77_ring_decode`, ARM `0x030042f4`) for a stream of `size`
/// output bytes (the header's size; the ring position starts at 0xFEE). Every byte also goes into the ring,
/// except the one that ends the stream; running out inside a reference skips that byte's ring store and goes on
/// with the next flag, and only a literal ends the routine (ported as is).
pub fn ring_decode(io: &mut impl RingIo, size: u32) {
    let (mut stored, mut left, mut pos) = (0u32, size as i64, 0xFEEusize);
    let (mut flags, mut bit) = (7u32, 7u32);
    loop {
        flags <<= 1;
        bit += 1;
        if bit == 8 {
            bit = 0;
            flags = io.next() as u32;
        }
        if flags & 0x80 == 0 {
            let b = io.next();
            if stored < size {
                left -= 1;
                io.put(b);
                if left <= 0 {
                    return;
                }
            }
            io.set_ring(pos, b);
            stored += 1;
            pos = (pos + 1) & 0xFFF;
            continue;
        }
        let (hi, lo) = (io.next() as usize, io.next() as usize);
        let (len, disp) = ((hi >> 4) + 3, (hi & 0xF) << 8 | lo);
        for _ in 0..len {
            let b = io.ring(pos.wrapping_sub(disp + 1) & 0xFFF);
            if stored < size {
                left -= 1;
                io.put(b);
                if left <= 0 {
                    break;
                }
            }
            io.set_ring(pos, b);
            stored += 1;
            pos = (pos + 1) & 0xFFF;
        }
    }
}

/// The game's decompressor (IWRAM routine at ROM `0x169208`, called by `FUN_08163d30`): the BIOS LZ77 bit
/// stream (`10 ss ss ss` header, MSB-first flags, 2-byte references of length 3..18 and distance 1..4096),
/// decoded through a 4 KiB ring that starts at 0xFEE with bytes 0..0xFED set to 0xFF. References before the
/// start of the output therefore read 0xFF (15 menu blobs rely on it). It writes the header size, which is 8
/// more than the stream encodes; the tail is decoded from the bytes that follow (`docs/formats/ui.md`).
///
/// Ring bytes 0xFEE..0xFFF are uninitialised heap in the game (0xFF here); no stream in the ROM reaches
/// them (`unpack_lowest_reference`, tested), so the output is exact.
pub fn unpack(rom: &[u8], at: usize) -> Vec<u8> {
    struct Local<'a> {
        rom: &'a [u8],
        src: usize,
        ring: [u8; 0x1000],
        out: Vec<u8>,
    }
    impl RingIo for Local<'_> {
        fn next(&mut self) -> u8 {
            let b = self.rom.get(self.src).copied().unwrap_or(0);
            self.src += 1;
            b
        }
        fn ring(&self, i: usize) -> u8 {
            self.ring[i]
        }
        fn set_ring(&mut self, i: usize, b: u8) {
            self.ring[i] = b;
        }
        fn put(&mut self, b: u8) {
            self.out.push(b);
        }
    }
    let size = u32_at(rom, at) >> 8;
    let mut io = Local {
        rom,
        src: at + 4,
        ring: [0xFF; 0x1000],
        out: Vec::with_capacity(size as usize + 18),
    };
    ring_decode(&mut io, size);
    io.out
}

/// The lowest output position a packed stream references, relative to its first output byte, over the
/// `size − 8` bytes the stream really encodes. Negative: it reads the ring's 0xFF fill (the plain BIOS
/// decoder `crate::lz77` cannot decode it); below −0xFEE: it would read uninitialised ring bytes.
pub fn unpack_lowest_reference(rom: &[u8], at: usize) -> i64 {
    let size = (u32_at(rom, at) >> 8) as i64 - 8;
    let (mut src, mut n, mut flags, mut bit, mut lowest) = (at + 4, 0i64, 7u32, 7u32, 0i64);
    while n < size {
        flags <<= 1;
        bit += 1;
        if bit == 8 {
            bit = 0;
            flags = rom[src] as u32;
            src += 1;
        }
        if flags & 0x80 == 0 {
            src += 1;
            n += 1;
        } else {
            let (hi, lo) = (rom[src] as i64, rom[src + 1] as i64);
            src += 2;
            lowest = lowest.min(n - ((hi & 0xF) << 8 | lo) - 1);
            n += (hi >> 4) + 3;
        }
    }
    lowest
}

/// Exact port of `FUN_08164d90(dst, stride, src, w, h, key)`, the menu blitter: copies `w × h` bytes row by
/// row, skipping bytes equal to `key` (0 at every call seen), through 16-bit read-modify-write. It does not
/// clip; bytes outside `fb` are dropped here (NOT 1:1 (N1) only for such out-of-frame calls).
#[allow(clippy::too_many_arguments)] // mirrors the routine's arguments
pub fn blit(fb: &mut [u8], stride: usize, x: i32, y: i32, src: &[u8], w: usize, h: usize, key: u8) {
    for row in 0..h {
        for col in 0..w {
            let p = src[row * w + col];
            let at = (y as i64 + row as i64) * stride as i64 + x as i64 + col as i64;
            if p != key && (0..fb.len() as i64).contains(&at) {
                fb[at as usize] = p;
            }
        }
    }
}

/// The raw bytes of text-table string `key` in `lang` (0 En, 1 Fr, 2 De, 3 It, 4 Es), for `decode_text`
/// and `Font::draw` (`crate::text` decodes them with `decode_text`).
pub fn text_bytes(rom: &[u8], key: usize, lang: usize) -> Vec<u8> {
    let at = ptr(rom, 0x7E_86A0 + 4 * (977 * (lang + 1) + key));
    rom[at..at + rom[at..].iter().position(|&b| b == 0).unwrap()].to_vec()
}

/// 8bpp pixels (palette indices, row-major) of a menu or vehicle material: packed if `kind` bit 6, else raw.
pub fn pixels_8bpp(rom: &[u8], texels: usize, m: &Material) -> Vec<u8> {
    let (at, n) = (texels + m.offset, m.width * m.height);
    if m.kind & 0x40 != 0 {
        let mut p = unpack(rom, at);
        p.resize(n, 0);
        p
    } else {
        rom[at..at + n].to_vec()
    }
}

/// 4bpp pixels of a HUD sprite material as one index (0..15) per pixel, row-major. `kind` bit 2 means GBA
/// OBJ tile order (8×8 tiles, row-major, as uploaded to OBJ VRAM); otherwise linear rows (material 135,
/// the 512×384 minimap, is read that way by `FUN_08142440`). Low nibble first.
pub fn pixels_4bpp(rom: &[u8], texels: usize, m: &Material) -> Vec<u8> {
    let (w, h, at) = (m.width, m.height, texels + m.offset);
    let nibble = |i: usize| (rom[at + i / 2] >> (4 * (i & 1))) & 15;
    (0..w * h)
        .map(|p| {
            let (x, y) = (p % w, p / w);
            if m.kind & 4 != 0 {
                let tile = (y / 8) * (w / 8) + x / 8;
                nibble(tile * 64 + (y % 8) * 8 + x % 8)
            } else {
                nibble(p)
            }
        })
        .collect()
}

/// 256 BGR555 colours at `at`.
pub fn palette_at(rom: &[u8], at: usize) -> Vec<u16> {
    (0..256).map(|i| u16_at(rom, at + 2 * i)).collect()
}

/// The OBJ palette a race level loads: descriptor `+0x04 + (+0x58) × 2` (`race_load_palettes`). At run time
/// bank 13 is then replaced by the route's minimap colours (`minimap_palette`).
pub fn obj_palette(rom: &[u8], level: usize) -> Vec<u16> {
    let rec = LEVEL_TABLE + 0x68 * level;
    palette_at(rom, ptr(rom, rec + 4) + 2 * u16_at(rom, rec + 0x58) as usize)
}

/// The 16 minimap colours for `route` (1-based, as the game stores it), which `FUN_081430c4` loads into OBJ
/// palette entries 0xD0..0xDF.
pub fn minimap_palette(rom: &[u8], route: usize) -> Vec<u16> {
    let at = MINIMAP_PALETTES + 0x20 * rom[MINIMAP_PALETTE_OF_ROUTE + route - 1] as usize;
    (0..16).map(|i| u16_at(rom, at + 2 * i)).collect()
}

/// Screen table entry (8 bytes): an origin and a run of elements (`FUN_08161c24`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Screen {
    pub x: i16,
    pub y: i16,
    pub first: usize,
    pub count: usize,
}

/// Sprite element (0x14 bytes). Element `k` of a screen drives OAM entry `55 - k`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Element {
    pub x: i16,
    pub y: i16,
    /// First material; the object's `frame` is added to it.
    pub material: u16,
    /// OBJ tile slot, relative to the OBJ tile base (`0x030064E0`, 0x200 in the bitmap modes).
    pub tile: u16,
    /// `+0x0C`: the element's index within its screen.
    pub slot: u8,
    /// `+0x0D`: OBJ shape and size, 0..11 (`FUN_08160f10`; see `OBJ_SIZES`).
    pub size: u8,
    /// `+0x11`: palette bank at init; -1 = 256 colours.
    pub palette: i8,
    pub raw: [u8; 0x14],
}

/// Width and height of the 12 OBJ size codes of `Element::size`.
pub const OBJ_SIZES: [(usize, usize); 12] = [
    (8, 8),
    (16, 16),
    (32, 32),
    (64, 64),
    (16, 8),
    (32, 8),
    (32, 16),
    (64, 32),
    (8, 16),
    (8, 32),
    (16, 32),
    (32, 64),
];

/// A level's sprite tables. The screen table directly precedes the element table, which gives its length.
#[derive(Debug, Clone)]
pub struct SpriteBank {
    pub texels: usize,
    pub materials: Vec<Material>,
    pub screens: Vec<Screen>,
    pub elements: Vec<Element>,
}

/// Sprite tables of a level descriptor (`LEVEL_TABLE + 0x68 × level` or `MENU_DESCRIPTOR`).
pub fn sprite_bank(rom: &[u8], descriptor: usize) -> SpriteBank {
    let (screens_at, elements_at) = (ptr(rom, descriptor + 0x2C), ptr(rom, descriptor + 0x30));
    let screens: Vec<Screen> = (0..(elements_at - screens_at) / 8)
        .map(|i| {
            let s = screens_at + 8 * i;
            Screen {
                x: u16_at(rom, s) as i16,
                y: u16_at(rom, s + 2) as i16,
                first: u16_at(rom, s + 4) as usize,
                count: u16_at(rom, s + 6) as usize,
            }
        })
        .collect();
    let n = screens.iter().map(|s| s.first + s.count).max().unwrap_or(0);
    let elements = (0..n)
        .map(|i| {
            let e = elements_at + 0x14 * i;
            let raw: [u8; 0x14] = rom[e..e + 0x14].try_into().unwrap();
            Element {
                x: u16_at(rom, e) as i16,
                y: u16_at(rom, e + 2) as i16,
                material: u16_at(rom, e + 4),
                tile: u16_at(rom, e + 6),
                slot: raw[0x0C],
                size: raw[0x0D],
                palette: raw[0x11] as i8,
                raw,
            }
        })
        .collect();
    SpriteBank {
        texels: ptr(rom, descriptor + 0x10),
        materials: materials(rom, ptr(rom, descriptor + 0x24)),
        screens,
        elements,
    }
}

/// Run-time state of one element (0x10 bytes; the array at sprite-screen `+0x14`, world `+0xB8` in a race).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Object {
    /// Bit 0 visible, bit 1 semi-transparent (OBJ mode 1).
    pub flags: u16,
    /// Affine scale (0x100 = 1); both 0 = none.
    pub scale: [i16; 2],
    /// Added to the element's material.
    pub frame: i16,
    /// Frame currently in VRAM; a difference triggers an upload.
    pub loaded: i16,
    pub dy: i16,
    pub dx: i16,
    /// Rotation, 0x4000 = one turn; 0 = none.
    pub angle: u16,
}

impl Object {
    pub fn from_bytes(b: &[u8]) -> Self {
        let h = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
        Object {
            flags: h(0),
            scale: [h(2) as i16, h(4) as i16],
            frame: h(6) as i16,
            loaded: h(8) as i16,
            dy: h(0xA) as i16,
            dx: h(0xC) as i16,
            angle: h(0xE),
        }
    }
}

/// A tile upload to OBJ VRAM (`FUN_08161a98`): `len` bytes from ROM `src` to tile `tile` (absolute OBJ tile
/// number; VRAM address `0x06010000 + 32 × tile`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Upload {
    pub tile: usize,
    pub src: usize,
    pub len: usize,
}

/// Shadow OAM as the game keeps it at `0x030064F0`: 128 entries of four u16 (attr0, attr1, attr2 and the
/// interleaved affine parameter), copied to OAM each frame.
pub type Oam = [[u16; 4]; 128];

/// Exact port of `FUN_08161c24(screen, init)`: writes the shadow OAM for `screen` of `bank` from `objects`
/// and returns the VRAM uploads in call order. Element `k` uses OAM entry `0x37 - k`; affine objects take
/// matrices 1, 2, … in element order. `tile_base` is the OBJ tile base (`*0x030064E0`, 0x200 in a race).
/// Fields the routine never writes (attr0 mosaic, attr2 priority) keep what `oam` held.
pub fn update_sprites(
    rom: &[u8],
    bank: &SpriteBank,
    screen: usize,
    objects: &mut [Object],
    oam: &mut Oam,
    init: bool,
    tile_base: u16,
) -> Vec<Upload> {
    let s = bank.screens[screen];
    let mut uploads = Vec::new();
    let mut upload = |slot: u16, m: &Material, bits: usize| {
        let bytes = if bits == 8 {
            m.width * m.height
        } else {
            m.width * m.height / 2
        };
        uploads.push(Upload {
            tile: (slot.wrapping_add(tile_base)) as usize,
            src: bank.texels + m.offset,
            len: bytes & !31,
        });
    };
    let mut matrix = 1usize;
    for (k, o) in objects.iter_mut().enumerate().take(s.count) {
        let (e, i) = (bank.elements[s.first + k], 0x37 - k);
        let a = &mut oam[i];
        if o.flags & 1 != 0 {
            // FUN_081611c4: x into attr1 bits 0..8, y into the low byte of attr0.
            let x = (e.x.wrapping_add(s.x) as i32 + o.dx as i32) as u16;
            let y = (e.y.wrapping_add(s.y) as i32 + o.dy as i32) as u16;
            a[1] = a[1] & 0xFE00 | x & 0x1FF;
            a[0] = a[0] & 0xFF00 | y & 0xFF;
        } else {
            // FUN_081610c8: y = 160, affine mode off.
            a[0] = a[0] & 0xFC00 | 0xA0;
        }
        let set_affine_index = |a: &mut [u16; 4], m: usize| a[1] = a[1] & !(0x1F << 9) | ((m as u16 & 0x1F) << 9);
        if !init {
            if o.frame != o.loaded {
                let m = &bank.materials[(e.material as i32 + o.frame as i32) as usize];
                o.loaded = o.frame;
                if e.palette == -1 {
                    upload(e.tile, m, 8);
                } else {
                    a[2] = a[2] & 0x0FFF | (m.palette & 0xF) << 12;
                    upload(e.tile, m, 4);
                }
            }
            a[0] = a[0] & !(3 << 10) | (((o.flags >> 1) & 1) << 10);
            let affine = o.angle != 0 || o.scale != [0, 0];
            if !affine {
                set_affine_index(a, 0);
                continue;
            }
            a[0] = a[0] & !(3 << 8) | 1 << 8;
            set_affine_index(a, matrix);
            let [sx, sy] = if o.angle != 0 && (o.scale[0] == 0 || o.scale[1] == 0) {
                [0x100, 0x100]
            } else {
                [o.scale[0] as i32, o.scale[1] as i32]
            };
            let angle = if o.angle != 0 { o.angle as u32 } else { 0 };
            let (c, sn) = (
                nfsgba_fixed::cos_q14(rom, angle as i32),
                nfsgba_fixed::sin_q14(rom, angle as i32),
            );
            // FUN_0816144c: pa = sx·cos, pb = sx·sin, pc = −sy·sin, pd = sy·cos (>> 14).
            for (j, v) in [(sx * c) >> 14, (sx * sn) >> 14, (sy * -sn) >> 14, (sy * c) >> 14]
                .into_iter()
                .enumerate()
            {
                oam[matrix * 4 + j][3] = v as u16;
            }
            matrix += 1;
        } else {
            let m = &bank.materials[e.material as usize];
            a[2] = a[2] & 0xFC00 | (e.tile.wrapping_add(tile_base) & 0x3FF);
            let (shape, size) = match e.size {
                0..=3 => (0, e.size),
                4..=7 => (1, e.size - 4),
                _ => (2, e.size.min(11) - 8),
            };
            a[0] = a[0] & 0x3FFF | (shape as u16) << 14;
            a[1] = a[1] & 0x3FFF | (size as u16) << 14;
            set_affine_index(a, 0);
            a[0] = a[0] & !(1 << 13) | ((e.palette == -1) as u16) << 13;
            a[2] = a[2] & 0x0FFF | ((e.palette as u8 as u16) & 0xF) << 12;
            a[0] &= !(3 << 8);
            // FUN_0816131c(entry, 1): attr1 bit 12 set, bit 13 clear; the frame update rewrites both with
            // the affine index.
            a[1] = a[1] & !(1 << 13) | 1 << 12;
            upload(e.tile, m, if e.palette == -1 { 8 } else { 4 });
        }
    }
    uploads
}

/// A font: descriptor at `FONTS + 0x18 × id` (id = font id − 0xC) with its glyph material.
#[derive(Debug, Clone)]
pub struct Font {
    /// `+0x01` bit 0: skip pixels equal to `key`.
    pub flags: u8,
    pub key: u8,
    /// `+0x03`: added after every character.
    pub spacing: i8,
    /// `+0x04`, `+0x06`: glyph cell; glyph `g` is `width × height` bytes at `glyphs + g × width × height`.
    pub width: usize,
    pub height: usize,
    /// `+0x08`: line advance of the word-wrapping renderer.
    pub line_height: usize,
    /// ROM offset of glyph 0 (the game fills descriptor `+0x0C` with it at run time).
    pub glyphs: usize,
    /// `+0x10`: drawn width per glyph (224 bytes).
    pub widths: Vec<u8>,
    /// `+0x14`: rows to shift each glyph down (224 signed bytes; negative for raised capitals).
    pub y_offsets: Vec<i8>,
    pub raw: [u8; 0x18],
}

/// Font `id` (0..3 for the game's font ids 0xC..0xF) with the glyphs of menu material `glyph_material`.
pub fn font(rom: &[u8], id: usize, glyph_material: usize) -> Font {
    let d = FONTS + 0x18 * id;
    let raw: [u8; 0x18] = rom[d..d + 0x18].try_into().unwrap();
    let (widths, yoffs) = (ptr(rom, d + 0x10), ptr(rom, d + 0x14));
    Font {
        flags: raw[1],
        key: raw[2],
        spacing: raw[3] as i8,
        width: u16_at(rom, d + 4) as usize,
        height: u16_at(rom, d + 6) as usize,
        line_height: u16_at(rom, d + 8) as usize,
        glyphs: MENU_TEXELS + u32_at(rom, MENU_MATERIALS + 0x24 * glyph_material + 8) as usize,
        widths: rom[widths..widths + 224].to_vec(),
        y_offsets: rom[yoffs..yoffs + 224].iter().map(|&b| b as i8).collect(),
        raw,
    }
}

/// The four fonts with their usual glyph materials (`FONT_MATERIALS`).
pub fn fonts(rom: &[u8]) -> Vec<Font> {
    (0..4).map(|f| font(rom, f, FONT_MATERIALS[f])).collect()
}

/// Horizontal alignment of `Font::draw` (`FUN_08162860` argument 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Centre,
    Right,
}

impl Font {
    /// Width the game measures for `text`: per character the glyph width (or glyph 0's for `~`, nothing for
    /// unmapped bytes) plus `spacing`.
    pub fn measure(&self, rom: &[u8], text: &[u8]) -> i32 {
        text.iter()
            .map(|&c| {
                let g = rom[CHAR_MAP + c as usize];
                let w = match (g, c) {
                    (0xFF, _) => 0,
                    (_, 0x7E) => self.widths[0] as i32,
                    _ => self.widths[g as usize] as i32,
                };
                w + self.spacing as i32
            })
            .sum()
    }

    /// Exact port of `FUN_08162860` + `FUN_08162720`: draws `text` into an 8bpp frame buffer (`fb`, `stride`
    /// bytes per row) at (`x`, `y`), adding `colour` to every glyph pixel. The game draws only when
    /// `0 <= y <= 159 - height` and the aligned `x < stride`.
    ///
    /// NOT 1:1 (N1) only where the game would write outside the frame buffer (negative aligned `x` on row 0):
    /// such bytes are dropped here.
    #[allow(clippy::too_many_arguments)] // mirrors FUN_08162860
    pub fn draw(
        &self,
        rom: &[u8],
        fb: &mut [u8],
        stride: usize,
        x: i32,
        y: i32,
        text: &[u8],
        colour: i16,
        align: Align,
    ) {
        let width = self.measure(rom, text);
        let x = match align {
            Align::Left => x,
            Align::Centre => x - (width >> 1),
            Align::Right => x - width,
        };
        if y < 0 || y > 0x9F - self.height as i32 || x >= stride as i32 {
            return;
        }
        let mut at = y as i64 * stride as i64 + x as i64;
        let w0 = self.widths[0] as i64;
        for &c in text {
            let g = rom[CHAR_MAP + c as usize];
            let advance = if g == 0xFF {
                0
            } else if c == 0x7E {
                w0
            } else {
                if g != 0 {
                    let dst = at + self.y_offsets[g as usize] as i64 * stride as i64;
                    self.blit(rom, fb, stride, g as usize, dst, colour);
                }
                self.widths[g as usize] as i64
            };
            at += advance + self.spacing as i64;
        }
    }

    /// Exact port of `FUN_08163278` + `FUN_08162a1c`: word-wrapped text from (`x`, `y`), lines
    /// `line_height` rows apart, at most `max_width` pixels per line and `max_lines` lines. A word is measured
    /// without its trailing space and moves to a new line when it would pass `max_width`; `\n` starts a new
    /// line. Returns the number of line breaks taken (the game's return value).
    #[allow(clippy::too_many_arguments)] // mirrors FUN_08163278
    pub fn draw_wrapped(
        &self,
        rom: &[u8],
        fb: &mut [u8],
        stride: usize,
        x: i32,
        y: i32,
        text: &[u8],
        max_width: i32,
        max_lines: i32,
        colour: i16,
    ) -> i32 {
        if y < 0 || y > 0x9F - self.height as i32 || x >= stride as i32 {
            return 0;
        }
        let s = stride as i64;
        let byte = |i: usize| text.get(i).copied().unwrap_or(0);
        let glyph = |c: u8| rom[CHAR_MAP + c as usize];
        let width = |c: u8| match glyph(c) {
            0xFF => 0,
            _ if c == 0x7E => self.widths[0] as i32,
            g => self.widths[g as usize] as i32,
        };
        let (mut line, mut used, mut lines, mut i) = (y as i64 * s + x as i64, 0i32, 0i32, 0usize);
        let mut at = line;
        loop {
            if byte(i) == b'\n' {
                i += 1;
                (used, line, lines) = (0, line + self.line_height as i64 * s, lines + 1);
                at = line;
                if lines >= max_lines {
                    return lines;
                }
            }
            if byte(i) == 0 {
                return lines;
            }
            let (mut n, mut word) = (0, 0);
            loop {
                match byte(i + n) {
                    b'\n' => break,
                    0 | b' ' => {
                        n += 1;
                        break;
                    }
                    c => word += width(c) + self.spacing as i32,
                }
                n += 1;
            }
            if max_width < used + word {
                (used, line, lines) = (0, line + self.line_height as i64 * s, lines + 1);
                at = line;
                if lines >= max_lines {
                    return lines;
                }
            }
            for _ in 0..n {
                let (c, g) = (byte(i), glyph(byte(i)));
                if g != 0xFF && c != 0x7E && g != 0 {
                    self.blit(
                        rom,
                        fb,
                        stride,
                        g as usize,
                        at + self.y_offsets[g as usize] as i64 * s,
                        colour,
                    );
                }
                let advance = width(c) + self.spacing as i32;
                (at, used, i) = (at + advance as i64, used + advance, i + 1);
                if byte(i) == 0 {
                    return lines;
                }
            }
        }
    }

    /// `FUN_08162720`: one glyph, written through 16-bit read-modify-write as on VRAM.
    fn blit(&self, rom: &[u8], fb: &mut [u8], stride: usize, g: usize, dst: i64, colour: i16) {
        if g == 0x7E {
            return;
        }
        let (w, drawn) = (self.width, self.widths[g] as usize);
        let src = self.glyphs + g * w * self.height;
        for row in 0..self.height {
            for col in 0..drawn {
                let p = rom[src + row * w + col];
                if self.flags & 1 != 0 && p == self.key {
                    continue;
                }
                let a = dst + (row * stride + col) as i64;
                let v = (p as i32 + colour as i32) as u16;
                let even = (a & !1) as usize;
                if a < 0 || even + 1 >= fb.len() {
                    continue;
                }
                if a & 1 == 0 {
                    fb[even] = v as u8;
                    fb[even + 1] |= (v >> 8) as u8; // `h & 0xFF00 | v`: a carry leaks into the odd byte
                } else {
                    fb[even + 1] = v as u8;
                }
            }
        }
    }
}

/// Decodes game text: Windows-1252, except `{` and `|`, which the fonts draw as the A and B buttons.
pub fn decode_text(bytes: &[u8]) -> String {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8D}', 'Ž', '\u{8F}', '\u{90}', '‘',
        '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9D}', 'ž', 'Ÿ',
    ];
    bytes
        .iter()
        .map(|&b| match b {
            0x7B => 'Ⓐ',
            0x7C => 'Ⓑ',
            0x80..=0x9F => HIGH[b as usize - 0x80],
            _ => b as char,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nfsgba_testkit::rom;

    /// A reference dump (`tools/mgba_ctl.py dump`).
    fn dump(name: &str, domain: &str) -> Option<Vec<u8>> {
        nfsgba_testkit::read(&format!("{name}.{domain}.bin"))
    }

    fn u16s(b: &[u8]) -> Vec<u16> {
        b.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
    }

    #[test]
    fn tables_have_the_documented_sizes() {
        let Some(rom) = rom() else { return };
        let menu = sprite_bank(&rom, MENU_DESCRIPTOR);
        assert_eq!((menu.texels, menu.materials.len()), (MENU_TEXELS, 273));
        assert_eq!((menu.screens.len(), menu.elements.len()), (10, 47));
        let hud = sprite_bank(&rom, LEVEL_TABLE);
        assert_eq!((hud.texels, hud.materials.len()), (HUD_TEXELS, 280));
        assert_eq!((hud.screens.len(), hud.elements.len()), (4, 185));
        for l in 0..12 {
            let rec = LEVEL_TABLE + 0x68 * l;
            assert_eq!(u16_at(&rom, rec + 0x58), 0, "level {l} OBJ palette");
            for field in [0x04, 0x10, 0x24, 0x2C, 0x30] {
                assert_eq!(
                    u32_at(&rom, rec + field),
                    u32_at(&rom, LEVEL_TABLE + field),
                    "level {l} +{field:#x}"
                );
            }
        }
        // The tables tile the ROM: HUD texels end at the OBJ palettes, menu texels at the menu palettes.
        let last = hud.materials.last().unwrap();
        assert_eq!(HUD_TEXELS + last.offset + last.width * last.height / 2, OBJ_PALETTES);
        let last = menu.materials.last().unwrap();
        assert_eq!(MENU_TEXELS + last.offset + last.width * last.height, MENU_PALETTES);
        assert_eq!(MENU_PALETTES + 0x200 * MENU_PALETTE_COUNT, MENU_MATERIALS);
    }

    #[test]
    fn unpack_matches_the_bios_decoder_and_stays_in_the_ring() {
        let Some(rom) = rom() else { return };
        let vehicle_texels = ptr(&rom, LEVEL_TABLE + 0x0C);
        let menu = materials(&rom, MENU_MATERIALS).into_iter().map(|m| (MENU_TEXELS, m));
        let vehicle = materials(&rom, ptr(&rom, LEVEL_TABLE + 0x20))
            .into_iter()
            .map(|m| (vehicle_texels, m));
        let (mut packed, mut before_start) = (0, 0);
        for (base, m) in menu.chain(vehicle).filter(|(_, m)| m.kind & 0x40 != 0) {
            let at = base + m.offset;
            let lowest = unpack_lowest_reference(&rom, at);
            assert!(lowest >= -0xFEE, "{at:#x} reads uninitialised ring bytes");
            let out = unpack(&rom, at);
            assert!(out.len() >= m.width * m.height);
            if lowest < 0 {
                before_start += 1;
            } else {
                // Self-contained streams: identical to the BIOS decoder over the encoded bytes.
                let bios = crate::lz77(&rom, at);
                assert_eq!(out[..bios.len()], bios[..], "{at:#x}");
            }
            packed += 1;
        }
        assert_eq!((packed, before_start), (299, 15));
    }

    #[test]
    fn language_screen_is_recomposed_exactly() {
        let Some(rom) = rom() else { return };
        let (Some(vram), Some(pal)) = (dump("ui-2d/lang", "vram"), dump("ui-2d/lang", "palette")) else {
            return;
        };
        // menu_scene_setup(material 4, palette 4); FUN_08136d74 blits the highlighted flag (material 181);
        // FUN_0812bd60(TEXT_SELECT) draws the prompt: text right-aligned at (238, 147) in font 0xE, then the
        // A button (material 156) at (221 - width, 143).
        let menu = materials(&rom, MENU_MATERIALS);
        let mut fb = pixels_8bpp(&rom, MENU_TEXELS, &menu[4]);
        let sprite = |m: usize| (pixels_8bpp(&rom, MENU_TEXELS, &menu[m]), menu[m].width, menu[m].height);
        let (flag, w, h) = sprite(181);
        blit(&mut fb, 240, 9, 19, &flag, w, h, 0);
        let font = &fonts(&rom)[2];
        let select = text_bytes(&rom, 498, 0);
        font.draw(&rom, &mut fb, 240, 0xEE, 0x93, &select, 0, Align::Right);
        let (button, w, h) = sprite(156);
        blit(&mut fb, 240, 0xDD - font.measure(&rom, &select), 0x8F, &button, w, h, 0);
        let differ = fb.iter().zip(&vram).filter(|(a, b)| a != b).count();
        assert_eq!(differ, 0, "bytes differing from the frame buffer");
        assert_eq!(u16s(&pal[..512]), palette_at(&rom, MENU_PALETTES + 4 * 0x200));
    }

    /// The displayed mode-4 page of a dump (DISPCNT bit 4) and its BG palette.
    fn screen(name: &str) -> Option<(Vec<u8>, Vec<u16>)> {
        let (io, vram, pal) = (dump(name, "io")?, dump(name, "vram")?, dump(name, "palette")?);
        let page = if io[0] & 0x10 != 0 { 0xA000 } else { 0 };
        Some((vram[page..page + 240 * 160].to_vec(), u16s(&pal[..512])))
    }

    #[test]
    fn intro_screens_are_recomposed_exactly() {
        let Some(rom) = rom() else { return };
        let menu = materials(&rom, MENU_MATERIALS);
        // Health and safety (English) and the EA logo: plain images; material 7 uses palette 1, whose colours
        // 0..3 the health screen (FUN_081315a0, state 0x2F) replaces from 0x7E5E48; colour 4 is animated.
        for (name, m, p, patched) in [("ui-2d/n1", 7, 1, 5), ("ui-2d/n2", 2, 2, 0)] {
            let Some((fb, pal)) = screen(name) else { return };
            assert!(pixels_8bpp(&rom, MENU_TEXELS, &menu[m]) == fb, "{name}");
            let mut want = palette_at(&rom, MENU_PALETTES + 0x200 * p);
            for (i, c) in want.iter_mut().enumerate().take(patched.min(4)) {
                *c = u16_at(&rom, 0x7E_5E48 + 2 * i);
            }
            assert_eq!(pal[patched..], want[patched..], "{name}");
            assert_eq!(pal[..patched.min(4)], want[..patched.min(4)], "{name}");
        }
        // Public service announcement: material 1 (palette 1), the title centred in font 0xD at x 120,
        // rows 6 and 16, and TEXT_PUBLIC_SERVICE_TEXT_1 wrapped in font 0xE from (8, 30), colour 8.
        let Some((want, pal)) = screen("ui-2d/n3") else { return };
        let (f, mut fb) = (fonts(&rom), pixels_8bpp(&rom, MENU_TEXELS, &menu[1]));
        f[1].draw(&rom, &mut fb, 240, 120, 6, &text_bytes(&rom, 410, 0), 0, Align::Centre);
        f[1].draw(&rom, &mut fb, 240, 120, 16, &text_bytes(&rom, 411, 0), 0, Align::Centre);
        f[2].draw_wrapped(&rom, &mut fb, 240, 8, 30, &text_bytes(&rom, 409, 0), 224, 99, 8);
        let differ = fb.iter().zip(&want).filter(|(a, b)| a != b).count();
        assert_eq!(differ, 0, "bytes differing from the PSA frame buffer");
        assert_eq!(pal, palette_at(&rom, MENU_PALETTES + 0x200));

        // Title: material 5 (palette 5) with the logo (202), PRESS START (191, English; 191..195 by
        // language), LICENSED BY NINTENDO (203) and the copyright line (196, English) blitted over it. The
        // logo at x 96 is 146 wide: the blitter does not clip, so its last two columns wrap to the next row.
        let Some((want, pal)) = screen("ui-2d/n7") else { return };
        let mut fb = pixels_8bpp(&rom, MENU_TEXELS, &menu[5]);
        for (m, x, y) in [(202, 96, 4), (191, 170, 80), (203, 48, 138), (196, 0, 150)] {
            let (w, h) = (menu[m].width, menu[m].height);
            blit(&mut fb, 240, x, y, &pixels_8bpp(&rom, MENU_TEXELS, &menu[m]), w, h, 0);
        }
        assert!(fb == want, "title screen");
        assert_eq!(pal, palette_at(&rom, MENU_PALETTES + 5 * 0x200));
    }

    #[test]
    fn hud_matches_the_race_dump() {
        let Some(rom) = rom() else { return };
        let (Some(oam), Some(vram), Some(pal), Some(wram)) = (
            dump("mgba/race", "oam"),
            dump("mgba/race", "vram"),
            dump("mgba/race", "palette"),
            dump("mgba/race", "wram"),
        ) else {
            return;
        };
        // OBJ palette: level 0's, with bank 13 replaced by the minimap colours of the reference route (23).
        let mut expect = obj_palette(&rom, 0);
        expect[0xD0..0xE0].copy_from_slice(&minimap_palette(&rom, 23));
        assert_eq!(u16s(&pal[512..]), expect);

        // The race's sprite screen is world + 0xA4 (0x03000164): object array at 0x0201F828, screen 0.
        let bank = sprite_bank(&rom, LEVEL_TABLE);
        let objects: Vec<Object> = (0..47)
            .map(|k| Object::from_bytes(&wram[0x1_F828 + 16 * k..]))
            .collect();
        let reference: Vec<[u16; 4]> = (0..128)
            .map(|i| {
                let w = u16s(&oam[8 * i..8 * i + 8]);
                [w[0], w[1], w[2], w[3]]
            })
            .collect();
        // Start from the dump with every bit the routine owns inverted, then run init and one frame (forcing
        // the upload path, which rewrites the palette bank from the material).
        let mut shadow: Oam = reference.clone().try_into().unwrap();
        for a in shadow.iter_mut().skip(9).take(47) {
            a[0] ^= 0xEFFF;
            a[1] ^= 0xFFFF;
            a[2] ^= 0xF3FF;
        }
        let mut live = objects.clone();
        let uploads = update_sprites(&rom, &bank, 0, &mut live, &mut shadow, true, 0x200);
        for o in live.iter_mut() {
            o.loaded = o.frame ^ 1;
        }
        update_sprites(&rom, &bank, 0, &mut live, &mut shadow, false, 0x200);
        for (k, o) in objects.iter().enumerate() {
            let i = 0x37 - k;
            // A hidden element's x is never written (it keeps whatever the entry held before).
            let x_mask = if o.flags & 1 != 0 { 0xFFFF } else { 0xFE00 };
            let got = [shadow[i][0], shadow[i][1] & x_mask, shadow[i][2]];
            let want = [reference[i][0], reference[i][1] & x_mask, reference[i][2]];
            assert_eq!(got, want, "element {k}, OAM entry {i}");
        }
        let affine = objects
            .iter()
            .filter(|o| o.flags & 1 != 0 && (o.angle != 0 || o.scale != [0, 0]));
        for m in 1..=affine.count() {
            let got: Vec<u16> = (0..4).map(|j| shadow[4 * m + j][3]).collect();
            let want: Vec<u16> = (0..4).map(|j| reference[4 * m + j][3]).collect();
            assert_eq!(got, want, "affine matrix {m}");
        }
        // Uploaded tiles are in OBJ VRAM, except the needle (37) and the minimap (38), which are redrawn in
        // VRAM at run time.
        for (k, u) in uploads.iter().enumerate() {
            if k == 37 || k == 38 {
                continue;
            }
            let m = &bank.materials[(bank.elements[k].material as i32 + objects[k].frame as i32) as usize];
            let (v, src) = (0x1_0000 + 32 * u.tile, HUD_TEXELS + m.offset);
            assert_eq!(vram[v..v + u.len], rom[src..src + u.len], "element {k} tiles");
        }
    }

    #[test]
    fn text_uses_windows_1252_and_button_glyphs() {
        let Some(rom) = rom() else { return };
        assert_eq!(decode_text(&text_bytes(&rom, 500, 2)), "STRECKE WÄHLEN");
        assert_eq!(decode_text(b"{ SELECT"), "\u{24B6} SELECT");
        // The glyph map is the identity from 0x20, except 0x8E, which borrows the empty glyph of 0x84.
        for c in 0x20..=0xFFusize {
            let want = if c == 0x8E { 0x64 } else { c - 0x20 };
            assert_eq!(rom[CHAR_MAP + c] as usize, want);
        }
        assert!((0..0x20).all(|c| rom[CHAR_MAP + c] == 0xFF));
        let f = fonts(&rom);
        assert_eq!([f[0].width, f[0].height, f[1].height, f[2].height], [13, 17, 13, 12]);
        let menu = materials(&rom, MENU_MATERIALS);
        for (font, &m) in f.iter().zip(&FONT_MATERIALS) {
            assert_eq!(menu[m].height, 224 * font.height, "224 glyphs of {} rows", font.height);
        }
    }
}
