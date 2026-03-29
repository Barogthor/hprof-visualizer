//! Top-level entry point for opening and indexing an hprof file.
//!
//! [`HprofFile`] memory-maps the file, parses its header, and runs the
//! first-pass indexer in one call, making all structural metadata available
//! via [`HprofFile::index`] after construction. Truncated or corrupted
//! records are non-fatal and collected in [`HprofFile::index_warnings`].
//!
//! Use [`HprofFile::from_path_with_progress`] to receive byte-offset callbacks
//! during indexing, or [`HprofFile::from_path`] for a no-op convenience
//! wrapper.

use std::collections::HashSet;
use std::path::Path;

use byteorder::{BigEndian, ReadBytesExt};
use hprof_api::{MemoryBudget, NullProgressObserver, ParseProgressObserver, ProgressNotifier};
use memmap2::Mmap;
use rayon::prelude::*;
use rustc_hash::FxHashMap;

use crate::indexer::{first_pass::run_first_pass, precise::PreciseIndex, segment::SegmentFilter};
use crate::tags::HeapSubTag;
use crate::{HprofError, HprofHeader, RawInstance, open_readonly, parse_header, read_id};

/// Metadata for an `OBJECT_ARRAY_DUMP` sub-record.
///
/// Contains the array header without deserializing
/// elements. Use [`HprofFile::read_object_array_element`]
/// for O(1) positional access.
#[derive(Debug, Clone)]
pub struct ObjectArrayMeta {
    /// Class ID of the array's element type.
    pub class_id: u64,
    /// Number of elements in the array.
    pub num_elements: u32,
    /// Byte offset (relative to records section) of the
    /// first element in the array.
    pub elements_offset: u64,
}

/// Result of a batch instance resolution.
///
/// Contains both the parsed instances and their byte
/// offsets for caching in `OffsetCache`.
#[derive(Debug)]
pub struct BatchResult {
    /// Parsed `INSTANCE_DUMP` results keyed by object ID.
    pub instances: FxHashMap<u64, RawInstance>,
    /// Byte offsets (relative to records section) keyed
    /// by object ID, for cache insertion.
    pub offsets: FxHashMap<u64, u64>,
}

/// An open hprof file with a parsed header and populated structural index.
///
/// ## Fields
/// - `header`: [`HprofHeader`] — parsed file header (version, id_size,
///   timestamp).
/// - `index`: [`PreciseIndex`] — O(1) lookup maps for all structural records.
/// - `index_warnings`: non-fatal parse errors collected during indexing.
/// - `records_attempted`: known-type records whose payload window was within
///   bounds. Unknown-tag records are silently skipped and not counted here.
/// - `records_indexed`: records successfully parsed and inserted into the index.
/// - `segment_filters`: probabilistic per-segment filters for object ID
///   resolution. Each [`SegmentFilter`] covers a 64 MiB slice of the records
///   section and allows fast candidate-segment lookup before a targeted scan.
///
/// The internal `mmap` field keeps the memory mapping alive and is used
/// by [`HprofFile::resolve_string`] to lazily decode string content.
#[derive(Debug)]
pub struct HprofFile {
    /// Memory mapping — used by `resolve_string` and kept alive for
    /// the duration of this struct's lifetime.
    mmap: Mmap,
    /// Parsed hprof file header.
    pub header: HprofHeader,
    /// O(1) lookup index built from the first sequential pass.
    pub index: PreciseIndex,
    /// Warnings collected during indexing (non-fatal parse errors).
    pub index_warnings: Vec<String>,
    /// Records whose header and payload window were valid.
    pub records_attempted: u64,
    /// Records successfully parsed and inserted into the index.
    pub records_indexed: u64,
    /// Probabilistic per-segment filters for object ID resolution.
    // `SegmentFilter` is `pub(crate)` — object resolution via segment filters is Story 3.4.
    #[allow(private_interfaces)]
    pub segment_filters: Vec<SegmentFilter>,
    /// Byte offset of the first record (immediately after the file header).
    pub records_start: usize,
    /// Location of every HEAP_DUMP / HEAP_DUMP_SEGMENT
    /// record. See [`HeapRecordRange`].
    pub heap_record_ranges: Vec<crate::indexer::HeapRecordRange>,
}

impl HprofFile {
    /// Opens `path` as a read-only mmap, parses the
    /// header, and indexes all structural records,
    /// reporting progress through the observer.
    ///
    /// Truncated or corrupted records are non-fatal:
    /// they are collected in
    /// [`HprofFile::index_warnings`] and indexing
    /// continues where possible.
    ///
    /// ## Errors
    /// - [`HprofError::MmapFailed`] — file not found or
    ///   OS mapping failed.
    /// - [`HprofError::UnsupportedVersion`] — unrecognised
    ///   hprof version string.
    /// - [`HprofError::TruncatedRecord`] — file header is
    ///   truncated.
    pub fn from_path_with_progress(
        path: &Path,
        observer: &mut dyn ParseProgressObserver,
        budget: MemoryBudget,
    ) -> Result<Self, HprofError> {
        let mmap = open_readonly(path)?;
        let header = parse_header(&mmap)?;
        let records_start = header.records_start;
        let mut notifier = ProgressNotifier::new(observer);
        let result = run_first_pass(
            &mmap[records_start..],
            header.id_size,
            records_start as u64,
            &mut notifier,
            budget,
        );
        Ok(Self {
            mmap,
            header,
            index: result.index,
            index_warnings: result.warnings,
            records_attempted: result.records_attempted,
            records_indexed: result.records_indexed,
            segment_filters: result.segment_filters,
            records_start,
            heap_record_ranges: result.heap_record_ranges,
        })
    }

    /// Opens `path` and indexes it without progress.
    ///
    /// Convenience wrapper around
    /// [`HprofFile::from_path_with_progress`].
    ///
    /// ## Errors
    /// - [`HprofError::MmapFailed`] — file not found or
    ///   OS mapping failed.
    /// - [`HprofError::UnsupportedVersion`] — unrecognised
    ///   hprof version string.
    /// - [`HprofError::TruncatedRecord`] — file header is
    ///   truncated.
    pub fn from_path(path: &Path) -> Result<Self, HprofError> {
        Self::from_path_with_progress(path, &mut NullProgressObserver, MemoryBudget::Unlimited)
    }

    /// Returns the raw bytes of the records section (immediately after the
    /// file header).
    pub fn records_bytes(&self) -> &[u8] {
        &self.mmap[self.records_start..]
    }

    /// Resolves a [`HprofStringRef`] into an owned `String` by reading
    /// the content bytes directly from the mmap.
    ///
    /// The offset in `sref` is relative to the records section start.
    /// Invalid UTF-8 bytes are replaced with `\u{FFFD}`.
    pub fn resolve_string(&self, sref: &crate::HprofStringRef) -> String {
        sref.resolve(&self.mmap[self.records_start..])
    }

