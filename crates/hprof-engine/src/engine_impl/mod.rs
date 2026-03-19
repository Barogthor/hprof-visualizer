//! Concrete `Engine` implementation of [`NavigationEngine`].
//!
//! Wraps an [`hprof_parser::HprofFile`] and exposes the high-level
//! navigation API. Parser internals are fully encapsulated — callers only
//! depend on `hprof-engine`.

use std::collections::HashMap as StdHashMap;
use std::sync::Mutex;

use rustc_hash::FxHashMap;
use std::path::Path;
use std::sync::Arc;

use hprof_api::{NullProgressObserver, ParseProgressObserver, ProgressNotifier};
use rayon::prelude::*;

use hprof_parser::{HprofFile, PreciseIndex, RawInstance, StaticValue};

use hprof_parser::jvm_to_java;

use crate::{
    EngineConfig, HprofError,
    cache::lru::{EVICTION_TARGET, EVICTION_TRIGGER},
    engine::{
        CollectionPage, FieldInfo, FieldValue, FrameInfo, LineNumber, NavigationEngine, ThreadInfo,
        ThreadState, VariableInfo, VariableValue,
    },
};

/// Java collection class name suffixes used for entry count detection.
const COLLECTION_CLASS_SUFFIXES: &[&str] = &[
    "HashMap",
    "LinkedHashMap",
    "TreeMap",
    "ConcurrentHashMap",
    "Hashtable",
    "ArrayList",
    "LinkedList",
    "Vector",
    "ArrayDeque",
    "HashSet",
    "LinkedHashSet",
    "TreeSet",
    "CopyOnWriteArrayList",
    "PriorityQueue",
];

/// Returns the entry count for known collection types, or `None` for plain objects.
///
/// Detects collection type by matching the short class name (after last `.`)
/// against [`COLLECTION_CLASS_SUFFIXES`]. Then searches the immediate class
/// hierarchy for an `Int` or `Long` field named `size`, `elementCount`, or
/// `count`.
pub(crate) fn collection_entry_count(
    raw: &RawInstance,
    index: &PreciseIndex,
    id_size: u32,
    records_bytes: &[u8],
) -> Option<u64> {
    let class_name = index.class_names_by_id.get(&raw.class_object_id)?;
    let short_name = class_name.rsplit('.').next().unwrap_or(class_name.as_str());
    if !COLLECTION_CLASS_SUFFIXES
        .iter()
        .any(|&s| short_name.eq_ignore_ascii_case(s))
    {
        return None;
    }

    let fields = crate::resolver::decode_fields(raw, index, id_size, records_bytes);
    for field in fields {
        if !matches!(field.name.as_str(), "size" | "elementCount" | "count") {
            continue;
        }
        match field.value {
            FieldValue::Int(v) if v >= 0 => return Some(v as u64),
            FieldValue::Long(v) if v >= 0 => return Some(v as u64),
            _ => {}
        }
    }

    None
}

/// Returns the byte size of a field given its type code.
pub(crate) fn field_byte_size(type_code: u8, id_size: u32) -> usize {
    match type_code {
        2 => id_size as usize,
        4 => 1,
        5 => 2,
        6 => 4,
        7 => 8,
        8 => 1,
        9 => 2,
        10 => 4,
        11 => 8,
        _ => 0,
    }
}

/// Returns a human-readable type name for a primitive array
/// element type code.
pub(crate) fn prim_array_type_name(elem_type: u8) -> &'static str {
    match elem_type {
        4 => "boolean",
        5 => "char",
        6 => "float",
        7 => "double",
        8 => "byte",
        9 => "short",
        10 => "int",
        11 => "long",
        _ => "unknown",
    }
}

/// Resolves an inline display value for wrapper types
/// (`String`, `Integer`, `Boolean`, …).
///
/// Returns `Some("\"text\""`) for strings, `Some("42")` for
/// boxed primitives, `None` for other classes.
pub(crate) fn resolve_inline_value(
    class_name: &str,
    hfile: &HprofFile,
    object_id: u64,
) -> Option<String> {
    if class_name == "java.lang.String" {
        let s = Engine::resolve_string_static(hfile, object_id)?;
        return Some(format!("\"{}\"", truncate_inline(s)));
    }
    if !matches!(
        class_name,
        "java.lang.Boolean"
            | "java.lang.Byte"
            | "java.lang.Short"
            | "java.lang.Integer"
            | "java.lang.Long"
            | "java.lang.Float"
            | "java.lang.Double"
            | "java.lang.Character"
    ) {
        return None;
    }
    let raw = Engine::read_instance(hfile, object_id)?;
    let fields = crate::resolver::decode_fields(
        &raw,
        &hfile.index,
        hfile.header.id_size,
        hfile.records_bytes(),
    );
    fields
        .iter()
        .find(|f| f.name == "value")
        .map(|f| match &f.value {
            crate::engine::FieldValue::Bool(b) => b.to_string(),
            crate::engine::FieldValue::Char(c) => format!("'{c}'"),
            crate::engine::FieldValue::Byte(n) => n.to_string(),
            crate::engine::FieldValue::Short(n) => n.to_string(),
            crate::engine::FieldValue::Int(n) => n.to_string(),
            crate::engine::FieldValue::Long(n) => n.to_string(),
            crate::engine::FieldValue::Float(n) => format!("{n}"),
            crate::engine::FieldValue::Double(n) => format!("{n}"),
            _ => "?".to_string(),
        })
}

