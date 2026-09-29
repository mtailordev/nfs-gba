//! Readers for Need for Speed Carbon: Own the City (GBA, `BN7E`) data, read straight from the user's ROM.
//! Formats are documented in `docs/formats/`; offsets are ROM offsets (GBA address minus `0x0800_0000`).

use std::{env, fs, io, path::PathBuf};

pub const ROM_BASE: u32 = 0x0800_0000;
/// BN7E level descriptors (0x68-byte records). Every record shares the city and the vehicle model bank.
pub const LEVEL_TABLE: usize = 0x7F_2B08;

fn u16_at(rom: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([rom[o], rom[o + 1]])
}
fn i16_at(rom: &[u8], o: usize) -> i16 {
    u16_at(rom, o) as i16
}
fn u32_at(rom: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([rom[o], rom[o + 1], rom[o + 2], rom[o + 3]])
}
fn ptr(rom: &[u8], o: usize) -> usize {
    (u32_at(rom, o) - ROM_BASE) as usize
}

/// `$NFSGBA_DATA`, else `NFSGBA_DATA` from `./.env`, else `./data` (run from the repo root).
pub fn data_dir() -> PathBuf {
    if let Ok(v) = env::var("NFSGBA_DATA") {
        return v.into();
    }
    fs::read_to_string(".env")
        .ok()
        .and_then(|s| {
            s.lines().find_map(|l| {
                l.strip_prefix("NFSGBA_DATA=")
                    .map(|v| v.trim().trim_matches('"').to_owned())
            })
        })
        .map_or_else(|| "data".into(), PathBuf::from)
}

/// The canonical ROM named by `vault/manifest.json` (built by `tools/vault.py`).
pub fn canonical_rom() -> io::Result<Vec<u8>> {
    let dir = data_dir();
    let manifest: serde_json::Value = serde_json::from_slice(&fs::read(dir.join("vault/manifest.json"))?)?;
    let target = &manifest["canonical_target"];
    let rom = manifest["roms"]
        .as_array()
        .and_then(|roms| roms.iter().find(|r| &r["sha1"] == target))
        .and_then(|r| r["vault_file"].as_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "manifest has no canonical ROM"))?;
    fs::read(dir.join(rom))
}

/// GBA BIOS LZ77 (type 0x10) at `at`. The size field claims 8 bytes more than the stream encodes
/// (`docs/formats/lz77-images.md`), so only `size - 8` bytes are decoded and returned.
pub fn lz77(rom: &[u8], at: usize) -> Vec<u8> {
    let size = (u32_at(rom, at) >> 8) as usize - 8;
    let (mut out, mut i) = (Vec::with_capacity(size), at + 4);
    while out.len() < size {
        let flags = rom[i];
        i += 1;
        for bit in 0..8 {
            if out.len() >= size {
                break;
            }
            if flags & (0x80 >> bit) != 0 {
                let (n, back) = (
                    (rom[i] >> 4) as usize + 3,
                    ((rom[i] as usize & 0xF) << 8 | rom[i + 1] as usize) + 1,
                );
                i += 2;
                for _ in 0..n {
                    out.push(out[out.len() - back]);
                }
            } else {
                out.push(rom[i]);
                i += 1;
            }
        }
    }
    out.truncate(size);
    out
}

/// NUL-terminated string `key` in language `lang` (0 En, 1 Fr, 2 De, 3 It, 4 Es) from the text table at
/// `0x7E86A0`; `lang = None` gives the key name itself (`docs/formats/text-table.md`).
pub fn text(rom: &[u8], key: usize, lang: Option<usize>) -> String {
    const TABLE: usize = 0x7E_86A0;
    const KEYS: usize = 977;
    let at = ptr(rom, TABLE + 4 * lang.map_or(key, |l| KEYS * (l + 1) + key));
    let end = rom[at..].iter().position(|&b| b == 0).map_or(rom.len(), |n| at + n);
    rom[at..end].iter().map(|&b| b as char).collect() // 8-bit, Latin-1 as far as seen
}

