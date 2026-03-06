//! Navigation Engine trait, `Engine::from_file()` factory, LRU cache,
//! `MemorySize` tracking, object resolution, and pagination logic.

use std::path::Path;

pub use hprof_parser::{HprofError, HprofHeader, HprofVersion};

/// Opens an hprof file in read-only mmap mode and parses its header.
pub fn open_hprof_header(path: &Path) -> Result<HprofHeader, HprofError> {
    let mmap = hprof_parser::open_readonly(path)?;
    hprof_parser::parse_header(&mmap)
}
