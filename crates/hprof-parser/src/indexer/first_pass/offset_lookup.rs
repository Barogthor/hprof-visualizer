//! Filter-based object offset lookup using segment
//! entry points and BinaryFuse8 filters.
//!
//! Replaces the former `all_offsets: Vec<ObjectOffset>`
//! approach with batched segment scans bounded to 64 MiB
//! windows.

use std::collections::HashSet;
use std::io::Cursor;

use byteorder::{BigEndian, ReadBytesExt};
use rustc_hash::FxHashMap;

use super::hprof_primitives::{
    gc_root_skip_size, parse_class_dump, primitive_element_size, skip_n,
};
use crate::id::IdSize;
use crate::indexer::segment::{SEGMENT_SIZE, SegmentFilter};
use crate::read_id;
use crate::tags::HeapSubTag;

/// Marks the byte position of the first sub-record tag
/// at or after a [`SEGMENT_SIZE`] boundary.
///
/// Used by [`batch_lookup_by_filter`] to jump directly
/// into the correct 64 MiB window instead of parsing
/// from the start of a multi-GB HEAP_DUMP_SEGMENT.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SegmentEntryPoint {
    /// Zero-based segment index
    /// (`byte_position / SEGMENT_SIZE`).
    pub segment_index: usize,
    /// Absolute byte position (in the records data
    /// section) of the tag byte of the first sub-record
    /// at or after `segment_index * SEGMENT_SIZE`.
    pub scan_offset: usize,
}

/// Tracks segment boundary crossings during heap
/// extraction and emits [`SegmentEntryPoint`] entries.
pub(super) struct EntryPointTracker {
    prev_seg: Option<usize>,
    entry_points: Vec<SegmentEntryPoint>,
}

impl EntryPointTracker {
    pub(super) fn new() -> Self {
        Self {
            prev_seg: None,
            entry_points: Vec::new(),
        }
    }

    /// Records an entry point if `tag_pos` crosses into
    /// a new [`SEGMENT_SIZE`] segment.
    pub(super) fn track(&mut self, tag_pos: usize) {
        let seg = tag_pos / SEGMENT_SIZE;
        if self.prev_seg != Some(seg) {
            self.entry_points.push(SegmentEntryPoint {
                segment_index: seg,
                scan_offset: tag_pos,
            });
            self.prev_seg = Some(seg);
        }
    }

    pub(super) fn finish(self) -> Vec<SegmentEntryPoint> {
        self.entry_points
    }
}

/// Scans sub-records in `data[scan_offset..scan_end]`
/// for INSTANCE_DUMP or PRIM_ARRAY_DUMP whose
/// `object_id` is in `target_ids`.
///
/// Returns `(object_id, tag_byte_offset)` pairs.
/// `scan_end` is clamped to `data.len()`.
pub(crate) fn scan_segment_for_objects(
    data: &[u8],
    scan_offset: usize,
    scan_end: usize,
    id_size: IdSize,
    target_ids: &HashSet<u64>,
) -> Vec<(u64, u64)> {
    let end = scan_end.min(data.len());
    if scan_offset >= end {
        return Vec::new();
    }
    let slice = &data[scan_offset..end];
    let mut cursor = Cursor::new(slice);
    let mut results = Vec::new();

    while let Ok(raw) = cursor.read_u8() {
        let tag_pos = scan_offset + cursor.position() as usize - 1;
        let sub_tag = HeapSubTag::from(raw);

        match sub_tag {
            HeapSubTag::InstanceDump => {
                let Ok(obj_id) = read_id(&mut cursor, id_size) else {
                    break;
                };
                if target_ids.contains(&obj_id) {
                    results.push((obj_id, tag_pos as u64));
                }
                // skip: stack_serial(4) + class_id(id) +
                //   num_bytes(4) + field_data
                let Ok(_) = cursor.read_u32::<BigEndian>() else {
                    break;
                };
                let Ok(_) = read_id(&mut cursor, id_size) else {
                    break;
                };
                let Ok(num_bytes) = cursor.read_u32::<BigEndian>() else {
                    break;
                };
                if !skip_n(&mut cursor, num_bytes as usize) {
                    break;
                }
            }
            HeapSubTag::PrimArrayDump => {
                let Ok(arr_id) = read_id(&mut cursor, id_size) else {
                    break;
                };
                if target_ids.contains(&arr_id) {
                    results.push((arr_id, tag_pos as u64));
                }
                // skip: stack_serial(4) +
                //   num_elements(4) + elem_type(1) + data
                let Ok(_) = cursor.read_u32::<BigEndian>() else {
                    break;
                };
                let Ok(num_elements) = cursor.read_u32::<BigEndian>() else {
                    break;
                };
                let Ok(elem_type) = cursor.read_u8() else {
                    break;
                };
                let elem_size = primitive_element_size(elem_type);
                if elem_size == 0 {
                    break;
                }
                if !skip_n(&mut cursor, num_elements as usize * elem_size) {
                    break;
                }
            }
            HeapSubTag::ObjectArrayDump => {
                // Read arr_id, skip rest.
                // Thread objects are never
                // OBJECT_ARRAY instances.
                let Ok(_arr_id) = read_id(&mut cursor, id_size) else {
                    break;
                };
                let Ok(_) = cursor.read_u32::<BigEndian>() else {
                    break;
                };
                let Ok(num_elements) = cursor.read_u32::<BigEndian>() else {
                    break;
                };
                let Ok(_) = read_id(&mut cursor, id_size) else {
                    break;
                };
                if !skip_n(&mut cursor, num_elements as usize * id_size.as_usize()) {
                    break;
                }
            }
            HeapSubTag::ClassDump => {
                if parse_class_dump(&mut cursor, id_size).is_none() {
                    break;
                }
            }
            t if gc_root_skip_size(t, id_size).is_some() => {
                if !skip_n(&mut cursor, gc_root_skip_size(t, id_size).unwrap()) {
                    break;
                }
            }
            _ => break,
        }
    }

    results
}