/// A car from the car table at `0x7F0BD8` (15 × 0x58 bytes).
#[derive(Debug, Clone)]
pub struct Car {
    pub name: String,
    /// First vehicle material (atlas); the car's paint variants follow it.
    pub first_material: usize,
    pub paint_variants: usize,
    /// Model-bank indices for high, medium and low detail.
    pub models: [usize; 3],
}

pub fn cars(rom: &[u8]) -> Vec<Car> {
    const TABLE: usize = 0x7F_0BD8;
    let rec = |i: usize| TABLE + 0x58 * i;
    let n = (0..).take_while(|&i| u32_at(rom, rec(i) + 4) as usize == i).count();
    let materials = ptr(rom, LEVEL_TABLE + 0x20);
    let size = |m: usize| u32_at(rom, materials + 0x24 * m + 0x0C); // width and height together
    (0..n)
        .map(|i| {
            let first = u16_at(rom, rec(i) + 0x0C) as usize;
            let next = if i + 1 < n {
                u16_at(rom, rec(i + 1) + 0x0C) as usize
            } else {
                vehicle_material_count(rom)
            };
            // The last car's variants end where the atlas size changes (small 40×40 textures follow).
            let variants = (first..next).take_while(|&m| size(m) == size(first)).count();
            let mid = u16_at(rom, rec(i) + 0x14) as usize;
            Car {
                name: text(rom, u32_at(rom, rec(i)) as usize, Some(0)),
                first_material: first,
                paint_variants: variants,
                models: [mid - 1, mid, u16_at(rom, rec(i) + 0x10) as usize],
            }
        })
        .collect()
}

fn vehicle_material_count(rom: &[u8]) -> usize {
    let materials = ptr(rom, LEVEL_TABLE + 0x20);
    (0..)
        .take_while(|&i| u16_at(rom, materials + 0x24 * i) as usize == i)
        .count()
}

/// Vehicle materials (level record `+0x20`, same layout as city materials). Texels are BIOS-LZ77 blobs at
/// level record `+0x0C` + material `+0x08`, 5-bit indices into a car palette (`paint_palettes`).
pub fn vehicle_textures(rom: &[u8]) -> Vec<Texture> {
    let (materials, base) = (ptr(rom, LEVEL_TABLE + 0x20), ptr(rom, LEVEL_TABLE + 0x0C));
    (0..vehicle_material_count(rom))
        .map(|i| {
            let m = materials + 0x24 * i;
            let (width, height) = (u16_at(rom, m + 0x0C) as usize, u16_at(rom, m + 0x0E) as usize);
            let at = base + u32_at(rom, m + 8) as usize;
            // Most are LZ77 with size w*h + 8; the 36 128×100 materials are not (format unknown, read raw).
            let mut pixels = if rom[at] == 0x10 && (u32_at(rom, at) >> 8) as usize == width * height + 8 {
                lz77(rom, at)
            } else {
                rom[at..at + width * height].to_vec()
            };
            pixels.resize(width * height, 0);
            Texture { width, height, pixels }
        })
        .collect()
}

/// The 20 paint presets at `0x7E6EEC` (0x80 apart), as 32-colour RGBA palettes indexed by atlas pixel value.
/// The game loads a car's atlas pixel `i` as colour `i ^ 16`: 0..15 are the body (paint ramp, preset entries
/// 16..31), 16..31 glass, lights and trim (entries 0..15). In a race the paint ramp is generated at runtime
/// from the chosen colour, so these presets are only a stand-in (hypothesis: paint-shop presets).
pub fn paint_palettes(rom: &[u8]) -> Vec<Vec<[u8; 4]>> {
    (0..20)
        .map(|k| {
            (0..32)
                .map(|i| bgr555(u16_at(rom, 0x7E_6EEC + 0x80 * k + 2 * (i ^ 16))))
                .collect()
        })
        .collect()
}

