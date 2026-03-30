//! Low-level hprof binary parsing primitives and
//! cross-cutting utilities.

use std::time::{Duration, Instant};

/// Minimum bytes between consecutive progress callbacks.
pub(super) const PROGRESS_REPORT_INTERVAL: usize = 4 * 1024 * 1024;

/// Maximum time between consecutive progress callbacks.
pub(super) const PROGRESS_REPORT_MAX_INTERVAL: Duration = Duration::from_secs(1);

/// Maximum number of distinct warning strings kept in
/// [`crate::indexer::IndexResult::warnings`].
pub(super) const MAX_WARNINGS: usize = 100;

/// Heap segments below this total size use the sequential
/// path.
pub(super) const PARALLEL_THRESHOLD: u64 = 32 * 1024 * 1024;

/// Calls `notifier.bytes_scanned` when enough bytes or
/// time have elapsed since the last report.
///
/// `pos` is the relative cursor position within the
/// records section. `base_offset` is the absolute file
/// offset of the records section start. The notifier
/// receives the absolute offset `base_offset + pos`.
pub(super) fn maybe_report_progress(
    pos: usize,
    base_offset: u64,
    last_progress_bytes: &mut usize,
    last_progress_at: &mut Instant,
    notifier: &mut hprof_api::ProgressNotifier,
) -> bool {
    let now = Instant::now();
    let enough_bytes = pos.saturating_sub(*last_progress_bytes) >= PROGRESS_REPORT_INTERVAL;
    let enough_time = now.duration_since(*last_progress_at) >= PROGRESS_REPORT_MAX_INTERVAL;
    if enough_bytes || enough_time {
        notifier.bytes_scanned(base_offset + pos as u64);
        *last_progress_bytes = pos;
        *last_progress_at = now;
        true
    } else {
        false
    }
}