    /// Finds a `PRIMITIVE_ARRAY_DUMP` (sub-tag `0x23`) for `array_id`.
    ///
    /// Uses the same BinaryFuse8 segment filters as [`find_instance`].
    /// Returns `(element_type, raw_bytes)` where `element_type` is the hprof
    /// primitive type code (5=char, 8=byte, etc.) and `raw_bytes` is the flat
    /// array data.
    ///
    /// Returns `None` if the array is not found (absent or filter false-positive).
    pub fn find_prim_array(&self, array_id: u64) -> Option<(u8, Vec<u8>)> {
        use crate::indexer::segment::SEGMENT_SIZE;

        let records = self.records_bytes();
        let id_size = self.header.id_size;

        let candidate_segs: Vec<usize> = self
            .segment_filters
            .iter()
            .filter(|f| f.contains(array_id))
            .map(|f| f.segment_index)
            .collect();

        if candidate_segs.is_empty() {
            return None;
        }

        for r in &self.heap_record_ranges {
            let payload_end = r.payload_start + r.payload_length;

            let overlaps = candidate_segs.iter().any(|&seg| {
                let seg_start = seg as u64 * SEGMENT_SIZE as u64;
                let seg_end = seg_start + SEGMENT_SIZE as u64;
                r.payload_start < seg_end && payload_end > seg_start
            });

            if !overlaps {
                continue;
            }

            let start = r.payload_start as usize;
            let end = (payload_end as usize).min(records.len());
            if start >= records.len() {
                continue;
            }

            if let Some(result) = scan_for_prim_array(&records[start..end], array_id, id_size) {
                return Some(result);
            }
        }

        None
    }

    /// Finds an `OBJECT_ARRAY_DUMP` (sub-tag `0x22`) for `array_id`.
    ///
    /// Uses BinaryFuse8 segment filters like [`find_prim_array`].
    /// Returns `(element_class_id, element_ids)`.
    ///
    /// Returns `None` if not found (absent or filter
    /// false-positive).
    pub fn find_object_array(&self, array_id: u64) -> Option<(u64, Vec<u64>)> {
        let meta = self.find_object_array_meta(array_id)?;
        let n = meta.num_elements as usize;
        let mut elems = Vec::with_capacity(n);
        for i in 0..meta.num_elements {
            elems.push(self.read_object_array_element(&meta, i)?);
        }
        Some((meta.class_id, elems))
    }

    /// Returns metadata for an `OBJECT_ARRAY_DUMP` without
    /// deserializing elements. O(1) element access via
    /// [`read_object_array_element`].
    ///
    /// Returns `None` if not found.
    pub fn find_object_array_meta(&self, array_id: u64) -> Option<ObjectArrayMeta> {
        use crate::indexer::segment::SEGMENT_SIZE;

        let records = self.records_bytes();
        let id_size = self.header.id_size;

        let candidate_segs: Vec<usize> = self
            .segment_filters
            .iter()
            .filter(|f| f.contains(array_id))
            .map(|f| f.segment_index)
            .collect();

        if candidate_segs.is_empty() {
            return None;
        }

        for r in &self.heap_record_ranges {
            let payload_end = r.payload_start + r.payload_length;

            let overlaps = candidate_segs.iter().any(|&seg| {
                let seg_start = seg as u64 * SEGMENT_SIZE as u64;
                let seg_end = seg_start + SEGMENT_SIZE as u64;
                r.payload_start < seg_end && payload_end > seg_start
            });

            if !overlaps {
                continue;
            }

            let start = r.payload_start as usize;
            let end = (payload_end as usize).min(records.len());
            if start >= records.len() {
                continue;
            }

            if let Some(result) =
                scan_for_object_array_meta(&records[start..end], array_id, id_size, r.payload_start)
            {
                return Some(result);
            }
        }

        None
    }

    /// Reads a single element from an `OBJECT_ARRAY_DUMP`
    /// at `index` via O(1) arithmetic.
    ///
    /// Returns `None` if `index >= meta.num_elements` or
    /// the computed offset is out of bounds.
    pub fn read_object_array_element(&self, meta: &ObjectArrayMeta, index: u32) -> Option<u64> {
        if index >= meta.num_elements {
            return None;
        }
        let id_sz = self.header.id_size as usize;
        let byte_offset = (index as usize)
            .checked_mul(id_sz)?
            .checked_add(meta.elements_offset as usize)?;
        let records = self.records_bytes();
        if byte_offset + id_sz > records.len() {
            return None;
        }
        let mut cursor = std::io::Cursor::new(&records[byte_offset..byte_offset + id_sz]);
        read_id(&mut cursor, self.header.id_size).ok()
    }

    /// Reads an `INSTANCE_DUMP` sub-record at a known byte offset.
    ///
    /// `offset` is relative to the records section and must point to the
    /// sub-tag byte (0x21). Returns `None` if the data at `offset` is not
    /// a valid INSTANCE_DUMP.
    pub fn read_instance_at_offset(&self, offset: u64) -> Option<RawInstance> {
        let records = self.records_bytes();
        let start = offset as usize;
        if start >= records.len() {
            return None;
        }
        let data = &records[start..];
        let mut cursor = std::io::Cursor::new(data);
        let sub_tag = HeapSubTag::from(cursor.read_u8().ok()?);
        if sub_tag != HeapSubTag::InstanceDump {
            return None;
        }
        let _obj_id = read_id(&mut cursor, self.header.id_size).ok()?;
        let _stack_serial = cursor.read_u32::<BigEndian>().ok()?;
        let class_object_id = read_id(&mut cursor, self.header.id_size).ok()?;
        let num_bytes = cursor.read_u32::<BigEndian>().ok()? as usize;
        let pos = cursor.position() as usize;
        if pos + num_bytes > data.len() {
            return None;
        }
        Some(RawInstance {
            class_object_id,
            data: data[pos..pos + num_bytes].to_vec(),
        })
    }

    /// Reads a `PRIMITIVE_ARRAY_DUMP` sub-record at a known byte offset.
    ///
    /// `offset` is relative to the records section and must point to the
    /// sub-tag byte (0x23). Returns `(element_type, raw_bytes)`.
    pub fn read_prim_array_at_offset(&self, offset: u64) -> Option<(u8, Vec<u8>)> {
        use crate::indexer::first_pass::value_byte_size;

        let records = self.records_bytes();
        let start = offset as usize;
        if start >= records.len() {
            return None;
        }
        let data = &records[start..];
        let mut cursor = std::io::Cursor::new(data);
        let sub_tag = HeapSubTag::from(cursor.read_u8().ok()?);
        if sub_tag != HeapSubTag::PrimArrayDump {
            return None;
        }
        let _arr_id = read_id(&mut cursor, self.header.id_size).ok()?;
        let _stack_serial = cursor.read_u32::<BigEndian>().ok()?;
        let num_elements = cursor.read_u32::<BigEndian>().ok()? as usize;
        let elem_type = cursor.read_u8().ok()?;
        let elem_size = value_byte_size(elem_type, self.header.id_size);
        if elem_size == 0 {
            return None;
        }
        let byte_count = num_elements.checked_mul(elem_size)?;
        let pos = cursor.position() as usize;
        if pos + byte_count > data.len() {
            return None;
        }
        Some((elem_type, data[pos..pos + byte_count].to_vec()))
    }