/// A race route from the route table at `0x7F2798` (0x14-byte records, read by `FUN_08139454`):
/// `+0x00` four template entities (0xA4 bytes each; `+0x0C/+0x10/+0x14` = 8.8 position) and `+0x08` the
/// racing line: 24-byte waypoints `(x, z, ?, -1, cumulative distance, sector)`.
#[derive(Debug, Clone)]
pub struct Route {
    /// Start grid, city units: the player first, then three opponents.
    pub grid: Vec<[i32; 3]>,
    pub waypoints: Vec<Waypoint>,
}

#[derive(Debug, Clone, Copy)]
pub struct Waypoint {
    pub x: i32,
    pub z: i32,
    /// Distance along the route from the first waypoint.
    pub distance: i32,
    pub sector: usize,
}

pub fn routes(rom: &[u8]) -> Vec<Route> {
    const TABLE: usize = 0x7F_2798;
    let sectors = city(rom).len();
    // Records end where `+0x10` stops being zero (the level descriptors follow).
    (0..)
        .map(|i| TABLE + 0x14 * i)
        .take_while(|&r| u32_at(rom, r + 0x10) == 0)
        .map(|r| {
            let entities = ptr(rom, r);
            let grid = (0..4)
                .map(|e| [0, 4, 8].map(|k| u32_at(rom, entities + 0xA4 * e + 0x0C + k) as i32 >> 8))
                .collect();
            let mut waypoints: Vec<Waypoint> = Vec::new();
            if u32_at(rom, r + 8) != 0 {
                let line = ptr(rom, r + 8);
                for w in (0..0x1800 / 24).map(|k| line + 24 * k) {
                    let wp = Waypoint {
                        x: u32_at(rom, w) as i32,
                        z: u32_at(rom, w + 4) as i32,
                        distance: u32_at(rom, w + 16) as i32,
                        sector: u32_at(rom, w + 20) as usize,
                    };
                    let ordered = waypoints.last().is_none_or(|p| wp.distance >= p.distance);
                    if u32_at(rom, w + 12) != u32::MAX || !ordered || wp.sector >= sectors {
                        break;
                    }
                    waypoints.push(wp);
                }
            }
            Route { grid, waypoints }
        })
        .collect()
}

/// A car palette for atlas pixels 0..31: the body ramp generated from `paint` (0..31 per channel, BGR555 scale)
/// and the trim (glass, lights) from paint preset `trim_preset`.
///
/// The game builds the body ramp at runtime from the chosen colour. This reconstruction reproduces the ramp in
/// the reference race's RAM (red Cobalt): pixel 0 dark grey, 1..8 highlights blending from ~52% towards white
/// down to ~16%, 9 the paint itself, 10..15 fading to black. The generator itself is not decoded.
pub fn car_palette(rom: &[u8], paint: [u8; 3], trim_preset: usize) -> Vec<[u8; 4]> {
    let shade = |c: [f32; 3]| {
        let [r, g, b] = c.map(|v| (v.clamp(0.0, 31.0).round() as u16).min(31));
        bgr555(r | g << 5 | b << 10)
    };
    let base = paint.map(f32::from);
    let body = (0..16).map(|i| match i {
        0 => shade([4.0; 3]),
        1..=8 => {
            let t = 0.52 - (i - 1) as f32 * 0.052;
            shade(base.map(|c| c + (31.0 - c) * t))
        }
        _ => shade(base.map(|c| c * (15 - i) as f32 / 6.0)),
    });
    let trim = paint_palettes(rom)[trim_preset][16..].to_vec();
    body.chain(trim).collect()
}

