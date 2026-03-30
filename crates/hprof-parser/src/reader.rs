//! Centralized binary reader for hprof data.
//!
//! [`RecordReader`] wraps a `Cursor<&[u8]>` with an
//! [`IdSize`] context, providing typed read methods
//! for all hprof primitive types. All parsing in the
//! crate goes through this struct — callers never
//! manipulate raw cursors or pass `id_size` around.

use byteorder::{BigEndian, ReadBytesExt};
use std::io::Cursor;

use crate::id::IdSize;
use crate::HprofError;

/// Wraps a cursor over raw bytes with id_size context.
pub struct RecordReader<'a> {
    cursor: Cursor<&'a [u8]>,
    id_size: IdSize,
}

impl<'a> RecordReader<'a> {
    /// Creates a new reader over `data` with the given
    /// `id_size`.
    pub fn new(data: &'a [u8], id_size: IdSize) -> Self {
        Self {
            cursor: Cursor::new(data),
            id_size,
        }
    }

    /// Returns the id_size.
    pub fn id_size(&self) -> IdSize {
        self.id_size
    }

    /// Returns the current byte position.
    pub fn position(&self) -> u64 {
        self.cursor.position()
    }

    /// Sets the current byte position.
    pub fn set_position(&mut self, pos: u64) {
        self.cursor.set_position(pos);
    }

    /// Returns the number of bytes remaining.
    pub fn remaining(&self) -> u64 {
        let len = self.cursor.get_ref().len() as u64;
        len.saturating_sub(self.cursor.position())
    }

    /// Reads a `u8`.
    pub fn read_u8(&mut self) -> Option<u8> {
        self.cursor.read_u8().ok()
    }

    /// Reads a big-endian `u16`.
    pub fn read_u16(&mut self) -> Option<u16> {
        self.cursor.read_u16::<BigEndian>().ok()
    }

    /// Reads a big-endian `u32`.
    pub fn read_u32(&mut self) -> Option<u32> {
        self.cursor.read_u32::<BigEndian>().ok()
    }

    /// Reads a big-endian `u64`.
    pub fn read_u64(&mut self) -> Option<u64> {
        self.cursor.read_u64::<BigEndian>().ok()
    }

    /// Reads a big-endian `i32`.
    pub fn read_i32(&mut self) -> Option<i32> {
        self.cursor.read_i32::<BigEndian>().ok()
    }

    /// Reads an object ID (4 or 8 bytes depending on
    /// `id_size`), returned as `u64`.
    pub fn read_id(&mut self) -> Option<u64> {
        self.read_id_result().ok()
    }

    /// Reads an object ID, returning a typed error on
    /// truncation. Use this when the caller needs to
    /// produce a detailed warning message.
    pub fn read_id_result(
        &mut self,
    ) -> Result<u64, HprofError> {
        match self.id_size {
            IdSize::Four => self
                .cursor
                .read_u32::<BigEndian>()
                .map(|v| v as u64)
                .map_err(|_| HprofError::TruncatedRecord),
            IdSize::Eight => self
                .cursor
                .read_u64::<BigEndian>()
                .map_err(|_| HprofError::TruncatedRecord),
        }
    }

    /// Advances the cursor by `n` bytes. Returns `false`
    /// if out of bounds (cursor unchanged).
    pub fn skip(&mut self, n: usize) -> bool {
        let pos = self.cursor.position() as usize;
        let new_pos = match pos.checked_add(n) {
            Some(p) => p,
            None => return false,
        };
        if new_pos > self.cursor.get_ref().len() {
            return false;
        }
        self.cursor.set_position(new_pos as u64);
        true
    }

