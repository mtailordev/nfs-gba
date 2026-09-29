//! The sound engine of Need for Speed Carbon: Own the City (GBA, `BN7E`): Logik State's LS_Play ("GBAModPlay
//! 3.0") module player, its sound effects and its software mixer, rewritten from the game's code.
//!
//! Everything here is exact against the reference build unless a comment names a FIDELITY entry (A1–A6); the format, the code
//! addresses and how it was checked are in `docs/formats/audio.md`. Addresses are GBA addresses: the engine
//! keeps sample and pattern pointers as the game does, and reads them through [`Rom`].

pub mod engine;
pub mod format;
pub mod mixer;
pub mod ram;
#[cfg(test)]
mod tests;

pub use engine::Engine;

/// The cartridge ROM seen at `0x0800_0000`. Reads outside it return 0 (the game never makes any).
#[derive(Clone, Copy)]
pub struct Rom<'a>(pub &'a [u8]);

impl<'a> Rom<'a> {
    pub const BASE: u32 = 0x0800_0000;

    pub fn u8(self, addr: u32) -> u8 {
        let o = addr.wrapping_sub(Self::BASE) as usize;
        self.0.get(o).copied().unwrap_or(0)
    }
    pub fn i8(self, addr: u32) -> i8 {
        self.u8(addr) as i8
    }
    pub fn u16(self, addr: u32) -> u16 {
        u16::from_le_bytes([self.u8(addr), self.u8(addr.wrapping_add(1))])
    }
    pub fn u32(self, addr: u32) -> u32 {
        u32::from_le_bytes([0, 1, 2, 3].map(|k| self.u8(addr.wrapping_add(k))))
    }
    /// `len` bytes from `addr`, clipped to the ROM.
    pub fn bytes(self, addr: u32, len: u32) -> &'a [u8] {
        let o = (addr.wrapping_sub(Self::BASE) as usize).min(self.0.len());
        &self.0[o..(o + len as usize).min(self.0.len())]
    }
}
