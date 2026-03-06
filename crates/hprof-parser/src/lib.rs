//! Binary hprof format parsing for the hprof-visualizer project.
//!
//! Current modules: error types ([`HprofError`]), memory-mapped file access
//! ([`open_readonly`]), header parsing ([`parse_header`]), ID reading utility
//! ([`read_id`]), record-level parsing ([`RecordHeader`], [`parse_record_header`],
//! [`skip_record`]), and a feature-gated test builder ([`HprofTestBuilder`],
//! enabled with `features = ["test-utils"]`).

pub(crate) mod error;
pub use error::HprofError;

pub(crate) mod mmap;
pub use mmap::open_readonly;

pub(crate) mod header;
pub use header::{HprofHeader, HprofVersion, parse_header};

pub(crate) mod id;
pub use id::read_id;

pub(crate) mod record;
pub use record::{RecordHeader, parse_record_header, skip_record};

#[cfg(feature = "test-utils")]
pub(crate) mod test_utils;
#[cfg(feature = "test-utils")]
pub use test_utils::HprofTestBuilder;