/// Truncates a string for inline display (max 80 chars).
fn truncate_inline(s: String) -> String {
    if s.chars().count() <= 80 {
        s
    } else {
        let end = s.char_indices().nth(78).map(|(i, _)| i).unwrap_or(s.len());
        format!("{}..", &s[..end])
    }
}

/// Decodes a flat primitive array as a Java String.
///
/// - `elem_type == 5` (char, UTF-16BE): interprets bytes as big-endian `u16` pairs,
///   converts each to `char` via `char::from_u32`, uses replacement char `\u{FFFD}` on
///   invalid code units.
/// - `elem_type == 8` (byte, Latin-1): interprets each byte as an ISO-8859-1 character.
/// - Any other type: returns a non-fatal placeholder string.
fn decode_prim_array_as_string(elem_type: u8, bytes: &[u8]) -> String {
    match elem_type {
        5 => {
            let units: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16_lossy(&units).to_string()
        }
        8 => bytes.iter().map(|&b| b as char).collect(),
        _ => format!("<prim_array type={elem_type}>"),
    }
}

/// Cached metadata for a single thread, resolved once at load time.
struct ThreadMetadata {
    name: String,
    state: ThreadState,
}

impl hprof_api::MemorySize for ThreadMetadata {
    fn memory_size(&self) -> usize {
        std::mem::size_of::<Self>() + self.name.capacity()
    }
}

/// Navigation engine backed by a memory-mapped hprof file.
///
/// Constructed via [`Engine::from_file`]. Implements [`NavigationEngine`]
/// so the TUI frontend does not need to know about parser internals.
pub struct Engine {
    hfile: Arc<HprofFile>,
    /// Pre-resolved thread metadata (serial → name + state).
    /// Built once at construction to avoid repeated heap scans.
    thread_cache: FxHashMap<u32, ThreadMetadata>,
    /// Tracks total heap memory consumed by parsed/cached data.
    memory_counter: Arc<crate::cache::MemoryCounter>,
    /// LRU cache for expanded object fields.
    object_cache: crate::cache::ObjectCache,
    /// Skip-indexes for variable-size collection pagination.
    /// Lazily created when `offset > 0` is requested.
    skip_indexes: Mutex<StdHashMap<u64, crate::pagination::SkipIndex>>,
    /// Active background walkers keyed by collection ID.
    walkers: Mutex<StdHashMap<u64, crate::pagination::WalkerHandle>>,
}

impl Engine {
    /// Opens `path`, runs the first-pass indexer, and returns a ready-to-use
    /// engine.
    ///
    /// ## Errors
    /// - [`HprofError::MmapFailed`] — file not found or OS mapping failed.
    /// - [`HprofError::UnsupportedVersion`] — unrecognised version string.
    /// - [`HprofError::TruncatedRecord`] — file header is truncated.
    pub fn from_file(path: &Path, config: &EngineConfig) -> Result<Self, HprofError> {
        let mut null_obs = NullProgressObserver;
        let hfile = Arc::new(HprofFile::from_path_with_progress(
            path,
            &mut null_obs,
            config.memory_budget(),
        )?);
        let mut notifier = ProgressNotifier::new(&mut null_obs);
        let thread_cache = Self::build_thread_cache(&hfile, &mut notifier);
        let budget = config.effective_budget();
        let counter = Arc::new(crate::cache::MemoryCounter::new(budget));
        counter.add(Self::initial_memory(&hfile, &thread_cache));
        Ok(Self {
            hfile,
            thread_cache,
            memory_counter: counter,
            object_cache: crate::cache::ObjectCache::new(),
            skip_indexes: Mutex::new(StdHashMap::new()),
            walkers: Mutex::new(StdHashMap::new()),
        })
    }