/// Performs batched filter-based object lookups across
/// all segments.
///
/// For each filter, checks if any `target_ids` might
/// be in that segment (via BinaryFuse8). If so, scans
/// each HEAP_DUMP_SEGMENT payload that overlaps the
/// 64 MiB window using entry points.
///
/// Returns `(result_map, warnings)`. Callers must pass
/// warnings to `ctx.push_warning()`.
pub(crate) fn batch_lookup_by_filter(
    filters: &[SegmentFilter],
    entry_points: &[SegmentEntryPoint],
    data: &[u8],
    id_size: IdSize,
    target_ids: &HashSet<u64>,
    heap_ranges: &[crate::indexer::HeapRecordRange],
) -> (FxHashMap<u64, u64>, Vec<String>) {
    let mut result_map: FxHashMap<u64, u64> = FxHashMap::default();
    let mut warnings: Vec<String> = Vec::new();

    if target_ids.is_empty() {
        return (result_map, warnings);
    }

    // Assert filters are sorted by segment_index
    // (ADR-4: non-monotonic = unrecoverable data error).
    for w in filters.windows(2) {
        assert!(
            w[0].segment_index <= w[1].segment_index,
            "segment filters not sorted: {} > {}",
            w[0].segment_index,
            w[1].segment_index,
        );
    }

    for filter in filters {
        // Collect target IDs that pass this filter
        let candidates: HashSet<u64> = target_ids
            .iter()
            .copied()
            .filter(|id| !result_map.contains_key(id) && filter.contains(*id))
            .collect();

        if candidates.is_empty() {
            continue;
        }

        // Find entry point via binary search
        let ep_idx =
            entry_points.binary_search_by_key(&filter.segment_index, |ep| ep.segment_index);
        let scan_start = match ep_idx {
            Ok(i) => entry_points[i].scan_offset,
            Err(_) => {
                warnings.push(format!(
                    "no entry point for segment {}; \
                     skipping filter lookup",
                    filter.segment_index,
                ));
                continue;
            }
        };

        let scan_end = (filter.segment_index + 1) * SEGMENT_SIZE;

        // Scan each HEAP_DUMP_SEGMENT payload that
        // overlaps [scan_start, scan_end).
        for hr in heap_ranges {
            let ps = hr.payload_start as usize;
            let pe = ps + hr.payload_length as usize;
            // Skip ranges entirely outside the window
            if pe <= scan_start || ps >= scan_end {
                continue;
            }
            // Clamp to scan window
            let from = scan_start.max(ps);
            let to = scan_end.min(pe);

            let found = scan_segment_for_objects(data, from, to, id_size, &candidates);
            for (id, offset) in found {
                result_map.insert(id, offset);
            }
        }
    }

    // Warn about target IDs not found
    for id in target_ids {
        if !result_map.contains_key(id) {
            warnings.push(format!(
                "object ID 0x{id:X} not found in any \
                 segment filter"
            ));
        }
    }

    (result_map, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexer::HeapRecordRange;
    use crate::indexer::segment::SegmentFilterBuilder;

    /// Creates a single HeapRecordRange covering
    /// [0, data_len).
    fn single_range(data_len: usize) -> Vec<HeapRecordRange> {
        vec![HeapRecordRange {
            payload_start: 0,
            payload_length: data_len as u64,
        }]
    }

    /// Builds raw sub-record bytes for an
    /// INSTANCE_DUMP (sub-tag 0x21).
    fn make_instance_sub(obj_id: u64, class_id: u64, id_size: IdSize) -> Vec<u8> {
        let sz = id_size.as_usize();
        let mut buf = vec![0x21u8];
        buf.extend_from_slice(&obj_id.to_be_bytes()[8 - sz..]);
        buf.extend_from_slice(&0u32.to_be_bytes());
        buf.extend_from_slice(&class_id.to_be_bytes()[8 - sz..]);
        buf.extend_from_slice(&0u32.to_be_bytes());
        buf
    }

    /// Builds a SegmentFilter from raw IDs for
    /// segment 0.
    fn build_filter_seg0(ids: &[u64]) -> Vec<SegmentFilter> {
        let mut builder = SegmentFilterBuilder::new();
        for &id in ids {
            builder.add(0, id);
        }
        let (filters, _) = builder.finish();
        filters
    }

    // ── 2.4: batch_lookup finds correct offsets ──

    #[test]
    fn batch_lookup_finds_known_objects() {
        let id_size = IdSize::Eight;
        let mut data = Vec::new();
        data.extend(make_instance_sub(0xA, 100, id_size));
        let pos_b = data.len();
        data.extend(make_instance_sub(0xB, 100, id_size));

        let filters = build_filter_seg0(&[0xA, 0xB]);
        let ep = vec![SegmentEntryPoint {
            segment_index: 0,
            scan_offset: 0,
        }];

        let target: HashSet<u64> = [0xA, 0xB].into_iter().collect();
        let ranges = single_range(data.len());
        let (found, _) = batch_lookup_by_filter(&filters, &ep, &data, id_size, &target, &ranges);

        assert_eq!(found.get(&0xA), Some(&0), "0xA at offset 0");
        assert_eq!(
            found.get(&0xB),
            Some(&(pos_b as u64)),
            "0xB at offset {pos_b}"
        );
    }

    // ── 2.5: target ID not in any segment ──

    #[test]
    fn batch_lookup_missing_id_absent_from_result() {
        let id_size = IdSize::Eight;
        let data = make_instance_sub(0xA, 100, id_size);
        let filters = build_filter_seg0(&[0xA]);
        let ep = vec![SegmentEntryPoint {
            segment_index: 0,
            scan_offset: 0,
        }];

        let target: HashSet<u64> = [0xDEAD].into_iter().collect();
        let ranges = single_range(data.len());
        let (found, warns) =
            batch_lookup_by_filter(&filters, &ep, &data, id_size, &target, &ranges);

        assert!(!found.contains_key(&0xDEAD), "0xDEAD should not be found");
        assert!(
            warns.iter().any(|w| w.contains("0xDEAD")),
            "must warn about missing ID"
        );
    }

    // ── 2.6: multiple target IDs in same segment ──

    #[test]
    fn batch_lookup_multiple_ids_same_segment() {
        let id_size = IdSize::Eight;
        let mut data = Vec::new();
        data.extend(make_instance_sub(1, 100, id_size));
        data.extend(make_instance_sub(2, 100, id_size));
        data.extend(make_instance_sub(3, 100, id_size));

        let filters = build_filter_seg0(&[1, 2, 3]);
        let ep = vec![SegmentEntryPoint {
            segment_index: 0,
            scan_offset: 0,
        }];

        let target: HashSet<u64> = [1, 2, 3].into_iter().collect();
        let ranges = single_range(data.len());
        let (found, _) = batch_lookup_by_filter(&filters, &ep, &data, id_size, &target, &ranges);

        assert_eq!(found.len(), 3, "all 3 objects must be found");
    }

    // ── scan_segment_for_objects unit tests ──

    #[test]
    fn scan_finds_instance_dump() {
        let id_size = IdSize::Eight;
        let data = make_instance_sub(42, 100, id_size);

        let target: HashSet<u64> = [42].into_iter().collect();
        let found = scan_segment_for_objects(&data, 0, data.len(), id_size, &target);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, 42);
        assert_eq!(found[0].1, 0);
    }

    #[test]
    fn scan_finds_prim_array_dump() {
        let id_size = IdSize::Eight;
        let mut data = Vec::new();
        data.push(0x23); // PRIM_ARRAY_DUMP
        data.extend_from_slice(&99u64.to_be_bytes());
        data.extend_from_slice(&0u32.to_be_bytes());
        data.extend_from_slice(&2u32.to_be_bytes());
        data.push(10); // PRIM_TYPE_INT
        data.extend_from_slice(&[0u8; 8]);

        let target: HashSet<u64> = [99].into_iter().collect();
        let found = scan_segment_for_objects(&data, 0, data.len(), id_size, &target);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, 99);
    }

    #[test]
    fn scan_skips_non_target_objects() {
        let id_size = IdSize::Eight;
        let data = make_instance_sub(1, 100, id_size);

        let target: HashSet<u64> = [999].into_iter().collect();
        let found = scan_segment_for_objects(&data, 0, data.len(), id_size, &target);

        assert!(found.is_empty());
    }
}
