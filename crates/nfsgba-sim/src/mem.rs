//! The game's data address space: cartridge ROM, EWRAM and IWRAM.
//!
//! The simulation keeps the game's own memory layout (entity array, physics structs, world struct, globals) so
//! that every step can be compared byte for byte with the reference build. Accesses are little-endian and
//! aligned the way the game does them; ROM is read-only.

pub const ROM: u32 = 0x0800_0000;
pub const EWRAM: u32 = 0x0200_0000;
pub const IWRAM: u32 = 0x0300_0000;

#[derive(Clone)]
pub struct Mem {
    pub rom: Vec<u8>,
    pub ewram: Vec<u8>,
    pub iwram: Vec<u8>,
}

impl Mem {
    /// ROM plus RAM snapshots (256 KiB EWRAM, 32 KiB IWRAM), e.g. from `mgba_remote.lua`'s `dump`.
    pub fn new(rom: Vec<u8>, ewram: Vec<u8>, iwram: Vec<u8>) -> Self {
        assert_eq!((ewram.len(), iwram.len()), (0x4_0000, 0x8000), "EWRAM and IWRAM sizes");
        Mem { rom, ewram, iwram }
    }

    fn slot(&self, addr: u32) -> (&[u8], usize) {
        match addr >> 24 {
            0x02 => (&self.ewram, (addr & 0x3_FFFF) as usize),
            0x03 => (&self.iwram, (addr & 0x7FFF) as usize),
            0x08 | 0x09 => (&self.rom, (addr - ROM) as usize),
            _ => panic!("read from unmapped address {addr:#010x}"),
        }
    }

    fn slot_mut(&mut self, addr: u32) -> (&mut [u8], usize) {
        match addr >> 24 {
            0x02 => (&mut self.ewram, (addr & 0x3_FFFF) as usize),
            0x03 => (&mut self.iwram, (addr & 0x7FFF) as usize),
            _ => panic!("write to non-RAM address {addr:#010x}"),
        }
    }

    pub fn u8(&self, addr: u32) -> u8 {
        let (m, o) = self.slot(addr);
        m[o]
    }
    pub fn i8(&self, addr: u32) -> i8 {
        self.u8(addr) as i8
    }
    pub fn u16(&self, addr: u32) -> u16 {
        let (m, o) = self.slot(addr);
        u16::from_le_bytes([m[o], m[o + 1]])
    }
    pub fn i16(&self, addr: u32) -> i16 {
        self.u16(addr) as i16
    }
    pub fn u32(&self, addr: u32) -> u32 {
        let (m, o) = self.slot(addr);
        u32::from_le_bytes([m[o], m[o + 1], m[o + 2], m[o + 3]])
    }
    pub fn i32(&self, addr: u32) -> i32 {
        self.u32(addr) as i32
    }

    pub fn set_u8(&mut self, addr: u32, v: u8) {
        let (m, o) = self.slot_mut(addr);
        m[o] = v;
    }
    pub fn set_u16(&mut self, addr: u32, v: u16) {
        let (m, o) = self.slot_mut(addr);
        m[o..o + 2].copy_from_slice(&v.to_le_bytes());
    }
    pub fn set_i16(&mut self, addr: u32, v: i16) {
        self.set_u16(addr, v as u16);
    }
    pub fn set_u32(&mut self, addr: u32, v: u32) {
        let (m, o) = self.slot_mut(addr);
        m[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }
    pub fn set_i32(&mut self, addr: u32, v: i32) {
        self.set_u32(addr, v as u32);
    }

    /// Three consecutive words (a vector).
    pub fn vec3(&self, addr: u32) -> [i32; 3] {
        [self.i32(addr), self.i32(addr + 4), self.i32(addr + 8)]
    }
    pub fn set_vec3(&mut self, addr: u32, v: [i32; 3]) {
        for (k, c) in v.into_iter().enumerate() {
            self.set_i32(addr + 4 * k as u32, c);
        }
    }

    pub fn bytes(&self, addr: u32, len: usize) -> &[u8] {
        let (m, o) = self.slot(addr);
        &m[o..o + len]
    }
    pub fn set_bytes(&mut self, addr: u32, data: &[u8]) {
        let (m, o) = self.slot_mut(addr);
        m[o..o + data.len()].copy_from_slice(data);
    }
}