    /// Opens `path`, runs the first-pass indexer with
    /// progress reporting, and returns a ready-to-use
    /// engine.
    ///
    /// The observer receives scan, segment, and name
    /// resolution progress signals.
    ///
    /// ## Errors
    /// See [`Engine::from_file`].
    pub fn from_file_with_progress(
        path: &Path,
        config: &EngineConfig,
        observer: &mut dyn ParseProgressObserver,
    ) -> Result<Self, HprofError> {
        let hfile = Arc::new(HprofFile::from_path_with_progress(
            path,
            observer,
            config.memory_budget(),
        )?);
        let mut notifier = ProgressNotifier::new(observer);
        let thread_cache = Self::build_thread_cache(&hfile, &mut notifier);
        let budget = config.effective_budget();
        let counter = Arc::new(crate::cache::MemoryCounter::new(budget));
        counter.add(Self::initial_memory(&hfile, &thread_cache));
        Ok(Self {
            hfile,
            thread_cache,
            memory_counter: counter,
            object_cache: crate::cache::ObjectCache::new(),
            skip_indexes: Mutex::new(StdHashMap::new()),
            walkers: Mutex::new(StdHashMap::new()),
        })
    }

    /// Resolves all thread metadata (name + state) once
    /// at load time.
    ///
    /// Processes threads in chunks with rayon `par_iter`,
    /// reporting incremental progress after each chunk.
    fn build_thread_cache(
        hfile: &HprofFile,
        notifier: &mut ProgressNotifier,
    ) -> FxHashMap<u32, ThreadMetadata> {
        let total = hfile.index.threads.len();
        if total == 0 {
            return FxHashMap::default();
        }
        let thread_list: Vec<_> = hfile.index.threads.iter().collect();
        let chunk_size = (total / 10).clamp(1, 50);
        let mut cache = FxHashMap::with_capacity_and_hasher(total, Default::default());
        for chunk in thread_list.chunks(chunk_size) {
            let results: Vec<(u32, ThreadMetadata)> = chunk
                .par_iter()
                .map(|&(&serial, t)| {
                    let heap_result = Self::resolve_thread_from_heap(hfile, t);
                    let (name, state) = match heap_result {
                        Some((n, s)) => (n, s),
                        None => {
                            let name = if t.name_string_id != 0 {
                                hfile
                                    .index
                                    .strings
                                    .get(&t.name_string_id)
                                    .map(|sref| hfile.resolve_string(sref))
                                    .unwrap_or_else(|| format!("Thread-{}", serial))
                            } else {
                                format!("Thread-{}", serial)
                            };
                            (name, ThreadState::Unknown)
                        }
                    };
                    (serial, ThreadMetadata { name, state })
                })
                .collect();
            cache.extend(results);
            notifier.names_resolved(cache.len(), total);
        }
        cache
    }

    /// Computes initial memory usage from PreciseIndex +
    /// thread_cache at construction time.
    fn initial_memory(hfile: &HprofFile, thread_cache: &FxHashMap<u32, ThreadMetadata>) -> usize {
        use hprof_api::MemorySize;
        let index_size = hfile.index.memory_size();
        let cache_size: usize = thread_cache.values().map(|tm| tm.memory_size()).sum();
        let cache_overhead =
            hprof_api::fxhashmap_memory_size::<u32, ThreadMetadata>(thread_cache.capacity());
        index_size + cache_size + cache_overhead
    }

    /// Resolves thread name and state from the heap Thread object.
    ///
    /// Returns `None` only if the thread has no heap object or
    /// `find_instance` fails. Name resolution failure still
    /// preserves the state extracted from `threadStatus`.
    fn resolve_thread_from_heap(
        hfile: &HprofFile,
        t: &hprof_parser::HprofThread,
    ) -> Option<(String, ThreadState)> {
        let &obj_id = hfile.index.thread_object_ids.get(&t.thread_serial)?;
        let raw = Self::read_instance(hfile, obj_id)?;
        let fields = crate::resolver::decode_fields(
            &raw,
            &hfile.index,
            hfile.header.id_size,
            hfile.records_bytes(),
        );

        // Extract threadStatus from the Thread instance.
        // JDK <19: threadStatus is a direct int field.
        // JDK 19+: threadStatus lives inside Thread$FieldHolder
        //          accessed via the "holder" ObjectRef field.
        let state = Self::extract_thread_status(hfile, &fields).unwrap_or(ThreadState::Unknown);

        // Extract name (ObjectRef → String instance → char[]/byte[])
        let name = Self::resolve_thread_name_from_fields(hfile, &fields);

        Some((
            name.unwrap_or_else(|| format!("Thread-{}", t.thread_serial)),
            state,
        ))
    }

