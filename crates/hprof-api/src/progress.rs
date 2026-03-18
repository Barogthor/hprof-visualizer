//! Progress observation for first-pass indexing.
//!
//! [`ParseProgressObserver`] receives phase-appropriate
//! signals during indexing. [`ProgressNotifier`] is a
//! newtype wrapper for internal use (avoids generic
//! monomorphisation). [`NullProgressObserver`] is a no-op
//! implementation for callers that don't need progress.

/// Observer for first-pass indexing progress.
///
/// Implementors receive phase-appropriate signals:
/// - `on_bytes_scanned`: sequential record scan (monotone
///   byte offset, throttled by caller)
/// - `on_segment_completed`: heap segment extraction
///   (done/total, always called per segment)
/// - `on_names_resolved`: thread name resolution
///   (done/total)
pub trait ParseProgressObserver {
    /// Sequential record scan progress.
    ///
    /// `position` is an absolute byte offset from the
    /// start of the file (includes the header).
    /// Guaranteed monotonically increasing. Throttled
    /// to ~4 MiB / 1s intervals by the caller.
    fn on_bytes_scanned(&mut self, position: u64);

    /// A heap segment finished extraction.
    ///
    /// `done` increases by 1 each call, from 1 to
    /// `total`. Called once per segment regardless of
    /// parallel or sequential path. `done` is strictly
    /// monotonically increasing — the counter is
    /// incremented on the main thread after each
    /// segment result is merged.
    fn on_segment_completed(&mut self, done: usize, total: usize);

    /// Thread name resolution progress.
    ///
    /// `done` ranges from `chunk_size` to `total`,
    /// increasing by `chunk_size` each call (final call
    /// may be less than a full chunk). Unlike
    /// `on_segment_completed`, this does NOT increment
    /// by 1. Never called when `total == 0`.
    fn on_names_resolved(&mut self, done: usize, total: usize);

    /// Heap extraction byte-level progress.
    ///
    /// `done` and `total` are `u64` (not `usize`) because
    /// byte counts can exceed 4 GB on 32-bit targets.
    /// Called once per segment completion with cumulative
    /// bytes extracted so far. Default no-op so existing
    /// impls don't break.
    fn on_heap_bytes_extracted(
        &mut self,
        _done: u64,
        _total: u64,
    ) {
    }

    /// A named loading phase has started.
    ///
    /// `phase` is the full spinner label including
    /// trailing "…". Called once per phase transition.
    /// Default no-op so existing impls don't break.
    fn on_phase_changed(&mut self, _phase: &str) {}
}

/// No-op observer for callers that don't need progress.
pub struct NullProgressObserver;

impl ParseProgressObserver for NullProgressObserver {
    fn on_bytes_scanned(&mut self, _position: u64) {}
    fn on_segment_completed(&mut self, _done: usize, _total: usize) {}
    fn on_names_resolved(&mut self, _done: usize, _total: usize) {}
}

/// Newtype wrapping `&mut dyn ParseProgressObserver`.
///
/// Public so that downstream crates (`hprof-parser`,
/// `hprof-engine`) can accept it in their internal
/// functions without re-importing the trait. The public
/// entry points (`HprofFile`, `Engine`) accept
/// `&mut dyn ParseProgressObserver`; a
/// `ProgressNotifier` is created at the boundary and
/// threaded inward.
pub struct ProgressNotifier<'a>(&'a mut dyn ParseProgressObserver);

impl<'a> ProgressNotifier<'a> {
    /// Wraps an observer for internal use.
    pub fn new(observer: &'a mut dyn ParseProgressObserver) -> Self {
        Self(observer)
    }

    /// Reports sequential scan progress (absolute
    /// file offset).
    pub fn bytes_scanned(&mut self, position: u64) {
        self.0.on_bytes_scanned(position);
    }

    /// Reports a heap segment extraction completion.
    pub fn segment_completed(&mut self, done: usize, total: usize) {
        self.0.on_segment_completed(done, total);
    }

    /// Reports thread name resolution progress.
    pub fn names_resolved(&mut self, done: usize, total: usize) {
        self.0.on_names_resolved(done, total);
    }

    /// Reports heap extraction byte-level progress.
    pub fn heap_bytes_extracted(
        &mut self,
        done: u64,
        total: u64,
    ) {
        debug_assert!(done <= total);
        self.0.on_heap_bytes_extracted(done, total);
    }

