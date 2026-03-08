//! In-memory O(1) indexes for all structural hprof records, built by the
//! first-pass indexer.
//!
//! [`PreciseIndex`] holds [`HashMap`] collections, each keyed by the
//! primary identifier of its record type:
//!
//! | Field | Key type | Record |
//! |---|---|---|
//! | `strings` | `u64` string ID | `STRING_IN_UTF8` |
//! | `classes` | `u32` class serial | `LOAD_CLASS` |
//! | `threads` | `u32` thread serial | `START_THREAD` |
//! | `stack_frames` | `u64` frame ID | `STACK_FRAME` |
//! | `stack_traces` | `u32` stack trace serial | `STACK_TRACE` |
//! | `java_frame_roots` | `u64` frame ID | `GC_ROOT_JAVA_FRAME` |
//! | `class_dumps` | `u64` class object ID | `CLASS_DUMP` |
//! | `thread_object_ids` | `u32` thread serial | `ROOT_THREAD_OBJ` |
//! | `class_names_by_id` | `u64` class object ID | derived from `LOAD_CLASS` |

use std::collections::HashMap;

use crate::{ClassDef, ClassDumpInfo, HprofString, HprofThread, StackFrame, StackTrace};

/// O(1) lookup index populated by a single sequential pass over an hprof file.
///
/// All maps are public so callers can inspect them directly. Maps are
/// populated by [`crate::indexer::first_pass::run_first_pass`] and are
/// read-only after construction.
#[derive(Debug)]
pub struct PreciseIndex {
    /// `STRING_IN_UTF8` records keyed by string object ID.
    pub strings: HashMap<u64, HprofString>,
    /// `LOAD_CLASS` records keyed by class serial number.
    pub classes: HashMap<u32, ClassDef>,
    /// `START_THREAD` records keyed by thread serial number.
    pub threads: HashMap<u32, HprofThread>,
    /// `STACK_FRAME` records keyed by frame ID.
    pub stack_frames: HashMap<u64, StackFrame>,
    /// `STACK_TRACE` records keyed by stack trace serial number.
    pub stack_traces: HashMap<u32, StackTrace>,
    /// GC root object IDs keyed by frame ID. Populated during first pass by
    /// correlating `GC_ROOT_JAVA_FRAME` sub-records with `STACK_TRACE` records.
    ///
    /// Key: `frame_id` (u64) — Value: Vec of object IDs rooted at that frame.
    pub java_frame_roots: HashMap<u64, Vec<u64>>,
    /// `CLASS_DUMP` sub-records keyed by `class_object_id`.
    pub class_dumps: HashMap<u64, ClassDumpInfo>,
    /// `ROOT_THREAD_OBJ` heap object IDs keyed by thread serial.
    /// Maps thread_serial → object_id from the heap.
    pub thread_object_ids: HashMap<u32, u64>,
    /// Java class names keyed by `class_object_id`.
    ///
    /// Populated from `LOAD_CLASS` records during the first pass. Binary JVM
    /// names (`java/util/HashMap`) are normalised to dot notation
    /// (`java.util.HashMap`).
    pub class_names_by_id: HashMap<u64, String>,
    /// Object-ID → byte offset (relative to records section) for
    /// thread-related heap objects: Thread instances, their `name`
    /// String instances, the backing `char[]/byte[]` arrays, and
    /// JDK 19+ `FieldHolder` instances.
    ///
    /// Populated after the first pass by cross-referencing
    /// `thread_object_ids` with a temporary offset index, then
    /// following transitive references (`Thread.name` → `String` →
    /// `String.value` → `char[]`, `Thread.holder` → `FieldHolder`).
    ///
    /// Used by the engine for O(1) offset-based reads during thread
    /// name and state resolution, falling back to linear scan when
    /// an ID is not present.
    pub instance_offsets: HashMap<u64, u64>,
}

impl PreciseIndex {
    /// Creates a new empty index.
    pub fn new() -> Self {
        Self {
            strings: HashMap::new(),
            classes: HashMap::new(),
            threads: HashMap::new(),
            stack_frames: HashMap::new(),
            stack_traces: HashMap::new(),
            java_frame_roots: HashMap::new(),
            class_dumps: HashMap::new(),
            thread_object_ids: HashMap::new(),
            class_names_by_id: HashMap::new(),
            instance_offsets: HashMap::new(),
        }
    }
}

impl Default for PreciseIndex {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ClassDef, HprofString, HprofThread, StackFrame, StackTrace};

    #[test]
    fn new_creates_empty_index() {
        let index = PreciseIndex::new();
        assert!(index.strings.is_empty());
        assert!(index.classes.is_empty());
        assert!(index.threads.is_empty());
        assert!(index.stack_frames.is_empty());
        assert!(index.stack_traces.is_empty());
        assert!(index.java_frame_roots.is_empty());
        assert!(index.class_dumps.is_empty());
        assert!(index.thread_object_ids.is_empty());
        assert!(index.class_names_by_id.is_empty());
    }

    #[test]
    fn insert_and_retrieve_string_by_id() {
        let mut index = PreciseIndex::new();
        index.strings.insert(
            5,
            HprofString {
                id: 5,
                value: "hello".into(),
            },
        );
        let s = index.strings.get(&5).unwrap();
        assert_eq!(s.id, 5);
        assert_eq!(s.value, "hello");
    }

    #[test]
    fn insert_and_retrieve_class_by_serial() {
        let mut index = PreciseIndex::new();
        index.classes.insert(
            1,
            ClassDef {
                class_serial: 1,
                class_object_id: 100,
                stack_trace_serial: 0,
                class_name_string_id: 200,
            },
        );
        let c = index.classes.get(&1).unwrap();
        assert_eq!(c.class_serial, 1);
        assert_eq!(c.class_object_id, 100);
    }

    #[test]
    fn insert_and_retrieve_thread_by_serial() {
        let mut index = PreciseIndex::new();
        index.threads.insert(
            2,
            HprofThread {
                thread_serial: 2,
                object_id: 300,
                stack_trace_serial: 0,
                name_string_id: 1,
                group_name_string_id: 2,
                group_parent_name_string_id: 3,
            },
        );
        let t = index.threads.get(&2).unwrap();
        assert_eq!(t.thread_serial, 2);
        assert_eq!(t.object_id, 300);
    }

    #[test]
    fn insert_and_retrieve_stack_frame_by_id() {
        let mut index = PreciseIndex::new();
        index.stack_frames.insert(
            10,
            StackFrame {
                frame_id: 10,
                method_name_string_id: 1,
                method_sig_string_id: 2,
                source_file_string_id: 3,
                class_serial: 5,
                line_number: 42,
            },
        );
        let f = index.stack_frames.get(&10).unwrap();
        assert_eq!(f.frame_id, 10);
        assert_eq!(f.line_number, 42);
    }

    #[test]
    fn insert_and_retrieve_stack_trace_by_serial() {
        let mut index = PreciseIndex::new();
        index.stack_traces.insert(
            3,
            StackTrace {
                stack_trace_serial: 3,
                thread_serial: 1,
                frame_ids: vec![10, 20],
            },
        );
        let st = index.stack_traces.get(&3).unwrap();
        assert_eq!(st.stack_trace_serial, 3);
        assert_eq!(st.frame_ids, vec![10, 20]);
    }
}