    /// Extracts `threadStatus` from Thread fields (JDK <19 direct,
    /// JDK 19+ via `holder` → `FieldHolder.threadStatus`).
    fn extract_thread_status(
        hfile: &HprofFile,
        fields: &[crate::engine::FieldInfo],
    ) -> Option<ThreadState> {
        // JDK <19: direct threadStatus field
        if let Some(state) = fields.iter().find_map(|f| {
            if f.name == "threadStatus"
                && let crate::engine::FieldValue::Int(v) = f.value
            {
                Some(thread_state_from_status(v))
            } else {
                None
            }
        }) {
            return Some(state);
        }

        // JDK 19+: holder → FieldHolder.threadStatus
        let holder_id = fields.iter().find_map(|f| {
            if f.name == "holder"
                && let crate::engine::FieldValue::ObjectRef { id, .. } = f.value
            {
                Some(id)
            } else {
                None
            }
        })?;
        let holder_raw = Self::read_instance(hfile, holder_id)?;
        let holder_fields = crate::resolver::decode_fields(
            &holder_raw,
            &hfile.index,
            hfile.header.id_size,
            hfile.records_bytes(),
        );
        holder_fields.iter().find_map(|f| {
            if f.name == "threadStatus"
                && let crate::engine::FieldValue::Int(v) = f.value
            {
                Some(thread_state_from_status(v))
            } else {
                None
            }
        })
    }

    /// Reads an instance, accessible to sibling modules.
    pub(crate) fn read_instance_public(hfile: &HprofFile, obj_id: u64) -> Option<RawInstance> {
        Self::read_instance(hfile, obj_id)
    }

    /// Resolves a `java.lang.String` object to its content.
    ///
    /// Accessible to sibling modules (pagination).
    pub(crate) fn resolve_string_static(hfile: &HprofFile, object_id: u64) -> Option<String> {
        let raw = Self::read_instance(hfile, object_id)?;
        let fields = crate::resolver::decode_fields(
            &raw,
            &hfile.index,
            hfile.header.id_size,
            hfile.records_bytes(),
        );
        let value_id = fields.iter().find_map(|f| {
            if f.name == "value"
                && let crate::engine::FieldValue::ObjectRef { id, .. } = f.value
            {
                return Some(id);
            }
            None
        })?;
        let (elem_type, bytes) = hfile.find_prim_array(value_id)?;
        Some(decode_prim_array_as_string(elem_type, &bytes))
    }

    /// Reads an instance by offset if available, falling back to
    /// `find_instance`.
    fn read_instance(hfile: &HprofFile, obj_id: u64) -> Option<RawInstance> {
        if let Some(off) = hfile.index.instance_offsets.get(obj_id)
            && let Some(raw) = hfile.read_instance_at_offset(off)
        {
            return Some(raw);
        }
        if let Some((raw, offset)) = hfile.find_instance(obj_id) {
            hfile.index.instance_offsets.insert(obj_id, offset);
            return Some(raw);
        }
        None
    }

    /// Reads a primitive array by offset if available, falling back to
    /// `find_prim_array`.
    fn read_prim_array(hfile: &HprofFile, arr_id: u64) -> Option<(u8, Vec<u8>)> {
        if let Some(off) = hfile.index.instance_offsets.get(arr_id)
            && let Some(r) = hfile.read_prim_array_at_offset(off)
        {
            return Some(r);
        }
        hfile.find_prim_array(arr_id)
    }

    /// Extracts the thread name from decoded Thread instance fields.
    fn resolve_thread_name_from_fields(
        hfile: &HprofFile,
        fields: &[crate::engine::FieldInfo],
    ) -> Option<String> {
        let name_obj_id = fields.iter().find_map(|f| {
            if f.name == "name"
                && let crate::engine::FieldValue::ObjectRef { id, .. } = f.value
            {
                Some(id)
            } else {
                None
            }
        })?;
        let str_raw = Self::read_instance(hfile, name_obj_id)?;
        let str_fields = crate::resolver::decode_fields(
            &str_raw,
            &hfile.index,
            hfile.header.id_size,
            hfile.records_bytes(),
        );
        let value_id = str_fields.iter().find_map(|f| {
            if f.name == "value"
                && let crate::engine::FieldValue::ObjectRef { id, .. } = f.value
            {
                Some(id)
            } else {
                None
            }
        })?;
        let (elem_type, bytes) = Self::read_prim_array(hfile, value_id)?;
        Some(decode_prim_array_as_string(elem_type, &bytes))
    }
}

/// Maps a HotSpot `threadStatus` int to [`ThreadState`].
///
/// Uses the same bitmask logic as `java.lang.Thread.State` via
/// `jdk.internal.misc.VM.toThreadState(int)`:
/// - `0x0004` (bit 2) → RUNNABLE
/// - `0x0400` (bit 10) → BLOCKED
/// - `0x0010 | 0x0020` (bits 4-5) → WAITING / TIMED_WAITING
/// - `0x0001` (bit 0) → NEW
/// - `0x0002` (bit 1) → TERMINATED
fn thread_state_from_status(status: i32) -> ThreadState {
    if status & 0x0004 != 0 {
        ThreadState::Runnable
    } else if status & 0x0400 != 0 {
        ThreadState::Blocked
    } else if status & 0x0030 != 0 {
        ThreadState::Waiting
    } else {
        ThreadState::Unknown
    }
}

