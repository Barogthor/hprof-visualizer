//! Skip-index for O(skip_interval) page access on
//! variable-size collection chains.
//!
//! Stores checkpoints at regular intervals during chain
//! traversal. On subsequent page requests, the walk resumes
//! from the nearest checkpoint instead of scanning from the
//! beginning.

/// Resume state at a skip-index checkpoint.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SkipCheckpoint {
    /// LinkedList: the node object ID at this checkpoint.
    LinkedListNode { node_id: u64 },
    /// HashMap/LinkedHashMap/ConcurrentHashMap: the table
    /// slot index and current node ID within that slot's
    /// chain.
    HashMapSlot { slot_index: usize, node_id: u64 },
}

/// Skip-index for O(skip_interval) page access on
/// variable-size collection chains.
///
/// Stores checkpoints at regular intervals during chain
/// traversal. On subsequent page requests, the walk resumes
/// from the nearest checkpoint instead of scanning from the
/// beginning.
pub(crate) struct SkipIndex {
    /// Interval between checkpoints (default 100).
    interval: usize,
    /// Checkpoints: entry index → resume state.
    /// Sorted by entry index (always multiples of
    /// `interval`): 0, interval, 2*interval, …
    checkpoints: Vec<SkipCheckpoint>,
    /// True if the full collection has been traversed.
    complete: bool,
}

impl SkipIndex {
    /// Creates a new empty skip-index with the given
    /// checkpoint interval.
    pub(crate) fn new(interval: usize) -> Self {
        Self {
            interval,
            checkpoints: Vec::new(),
            complete: false,
        }
    }

    /// Records a checkpoint at `entry_index` if it is the
    /// next expected checkpoint boundary.
    ///
    /// The next expected index is always
    /// `checkpoints.len() * interval`. Calls with any other
    /// `entry_index` are silently ignored (idempotent,
    /// gap-safe).
    pub(crate) fn record(&mut self, entry_index: usize, checkpoint: SkipCheckpoint) {
        let next_expected = self.checkpoints.len() * self.interval;
        if entry_index == next_expected {
            self.checkpoints.push(checkpoint);
        }
    }

    /// Returns the highest checkpoint with index ≤
    /// `entry_index`, or `None` if the index is empty.
    ///
    /// The returned tuple is `(checkpoint_index, &checkpoint)`.
    /// The caller walks forward from `checkpoint_index` to
    /// reach `entry_index`.
    pub(crate) fn nearest_before(&self, entry_index: usize) -> Option<(usize, &SkipCheckpoint)> {
        if self.checkpoints.is_empty() {
            return None;
        }
        // Checkpoint at position i covers entry_index
        // = i * interval. We want the largest i such that
        // i * interval ≤ entry_index.
        let i = (entry_index / self.interval).min(self.checkpoints.len() - 1);
        Some((i * self.interval, &self.checkpoints[i]))
    }

    /// Marks the skip-index as complete (full collection
    /// traversed). Prevents unnecessary re-traversal.
    pub(crate) fn mark_complete(&mut self) {
        self.complete = true;
    }

    /// Returns `true` if the full collection has been
    /// traversed.
    #[cfg(test)]
    pub(crate) fn is_complete(&self) -> bool {
        self.complete
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_and_nearest_before_interval_3() {
        let mut si = SkipIndex::new(3);
        let cp0 = SkipCheckpoint::LinkedListNode { node_id: 100 };
        let cp3 = SkipCheckpoint::LinkedListNode { node_id: 200 };
        let cp6 = SkipCheckpoint::LinkedListNode { node_id: 300 };

        si.record(0, cp0.clone());
        si.record(3, cp3.clone());
        si.record(6, cp6.clone());

        let (idx, cp) = si.nearest_before(5).unwrap();
        assert_eq!(idx, 3);
        assert_eq!(*cp, cp3);
    }

    #[test]
    fn nearest_before_between_first_two() {
        let mut si = SkipIndex::new(3);
        let cp0 = SkipCheckpoint::LinkedListNode { node_id: 100 };
        let cp3 = SkipCheckpoint::LinkedListNode { node_id: 200 };

        si.record(0, cp0.clone());
        si.record(3, cp3);

        let (idx, cp) = si.nearest_before(1).unwrap();
        assert_eq!(idx, 0);
        assert_eq!(*cp, cp0);
    }

    #[test]
    fn nearest_before_empty_returns_none() {
        let si = SkipIndex::new(3);
        assert!(si.nearest_before(5).is_none());
    }

    #[test]
    fn mark_complete_and_is_complete() {
        let mut si = SkipIndex::new(10);
        assert!(!si.is_complete());
        si.mark_complete();
        assert!(si.is_complete());
    }

    #[test]
    fn duplicate_record_is_idempotent() {
        let mut si = SkipIndex::new(3);
        let cp0a = SkipCheckpoint::LinkedListNode { node_id: 100 };
        let cp0b = SkipCheckpoint::LinkedListNode { node_id: 999 };

        si.record(0, cp0a.clone());
        // Second call at same index: not the next expected
        // (next expected is now 3), so it's a no-op.
        si.record(0, cp0b);

        assert_eq!(si.checkpoints.len(), 1);
        assert_eq!(si.checkpoints[0], cp0a);
    }

    #[test]
    fn gap_skipping_no_ops() {
        let mut si = SkipIndex::new(10);
        let cp0 = SkipCheckpoint::LinkedListNode { node_id: 100 };
        si.record(0, cp0.clone());

        // Skip 10, try 20 and 30 — both are no-ops
        // because 10 was never recorded.
        si.record(20, SkipCheckpoint::LinkedListNode { node_id: 200 });
        si.record(30, SkipCheckpoint::LinkedListNode { node_id: 300 });

        assert_eq!(si.checkpoints.len(), 1);
        let (idx, cp) = si.nearest_before(25).unwrap();
        assert_eq!(idx, 0);
        assert_eq!(*cp, cp0);
    }

    #[test]
    fn hashmap_checkpoint_variant() {
        let mut si = SkipIndex::new(5);
        let cp0 = SkipCheckpoint::HashMapSlot {
            slot_index: 0,
            node_id: 10,
        };
        let cp5 = SkipCheckpoint::HashMapSlot {
            slot_index: 3,
            node_id: 42,
        };

        si.record(0, cp0);
        si.record(5, cp5.clone());

        let (idx, cp) = si.nearest_before(7).unwrap();
        assert_eq!(idx, 5);
        assert_eq!(*cp, cp5);
    }
}
