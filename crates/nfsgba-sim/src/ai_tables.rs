//! The ROM tables of the opponents' AI, the wingman and the traffic cars, parsed once (`GameData::ai`).

/// BN7E offsets (ROM file offsets; the game's addresses are these plus `0x0800_0000`).
mod bn7e {
    pub const UPGRADES: usize = 0x7F_42B4;
    pub const WINGMAN_GAP: usize = 0x7F_4284;
    pub const LANE_LOOKAHEAD: usize = 0x7F_5A50;
    pub const BOOST_TIME: usize = 0x7F_3DE8;
    pub const BOOST_SHIFT: usize = 0x7F_3DF4;
    pub const TRAFFIC_LANES: usize = 0x7F_5488;
    pub const TRAFFIC_TYPES: usize = 0x7F_546C;
    pub const KNOCK_SHIFT: usize = 0x7F_5604;
    pub const RACER_SHIFT: usize = 0x7F_5684;
    pub const POINT_COUNT: usize = 0x7F_5704;
    pub const HIT_RADIUS: usize = 0x7F_5784;
    pub const POINTS: usize = 0x7F_5804;
    pub const ENDS_RACE: usize = 0x7F_5924;
    /// Table sizes: traffic types (the per-type tables), models.
    pub const TYPES: usize = 32;
    pub const MODELS: usize = 7;
}

#[derive(Debug, Clone)]
pub struct AiTables {
    /// Upgrade level of a computer car that is not a racer (the wingman), by wingman.
    pub upgrades: Vec<i32>,
    /// Wingman settings per wingman (`FUN_081410c0`).
    pub wingman_gap: Vec<i32>,
    /// Look-ahead distances by lane when the steering term is large.
    pub lane_lookahead: [i32; 5],
    /// Boost timer reloads and shifts per start value.
    pub boost_time: Vec<i32>,
    pub boost_shift: Vec<u32>,
    pub traffic: TrafficTables,
}

/// Traffic cars, per traffic type (entity `+0x7C`) unless noted.
#[derive(Debug, Clone)]
pub struct TrafficTables {
    pub lanes: [i32; 4],
    /// Per model: the model and the paint.
    pub models: Vec<(u16, u16)>,
    /// Collision point count, the points' offsets along the car (two), the hit radius.
    pub point_count: Vec<i32>,
    pub points: Vec<[i32; 2]>,
    pub hit_radius: Vec<i32>,
    /// How a hit's impulse is shared between the traffic car and the racer (shifts).
    pub knock_shift: Vec<u32>,
    pub racer_shift: Vec<u32>,
    /// A hit ends the race for the player (phase 7).
    pub ends_race: Vec<i16>,
}

impl AiTables {
    pub fn parse(rom: &[u8]) -> AiTables {
        use bn7e::*;
        let word = |o: usize| u32::from_le_bytes(rom[o..o + 4].try_into().unwrap());
        let half = |o: usize| i16::from_le_bytes(rom[o..o + 2].try_into().unwrap());
        let words = |o: usize, n: usize| (0..n).map(|k| word(o + 4 * k)).collect::<Vec<_>>();
        let signed = |o: usize, n: usize| words(o, n).into_iter().map(|v| v as i32).collect::<Vec<_>>();
        AiTables {
            upgrades: signed(UPGRADES, 12),
            wingman_gap: signed(WINGMAN_GAP, 12),
            lane_lookahead: std::array::from_fn(|k| word(LANE_LOOKAHEAD + 4 * k) as i32),
            boost_time: signed(BOOST_TIME, 3),
            boost_shift: words(BOOST_SHIFT, 3),
            traffic: TrafficTables {
                lanes: std::array::from_fn(|k| word(TRAFFIC_LANES + 4 * k) as i32),
                models: (0..MODELS)
                    .map(|k| {
                        (
                            half(TRAFFIC_TYPES + 4 * k) as u16,
                            half(TRAFFIC_TYPES + 4 * k + 2) as u16,
                        )
                    })
                    .collect(),
                point_count: signed(POINT_COUNT, TYPES),
                points: (0..TYPES)
                    .map(|k| [word(POINTS + 8 * k) as i32, word(POINTS + 8 * k + 4) as i32])
                    .collect(),
                hit_radius: signed(HIT_RADIUS, TYPES),
                knock_shift: words(KNOCK_SHIFT, TYPES),
                racer_shift: words(RACER_SHIFT, TYPES),
                ends_race: (0..TYPES).map(|k| half(ENDS_RACE + 2 * k)).collect(),
            },
        }
    }
}
