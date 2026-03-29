//! Read-only memory-mapped file access for hprof files.
//!
//! # Safety
//!
//! The caller must ensure the backing file is not modified while the [`Mmap`]
//! is alive. This module opens files in read-only mode, so accidental writes
//! through the mapping are not possible, but external processes modifying the
//! file during a live mapping produce undefined behaviour.
//!
//! # Example
//!
//! ```no_run
//! use std::path::Path;
//! use hprof_parser::open_readonly;
//!
//! let mmap = open_readonly(Path::new("heap.hprof")).unwrap();
//! println!("file size: {} bytes", mmap.len());
//! ```

use std::fs::File;
use std::path::Path;

use memmap2::{Mmap, MmapOptions};

use crate::HprofError;

/// Opens `path` as a read-only memory-mapped file.
///
/// # Parameters
/// - `path: &Path` — filesystem path to the hprof file.
///
/// # Returns
/// - `Ok(Mmap)` — the mapped region; dereferences to `&[u8]`.
/// - `Err(HprofError::MmapFailed(_))` — file not found, permission denied,
///   or the OS mapping call failed.
pub fn open_readonly(path: &Path) -> Result<Mmap, HprofError> {
    let file = File::open(path).map_err(HprofError::MmapFailed)?;
    // SAFETY: The file is opened read-only. The caller is responsible for not
    // modifying the file during the lifetime of the Mmap.
    unsafe { MmapOptions::new().map(&file) }.map_err(HprofError::MmapFailed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_existent_path_returns_mmap_failed() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let missing_path = tmp.path().to_path_buf();
        drop(tmp);

        let result = open_readonly(&missing_path);
        assert!(matches!(result, Err(HprofError::MmapFailed(_))));
    }
}

#[cfg(all(test, feature = "test-utils"))]
mod builder_tests {
    use super::*;
    use crate::test_utils::HprofTestBuilder;
    use crate::{HprofVersion, parse_header};
    use std::io::Write;

    #[test]
    fn valid_temp_file_returns_ok_mmap_with_correct_length() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_string(1, "main")
            .build();
        let expected_len = bytes.len();

        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();

        let mmap = open_readonly(tmp.path()).unwrap();
        assert_eq!(mmap.len(), expected_len);
    }

    #[test]
    fn builder_bytes_mmap_then_parse_header() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_string(1, "main")
            .build();

        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();

        let mmap = open_readonly(tmp.path()).unwrap();
        let header = parse_header(&mmap).unwrap();

        assert_eq!(header.version, HprofVersion::V1_0_2);
        assert_eq!(header.id_size, crate::id::IdSize::Eight);
    }
}
