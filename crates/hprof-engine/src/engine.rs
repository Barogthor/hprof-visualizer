//! `NavigationEngine` trait and view model types for the TUI frontend.
//!
//! Defines the high-level API consumed by the TUI without exposing any
//! `hprof-parser` internals. All concrete types returned by the trait are
//! defined here alongside the trait itself.

use hprof_api::MemorySize;

/// Thread execution state, inferred from heap dump object data.
///
/// `Unknown` is returned until Story 3.4 resolves state from the
/// Thread object's instance dump via the object resolver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadState {
    Unknown,
    Runnable,
    Waiting,
    Blocked,
}

/// Minimal information about a Java thread found in the heap dump.
#[derive(Debug, Clone)]
pub struct ThreadInfo {
    /// The serial number assigned to this thread in the `START_THREAD` record.
    pub thread_serial: u32,
    /// Thread name resolved from structural strings, or `"<unknown:{id}>"` if
    /// the string record is missing.
    pub name: String,
    /// Execution state. `Unknown` until Story 3.4 resolves it via object
    /// resolution.
    pub state: ThreadState,
}

/// Line number information for a stack frame.
///
/// Encodes the `i32` line_number field from `STACK_FRAME` records:
/// - `> 0` → `Line(n)` (actual source line)
/// - `0` → `NoInfo` (no line information available)
/// - `-1` → `Unknown`
/// - `-2` → `Compiled` (optimised method)
/// - `_ < -2` → `Native`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineNumber {
    Line(u32),
    NoInfo,
    Unknown,
    Compiled,
    Native,
}

impl LineNumber {
    /// Converts a raw `i32` from a `STACK_FRAME` record to a [`LineNumber`].
    pub fn from_raw(n: i32) -> Self {
        match n {
            n if n > 0 => LineNumber::Line(n as u32),
            0 => LineNumber::NoInfo,
            -1 => LineNumber::Unknown,
            -2 => LineNumber::Compiled,
            _ => LineNumber::Native,
        }
    }
}

/// Display information for one stack frame.
#[derive(Debug, Clone)]
pub struct FrameInfo {
    /// Unique frame identifier from the `STACK_FRAME` record.
    pub frame_id: u64,
    /// Human-readable method name (resolved from structural strings).
    pub method_name: String,
    /// Human-readable class name (JVM binary name → Java simple name).
    pub class_name: String,
    /// Source file name, or empty string if the string ID resolved to nothing.
    pub source_file: String,
    /// Source line number.
    pub line: LineNumber,
    /// Whether this frame has GC root variables that can be expanded.
    pub has_variables: bool,
}

/// The value of a local variable (GC root) for a stack frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VariableValue {
    /// Null reference (object_id == 0).
    Null,
    /// Non-null object reference with resolved class name.
    ObjectRef {
        /// Heap object ID.
        id: u64,
        /// Resolved class name, or `"Object"` if unresolvable.
        class_name: String,
        /// Element count if the object is a collection or array, `None` otherwise.
        entry_count: Option<u64>,
    },
}

/// One local variable entry for a stack frame.
///
/// hprof `GC_ROOT_JAVA_FRAME` records carry object IDs but no variable names.
/// Variables are numbered by their 0-based position in the root list.
#[derive(Debug, Clone)]
pub struct VariableInfo {
    /// 0-based index in the frame's root list (used as display label).
    pub index: usize,
    /// Resolved variable value.
    pub value: VariableValue,
}

/// Value of one object field, decoded from instance data bytes.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldValue {
    /// Object reference with ID 0 (null).
    Null,
    /// Non-null object reference.
    ///
    /// - `id`: heap object ID
    /// - `class_name`: Java dot-notation class name
    ///   (e.g. `"java.util.HashMap"`),
    ///   or `"Object"` when not resolved
    /// - `entry_count`: collection element count if the object
    ///   is a known collection type, `None` otherwise
    /// - `inline_value`: eagerly resolved display value for
    ///   wrapper types (String, Integer, Boolean …)
    ObjectRef {
        id: u64,
        class_name: String,
        entry_count: Option<u64>,
        inline_value: Option<String>,
    },
    Bool(bool),
    /// UTF-16 code unit decoded to Rust `char`
    /// (replacement char on invalid).
    Char(char),
    Float(f32),
    Double(f64),
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
}

/// One field of an expanded object instance.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldInfo {
    /// Human-readable field name resolved from structural strings.
    pub name: String,
    /// Decoded field value.
    pub value: FieldValue,
}

