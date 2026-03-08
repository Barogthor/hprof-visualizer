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

use std::path::Path;

use byteorder::{BigEndian, ReadBytesExt};
use memmap2::Mmap;

use crate::indexer::{first_pass::run_first_pass, precise::PreciseIndex, segment::SegmentFilter};
use crate::{HprofError, HprofHeader, RawInstance, open_readonly, parse_header, read_id};

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
/// The internal `_mmap` field keeps the memory mapping alive for the duration
/// of this struct's lifetime. It must not be dropped early.
#[derive(Debug)]
pub struct HprofFile {
    /// Keeps the memory mapping alive — must not be removed.
    _mmap: Mmap,
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
    /// `(payload_start, payload_length)` for every HEAP_DUMP / HEAP_DUMP_SEGMENT record.
    /// Offsets are relative to the records section (`records_bytes()`).
    pub heap_record_ranges: Vec<(u64, u64)>,
}

impl HprofFile {
    /// Opens `path` as a read-only mmap, parses the header, and indexes all
    /// structural records, calling `progress_fn` with the current byte offset
    /// every [`PROGRESS_REPORT_INTERVAL`] bytes and once after the final record.
    ///
    /// Truncated or corrupted records are non-fatal: they are collected in
    /// [`HprofFile::index_warnings`] and indexing continues where possible.
    ///
    /// The byte offset passed to `progress_fn` is an absolute file offset from
    /// the beginning of the file (including the header).
    ///
    /// ## Errors
    /// - [`HprofError::MmapFailed`] — file not found or OS mapping failed.
    /// - [`HprofError::UnsupportedVersion`] — unrecognised hprof version string.
    /// - [`HprofError::TruncatedRecord`] — file header is truncated.
    ///
    /// ## Progress
    /// `progress_fn(bytes)` — absolute file offset, called every 4 MiB or
    /// once per second during the scan. Segment filters are built inline
    /// during the scan so no separate filter phase callback is needed.
    pub fn from_path_with_progress(
        path: &Path,
        mut progress_fn: impl FnMut(u64),
    ) -> Result<Self, HprofError> {
        let mmap = open_readonly(path)?;
        let header = parse_header(&mmap)?;
        let records_start = header_end(&mmap)?;
        let base_offset = records_start as u64;
        let result = run_first_pass(&mmap[records_start..], header.id_size, |bytes| {
            progress_fn(base_offset.saturating_add(bytes))
        });
        Ok(Self {
            _mmap: mmap,
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

    /// Opens `path` and indexes it without a progress callback.
    ///
    /// Convenience wrapper around [`HprofFile::from_path_with_progress`].
    ///
    /// ## Errors
    /// - [`HprofError::MmapFailed`] — file not found or OS mapping failed.
    /// - [`HprofError::UnsupportedVersion`] — unrecognised hprof version string.
    /// - [`HprofError::TruncatedRecord`] — file header is truncated.
    pub fn from_path(path: &Path) -> Result<Self, HprofError> {
        Self::from_path_with_progress(path, |_| {})
    }

    /// Returns the raw bytes of the records section (immediately after the
    /// file header).
    pub fn records_bytes(&self) -> &[u8] {
        &self._mmap[self.records_start..]
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

        for &(payload_start, payload_len) in &self.heap_record_ranges {
            let payload_end = payload_start + payload_len;

            let overlaps = candidate_segs.iter().any(|&seg| {
                let seg_start = seg as u64 * SEGMENT_SIZE as u64;
                let seg_end = seg_start + SEGMENT_SIZE as u64;
                payload_start < seg_end && payload_end > seg_start
            });

            if !overlaps {
                continue;
            }

            let start = payload_start as usize;
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
        let sub_tag = cursor.read_u8().ok()?;
        if sub_tag != 0x21 {
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
        let sub_tag = cursor.read_u8().ok()?;
        if sub_tag != 0x23 {
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
    pub fn find_instance(&self, object_id: u64) -> Option<RawInstance> {
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

        for &(payload_start, payload_len) in &self.heap_record_ranges {
            let payload_end = payload_start + payload_len;

            let overlaps = candidate_segs.iter().any(|&seg| {
                let seg_start = seg as u64 * SEGMENT_SIZE as u64;
                let seg_end = seg_start + SEGMENT_SIZE as u64;
                payload_start < seg_end && payload_end > seg_start
            });

            if !overlaps {
                continue;
            }

            let start = payload_start as usize;
            let end = (payload_end as usize).min(records.len());
            if start >= records.len() {
                continue;
            }

            if let Some(raw) = scan_for_instance(&records[start..end], object_id, id_size) {
                return Some(raw);
            }
        }

        None
    }
}

fn scan_for_instance(data: &[u8], target_id: u64, id_size: u32) -> Option<RawInstance> {
    use std::io::Cursor;

    let mut cursor = Cursor::new(data);
    loop {
        let sub_tag = match cursor.read_u8() {
            Ok(t) => t,
            Err(_) => return None,
        };
        match sub_tag {
            0x21 => {
                let obj_id = match read_id(&mut cursor, id_size) {
                    Ok(id) => id,
                    Err(_) => return None,
                };
                let _stack_serial = match cursor.read_u32::<BigEndian>() {
                    Ok(v) => v,
                    Err(_) => return None,
                };
                let class_object_id = match read_id(&mut cursor, id_size) {
                    Ok(id) => id,
                    Err(_) => return None,
                };
                let num_bytes = match cursor.read_u32::<BigEndian>() {
                    Ok(n) => n as usize,
                    Err(_) => return None,
                };
                let pos = cursor.position() as usize;
                if pos + num_bytes > data.len() {
                    return None;
                }
                if obj_id == target_id {
                    return Some(RawInstance {
                        class_object_id,
                        data: data[pos..pos + num_bytes].to_vec(),
                    });
                }
                cursor.set_position((pos + num_bytes) as u64);
            }
            _ => {
                if !skip_sub_record(&mut cursor, sub_tag, id_size) {
                    return None;
                }
            }
        }
    }
}

fn scan_for_prim_array(data: &[u8], target_id: u64, id_size: u32) -> Option<(u8, Vec<u8>)> {
    use std::io::Cursor;

    let mut cursor = Cursor::new(data);
    loop {
        let sub_tag = match cursor.read_u8() {
            Ok(t) => t,
            Err(_) => return None,
        };
        if sub_tag == 0x23 {
            let arr_id = match read_id(&mut cursor, id_size) {
                Ok(id) => id,
                Err(_) => return None,
            };
            let _stack_serial = match cursor.read_u32::<BigEndian>() {
                Ok(v) => v,
                Err(_) => return None,
            };
            let num_elements = match cursor.read_u32::<BigEndian>() {
                Ok(n) => n as usize,
                Err(_) => return None,
            };
            let elem_type = match cursor.read_u8() {
                Ok(t) => t,
                Err(_) => return None,
            };
            let elem_size = {
                use crate::indexer::first_pass::value_byte_size;
                value_byte_size(elem_type, id_size)
            };
            if elem_size == 0 {
                return None;
            }
            let byte_count = num_elements.checked_mul(elem_size)?;
            let pos = cursor.position() as usize;
            if pos + byte_count > data.len() {
                return None;
            }
            if arr_id == target_id {
                return Some((elem_type, data[pos..pos + byte_count].to_vec()));
            }
            cursor.set_position((pos + byte_count) as u64);
        } else if !skip_sub_record(&mut cursor, sub_tag, id_size) {
            return None;
        }
    }
}

fn skip_sub_record(cursor: &mut std::io::Cursor<&[u8]>, sub_tag: u8, id_size: u32) -> bool {
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
        0x01 => skip_n(cursor, id_size as usize),
        0x02 => skip_n(cursor, 2 * id_size as usize),
        0x03 => skip_n(cursor, id_size as usize + 8),
        0x04 => skip_n(cursor, id_size as usize + 8),
        0x05 => skip_n(cursor, id_size as usize + 4),
        0x06 => skip_n(cursor, id_size as usize),
        0x07 => skip_n(cursor, id_size as usize + 4),
        0x08 => skip_n(cursor, id_size as usize + 8),
        0x09 => skip_n(cursor, id_size as usize + 8),
        0x20 => parse_class_dump(cursor, id_size).is_some(),
        0x21 => {
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
        0x22 => {
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
        0x23 => {
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

/// Returns the byte offset of the first record in `data`.
///
/// Scans for the null terminator of the version string, then skips
/// `id_size` (u32, 4 bytes) and timestamp (u64, 8 bytes).
///
/// ## Errors
/// - [`HprofError::TruncatedRecord`] if no null byte is found.
fn header_end(data: &[u8]) -> Result<usize, HprofError> {
    let null_pos = data
        .iter()
        .position(|&b| b == 0)
        .ok_or(HprofError::TruncatedRecord)?;
    Ok(null_pos + 1 + 4 + 8) // null-term + id_size(u32) + timestamp(u64)
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
    fn from_path_with_progress_on_valid_file_calls_callback_at_least_once() {
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(b"JAVA PROFILE 1.0.2\0");
        bytes.extend_from_slice(&8u32.to_be_bytes());
        bytes.extend_from_slice(&0u64.to_be_bytes());
        // Add one string record so the records section is non-empty.
        bytes.push(0x01); // tag
        bytes.extend_from_slice(&0u32.to_be_bytes()); // time_offset
        let id_bytes = 1u64.to_be_bytes();
        bytes.extend_from_slice(&(id_bytes.len() as u32).to_be_bytes()); // length
        bytes.extend_from_slice(&id_bytes); // payload

        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();

        let mut call_count = 0usize;
        let mut last_offset = None;
        HprofFile::from_path_with_progress(tmp.path(), |b| {
            call_count += 1;
            last_offset = Some(b);
        })
        .unwrap();
        assert!(
            call_count >= 1,
            "progress callback must be called at least once"
        );
        assert_eq!(
            last_offset,
            Some(bytes.len() as u64),
            "a callback should report the absolute file offset"
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
        let raw = hfile.find_instance(0xDEAD).expect("must find instance");
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
        let r1 = hfile.find_instance(0x0001).unwrap();
        assert_eq!(r1.class_object_id, 10);
        assert_eq!(r1.data, vec![0xAAu8]);
        let r2 = hfile.find_instance(0x0002).unwrap();
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
        let raw = hfile.find_instance(0xCAFE).unwrap();
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
        assert_eq!(hfile.index.strings[&99].value, "thread-main");
        assert!(hfile.index_warnings.is_empty());
        assert_eq!(hfile.records_attempted, 1);
        assert_eq!(hfile.records_indexed, 1);
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
        let offset = *hfile
            .index
            .instance_offsets
            .get(&obj_id)
            .expect("offset must be recorded");
        let raw = hfile
            .read_instance_at_offset(offset)
            .expect("must read instance");
        assert_eq!(raw.class_object_id, class_id);
        assert_eq!(raw.data, data);
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
        // The prim array sub-record starts at the heap payload.
        // heap_record_ranges[0].0 is the payload_start offset.
        let payload_start = hfile.heap_record_ranges[0].0;
        let (elem_type, result_data) = hfile
            .read_prim_array_at_offset(payload_start)
            .expect("must read prim array");
        assert_eq!(elem_type, 5);
        assert_eq!(result_data, elem_data);
    }
}