    /// Reports a loading phase transition.
    pub fn phase_changed(&mut self, phase: &str) {
        self.0.on_phase_changed(phase);
    }
}

#[cfg(feature = "test-utils")]
#[derive(Debug, Clone, PartialEq)]
pub enum ProgressEvent {
    BytesScanned(u64),
    SegmentCompleted { done: usize, total: usize },
    HeapBytesExtracted { done: u64, total: u64 },
    NamesResolved { done: usize, total: usize },
    PhaseChanged(String),
}

#[cfg(feature = "test-utils")]
#[derive(Debug, Default)]
pub struct TestObserver {
    pub events: Vec<ProgressEvent>,
}

#[cfg(feature = "test-utils")]
impl ParseProgressObserver for TestObserver {
    fn on_bytes_scanned(&mut self, position: u64) {
        self.events.push(ProgressEvent::BytesScanned(position));
    }
    fn on_segment_completed(&mut self, done: usize, total: usize) {
        self.events
            .push(ProgressEvent::SegmentCompleted { done, total });
    }
    fn on_heap_bytes_extracted(
        &mut self,
        done: u64,
        total: u64,
    ) {
        self.events
            .push(ProgressEvent::HeapBytesExtracted {
                done,
                total,
            });
    }
    fn on_names_resolved(&mut self, done: usize, total: usize) {
        self.events
            .push(ProgressEvent::NamesResolved { done, total });
    }
    fn on_phase_changed(&mut self, phase: &str) {
        self.events
            .push(ProgressEvent::PhaseChanged(phase.to_owned()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_observer_compiles_and_is_callable() {
        let mut obs = NullProgressObserver;
        obs.on_bytes_scanned(42);
        obs.on_segment_completed(1, 10);
        obs.on_names_resolved(5, 10);
        obs.on_phase_changed("test phase");
    }

    #[test]
    fn notifier_delegates_phase_changed_to_observer() {
        struct CountingObserver {
            phase_calls: usize,
        }
        impl ParseProgressObserver for CountingObserver {
            fn on_bytes_scanned(&mut self, _: u64) {}
            fn on_segment_completed(&mut self, _: usize, _: usize) {}
            fn on_names_resolved(&mut self, _: usize, _: usize) {}
            fn on_phase_changed(&mut self, _: &str) {
                self.phase_calls += 1;
            }
        }

        let mut obs = CountingObserver { phase_calls: 0 };
        let mut notifier = ProgressNotifier::new(&mut obs);
        notifier.phase_changed("phase\u{2026}");
        assert_eq!(obs.phase_calls, 1);
    }
}

#[cfg(all(test, feature = "test-utils"))]
mod test_utils_tests {
    use super::*;

    #[test]
    fn test_observer_collects_all_event_types() {
        let mut obs = TestObserver::default();
        obs.on_bytes_scanned(100);
        obs.on_segment_completed(1, 3);
        obs.on_heap_bytes_extracted(1024, 4096);
        obs.on_names_resolved(2, 4);
        obs.on_phase_changed("phase\u{2026}");
        assert_eq!(obs.events.len(), 5);
        assert_eq!(obs.events[0], ProgressEvent::BytesScanned(100));
        assert_eq!(
            obs.events[1],
            ProgressEvent::SegmentCompleted { done: 1, total: 3 }
        );
        assert_eq!(
            obs.events[2],
            ProgressEvent::HeapBytesExtracted {
                done: 1024,
                total: 4096
            }
        );
        assert_eq!(
            obs.events[3],
            ProgressEvent::NamesResolved { done: 2, total: 4 }
        );
        assert_eq!(
            obs.events[4],
            ProgressEvent::PhaseChanged("phase\u{2026}".to_owned())
        );
    }

    #[test]
    fn test_observer_captures_phase_changed() {
        let mut obs = TestObserver::default();
        obs.on_phase_changed("Building segment filters\u{2026}");
        obs.on_phase_changed("Resolving threads (round 1/3)\u{2026}");
        assert_eq!(obs.events.len(), 2);
        assert_eq!(
            obs.events[0],
            ProgressEvent::PhaseChanged("Building segment filters\u{2026}".to_owned())
        );
        assert_eq!(
            obs.events[1],
            ProgressEvent::PhaseChanged("Resolving threads (round 1/3)\u{2026}".to_owned())
        );
    }
}
