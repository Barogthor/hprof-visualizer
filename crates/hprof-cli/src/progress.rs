//! CLI progress observer for hprof loading phases.
//!
//! [`CliProgressObserver`] implements
//! [`ParseProgressObserver`] with three indicatif bars:
//! scan bytes, segment completion, and name resolution.

use std::time::{Duration, Instant};

use hprof_api::ParseProgressObserver;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

/// Drives indicatif progress bars from observer signals.
pub struct CliProgressObserver {
    mp: MultiProgress,
    scan_bar: ProgressBar,
    segment_bar: Option<ProgressBar>,
    phase_bar: Option<ProgressBar>,
    name_bar: Option<ProgressBar>,
    start: Instant,
    total_bytes: u64,
}

impl CliProgressObserver {
    /// Creates a new observer with a scan bar
    /// registered to `mp`.
    pub fn new(mp: &MultiProgress, total_bytes: u64) -> Self {
        let pb = mp.add(ProgressBar::new(total_bytes));
        pb.set_style(
            ProgressStyle::with_template(
                "[{elapsed_precise}] \
                 [{bar:40.cyan/blue}] \
                 {bytes}/{total_bytes} \
                 {percent:.1}% ({bytes_per_sec}, \
                 ETA {eta})",
            )
            .unwrap()
            .progress_chars("=>-"),
        );
        Self {
            mp: mp.clone(),
            scan_bar: pb,
            segment_bar: None,
            phase_bar: None,
            name_bar: None,
            start: Instant::now(),
            total_bytes,
        }
    }

    /// Finishes all active bars and prints the elapsed
    /// summary line.
    pub fn finish(&mut self) {
        if let Some(bar) = self.phase_bar.take() {
            bar.finish_and_clear();
        }
        if let Some(bar) = self.name_bar.take() {
            bar.finish_and_clear();
        }
        if let Some(bar) = self.segment_bar.take() {
            bar.finish_and_clear();
        }
        self.scan_bar.finish_and_clear();
        let elapsed = self.start.elapsed();
        let secs = elapsed.as_secs_f64();
        if secs > 0.0 {
            let gb_per_sec = self.total_bytes as f64 / secs / 1_073_741_824.0;
            eprintln!(
                "Loaded in {elapsed:.1?} \
                 ({gb_per_sec:.2} GB/s)",
            );
        } else {
            eprintln!("Loaded in {elapsed:.1?}");
        }
    }
}

impl ParseProgressObserver for CliProgressObserver {
    fn on_bytes_scanned(&mut self, position: u64) {
        self.scan_bar.set_position(position);
    }

    fn on_segment_completed(&mut self, done: usize, total: usize) {
        let bar = self.segment_bar.get_or_insert_with(|| {
            self.scan_bar.finish();
            let pb = self.mp.add(ProgressBar::new(total as u64));
            pb.set_style(
                ProgressStyle::with_template(
                    "[{elapsed_precise}] \
                     [{bar:40.green/blue}] \
                     {pos}/{len} segments \
                     ({percent}%, ETA {eta})",
                )
                .unwrap()
                .progress_chars("=>-"),
            );
            pb
        });
        bar.set_length(total as u64);
        bar.set_position(done as u64);
        if done == total {
            bar.finish();
        }
    }

    fn on_phase_changed(&mut self, phase: &str) {
        if let Some(pb) = self.phase_bar.take() {
            pb.finish_and_clear();
        }
        if let Some(bar) = &self.segment_bar
            && bar.position()
                < bar.length().unwrap_or(0)
        {
            bar.finish_and_clear();
            self.segment_bar = None;
        }
        let pb = self.mp.add(ProgressBar::new_spinner());
        pb.set_style(
            ProgressStyle::with_template(
                "[{elapsed_precise}] \
                 {spinner:.green} {msg}",
            )
            .unwrap(),
        );
        pb.set_message(phase.to_owned());
        pb.enable_steady_tick(
            Duration::from_millis(120),
        );
        self.phase_bar = Some(pb);
    }

    fn on_names_resolved(
        &mut self,
        done: usize,
        total: usize,
    ) {
        if self.name_bar.is_none() {
            if let Some(pb) = self.phase_bar.take() {
                pb.finish_and_clear();
            }
            let pb = self
                .mp
                .add(ProgressBar::new(total as u64));
            pb.set_style(
                ProgressStyle::with_template(
                    "[{elapsed_precise}] \
                     {spinner:.green} Resolving \
                     thread names\u{2026} {pos}/{len}",
                )
                .unwrap(),
            );
            pb.enable_steady_tick(
                Duration::from_millis(120),
            );
            self.name_bar = Some(pb);
        }
        let bar = self.name_bar.as_ref().unwrap();
        bar.set_length(total as u64);
        bar.set_position(done as u64);
        if done == total {
            bar.finish();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_observer_constructs_without_panic() {
        let mp = MultiProgress::new();
        let _obs = CliProgressObserver::new(&mp, 1024);
    }

    #[test]
    fn cli_observer_on_bytes_scanned_does_not_panic() {
        let mp = MultiProgress::new();
        let mut obs = CliProgressObserver::new(&mp, 1024);
        obs.on_bytes_scanned(512);
    }

    #[test]
    fn cli_observer_on_segment_completed_does_not_panic() {
        let mp = MultiProgress::new();
        let mut obs = CliProgressObserver::new(&mp, 1024);
        obs.on_segment_completed(1, 5);
        obs.on_segment_completed(5, 5);
    }
}
