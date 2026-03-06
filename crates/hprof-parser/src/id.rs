//! ID reading utility for hprof files.
//!
//! Provides [`read_id`] which reads a 4- or 8-byte big-endian object ID from a
//! [`Cursor`](std::io::Cursor) and returns it as `u64`.  All ID reads in the
//! parser **must** go through this function — never hardcode 4 or 8 bytes at
//! call sites.

use byteorder::{BigEndian, ReadBytesExt};
use std::io::Cursor;

use crate::HprofError;

/// Reads an object ID of `id_size` bytes from `cursor` as a big-endian `u64`.
///
/// # Parameters
/// - `cursor`: `&mut Cursor<&[u8]>` — positioned at the first byte of the ID.
/// - `id_size`: `u32` — must be `4` or `8`; any other value returns
///   [`HprofError::CorruptedData`].
///
/// # Errors
/// - [`HprofError::TruncatedRecord`] — fewer bytes remain than `id_size`.
/// - [`HprofError::CorruptedData`] — `id_size` is not `4` or `8`.
pub fn read_id(cursor: &mut Cursor<&[u8]>, id_size: u32) -> Result<u64, HprofError> {
    match id_size {
        4 => cursor
            .read_u32::<BigEndian>()
            .map(|v| v as u64)
            .map_err(|_| HprofError::TruncatedRecord),
        8 => cursor
            .read_u64::<BigEndian>()
            .map_err(|_| HprofError::TruncatedRecord),
        _ => Err(HprofError::CorruptedData(format!(
            "invalid id_size: {id_size}, expected 4 or 8"
        ))),
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
        let id = read_id(&mut cursor, 4).unwrap();
        assert_eq!(id, 0x0102_0304u64);
        assert_eq!(cursor.position(), 4);
    }

    #[test]
    fn read_id_8_byte() {
        let bytes = 0x0102_0304_0506_0708u64.to_be_bytes();
        let mut cursor = Cursor::new(bytes.as_slice());
        let id = read_id(&mut cursor, 8).unwrap();
        assert_eq!(id, 0x0102_0304_0506_0708u64);
        assert_eq!(cursor.position(), 8);
    }

    #[test]
    fn read_id_insufficient_bytes_4_returns_truncated() {
        let bytes = [0x01u8, 0x02]; // only 2 bytes, need 4
        let mut cursor = Cursor::new(bytes.as_slice());
        let err = read_id(&mut cursor, 4).unwrap_err();
        assert!(matches!(err, HprofError::TruncatedRecord));
    }

    #[test]
    fn read_id_insufficient_bytes_8_returns_truncated() {
        let bytes = [0x01u8, 0x02, 0x03, 0x04]; // only 4 bytes, need 8
        let mut cursor = Cursor::new(bytes.as_slice());
        let err = read_id(&mut cursor, 8).unwrap_err();
        assert!(matches!(err, HprofError::TruncatedRecord));
    }

    #[test]
    fn read_id_unsupported_size_returns_corrupted() {
        let bytes = [0x01u8, 0x02];
        let mut cursor = Cursor::new(bytes.as_slice());
        let err = read_id(&mut cursor, 2).unwrap_err();
        assert!(matches!(err, HprofError::CorruptedData(_)));
    }
}