/// One entry in a paginated collection view.
#[derive(Debug, Clone, PartialEq)]
pub struct EntryInfo {
    /// 0-based position of this entry in the collection.
    pub index: usize,
    /// Key for map-like collections, `None` for lists/arrays.
    pub key: Option<FieldValue>,
    /// The entry value.
    pub value: FieldValue,
}

/// A page of collection entries returned by
/// [`NavigationEngine::get_page`].
#[derive(Debug, Clone)]
pub struct CollectionPage {
    /// Resolved entries for this page.
    pub entries: Vec<EntryInfo>,
    /// Total number of entries in the collection.
    pub total_count: u64,
    /// Offset of the first entry in this page.
    pub offset: usize,
    /// Whether more entries exist beyond this page.
    pub has_more: bool,
}

impl MemorySize for ThreadInfo {
    fn memory_size(&self) -> usize {
        std::mem::size_of::<Self>() + self.name.capacity()
    }
}

impl MemorySize for FrameInfo {
    fn memory_size(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.method_name.capacity()
            + self.class_name.capacity()
            + self.source_file.capacity()
    }
}

impl MemorySize for VariableValue {
    fn memory_size(&self) -> usize {
        std::mem::size_of::<Self>()
            + match self {
                VariableValue::Null => 0,
                VariableValue::ObjectRef { class_name, .. } => class_name.capacity(),
            }
    }
}

impl MemorySize for VariableInfo {
    fn memory_size(&self) -> usize {
        std::mem::size_of::<Self>() + self.value.memory_size()
            - std::mem::size_of::<VariableValue>()
    }
}

impl MemorySize for FieldValue {
    fn memory_size(&self) -> usize {
        std::mem::size_of::<Self>()
            + match self {
                FieldValue::ObjectRef {
                    class_name,
                    inline_value,
                    ..
                } => class_name.capacity() + inline_value.as_ref().map_or(0, |s| s.capacity()),
                _ => 0,
            }
    }
}

impl MemorySize for FieldInfo {
    fn memory_size(&self) -> usize {
        std::mem::size_of::<Self>() + self.name.capacity() + self.value.memory_size()
            - std::mem::size_of::<FieldValue>()
    }
}

impl MemorySize for EntryInfo {
    fn memory_size(&self) -> usize {
        std::mem::size_of::<Self>()
            + self
                .key
                .as_ref()
                .map_or(0, |k| k.memory_size() - std::mem::size_of::<FieldValue>())
            + self.value.memory_size()
            - std::mem::size_of::<FieldValue>()
    }
}

impl MemorySize for CollectionPage {
    fn memory_size(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.entries.capacity() * std::mem::size_of::<EntryInfo>()
            + self
                .entries
                .iter()
                .map(|e| e.memory_size() - std::mem::size_of::<EntryInfo>())
                .sum::<usize>()
    }
}

/// High-level navigation API consumed by the TUI frontend.
///
/// Implemented by [`crate::Engine`]. All methods are pure reads; the engine
/// holds no mutable state at this abstraction level.
pub trait NavigationEngine {
    /// Returns non-fatal parse warnings collected during indexing.
    fn warnings(&self) -> &[String];

    /// Returns all threads indexed in the heap dump, with names resolved
    /// from structural strings. Sorted by `thread_serial` for determinism.
    fn list_threads(&self) -> Vec<ThreadInfo>;

    /// Returns `Some(ThreadInfo)` for the given `thread_serial`, `None` if
    /// not found.
    fn select_thread(&self, thread_serial: u32) -> Option<ThreadInfo>;

    /// Returns the stack frames for the given thread serial.
    fn get_stack_frames(&self, thread_serial: u32) -> Vec<FrameInfo>;

    /// Returns the local variables for the given frame ID.
    fn get_local_variables(&self, frame_id: u64) -> Vec<VariableInfo>;

    /// Expands an object and returns its decoded fields.
    ///
    /// Returns `None` if the object cannot be resolved (not in file or
    /// BinaryFuse8 false positive).
    fn expand_object(&self, object_id: u64) -> Option<Vec<FieldInfo>>;

    /// Returns the class object ID of an instance object.
    ///
    /// Returns `None` when `object_id` cannot be resolved to an instance.
    fn class_of_object(&self, object_id: u64) -> Option<u64>;

    /// Returns static fields for a class object as resolved [`FieldInfo`] rows.
    ///
    /// Returns an empty vector when the class has no static fields or the class
    /// dump is unavailable.
    fn get_static_fields(&self, class_object_id: u64) -> Vec<FieldInfo>;

    /// Returns a page of entries from a paginated collection.
    ///
    /// Returns `None` if the object is not found, not a
    /// supported collection type, or cannot be paginated.
    fn get_page(&self, collection_id: u64, offset: usize, limit: usize) -> Option<CollectionPage>;

