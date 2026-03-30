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
}
