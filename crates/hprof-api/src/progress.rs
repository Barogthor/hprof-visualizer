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
    /// incremented in the main thread after
    /// `par_iter().collect()`, never inside workers.
    fn on_segment_completed(&mut self, done: usize, total: usize);

    /// Thread name resolution progress.
    ///
    /// `done` ranges from `chunk_size` to `total`,
    /// increasing by `chunk_size` each call (final call
    /// may be less than a full chunk). Unlike
    /// `on_segment_completed`, this does NOT increment
    /// by 1. Never called when `total == 0`.
    fn on_names_resolved(&mut self, done: usize, total: usize);
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
}

#[cfg(feature = "test-utils")]
#[derive(Debug, Clone, PartialEq)]
pub enum ProgressEvent {
    BytesScanned(u64),
    SegmentCompleted { done: usize, total: usize },
    NamesResolved { done: usize, total: usize },
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
    fn on_names_resolved(&mut self, done: usize, total: usize) {
        self.events
            .push(ProgressEvent::NamesResolved { done, total });
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
    }

    #[test]
    fn notifier_delegates_to_observer() {
        let mut obs = NullProgressObserver;
        let mut notifier = ProgressNotifier::new(&mut obs);
        notifier.bytes_scanned(100);
        notifier.segment_completed(1, 5);
        notifier.names_resolved(2, 4);
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
        obs.on_names_resolved(2, 4);
        assert_eq!(obs.events.len(), 3);
        assert_eq!(obs.events[0], ProgressEvent::BytesScanned(100));
        assert_eq!(
            obs.events[1],
            ProgressEvent::SegmentCompleted { done: 1, total: 3 }
        );
        assert_eq!(
            obs.events[2],
            ProgressEvent::NamesResolved { done: 2, total: 4 }
        );
    }
}
