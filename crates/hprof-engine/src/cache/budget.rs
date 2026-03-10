//! Thread-safe atomic memory counter for tracking heap usage
//! of parsed hprof data.

use std::sync::atomic::{AtomicUsize, Ordering};

/// Tracks total memory consumed by parsed data using an
/// atomic counter. Thread-safe for concurrent access.
///
/// Uses `Ordering::Relaxed` — approximate tracking is
/// sufficient for memory budget decisions.
pub struct MemoryCounter {
    bytes: AtomicUsize,
}

impl MemoryCounter {
    /// Creates a new counter initialized to zero.
    pub fn new() -> Self {
        Self {
            bytes: AtomicUsize::new(0),
        }
    }

    /// Atomically increments the counter by `bytes`.
    pub fn add(&self, bytes: usize) {
        self.bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Atomically decrements the counter by `bytes`.
    pub fn subtract(&self, bytes: usize) {
        self.bytes.fetch_sub(bytes, Ordering::Relaxed);
    }

    /// Returns the current value of the counter.
    pub fn current(&self) -> usize {
        self.bytes.load(Ordering::Relaxed)
    }

    /// Resets the counter to zero. For testing only.
    pub fn reset(&self) {
        self.bytes.store(0, Ordering::Relaxed);
    }
}

impl Default for MemoryCounter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn new_counter_is_zero() {
        let c = MemoryCounter::new();
        assert_eq!(c.current(), 0);
    }

    #[test]
    fn add_increments() {
        let c = MemoryCounter::new();
        c.add(100);
        assert_eq!(c.current(), 100);
        c.add(50);
        assert_eq!(c.current(), 150);
    }

    #[test]
    fn subtract_decrements() {
        let c = MemoryCounter::new();
        c.add(200);
        c.subtract(75);
        assert_eq!(c.current(), 125);
    }

    #[test]
    fn reset_sets_to_zero() {
        let c = MemoryCounter::new();
        c.add(999);
        c.reset();
        assert_eq!(c.current(), 0);
    }

    #[test]
    fn concurrent_add_subtract_is_consistent() {
        let counter = Arc::new(MemoryCounter::new());
        let n = 1000;
        let increment = 10;

        let handles: Vec<_> = (0..4)
            .map(|i| {
                let c = Arc::clone(&counter);
                thread::spawn(move || {
                    for _ in 0..n {
                        if i % 2 == 0 {
                            c.add(increment);
                        } else {
                            c.subtract(increment);
                        }
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        // 2 threads add, 2 subtract: net = 0
        assert_eq!(counter.current(), 0);
    }

    #[test]
    fn default_is_zero() {
        let c = MemoryCounter::default();
        assert_eq!(c.current(), 0);
    }
}
