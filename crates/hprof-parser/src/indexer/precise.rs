//! In-memory O(1) indexes for all structural hprof records, built by the
//! first-pass indexer.
//!
//! [`PreciseIndex`] holds [`FxHashMap`] collections, each keyed by the
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
//! | `field_names` | `u64` string ID | derived from `CLASS_DUMP` field metadata |

use std::sync::RwLock;

use rustc_hash::FxHashMap;

use hprof_api::{MemorySize, fxhashmap_memory_size};

use crate::{ClassDef, ClassDumpInfo, HprofStringRef, HprofThread, StackFrame, StackTrace};

/// Thread-safe wrapper around `FxHashMap<u64, u64>` for
/// instance offset caching.
///
/// Uses `std::sync::RwLock` for interior mutability.
/// Callers use `.get()`, `.insert()`, `.insert_batch()`
/// without knowing about the lock.
///
/// # Lock-poisoning policy
///
/// All methods recover from a poisoned `RwLock` via
/// `unwrap_or_else(|e| e.into_inner())`. This is safe
/// because the cache is **insert-only**: no method
/// removes or reorders entries, so a panic during a
/// write cannot leave the map in an inconsistent
/// state. The worst case is a missing entry, which
/// callers already handle (all lookups return
/// `Option`).
#[derive(Debug)]
pub(crate) struct OffsetCache {
    inner: RwLock<FxHashMap<u64, u64>>,
}

impl OffsetCache {
    /// Creates an empty cache.
    pub(crate) fn new() -> Self {
        Self {
            inner: RwLock::new(FxHashMap::default()),
        }
    }

    /// Returns the offset for `id`, if cached.
    pub(crate) fn get(&self, id: u64) -> Option<u64> {
        self.inner
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .copied()
    }

    /// Inserts a single offset.
    pub(crate) fn insert(&self, id: u64, offset: u64) {
        self.inner
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, offset);
    }

    /// Inserts all entries from `offsets` in a single
    /// write-lock acquisition.
    pub(crate) fn insert_batch(&self, offsets: &FxHashMap<u64, u64>) {
        let mut guard = self.inner.write().unwrap_or_else(|e| e.into_inner());
        for (&id, &off) in offsets {
            guard.insert(id, off);
        }
    }

    /// Returns the number of cached entries.
    pub(crate) fn len(&self) -> usize {
        self.inner.read().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// Returns `true` if `id` is in the cache.
    pub(crate) fn contains(&self, id: &u64) -> bool {
        self.inner
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(id)
    }

    /// Returns all values as a `Vec`.
    pub(crate) fn values(&self) -> Vec<u64> {
        self.inner
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .copied()
            .collect()
    }

    /// Returns all keys as a `Vec`.
    #[cfg(test)]
    pub(crate) fn keys(&self) -> Vec<u64> {
        self.inner
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .copied()
            .collect()
    }

    /// Returns the capacity of the inner map (for
    /// memory size calculations).
    pub(crate) fn capacity(&self) -> usize {
        self.inner
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .capacity()
    }
}

impl Default for OffsetCache {
    fn default() -> Self {
        Self::new()
    }
}