    /// Resolves the content of a `java.lang.String` object from the hprof file.
    ///
    /// Returns `Some(value)` if the String's backing primitive array is found and
    /// decoded, `None` if the object or its backing array cannot be located.
    fn resolve_string(&self, object_id: u64) -> Option<String>;

    /// Returns total bytes currently tracked by the memory
    /// budget counter (PreciseIndex + thread cache + any
    /// expanded objects/pages).
    fn memory_used(&self) -> usize;

    /// Returns the memory budget in bytes.
    ///
    /// Auto-calculated (50% of RAM) or explicitly set via
    /// `--memory-limit` / config.
    fn memory_budget(&self) -> u64;

    /// Returns the percentage of attempted records successfully indexed
    /// (100.0 when the file is complete, < 100.0 for truncated/corrupt files).
    fn indexing_ratio(&self) -> f64;

    /// Returns `true` when every attempted record was successfully indexed.
    ///
    /// Uses integer comparison to avoid floating-point imprecision.
    /// Prefer this over `indexing_ratio() == 100.0`.
    fn is_fully_indexed(&self) -> bool;

    /// Returns the byte size of the non-evictable skeleton — the
    /// `PreciseIndex` held permanently in `HprofFile`.
    fn skeleton_bytes(&self) -> usize;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyEngine;

    impl NavigationEngine for DummyEngine {
        fn warnings(&self) -> &[String] {
            &[]
        }
        fn list_threads(&self) -> Vec<ThreadInfo> {
            vec![]
        }
        fn select_thread(&self, _thread_serial: u32) -> Option<ThreadInfo> {
            None
        }
        fn get_stack_frames(&self, _thread_serial: u32) -> Vec<FrameInfo> {
            vec![FrameInfo {
                frame_id: 1,
                method_name: "foo".to_string(),
                class_name: "Bar".to_string(),
                source_file: "Bar.java".to_string(),
                line: LineNumber::Line(42),
                has_variables: false,
            }]
        }
        fn get_local_variables(&self, _frame_id: u64) -> Vec<VariableInfo> {
            vec![VariableInfo {
                index: 0,
                value: VariableValue::Null,
            }]
        }
        fn expand_object(&self, _object_id: u64) -> Option<Vec<FieldInfo>> {
            Some(vec![])
        }
        fn class_of_object(&self, _object_id: u64) -> Option<u64> {
            None
        }
        fn get_static_fields(&self, _class_object_id: u64) -> Vec<FieldInfo> {
            vec![]
        }
        fn get_page(
            &self,
            _collection_id: u64,
            _offset: usize,
            _limit: usize,
        ) -> Option<CollectionPage> {
            None
        }
        fn resolve_string(&self, _object_id: u64) -> Option<String> {
            None
        }
        fn memory_used(&self) -> usize {
            0
        }
        fn memory_budget(&self) -> u64 {
            u64::MAX
        }
        fn indexing_ratio(&self) -> f64 {
            100.0
        }
        fn is_fully_indexed(&self) -> bool {
            true
        }
        fn skeleton_bytes(&self) -> usize {
            0
        }
    }

    mod enum_variants {
        //! Smoke-tests verifying that enum variants can be constructed and are distinct.
        use super::*;

        #[test]
        fn field_value_variants_exist() {
            let _null = FieldValue::Null;
            let _obj = FieldValue::ObjectRef {
                id: 42,
                class_name: "Object".to_string(),
                entry_count: None,
                inline_value: None,
            };
            let _b = FieldValue::Bool(true);
            let _c = FieldValue::Char('A');
            let _f = FieldValue::Float(1.0);
            let _d = FieldValue::Double(2.0);
            let _byte = FieldValue::Byte(-1);
            let _short = FieldValue::Short(100);
            let _int = FieldValue::Int(42);
            let _long = FieldValue::Long(i64::MAX);
            assert_eq!(_null, FieldValue::Null);
        }

        #[test]
        fn variable_value_null_and_object_ref_variants_exist() {
            assert_eq!(VariableValue::Null, VariableValue::Null);
            let oref = VariableValue::ObjectRef {
                id: 1,
                class_name: "Object".to_string(),
                entry_count: None,
            };
            assert_eq!(
                oref,
                VariableValue::ObjectRef {
                    id: 1,
                    class_name: "Object".to_string(),
                    entry_count: None,
                }
            );
            assert_ne!(
                VariableValue::Null,
                VariableValue::ObjectRef {
                    id: 0,
                    class_name: "Object".to_string(),
                    entry_count: None,
                }
            );
        }