    /// Finds and returns the raw instance dump for `object_id`.
    ///
    /// Uses BinaryFuse8 segment filters to narrow candidate segments, then
    /// performs a targeted scan of overlapping heap record payloads.
    ///
    /// Returns `None` if the object is not found (absent or filter
    /// false-positive).
    pub fn find_instance(&self, object_id: u64) -> Option<(RawInstance, u64)> {
        use crate::indexer::segment::SEGMENT_SIZE;

        let records = self.records_bytes();
        let id_size = self.header.id_size;

        let candidate_segs: Vec<usize> = self
            .segment_filters
            .iter()
            .filter(|f| f.contains(object_id))
            .map(|f| f.segment_index)
            .collect();

        if candidate_segs.is_empty() {
            return None;
        }

        for r in &self.heap_record_ranges {
            let payload_end = r.payload_start + r.payload_length;

            let overlaps = candidate_segs.iter().any(|&seg| {
                let seg_start = seg as u64 * SEGMENT_SIZE as u64;
                let seg_end = seg_start + SEGMENT_SIZE as u64;
                r.payload_start < seg_end && payload_end > seg_start
            });

            if !overlaps {
                continue;
            }

            let start = r.payload_start as usize;
            let end = (payload_end as usize).min(records.len());
            if start >= records.len() {
                continue;
            }

            if let Some((raw, rel_offset)) =
                scan_for_instance(&records[start..end], object_id, id_size)
            {
                let abs_offset = start as u64 + rel_offset;
                return Some((raw, abs_offset));
            }
        }

        None
    }

    /// Resolves multiple object instances in a single
    /// pass per segment, returning parsed instances and
    /// their byte offsets.
    ///
    /// Groups IDs by candidate segment (via
    /// `segment_filters.contains()`), then performs ONE
    /// linear scan per distinct segment collecting all
    /// matching INSTANCE_DUMP records.
    ///
    /// This method is side-effect-free: it does NOT read
    /// or write `OffsetCache`. The caller pre-partitions
    /// IDs (cached vs uncached) and inserts offsets after.
    pub fn batch_find_instances(&self, object_ids: &[u64]) -> BatchResult {
        use crate::indexer::segment::SEGMENT_SIZE;
        self.batch_find_instances_inner(object_ids, SEGMENT_SIZE)
    }

    /// Internal implementation with configurable
    /// `segment_size` for testability.
    pub(crate) fn batch_find_instances_inner(
        &self,
        object_ids: &[u64],
        segment_size: usize,
    ) -> BatchResult {
        let mut result = BatchResult {
            instances: FxHashMap::default(),
            offsets: FxHashMap::default(),
        };

        if object_ids.is_empty() {
            return result;
        }

        let records = self.records_bytes();
        let id_size = self.header.id_size;

        #[cfg(feature = "dev-profiling")]
        let _span = tracing::debug_span!(
            "batch_find_instances_parallel",
            num_uncached_ids = object_ids.len(),
        )
        .entered();

        // Phase 1 — Group IDs by candidate segment.
        // An ID may match multiple segment filters
        // (BinaryFuse8 false positives). Group it into
        // ALL matching segments.
        let mut seg_targets: FxHashMap<usize, HashSet<u64>> = FxHashMap::default();

        for &id in object_ids {
            for filter in &self.segment_filters {
                if filter.contains(id) {
                    seg_targets
                        .entry(filter.segment_index)
                        .or_default()
                        .insert(id);
                }
            }
        }

        // Phase 2 — Scan each segment group in parallel.
        #[cfg(feature = "dev-profiling")]
        tracing::debug!(
            seg_count = seg_targets.len(),
            "batch_find_instances_parallel segments"
        );
        let per_seg: Vec<_> = seg_targets
            .par_iter()
            .map(|(&seg_idx, targets)| {
                let seg_start = seg_idx as u64 * segment_size as u64;
                let seg_end = seg_start + segment_size as u64;
                let mut local_instances: FxHashMap<u64, RawInstance> = FxHashMap::default();
                let mut local_offsets: FxHashMap<u64, u64> = FxHashMap::default();

                for r in &self.heap_record_ranges {
                    let payload_end = r.payload_start + r.payload_length;
                    let overlaps = r.payload_start < seg_end && payload_end > seg_start;
                    if !overlaps {
                        continue;
                    }
                    let start = r.payload_start as usize;
                    if start >= records.len() {
                        continue;
                    }
                    let end = (payload_end as usize).min(records.len());
                    let found = scan_segment_for_instances(&records[start..end], targets, id_size);
                    for (obj_id, raw, offset) in found {
                        let abs_offset = start as u64 + offset;
                        local_instances.entry(obj_id).or_insert(raw);
                        local_offsets.entry(obj_id).or_insert(abs_offset);
                    }
                }
                (local_instances, local_offsets)
            })
            .collect();

        // Phase 3 — Sequential merge: first-found wins.
        // FxHashMap iteration order is unspecified, so for IDs
        // with BinaryFuse8 false positives (found in multiple
        // segment scans), the winning offset is arbitrary but
        // always valid — scan_segment_for_instances only returns
        // records it physically found in the slice.
        for (local_instances, local_offsets) in per_seg {
            for (id, raw) in local_instances {
                result.instances.entry(id).or_insert(raw);
            }
            for (id, off) in local_offsets {
                result.offsets.entry(id).or_insert(off);
            }
        }

        result
    }
}

/// Controls iteration in [`walk_heap_subrecords`].
enum SubRecordAction {
    /// Callback did not consume the sub-record body.
    /// The walker calls `skip_sub_record` to advance
    /// past it.
    Continue,
    /// Callback already advanced the cursor past the
    /// sub-record body.
    Consumed,
    /// Stop the walk immediately.
    Break,
}

/// Walks every heap sub-record in `data`, invoking
/// `callback` for each one.
///
/// The cursor is positioned just **after** the sub-tag
/// byte when the callback is invoked. The callback must
/// return [`SubRecordAction`] to indicate whether it
/// consumed the sub-record body or wants the walker to
/// skip it.
///
/// Stops on `SubRecordAction::Break`, I/O error, or
/// when `skip_sub_record` fails (truncated data).
fn walk_heap_subrecords<F>(data: &[u8], id_size: u32, mut callback: F)
where
    F: FnMut(HeapSubTag, u64, &mut std::io::Cursor<&[u8]>) -> SubRecordAction,
{
    use std::io::Cursor;

    let mut cursor = Cursor::new(data);
    loop {
        let tag_pos = cursor.position();
        let sub_tag = match cursor.read_u8() {
            Ok(t) => HeapSubTag::from(t),
            Err(_) => return,
        };
        match callback(sub_tag, tag_pos, &mut cursor) {
            SubRecordAction::Break => return,
            SubRecordAction::Consumed => {}
            SubRecordAction::Continue => {
                if !skip_sub_record(&mut cursor, sub_tag, id_size) {
                    return;
                }
            }
        }
    }
}

fn scan_for_instance(data: &[u8], target_id: u64, id_size: u32) -> Option<(RawInstance, u64)> {
    let mut result = None;
    walk_heap_subrecords(data, id_size, |sub_tag, tag_pos, cursor| {
        if sub_tag != HeapSubTag::InstanceDump {
            return SubRecordAction::Continue;
        }
        let obj_id = match read_id(cursor, id_size) {
            Ok(id) => id,
            Err(_) => return SubRecordAction::Break,
        };
        let _serial = match cursor.read_u32::<BigEndian>() {
            Ok(v) => v,
            Err(_) => return SubRecordAction::Break,
        };
        let class_object_id = match read_id(cursor, id_size) {
            Ok(id) => id,
            Err(_) => return SubRecordAction::Break,
        };
        let num_bytes = match cursor.read_u32::<BigEndian>() {
            Ok(n) => n as usize,
            Err(_) => return SubRecordAction::Break,
        };
        let pos = cursor.position() as usize;
        if pos + num_bytes > data.len() {
            return SubRecordAction::Break;
        }
        if obj_id == target_id {
            result = Some((
                RawInstance {
                    class_object_id,
                    data: data[pos..pos + num_bytes].to_vec(),
                },
                tag_pos,
            ));
            return SubRecordAction::Break;
        }
        cursor.set_position((pos + num_bytes) as u64);
        SubRecordAction::Consumed
    });
    result
}