/// O(1) lookup index populated by a single sequential pass over an hprof file.
///
/// All maps are public so callers can inspect them directly. Maps are
/// populated by [`crate::indexer::first_pass::run_first_pass`] and are
/// read-only after construction.
#[derive(Debug)]
pub struct PreciseIndex {
    /// `STRING_IN_UTF8` records keyed by string object ID.
    pub strings: FxHashMap<u64, HprofStringRef>,
    /// `LOAD_CLASS` records keyed by class serial number.
    pub classes: FxHashMap<u32, ClassDef>,
    /// `START_THREAD` records keyed by thread serial number.
    pub threads: FxHashMap<u32, HprofThread>,
    /// `STACK_FRAME` records keyed by frame ID.
    pub stack_frames: FxHashMap<u64, StackFrame>,
    /// `STACK_TRACE` records keyed by stack trace serial number.
    pub stack_traces: FxHashMap<u32, StackTrace>,
    /// GC root object IDs keyed by frame ID. Populated during first
    /// pass by correlating `GC_ROOT_JAVA_FRAME` sub-records with
    /// `STACK_TRACE` records.
    ///
    /// Key: `frame_id` (u64) — Value: Vec of object IDs rooted at
    /// that frame.
    pub java_frame_roots: FxHashMap<u64, Vec<u64>>,
    /// `CLASS_DUMP` sub-records keyed by `class_object_id`.
    pub class_dumps: FxHashMap<u64, ClassDumpInfo>,
    /// `ROOT_THREAD_OBJ` heap object IDs keyed by thread serial.
    /// Maps thread_serial → object_id from the heap.
    pub thread_object_ids: FxHashMap<u32, u64>,
    /// Java class names keyed by `class_object_id`.
    ///
    /// Populated from `LOAD_CLASS` records during the first pass.
    /// Binary JVM names (`java/util/HashMap`) are normalised to
    /// dot notation (`java.util.HashMap`).
    pub class_names_by_id: FxHashMap<u64, String>,
    /// Field names keyed by `name_string_id`.
    ///
    /// Populated after heap extraction by resolving unique
    /// `ClassDumpInfo.instance_fields` and
    /// `ClassDumpInfo.static_fields` name IDs through
    /// `strings`.
    pub field_names: FxHashMap<u64, String>,
    /// Object-ID → byte offset (relative to records section) for
    /// cached instance locations. Initially populated with
    /// thread-related objects during first pass; later extended
    /// by batch-scan results at runtime.
    ///
    /// Uses interior mutability (`RwLock`) so callers can insert
    /// offsets without `&mut self` on `HprofFile`.
    ///
    /// Access through delegating methods (`get_offset`,
    /// `insert_offset`, etc.) rather than this field directly.
    pub(crate) instance_offsets: OffsetCache,
}

impl PreciseIndex {
    /// Creates a new empty index with no pre-allocation.
    pub fn new() -> Self {
        Self {
            strings: FxHashMap::default(),
            classes: FxHashMap::default(),
            threads: FxHashMap::default(),
            stack_frames: FxHashMap::default(),
            stack_traces: FxHashMap::default(),
            java_frame_roots: FxHashMap::default(),
            class_dumps: FxHashMap::default(),
            thread_object_ids: FxHashMap::default(),
            class_names_by_id: FxHashMap::default(),
            field_names: FxHashMap::default(),
            instance_offsets: OffsetCache::new(),
        }
    }

    /// Creates a new index with pre-allocated maps sized from
    /// `data_len` (byte length of the records section).
    ///
    /// Capacities are capped to avoid multi-GB reservations on
    /// very large dumps (>10 GB).
    pub fn with_capacity(data_len: usize) -> Self {
        let string_cap = (data_len / 300).min(500_000);
        let class_cap = (data_len / 5000).min(100_000);
        Self {
            strings: FxHashMap::with_capacity_and_hasher(string_cap, Default::default()),
            classes: FxHashMap::with_capacity_and_hasher(class_cap, Default::default()),
            threads: FxHashMap::default(),
            stack_frames: FxHashMap::default(),
            stack_traces: FxHashMap::default(),
            java_frame_roots: FxHashMap::default(),
            class_dumps: FxHashMap::with_capacity_and_hasher(class_cap, Default::default()),
            thread_object_ids: FxHashMap::default(),
            class_names_by_id: FxHashMap::with_capacity_and_hasher(class_cap, Default::default()),
            field_names: FxHashMap::with_capacity_and_hasher(class_cap, Default::default()),
            instance_offsets: OffsetCache::new(),
        }
    }
}

// ── Public offset-cache façade ──────────────────────────

impl PreciseIndex {
    /// Returns the cached byte offset for `id`, if any.
    pub fn get_offset(&self, id: u64) -> Option<u64> {
        self.instance_offsets.get(id)
    }

