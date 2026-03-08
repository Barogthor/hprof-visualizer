//! Binary hprof format parsing for the hprof-visualizer project.
//!
//! The primary entry point is [`HprofFile`]: open a file, parse its header,
//! and index all structural records in a single call. [`PreciseIndex`] exposes
//! the resulting O(1) HashMap lookups for threads, stack frames, stack traces,
//! class definitions, and strings.
//!
//! Lower-level modules: error types ([`HprofError`]), memory-mapped file
//! access ([`open_readonly`]), header parsing ([`parse_header`]), ID reading
//! utility ([`read_id`]), record-level parsing ([`RecordHeader`],
//! [`parse_record_header`], [`skip_record`]), string records ([`HprofString`],
//! [`parse_string_record`]), structural records ([`ClassDef`], [`HprofThread`],
//! [`StackFrame`], [`StackTrace`] and their parsers), and a feature-gated test
//! builder ([`HprofTestBuilder`], enabled with `features = ["test-utils"]`).

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

pub(crate) mod strings;
pub use strings::{HprofString, parse_string_record};

pub(crate) mod types;
pub use types::{
    ClassDef, ClassDumpInfo, FieldDef, HprofThread, RawInstance, StackFrame, StackTrace,
    parse_load_class, parse_stack_frame, parse_stack_trace, parse_start_thread,
};

pub(crate) mod java_types;
pub use java_types::jvm_to_java;

pub(crate) mod indexer;
pub use indexer::precise::PreciseIndex;

pub(crate) mod hprof_file;
pub use hprof_file::HprofFile;

#[cfg(feature = "test-utils")]
pub(crate) mod test_utils;
#[cfg(feature = "test-utils")]
pub use test_utils::HprofTestBuilder;