/// One edge of a sector: from this wall's point to the next wall's point (the last wraps to the first).
#[derive(Debug, Clone)]
pub struct Wall {
    pub x: i32,
    pub z: i32,
    /// Heights at the start and end of the edge; `-y` is up.
    pub top: [i16; 2],
    pub bottom: [i16; 2],
    /// Neighbouring sector for a portal, `-1` for a solid wall.
    pub link: i16,
    pub material: u16,
    /// Floor texture coordinates at this corner; 16,384 = one texture width/height (hypothesis: one 512×512
    /// roundabout texture spans exactly one road width).
    pub floor_uv: [i32; 2],
    /// Wall texture: u start in texels (`+0x28`), u span in 1/256 textures (`+0x40`), v at the top and bottom
    /// of both ends (`+0x10`/`+0x18` start, `+0x14`/`+0x1C` end; 16,384 = one texture), flags (`+0x2E`).
    pub tex_u: u16,
    pub tex_span: u16,
    pub tex_v: [i32; 4],
    pub flags: u16,
    /// Light at this corner, red/green/blue (`+0x3C..+0x3E`, BGR555 channel scale); see `light_factor`.
    pub light: [u8; 3],
}

impl Wall {
    /// Normalised (u, v) for the start-top, end-top, end-bottom and start-bottom corners of a solid wall,
    /// following `FUN_030013ac` / `FUN_03000304`: u in texels is `u >> 7` with `u0 = +0x28 << 7`,
    /// `u1 = u0 + (+0x40 << (log2 width - 1))`; flag bit 1 runs u the other way.
    pub fn uv(&self, texture_width: usize) -> [[f32; 2]; 4] {
        let w = texture_width as f32;
        let (mut u0, mut u1) = (
            self.tex_u as f32 / w,
            self.tex_u as f32 / w + self.tex_span as f32 / 256.0,
        );
        if self.flags & 2 != 0 {
            std::mem::swap(&mut u0, &mut u1);
        }
        let v = self.tex_v.map(|v| v as f32 / 16384.0);
        [[u0, v[0]], [u1, v[1]], [u1, v[3]], [u0, v[2]]]
    }
}

#[derive(Debug, Clone)]
pub struct Sector {
    pub first: usize,
    /// Floor and ceiling materials (indices into `city_textures`); 0 = not drawn (`FUN_0300224c` skips the pass).
    pub floor: u16,
    pub ceiling: u16,
    pub walls: Vec<Wall>,
}

/// A level environment: one of the level descriptors at `0x7F2B08` (0x68 bytes each; all share the city and
/// the vehicle bank). The race init `FUN_08138a9c` loads palette `+0x00 + (+0x5A) * 2` and the descriptor names
/// the sky: gradient material `+0x5E`, skyline material `+0x60`.
#[derive(Debug, Clone)]
pub struct Environment {
    /// Index into the 14 city palettes (`city_palette`).
    pub palette: usize,
    pub sky: Sky,
}

/// A sky: 64 BGR555 gradient colours (top to bottom, drawn per scanline) and a 240×64 skyline panorama
/// (8bpp through the city palette, colour 0 = sky).
#[derive(Debug, Clone)]
pub struct Sky {
    pub gradient: Vec<[u8; 4]>,
    pub skyline: Texture,
}

/// The 12 environments; the descriptors end where `+0x00` stops pointing at the shared palette block
/// (a variant with another palette block follows at `0x7F2FE8`).
pub fn environments(rom: &[u8]) -> Vec<Environment> {
    let (materials, texels) = (ptr(rom, LEVEL_TABLE + 0x1C), ptr(rom, LEVEL_TABLE + 0x08));
    let textures = city_textures(rom);
    let first = u32_at(rom, LEVEL_TABLE);
    (0..)
        .map(|i| LEVEL_TABLE + 0x68 * i)
        .take_while(|&r| u32_at(rom, r) == first)
        .map(|r| {
            let (gradient, skyline) = (u16_at(rom, r + 0x5E) as usize, u16_at(rom, r + 0x60) as usize);
            let at = texels + u32_at(rom, materials + 0x24 * gradient + 8) as usize;
            Environment {
                palette: u16_at(rom, r + 0x5A) as usize * 2 / 0x200,
                sky: Sky {
                    gradient: (0..64).map(|k| bgr555(u16_at(rom, at + 2 * k))).collect(),
                    skyline: textures[skyline].clone(),
                },
            }
        })
        .collect()
}

