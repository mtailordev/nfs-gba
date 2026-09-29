//! The data the player reads: GBAMOD30 modules, the sample bank and the sound-effect table
//! (`docs/formats/audio.md`). The player itself walks the raw bytes the way the game does; these readers
//! are for listing, extracting and testing.

use crate::Rom;

/// The five music modules (`u32` pointers), read by the game's `play_music` (0x08136054).
pub const MODULE_TABLE: u32 = 0x087E_E238;
pub const MODULE_COUNT: usize = 5;
/// The sample bank: 256 headers of 16 bytes, then the samples back to back (signed 8-bit PCM).
pub const SAMPLE_BANK: u32 = 0x0805_0FD4;
/// The sound-effect table: a `u32` count, that many 24-byte entries, then the sample data.
pub const SFX_TABLE: u32 = 0x0800_0210;
/// Carbon's sound id -> sound-effect channel slot, one byte per id (40 ids).
pub const SFX_SLOTS: u32 = 0x087E_E24C;

pub const MAGIC: &[u8; 8] = b"GBAMOD30";
/// Note value meaning "no note" in the pattern data.
pub const NO_NOTE: u16 = 0x1FF;

/// A GBAMOD30 module header (0x650 bytes; the patterns follow).
#[derive(Clone, Debug, PartialEq)]
pub struct Module {
    pub addr: u32,
    /// `+0x28`: music channels (the rest of the eight, at most `max_sfx`, go to sound effects).
    pub channels: u32,
    /// `+0x34`: order-list length the player wraps at (255 in every Carbon module: songs loop with `B00`).
    pub order_count: u32,
    /// `+0x13C`: rows per pattern.
    pub rows: u32,
    /// `+0x140`: nonzero selects the linear (note table) pitch mode; Carbon's modules all use periods.
    pub linear: u32,
    /// `+0x144`, `+0x148`: initial tempo (BPM) and speed (ticks per row).
    pub tempo: u32,
    pub speed: u32,
    /// `+0x14C`: the order list (pattern numbers), 256 bytes.
    pub orders: Vec<u8>,
}

impl Module {
    /// Reads the header at `addr`; `None` if the magic is missing (the player refuses those too).
    pub fn parse(rom: Rom, addr: u32) -> Option<Module> {
        if rom.bytes(addr, 8) != MAGIC {
            return None;
        }
        let w = |o: u32| rom.u32(addr + o);
        Some(Module {
            addr,
            channels: w(0x28),
            order_count: w(0x34),
            rows: w(0x13C),
            linear: w(0x140),
            tempo: w(0x144),
            speed: w(0x148),
            orders: rom.bytes(addr + 0x14C, 256).to_vec(),
        })
    }

    /// Start of the pattern data (`+0x650`); pattern offsets at `+0x24C` are relative to it.
    pub fn patterns(&self) -> u32 {
        self.addr + 0x650
    }

    pub fn pattern_addr(&self, rom: Rom, pattern: u8) -> u32 {
        self.patterns() + rom.u32(self.addr + 0x24C + 4 * pattern as u32)
    }

    /// Decodes a whole pattern into `rows × channels` cells, walking the three run-length streams of each
    /// channel exactly as the player does (`FUN_0815248c`).
    pub fn pattern(&self, rom: Rom, pattern: u8) -> Vec<Vec<Cell>> {
        let mut block = self.pattern_addr(rom, pattern);
        let mut out = vec![Vec::new(); self.rows as usize];
        for _ in 0..self.channels {
            let (first, b, c, size) = (
                block + 0x14,
                rom.u32(block + 4),
                rom.u32(block + 8),
                rom.u32(block + 0x10),
            );
            let mut s = [Stream::default(), Stream::default(), Stream::default()];
            for (r, row) in out.iter_mut().enumerate() {
                let start = |at: u32| (r == 0).then_some(at);
                let (a, v, e) = (
                    s[0].read(rom, 2, start(first)),
                    s[1].read(rom, 1, start(first + b)),
                    s[2].read(rom, 2, start(first + c)),
                );
                row.push(Cell {
                    note: ((a >> 7) & 0x1FF) as u16,
                    instrument: (a & 0x7F) as u8,
                    volume: v as u8,
                    effect: (e >> 8) as u8,
                    param: e as u8,
                });
            }
            block = first + size;
        }
        out
    }
}

