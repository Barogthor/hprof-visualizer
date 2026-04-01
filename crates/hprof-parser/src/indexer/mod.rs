//! Indexing subsystem: builds in-memory lookup structures from a sequential
//! hprof file pass.
//!
//! - [`precise`] — [`precise::PreciseIndex`]: five `HashMap` collections for
//!   O(1) lookup of structural records.
//! - [`first_pass`] — [`first_pass::run_first_pass`]: single sequential scan
//!   that populates a [`precise::PreciseIndex`] and returns an [`IndexResult`].
//! - [`segment`] — [`segment::SegmentFilter`]: per-64-MiB BinaryFuse8 filters
//!   for fast candidate-segment lookup by object ID.

pub mod first_pass;
pub(crate) mod precise;
pub(crate) mod segment;
pub(crate) mod validation;

use precise::PreciseIndex;
use segment::SegmentFilter;

/// A single `HEAP_DUMP` or `HEAP_DUMP_SEGMENT` record
/// location. Offsets are relative to the records section.
#[derive(Debug, Clone, Copy)]
pub struct HeapRecordRange {
    /// Byte offset of the payload start within the
    /// records section.
    pub payload_start: u64,
    /// Length of the payload in bytes.
    pub payload_length: u64,
}

/// Post-extraction allocation diagnostics.
///
/// Captures entry point count, filter lookup timing,
/// and `PreciseIndex` heap size.
///
/// Only available with the `test-utils` feature.
#[cfg(feature = "test-utils")]
#[derive(Debug, Default, Clone, Copy)]
pub struct DiagnosticInfo {
    /// Number of segment entry points recorded
    /// during extraction.
    pub entry_point_count: usize,
    /// Time spent in batched filter lookups during
    /// thread resolution (milliseconds).
    pub filter_lookup_ms: u64,
    /// Estimated heap bytes of the `PreciseIndex`
    /// structures, computed via
    /// [`hprof_api::MemorySize`].
    pub precise_index_heap_bytes: usize,
}

/// Result of a tolerant first-pass index run.
///
/// All non-fatal errors are collected in `warnings` rather than propagated.
/// Use [`IndexResult::percent_indexed`] to derive the success ratio.
///
/// Note: `records_attempted` counts only **known** record types whose payload
/// window was within bounds. Unknown tags are silently skipped and not counted.
#[derive(Debug)]
pub struct IndexResult {
    /// Populated structural index.
    pub index: PreciseIndex,
    /// Human-readable description of each skipped or corrupted record.
    pub warnings: Vec<String>,
    /// Records where the header was valid and payload window was within bounds.
    pub records_attempted: u64,
    /// Records successfully parsed and inserted into the index.
    pub records_indexed: u64,
    /// Per-segment BinaryFuse8 filters built from heap dump records.
    pub(crate) segment_filters: Vec<SegmentFilter>,
    /// Location of every `HEAP_DUMP` (0x0C) and
    /// `HEAP_DUMP_SEGMENT` (0x1C) record.
    pub heap_record_ranges: Vec<HeapRecordRange>,
    /// `true` when at least one heap segment had
    /// unrecognised or truncated sub-tags.
    pub has_heap_parse_anomalies: bool,
    /// Post-extraction allocation diagnostics.
    ///
    /// Only populated with the `test-utils` feature.
    #[cfg(feature = "test-utils")]
    pub diagnostics: DiagnosticInfo,
}

impl IndexResult {
    /// Returns the percentage of attempted records successfully indexed.
    ///
    /// Returns `100.0` when no records were attempted (empty file).
    pub fn percent_indexed(&self) -> f64 {
        if self.records_attempted == 0 {
            return 100.0;
        }
        self.records_indexed as f64 / self.records_attempted as f64 * 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_result(attempted: u64, indexed: u64) -> IndexResult {
        IndexResult {
            index: PreciseIndex::new(),
            warnings: Vec::new(),
            records_attempted: attempted,
            records_indexed: indexed,
            segment_filters: Vec::new(),
            heap_record_ranges: Vec::new(),
            has_heap_parse_anomalies: false,
            #[cfg(feature = "test-utils")]
            diagnostics: DiagnosticInfo::default(),
        }
    }

    #[test]
    fn percent_indexed_zero_attempted_returns_100() {
        let r = make_result(0, 0);
        assert_eq!(r.percent_indexed(), 100.0);
    }

    #[test]
    fn percent_indexed_all_indexed_returns_100() {
        let r = make_result(10, 10);
        assert_eq!(r.percent_indexed(), 100.0);
    }

    #[test]
    fn percent_indexed_partial_returns_correct_ratio() {
        let r = make_result(10, 8);
        assert_eq!(r.percent_indexed(), 80.0);
    }
}