/// Per-channel palette multiplier for a wall light (`Wall::light`), as `FUN_0813a514` applies it to the whole
/// palette at the player's position: `light * 256 * 2/3 + 0x400` in 4.12 fixed point, capped at 0xFFF.
pub fn light_factor(light: [u8; 3]) -> [f32; 3] {
    light.map(|b| ((b as f32 * 512.0 / 3.0 + 1024.0) / 4096.0).min(4095.0 / 4096.0))
}

fn bgr555(c: u16) -> [u8; 4] {
    let channel = |shift: u16| (((c >> shift) & 31) * 255 / 31) as u8;
    [channel(0), channel(5), channel(10), 255]
}

/// A city texture: 8bpp palette indices, row-major.
#[derive(Debug, Clone)]
pub struct Texture {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}

/// City palette `index` of the 14 at level record `+0x00` (0x200 bytes apart) as RGBA8; index 0 (magenta
/// `0x7C1F`) is transparent. Environments pick one (`Environment::palette`).
pub fn city_palette(rom: &[u8], index: usize) -> Vec<[u8; 4]> {
    let at = ptr(rom, LEVEL_TABLE) + 0x200 * index;
    (0..256)
        .map(|i| {
            let [r, g, b, _] = bgr555(u16_at(rom, at + 2 * i));
            [r, g, b, if i == 0 { 0 } else { 255 }]
        })
        .collect()
}

/// City materials (level record `+0x1C`, 0x24 bytes, self-indexed) and their texels (record `+0x08` + offset),
/// returned row-major. Materials with `+0x02 == 2` (building walls) are column-mapped: one map byte per u at
/// `+0x04` picks a unique column, stored column-major (`height` texels each) at `+0x08`.
pub fn city_textures(rom: &[u8]) -> Vec<Texture> {
    let (materials, texels) = (ptr(rom, LEVEL_TABLE + 0x1C), ptr(rom, LEVEL_TABLE + 0x08));
    (0..)
        .map(|i| (i, materials + 0x24 * i))
        .take_while(|&(i, m)| u16_at(rom, m) as usize == i)
        .map(|(_, m)| {
            let (width, height) = (u16_at(rom, m + 0x0C) as usize, u16_at(rom, m + 0x0E) as usize);
            let at = texels + u32_at(rom, m + 8) as usize;
            let pixels = if u16_at(rom, m + 2) == 2 {
                let map = texels + u32_at(rom, m + 4) as usize;
                (0..width * height)
                    .map(|i| rom[at + rom[map + i % width] as usize * height + i / width])
                    .collect()
            } else {
                rom[at..at + width * height].to_vec()
            };
            Texture { width, height, pixels }
        })
        .collect()
}

