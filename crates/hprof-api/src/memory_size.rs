//! Trait and helpers for estimating heap memory footprint
//! of parsed hprof structures.
//!
//! [`MemorySize`] is the single cross-cutting trait that all
//! parsed domain types implement, enabling the memory budget
//! subsystem (Story 5.3+) to track and limit heap usage.

use std::mem::size_of;

/// Reports the estimated heap memory footprint of a value.
///
/// Implementations must account for:
/// - `std::mem::size_of::<Self>()` (static/stack portion)
/// - Heap allocations: `Vec` capacity, `String` capacity,
///   `HashMap` buckets, and nested `MemorySize` types
pub trait MemorySize {
    /// Returns the estimated total bytes consumed by this
    /// value, including both the static struct size and any
    /// heap allocations.
    fn memory_size(&self) -> usize;
}

/// Estimates the memory used by an `FxHashMap<K, V>`.
///
/// Uses `capacity * (size_of::<K>() + size_of::<V>() + 8)`
/// where the 8 bytes account for hashbrown control bytes.
///
/// - `capacity`: the map's `.capacity()` (allocated slots,
///   not `.len()`)
/// - Returns: estimated bytes for buckets only (does NOT
///   include heap allocations inside values — caller must
///   add those separately)
pub fn fxhashmap_memory_size<K, V>(capacity: usize) -> usize {
    capacity * (size_of::<K>() + size_of::<V>() + 8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trait_can_be_implemented_and_called() {
        struct Dummy;
        impl MemorySize for Dummy {
            fn memory_size(&self) -> usize {
                size_of::<Self>()
            }
        }
        let d = Dummy;
        assert_eq!(d.memory_size(), size_of::<Dummy>());
    }

    #[test]
    fn fxhashmap_helper_zero_capacity_returns_zero() {
        assert_eq!(fxhashmap_memory_size::<u64, u64>(0), 0);
    }

    #[test]
    fn fxhashmap_helper_includes_control_bytes() {
        let cap = 16;
        let expected = cap * (size_of::<u32>() + size_of::<u64>() + 8);
        assert_eq!(fxhashmap_memory_size::<u32, u64>(cap), expected);
    }

    #[test]
    fn fxhashmap_helper_u64_u64_calculation() {
        let cap = 100;
        // 8 + 8 + 8 = 24 bytes per slot
        assert_eq!(fxhashmap_memory_size::<u64, u64>(cap), 100 * 24);
    }
}
