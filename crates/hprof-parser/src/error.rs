//! Error types for hprof parsing.
//!
//! Defines [`HprofError`] with fatal/non-fatal variants used across all parsing
//! code. Severity is determined by callers, not encoded in the enum itself.

use thiserror::Error;

/// All errors that can arise during hprof file parsing.
///
/// ## Fatal variants (caller should abort)
/// - [`HprofError::UnsupportedVersion`]
/// - [`HprofError::MmapFailed`]
/// - [`HprofError::IoError`]
///
/// ## Non-fatal variants (caller may collect as warning and continue)
/// - [`HprofError::TruncatedRecord`]
/// - [`HprofError::InvalidId`]
/// - [`HprofError::UnknownRecordType`]
/// - [`HprofError::CorruptedData`]
#[derive(Debug, Error)]
pub enum HprofError {
    #[error("record is truncated — insufficient bytes remaining")]
    TruncatedRecord,

    #[error("invalid object ID: {0}")]
    InvalidId(u64),

    #[error("unknown record tag: 0x{tag:02X}")]
    UnknownRecordType { tag: u8 },

    #[error("corrupted data: {0}")]
    CorruptedData(String),

    #[error("unsupported hprof version: {0}")]
    UnsupportedVersion(String),

    #[error("mmap failed: {0}")]
    MmapFailed(std::io::Error),

    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn truncated_record_display() {
        let msg = HprofError::TruncatedRecord.to_string();
        assert_eq!(msg, "record is truncated — insufficient bytes remaining");
    }

    #[test]
    fn invalid_id_display() {
        let msg = HprofError::InvalidId(42).to_string();
        assert_eq!(msg, "invalid object ID: 42");
    }

    #[test]
    fn unknown_record_type_display() {
        let msg = HprofError::UnknownRecordType { tag: 0xAB }.to_string();
        assert_eq!(msg, "unknown record tag: 0xAB");
    }

    #[test]
    fn corrupted_data_display() {
        let msg = HprofError::CorruptedData("bad length".to_string()).to_string();
        assert_eq!(msg, "corrupted data: bad length");
    }

    #[test]
    fn unsupported_version_display() {
        let msg = HprofError::UnsupportedVersion("JAVA PROFILE 9.9".to_string()).to_string();
        assert_eq!(msg, "unsupported hprof version: JAVA PROFILE 9.9");
    }

    #[test]
    fn io_error_from_conversion() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let hprof_err: HprofError = io_err.into();
        assert!(hprof_err.to_string().contains("file not found"));
    }

    #[test]
    fn mmap_failed_display() {
        let io_err = io::Error::new(io::ErrorKind::PermissionDenied, "permission denied");
        let msg = HprofError::MmapFailed(io_err).to_string();
        assert!(msg.contains("mmap failed"));
        assert!(msg.contains("permission denied"));
    }

    #[test]
    fn unknown_record_type_display_zero_padded() {
        let msg = HprofError::UnknownRecordType { tag: 0x0F }.to_string();
        assert_eq!(msg, "unknown record tag: 0x0F");
    }
}