/// The city: 2.5D portal/sector world (`docs/formats/city-sectors.md`).
pub fn city(rom: &[u8]) -> Vec<Sector> {
    let walls_at = ptr(rom, LEVEL_TABLE + 0x14);
    let sectors_at = ptr(rom, LEVEL_TABLE + 0x18);
    (0..(walls_at - sectors_at) / 0x30)
        .map(|s| {
            let o = sectors_at + 0x30 * s;
            let first = u16_at(rom, o) as usize;
            let walls = (first..first + u16_at(rom, o + 2) as usize)
                .map(|k| {
                    let w = walls_at + 0x44 * k;
                    Wall {
                        x: u32_at(rom, w) as i32,
                        z: u32_at(rom, w + 4) as i32,
                        top: [i16_at(rom, w + 8), i16_at(rom, w + 12)],
                        bottom: [i16_at(rom, w + 10), i16_at(rom, w + 14)],
                        link: i16_at(rom, w + 0x30),
                        material: u16_at(rom, w + 0x2C),
                        floor_uv: [u32_at(rom, w + 0x20) as i32, u32_at(rom, w + 0x24) as i32],
                        tex_u: u16_at(rom, w + 0x28),
                        tex_span: u16_at(rom, w + 0x40),
                        tex_v: [0x10, 0x14, 0x18, 0x1C].map(|k| u32_at(rom, w + k) as i32),
                        flags: u16_at(rom, w + 0x2E),
                        light: [rom[w + 0x3C], rom[w + 0x3D], rom[w + 0x3E]],
                    }
                })
                .collect();
            Sector {
                first,
                floor: u16_at(rom, o + 8),
                ceiling: u16_at(rom, o + 4),
                walls,
            }
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct Polygon {
    /// 3 or 4 indices into `Model::verts`, and as many into `Model::uvs`.
    pub verts: Vec<u16>,
    pub uvs: Vec<u16>,
}

#[derive(Debug, Clone)]
pub struct Model {
    /// Bit 0: textured.
    pub flags: u16,
    /// Raw int16 units; `-y` is up.
    pub verts: Vec<[i16; 3]>,
    pub polys: Vec<Polygon>,
    pub uvs: Vec<[u16; 2]>,
}

/// The vehicle model bank: 102 models (`docs/formats/vehicle-models.md`).
pub fn models(rom: &[u8]) -> Vec<Model> {
    let t = LEVEL_TABLE;
    let (models_at, verts_at, idx_at, uvidx_at) = (
        ptr(rom, t + 0x34),
        ptr(rom, t + 0x38),
        ptr(rom, t + 0x3C),
        ptr(rom, t + 0x40),
    );
    let (uvs_at, sizes_at) = (ptr(rom, t + 0x48), ptr(rom, t + 0x54));
    (0..(verts_at - models_at) / 40)
        .map(|i| {
            let o = models_at + 40 * i;
            let field = |k: usize| u32_at(rom, o + 4 * k) as usize;
            let (vstart, mut istart, uvstart, mut uvistart, sstart) =
                (field(0), field(1), field(2), field(6), field(7));
            let (flags, npoly, nvert, nuv) = (
                u16_at(rom, o + 0x20),
                u16_at(rom, o + 0x22) as usize,
                u16_at(rom, o + 0x24) as usize,
                u16_at(rom, o + 0x26) as usize,
            );
            let verts = (0..nvert)
                .map(|k| {
                    let v = verts_at + 6 * (vstart + k);
                    [i16_at(rom, v), i16_at(rom, v + 2), i16_at(rom, v + 4)]
                })
                .collect();
            let polys = (0..npoly)
                .map(|k| {
                    let n = rom[sizes_at + sstart + k] as usize;
                    let read = |at: usize, start: usize| (0..n).map(|j| u16_at(rom, at + 2 * (start + j))).collect();
                    let p = Polygon {
                        verts: read(idx_at, istart),
                        uvs: read(uvidx_at, uvistart),
                    };
                    istart += n;
                    uvistart += n;
                    p
                })
                .collect();
            let uvs = (0..nuv)
                .map(|k| {
                    [
                        u16_at(rom, uvs_at + 4 * (uvstart + k)),
                        u16_at(rom, uvs_at + 4 * (uvstart + k) + 2),
                    ]
                })
                .collect();
            Model {
                flags,
                verts,
                polys,
                uvs,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests against the user's ROM; skipped (with a note) when no vault exists.
    fn rom() -> Option<Vec<u8>> {
        std::env::set_current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")).unwrap();
        canonical_rom()
            .map_err(|e| eprintln!("skipping: no ROM vault ({e})"))
            .ok()
    }

    #[test]
    fn city_sectors_tile_walls_and_portals_match() {
        let Some(rom) = rom() else { return };
        let sectors = city(&rom);
        assert_eq!(sectors.len(), 1113);
        for pair in sectors.windows(2) {
            assert_eq!(pair[0].first + pair[0].walls.len(), pair[1].first);
        }
        let edges = |s: &Sector| -> Vec<((i32, i32), (i32, i32))> {
            let w = &s.walls;
            (0..w.len())
                .map(|k| ((w[k].x, w[k].z), (w[(k + 1) % w.len()].x, w[(k + 1) % w.len()].z)))
                .collect()
        };
        let (mut portals, mut matched) = (0, 0);
        for s in &sectors {
            for (w, (p, q)) in s.walls.iter().zip(edges(s)) {
                if w.link >= 0 {
                    portals += 1;
                    matched += edges(&sectors[w.link as usize]).contains(&(q, p)) as usize;
                }
            }
        }
        assert!(matched * 100 > portals * 99, "{matched}/{portals} portals match");
    }

    #[test]
    fn city_materials_cover_every_wall_and_floor() {
        let Some(rom) = rom() else { return };
        let textures = city_textures(&rom);
        assert_eq!(textures.len(), 227); // fills the table up to the next array (0x722DD4)
        assert!(
            textures
                .iter()
                .all(|t| t.width > 0 && t.height > 0 && t.pixels.len() == t.width * t.height)
        );
        for s in city(&rom) {
            assert!((s.floor as usize) < textures.len());
            assert!(s.walls.iter().all(|w| (w.material as usize) < textures.len()));
        }
        assert_eq!(city_palette(&rom, 0)[0][3], 0);
        let environments = environments(&rom);
        assert_eq!(environments.len(), 12);
        assert_eq!(environments[1].palette, 3); // the reference race's base palette (checked against its RAM)
        assert!(environments.iter().all(|e| e.palette < 14
            && e.sky.gradient.len() == 64
            && (e.sky.skyline.width, e.sky.skyline.height) == (240, 64)));
    }

    #[test]
    fn car_table_names_models_and_atlases() {
        let Some(rom) = rom() else { return };
        let cars = cars(&rom);
        assert_eq!(cars.len(), 15);
        assert_eq!(cars[2].name, "Chevy Cobalt SS"); // the player's car in the reference race
        assert_eq!((cars[2].first_material, cars[2].models), (8, [8, 9, 10]));
        assert_eq!(cars.iter().map(|c| c.paint_variants).sum::<usize>(), 45);
        let textures = vehicle_textures(&rom);
        for c in &cars {
            for m in c.first_material..c.first_material + c.paint_variants {
                let t = &textures[m];
                assert_eq!((t.width, t.height), (256, 200), "{}", c.name);
                assert!(
                    t.pixels.iter().all(|&p| p < 32),
                    "{}: atlas uses more than 32 colours",
                    c.name
                );
            }
        }
    }

    #[test]
    fn routes_match_the_reference_race() {
        let Some(rom) = rom() else { return };
        let routes = routes(&rom);
        assert_eq!(routes.len(), 44);
        // Quick Play race in the reference run: route 23, player start (118400, 0, -64320) city units.
        let r = &routes[23];
        assert_eq!(r.grid[0], [118_400, 0, -64_320]);
        assert_eq!(
            (
                r.waypoints.len(),
                r.waypoints[0].sector,
                r.waypoints.last().unwrap().distance
            ),
            (19, 760, 58_231)
        );
        let sectors = city(&rom).len();
        assert!(routes.iter().flat_map(|r| &r.waypoints).all(|w| w.sector < sectors));
    }

    #[test]
    fn models_index_in_range() {
        let Some(rom) = rom() else { return };
        let models = models(&rom);
        assert_eq!(models.len(), 102);
        for m in &models {
            for p in &m.polys {
                assert!(matches!(p.verts.len(), 3 | 4));
                assert!(p.verts.iter().all(|&k| (k as usize) < m.verts.len()));
                if m.flags & 1 != 0 {
                    assert!(p.uvs.iter().all(|&k| (k as usize) < m.uvs.len()));
                }
            }
        }
    }
}