/// One channel of one row.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cell {
    /// Note number, [`NO_NOTE`] for none.
    pub note: u16,
    /// Sample bank index + 1, 0 for none.
    pub instrument: u8,
    /// Volume column: volume + 1, 0 for none.
    pub volume: u8,
    pub effect: u8,
    pub param: u8,
}

/// A run-length stream of `(count, value)` records with 1- or 2-byte big-endian values: the 12-byte decoder
/// state the player keeps per stream (`+0` next record, `+4` rows left, `+8` value).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stream {
    pub ptr: u32,
    pub count: i32,
    pub value: u32,
}

impl Stream {
    /// The value for the next row (`FUN_08151d8c` for 2-byte values, `FUN_08151d60` for 1-byte ones).
    /// `start` restarts the stream at a record address. A record's value lasts `count` rows; a record read
    /// with a count below 1 is replaced by the following one (only once).
    pub fn read(&mut self, rom: Rom, width: u32, start: Option<u32>) -> u32 {
        let record = |p: u32| {
            (
                rom.u8(p) as i32,
                (1..=width).fold(0, |v, k| v << 8 | rom.u8(p + k) as u32),
                p + 1 + width,
            )
        };
        let (mut count, mut value, mut ptr) = match start {
            Some(p) => record(p),
            None => (self.count, self.value, self.ptr),
        };
        if count < 1 {
            (count, value, ptr) = record(ptr);
        }
        *self = Stream {
            ptr,
            count: count - 1,
            value,
        };
        value
    }
}

/// A sample bank header (`+0x00` of each 16-byte record).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    /// Where the data starts: the bank's data follows the 0x1000-byte header block, in header order.
    pub addr: u32,
    /// `+0x0`: length in bytes (0 = empty slot).
    pub len: u32,
    /// `+0x4`: fine tune (halved into the period).
    pub finetune: i16,
    /// `+0x6`: default volume (0..64).
    pub volume: u16,
    /// `+0x8`, `+0xA`: loop start and length in bytes (length 0 = no loop).
    pub loop_start: u16,
    pub loop_len: u16,
    /// `+0xC`: relative note.
    pub rel_note: i16,
}

/// The 256 slots of a sample bank, as `FUN_08151ec4` lays them out.
pub fn sample_bank(rom: Rom, bank: u32) -> Vec<Option<Sample>> {
    let mut data = bank + 0x1000;
    (0..256)
        .map(|i| {
            let h = bank + 16 * i;
            let len = rom.u32(h);
            (len != 0).then(|| {
                let s = Sample {
                    addr: data,
                    len,
                    finetune: rom.u16(h + 4) as i16,
                    volume: rom.u16(h + 6),
                    loop_start: rom.u16(h + 8),
                    loop_len: rom.u16(h + 10),
                    rel_note: rom.u16(h + 12) as i16,
                };
                data += len;
                s
            })
        })
        .collect()
}

/// A sound-effect entry (24 bytes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sfx {
    /// `+0x00`: offset into the data after the table, turned into an address here.
    pub addr: u32,
    /// `+0x04`: length in bytes.
    pub len: u32,
    /// `+0x0C`, `+0x0E`: loop start and length in bytes.
    pub loop_start: u16,
    pub loop_len: u16,
    /// `+0x10`: default playback rate in Hz.
    pub rate: u32,
    /// `+0x14`: a flag that selects another sample format in the mode-0 mixer; 0 in every Carbon entry.
    pub flag: u8,
}

pub fn sfx_table(rom: Rom, table: u32) -> Vec<Sfx> {
    let count = rom.u32(table);
    let data = table + 4 + 24 * count;
    (0..count)
        .map(|i| {
            let e = table + 4 + 24 * i;
            Sfx {
                addr: data + rom.u32(e),
                len: rom.u32(e + 4),
                loop_start: rom.u16(e + 12),
                loop_len: rom.u16(e + 14),
                rate: rom.u32(e + 16),
                flag: rom.u8(e + 20),
            }
        })
        .collect()
}
