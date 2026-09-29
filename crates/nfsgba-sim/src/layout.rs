//! The one way to give typed game state its place in the GBA RAM image: [`layout!`] declares a struct with the
//! offset of each field and gives it [`Field::load`] / [`Field::store`] (`docs/engine/typed-state.md`).
//!
//! Typed code never reads RAM; the adapters at a subsystem's call boundary load its state, run it, and store it
//! back. The replay tests prove that the typed state and its layout carry everything the game keeps.

use std::marker::PhantomData;

use crate::mem::Mem;

/// A value with a fixed size and place in the game's memory.
pub trait Field: Sized {
    const SIZE: u32;
    fn load(m: &Mem, at: u32) -> Self;
    fn store(&self, m: &mut Mem, at: u32);
}

/// A struct declared with [`layout!`]: its fields as (name, offset, size).
pub trait Layout: Field {
    const FIELDS: &'static [(&'static str, u32, u32)];
}

macro_rules! scalar {
    ($($t:ty),*) => {$(
        impl Field for $t {
            const SIZE: u32 = size_of::<$t>() as u32;
            fn load(m: &Mem, at: u32) -> Self {
                <$t>::from_le_bytes(m.bytes(at, size_of::<$t>()).try_into().unwrap())
            }
            fn store(&self, m: &mut Mem, at: u32) {
                m.set_bytes(at, &self.to_le_bytes());
            }
        }
    )*};
}
scalar!(u8, i8, u16, i16, u32, i32);

impl<T: Field, const N: usize> Field for [T; N] {
    const SIZE: u32 = T::SIZE * N as u32;
    fn load(m: &Mem, at: u32) -> Self {
        std::array::from_fn(|k| T::load(m, at + T::SIZE * k as u32))
    }
    fn store(&self, m: &mut Mem, at: u32) {
        for (k, v) in self.iter().enumerate() {
            v.store(m, at + T::SIZE * k as u32);
        }
    }
}

/// A pointer field: the GBA address of a `T` (0 = none). Typed code does not follow it: the adapter resolves it
/// to an index ([`Ptr::index_from`]) or a loaded value ([`Ptr::read`]), and the address round-trips unchanged.
pub struct Ptr<T> {
    pub addr: u32,
    to: PhantomData<fn() -> T>,
}

impl<T> Ptr<T> {
    pub const NULL: Self = Ptr::new(0);

    pub const fn new(addr: u32) -> Self {
        Ptr { addr, to: PhantomData }
    }

    pub fn is_null(self) -> bool {
        self.addr == 0
    }
}

impl<T: Field> Ptr<T> {
    pub fn read(self, m: &Mem) -> T {
        T::load(m, self.addr)
    }

    pub fn write(self, m: &mut Mem, v: &T) {
        v.store(m, self.addr);
    }

    /// The first `n` elements of the array that starts here.
    pub fn read_n(self, m: &Mem, n: u32) -> Vec<T> {
        (0..n).map(|k| self.at(k).read(m)).collect()
    }

    /// Element `index` of the array that starts here.
    pub fn at(self, index: u32) -> Self {
        Ptr::new(self.addr + T::SIZE * index)
    }

    /// Which element of the array at `base` this points to.
    pub fn index_from(self, base: Self) -> u32 {
        (self.addr - base.addr) / T::SIZE
    }
}

impl<T> Clone for Ptr<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Ptr<T> {}
impl<T> PartialEq for Ptr<T> {
    fn eq(&self, o: &Self) -> bool {
        self.addr == o.addr
    }
}
impl<T> Eq for Ptr<T> {}
impl<T> Default for Ptr<T> {
    fn default() -> Self {
        Self::NULL
    }
}
impl<T> std::fmt::Debug for Ptr<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Ptr({:#010x})", self.addr)
    }
}

impl<T> Field for Ptr<T> {
    const SIZE: u32 = 4;
    fn load(m: &Mem, at: u32) -> Self {
        Ptr::new(u32::load(m, at))
    }
    fn store(&self, m: &mut Mem, at: u32) {
        self.addr.store(m, at);
    }
}

