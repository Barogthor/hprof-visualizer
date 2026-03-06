//! hprof file header parsing.
//!
//! Parses the binary header at the start of an hprof dump:
//!
//! ```text
//! [null-terminated version string]   variable length, ends with 0x00
//! [id_size: u32 big-endian]          4 bytes — values 4 or 8
//! [dump_timestamp: u64 big-endian]   8 bytes — millis since epoch (ignored)
//! ```
//!
//! Use [`parse_header`] to extract [`HprofVersion`] and `id_size` from raw bytes.

use byteorder::{BigEndian, ReadBytesExt};
use std::io::Cursor;

use crate::HprofError;

/// Known hprof format versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HprofVersion {
    /// "JAVA PROFILE 1.0.1"
    V1_0_1,
    /// "JAVA PROFILE 1.0.2"
    V1_0_2,
}

/// Parsed contents of an hprof file header.
#[derive(Debug, Clone)]
pub struct HprofHeader {
    /// Format version of the dump.
    pub version: HprofVersion,
    /// Byte width of object IDs in this dump (4 or 8).
    pub id_size: u32,
}

/// Parses an hprof header from a raw byte slice.
///
/// # Parameters
/// - `data: &[u8]` — raw bytes starting at offset 0 of the hprof file.
///
/// # Returns
/// - `Ok(HprofHeader)` on success.
/// - `Err(HprofError::TruncatedRecord)` if the data ends before a complete header.
/// - `Err(HprofError::UnsupportedVersion(String))` if the version string is not
///   `"JAVA PROFILE 1.0.1"` or `"JAVA PROFILE 1.0.2"`.
/// - `Err(HprofError::CorruptedData(String))` if the version bytes are not valid UTF-8.
pub fn parse_header(data: &[u8]) -> Result<HprofHeader, HprofError> {
    let null_pos = data
        .iter()
        .position(|&b| b == 0)
        .ok_or(HprofError::TruncatedRecord)?;

    let version_str = std::str::from_utf8(&data[..null_pos])
        .map_err(|e| HprofError::CorruptedData(e.to_string()))?;

    let version = match version_str {
        "JAVA PROFILE 1.0.1" => HprofVersion::V1_0_1,
        "JAVA PROFILE 1.0.2" => HprofVersion::V1_0_2,
        other => return Err(HprofError::UnsupportedVersion(other.to_owned())),
    };

    let mut cursor = Cursor::new(&data[null_pos + 1..]);
    let id_size = cursor
        .read_u32::<BigEndian>()
        .map_err(|_| HprofError::TruncatedRecord)?;
    if id_size != 4 && id_size != 8 {
        return Err(HprofError::CorruptedData(format!(
            "invalid id_size: {id_size}, expected 4 or 8"
        )));
    }
    let _timestamp = cursor
        .read_u64::<BigEndian>()
        .map_err(|_| HprofError::TruncatedRecord)?;

    Ok(HprofHeader { version, id_size })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncated_no_null_returns_truncated_record() {
        let data = b"JAVA PROFILE";
        assert!(matches!(
            parse_header(data),
            Err(HprofError::TruncatedRecord)
        ));
    }

    #[test]
    fn truncated_after_version_string_returns_truncated_record() {
        let data = b"JAVA PROFILE 1.0.2\0";
        assert!(matches!(
            parse_header(data),
            Err(HprofError::TruncatedRecord)
        ));
    }

    #[test]
    fn truncated_missing_timestamp_returns_truncated_record() {
        let mut data = b"JAVA PROFILE 1.0.2\0".to_vec();
        data.extend_from_slice(&4u32.to_be_bytes()); // id_size only, no timestamp
        assert!(matches!(
            parse_header(&data),
            Err(HprofError::TruncatedRecord)
        ));
    }

    #[test]
    fn invalid_version_returns_unsupported_version() {
        let mut data = b"NOT HPROF\0".to_vec();
        data.extend_from_slice(&4u32.to_be_bytes());
        data.extend_from_slice(&0u64.to_be_bytes());
        let err = parse_header(&data).unwrap_err();
        assert!(matches!(err, HprofError::UnsupportedVersion(_)));
        if let HprofError::UnsupportedVersion(s) = err {
            assert_eq!(s, "NOT HPROF");
        }
    }

    #[test]
    fn empty_input_returns_truncated_record() {
        assert!(matches!(
            parse_header(&[]),
            Err(HprofError::TruncatedRecord)
        ));
    }

    #[test]
    fn invalid_id_size_3_returns_corrupted_data() {
        let mut data = b"JAVA PROFILE 1.0.2\0".to_vec();
        data.extend_from_slice(&3u32.to_be_bytes());
        data.extend_from_slice(&0u64.to_be_bytes());
        assert!(matches!(
            parse_header(&data),
            Err(HprofError::CorruptedData(_))
        ));
    }

    #[test]
    fn invalid_id_size_0_returns_corrupted_data() {
        let mut data = b"JAVA PROFILE 1.0.2\0".to_vec();
        data.extend_from_slice(&0u32.to_be_bytes());
        data.extend_from_slice(&0u64.to_be_bytes());
        assert!(matches!(
            parse_header(&data),
            Err(HprofError::CorruptedData(_))
        ));
    }

    #[test]
    fn valid_101_4byte_ids() {
        let mut data = b"JAVA PROFILE 1.0.1\0".to_vec();
        data.extend_from_slice(&4u32.to_be_bytes());
        data.extend_from_slice(&0u64.to_be_bytes());
        let header = parse_header(&data).unwrap();
        assert_eq!(header.version, HprofVersion::V1_0_1);
        assert_eq!(header.id_size, 4);
    }

    #[test]
    fn valid_102_8byte_ids() {
        let mut data = b"JAVA PROFILE 1.0.2\0".to_vec();
        data.extend_from_slice(&8u32.to_be_bytes());
        data.extend_from_slice(&0u64.to_be_bytes());
        let header = parse_header(&data).unwrap();
        assert_eq!(header.version, HprofVersion::V1_0_2);
        assert_eq!(header.id_size, 8);
    }
}

#[cfg(all(test, feature = "test-utils"))]
mod builder_tests {
    use super::*;
    use crate::test_utils::HprofTestBuilder;

    #[test]
    fn parse_valid_102_8byte_ids_from_builder() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_string(1, "main")
            .build();
        let header = parse_header(&bytes).unwrap();
        assert_eq!(header.version, HprofVersion::V1_0_2);
        assert_eq!(header.id_size, 8);
    }

    #[test]
    fn parse_valid_101_4byte_ids_from_builder() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.1", 4).build();
        let header = parse_header(&bytes).unwrap();
        assert_eq!(header.version, HprofVersion::V1_0_1);
        assert_eq!(header.id_size, 4);
    }
}
