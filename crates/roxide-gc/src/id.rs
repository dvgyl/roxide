//! Unified [`SexpId`] handle whose bit pattern doubles as the M4 C `SEXP` value.

use std::num::NonZeroU64;

/// Maximum generation number before a heap slot is permanently retired (`2^28 - 1`).
pub const MAX_GENERATION: u32 = (1u32 << 28) - 1;

/// Handle to a heap-allocated R object.
///
/// Bit layout (ADR-0002):
/// - bits `[63..36]`: 28-bit generation (`0` indicates a permanent singleton slot)
/// - bits `[35..4]`: 32-bit `slot_index + 1` (never zero)
/// - bits `[3..0]`: zero (16-byte aligned opaque bit pattern for Milestone 4 `SEXP`)
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
#[repr(transparent)]
pub struct SexpId(NonZeroU64);

impl SexpId {
    /// Construct a handle from a slot index and generation.
    ///
    /// Returns `None` if `index == u32::MAX` or `generation > MAX_GENERATION`.
    #[must_use]
    pub const fn new(index: u32, generation: u32) -> Option<Self> {
        if index == u32::MAX || generation > MAX_GENERATION {
            return None;
        }
        let encoded_index = (index as u64 + 1) << 4;
        let encoded_gen = (generation as u64) << 36;
        match NonZeroU64::new(encoded_gen | encoded_index) {
            Some(nz) => Some(Self(nz)),
            None => None,
        }
    }

    /// Construct a permanent (generation 0) singleton handle for a fixed compile-time index.
    ///
    /// # Panics
    ///
    /// Panics at compile time (or debug init) only if `index == u32::MAX`.
    #[must_use]
    pub const fn permanent(index: u32) -> Self {
        match Self::new(index, 0) {
            Some(id) => id,
            None => panic!("permanent SexpId index out of range"),
        }
    }

    /// Return the 0-based heap slot index.
    #[must_use]
    pub const fn index(self) -> u32 {
        (((self.0.get() >> 4) & 0xFFFF_FFFF) - 1) as u32
    }

    /// Return the 28-bit slot generation (`0` for permanent singletons).
    #[must_use]
    pub const fn generation(self) -> u32 {
        (self.0.get() >> 36) as u32
    }

    /// Return true if this handle refers to a permanent generation-0 slot.
    #[must_use]
    pub const fn is_permanent(self) -> bool {
        self.generation() == 0
    }

    /// Raw 64-bit representation (identical to the M4 opaque C `SEXP` pointer bits).
    #[must_use]
    pub const fn to_bits(self) -> u64 {
        self.0.get()
    }

    /// Decode a raw 64-bit pattern, checking structural bit invariants only.
    ///
    /// Liveness and generation matching are still checked by [`crate::Heap::get`].
    #[must_use]
    pub const fn from_bits(bits: u64) -> Option<Self> {
        if bits == 0 || (bits & 0xF) != 0 {
            return None;
        }
        let idx_plus_one = (bits >> 4) & 0xFFFF_FFFF;
        if idx_plus_one == 0 {
            return None;
        }
        match NonZeroU64::new(bits) {
            Some(nz) => Some(Self(nz)),
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_sexp_id_bits() {
        let p = SexpId::permanent(0);
        assert_eq!(p.index(), 0);
        assert_eq!(p.generation(), 0);
        assert!(p.is_permanent());
        assert_eq!(p.to_bits() & 0xF, 0);
        assert_eq!(SexpId::from_bits(p.to_bits()), Some(p));

        let dyn_id = SexpId::new(42, 7).unwrap();
        assert_eq!(dyn_id.index(), 42);
        assert_eq!(dyn_id.generation(), 7);
        assert!(!dyn_id.is_permanent());
        assert_eq!(SexpId::from_bits(dyn_id.to_bits()), Some(dyn_id));

        assert_eq!(SexpId::from_bits(0), None);
        assert_eq!(SexpId::from_bits(1), None);
        assert_eq!(SexpId::from_bits(1u64 << 36), None);
        assert_eq!(std::mem::size_of::<Option<SexpId>>(), 8);
    }
}