    /// Returns a zero-copy slice of `n` bytes at the
    /// current position, advancing the cursor.
    /// Returns `None` if fewer than `n` bytes remain.
    pub fn read_bytes(
        &mut self,
        n: usize,
    ) -> Option<&'a [u8]> {
        let pos = self.cursor.position() as usize;
        let end = pos.checked_add(n)?;
        let data = *self.cursor.get_ref();
        if end > data.len() {
            return None;
        }
        self.cursor.set_position(end as u64);
        Some(&data[pos..end])
    }

    // -- top-level record parsing methods --

    /// Reads a record header: tag (`u8`), time offset
    /// (`u32`, discarded), and payload length (`u32`).
    pub fn parse_record_header(
        &mut self,
    ) -> Option<crate::RecordHeader> {
        let tag = self.read_u8()?;
        let _time_offset = self.read_u32()?;
        let length = self.read_u32()?;
        Some(crate::RecordHeader { tag, length })
    }

    /// Advances past the payload described by `header`.
    /// Returns `false` if there are not enough bytes.
    pub fn skip_record(
        &mut self,
        header: &crate::RecordHeader,
    ) -> bool {
        self.skip(header.length as usize)
    }

    /// Parses a `LOAD_CLASS` record body into a
    /// [`ClassDef`](crate::ClassDef).
    pub fn parse_load_class(
        &mut self,
    ) -> Option<crate::ClassDef> {
        let class_serial = self.read_u32()?;
        let class_object_id = self.read_id()?;
        let stack_trace_serial = self.read_u32()?;
        let class_name_string_id = self.read_id()?;
        Some(crate::ClassDef {
            class_serial,
            class_object_id,
            stack_trace_serial,
            class_name_string_id,
        })
    }

    /// Parses a `STACK_FRAME` record body into a
    /// [`StackFrame`](crate::StackFrame).
    pub fn parse_stack_frame(
        &mut self,
    ) -> Option<crate::StackFrame> {
        let frame_id = self.read_id()?;
        let method_name_string_id = self.read_id()?;
        let method_sig_string_id = self.read_id()?;
        let source_file_string_id = self.read_id()?;
        let class_serial = self.read_u32()?;
        let line_number = self.read_i32()?;
        Some(crate::StackFrame {
            frame_id,
            method_name_string_id,
            method_sig_string_id,
            source_file_string_id,
            class_serial,
            line_number,
        })
    }

    /// Parses a `STACK_TRACE` record body into a
    /// [`StackTrace`](crate::StackTrace).
    ///
    /// Validates that `num_frames * id_size` bytes remain
    /// before reading frame IDs.
    pub fn parse_stack_trace(
        &mut self,
    ) -> Option<crate::StackTrace> {
        let stack_trace_serial = self.read_u32()?;
        let thread_serial = self.read_u32()?;
        let num_frames = self.read_u32()?;
        let required = (num_frames as u64)
            .checked_mul(self.id_size.as_u32() as u64)?;
        if required > self.remaining() {
            return None;
        }
        let mut frame_ids =
            Vec::with_capacity(num_frames as usize);
        for _ in 0..num_frames {
            frame_ids.push(self.read_id()?);
        }
        Some(crate::StackTrace {
            stack_trace_serial,
            thread_serial,
            frame_ids,
        })
    }

    /// Parses a `START_THREAD` record body into an
    /// [`HprofThread`](crate::HprofThread).
    ///
    /// The `group_name_string_id` and
    /// `group_parent_name_string_id` fields default to
    /// `0` when absent.
    pub fn parse_start_thread(
        &mut self,
    ) -> Option<crate::HprofThread> {
        let thread_serial = self.read_u32()?;
        let object_id = self.read_id()?;
        let stack_trace_serial = self.read_u32()?;
        let name_string_id = self.read_id()?;
        let group_name_string_id =
            self.read_id().unwrap_or(0);
        let group_parent_name_string_id =
            self.read_id().unwrap_or(0);
        Some(crate::HprofThread {
            thread_serial,
            object_id,
            stack_trace_serial,
            name_string_id,
            group_name_string_id,
            group_parent_name_string_id,
        })
    }

    /// Parses a `STRING_IN_UTF8` record body into an
    /// [`HprofStringRef`](crate::HprofStringRef).
    ///
    /// `payload_length` is the total record payload size.
    /// `record_body_start` is the file offset where the
    /// payload begins (used to compute the string offset).
    pub fn parse_string_ref(
        &mut self,
        payload_length: u32,
        record_body_start: u64,
    ) -> Option<crate::HprofStringRef> {
        if payload_length < self.id_size.as_u32() {
            return None;
        }
        let id = self.read_id()?;
        let content_len =
            payload_length - self.id_size.as_u32();
        let offset =
            record_body_start + self.id_size.as_u32() as u64;
        if !self.skip(content_len as usize) {
            return None;
        }
        Some(crate::HprofStringRef {
            id,
            offset,
            len: content_len,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_u8_returns_byte() {
        let data = [0x42];
        let mut r = RecordReader::new(&data, IdSize::Four);
        assert_eq!(r.read_u8(), Some(0x42));
        assert_eq!(r.position(), 1);
    }

    #[test]
    fn read_u8_empty_returns_none() {
        let data = [];
        let mut r = RecordReader::new(&data, IdSize::Four);
        assert_eq!(r.read_u8(), None);
    }

    #[test]
    fn read_u32_big_endian() {
        let data = 0x01020304u32.to_be_bytes();
        let mut r = RecordReader::new(&data, IdSize::Four);
        assert_eq!(r.read_u32(), Some(0x01020304));
        assert_eq!(r.position(), 4);
    }

    #[test]
    fn read_id_four_byte() {
        let data = 0x0A0B0C0Du32.to_be_bytes();
        let mut r = RecordReader::new(&data, IdSize::Four);
        assert_eq!(r.read_id(), Some(0x0A0B0C0D));
        assert_eq!(r.position(), 4);
    }

    #[test]
    fn read_id_eight_byte() {
        let data = 0x0102030405060708u64.to_be_bytes();
        let mut r = RecordReader::new(&data, IdSize::Eight);
        assert_eq!(r.read_id(), Some(0x0102030405060708));
        assert_eq!(r.position(), 8);
    }

    #[test]
    fn read_id_insufficient_bytes_returns_none() {
        let data = [0x01, 0x02];
        let mut r = RecordReader::new(&data, IdSize::Four);
        assert_eq!(r.read_id(), None);
    }

    #[test]
    fn skip_advances_cursor() {
        let data = [0u8; 10];
        let mut r = RecordReader::new(&data, IdSize::Four);
        assert!(r.skip(5));
        assert_eq!(r.position(), 5);
    }

    #[test]
    fn skip_overflow_returns_false() {
        let data = [0u8; 4];
        let mut r = RecordReader::new(&data, IdSize::Four);
        r.set_position(1);
        assert!(!r.skip(usize::MAX));
        assert_eq!(r.position(), 1);
    }

    #[test]
    fn skip_past_end_returns_false() {
        let data = [0u8; 4];
        let mut r = RecordReader::new(&data, IdSize::Four);
        assert!(!r.skip(5));
        assert_eq!(r.position(), 0);
    }

    #[test]
    fn read_bytes_returns_slice() {
        let data = [1, 2, 3, 4, 5];
        let mut r = RecordReader::new(&data, IdSize::Four);
        r.set_position(1);
        assert_eq!(
            r.read_bytes(3),
            Some([2, 3, 4].as_slice())
        );
        assert_eq!(r.position(), 4);
    }

    #[test]
    fn read_bytes_past_end_returns_none() {
        let data = [1, 2];
        let mut r = RecordReader::new(&data, IdSize::Four);
        assert_eq!(r.read_bytes(3), None);
        assert_eq!(r.position(), 0);
    }

    #[test]
    fn remaining_returns_correct_count() {
        let data = [0u8; 10];
        let mut r = RecordReader::new(&data, IdSize::Four);
        assert_eq!(r.remaining(), 10);
        r.set_position(7);
        assert_eq!(r.remaining(), 3);
    }

    #[test]
    fn id_size_accessor() {
        let r = RecordReader::new(&[], IdSize::Eight);
        assert_eq!(r.id_size(), IdSize::Eight);
    }

    #[test]
    fn parse_record_header_valid() {
        let mut data = vec![0x01];
        data.extend_from_slice(&0u32.to_be_bytes());
        data.extend_from_slice(&10u32.to_be_bytes());
        let mut r =
            RecordReader::new(&data, IdSize::Four);
        let h = r.parse_record_header().unwrap();
        assert_eq!(h.tag, 0x01);
        assert_eq!(h.length, 10);
        assert_eq!(r.position(), 9);
    }

    #[test]
    fn parse_record_header_truncated() {
        let data = [0x01, 0x00];
        let mut r =
            RecordReader::new(&data, IdSize::Four);
        assert!(r.parse_record_header().is_none());
    }

    #[test]
    fn parse_load_class_valid() {
        let id_size = IdSize::Eight;
        let mut data = Vec::new();
        data.extend_from_slice(&1u32.to_be_bytes());
        data.extend_from_slice(&100u64.to_be_bytes());
        data.extend_from_slice(&2u32.to_be_bytes());
        data.extend_from_slice(&200u64.to_be_bytes());
        let mut r = RecordReader::new(&data, id_size);
        let c = r.parse_load_class().unwrap();
        assert_eq!(c.class_serial, 1);
        assert_eq!(c.class_object_id, 100);
        assert_eq!(c.stack_trace_serial, 2);
        assert_eq!(c.class_name_string_id, 200);
    }

    #[test]
    fn parse_stack_frame_valid() {
        let id_size = IdSize::Eight;
        let mut data = Vec::new();
        data.extend_from_slice(&1u64.to_be_bytes());
        data.extend_from_slice(&2u64.to_be_bytes());
        data.extend_from_slice(&3u64.to_be_bytes());
        data.extend_from_slice(&4u64.to_be_bytes());
        data.extend_from_slice(&5u32.to_be_bytes());
        data.extend_from_slice(&42i32.to_be_bytes());
        let mut r = RecordReader::new(&data, id_size);
        let f = r.parse_stack_frame().unwrap();
        assert_eq!(f.frame_id, 1);
        assert_eq!(f.method_name_string_id, 2);
        assert_eq!(f.method_sig_string_id, 3);
        assert_eq!(f.source_file_string_id, 4);
        assert_eq!(f.class_serial, 5);
        assert_eq!(f.line_number, 42);
    }

    #[test]
    fn parse_stack_trace_valid() {
        let id_size = IdSize::Eight;
        let mut data = Vec::new();
        data.extend_from_slice(&10u32.to_be_bytes());
        data.extend_from_slice(&1u32.to_be_bytes());
        data.extend_from_slice(&2u32.to_be_bytes());
        data.extend_from_slice(&0xAAu64.to_be_bytes());
        data.extend_from_slice(&0xBBu64.to_be_bytes());
        let mut r = RecordReader::new(&data, id_size);
        let t = r.parse_stack_trace().unwrap();
        assert_eq!(t.stack_trace_serial, 10);
        assert_eq!(t.thread_serial, 1);
        assert_eq!(t.frame_ids, vec![0xAA, 0xBB]);
    }

    #[test]
    fn parse_stack_trace_truncated_frames() {
        let id_size = IdSize::Eight;
        let mut data = Vec::new();
        data.extend_from_slice(&10u32.to_be_bytes());
        data.extend_from_slice(&1u32.to_be_bytes());
        data.extend_from_slice(&5u32.to_be_bytes());
        data.extend_from_slice(&0xAAu64.to_be_bytes());
        let mut r = RecordReader::new(&data, id_size);
        assert!(r.parse_stack_trace().is_none());
    }

    #[test]
    fn parse_start_thread_full() {
        let id_size = IdSize::Eight;
        let mut data = Vec::new();
        data.extend_from_slice(&1u32.to_be_bytes());
        data.extend_from_slice(&0xA0u64.to_be_bytes());
        data.extend_from_slice(&2u32.to_be_bytes());
        data.extend_from_slice(&0xB0u64.to_be_bytes());
        data.extend_from_slice(&0xC0u64.to_be_bytes());
        data.extend_from_slice(&0xD0u64.to_be_bytes());
        let mut r = RecordReader::new(&data, id_size);
        let t = r.parse_start_thread().unwrap();
        assert_eq!(t.thread_serial, 1);
        assert_eq!(t.object_id, 0xA0);
        assert_eq!(t.name_string_id, 0xB0);
        assert_eq!(t.group_name_string_id, 0xC0);
        assert_eq!(t.group_parent_name_string_id, 0xD0);
    }

    #[test]
    fn parse_start_thread_optional_group_fields() {
        let id_size = IdSize::Eight;
        let mut data = Vec::new();
        data.extend_from_slice(&1u32.to_be_bytes());
        data.extend_from_slice(&0xA0u64.to_be_bytes());
        data.extend_from_slice(&2u32.to_be_bytes());
        data.extend_from_slice(&0xB0u64.to_be_bytes());
        let mut r = RecordReader::new(&data, id_size);
        let t = r.parse_start_thread().unwrap();
        assert_eq!(t.group_name_string_id, 0);
        assert_eq!(t.group_parent_name_string_id, 0);
    }

    #[test]
    fn parse_string_ref_valid() {
        let id_size = IdSize::Eight;
        let mut data = Vec::new();
        data.extend_from_slice(&42u64.to_be_bytes());
        data.extend_from_slice(b"hello");
        let payload_len = data.len() as u32;
        let mut r = RecordReader::new(&data, id_size);
        let s =
            r.parse_string_ref(payload_len, 100).unwrap();
        assert_eq!(s.id, 42);
        assert_eq!(s.offset, 100 + 8);
        assert_eq!(s.len, 5);
    }
}
