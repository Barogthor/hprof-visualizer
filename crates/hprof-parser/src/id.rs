//! ID reading utility for hprof files.
//!
//! Provides [`read_id`] which reads a 4- or 8-byte big-endian object ID from a
//! [`Cursor`](std::io::Cursor) and returns it as `u64`.  All ID reads in the
//! parser **must** go through this function — never hardcode 4 or 8 bytes at
//! call sites.

use byteorder::{BigEndian, ReadBytesExt};
use std::io::Cursor;

use crate::HprofError;

/// Byte width of hprof object identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IdSize {
    /// 4-byte (32-bit) object IDs.
    Four,
    /// 8-byte (64-bit) object IDs.
    Eight,
}

impl IdSize {
    /// Parses a raw `u32` from the file header into an
    /// `IdSize`.
    ///
    /// Returns `Err(HprofError::CorruptedData)` if `value`
    /// is not 4 or 8.
    pub fn from_raw(value: u32) -> Result<Self, HprofError> {
        match value {
            4 => Ok(Self::Four),
            8 => Ok(Self::Eight),
            _ => Err(HprofError::CorruptedData(format!(
                "invalid id_size: {value}, \
                 expected 4 or 8"
            ))),
        }
    }

    /// Returns the byte width as `u32`.
    pub fn as_u32(self) -> u32 {
        match self {
            Self::Four => 4,
            Self::Eight => 8,
        }
    }

    /// Returns the byte width as `usize`.
    pub fn as_usize(self) -> usize {
        self.as_u32() as usize
    }
}

/// Reads an object ID of `id_size` bytes from `cursor`
/// as a big-endian `u64`.
///
/// # Parameters
/// - `cursor`: `&mut Cursor<&[u8]>` — positioned at the
///   first byte of the ID.
/// - `id_size`: [`IdSize`] — byte width (4 or 8).
///
/// # Errors
/// - [`HprofError::TruncatedRecord`] — fewer bytes remain
///   than `id_size`.
pub fn read_id(cursor: &mut Cursor<&[u8]>, id_size: IdSize) -> Result<u64, HprofError> {
    match id_size {
        IdSize::Four => cursor
            .read_u32::<BigEndian>()
            .map(|v| v as u64)
            .map_err(|_| HprofError::TruncatedRecord),
        IdSize::Eight => cursor
            .read_u64::<BigEndian>()
            .map_err(|_| HprofError::TruncatedRecord),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn read_id_4_byte() {
        let bytes = 0x0102_0304u32.to_be_bytes();
        let mut cursor = Cursor::new(bytes.as_slice());
        let id = read_id(&mut cursor, IdSize::Four).unwrap();
        assert_eq!(id, 0x0102_0304u64);
        assert_eq!(cursor.position(), 4);
    }

    #[test]
    fn read_id_8_byte() {
        let bytes = 0x0102_0304_0506_0708u64.to_be_bytes();
        let mut cursor = Cursor::new(bytes.as_slice());
        let id = read_id(&mut cursor, IdSize::Eight).unwrap();
        assert_eq!(id, 0x0102_0304_0506_0708u64);
        assert_eq!(cursor.position(), 8);
    }

    #[test]
    fn read_id_insufficient_bytes_4_returns_truncated() {
        let bytes = [0x01u8, 0x02];
        let mut cursor = Cursor::new(bytes.as_slice());
        let err = read_id(&mut cursor, IdSize::Four).unwrap_err();
        assert!(matches!(err, HprofError::TruncatedRecord));
    }

    #[test]
    fn read_id_insufficient_bytes_8_returns_truncated() {
        let bytes = [0x01u8, 0x02, 0x03, 0x04];
        let mut cursor = Cursor::new(bytes.as_slice());
        let err = read_id(&mut cursor, IdSize::Eight).unwrap_err();
        assert!(matches!(err, HprofError::TruncatedRecord));
    }

    #[test]
    fn id_size_from_raw_valid() {
        assert_eq!(IdSize::from_raw(4).unwrap(), IdSize::Four);
        assert_eq!(IdSize::from_raw(8).unwrap(), IdSize::Eight);
    }

    #[test]
    fn id_size_from_raw_invalid() {
        assert!(IdSize::from_raw(0).is_err());
        assert!(IdSize::from_raw(3).is_err());
        assert!(IdSize::from_raw(16).is_err());
    }

    #[test]
    fn id_size_as_u32() {
        assert_eq!(IdSize::Four.as_u32(), 4);
        assert_eq!(IdSize::Eight.as_u32(), 8);
    }

    #[test]
    fn id_size_as_usize() {
        assert_eq!(IdSize::Four.as_usize(), 4);
        assert_eq!(IdSize::Eight.as_usize(), 8);
    }
}
