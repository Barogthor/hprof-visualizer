//! Per-segment BinaryFuse8 probabilistic filters for object ID lookup.
//!
//! The file is divided into fixed-size [`SEGMENT_SIZE`] slices. During the
//! first pass, [`SegmentFilterBuilder`] collects object IDs keyed by their
//! segment index. Calling [`SegmentFilterBuilder::build`] produces one
//! [`SegmentFilter`] per non-empty segment.
//!
//! Filters have a ~0.4 % false-positive rate. A `false` result is a
//! guaranteed absence; a `true` result means the object *probably* lives in
//! that segment.

use std::collections::HashMap;

use xorf::{BinaryFuse8, Filter};

/// Size of one file segment in bytes (64 MiB).
pub(crate) const SEGMENT_SIZE: usize = 64 * 1024 * 1024;

/// A BinaryFuse8 filter covering one 64 MiB slice of the hprof file.
// `filter` and `contains` are used only in tests today; the engine (Story 3.4+)
// will call `contains` in production. Suppress false dead_code warnings.
#[allow(dead_code)]
pub(crate) struct SegmentFilter {
    /// Zero-based index of the segment this filter covers.
    pub segment_index: usize,
    filter: BinaryFuse8,
}

impl SegmentFilter {
    /// Returns `true` if `id` was in the construction set (or is a false
    /// positive, ~0.4 % chance).
    // Used by the engine (Story 3.4+); suppress dead_code until then.
    #[allow(dead_code)]
    pub(crate) fn contains(&self, id: u64) -> bool {
        self.filter.contains(&id)
    }
}

impl std::fmt::Debug for SegmentFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SegmentFilter {{ segment_index: {} }}",
            self.segment_index
        )
    }
}

/// Accumulates object IDs per segment during the first pass, then builds
/// [`SegmentFilter`] instances via [`build`](SegmentFilterBuilder::build).
pub(crate) struct SegmentFilterBuilder {
    buckets: HashMap<usize, Vec<u64>>,
}

impl SegmentFilterBuilder {
    /// Creates an empty builder.
    pub(crate) fn new() -> Self {
        Self {
            buckets: HashMap::new(),
        }
    }

    /// Records `id` as belonging to the segment that contains `data_offset`.
    pub(crate) fn add(&mut self, data_offset: usize, id: u64) {
        let seg = data_offset / SEGMENT_SIZE;
        self.buckets.entry(seg).or_default().push(id);
    }

    /// Consumes the builder and produces one [`SegmentFilter`] per non-empty
    /// segment. Segments whose filter construction fails are silently skipped.
    ///
    /// `progress_fn` is called after each segment is built with
    /// `(segments_done, segments_total)`.
    pub(crate) fn build_with_progress(
        self,
        mut progress_fn: impl FnMut(usize, usize),
    ) -> Vec<SegmentFilter> {
        let total = self.buckets.len();
        let mut filters = Vec::new();
        for (segment_index, mut ids) in self.buckets {
            ids.sort_unstable();
            ids.dedup();
            if let Ok(filter) = BinaryFuse8::try_from(ids.as_slice()) {
                filters.push(SegmentFilter {
                    segment_index,
                    filter,
                });
            }
            progress_fn(filters.len(), total);
        }
        filters
    }

    /// Consumes the builder and produces one [`SegmentFilter`] per non-empty
    /// segment without a progress callback.
    pub(crate) fn build(self) -> Vec<SegmentFilter> {
        self.build_with_progress(|_, _| {})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_empty_returns_empty_vec() {
        let builder = SegmentFilterBuilder::new();
        assert!(builder.build().is_empty());
    }

    #[test]
    fn add_single_id_produces_one_filter_segment_0_contains_true() {
        let mut builder = SegmentFilterBuilder::new();
        builder.add(0, 42);
        let filters = builder.build();
        assert_eq!(filters.len(), 1);
        assert_eq!(filters[0].segment_index, 0);
        assert!(filters[0].contains(42));
    }

    #[test]
    fn add_id_at_segment_size_offset_produces_segment_1() {
        let mut builder = SegmentFilterBuilder::new();
        builder.add(SEGMENT_SIZE, 99);
        let filters = builder.build();
        assert_eq!(filters.len(), 1);
        assert_eq!(filters[0].segment_index, 1);
        assert!(filters[0].contains(99));
    }

    #[test]
    fn two_segments_two_filters_correct_membership() {
        let mut builder = SegmentFilterBuilder::new();
        // segment 3 — two objects
        builder.add(3 * SEGMENT_SIZE + 100, 1001);
        builder.add(3 * SEGMENT_SIZE + 200, 1002);
        // segment 0 — one object
        builder.add(0, 500);
        let mut filters = builder.build();
        filters.sort_by_key(|f| f.segment_index);
        assert_eq!(filters.len(), 2);
        let seg0 = &filters[0];
        let seg3 = &filters[1];
        assert_eq!(seg0.segment_index, 0);
        assert_eq!(seg3.segment_index, 3);
        assert!(seg3.contains(1001));
        assert!(seg3.contains(1002));
    }

    #[test]
    fn segment0_does_not_contain_segment3_id() {
        let mut builder = SegmentFilterBuilder::new();
        builder.add(0, 500);
        builder.add(3 * SEGMENT_SIZE, 9999);
        let mut filters = builder.build();
        filters.sort_by_key(|f| f.segment_index);
        let seg0 = &filters[0];
        // 9999 was only added to segment 3 — guaranteed false negative in seg0
        assert!(!seg0.contains(9999));
    }

    #[test]
    fn duplicate_ids_same_segment_deduped_filter_contains_once() {
        let mut builder = SegmentFilterBuilder::new();
        builder.add(0, 77);
        builder.add(100, 77);
        builder.add(200, 77);
        let filters = builder.build();
        assert_eq!(filters.len(), 1);
        assert!(filters[0].contains(77));
    }
}
