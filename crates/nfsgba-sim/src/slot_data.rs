//! The ROM tables the matrix slots and the effect sprites read, parsed once (`GameData::effects`). Offsets are
//! BN7E file offsets (the GBA address is `0x08000000` more), each with the game's table it is.

/// The car table (0x58 bytes per car, `0x087F0BD8`; 15 cars up to the handling records).
const CAR_TABLE: usize = 0x7F_0BD8;
pub const CARS: usize = 15;
/// The exhaust flame points (`0x087F3E00`, i16, 6 per side, per car and per exhaust row).
const EXHAUST: usize = 0x7F_3E00;
/// The spoiler mount (`0x087F0816`, i16 pairs, 0x40 bytes per car).
const SPOILER: usize = 0x7F_0816;
/// The rim redraw entries (`0x087EF816`, 0x10 bytes, 15 per car).
const RIMS: usize = 0x7E_F816;
/// The effect materials (`0x0836CF5C`, 0x24 bytes each): the palette byte at `+0x20`, the frame count at `+0x10`.
const MATERIALS: usize = 0x36_CF5C;
/// The last one any effect uses is the traffic lights' + the small-size step (`0x273C` and `0x24` further).
const MATERIAL_COUNT: usize = 0x273C / 0x24 + 2;

/// The material of the lights' flare (`0x0836F698`: the traffic lights' and the flame frame's) and of the billboard
/// (`0x0836D010`), the exhaust (`0x0836D304`), as indices from the first effect material.
pub const FLARE: usize = 0x273C / 0x24;
pub const BILLBOARD: usize = 0xB4 / 0x24;
pub const EXHAUST_MATERIAL: usize = 0x1A;

#[derive(Debug, Clone)]
pub struct EffectTables {
    /// Per car: the body's light points (`+0x26..` sparks, `+0x2C..` rear lights, `+0x32..` brake lights).
    pub cars: Vec<[i16; 0x2C]>,
    pub exhaust: Vec<i16>,
    /// Per car and spoiler row: the mount (y, z).
    pub spoiler: Vec<[i16; 2]>,
    /// Per car and rim row: first x, first y, material, (unused), second x, second y (halves 0..6).
    pub rims: Vec<[i16; 8]>,
    /// Per effect material: the palette byte and the frame count.
    pub palette: Vec<u8>,
    pub frames: Vec<u16>,
}

impl EffectTables {
    pub fn parse(rom: &[u8]) -> EffectTables {
        let half = |o: usize| i16::from_le_bytes([rom[o], rom[o + 1]]);
        EffectTables {
            cars: (0..CARS)
                .map(|c| std::array::from_fn(|k| half(CAR_TABLE + 0x58 * c + 2 * k)))
                .collect(),
            exhaust: (0..CARS * 24 + 16).map(|k| half(EXHAUST + 2 * k)).collect(),
            spoiler: (0..CARS * 16 + 4)
                .map(|k| [half(SPOILER + 4 * k), half(SPOILER + 4 * k + 2)])
                .collect(),
            rims: (0..CARS * 15)
                .map(|k| std::array::from_fn(|j| half(RIMS + 0x10 * k + 2 * j)))
                .collect(),
            palette: (0..MATERIAL_COUNT).map(|k| rom[MATERIALS + 0x24 * k + 0x20]).collect(),
            frames: (0..MATERIAL_COUNT)
                .map(|k| half(MATERIALS + 0x24 * k + 0x10) as u16)
                .collect(),
        }
    }
}