impl Engine {
    /// Resolves display metadata for an object reference.
    fn enrich_object_ref_parts(&self, object_id: u64) -> (String, Option<u64>, Option<String>) {
        if let Some(child_raw) = Self::read_instance(&self.hfile, object_id) {
            let class_name = self
                .hfile
                .index
                .class_names_by_id
                .get(&child_raw.class_object_id)
                .cloned()
                .unwrap_or_else(|| "Object".to_string());
            let inline_value = resolve_inline_value(&class_name, &self.hfile, object_id);
            let entry_count = collection_entry_count(
                &child_raw,
                &self.hfile.index,
                self.hfile.header.id_size,
                self.hfile.records_bytes(),
            );
            (class_name, entry_count, inline_value)
        } else if let Some(meta) = self.hfile.find_object_array_meta(object_id) {
            ("Object[]".to_string(), Some(meta.num_elements as u64), None)
        } else if let Some((elem_type, bytes)) = self.hfile.find_prim_array(object_id) {
            let type_name = prim_array_type_name(elem_type);
            let elem_size = field_byte_size(elem_type, 0);
            let count = if elem_size > 0 {
                bytes.len() / elem_size
            } else {
                0
            };
            (format!("{type_name}[]"), Some(count as u64), None)
        } else {
            ("Object".to_string(), None, None)
        }
    }

    fn static_value_to_field_value(&self, value: &StaticValue) -> FieldValue {
        match value {
            StaticValue::ObjectRef(id) => {
                if *id == 0 {
                    FieldValue::Null
                } else {
                    let (class_name, entry_count, inline_value) = self.enrich_object_ref_parts(*id);
                    FieldValue::ObjectRef {
                        id: *id,
                        class_name,
                        entry_count,
                        inline_value,
                    }
                }
            }
            StaticValue::Bool(v) => FieldValue::Bool(*v),
            StaticValue::Char(v) => FieldValue::Char(*v),
            StaticValue::Float(v) => FieldValue::Float(*v),
            StaticValue::Double(v) => FieldValue::Double(*v),
            StaticValue::Byte(v) => FieldValue::Byte(*v),
            StaticValue::Short(v) => FieldValue::Short(*v),
            StaticValue::Int(v) => FieldValue::Int(*v),
            StaticValue::Long(v) => FieldValue::Long(*v),
        }
    }

    fn resolve_name(&self, name_string_id: u64) -> String {
        self.hfile
            .index
            .strings
            .get(&name_string_id)
            .map(|sref| self.hfile.resolve_string(sref))
            .unwrap_or_else(|| format!("<unknown:{}>", name_string_id))
    }

    /// Returns cached metadata for a thread, with fallback defaults.
    fn thread_meta(&self, serial: u32) -> ThreadMetadata {
        self.thread_cache
            .get(&serial)
            .map(|m| ThreadMetadata {
                name: m.name.clone(),
                state: m.state,
            })
            .unwrap_or_else(|| ThreadMetadata {
                name: format!("Thread-{}", serial),
                state: ThreadState::Unknown,
            })
    }

    /// Decodes an object's fields from mmap (no caching).
    ///
    /// This is the full decode + enrichment pass extracted
    /// from the previous `expand_object` body.
    fn decode_object_fields(&self, object_id: u64) -> Option<Vec<FieldInfo>> {
        let raw = Self::read_instance(&self.hfile, object_id)?;
        let mut fields = crate::resolver::decode_fields(
            &raw,
            &self.hfile.index,
            self.hfile.header.id_size,
            self.hfile.records_bytes(),
        );
        for field in &mut fields {
            if let crate::engine::FieldValue::ObjectRef {
                id,
                class_name,
                entry_count,
                inline_value,
            } = &mut field.value
            {
                let (resolved_class_name, resolved_entry_count, resolved_inline_value) =
                    self.enrich_object_ref_parts(*id);
                *class_name = resolved_class_name;
                *entry_count = resolved_entry_count;
                *inline_value = resolved_inline_value;
            }
        }
        Some(fields)
    }

