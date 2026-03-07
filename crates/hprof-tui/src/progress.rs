//! Progress bar for the hprof first-pass indexing stage.
//!
//! [`ProgressReporter`] wraps an [`indicatif::ProgressBar`] and exposes a
//! simple API compatible with the `progress_fn` callback accepted by
//! [`hprof_engine::open_hprof_file_with_progress`].
//!
//! Example output during indexing:
//! ```text
//! [00:00:03] [=========>-----------] 512MiB/1.00GiB (1.24GB/s, ETA 2s)
//! ```
//!
//! On completion, [`ProgressReporter::finish`] clears the bar and prints:
//! ```text
//! Indexed 142057/142057 records in 3.2s (1.24 GB/s)
//! ```

use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;

/// Drives an [`indicatif::ProgressBar`] from byte-offset callbacks.
pub struct ProgressReporter {
    pb: ProgressBar,
    start: std::time::Instant,
    total_bytes: u64,
}

impl ProgressReporter {
    /// Creates a new reporter for a file of `total_bytes` bytes.
    pub fn new(total_bytes: u64) -> Self {
        let pb = ProgressBar::new(total_bytes);
        pb.set_style(
            ProgressStyle::with_template(
                "[{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} \
                 {percent:.1}% ({bytes_per_sec}, ETA {eta})",
            )
            .unwrap()
            .progress_chars("=>-"),
        );
        pb.enable_steady_tick(Duration::from_secs(1));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_constructs_without_panic() {
        let _reporter = ProgressReporter::new(1024);
    }

    #[test]
    fn on_bytes_processed_does_not_panic() {
        let mut reporter = ProgressReporter::new(1024);
        reporter.on_bytes_processed(512);
    }
}
