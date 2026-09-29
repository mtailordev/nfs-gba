//! Need for Speed Carbon: Own the City (GBA) game simulation, ported exactly from the ROM.
//!
//! The port keeps the game's memory layout (entity array, 0x4FC-byte car physics structs, the world struct and
//! the globals), so every step can be compared byte for byte with the reference build. The step reads the
//! ROM and the race's RAM state ([`mem::Mem`]); `docs/engine/physics.md` documents the fields and algorithms.
//! The migration to typed state ([`state`], [`layout`], [`data::GameData`]) is under way:
//! `docs/engine/typed-state.md`.

// Fixed-point expressions are written the way the game's C writes them (`a * b >> 12`); Rust gives `*` and `>>`
// the same precedence as C, so the parentheses clippy asks for would only add noise.
#![allow(clippy::precedence)]

pub mod ai;
pub mod body;
pub mod car;
pub mod contact;
pub mod data;
pub mod heap;
pub mod init;
pub mod layout;
pub mod math;
pub mod mem;
pub mod route;
pub mod sound;
pub mod state;
pub mod traffic;
pub mod traffic_ai;
pub mod walls;
pub mod world;

pub use mem::Mem;

/// The game state the simulation runs on, plus the sound commands the step issued (the audio engine is a
/// separate subsystem; the commands are recorded, not executed).
#[derive(Clone)]
pub struct Sim {
    pub mem: Mem,
    pub sounds: Vec<sound::Command>,
}

impl Sim {
    pub fn new(mem: Mem) -> Self {
        Sim {
            mem,
            sounds: Vec::new(),
        }
    }
}

/// A code path of the game that this port does not have yet. The step stops rather than guess (1:1 rule).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unported(pub &'static str);

impl std::fmt::Display for Unported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unported game code: {}", self.0)
    }
}

impl std::error::Error for Unported {}

pub type Result<T> = std::result::Result<T, Unported>;