/// Declares typed state with the offset of each field (a literal, after the field's doc comment):
///
/// ```ignore
/// layout! {
///     /// A 0x20-byte struct: `Thing::load(m, base)`, `thing.store(m, base)`.
///     pub struct Thing: 0x20 {
///         0x00 index: u16,
///         0x04 pos: [i32; 3],
///     }
///     /// Globals: size 0, offsets are absolute addresses (`Globals::load(m, 0)`).
///     pub struct Globals: 0 {
///         0x0300_0048 phase: u32,
///     }
///     /// A struct declared elsewhere (every field listed, all public).
///     impl nfsgba_formats::render::Piece: 0x20 { 0x00 dx: i16, ... }
/// }
/// ```
#[macro_export]
macro_rules! layout {
    () => {};
    ($(#[$meta:meta])* $vis:vis struct $name:ident: $size:literal {
        $($(#[$fmeta:meta])* $off:literal $field:ident: $ty:ty),* $(,)?
    } $($rest:tt)*) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Default, PartialEq, Eq)]
        $vis struct $name { $($(#[$fmeta])* pub $field: $ty,)* }
        $crate::layout!(impl $name: $size { $($off $field: $ty),* } $($rest)*);
    };
    ($(#[$imeta:meta])* impl $name:path: $size:literal { $($off:literal $field:ident: $ty:ty),* $(,)? } $($rest:tt)*) => {
        $(#[$imeta])*
        impl $crate::layout::Field for $name {
            const SIZE: u32 = $size;
            fn load(m: &$crate::Mem, at: u32) -> Self {
                Self { $($field: <$ty as $crate::layout::Field>::load(m, at + $off),)* }
            }
            fn store(&self, m: &mut $crate::Mem, at: u32) {
                $(<$ty as $crate::layout::Field>::store(&self.$field, m, at + $off);)*
            }
        }
        impl $crate::layout::Layout for $name {
            const FIELDS: &'static [(&'static str, u32, u32)] =
                &[$((stringify!($field), $off, <$ty as $crate::layout::Field>::SIZE)),*];
        }
        $crate::layout!($($rest)*);
    };
}

/// Panics if two fields of `T` overlap or one ends past `T::SIZE` (size 0: globals, no end).
pub fn assert_disjoint<T: Layout>(name: &str) {
    let mut f = T::FIELDS.to_vec();
    f.sort_by_key(|&(_, o, _)| o);
    for w in f.windows(2) {
        let ((a, ao, an), (b, bo, _)) = (w[0], w[1]);
        assert!(
            ao + an <= bo,
            "{name}: {a} ({ao:#x}, {an} bytes) overlaps {b} ({bo:#x})"
        );
    }
    if let (Some(&(last, o, n)), true) = (f.last(), T::SIZE != 0) {
        assert!(o + n <= T::SIZE, "{name}: {last} ends past the struct ({:#x})", T::SIZE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    crate::layout! {
        struct Probe: 0x10 {
            0x00 a: u16,
            0x02 b: i8,
            0x04 v: [i32; 2],
            0x0C p: Ptr<Probe>,
        }
        struct At: 0 {
            0x0300_0020 x: i16,
        }
    }

    #[test]
    fn load_store_round_trip() {
        let mut iwram = vec![0; 0x8000];
        for (k, b) in iwram.iter_mut().enumerate() {
            *b = k as u8 ^ 0x5A;
        }
        let m = Mem::new(Vec::new(), vec![0; 0x4_0000], iwram);
        let p = Probe::load(&m, 0x0300_0100);
        assert_eq!(p.a, m.u16(0x0300_0100));
        assert_eq!(p.b, m.i8(0x0300_0102));
        assert_eq!(p.v, [m.i32(0x0300_0104), m.i32(0x0300_0108)]);
        assert_eq!(p.p.addr, m.u32(0x0300_010C));
        assert_eq!(At::load(&m, 0).x, m.i16(0x0300_0020));
        let mut c = Mem::new(Vec::new(), vec![0; 0x4_0000], vec![0; 0x8000]);
        p.store(&mut c, 0x0300_0100);
        assert_eq!(c.bytes(0x0300_0100, 3), m.bytes(0x0300_0100, 3));
        assert_eq!(c.bytes(0x0300_0104, 12), m.bytes(0x0300_0104, 12));
        assert_eq!(c.u8(0x0300_0103), 0, "undeclared bytes stay untouched");
        let base = Ptr::<Probe>::new(0x0300_0100);
        assert_eq!(base.at(3).index_from(base), 3);
        assert_disjoint::<Probe>("Probe");
        assert_disjoint::<At>("At");
    }

    #[test]
    #[should_panic(expected = "overlaps")]
    fn overlap_is_caught() {
        crate::layout! {
            struct Bad: 8 {
                0 a: u32,
                2 b: u16,
            }
        }
        assert_disjoint::<Bad>("Bad");
    }
}