    /// Drains pending walker messages for a collection
    /// and applies checkpoints to the skip-index.
    ///
    /// Lock ordering: `walkers` first, then
    /// `skip_indexes`. Never reverse.
    fn drain_walker(&self, collection_id: u64) {
        // Phase 1: drain messages from walker handle
        let (messages, should_remove) = {
            let walkers = self.walkers.lock().unwrap_or_else(|e| e.into_inner());
            let Some(handle) = walkers.get(&collection_id) else {
                return;
            };
            let (msgs, disconnected) = handle.try_drain();
            let has_complete = msgs
                .iter()
                .any(|m| matches!(m, crate::pagination::WalkMessage::Complete));
            let remove = has_complete || disconnected;
            (msgs, remove)
        };

        if messages.is_empty() && !should_remove {
            return;
        }

        // Remove handle if walker completed or
        // disconnected
        if should_remove {
            let mut walkers = self.walkers.lock().unwrap_or_else(|e| e.into_inner());
            let removed = walkers.remove(&collection_id);
            // Log unexpected termination (disconnect
            // without Complete)
            let has_complete = messages
                .iter()
                .any(|m| matches!(m, crate::pagination::WalkMessage::Complete));
            if !has_complete {
                if let Some(mut handle) = removed {
                    eprintln!(
                        "Walker for collection \
                         0x{:X} terminated \
                         unexpectedly",
                        collection_id,
                    );
                    if let Some(jh) = handle.join_handle.take() {
                        let _ = jh.join();
                    }
                }
            } else if let Some(mut handle) = removed
                && let Some(jh) = handle.join_handle.take()
            {
                let _ = jh.join();
            }
        }

        // Phase 2: apply checkpoints to skip-index
        // Walker checkpoints may be dropped by record()
        // if an on-demand walk already extended the
        // skip-index further. This is expected.
        let mut guard = self.skip_indexes.lock().unwrap_or_else(|e| e.into_inner());
        for msg in &messages {
            match msg {
                crate::pagination::WalkMessage::Batch { checkpoints } => {
                    let si = guard.entry(collection_id).or_insert_with(|| {
                        crate::pagination::SkipIndex::new(crate::pagination::SKIP_INTERVAL)
                    });
                    for (idx, cp) in checkpoints {
                        si.record(*idx, cp.clone());
                    }
                }
                crate::pagination::WalkMessage::Complete => {
                    if let Some(si) = guard.get_mut(&collection_id) {
                        si.mark_complete();
                    }
                    dbg_log!("walker completed for 0x{:X}", collection_id,);
                }
            }
        }
    }
}

impl Engine {
    /// Spawns a background walker for the given
    /// collection. No-op if already walking, already
    /// complete, or walker cap reached.
    pub fn spawn_walker(&self, collection_id: u64) {
        use crate::pagination::{MAX_WALKERS, walk_collection_background};

        let mut walkers = self.walkers.lock().unwrap_or_else(|e| e.into_inner());

        // Dedup: already has a walker
        if walkers.contains_key(&collection_id) {
            return;
        }

        // Already complete: skip re-walk
        {
            let si_guard = self.skip_indexes.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(si) = si_guard.get(&collection_id)
                && si.is_complete()
            {
                return;
            }
        }

        // Cap reached
        if walkers.len() >= MAX_WALKERS {
            return;
        }

        let (tx, rx) = std::sync::mpsc::channel();
        let progress = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let hfile = Arc::clone(&self.hfile);
        let p = Arc::clone(&progress);
        let c = Arc::clone(&cancel);

        let jh = std::thread::spawn(move || {
            walk_collection_background(hfile, collection_id, tx, p, c);
        });

        dbg_log!("walker spawned for collection 0x{:X}", collection_id,);

        walkers.insert(
            collection_id,
            crate::pagination::WalkerHandle::new(rx, jh, progress, cancel),
        );
    }

    /// Cancels an active walker, joining its thread.
    pub fn cancel_walker(&self, collection_id: u64) {
        let mut walkers = self.walkers.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(mut handle) = walkers.remove(&collection_id) {
            handle.cancel();
            if let Some(jh) = handle.join_handle.take() {
                let _ = jh.join();
            }
        }
    }

    /// Returns the walker progress for the given
    /// collection, or `None` if no walker is active.
    pub fn walker_progress(&self, collection_id: u64) -> Option<usize> {
        let walkers = self.walkers.lock().unwrap_or_else(|e| e.into_inner());
        walkers.get(&collection_id).map(|h| h.progress())
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        if let Ok(mut walkers) = self.walkers.lock() {
            for (_, mut handle) in walkers.drain() {
                handle.cancel();
                if let Some(jh) = handle.join_handle.take() {
                    let _ = jh.join();
                }
            }
        }
    }
}

impl NavigationEngine for Engine {
    fn warnings(&self) -> &[String] {
        &self.hfile.index_warnings
    }

    fn list_threads(&self) -> Vec<ThreadInfo> {
        let mut threads: Vec<ThreadInfo> = self
            .hfile
            .index
            .threads
            .values()
            .map(|t| {
                let meta = self.thread_meta(t.thread_serial);
                ThreadInfo {
                    thread_serial: t.thread_serial,
                    name: meta.name,
                    state: meta.state,
                }
            })
            .collect();
        threads.sort_by_key(|t| t.thread_serial);
        threads
    }

