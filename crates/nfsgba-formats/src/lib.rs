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
        .and_then(|s| s.lines().find_map(|l| l.strip_prefix("NFSGBA_DATA=").map(|v| v.trim().trim_matches('"').to_owned())))
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
}

#[derive(Debug, Clone)]
pub struct Sector {
    pub first: usize,
    /// Floor material (index into `city_textures`).
    pub floor: u16,
    pub walls: Vec<Wall>,
}

/// A city texture: 8bpp palette indices, row-major.
#[derive(Debug, Clone)]
pub struct Texture {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}

/// City palette (level record `+0x00`) as RGBA8; index 0 (magenta `0x7C1F`) is transparent.
pub fn city_palette(rom: &[u8]) -> Vec<[u8; 4]> {
    let at = ptr(rom, LEVEL_TABLE);
    (0..256)
        .map(|i| {
            let c = u16_at(rom, at + 2 * i);
            let channel = |shift: u16| (((c >> shift) & 31) * 255 / 31) as u8;
            [channel(0), channel(5), channel(10), if i == 0 { 0 } else { 255 }]
        })
        .collect()
}

/// City materials (level record `+0x1C`, 0x24 bytes, self-indexed) and their texels (record `+0x08` + offset).
pub fn city_textures(rom: &[u8]) -> Vec<Texture> {
    let (materials, texels) = (ptr(rom, LEVEL_TABLE + 0x1C), ptr(rom, LEVEL_TABLE + 0x08));
    (0..)
        .map(|i| (i, materials + 0x24 * i))
        .take_while(|&(i, m)| u16_at(rom, m) as usize == i)
        .map(|(_, m)| {
            let (width, height) = (u16_at(rom, m + 0x0C) as usize, u16_at(rom, m + 0x0E) as usize);
            let at = texels + u32_at(rom, m + 8) as usize;
            Texture { width, height, pixels: rom[at..at + width * height].to_vec() }
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
                    }
                })
                .collect();
            Sector { first, floor: u16_at(rom, o + 8), walls }
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
    let (models_at, verts_at, idx_at, uvidx_at) = (ptr(rom, t + 0x34), ptr(rom, t + 0x38), ptr(rom, t + 0x3C), ptr(rom, t + 0x40));
    let (uvs_at, sizes_at) = (ptr(rom, t + 0x48), ptr(rom, t + 0x54));
    (0..(verts_at - models_at) / 40)
        .map(|i| {
            let o = models_at + 40 * i;
            let field = |k: usize| u32_at(rom, o + 4 * k) as usize;
            let (vstart, mut istart, uvstart, mut uvistart, sstart) = (field(0), field(1), field(2), field(6), field(7));
            let (flags, npoly, nvert, nuv) =
                (u16_at(rom, o + 0x20), u16_at(rom, o + 0x22) as usize, u16_at(rom, o + 0x24) as usize, u16_at(rom, o + 0x26) as usize);
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
                    let p = Polygon { verts: read(idx_at, istart), uvs: read(uvidx_at, uvistart) };
                    istart += n;
                    uvistart += n;
                    p
                })
                .collect();
            let uvs = (0..nuv).map(|k| [u16_at(rom, uvs_at + 4 * (uvstart + k)), u16_at(rom, uvs_at + 4 * (uvstart + k) + 2)]).collect();
            Model { flags, verts, polys, uvs }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests against the user's ROM; skipped (with a note) when no vault exists.
    fn rom() -> Option<Vec<u8>> {
        std::env::set_current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")).unwrap();
        canonical_rom().map_err(|e| eprintln!("skipping: no ROM vault ({e})")).ok()
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
            (0..w.len()).map(|k| ((w[k].x, w[k].z), (w[(k + 1) % w.len()].x, w[(k + 1) % w.len()].z))).collect()
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
        assert!(textures.iter().all(|t| t.width > 0 && t.height > 0 && t.pixels.len() == t.width * t.height));
        for s in city(&rom) {
            assert!((s.floor as usize) < textures.len());
            assert!(s.walls.iter().all(|w| (w.material as usize) < textures.len()));
        }
        assert_eq!(city_palette(&rom)[0][3], 0);
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