fn scan_segment_for_instances(
    data: &[u8],
    target_ids: &HashSet<u64>,
    id_size: u32,
) -> Vec<(u64, RawInstance, u64)> {
    let mut results = Vec::new();
    walk_heap_subrecords(data, id_size, |sub_tag, tag_pos, cursor| {
        if sub_tag != HeapSubTag::InstanceDump {
            return SubRecordAction::Continue;
        }
        let obj_id = match read_id(cursor, id_size) {
            Ok(id) => id,
            Err(_) => return SubRecordAction::Break,
        };
        let _serial = match cursor.read_u32::<BigEndian>() {
            Ok(v) => v,
            Err(_) => return SubRecordAction::Break,
        };
        let class_object_id = match read_id(cursor, id_size) {
            Ok(id) => id,
            Err(_) => return SubRecordAction::Break,
        };
        let num_bytes = match cursor.read_u32::<BigEndian>() {
            Ok(n) => n as usize,
            Err(_) => return SubRecordAction::Break,
        };
        let pos = cursor.position() as usize;
        if pos + num_bytes > data.len() {
            return SubRecordAction::Break;
        }
        if target_ids.contains(&obj_id) {
            results.push((
                obj_id,
                RawInstance {
                    class_object_id,
                    data: data[pos..pos + num_bytes].to_vec(),
                },
                tag_pos,
            ));
        }
        cursor.set_position((pos + num_bytes) as u64);
        SubRecordAction::Consumed
    });
    results
}

fn scan_for_prim_array(data: &[u8], target_id: u64, id_size: u32) -> Option<(u8, Vec<u8>)> {
    use crate::indexer::first_pass::value_byte_size;

    let mut result = None;
    walk_heap_subrecords(data, id_size, |sub_tag, _tag_pos, cursor| {
        if sub_tag != HeapSubTag::PrimArrayDump {
            return SubRecordAction::Continue;
        }
        let arr_id = match read_id(cursor, id_size) {
            Ok(id) => id,
            Err(_) => return SubRecordAction::Break,
        };
        let _serial = match cursor.read_u32::<BigEndian>() {
            Ok(v) => v,
            Err(_) => return SubRecordAction::Break,
        };
        let num_elements = match cursor.read_u32::<BigEndian>() {
            Ok(n) => n as usize,
            Err(_) => return SubRecordAction::Break,
        };
        let elem_type = match cursor.read_u8() {
            Ok(t) => t,
            Err(_) => return SubRecordAction::Break,
        };
        let elem_size = value_byte_size(elem_type, id_size);
        if elem_size == 0 {
            return SubRecordAction::Break;
        }
        let byte_count = match num_elements.checked_mul(elem_size) {
            Some(n) => n,
            None => return SubRecordAction::Break,
        };
        let pos = cursor.position() as usize;
        if pos + byte_count > data.len() {
            return SubRecordAction::Break;
        }
        if arr_id == target_id {
            result = Some((elem_type, data[pos..pos + byte_count].to_vec()));
            return SubRecordAction::Break;
        }
        cursor.set_position((pos + byte_count) as u64);
        SubRecordAction::Consumed
    });
    result
}

fn scan_for_object_array_meta(
    data: &[u8],
    target_id: u64,
    id_size: u32,
    data_base_offset: u64,
) -> Option<ObjectArrayMeta> {
    let mut result = None;
    walk_heap_subrecords(data, id_size, |sub_tag, _tag_pos, cursor| {
        if sub_tag != HeapSubTag::ObjectArrayDump {
            return SubRecordAction::Continue;
        }
        let arr_id = match read_id(cursor, id_size) {
            Ok(id) => id,
            Err(_) => return SubRecordAction::Break,
        };
        let _serial = match cursor.read_u32::<BigEndian>() {
            Ok(v) => v,
            Err(_) => return SubRecordAction::Break,
        };
        let num_elements = match cursor.read_u32::<BigEndian>() {
            Ok(n) => n,
            Err(_) => return SubRecordAction::Break,
        };
        let class_id = match read_id(cursor, id_size) {
            Ok(id) => id,
            Err(_) => return SubRecordAction::Break,
        };
        let byte_count = match (num_elements as usize).checked_mul(id_size as usize) {
            Some(n) => n,
            None => return SubRecordAction::Break,
        };
        let pos = cursor.position() as usize;
        let elements_offset = data_base_offset + pos as u64;
        if elements_offset as usize + byte_count > data_base_offset as usize + data.len() {
            return SubRecordAction::Break;
        }
        if arr_id == target_id {
            result = Some(ObjectArrayMeta {
                class_id,
                num_elements,
                elements_offset,
            });
            return SubRecordAction::Break;
        }
        cursor.set_position((pos + byte_count) as u64);
        SubRecordAction::Consumed
    });
    result
}