    fn select_thread(&self, thread_serial: u32) -> Option<ThreadInfo> {
        self.hfile.index.threads.get(&thread_serial).map(|t| {
            let meta = self.thread_meta(t.thread_serial);
            ThreadInfo {
                thread_serial: t.thread_serial,
                name: meta.name,
                state: meta.state,
            }
        })
    }

    fn get_stack_frames(&self, thread_serial: u32) -> Vec<FrameInfo> {
        let Some(thread) = self.hfile.index.threads.get(&thread_serial) else {
            return vec![];
        };
        let Some(trace) = self
            .hfile
            .index
            .stack_traces
            .get(&thread.stack_trace_serial)
        else {
            return vec![];
        };
        trace
            .frame_ids
            .iter()
            .filter_map(|&fid| self.hfile.index.stack_frames.get(&fid))
            .map(|sf| {
                let method_name = self.resolve_name(sf.method_name_string_id);
                let class_name = self
                    .hfile
                    .index
                    .classes
                    .get(&sf.class_serial)
                    .map(|c| {
                        let raw = self.resolve_name(c.class_name_string_id);
                        jvm_to_java(&raw)
                    })
                    .unwrap_or_else(|| format!("<class:{}>", sf.class_serial));
                let source_file = self.resolve_name(sf.source_file_string_id);
                let source_file = if source_file.starts_with("<unknown:") {
                    String::new()
                } else {
                    source_file
                };
                let has_variables = self.hfile.index.java_frame_roots.contains_key(&sf.frame_id);
                FrameInfo {
                    frame_id: sf.frame_id,
                    method_name,
                    class_name,
                    source_file,
                    line: LineNumber::from_raw(sf.line_number),
                    has_variables,
                }
            })
            .collect()
    }

    fn get_local_variables(&self, frame_id: u64) -> Vec<VariableInfo> {
        match self.hfile.index.java_frame_roots.get(&frame_id) {
            None => vec![],
            Some(ids) => ids
                .iter()
                .enumerate()
                .map(|(idx, &object_id)| VariableInfo {
                    index: idx,
                    value: if object_id == 0 {
                        VariableValue::Null
                    } else {
                        let (class_name, entry_count) = if let Some(raw) =
                            Self::read_instance(&self.hfile, object_id)
                        {
                            let cn = self
                                .hfile
                                .index
                                .class_names_by_id
                                .get(&raw.class_object_id)
                                .cloned()
                                .unwrap_or_else(|| "Object".to_string());
                            let ec = collection_entry_count(
                                &raw,
                                &self.hfile.index,
                                self.hfile.header.id_size,
                                self.hfile.records_bytes(),
                            );
                            (cn, ec)
                        } else if let Some(meta) = self.hfile.find_object_array_meta(object_id) {
                            ("Object[]".to_string(), Some(meta.num_elements as u64))
                        } else if let Some((etype, bytes)) = self.hfile.find_prim_array(object_id) {
                            let type_name = prim_array_type_name(etype).to_string();
                            let esz = field_byte_size(etype, self.hfile.header.id_size);
                            let cnt = if esz > 0 { bytes.len() / esz } else { 0 };
                            (format!("{type_name}[]"), Some(cnt as u64))
                        } else {
                            ("Object".to_string(), None)
                        };
                        VariableValue::ObjectRef {
                            id: object_id,
                            class_name,
                            entry_count,
                        }
                    },
                })
                .collect(),
        }
    }

    fn expand_object(&self, object_id: u64) -> Option<Vec<FieldInfo>> {
        // Cache hit: return clone, entry promoted to MRU
        if let Some(fields) = self.object_cache.get(object_id) {
            return Some(fields);
        }
        // Cache miss: decode from mmap
        let fields = self.decode_object_fields(object_id)?;
        // Re-check after decode — another thread may have inserted
        // while we were decoding (concurrent expand_object calls).
        if let Some(fields) = self.object_cache.get(object_id) {
            return Some(fields);
        }
        // Insert into cache, track memory
        let mem = self.object_cache.insert(object_id, fields.clone());
        self.memory_counter.add(mem);
        // Evict LRU entries: trigger at 80%, target 60%
        while self.memory_counter.usage_ratio() >= EVICTION_TRIGGER {
            if let Some(freed) = self.object_cache.evict_lru() {
                let current = self.memory_counter.current();
                self.memory_counter.subtract(freed.min(current));
                // TODO(11.5): evict skip_indexes on LRU
                if self.memory_counter.usage_ratio() < EVICTION_TARGET {
                    break;
                }
            } else {
                break; // cache empty
            }
        }
        Some(fields)
    }

