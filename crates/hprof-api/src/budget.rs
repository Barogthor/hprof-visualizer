//! Memory budget type for controlling heap extraction
//! chunk sizes.

/// Memory budget passed to the parser to control
/// per-segment pre-allocation during heap extraction.
///
/// - [`MemoryBudget::Unlimited`] — no chunking, the
///   full segment is extracted in one pass (default for
///   standalone callers without a memory budget).
/// - [`MemoryBudget::Bytes(n)`] — chunk extraction so
///   that per-thread allocation stays within
///   `n / num_threads` bytes (floored at 64 MB).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum MemoryBudget {
    /// No chunking — extract each segment in one pass.
    #[default]
    Unlimited,
    /// Budget in bytes — drives chunked extraction.
    Bytes(u64),
}

impl MemoryBudget {
    /// Returns the byte value if set, or `None` for
    /// [`Unlimited`](MemoryBudget::Unlimited).
    pub fn bytes(&self) -> Option<u64> {
        match self {
            Self::Unlimited => None,
            Self::Bytes(b) => Some(*b),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_unlimited() {
        assert_eq!(MemoryBudget::default(), MemoryBudget::Unlimited);
    }

    #[test]
    fn unlimited_bytes_is_none() {
        assert_eq!(MemoryBudget::Unlimited.bytes(), None);
    }

    #[test]
    fn bytes_variant_returns_value() {
        let b = MemoryBudget::Bytes(1024);
        assert_eq!(b.bytes(), Some(1024));
    }

    #[test]
    fn bytes_zero_returns_some_zero() {
        assert_eq!(MemoryBudget::Bytes(0).bytes(), Some(0));
    }

    #[test]
    fn copy_and_clone() {
        let a = MemoryBudget::Bytes(42);
        let b = a;
        assert_eq!(a, b);
    }
}
