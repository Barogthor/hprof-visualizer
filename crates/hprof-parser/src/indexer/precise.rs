//! In-memory O(1) indexes for all structural hprof records, built by the
//! first-pass indexer.
//!
//! [`PreciseIndex`] holds five [`HashMap`] collections, each keyed by the
//! primary identifier of its record type:
//!
//! | Field | Key type | Record |
//! |---|---|---|
//! | `strings` | `u64` string ID | `STRING_IN_UTF8` |
//! | `classes` | `u32` class serial | `LOAD_CLASS` |
//! | `threads` | `u32` thread serial | `START_THREAD` |
//! | `stack_frames` | `u64` frame ID | `STACK_FRAME` |
//! | `stack_traces` | `u32` stack trace serial | `STACK_TRACE` |

use std::collections::HashMap;

use crate::{ClassDef, HprofString, HprofThread, StackFrame, StackTrace};

/// O(1) lookup index populated by a single sequential pass over an hprof file.
///
/// All five maps are public so callers can inspect them directly. Maps are
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
