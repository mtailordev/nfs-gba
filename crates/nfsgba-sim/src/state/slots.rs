//! The state of the matrix slots and the effect sprites (`nfsgba-game/src/slots.rs`, `oam.rs`): the globals they
//! read, the sprite pool and a car's record. Names follow `docs/engine/game-loop.md` step 6 and `address-map.md`.

use super::Entity;
use crate::layout;
use crate::layout::Ptr;

layout! {
    /// The IWRAM globals of the slot builder and the effect sprites.
    pub struct SlotGlobals: 0 {
        /// Timer 3's count at the frame's start (the frame time the sparks integrate).
        0x0300_5934 timer3: i32,
        /// Matrix slots handed out this frame (`FUN_0814f8ac`).
        0x0300_5394 slot_counter: u32,
        /// The effect lights are on (`FUN_0814e414` returns without).
        0x0300_53E8 lights: u32,
        /// The car records (0x11 bytes per car).
        0x0300_539C records: Ptr<CarRecord>,
        /// Traffic is on; the eight traffic entities (0 none).
        0x0300_6090 traffic_on: u32,
        0x0300_6270 traffic: [Ptr<Entity>; 8],
        /// The AI cars (racers after the player).
        0x0300_57EC ai_cars: i32,
        /// Non-zero: the body matrix comes from the driver's physics orientation.
        0x0300_610C physics_orientation: i32,
        /// Non-zero: the sparks of a contact come from the first AI car.
        0x0300_618C sparks_from_ai: u32,
        /// The `rand_table` position.
        0x0300_64C8 rand: u32,
        /// Per racer: the rim buffer (unpacked pixels) and the car's atlas, in the heap.
        0x0300_6094 rim_pixels: [u32; 4],
        0x0300_6164 atlases: [u32; 4],
    }

    /// The effect-sprite pool header (`FUN_08161f38`'s list): objects, the first OAM entry, the count.
    pub struct SpritePool: 0 {
        0x0300_0058 objects: Ptr<Sprite>,
        0x0300_005C first: i16,
        0x0300_005E count: i16,
    }

    /// An effect sprite (0x14 bytes): screen position, in use, frame, scale, rotation, size class, kind, palette.
    pub struct Sprite: 0x14 {
        0x00 x: i16,
        0x02 y: i16,
        0x04 used: i16,
        0x08 frame: i16,
        0x0A scale_x: i16,
        0x0C scale_y: i16,
        0x0E angle: i16,
        0x10 size: i16,
        0x12 kind: u8,
        0x13 palette: u8,
    }

    /// A car's record (0x11 bytes at `records + 0x11 * car`): the spoiler and exhaust table rows, the rim, the paint.
    pub struct CarRecord: 0x11 {
        0x00 spoiler: u8,
        0x02 rim: u8,
        0x03 exhaust: u8,
        0x05 paint: u8,
    }

    /// Who races: the car and paint of each racer, the route and the environment.
    pub struct RaceSetup: 0 {
        /// The base palette buffer (256 colours).
        0x0300_55F0 palette: Ptr<u16>,
        0x0300_611C cars: [i8; 4],
        0x0300_5FEC paints: [i8; 4],
        0x0300_5720 route: u32,
        0x0300_006C env: u32,
    }

    /// A city material's run-time entry (world `+0x24`, 0x24 bytes): the size the rim redraw reads.
    pub struct MaterialInfo: 0x24 {
        0x0C width: u16,
        0x0E height: u16,
    }
}
