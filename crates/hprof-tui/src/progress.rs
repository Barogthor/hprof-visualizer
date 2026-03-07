//! Progress reporters for the two hprof indexing phases.
//!
//! - [`ProgressReporter`] — byte-level scan progress (phase 1).
//! - [`FilterProgressReporter`] — segment filter construction progress
//!   (phase 2: sort + BinaryFuse8 per 64 MiB segment).
//!
//! Example output during scan:
//! ```text
//! [00:00:03] [=========>-----------] 512MiB/1.00GiB 51.2% (1.24GB/s, ETA 2s)
//! ```
//! Example output during filter build:
//! ```text
//! [00:00:01] [=========>-----------]  8/16 segments
//! ```
//!
//! On completion, [`ProgressReporter::finish`] clears the bar and prints:
//! ```text
//! Indexed 142057/142057 records in 3.2s (1.24 GB/s)
//! ```

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::time::Duration;

/// Creates a new [`MultiProgress`] for coordinating the two indexing phase bars.
///
/// Callers that do not have `indicatif` as a direct dependency use this
/// constructor so they do not need to depend on the crate themselves.
pub fn new_multi_progress() -> MultiProgress {
    MultiProgress::new()
}

/// Drives an [`indicatif::ProgressBar`] from byte-offset callbacks.
pub struct ProgressReporter {
    pb: ProgressBar,
    start: std::time::Instant,
    total_bytes: u64,
}

impl ProgressReporter {
    /// Creates a new reporter registered with `mp` for a file of `total_bytes`
    /// bytes. Using [`MultiProgress`] prevents visual interference when a
    /// second bar is added later for the filter-build phase.
    pub fn new(mp: &MultiProgress, total_bytes: u64) -> Self {
        let pb = mp.add(ProgressBar::new(total_bytes));
        pb.set_style(
            ProgressStyle::with_template(
                "[{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} \
                 {percent:.1}% ({bytes_per_sec}, ETA {eta})",
            )
            .unwrap()
            .progress_chars("=>-"),
        );
        // No steady tick: the bar redraws only on set_position() calls, which
        // come from maybe_report_progress (≥1 per second during scan). During
        // the filter-build phase no set_position is called, so the bar freezes
        // at 100% instead of continuing to update the speed metric.
        Self {
            pb,
            start: std::time::Instant::now(),
            total_bytes,
        }
    }

    /// Advances the progress bar to `bytes` processed.
    pub fn on_bytes_processed(&mut self, bytes: u64) {
        self.pb.set_position(bytes);
    }

    /// Clears the bar and prints a one-line indexing summary to stdout.
    ///
    /// If `summary` contains warnings the indexing was incomplete: an
    /// additional user-facing warning line is printed to stderr showing the
    /// percentage of records successfully processed, followed by each
    /// low-level parser warning.
    pub fn finish(self, summary: &hprof_engine::IndexSummary) {
        let elapsed = self.start.elapsed();
        self.pb.finish_and_clear();
        let speed = self.total_bytes as f64 / elapsed.as_secs_f64() / 1e9;
        let percent = if summary.records_attempted > 0 {
            summary.records_indexed as f64 / summary.records_attempted as f64 * 100.0
        } else {
            100.0
        };
        println!(
            "Indexed {}/{} records ({:.1}%) in {:.1?} ({:.2} GB/s)",
            summary.records_indexed, summary.records_attempted, percent, elapsed, speed
        );
        if !summary.warnings.is_empty() {
            eprintln!(
                "warning: indexing incomplete — {:.1}% of records processed",
                percent
            );
            for w in &summary.warnings {
                eprintln!("warning: {w}");
            }
        }
    }
}

/// Drives a second [`indicatif::ProgressBar`] for the segment-filter build
/// phase (sort + BinaryFuse8 construction per 64 MiB segment).
///
/// The bar is created lazily on the first [`on_segment_built`] call so it
/// never appears during the scan phase.
///
/// [`on_segment_built`]: FilterProgressReporter::on_segment_built
pub struct FilterProgressReporter {
    mp: MultiProgress,
    pb: Option<ProgressBar>,
}

impl FilterProgressReporter {
    /// Creates a new reporter that will add its bar to `mp` on first use.
    pub fn new(mp: MultiProgress) -> Self {
        Self { mp, pb: None }
    }

    /// Called by the engine after each segment filter is built.
    ///
    /// `done` — segments completed so far; `total` — total segments to build.
    /// Creates the progress bar on the first invocation.
    pub fn on_segment_built(&mut self, done: usize, total: usize) {
        let pb = self.pb.get_or_insert_with(|| {
            let pb = self.mp.add(ProgressBar::new(total as u64));
            pb.set_style(
                ProgressStyle::with_template(
                    "[{elapsed_precise}] [{bar:40.green/white}] {pos}/{len} segments (ETA {eta})",
                )
                .unwrap()
                .progress_chars("=>-"),
            );
            pb.enable_steady_tick(Duration::from_secs(1));
            pb
        });
        pb.set_length(total as u64);
        pb.set_position(done as u64);
    }

    /// Clears the filter bar if it was ever shown.
    pub fn finish(self) {
        if let Some(pb) = self.pb {
            pb.finish_and_clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_constructs_without_panic() {
        let mp = MultiProgress::new();
        let _reporter = ProgressReporter::new(&mp, 1024);
    }

    #[test]
    fn on_bytes_processed_does_not_panic() {
        let mp = MultiProgress::new();
        let mut reporter = ProgressReporter::new(&mp, 1024);
        reporter.on_bytes_processed(512);
    }

    #[test]
    fn filter_reporter_on_segment_built_does_not_panic() {
        let mp = MultiProgress::new();
        let mut reporter = FilterProgressReporter::new(mp);
        reporter.on_segment_built(1, 10);
        reporter.on_segment_built(10, 10);
    }
}
