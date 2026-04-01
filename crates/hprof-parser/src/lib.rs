//! Binary hprof format parsing for the
//! hprof-visualizer project.
//!
//! The primary entry point is [`HprofFile`]: open a
//! file, parse its header, and index all structural
//! records in a single call. [`PreciseIndex`] exposes
//! the resulting O(1) HashMap lookups for threads,
//! stack frames, stack traces, class definitions,
//! and strings.
//!
//! All binary parsing goes through
//! [`RecordReader`] -- callers never manipulate raw
//! cursors or pass `id_size` around.
//!
//! Lower-level modules: error types ([`HprofError`]),
//! memory-mapped file access ([`open_readonly`]),
//! header parsing ([`parse_header`]), ID reading
//! utility ([`read_id`]), record types
//! ([`RecordHeader`], [`HprofStringRef`],
//! [`ClassDef`], [`HprofThread`], [`StackFrame`],
//! [`StackTrace`]), and a feature-gated test builder
//! ([`HprofTestBuilder`], enabled with
//! `features = ["test-utils"]`).

pub(crate) mod error;
pub use error::HprofError;

pub(crate) mod mmap;
pub use mmap::open_readonly;

pub(crate) mod format;
pub use format::{
    HprofHeader, HprofStringRef, HprofVersion, IdSize, RecordHeader, parse_header, read_id,
};

pub(crate) mod reader;
pub use reader::RecordReader;

pub(crate) mod heap_reader;
pub use heap_reader::{HeapSubRecord, HeapSubRecordIter};

pub(crate) mod types;
pub use types::{
    ClassDef, ClassDumpInfo, FieldDef, HprofThread, RawInstance, StackFrame, StackTrace,
    StaticFieldDef, StaticValue,
};

pub(crate) mod java_types;
pub use java_types::{
    PRIM_TYPE_BOOLEAN, PRIM_TYPE_BYTE, PRIM_TYPE_CHAR, PRIM_TYPE_DOUBLE, PRIM_TYPE_FLOAT,
    PRIM_TYPE_INT, PRIM_TYPE_LONG, PRIM_TYPE_OBJECT_REF, PRIM_TYPE_SHORT, jvm_to_java,
};

pub mod tags;
pub use tags::{HeapSubTag, RecordTag};

pub mod indexer;
pub use indexer::HeapRecordRange;
pub use indexer::precise::PreciseIndex;

pub(crate) mod hprof_file;
pub use hprof_file::{HprofFile, IndexStats};

pub(crate) mod resolution;
pub use resolution::{BatchResult, ObjectArrayMeta};

#[cfg(feature = "test-utils")]
pub(crate) mod test_utils;
#[cfg(feature = "test-utils")]
pub use test_utils::HprofTestBuilder;
