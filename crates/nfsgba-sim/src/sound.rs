//! Sound commands issued by the car step (for the local player only). The audio engine is a separate
//! subsystem; the simulation records what the game asks it to do instead of running it.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// `FUN_08135fdc(effect, 1)`: start effect `effect` (1..=39, table 0x087EE24C picks the channel).
    Play(u32),
    /// `FUN_08136028(effect)`: stop the effect's channel.
    Stop(u32),
    /// `FUN_081360b4(effect, pitch)`: set the effect's pitch.
    Pitch(u32, i32),
    /// `FUN_08152e40(sample, pitch, channel, volume)`: start a sample on a channel directly.
    Start {
        sample: u32,
        pitch: i32,
        channel: u32,
        volume: u32,
    },
}
