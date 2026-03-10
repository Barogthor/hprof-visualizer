//! Thread-safe atomic memory counter for tracking heap usage
//! of parsed hprof data.

use std::sync::atomic::{AtomicUsize, Ordering};

/// Tracks total memory consumed by parsed data using an
/// atomic counter. Thread-safe for concurrent access.
///
/// Uses `Ordering::Relaxed` — approximate tracking is
/// sufficient for memory budget decisions.
///
/// The budget is immutable after construction — set once
/// via `MemoryCounter::new(budget)`.
pub struct MemoryCounter {
    bytes: AtomicUsize,
    budget: u64,
}

impl MemoryCounter {
    /// Creates a new counter with the given budget.
    ///
    /// The budget is immutable after construction. Use
    /// `u64::MAX` for "unlimited" in tests.
    pub fn new(budget: u64) -> Self {
        Self {
            bytes: AtomicUsize::new(0),
            budget,
        }
    }

    /// Returns the memory budget in bytes.
    pub fn budget(&self) -> u64 {
        self.budget
    }

    /// Returns the ratio of current usage to budget.
    ///
    /// Casts both `current()` (`usize`) and `budget`
    /// (`u64`) to `f64` explicitly.
    pub fn usage_ratio(&self) -> f64 {
        self.current() as f64 / self.budget as f64
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

    /// Resets the counter to zero. Budget is NOT reset.
    pub fn reset(&self) {
        self.bytes.store(0, Ordering::Relaxed);
    }
}

impl Default for MemoryCounter {
    fn default() -> Self {
        Self::new(u64::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn new_counter_is_zero() {
        let c = MemoryCounter::new(u64::MAX);
        assert_eq!(c.current(), 0);
    }

    #[test]
    fn add_increments() {
        let c = MemoryCounter::new(u64::MAX);
        c.add(100);
        assert_eq!(c.current(), 100);
        c.add(50);
        assert_eq!(c.current(), 150);
    }

    #[test]
    fn subtract_decrements() {
        let c = MemoryCounter::new(u64::MAX);
        c.add(200);
        c.subtract(75);
        assert_eq!(c.current(), 125);
    }

    #[test]
    fn reset_sets_to_zero_but_keeps_budget() {
        let c = MemoryCounter::new(1_000_000);
        c.add(999);
        c.reset();
        assert_eq!(c.current(), 0);
        assert_eq!(c.budget(), 1_000_000);
    }

    #[test]
    fn concurrent_add_subtract_is_consistent() {
        let counter = Arc::new(MemoryCounter::new(u64::MAX));
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
    fn default_is_zero_with_max_budget() {
        let c = MemoryCounter::default();
        assert_eq!(c.current(), 0);
        assert_eq!(c.budget(), u64::MAX);
    }

    #[test]
    fn budget_getter_returns_construction_value() {
        let c = MemoryCounter::new(8_000_000_000);
        assert_eq!(c.budget(), 8_000_000_000);
    }

    #[test]
    fn usage_ratio_zero_when_empty() {
        let c = MemoryCounter::new(1_000_000);
        assert_eq!(c.usage_ratio(), 0.0);
    }

    #[test]
    fn usage_ratio_half_at_midpoint() {
        let c = MemoryCounter::new(1_000_000);
        c.add(500_000);
        let ratio = c.usage_ratio();
        assert!((ratio - 0.5).abs() < 1e-9, "expected 0.5, got {ratio}");
    }

    #[test]
    fn usage_ratio_full() {
        let c = MemoryCounter::new(1_000_000);
        c.add(1_000_000);
        let ratio = c.usage_ratio();
        assert!((ratio - 1.0).abs() < 1e-9, "expected 1.0, got {ratio}");
    }
}