    fn class_of_object(&self, object_id: u64) -> Option<u64> {
        let Some(raw) = Self::read_instance(&self.hfile, object_id) else {
            dbg_log!("class_of_object(0x{:X}) -> <none>", object_id);
            return None;
        };
        dbg_log!(
            "class_of_object(0x{:X}) -> class=0x{:X}",
            object_id,
            raw.class_object_id
        );
        Some(raw.class_object_id)
    }

    fn get_static_fields(&self, class_object_id: u64) -> Vec<FieldInfo> {
        let Some(class_dump) = self.hfile.index.class_dumps.get(&class_object_id) else {
            dbg_log!(
                "get_static_fields(class=0x{:X}) -> class_dump missing",
                class_object_id
            );
            return Vec::new();
        };

        dbg_log!(
            "get_static_fields(class=0x{:X}) -> raw_static_fields={}",
            class_object_id,
            class_dump.static_fields.len()
        );

        let fields: Vec<FieldInfo> = class_dump
            .static_fields
            .iter()
            .map(|f| FieldInfo {
                name: self.resolve_name(f.name_string_id),
                value: self.static_value_to_field_value(&f.value),
            })
            .collect();
        dbg_log!(
            "get_static_fields(class=0x{:X}) -> resolved_static_fields={}",
            class_object_id,
            fields.len()
        );
        fields
    }

    fn get_page(&self, collection_id: u64, offset: usize, limit: usize) -> Option<CollectionPage> {
        // Phase 1: drain walker (acquires+releases
        // both walkers AND skip_indexes locks).
        // Intentional two-phase approach for deadlock
        // prevention. TOCTOU between phases is safe —
        // both skip-index operations are monotonically
        // additive and record() is idempotent.
        self.drain_walker(collection_id);

        // Phase 2: existing pagination (acquires
        // skip_indexes lock independently)
        let mut guard = self.skip_indexes.lock().unwrap_or_else(|e| e.into_inner());
        let si = if offset > 0 {
            Some(
                guard
                    .entry(collection_id)
                    .or_insert_with(|| crate::pagination::SkipIndex::new(100)),
            )
        } else {
            guard.get_mut(&collection_id)
        };
        crate::pagination::get_page(&self.hfile, collection_id, offset, limit, si)
    }

    fn resolve_string(&self, object_id: u64) -> Option<String> {
        let raw = Self::read_instance(&self.hfile, object_id)?;
        let fields = crate::resolver::decode_fields(
            &raw,
            &self.hfile.index,
            self.hfile.header.id_size,
            self.hfile.records_bytes(),
        );
        let value_id = fields.iter().find_map(|f| {
            if f.name == "value"
                && let crate::engine::FieldValue::ObjectRef { id, .. } = f.value
            {
                return Some(id);
            }
            None
        })?;
        let (elem_type, bytes) = self.hfile.find_prim_array(value_id)?;
        Some(decode_prim_array_as_string(elem_type, &bytes))
    }

    fn memory_used(&self) -> usize {
        self.memory_counter.current()
    }

    fn memory_budget(&self) -> u64 {
        self.memory_counter.budget()
    }

    fn indexing_ratio(&self) -> f64 {
        if self.hfile.records_attempted == 0 {
            return 100.0;
        }
        self.hfile.records_indexed as f64 / self.hfile.records_attempted as f64 * 100.0
    }

    fn is_fully_indexed(&self) -> bool {
        // A file truncated mid-record breaks out of the scan loop before
        // incrementing records_attempted, so the ratio stays 100%.
        // index_warnings catches that case (payload-exceeds-file warnings).
        self.hfile.index_warnings.is_empty()
            && (self.hfile.records_attempted == 0
                || self.hfile.records_indexed >= self.hfile.records_attempted)
    }

    fn skeleton_bytes(&self) -> usize {
        use hprof_api::MemorySize;
        self.hfile.index.memory_size()
    }

    fn drain_walkers(&self) {
        let ids: Vec<u64> = {
            let walkers = self.walkers.lock().unwrap_or_else(|e| e.into_inner());
            walkers.keys().copied().collect()
        };
        for id in ids {
            self.drain_walker(id);
        }
    }

    fn spawn_walker(&self, collection_id: u64) {
        Engine::spawn_walker(self, collection_id);
    }

    fn cancel_walker(&self, collection_id: u64) {
        Engine::cancel_walker(self, collection_id);
    }

    fn walker_progress(&self, collection_id: u64) -> Option<usize> {
        Engine::walker_progress(self, collection_id)
    }
}

#[cfg(test)]
impl Engine {
    pub(crate) fn skip_index_count(&self) -> usize {
        self.skip_indexes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }
}

#[cfg(test)]
mod tests;