fn skip_sub_record(cursor: &mut std::io::Cursor<&[u8]>, sub_tag: HeapSubTag, id_size: u32) -> bool {
    use crate::indexer::first_pass::{parse_class_dump, value_byte_size};
    use std::io::Cursor;

    fn skip_n(cursor: &mut Cursor<&[u8]>, n: usize) -> bool {
        let pos = cursor.position() as usize;
        let new_pos = pos.saturating_add(n);
        if new_pos > cursor.get_ref().len() {
            return false;
        }
        cursor.set_position(new_pos as u64);
        true
    }

    match sub_tag {
        HeapSubTag::GcRootJniGlobal | HeapSubTag::GcRootThreadBlock => {
            skip_n(cursor, id_size as usize)
        }
        HeapSubTag::GcRootJniLocal => skip_n(cursor, 2 * id_size as usize),
        HeapSubTag::GcRootJavaFrame
        | HeapSubTag::GcRootThreadObj
        | HeapSubTag::GcRootInternedString => skip_n(cursor, id_size as usize + 8),
        HeapSubTag::GcRootNativeStack => skip_n(cursor, id_size as usize + 8),
        HeapSubTag::GcRootStickyClass | HeapSubTag::GcRootMonitorUsed => {
            skip_n(cursor, id_size as usize + 4)
        }
        HeapSubTag::ClassDump => parse_class_dump(cursor, id_size).is_some(),
        HeapSubTag::InstanceDump => {
            // INSTANCE_DUMP: obj_id + stack_serial(4) + class_id + num_bytes(4) + data
            if read_id(cursor, id_size).is_err() {
                return false;
            }
            if cursor.read_u32::<BigEndian>().is_err() {
                return false;
            }
            if read_id(cursor, id_size).is_err() {
                return false;
            }
            let Ok(num_bytes) = cursor.read_u32::<BigEndian>() else {
                return false;
            };
            skip_n(cursor, num_bytes as usize)
        }
        HeapSubTag::ObjectArrayDump => {
            // OBJECT_ARRAY_DUMP: array_id + stack_serial(4) + num_elements(4) + class_id + elements
            if read_id(cursor, id_size).is_err() {
                return false;
            }
            if cursor.read_u32::<BigEndian>().is_err() {
                return false;
            }
            let Ok(num_elements) = cursor.read_u32::<BigEndian>() else {
                return false;
            };
            if read_id(cursor, id_size).is_err() {
                return false;
            }
            skip_n(cursor, num_elements as usize * id_size as usize)
        }
        HeapSubTag::PrimArrayDump => {
            // PRIMITIVE_ARRAY_DUMP: array_id + stack_serial(4) + num_elements(4) + elem_type(1) + data
            if read_id(cursor, id_size).is_err() {
                return false;
            }
            if cursor.read_u32::<BigEndian>().is_err() {
                return false;
            }
            let Ok(num_elements) = cursor.read_u32::<BigEndian>() else {
                return false;
            };
            let Ok(elem_type) = cursor.read_u8() else {
                return false;
            };
            let elem_size = value_byte_size(elem_type, id_size);
            if elem_size == 0 {
                return false;
            }
            skip_n(cursor, num_elements as usize * elem_size)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn from_path_non_existent_returns_mmap_failed() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let missing_path = tmp.path().to_path_buf();
        drop(tmp);
        let result = HprofFile::from_path(&missing_path);
        assert!(matches!(result, Err(HprofError::MmapFailed(_))));
    }

    #[test]
    fn from_path_truncated_record_returns_partial_with_warning() {
        // Valid header + incomplete record (tag only, missing time_offset+length)
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(b"JAVA PROFILE 1.0.2\0");
        bytes.extend_from_slice(&8u32.to_be_bytes());
        bytes.extend_from_slice(&0u64.to_be_bytes());
        bytes.push(0x01); // tag byte only — truncated mid-header

        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();

        let hfile = HprofFile::from_path(tmp.path()).unwrap(); // Ok, not Err
        assert!(!hfile.index_warnings.is_empty());
        assert!(hfile.index.strings.is_empty());
    }

    #[test]
    fn from_path_with_progress_on_valid_file_calls_observer() {
        use hprof_api::ParseProgressObserver;

        struct CountingObserver {
            call_count: usize,
            last_offset: Option<u64>,
        }
        impl ParseProgressObserver for CountingObserver {
            fn on_bytes_scanned(&mut self, position: u64) {
                self.call_count += 1;
                self.last_offset = Some(position);
            }
            fn on_segment_completed(&mut self, _done: usize, _total: usize) {}
            fn on_names_resolved(&mut self, _done: usize, _total: usize) {}
        }

        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(b"JAVA PROFILE 1.0.2\0");
        bytes.extend_from_slice(&8u32.to_be_bytes());
        bytes.extend_from_slice(&0u64.to_be_bytes());
        bytes.push(0x01); // tag
        bytes.extend_from_slice(&0u32.to_be_bytes());
        let id_bytes = 1u64.to_be_bytes();
        bytes.extend_from_slice(&(id_bytes.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&id_bytes);

        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();

        let mut obs = CountingObserver {
            call_count: 0,
            last_offset: None,
        };
        HprofFile::from_path_with_progress(tmp.path(), &mut obs, MemoryBudget::Unlimited).unwrap();
        assert!(obs.call_count >= 1, "observer must be called at least once");
        assert_eq!(
            obs.last_offset,
            Some(bytes.len() as u64),
            "should report the absolute file offset"
        );
    }

    #[test]
    fn from_path_on_valid_file_compiles_and_succeeds() {
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(b"JAVA PROFILE 1.0.2\0");
        bytes.extend_from_slice(&8u32.to_be_bytes());
        bytes.extend_from_slice(&0u64.to_be_bytes());

        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();

        let result = HprofFile::from_path(tmp.path());
        assert!(result.is_ok(), "from_path must succeed with no-op callback");
    }

    #[test]
    fn from_path_valid_file_parses_header() {
        use crate::HprofVersion;

        // Build a minimal valid hprof file (header only, no records)
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(b"JAVA PROFILE 1.0.2\0");
        bytes.extend_from_slice(&8u32.to_be_bytes()); // id_size
        bytes.extend_from_slice(&0u64.to_be_bytes()); // timestamp

        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();

        let hfile = HprofFile::from_path(tmp.path()).unwrap();
        assert_eq!(hfile.header.version, HprofVersion::V1_0_2);
        assert_eq!(hfile.header.id_size, 8);
        assert!(hfile.index.strings.is_empty());
        assert!(hfile.index_warnings.is_empty());
        assert_eq!(hfile.records_attempted, 0);
        assert_eq!(hfile.records_indexed, 0);
    }

    // ── walk_heap_subrecords tests ──

    #[test]
    fn walk_full_traversal_visits_all_sub_records() {
        let id_size: u32 = 8;
        let mut payload = Vec::new();

        // Instance A (obj_id=0xAA, class=100, data=[0x11])
        payload.push(0x21);
        payload.extend_from_slice(&0xAAu64.to_be_bytes());
        payload.extend_from_slice(&0u32.to_be_bytes());
        payload.extend_from_slice(&100u64.to_be_bytes());
        payload.extend_from_slice(&1u32.to_be_bytes());
        payload.push(0x11);

        // Instance B (obj_id=0xBB, class=200, data=[0x22])
        payload.push(0x21);
        payload.extend_from_slice(&0xBBu64.to_be_bytes());
        payload.extend_from_slice(&0u32.to_be_bytes());
        payload.extend_from_slice(&200u64.to_be_bytes());
        payload.extend_from_slice(&1u32.to_be_bytes());
        payload.push(0x22);

        let mut visited = Vec::new();
        walk_heap_subrecords(&payload, id_size, |sub_tag, tag_pos, _cursor| {
            visited.push((sub_tag, tag_pos));
            SubRecordAction::Continue
        });
        assert_eq!(visited.len(), 2);
        assert_eq!(visited[0].0, HeapSubTag::InstanceDump);
        assert_eq!(visited[0].1, 0);
        assert_eq!(visited[1].0, HeapSubTag::InstanceDump);
        assert_eq!(visited[1].1, 26); // 1+8+4+8+4+1 = 26
    }

    #[test]
    fn walk_break_stops_iteration() {
        let id_size: u32 = 8;
        let mut payload = Vec::new();

        for obj_id in [0xAAu64, 0xBBu64] {
            payload.push(0x21);
            payload.extend_from_slice(&obj_id.to_be_bytes());
            payload.extend_from_slice(&0u32.to_be_bytes());
            payload.extend_from_slice(&100u64.to_be_bytes());
            payload.extend_from_slice(&0u32.to_be_bytes());
        }

        let mut count = 0u32;
        walk_heap_subrecords(&payload, id_size, |_sub_tag, _tag_pos, _cursor| {
            count += 1;
            SubRecordAction::Break
        });
        assert_eq!(count, 1);
    }

    #[test]
    fn walk_truncated_sub_record_exits_silently() {
        let payload = vec![0x21, 0x00, 0x00];
        let mut count = 0u32;
        walk_heap_subrecords(&payload, 8, |_sub_tag, _tag_pos, _cursor| {
            count += 1;
            SubRecordAction::Continue
        });
        assert_eq!(count, 1);
    }

    #[test]
    fn walk_consumed_action_skips_auto_skip() {
        let id_size: u32 = 8;
        let mut payload = Vec::new();
        payload.push(0x21);
        payload.extend_from_slice(&0xAAu64.to_be_bytes());
        payload.extend_from_slice(&0u32.to_be_bytes());
        payload.extend_from_slice(&100u64.to_be_bytes());
        payload.extend_from_slice(&1u32.to_be_bytes());
        payload.push(0xFF);

        let mut visited = Vec::new();
        walk_heap_subrecords(&payload, id_size, |sub_tag, _tag_pos, cursor| {
            visited.push(sub_tag);
            let _ = read_id(cursor, id_size);
            let _ = cursor.read_u32::<BigEndian>();
            let _ = read_id(cursor, id_size);
            if let Ok(n) = cursor.read_u32::<BigEndian>() {
                let pos = cursor.position() as usize;
                cursor.set_position((pos + n as usize) as u64);
            }
            SubRecordAction::Consumed
        });
        assert_eq!(visited.len(), 1);
    }

    #[test]
    fn walk_empty_data_invokes_no_callbacks() {
        let mut count = 0u32;
        walk_heap_subrecords(&[], 8, |_sub_tag, _tag_pos, _cursor| {
            count += 1;
            SubRecordAction::Continue
        });
        assert_eq!(count, 0);
    }
}

#[cfg(all(test, feature = "test-utils"))]
mod builder_tests {
    use super::*;
    use crate::test_utils::HprofTestBuilder;
    use std::io::Write;

    #[test]
    fn find_instance_returns_some_for_known_object_id() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_instance(0xDEAD, 0, 100, &[1, 2, 3, 4])
            .build();
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();
        let hfile = HprofFile::from_path(tmp.path()).unwrap();
        let (raw, _offset) = hfile.find_instance(0xDEAD).expect("must find instance");
        assert_eq!(raw.class_object_id, 100);
        assert_eq!(raw.data, vec![1u8, 2, 3, 4]);
    }

    #[test]
    fn find_instance_returns_none_for_unknown_object_id() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_instance(0xDEAD, 0, 100, &[])
            .build();
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();
        let hfile = HprofFile::from_path(tmp.path()).unwrap();
        assert!(hfile.find_instance(0xBEEF).is_none());
    }

    #[test]
    fn find_instance_two_instances_returns_correct_one() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_instance(0x0001, 0, 10, &[0xAA])
            .add_instance(0x0002, 0, 20, &[0xBB])
            .build();
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();
        let hfile = HprofFile::from_path(tmp.path()).unwrap();
        let (r1, _) = hfile.find_instance(0x0001).unwrap();
        assert_eq!(r1.class_object_id, 10);
        assert_eq!(r1.data, vec![0xAAu8]);
        let (r2, _) = hfile.find_instance(0x0002).unwrap();
        assert_eq!(r2.class_object_id, 20);
        assert_eq!(r2.data, vec![0xBBu8]);
    }

    #[test]
    fn find_instance_non_empty_field_data_returns_correct_bytes() {
        let data = vec![0x01u8, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_instance(0xCAFE, 0, 42, &data)
            .build();
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();
        let hfile = HprofFile::from_path(tmp.path()).unwrap();
        let (raw, _) = hfile.find_instance(0xCAFE).unwrap();
        assert_eq!(raw.data, data);
    }

    #[test]
    fn hprof_file_has_records_start_field_and_records_bytes() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_string(1, "x")
            .build();
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();
        let hfile = HprofFile::from_path(tmp.path()).unwrap();
        // records_start must be > 0 (past the header)
        assert!(hfile.records_start > 0);
        // records_bytes() slice must be shorter than the full mmap
        // (it excludes the header)
        assert!(hfile.records_bytes().len() < bytes.len());
    }

    #[test]
    fn heap_record_ranges_populated_for_instance_dump() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_instance(0xDEAD, 0, 100, &[1, 2, 3, 4])
            .build();
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();
        let hfile = HprofFile::from_path(tmp.path()).unwrap();
        assert_eq!(hfile.heap_record_ranges.len(), 1);
    }

    #[test]
    fn find_prim_array_char_array_returns_elem_type_and_bytes() {
        // char[] (elem_type=5): 2 chars = 4 bytes
        let char_bytes = vec![0x00u8, 0x68, 0x00, 0x69]; // 'h', 'i' in UTF-16BE
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_prim_array(0xCAFE, 0, 2, 5, &char_bytes)
            .build();
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();
        let hfile = HprofFile::from_path(tmp.path()).unwrap();
        let result = hfile.find_prim_array(0xCAFE).expect("must find char array");
        assert_eq!(result.0, 5);
        assert_eq!(result.1, char_bytes);
    }

    #[test]
    fn find_prim_array_byte_array_returns_elem_type_and_bytes() {
        // byte[] (elem_type=8)
        let byte_data = vec![0x68u8, 0x69]; // 'h', 'i'
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_prim_array(0xBEEF, 0, 2, 8, &byte_data)
            .build();
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();
        let hfile = HprofFile::from_path(tmp.path()).unwrap();
        let result = hfile.find_prim_array(0xBEEF).expect("must find byte array");
        assert_eq!(result.0, 8);
        assert_eq!(result.1, byte_data);
    }

    #[test]
    fn find_prim_array_unknown_id_returns_none() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_prim_array(0xCAFE, 0, 1, 8, &[0x41])
            .build();
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();
        let hfile = HprofFile::from_path(tmp.path()).unwrap();
        assert!(hfile.find_prim_array(0xDEAD).is_none());
    }

    #[test]
    fn from_path_with_instance_produces_one_segment_filter() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_instance(0xDEAD, 0, 100, &[])
            .build();

        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();

        let hfile = HprofFile::from_path(tmp.path()).unwrap();
        assert_eq!(hfile.segment_filters.len(), 1);
    }

    #[test]
    fn from_path_with_string_record_indexed() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_string(99, "thread-main")
            .build();

        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();

        let hfile = HprofFile::from_path(tmp.path()).unwrap();
        assert_eq!(hfile.index.strings.len(), 1);
        let sref = &hfile.index.strings[&99];
        assert_eq!(hfile.resolve_string(sref), "thread-main");
        assert!(hfile.index_warnings.is_empty());
        assert_eq!(hfile.records_attempted, 1);
        assert_eq!(hfile.records_indexed, 1);
    }

    #[test]
    fn from_path_preloads_field_name_cache_with_specific_string_id_mapping() {
        let name_string_id = 101u64;
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_string(name_string_id, "count")
            .add_string(102, "com/example/Foo")
            .add_class(1, 0x100, 0, 102)
            .add_class_dump(0x100, 0, 4, &[(name_string_id, 10u8)])
            .add_instance(0x200, 0, 0x100, &42i32.to_be_bytes())
            .build();

        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();

        let hfile = HprofFile::from_path(tmp.path()).unwrap();

        assert!(
            hfile.index.field_names.contains_key(&name_string_id),
            "field_names must contain specific field name string id"
        );
        assert_eq!(
            hfile
                .index
                .field_names
                .get(&name_string_id)
                .map(String::as_str),
            Some("count")
        );
    }

    #[test]
    fn read_instance_at_offset_returns_correct_data() {
        let obj_id = 0xDEAD_u64;
        let class_id = 100_u64;
        let data = vec![1u8, 2, 3, 4];
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_root_thread_obj(obj_id, 1, 0)
            .add_instance(obj_id, 0, class_id, &data)
            .build();
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();
        let hfile = HprofFile::from_path(tmp.path()).unwrap();
        let offset = hfile
            .index
            .instance_offsets
            .get(obj_id)
            .expect("offset must be recorded");
        let raw = hfile
            .read_instance_at_offset(offset)
            .expect("must read instance");
        assert_eq!(raw.class_object_id, class_id);
        assert_eq!(raw.data, data);
    }

    // ── Task 1.5: batch 5 instances across 2+ segments ──

    #[test]
    fn batch_find_five_instances_returns_all_with_correct_data() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_instance(0x01, 0, 100, &[0xA1])
            .add_instance(0x02, 0, 200, &[0xA2])
            .add_instance(0x03, 0, 300, &[0xA3])
            .add_instance(0x04, 0, 400, &[0xA4])
            .add_instance(0x05, 0, 500, &[0xA5])
            .build();
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();
        let hfile = HprofFile::from_path(tmp.path()).unwrap();

        let result = hfile.batch_find_instances(&[0x01, 0x02, 0x03, 0x04, 0x05]);

        assert_eq!(result.instances.len(), 5);
        assert_eq!(result.offsets.len(), 5);
        assert_eq!(result.instances[&0x01].class_object_id, 100);
        assert_eq!(result.instances[&0x01].data, vec![0xA1]);
        assert_eq!(result.instances[&0x03].class_object_id, 300);
        assert_eq!(result.instances[&0x05].class_object_id, 500);
        assert_eq!(result.instances[&0x05].data, vec![0xA5]);
    }

    // ── Task 1.5b: truncated sub-record tolerance ──

    #[test]
    fn batch_find_tolerates_truncated_sub_record() {
        // Layout:
        //  raw_heap_segment A:
        //    [0xAA valid INSTANCE_DUMP (1 byte data)]
        //    [0xCC valid INSTANCE_DUMP (1 byte data)]
        //    [truncated INSTANCE_DUMP header: tag + 2 bytes
        //     of the 8-byte ID — scanner stops here because
        //     the partial header cannot be skipped]
        //  raw_heap_segment B (via add_instance):
        //    [0xBB valid INSTANCE_DUMP (1 byte data)]
        //
        // BOUNDARY NOTE: the truncation is a HEADER
        // truncation (partial object_id). The scanner
        // cannot determine record size from a partial
        // header, so it stops at that point.
        // 0xAA and 0xCC (before truncation) are found;
        // 0xBB (separate segment) is also found.
        // Any record that would appear after a truncated
        // BODY (where num_bytes > remaining slice bytes)
        // would also be unreachable — the body truncation
        // advances the cursor to the slice end, so the
        // next read_u8() terminates the loop cleanly.
        let id_size = 8u32;

        let mut payload = Vec::new();

        // Valid INSTANCE_DUMP for 0xAA (26 bytes)
        payload.push(0x21);
        payload.extend_from_slice(&0xAAu64.to_be_bytes()); // id
        payload.extend_from_slice(&0u32.to_be_bytes()); // serial
        payload.extend_from_slice(&100u64.to_be_bytes()); // class
        payload.extend_from_slice(&1u32.to_be_bytes()); // num_bytes
        payload.push(0xFF); // data

        // Valid INSTANCE_DUMP for 0xCC (26 bytes)
        payload.push(0x21);
        payload.extend_from_slice(&0xCCu64.to_be_bytes());
        payload.extend_from_slice(&0u32.to_be_bytes());
        payload.extend_from_slice(&150u64.to_be_bytes());
        payload.extend_from_slice(&1u32.to_be_bytes());
        payload.push(0xDD);

        // Truncated INSTANCE_DUMP: tag + only 2 bytes
        // of the 8-byte object ID — scanner stops here.
        payload.push(0x21);
        payload.extend_from_slice(&[0x00, 0x00]);

        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", id_size)
            .add_raw_heap_segment(&payload)
            .add_instance(0xBB, 0, 200, &[0xCC])
            .build();

        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();
        let hfile = HprofFile::from_path(tmp.path()).unwrap();

        let result = hfile.batch_find_instances(&[0xAA, 0xCC, 0xBB]);

        // 0xAA and 0xCC from first segment (before truncation)
        assert!(
            result.instances.contains_key(&0xAA),
            "valid instance before truncation must be found"
        );
        assert!(
            result.instances.contains_key(&0xCC),
            "second valid instance before truncation must be found"
        );
        // 0xBB from second segment
        assert!(
            result.instances.contains_key(&0xBB),
            "instance in separate segment must be found"
        );
        assert_eq!(result.instances[&0xAA].class_object_id, 100);
        assert_eq!(result.instances[&0xCC].class_object_id, 150);
        assert_eq!(result.instances[&0xBB].class_object_id, 200);
    }

    // ── Task 1.5c: false-positive dedup ──

    #[test]
    fn batch_find_deduplicates_across_ranges() {
        // Same ID (0xAA) in a single-segment file.
        // First heap segment has 0xAA, second does not.
        // The dedup logic must return 0xAA exactly once.
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_instance(0xAA, 0, 100, &[0x11])
            .add_instance(0xBB, 0, 200, &[0x22])
            .build();

        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();
        let hfile = HprofFile::from_path(tmp.path()).unwrap();

        let result = hfile.batch_find_instances(&[0xAA]);

        assert_eq!(result.instances.len(), 1, "ID must appear exactly once");
        assert_eq!(result.instances[&0xAA].class_object_id, 100);
        assert_eq!(result.instances[&0xAA].data, vec![0x11]);
    }

    // ── Task 1.6: non-existing IDs → empty map ──

    #[test]
    fn batch_find_nonexistent_ids_returns_empty() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_instance(0xDEAD, 0, 100, &[])
            .build();
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();
        let hfile = HprofFile::from_path(tmp.path()).unwrap();

        let result = hfile.batch_find_instances(&[0xBEEF, 0xCAFE]);

        assert!(result.instances.is_empty());
        assert!(result.offsets.is_empty());
    }

    // ── Task 1.7: mix of existing and non-existing ──

    #[test]
    fn batch_find_mix_existing_and_nonexistent() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_instance(0x01, 0, 100, &[0xAA])
            .add_instance(0x02, 0, 200, &[0xBB])
            .build();
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();
        let hfile = HprofFile::from_path(tmp.path()).unwrap();

        let result = hfile.batch_find_instances(&[0x01, 0xDEAD, 0x02]);

        assert_eq!(result.instances.len(), 2);
        assert!(result.instances.contains_key(&0x01));
        assert!(result.instances.contains_key(&0x02));
        assert!(!result.instances.contains_key(&0xDEAD));
    }

    // ── Task 1.8: single ID matches find_instance ──

    #[test]
    fn batch_find_single_id_matches_find_instance() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_instance(0xCAFE, 0, 42, &[1, 2, 3, 4])
            .build();
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();
        let hfile = HprofFile::from_path(tmp.path()).unwrap();

        let (single, single_off) = hfile.find_instance(0xCAFE).unwrap();
        let batch = hfile.batch_find_instances(&[0xCAFE]);
        let batch_inst = &batch.instances[&0xCAFE];

        assert_eq!(single.class_object_id, batch_inst.class_object_id,);
        assert_eq!(single.data, batch_inst.data);
        assert_eq!(
            single_off, batch.offsets[&0xCAFE],
            "find_instance and batch offsets must match"
        );
    }

    // ── Story 11.3 Task 2: Parallel completeness tests ──

    #[test]
    fn parallel_batch_correctness_small_segment_size() {
        // 10 instances with distinct IDs. segment_size=1024
        // exercises the Phase 2 range-overlap arithmetic with
        // a small window. NOTE: all instances land in a single
        // SegmentFilter (file is tiny), so par_iter dispatches
        // to one thread — this validates correctness, not K ≥ 2
        // parallelism. K ≥ 2 speedup requires a large multi-
        // segment dump and is validated manually (see AC #1 note).
        let mut builder = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8);
        for i in 1u64..=10 {
            builder = builder.add_instance(i, 0, i * 100, &[i as u8]);
        }
        let bytes = builder.build();
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();
        let hfile = HprofFile::from_path(tmp.path()).unwrap();

        let ids: Vec<u64> = (1..=10).collect();
        let result = hfile.batch_find_instances_inner(&ids, 1024);

        assert_eq!(result.instances.len(), 10, "all 10 instances must be found");
        for i in 1u64..=10 {
            let raw = &result.instances[&i];
            assert_eq!(
                raw.class_object_id,
                i * 100,
                "class_object_id mismatch for ID {i}"
            );
            assert_eq!(raw.data, vec![i as u8], "data mismatch for ID {i}");
        }
        assert_eq!(result.offsets.len(), 10);
    }

    #[test]
    fn parallel_batch_single_filter_returns_all_items() {
        // All IDs fall within 1 segment filter (default
        // segment_size). Verifies no regression on K=1.
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_instance(0x10, 0, 100, &[0xAA])
            .add_instance(0x20, 0, 200, &[0xBB])
            .add_instance(0x30, 0, 300, &[0xCC])
            .build();
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();
        let hfile = HprofFile::from_path(tmp.path()).unwrap();

        let result = hfile.batch_find_instances(&[0x10, 0x20, 0x30]);

        assert_eq!(result.instances.len(), 3);
        assert_eq!(result.instances[&0x10].class_object_id, 100);
        assert_eq!(result.instances[&0x20].class_object_id, 200);
        assert_eq!(result.instances[&0x30].class_object_id, 300);
    }

    #[test]
    fn parallel_batch_empty_slice_returns_empty() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_instance(0x01, 0, 100, &[0xAA])
            .build();
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();
        let hfile = HprofFile::from_path(tmp.path()).unwrap();

        let result = hfile.batch_find_instances(&[]);

        assert!(result.instances.is_empty());
        assert!(result.offsets.is_empty());
    }

    #[test]
    fn read_prim_array_at_offset_returns_correct_data() {
        let arr_id = 0xCAFE_u64;
        let elem_data = vec![0x00u8, 0x68, 0x00, 0x69];
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_prim_array(arr_id, 0, 2, 5, &elem_data)
            .build();
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();
        let hfile = HprofFile::from_path(tmp.path()).unwrap();
        // The prim array sub-record starts at the heap
        // payload.
        let payload_start = hfile.heap_record_ranges[0].payload_start;
        let (elem_type, result_data) = hfile
            .read_prim_array_at_offset(payload_start)
            .expect("must read prim array");
        assert_eq!(elem_type, 5);
        assert_eq!(result_data, elem_data);
    }

    // ── Story 11.4: ObjectArrayMeta + O(1) reads ──

    fn hfile_from_bytes(bytes: &[u8]) -> HprofFile {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(bytes).unwrap();
        tmp.flush().unwrap();
        HprofFile::from_path(tmp.path()).unwrap()
    }

    #[test]
    fn find_object_array_meta_id_size_8() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_object_array(0xA, 0, 0xCC, &[0x1, 0x2, 0x3])
            .build();
        let hfile = hfile_from_bytes(&bytes);

        let meta = hfile.find_object_array_meta(0xA).expect("must find meta");
        assert_eq!(meta.class_id, 0xCC);
        assert_eq!(meta.num_elements, 3);

        assert_eq!(hfile.read_object_array_element(&meta, 0), Some(0x1));
        assert_eq!(hfile.read_object_array_element(&meta, 1), Some(0x2));
        assert_eq!(hfile.read_object_array_element(&meta, 2), Some(0x3));
        assert_eq!(
            hfile.read_object_array_element(&meta, 3),
            None,
            "out of bounds"
        );

        assert!(hfile.find_object_array_meta(0xBEEF).is_none(), "unknown ID");
    }

    #[test]
    fn find_object_array_meta_id_size_4() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 4)
            .add_object_array(0xA, 0, 0xCC, &[0x1, 0x2, 0x3])
            .build();
        let hfile = hfile_from_bytes(&bytes);

        let meta = hfile.find_object_array_meta(0xA).expect("must find meta");
        assert_eq!(meta.class_id, 0xCC);
        assert_eq!(meta.num_elements, 3);

        assert_eq!(hfile.read_object_array_element(&meta, 0), Some(0x1));
        assert_eq!(hfile.read_object_array_element(&meta, 2), Some(0x3));
        assert_eq!(hfile.read_object_array_element(&meta, 3), None);
    }

    #[test]
    fn find_object_array_meta_empty_array() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_object_array(0xA, 0, 0xCC, &[])
            .build();
        let hfile = hfile_from_bytes(&bytes);

        let meta = hfile.find_object_array_meta(0xA).expect("must find meta");
        assert_eq!(meta.num_elements, 0);
        assert_eq!(hfile.read_object_array_element(&meta, 0), None);
    }

    #[test]
    fn find_object_array_meta_skips_preceding_sub_records() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_instance(0xFF, 0, 100, &[0xDE, 0xAD])
            .add_object_array(0xA, 0, 0xCC, &[0x42])
            .build();
        let hfile = hfile_from_bytes(&bytes);

        let meta = hfile.find_object_array_meta(0xA).expect("must find meta");
        assert_eq!(meta.num_elements, 1);
        assert_eq!(hfile.read_object_array_element(&meta, 0), Some(0x42));
    }

    #[test]
    fn find_object_array_meta_truncated_returns_none() {
        // Build a valid 3-element array, then truncate
        // so num_elements claims 3 but only 1 element
        // fits in the payload.
        let id_size = 8u32;
        let mut payload = Vec::new();
        payload.push(0x22u8); // OBJECT_ARRAY_DUMP
        payload.extend_from_slice(&0xAu64.to_be_bytes()); // array_id
        payload.extend_from_slice(&0u32.to_be_bytes()); // stack_serial
        payload.extend_from_slice(&3u32.to_be_bytes()); // num_elements = 3
        payload.extend_from_slice(&0xCCu64.to_be_bytes()); // class_id
        // Only 1 element (8 bytes) instead of 3 (24)
        payload.extend_from_slice(&0x1u64.to_be_bytes());

        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", id_size)
            .add_raw_heap_segment(&payload)
            .build();
        let hfile = hfile_from_bytes(&bytes);

        assert!(
            hfile.find_object_array_meta(0xA).is_none(),
            "truncated array must return None"
        );
    }

    // ── Story 11.6 Task 1.4: find_instance returns offset ──

    #[test]
    fn find_instance_returns_valid_offset() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_instance(0xDEAD, 0, 100, &[1, 2, 3, 4])
            .build();
        let hfile = hfile_from_bytes(&bytes);
        let (raw, offset) = hfile.find_instance(0xDEAD).unwrap();
        assert_eq!(raw.class_object_id, 100);

        // Offset must point to a valid INSTANCE_DUMP
        let re_read = hfile
            .read_instance_at_offset(offset)
            .expect("offset must point to valid record");
        assert_eq!(re_read.class_object_id, 100);
        assert_eq!(re_read.data, vec![1u8, 2, 3, 4]);
    }

    #[test]
    fn find_object_array_composition_matches_original() {
        let elements = vec![0x10u64, 0x20, 0x30];
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_object_array(0xA, 0, 100, &elements)
            .build();
        let hfile = hfile_from_bytes(&bytes);

        let (class_id, elems) = hfile.find_object_array(0xA).expect("composition must work");
        assert_eq!(class_id, 100);
        assert_eq!(elems, elements);
    }
}
