//! Binary hprof format parsing, domain types, first-pass indexer,
//! BinaryFuse8 segment filter construction, and test builder
//! (feature-gated `test-utils`).

pub mod error;
pub use error::HprofError;

#[cfg(feature = "test-utils")]
pub(crate) mod test_utils;
#[cfg(feature = "test-utils")]
pub use test_utils::HprofTestBuilder;