    /// Caches a single object-ID → byte-offset mapping.
    pub fn insert_offset(&self, id: u64, offset: u64) {
        self.instance_offsets.insert(id, offset);
    }

    /// Caches multiple object-ID → byte-offset mappings
    /// in a single lock acquisition.
    pub fn insert_offset_batch(&self, offsets: &FxHashMap<u64, u64>) {
        self.instance_offsets.insert_batch(offsets);
    }

    /// Returns `true` if `id` has a cached offset.
    pub fn contains_offset(&self, id: &u64) -> bool {
        self.instance_offsets.contains(id)
    }

    /// Returns the number of cached offsets.
    pub fn offset_count(&self) -> usize {
        self.instance_offsets.len()
    }
}

impl MemorySize for PreciseIndex {
    fn memory_size(&self) -> usize {
        std::mem::size_of::<Self>()
            + fxhashmap_memory_size::<u64, HprofStringRef>(self.strings.capacity())
            + fxhashmap_memory_size::<u32, ClassDef>(self.classes.capacity())
            + fxhashmap_memory_size::<u32, HprofThread>(self.threads.capacity())
            + fxhashmap_memory_size::<u64, StackFrame>(self.stack_frames.capacity())
            + fxhashmap_memory_size::<u32, StackTrace>(self.stack_traces.capacity())
            + self
                .stack_traces
                .values()
                .map(|st| st.frame_ids.capacity() * std::mem::size_of::<u64>())
                .sum::<usize>()
            + fxhashmap_memory_size::<u64, Vec<u64>>(self.java_frame_roots.capacity())
            + self
                .java_frame_roots
                .values()
                .map(|v| v.capacity() * std::mem::size_of::<u64>())
                .sum::<usize>()
            + fxhashmap_memory_size::<u64, ClassDumpInfo>(self.class_dumps.capacity())
            + self
                .class_dumps
                .values()
                .map(|cd| {
                    cd.instance_fields.capacity() * std::mem::size_of::<crate::FieldDef>()
                        + cd.static_fields.capacity() * std::mem::size_of::<crate::StaticFieldDef>()
                })
                .sum::<usize>()
            + fxhashmap_memory_size::<u32, u64>(self.thread_object_ids.capacity())
            + fxhashmap_memory_size::<u64, String>(self.class_names_by_id.capacity())
            + self
                .class_names_by_id
                .values()
                .map(|s| s.capacity())
                .sum::<usize>()
            + fxhashmap_memory_size::<u64, String>(self.field_names.capacity())
            + self
                .field_names
                .values()
                .map(|s| s.capacity())
                .sum::<usize>()
            + fxhashmap_memory_size::<u64, u64>(self.instance_offsets.capacity())
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
    use crate::{ClassDef, HprofStringRef, HprofThread, StackFrame, StackTrace};

    // ── Task 2.3: batch-resolve then verify offsets ──

    #[test]
    fn offset_cache_insert_batch_and_contains() {
        let cache = OffsetCache::new();
        let mut batch = FxHashMap::default();
        batch.insert(0xAA, 100);
        batch.insert(0xBB, 200);
        batch.insert(0xCC, 300);

        cache.insert_batch(&batch);

        assert!(cache.contains(&0xAA));
        assert!(cache.contains(&0xBB));
        assert!(cache.contains(&0xCC));
        assert!(!cache.contains(&0xDD));
        assert_eq!(cache.len(), 3);
    }

    // ── Task 2.4: cached offset enables O(1) path ──

    #[test]
    fn offset_cache_get_returns_inserted_offset() {
        let cache = OffsetCache::new();
        cache.insert(0xAA, 42);

        assert_eq!(cache.get(0xAA), Some(42));
        assert_eq!(cache.get(0xBB), None);
    }

    // ── Task 2.5: concurrent access safety ──

    #[test]
    fn offset_cache_concurrent_insert_batch_and_read() {
        use std::sync::Arc;
        use std::thread;

        let cache = Arc::new(OffsetCache::new());
        let mut batch = FxHashMap::default();
        for i in 0..100u64 {
            batch.insert(i, i * 10);
        }

        let cache_w = Arc::clone(&cache);
        let batch_clone = batch.clone();
        let writer = thread::spawn(move || {
            cache_w.insert_batch(&batch_clone);
        });

        let cache_r = Arc::clone(&cache);
        let reader = thread::spawn(move || {
            // Spin-read until all entries visible.
            loop {
                let count = cache_r.len();
                if count == 100 {
                    break;
                }
                std::hint::spin_loop();
            }
            // Verify all 100 entries.
            for i in 0..100u64 {
                assert_eq!(cache_r.get(i), Some(i * 10), "entry {i} must be visible");
            }
        });

        writer.join().unwrap();
        reader.join().unwrap();

        assert_eq!(cache.len(), 100);
    }

    #[test]
    fn memory_size_empty_index_equals_static_size() {
        let index = PreciseIndex::new();
        assert_eq!(index.memory_size(), std::mem::size_of::<PreciseIndex>());
    }

    #[test]
    fn memory_size_populated_exceeds_static_size() {
        let mut index = PreciseIndex::new();
        index.strings.insert(
            1,
            HprofStringRef {
                id: 1,
                offset: 0,
                len: 5,
            },
        );
        index
            .class_names_by_id
            .insert(100, "java.lang.String".to_string());
        index.java_frame_roots.insert(10, vec![1, 2, 3]);
        index.stack_traces.insert(
            1,
            StackTrace {
                stack_trace_serial: 1,
                thread_serial: 1,
                frame_ids: vec![10, 20],
            },
        );
        let total = index.memory_size();
        let static_size = std::mem::size_of::<PreciseIndex>();
        assert!(
            total > static_size,
            "populated index ({total}) must exceed \
             static size ({static_size})"
        );
    }

    #[test]
    fn memory_size_accounts_for_string_capacity() {
        let mut index = PreciseIndex::new();
        let long_name = "a".repeat(200);
        index.class_names_by_id.insert(1, long_name.clone());
        let total = index.memory_size();
        assert!(total >= 200, "must include string capacity ({total})");
    }

    #[test]
    fn memory_size_accounts_for_field_name_capacity() {
        let mut index = PreciseIndex::new();
        let long_name = "b".repeat(240);
        index.field_names.insert(7, long_name);
        let total = index.memory_size();
        assert!(
            total >= std::mem::size_of::<PreciseIndex>() + 240,
            "must include field_names string capacity ({total})"
        );
    }

    #[test]
    fn field_names_map_is_string_id_to_name() {
        let mut index = PreciseIndex::new();
        let _: &FxHashMap<u64, String> = &index.field_names;
        index.field_names.insert(0xAA, "size".to_string());
        assert_eq!(
            index.field_names.get(&0xAA).map(String::as_str),
            Some("size")
        );
    }

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
        assert!(index.field_names.is_empty());
    }

    #[test]
    fn insert_and_retrieve_string_ref_by_id() {
        let mut index = PreciseIndex::new();
        index.strings.insert(
            5,
            HprofStringRef {
                id: 5,
                offset: 100,
                len: 5,
            },
        );
        let s = index.strings.get(&5).unwrap();
        assert_eq!(s.id, 5);
        assert_eq!(s.offset, 100);
        assert_eq!(s.len, 5);
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
    fn offset_cache_recovers_from_poisoned_lock() {
        use std::sync::Arc;
        use std::thread;

        let cache = Arc::new(OffsetCache::new());
        cache.insert(1, 100);

        // Poison the lock by panicking while holding a write guard.
        let cache2 = Arc::clone(&cache);
        let handle = thread::spawn(move || {
            let _guard = cache2.inner.write().unwrap();
            panic!("intentional poison");
        });
        let _ = handle.join();

        // The lock is now poisoned. Operations must still work.
        assert_eq!(cache.get(1), Some(100));
        cache.insert(2, 200);
        assert_eq!(cache.get(2), Some(200));
        assert_eq!(cache.len(), 2);
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
