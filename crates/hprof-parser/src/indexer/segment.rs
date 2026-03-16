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

use xorf::{BinaryFuse8, Filter};

/// Size of one file segment in bytes (64 MiB).
pub const SEGMENT_SIZE: usize = 64 * 1024 * 1024;

/// A BinaryFuse8 filter covering one 64 MiB slice of
/// the hprof file.
pub(crate) struct SegmentFilter {
    /// Zero-based index of the segment this filter
    /// covers.
    pub segment_index: usize,
    filter: BinaryFuse8,
}

impl SegmentFilter {
    /// Returns `true` if `id` was in the construction
    /// set (or is a false positive, ~0.4 % chance).
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

/// Builds [`SegmentFilter`] instances incrementally during the first pass.
///
/// When [`add`](SegmentFilterBuilder::add) detects that the object belongs
/// to a new segment, the previous segment's filter is finalized immediately
/// and its raw ID vector is freed. Call [`finish`](SegmentFilterBuilder::finish)
/// after the last record to finalize the final segment.
pub(crate) struct SegmentFilterBuilder {
    current_segment: Option<usize>,
    current_ids: Vec<u64>,
    filters: Vec<SegmentFilter>,
    warnings: Vec<String>,
}

impl SegmentFilterBuilder {
    /// Creates an empty builder.
    pub(crate) fn new() -> Self {
        Self {
            current_segment: None,
            current_ids: Vec::new(),
            filters: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// Records `id` as belonging to the segment that contains `data_offset`.
    ///
    /// When a new segment index is detected, the previous segment's filter
    /// is built immediately and its raw ID vector is freed.
    pub(crate) fn add(&mut self, data_offset: usize, id: u64) {
        let seg = data_offset / SEGMENT_SIZE;
        if self.current_segment != Some(seg) {
            self.finalize_current();
            self.current_segment = Some(seg);
        }
        self.current_ids.push(id);
    }

    /// Builds the filter for the current segment and resets state.
    fn finalize_current(&mut self) {
        if let Some(seg_idx) = self.current_segment.take() {
            let mut ids = std::mem::take(&mut self.current_ids);
            ids.sort_unstable();
            ids.dedup();
            match BinaryFuse8::try_from(ids.as_slice()) {
                Ok(filter) => {
                    self.filters.push(SegmentFilter {
                        segment_index: seg_idx,
                        filter,
                    });
                }
                Err(e) => {
                    self.warnings.push(format!(
                        "segment {seg_idx}: BinaryFuse8 build \
                         failed ({} IDs): {e}",
                        ids.len()
                    ));
                }
            }
        }
    }

    /// Returns the number of segment filters already built.
    #[allow(dead_code)]
    pub(crate) fn completed_count(&self) -> usize {
        self.filters.len()
    }

    /// Returns the number of raw IDs currently accumulated (not yet
    /// finalized).
    #[allow(dead_code)]
    pub(crate) fn pending_id_count(&self) -> usize {
        self.current_ids.len()
    }

    /// Finalizes the last segment and returns `(filters, warnings)`.
    pub(crate) fn finish(mut self) -> (Vec<SegmentFilter>, Vec<String>) {
        self.finalize_current();
        (self.filters, self.warnings)
    }

    /// Alias for [`finish`](SegmentFilterBuilder::finish).
    #[allow(dead_code)]
    pub(crate) fn build(self) -> (Vec<SegmentFilter>, Vec<String>) {
        self.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_empty_returns_empty_vec() {
        let builder = SegmentFilterBuilder::new();
        let (filters, _) = builder.build();
        assert!(filters.is_empty());
    }

    #[test]
    fn add_single_id_produces_one_filter_segment_0_contains_true() {
        let mut builder = SegmentFilterBuilder::new();
        builder.add(0, 42);
        let (filters, _) = builder.build();
        assert_eq!(filters.len(), 1);
        assert_eq!(filters[0].segment_index, 0);
        assert!(filters[0].contains(42));
    }

    #[test]
    fn add_id_at_segment_size_offset_produces_segment_1() {
        let mut builder = SegmentFilterBuilder::new();
        builder.add(SEGMENT_SIZE, 99);
        let (filters, _) = builder.build();
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
        let (mut filters, _) = builder.build();
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
    fn duplicate_ids_same_segment_deduped_filter_contains_once() {
        let mut builder = SegmentFilterBuilder::new();
        builder.add(0, 77);
        builder.add(100, 77);
        builder.add(200, 77);
        let (filters, _) = builder.build();
        assert_eq!(filters.len(), 1);
        assert!(filters[0].contains(77));
    }

    #[test]
    fn inline_filter_built_on_segment_change() {
        let mut builder = SegmentFilterBuilder::new();
        // Add IDs to segment 0
        builder.add(0, 42);
        builder.add(100, 43);
        assert_eq!(
            builder.completed_count(),
            0,
            "no filter yet while still in segment 0"
        );
        // Transition to segment 1 — should finalize seg 0
        builder.add(SEGMENT_SIZE, 99);
        assert_eq!(
            builder.completed_count(),
            1,
            "segment 0 filter must be built inline"
        );
        // Finish to get all filters
        let (filters, _) = builder.finish();
        assert_eq!(filters.len(), 2);
    }

    #[test]
    fn raw_ids_freed_after_segment_finalized() {
        let mut builder = SegmentFilterBuilder::new();
        builder.add(0, 42);
        builder.add(100, 43);
        // Transition to segment 1 — seg 0 IDs freed
        builder.add(SEGMENT_SIZE, 99);
        assert_eq!(
            builder.pending_id_count(),
            1,
            "only segment 1's single ID should remain"
        );
    }
}
