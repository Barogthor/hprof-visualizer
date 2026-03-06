//! Binary hprof format parsing, domain types, first-pass indexer,
//! BinaryFuse8 segment filter construction, and test builder
//! (feature-gated `test-utils`).

pub(crate) mod error;
pub use error::HprofError;

pub(crate) mod mmap;
pub use mmap::open_readonly;

pub(crate) mod header;
pub use header::{HprofHeader, HprofVersion, parse_header};

#[cfg(feature = "test-utils")]
pub(crate) mod test_utils;
#[cfg(feature = "test-utils")]
pub use test_utils::HprofTestBuilder;