        #[test]
        fn thread_state_variants_are_distinct() {
            assert_ne!(ThreadState::Unknown, ThreadState::Runnable);
            assert_ne!(ThreadState::Runnable, ThreadState::Waiting);
            assert_ne!(ThreadState::Waiting, ThreadState::Blocked);
        }
    }

    mod struct_construction {
        //! Verifies the presence and types of fields on domain structs.
        use super::*;

        #[test]
        fn field_info_has_name_and_value() {
            let f = FieldInfo {
                name: "count".to_string(),
                value: FieldValue::Int(42),
            };
            assert_eq!(f.name, "count");
            assert_eq!(f.value, FieldValue::Int(42));
        }

        #[test]
        fn frame_info_has_required_fields() {
            let f = FrameInfo {
                frame_id: 10,
                method_name: "run".to_string(),
                class_name: "Thread".to_string(),
                source_file: "Thread.java".to_string(),
                line: LineNumber::Line(100),
                has_variables: true,
            };
            assert_eq!(f.frame_id, 10);
            assert_eq!(f.method_name, "run");
            assert_eq!(f.class_name, "Thread");
            assert_eq!(f.source_file, "Thread.java");
            assert_eq!(f.line, LineNumber::Line(100));
        }

        #[test]
        fn variable_info_has_index_and_value() {
            let v = VariableInfo {
                index: 2,
                value: VariableValue::ObjectRef {
                    id: 0xDEAD,
                    class_name: "Object".to_string(),
                    entry_count: None,
                },
            };
            assert_eq!(v.index, 2);
            assert_eq!(
                v.value,
                VariableValue::ObjectRef {
                    id: 0xDEAD,
                    class_name: "Object".to_string(),
                    entry_count: None,
                }
            );
        }

        #[test]
        fn thread_info_has_state_field_of_type_thread_state() {
            let info = ThreadInfo {
                thread_serial: 1,
                name: "main".to_string(),
                state: ThreadState::Unknown,
            };
            assert_eq!(info.state, ThreadState::Unknown);
        }
    }

    mod navigation_engine_trait {
        //! Validates that the `NavigationEngine` trait compiles and that `DummyEngine`
        //! covers all methods with coherent values.
        use super::*;

        #[test]
        fn navigation_engine_trait_compiles_with_all_methods() {
            let engine = DummyEngine;
            assert!(engine.warnings().is_empty());
            assert!(engine.list_threads().is_empty());
            assert!(engine.select_thread(0).is_none());
            assert_eq!(engine.get_stack_frames(0).len(), 1);
            assert_eq!(engine.get_local_variables(0).len(), 1);
            assert!(engine.expand_object(0).unwrap().is_empty());
            assert!(engine.class_of_object(0).is_none());
            assert!(engine.get_static_fields(0).is_empty());
            assert!(engine.get_page(0, 0, 10).is_none());
        }

        #[test]
        fn indexing_ratio_100_for_complete_file() {
            assert_eq!(DummyEngine.indexing_ratio(), 100.0);
        }

        #[test]
        fn is_fully_indexed_true_for_complete_file() {
            assert!(DummyEngine.is_fully_indexed());
        }

        #[test]
        fn skeleton_bytes_zero_for_dummy() {
            assert_eq!(DummyEngine.skeleton_bytes(), 0);
        }
    }

    mod line_number {
        //! Covers all conversion cases of `LineNumber::from_raw()`: positive, zero,
        //! -1, -2, and values below -2.
        use super::*;

        #[test]
        fn line_number_from_raw_positive_gives_line() {
            assert_eq!(LineNumber::from_raw(42), LineNumber::Line(42));
        }

        #[test]
        fn line_number_from_raw_zero_gives_no_info() {
            assert_eq!(LineNumber::from_raw(0), LineNumber::NoInfo);
        }

        #[test]
        fn line_number_from_raw_minus_one_gives_unknown() {
            assert_eq!(LineNumber::from_raw(-1), LineNumber::Unknown);
        }

        #[test]
        fn line_number_from_raw_minus_two_gives_compiled() {
            assert_eq!(LineNumber::from_raw(-2), LineNumber::Compiled);
        }

        #[test]
        fn line_number_from_raw_less_than_minus_two_gives_native() {
            assert_eq!(LineNumber::from_raw(-3), LineNumber::Native);
            assert_eq!(LineNumber::from_raw(-100), LineNumber::Native);
        }
    }

