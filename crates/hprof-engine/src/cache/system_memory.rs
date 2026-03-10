//! Cross-platform system memory detection for automatic
//! budget calculation.

/// Detects total physical memory in bytes using `sysinfo`.
///
/// Returns `total_memory() / 2` as the auto-calculated
/// budget. Falls back to 512 MB if `total_memory()`
/// returns 0 (containers, broken cgroups).
pub fn auto_budget() -> u64 {
    let total = detect_total_memory();
    if total == 0 {
        eprintln!(
            "[warn] total_memory() returned 0 — \
             falling back to 512 MB budget"
        );
        return 512 * 1024 * 1024;
    }
    total / 2
}

/// Returns total physical memory in bytes.
///
/// Uses `sysinfo::System` with `refresh_memory()`.
/// Returns 0 on platforms where detection fails.
fn detect_total_memory() -> u64 {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_memory();
    sys.total_memory()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_total_memory_returns_positive() {
        let total = detect_total_memory();
        assert!(total > 0, "total_memory() should be > 0 on dev machines");
    }

    #[test]
    fn auto_budget_returns_half_of_total() {
        let budget = auto_budget();
        let total = detect_total_memory();
        if total > 0 {
            assert_eq!(budget, total / 2);
        } else {
            assert_eq!(budget, 512 * 1024 * 1024);
        }
    }

    #[test]
    fn auto_budget_is_reasonable() {
        let budget = auto_budget();
        // At least 256 MB on any dev machine
        assert!(budget >= 256 * 1024 * 1024, "budget {budget} too low");
        // Under 1 TB
        assert!(
            budget < 1024 * 1024 * 1024 * 1024,
            "budget {budget} unreasonably high"
        );
    }
}
