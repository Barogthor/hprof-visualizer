//! Progress reporters for hprof loading phases.
//!
//! - [`ProgressReporter`] — byte-level scan progress (unified scan + inline
//!   filter build).
//! - [`NameProgressReporter`] — thread name resolution spinner.

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
                "[{elapsed_precise}] [{bar:40.cyan/blue}] \
                 {bytes}/{total_bytes} \
                 {percent:.1}% ({bytes_per_sec}, ETA {eta})",
            )
            .unwrap()
            .progress_chars("=>-"),
        );
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

    /// Records elapsed time and total bytes for the summary line printed
    /// after all loading phases complete.
    pub fn elapsed_summary(&self) -> (std::time::Duration, u64) {
        (self.start.elapsed(), self.total_bytes)
    }
}

/// Drives a spinner for the thread name resolution phase.
///
/// Registered with the same [`MultiProgress`] to avoid terminal flickering.
pub struct NameProgressReporter {
    mp: MultiProgress,
    pb: Option<ProgressBar>,
}

impl NameProgressReporter {
    /// Creates a new reporter that will add its spinner to `mp` on first use.
    pub fn new(mp: &MultiProgress) -> Self {
        Self {
            mp: mp.clone(),
            pb: None,
        }
    }

    /// Called after each thread name is resolved.
    pub fn on_name_resolved(&mut self, done: usize, total: usize) {
        let pb = self.pb.get_or_insert_with(|| {
            let pb = self.mp.add(ProgressBar::new(total as u64));
            pb.set_style(
                ProgressStyle::with_template(
                    "[{elapsed_precise}] {spinner:.green} \
                     Resolving thread names… {pos}/{len}",
                )
                .unwrap(),
            );
            pb.enable_steady_tick(Duration::from_millis(120));
            pb
        });
        pb.set_length(total as u64);
        pb.set_position(done as u64);
    }

    /// Clears the spinner if it was ever shown.
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
}