    mod memory_size {
        //! Validates the `MemorySize` trait implementation for all domain types:
        //! `ThreadInfo`, `FrameInfo`, `FieldValue`, `FieldInfo`, `VariableValue`,
        //! `VariableInfo`, `EntryInfo`, `CollectionPage`.
        use super::*;

        #[test]
        fn thread_info_memory_size_includes_name() {
            let info = ThreadInfo {
                thread_serial: 1,
                name: String::from("main-thread"),
                state: ThreadState::Runnable,
            };
            let expected = std::mem::size_of::<ThreadInfo>() + info.name.capacity();
            assert_eq!(info.memory_size(), expected);
        }

        #[test]
        fn frame_info_memory_size_includes_strings() {
            let f = FrameInfo {
                frame_id: 1,
                method_name: "run".to_string(),
                class_name: "Thread".to_string(),
                source_file: "Thread.java".to_string(),
                line: LineNumber::Line(42),
                has_variables: false,
            };
            let expected = std::mem::size_of::<FrameInfo>()
                + f.method_name.capacity()
                + f.class_name.capacity()
                + f.source_file.capacity();
            assert_eq!(f.memory_size(), expected);
        }

        #[test]
        fn field_value_null_returns_static_size() {
            let v = FieldValue::Null;
            assert_eq!(v.memory_size(), std::mem::size_of::<FieldValue>());
        }

        #[test]
        fn field_value_object_ref_includes_strings() {
            let v = FieldValue::ObjectRef {
                id: 42,
                class_name: "java.lang.String".to_string(),
                entry_count: None,
                inline_value: Some("hello".to_string()),
            };
            let expected =
                std::mem::size_of::<FieldValue>() + "java.lang.String".len() + "hello".len();
            assert!(v.memory_size() >= expected);
        }

        #[test]
        fn field_info_memory_size_includes_name_and_value() {
            let f = FieldInfo {
                name: "count".to_string(),
                value: FieldValue::Int(42),
            };
            let expected = std::mem::size_of::<FieldInfo>() + f.name.capacity();
            assert_eq!(f.memory_size(), expected);
        }

        #[test]
        fn variable_value_null_returns_static_size() {
            let v = VariableValue::Null;
            assert_eq!(v.memory_size(), std::mem::size_of::<VariableValue>());
        }

        #[test]
        fn variable_value_obj_ref_includes_class_name() {
            let v = VariableValue::ObjectRef {
                id: 1,
                class_name: "Object".to_string(),
                entry_count: None,
            };
            let expected = std::mem::size_of::<VariableValue>() + "Object".len();
            assert!(v.memory_size() >= expected);
        }

        #[test]
        fn variable_info_memory_size() {
            let v = VariableInfo {
                index: 0,
                value: VariableValue::Null,
            };
            assert_eq!(v.memory_size(), std::mem::size_of::<VariableInfo>());
        }

        #[test]
        fn entry_info_memory_size() {
            let e = EntryInfo {
                index: 0,
                key: None,
                value: FieldValue::Int(1),
            };
            assert_eq!(e.memory_size(), std::mem::size_of::<EntryInfo>());
        }

        #[test]
        fn entry_info_memory_size_object_ref_value_includes_heap() {
            let class_name = "java.lang.String".to_string();
            let e = EntryInfo {
                index: 0,
                key: None,
                value: FieldValue::ObjectRef {
                    id: 1,
                    class_name: class_name.clone(),
                    entry_count: None,
                    inline_value: None,
                },
            };
            let expected = std::mem::size_of::<EntryInfo>() + class_name.capacity();
            assert_eq!(e.memory_size(), expected);
        }

        #[test]
        fn entry_info_memory_size_key_and_value_object_refs() {
            let key_name = "key_class".to_string();
            let val_name = "val_class".to_string();
            let e = EntryInfo {
                index: 0,
                key: Some(FieldValue::ObjectRef {
                    id: 1,
                    class_name: key_name.clone(),
                    entry_count: None,
                    inline_value: None,
                }),
                value: FieldValue::ObjectRef {
                    id: 2,
                    class_name: val_name.clone(),
                    entry_count: None,
                    inline_value: None,
                },
            };
            let expected =
                std::mem::size_of::<EntryInfo>() + key_name.capacity() + val_name.capacity();
            assert_eq!(e.memory_size(), expected);
        }

        #[test]
        fn collection_page_memory_size_includes_entries() {
            let page = CollectionPage {
                entries: vec![EntryInfo {
                    index: 0,
                    key: None,
                    value: FieldValue::Int(1),
                }],
                total_count: 1,
                offset: 0,
                has_more: false,
            };
            assert!(page.memory_size() > std::mem::size_of::<CollectionPage>());
        }
    }
}
