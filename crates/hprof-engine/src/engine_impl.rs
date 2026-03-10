//! Concrete `Engine` implementation of [`NavigationEngine`].
//!
//! Wraps an [`hprof_parser::HprofFile`] and exposes the high-level
//! navigation API. Parser internals are fully encapsulated — callers only
//! depend on `hprof-engine`.

use rustc_hash::FxHashMap;
use std::path::Path;
use std::sync::Arc;

use hprof_api::{NullProgressObserver, ParseProgressObserver, ProgressNotifier};
use rayon::prelude::*;

use hprof_parser::{HprofFile, PreciseIndex, RawInstance};

use hprof_parser::jvm_to_java;

use crate::{
    EngineConfig, HprofError,
    cache::lru::{EVICTION_TARGET, EVICTION_TRIGGER},
    engine::{
        CollectionPage, FieldInfo, FrameInfo, LineNumber, NavigationEngine, ThreadInfo,
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
/// fields for an `Int` or `Long` field named `size`, `elementCount`, or
/// `count`.
fn collection_entry_count(
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
    // Decode only the immediate class fields (not super) to find size/elementCount/count.
    let class_info = index.class_dumps.get(&raw.class_object_id)?;
    let mut cursor = std::io::Cursor::new(raw.data.as_slice());
    // Skip super-class fields first (we need to advance the cursor past them).
    // Walk super chain to compute prefix byte count.
    let mut super_ids: Vec<u64> = Vec::new();
    let mut cur_id = class_info.super_class_id;
    let mut visited = std::collections::HashSet::new();
    while cur_id != 0 {
        if !visited.insert(cur_id) {
            break;
        }
        if let Some(info) = index.class_dumps.get(&cur_id) {
            super_ids.push(cur_id);
            cur_id = info.super_class_id;
        } else {
            break;
        }
    }
    // Skip bytes for all super class fields (super-first order in the data).
    for &sid in super_ids.iter().rev() {
        if let Some(info) = index.class_dumps.get(&sid) {
            for field in &info.instance_fields {
                let bytes = field_byte_size(field.field_type, id_size);
                let pos = cursor.position() as usize + bytes;
                cursor.set_position(pos as u64);
            }
        }
    }
    // Now parse immediate class fields looking for size/elementCount/count.
    for field in &class_info.instance_fields {
        let resolved_name;
        let name = match index.strings.get(&field.name_string_id) {
            Some(sref) => {
                resolved_name = sref.resolve(records_bytes);
                resolved_name.as_str()
            }
            None => "",
        };
        let is_candidate = matches!(name, "size" | "elementCount" | "count");
        match field.field_type {
            10 => {
                use byteorder::{BigEndian, ReadBytesExt};
                if let Ok(v) = cursor.read_i32::<BigEndian>() {
                    if is_candidate && v >= 0 {
                        return Some(v as u64);
                    }
                } else {
                    break;
                }
            }
            11 => {
                use byteorder::{BigEndian, ReadBytesExt};
                if let Ok(v) = cursor.read_i64::<BigEndian>() {
                    if is_candidate && v >= 0 {
                        return Some(v as u64);
                    }
                } else {
                    break;
                }
            }
            _ => {
                let bytes = field_byte_size(field.field_type, id_size);
                let pos = cursor.position() as usize + bytes;
                cursor.set_position(pos as u64);
            }
        }
    }
    None
}

/// Returns the byte size of a field given its type code.
fn field_byte_size(type_code: u8, id_size: u32) -> usize {
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
fn prim_array_type_name(elem_type: u8) -> &'static str {
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
        let hfile = Arc::new(HprofFile::from_path(path)?);
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
        let hfile = Arc::new(HprofFile::from_path_with_progress(path, observer)?);
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
        if let Some(&off) = hfile.index.instance_offsets.get(&obj_id)
            && let Some(raw) = hfile.read_instance_at_offset(off)
        {
            return Some(raw);
        }
        hfile.find_instance(obj_id)
    }

    /// Reads a primitive array by offset if available, falling back to
    /// `find_prim_array`.
    fn read_prim_array(hfile: &HprofFile, arr_id: u64) -> Option<(u8, Vec<u8>)> {
        if let Some(&off) = hfile.index.instance_offsets.get(&arr_id)
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
    fn decode_object_fields(
        &self,
        object_id: u64,
    ) -> Option<Vec<FieldInfo>> {
        let raw = Self::read_instance(
            &self.hfile, object_id,
        )?;
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
                let child_id = *id;
                if let Some(child_raw) =
                    Self::read_instance(&self.hfile, child_id)
                {
                    let name = self
                        .hfile
                        .index
                        .class_names_by_id
                        .get(&child_raw.class_object_id)
                        .cloned()
                        .unwrap_or_else(|| {
                            "Object".to_string()
                        });
                    *inline_value = resolve_inline_value(
                        &name, &self.hfile, child_id,
                    );
                    *entry_count = collection_entry_count(
                        &child_raw,
                        &self.hfile.index,
                        self.hfile.header.id_size,
                        self.hfile.records_bytes(),
                    );
                    *class_name = name;
                } else if let Some((_class_id, elems)) =
                    self.hfile.find_object_array(child_id)
                {
                    *class_name = "Object[]".to_string();
                    *entry_count =
                        Some(elems.len() as u64);
                } else if let Some((elem_type, bytes)) =
                    self.hfile.find_prim_array(child_id)
                {
                    let type_name =
                        prim_array_type_name(elem_type);
                    let elem_size =
                        field_byte_size(elem_type, 0);
                    let count = if elem_size > 0 {
                        bytes.len() / elem_size
                    } else {
                        0
                    };
                    *class_name =
                        format!("{type_name}[]");
                    *entry_count = Some(count as u64);
                } else {
                    *class_name = "Object".to_string();
                }
            }
        }
        Some(fields)
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
                        let class_name = self
                            .hfile
                            .find_instance(object_id)
                            .and_then(|raw| {
                                self.hfile
                                    .index
                                    .class_names_by_id
                                    .get(&raw.class_object_id)
                                    .cloned()
                            })
                            .unwrap_or_else(|| "Object".to_string());
                        VariableValue::ObjectRef {
                            id: object_id,
                            class_name,
                        }
                    },
                })
                .collect(),
        }
    }

    fn expand_object(
        &self,
        object_id: u64,
    ) -> Option<Vec<FieldInfo>> {
        // Cache hit: return clone, entry promoted to MRU
        if let Some(fields) =
            self.object_cache.get(object_id)
        {
            return Some(fields);
        }
        // Cache miss: decode from mmap
        let fields =
            self.decode_object_fields(object_id)?;
        // Insert into cache, track memory
        let mem = self
            .object_cache
            .insert(object_id, fields.clone());
        self.memory_counter.add(mem);
        // Evict LRU entries: trigger at 80%, target 60%
        while self.memory_counter.usage_ratio()
            >= EVICTION_TRIGGER
        {
            if let Some(freed) =
                self.object_cache.evict_lru()
            {
                let current =
                    self.memory_counter.current();
                self.memory_counter
                    .subtract(freed.min(current));
                if self.memory_counter.usage_ratio()
                    < EVICTION_TARGET
                {
                    break;
                }
            } else {
                break; // cache empty
            }
        }
        Some(fields)
    }

    fn get_page(&self, collection_id: u64, offset: usize, limit: usize) -> Option<CollectionPage> {
        crate::pagination::get_page(&self.hfile, collection_id, offset, limit)
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
}

#[cfg(test)]
mod tests {
    use std::io::Write as IoWrite;

    use super::*;

    fn minimal_hprof_bytes() -> Vec<u8> {
        let mut bytes = b"JAVA PROFILE 1.0.2\0".to_vec();
        bytes.extend_from_slice(&8u32.to_be_bytes());
        bytes.extend_from_slice(&0u64.to_be_bytes());
        bytes
    }

    #[test]
    fn memory_used_positive_after_from_file() {
        let bytes = minimal_hprof_bytes();
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();

        let config = EngineConfig::default();
        let engine = Engine::from_file(tmp.path(), &config).unwrap();
        let used = engine.memory_used();
        assert!(used > 0, "memory_used must be > 0 after construction");
        assert!(
            used < bytes.len() * 1000,
            "memory_used ({used}) must be < file_size * 1000 ({})",
            bytes.len() * 1000
        );
    }

    #[test]
    fn memory_used_equals_precise_index_static_size_for_empty_file() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&minimal_hprof_bytes()).unwrap();
        tmp.flush().unwrap();

        let config = EngineConfig::default();
        let engine = Engine::from_file(tmp.path(), &config).unwrap();
        // Empty index: all maps have 0 capacity → memory_size() = size_of::<PreciseIndex>()
        // Empty thread_cache: cache_size=0, cache_overhead=0
        let expected = std::mem::size_of::<hprof_parser::PreciseIndex>();
        assert_eq!(
            engine.memory_used(),
            expected,
            "empty file: memory_used must equal PreciseIndex static size"
        );
    }

    #[test]
    fn warnings_returns_empty_slice_for_clean_file() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&minimal_hprof_bytes()).unwrap();
        tmp.flush().unwrap();

        let config = EngineConfig::default();
        let engine = Engine::from_file(tmp.path(), &config).unwrap();
        assert!(engine.warnings().is_empty());
    }

    #[test]
    fn from_file_with_progress_on_valid_file_calls_observer() {
        struct CountingObserver {
            call_count: usize,
        }
        impl ParseProgressObserver for CountingObserver {
            fn on_bytes_scanned(&mut self, _pos: u64) {
                self.call_count += 1;
            }
            fn on_segment_completed(&mut self, _d: usize, _t: usize) {}
            fn on_names_resolved(&mut self, _d: usize, _t: usize) {}
        }

        let mut bytes = b"JAVA PROFILE 1.0.2\0".to_vec();
        bytes.extend_from_slice(&8u32.to_be_bytes());
        bytes.extend_from_slice(&0u64.to_be_bytes());
        bytes.push(0x01);
        bytes.extend_from_slice(&0u32.to_be_bytes());
        let id_bytes = 1u64.to_be_bytes();
        bytes.extend_from_slice(&(id_bytes.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&id_bytes);

        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();

        let config = EngineConfig::default();
        let mut obs = CountingObserver { call_count: 0 };
        let result = Engine::from_file_with_progress(tmp.path(), &config, &mut obs);
        assert!(result.is_ok());
        assert!(obs.call_count >= 1, "observer must be called at least once");
    }

    #[test]
    fn from_file_with_progress_reports_monotonic_name_resolution() {
        #[derive(Default)]
        struct CapturingObserver {
            name_events: Vec<(usize, usize)>,
        }

        impl ParseProgressObserver for CapturingObserver {
            fn on_bytes_scanned(&mut self, _pos: u64) {}
            fn on_segment_completed(&mut self, _d: usize, _t: usize) {}
            fn on_names_resolved(&mut self, done: usize, total: usize) {
                self.name_events.push((done, total));
            }
        }

        let bytes = hprof_parser::HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_string(10, "main")
            .add_string(11, "worker-1")
            .add_string(12, "worker-2")
            .add_thread(1, 100, 0, 10, 0, 0)
            .add_thread(2, 101, 0, 11, 0, 0)
            .add_thread(3, 102, 0, 12, 0, 0)
            .build();

        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();

        let config = EngineConfig::default();
        let mut obs = CapturingObserver::default();
        let result = Engine::from_file_with_progress(tmp.path(), &config, &mut obs);

        assert!(result.is_ok());
        assert!(
            !obs.name_events.is_empty(),
            "observer must receive name resolution events"
        );

        let expected_total = obs.name_events[0].1;
        assert!(expected_total > 0, "name resolution total must be > 0");

        let mut last_done = 0usize;
        for (done, total) in &obs.name_events {
            assert_eq!(*total, expected_total, "total must stay constant");
            assert!(*done > last_done, "done must be strictly increasing");
            last_done = *done;
        }

        assert_eq!(last_done, expected_total, "final done must equal total");
    }

    #[test]
    fn from_file_on_missing_path_returns_mmap_failed() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let missing = tmp.path().to_path_buf();
        drop(tmp);

        let config = EngineConfig::default();
        let result = Engine::from_file(&missing, &config);
        assert!(matches!(result, Err(HprofError::MmapFailed(_))));
    }

    #[test]
    fn from_file_on_valid_hprof_returns_ok() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&minimal_hprof_bytes()).unwrap();
        tmp.flush().unwrap();

        let config = EngineConfig::default();
        let result = Engine::from_file(tmp.path(), &config);
        assert!(result.is_ok());
    }

    #[test]
    fn list_threads_on_file_with_no_threads_returns_empty() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&minimal_hprof_bytes()).unwrap();
        tmp.flush().unwrap();

        let config = EngineConfig::default();
        let engine = Engine::from_file(tmp.path(), &config).unwrap();
        assert!(engine.list_threads().is_empty());
    }

    #[test]
    fn select_thread_returns_none_for_missing_serial() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&minimal_hprof_bytes()).unwrap();
        tmp.flush().unwrap();

        let config = EngineConfig::default();
        let engine = Engine::from_file(tmp.path(), &config).unwrap();
        assert!(engine.select_thread(999).is_none());
    }

    mod stack_frame_tests {
        use std::io::Write as IoWrite;

        use hprof_parser::HprofTestBuilder;

        use super::*;
        use crate::engine::{LineNumber, VariableValue};

        fn engine_from_bytes(bytes: &[u8]) -> Engine {
            let mut tmp = tempfile::NamedTempFile::new().unwrap();
            tmp.write_all(bytes).unwrap();
            tmp.flush().unwrap();
            let config = EngineConfig::default();
            Engine::from_file(tmp.path(), &config).unwrap()
        }

        #[test]
        fn memory_used_with_populated_fixture() {
            let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
                .add_string(1, "run")
                .add_string(2, "()")
                .add_string(3, "Thread.java")
                .add_string(4, "java/lang/Thread")
                .add_class(1, 100, 0, 4)
                .add_stack_frame(10, 1, 2, 3, 1, 42)
                .add_stack_trace(1, 1, &[10])
                .add_thread(1, 200, 1, 1, 0, 0)
                .build();
            let engine = engine_from_bytes(&bytes);
            let used = engine.memory_used();
            assert!(used > 0, "memory_used ({used}) must be positive");
            assert!(
                used < bytes.len() * 1000,
                "memory_used ({used}) must be < file_size * 1000 ({})",
                bytes.len() * 1000
            );
        }

        #[test]
        fn get_stack_frames_returns_one_frame_for_thread_with_one_frame() {
            let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
                .add_string(1, "run")
                .add_string(2, "()")
                .add_string(3, "Thread.java")
                .add_string(4, "java/lang/Thread")
                .add_class(1, 100, 0, 4)
                .add_stack_frame(50, 1, 2, 3, 1, 10)
                .add_stack_trace(10, 1, &[50])
                .add_thread(1, 200, 10, 1, 0, 0)
                .build();
            let engine = engine_from_bytes(&bytes);
            let frames = engine.get_stack_frames(1);
            assert_eq!(frames.len(), 1);
        }

        #[test]
        fn get_stack_frames_method_name_resolves_from_string_id() {
            let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
                .add_string(1, "myMethod")
                .add_string(2, "()")
                .add_string(3, "Foo.java")
                .add_string(4, "com/example/Foo")
                .add_class(1, 100, 0, 4)
                .add_stack_frame(50, 1, 2, 3, 1, 5)
                .add_stack_trace(10, 1, &[50])
                .add_thread(1, 200, 10, 1, 0, 0)
                .build();
            let engine = engine_from_bytes(&bytes);
            let frames = engine.get_stack_frames(1);
            assert_eq!(frames[0].method_name, "myMethod");
        }

        #[test]
        fn get_stack_frames_class_name_is_human_readable() {
            let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
                .add_string(1, "get")
                .add_string(2, "()")
                .add_string(3, "HashMap.java")
                .add_string(4, "java/util/HashMap")
                .add_class(1, 100, 0, 4)
                .add_stack_frame(50, 1, 2, 3, 1, 1)
                .add_stack_trace(10, 1, &[50])
                .add_thread(1, 200, 10, 1, 0, 0)
                .build();
            let engine = engine_from_bytes(&bytes);
            let frames = engine.get_stack_frames(1);
            assert_eq!(frames[0].class_name, "HashMap");
        }

        #[test]
        fn get_stack_frames_line_number_42_gives_line_variant() {
            let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
                .add_string(1, "foo")
                .add_string(2, "()")
                .add_string(3, "Foo.java")
                .add_string(4, "Foo")
                .add_class(1, 100, 0, 4)
                .add_stack_frame(50, 1, 2, 3, 1, 42)
                .add_stack_trace(10, 1, &[50])
                .add_thread(1, 200, 10, 1, 0, 0)
                .build();
            let engine = engine_from_bytes(&bytes);
            let frames = engine.get_stack_frames(1);
            assert_eq!(frames[0].line, LineNumber::Line(42));
        }

        #[test]
        fn get_stack_frames_line_number_0_gives_no_info() {
            let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
                .add_string(1, "foo")
                .add_string(2, "()")
                .add_string(3, "Foo.java")
                .add_string(4, "Foo")
                .add_class(1, 100, 0, 4)
                .add_stack_frame(50, 1, 2, 3, 1, 0)
                .add_stack_trace(10, 1, &[50])
                .add_thread(1, 200, 10, 1, 0, 0)
                .build();
            let engine = engine_from_bytes(&bytes);
            let frames = engine.get_stack_frames(1);
            assert_eq!(frames[0].line, LineNumber::NoInfo);
        }

        #[test]
        fn get_stack_frames_line_number_minus_one_gives_unknown() {
            let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
                .add_string(1, "foo")
                .add_string(2, "()")
                .add_string(3, "Foo.java")
                .add_string(4, "Foo")
                .add_class(1, 100, 0, 4)
                .add_stack_frame(50, 1, 2, 3, 1, -1)
                .add_stack_trace(10, 1, &[50])
                .add_thread(1, 200, 10, 1, 0, 0)
                .build();
            let engine = engine_from_bytes(&bytes);
            let frames = engine.get_stack_frames(1);
            assert_eq!(frames[0].line, LineNumber::Unknown);
        }

        #[test]
        fn get_stack_frames_unknown_thread_serial_returns_empty() {
            let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8).build();
            let engine = engine_from_bytes(&bytes);
            assert!(engine.get_stack_frames(999).is_empty());
        }

        #[test]
        fn get_local_variables_non_null_root_returns_object_ref() {
            let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
                .add_string(1, "foo")
                .add_string(2, "()")
                .add_string(3, "")
                .add_string(4, "Foo")
                .add_class(1, 100, 0, 4)
                .add_stack_frame(50, 1, 2, 3, 1, 1)
                .add_stack_trace(10, 1, &[50])
                .add_thread(1, 200, 10, 1, 0, 0)
                .add_java_frame_root(42, 1, 0)
                .build();
            let engine = engine_from_bytes(&bytes);
            let vars = engine.get_local_variables(50);
            assert_eq!(vars.len(), 1);
            assert_eq!(vars[0].index, 0);
            assert!(matches!(
                vars[0].value,
                VariableValue::ObjectRef { id: 42, .. }
            ));
        }

        #[test]
        fn get_local_variables_null_root_returns_null() {
            let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
                .add_string(1, "foo")
                .add_string(2, "()")
                .add_string(3, "")
                .add_string(4, "Foo")
                .add_class(1, 100, 0, 4)
                .add_stack_frame(50, 1, 2, 3, 1, 1)
                .add_stack_trace(10, 1, &[50])
                .add_thread(1, 200, 10, 1, 0, 0)
                .add_java_frame_root(0, 1, 0)
                .build();
            let engine = engine_from_bytes(&bytes);
            let vars = engine.get_local_variables(50);
            assert_eq!(vars.len(), 1);
            assert_eq!(vars[0].value, VariableValue::Null);
        }

        #[test]
        fn get_local_variables_frame_with_no_roots_returns_empty() {
            let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8).build();
            let engine = engine_from_bytes(&bytes);
            assert!(engine.get_local_variables(999).is_empty());
        }

        #[test]
        fn get_local_variables_resolves_class_name() {
            let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
                .add_string(1, "foo")
                .add_string(2, "()")
                .add_string(3, "")
                .add_string(4, "Foo")
                .add_string(5, "sun/misc/NativeReferenceQueue")
                .add_class(1, 100, 0, 4)
                .add_class(2, 200, 0, 5)
                .add_stack_frame(50, 1, 2, 3, 1, 1)
                .add_stack_trace(10, 1, &[50])
                .add_thread(1, 300, 10, 1, 0, 0)
                .add_java_frame_root(42, 1, 0)
                .add_class_dump(200, 0, 0, &[])
                .add_instance(42, 0, 200, &[])
                .build();
            let engine = engine_from_bytes(&bytes);
            let vars = engine.get_local_variables(50);
            assert_eq!(vars.len(), 1);
            assert_eq!(
                vars[0].value,
                VariableValue::ObjectRef {
                    id: 42,
                    class_name: "sun.misc.NativeReferenceQueue".to_string(),
                }
            );
        }

        #[test]
        fn get_local_variables_unknown_instance_falls_back_to_object() {
            let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
                .add_string(1, "foo")
                .add_string(2, "()")
                .add_string(3, "")
                .add_string(4, "Foo")
                .add_class(1, 100, 0, 4)
                .add_stack_frame(50, 1, 2, 3, 1, 1)
                .add_stack_trace(10, 1, &[50])
                .add_thread(1, 300, 10, 1, 0, 0)
                // Root points to object 0x999 which is not in
                // the heap
                .add_java_frame_root(0x999, 1, 0)
                .build();
            let engine = engine_from_bytes(&bytes);
            let vars = engine.get_local_variables(50);
            assert_eq!(vars.len(), 1);
            assert_eq!(
                vars[0].value,
                VariableValue::ObjectRef {
                    id: 0x999,
                    class_name: "Object".to_string(),
                }
            );
        }

        #[test]
        fn list_threads_resolves_real_name_via_root_thread_obj() {
            // Chain: ROOT_THREAD_OBJ(obj=0x100, serial=1)
            //   → INSTANCE_DUMP(0x100, class=Thread, name→0x200)
            //   → String instance(0x200, value→0x300)
            //   → char[](0x300, "main-thread")
            let char_bytes: Vec<u8> = "main-thread"
                .encode_utf16()
                .flat_map(|c| c.to_be_bytes())
                .collect();
            let num_chars = "main-thread".encode_utf16().count() as u32;

            // Thread instance data: "name" field is ObjectRef
            // pointing to string obj 0x200
            let thread_data = 0x200u64.to_be_bytes().to_vec();
            // String instance data: "value" field is ObjectRef
            // pointing to char array 0x300
            let string_data = 0x300u64.to_be_bytes().to_vec();

            let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
                .add_string(10, "name")
                .add_string(11, "value")
                .add_string(12, "java/lang/Thread")
                .add_string(13, "java/lang/String")
                .add_class(1, 500, 0, 12) // Thread class
                .add_class(2, 600, 0, 13) // String class
                // Thread CLASS_DUMP: one ObjectRef field "name"
                .add_class_dump(500, 0, 8, &[(10, 2u8)])
                // String CLASS_DUMP: one ObjectRef field "value"
                .add_class_dump(600, 0, 8, &[(11, 2u8)])
                // STACK_TRACE for synthetic thread
                .add_stack_trace(10, 1, &[])
                // ROOT_THREAD_OBJ links serial=1 to obj=0x100
                .add_root_thread_obj(0x100, 1, 10)
                // Thread instance in heap
                .add_instance(0x100, 0, 500, &thread_data)
                // String instance for name
                .add_instance(0x200, 0, 600, &string_data)
                // Backing char array
                .add_prim_array(0x300, 0, num_chars, 5, &char_bytes)
                .build();
            let engine = engine_from_bytes(&bytes);
            let threads = engine.list_threads();
            assert_eq!(threads.len(), 1);
            assert_eq!(
                threads[0].name, "main-thread",
                "must resolve real thread name from heap"
            );
        }

        #[test]
        fn list_threads_falls_back_when_instance_not_found() {
            // ROOT_THREAD_OBJ points to object 0x999 which is
            // not in the heap → fallback to Thread-{serial}
            let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
                .add_stack_trace(10, 1, &[])
                .add_root_thread_obj(0x999, 1, 10)
                .build();
            let engine = engine_from_bytes(&bytes);
            let threads = engine.list_threads();
            assert_eq!(threads.len(), 1);
            assert_eq!(
                threads[0].name, "Thread-1",
                "must fall back to Thread-{{serial}}"
            );
        }
    }

    mod decode_prim_array_tests {
        use super::decode_prim_array_as_string;

        #[test]
        fn char_array_utf16_big_endian_decodes_correctly() {
            // 'h' = 0x0068, 'i' = 0x0069 in UTF-16BE
            let bytes = vec![0x00u8, 0x68, 0x00, 0x69];
            assert_eq!(decode_prim_array_as_string(5, &bytes), "hi");
        }

        #[test]
        fn byte_array_latin1_decodes_correctly() {
            let bytes = vec![0x68u8, 0x69]; // 'h', 'i'
            assert_eq!(decode_prim_array_as_string(8, &bytes), "hi");
        }

        #[test]
        fn char_array_with_surrogate_pair_uses_replacement_char() {
            // 0xD800 is a surrogate (invalid standalone char)
            let bytes = vec![0xD8u8, 0x00, 0x00, 0x41]; // surrogate + 'A'
            let result = decode_prim_array_as_string(5, &bytes);
            assert!(result.contains('\u{FFFD}'), "must contain replacement char");
            assert!(result.contains('A'));
        }

        #[test]
        fn unknown_elem_type_returns_non_empty_placeholder() {
            let result = decode_prim_array_as_string(99, &[]);
            assert!(!result.is_empty());
            assert!(result.contains("99"));
        }
    }

    mod resolve_string_tests {
        use std::io::Write as IoWrite;

        use hprof_parser::HprofTestBuilder;

        use super::*;

        fn engine_from_bytes(bytes: &[u8]) -> Engine {
            let mut tmp = tempfile::NamedTempFile::new().unwrap();
            tmp.write_all(bytes).unwrap();
            tmp.flush().unwrap();
            let config = EngineConfig::default();
            Engine::from_file(tmp.path(), &config).unwrap()
        }

        fn make_string_with_char_array(string_id: u64, array_id: u64, content: &str) -> Vec<u8> {
            // Build String instance: class 1000, field "value" (type 2 = ObjectRef) → array_id
            // char[] encoded as UTF-16BE
            let char_bytes: Vec<u8> = content
                .encode_utf16()
                .flat_map(|c| c.to_be_bytes())
                .collect();
            let num_chars = content.encode_utf16().count() as u32;
            let array_field_data = array_id.to_be_bytes().to_vec();

            HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
                .add_string(1, "value")
                .add_string(2, "java/lang/String")
                .add_class(1, 1000, 0, 2)
                .add_class_dump(1000, 0, 8, &[(1, 2u8)]) // one ObjectRef field named "value"
                .add_instance(string_id, 0, 1000, &array_field_data)
                .add_prim_array(array_id, 0, num_chars, 5, &char_bytes)
                .build()
        }

        #[test]
        fn resolve_string_with_char_array_returns_decoded_content() {
            let bytes = make_string_with_char_array(0x100, 0x200, "hello");
            let engine = engine_from_bytes(&bytes);
            assert_eq!(engine.resolve_string(0x100), Some("hello".to_string()));
        }

        #[test]
        fn resolve_string_with_byte_array_returns_decoded_content() {
            let byte_data = b"hi".to_vec();
            let array_field_data = 0x200u64.to_be_bytes().to_vec();
            let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
                .add_string(1, "value")
                .add_string(2, "java/lang/String")
                .add_class(1, 1000, 0, 2)
                .add_class_dump(1000, 0, 8, &[(1, 2u8)])
                .add_instance(0x100, 0, 1000, &array_field_data)
                .add_prim_array(0x200, 0, 2, 8, &byte_data)
                .build();
            let engine = engine_from_bytes(&bytes);
            assert_eq!(engine.resolve_string(0x100), Some("hi".to_string()));
        }

        #[test]
        fn resolve_string_backing_array_absent_returns_none() {
            // String instance points to array 0x999 which is not in the file
            let array_field_data = 0x999u64.to_be_bytes().to_vec();
            let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
                .add_string(1, "value")
                .add_class_dump(1000, 0, 8, &[(1, 2u8)])
                .add_instance(0x100, 0, 1000, &array_field_data)
                .build();
            let engine = engine_from_bytes(&bytes);
            assert!(engine.resolve_string(0x100).is_none());
        }

        #[test]
        fn resolve_string_no_value_field_returns_none() {
            // String instance with no fields
            let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
                .add_class_dump(1000, 0, 0, &[])
                .add_instance(0x100, 0, 1000, &[])
                .build();
            let engine = engine_from_bytes(&bytes);
            assert!(engine.resolve_string(0x100).is_none());
        }
    }

    mod collection_tests {
        use hprof_parser::{ClassDumpInfo, FieldDef, HprofStringRef, PreciseIndex, RawInstance};

        use super::collection_entry_count;

        fn make_int_index(
            class_id: u64,
            super_id: u64,
            field_name: &str,
            type_code: u8,
        ) -> (PreciseIndex, u64, Vec<u8>) {
            let mut index = PreciseIndex::new();
            let buf = field_name.as_bytes().to_vec();
            index.strings.insert(
                1,
                HprofStringRef {
                    id: 1,
                    offset: 0,
                    len: field_name.len() as u32,
                },
            );
            index.class_dumps.insert(
                class_id,
                ClassDumpInfo {
                    class_object_id: class_id,
                    super_class_id: super_id,
                    instance_size: 4,
                    instance_fields: vec![FieldDef {
                        name_string_id: 1,
                        field_type: type_code,
                    }],
                },
            );
            (index, class_id, buf)
        }

        #[test]
        fn hashmap_with_size_field_returns_entry_count() {
            let (mut index, class_id, buf) = make_int_index(100, 0, "size", 10);
            index
                .class_names_by_id
                .insert(class_id, "java.util.HashMap".to_string());
            let raw = RawInstance {
                class_object_id: class_id,
                data: 524288i32.to_be_bytes().to_vec(),
            };
            assert_eq!(collection_entry_count(&raw, &index, 8, &buf), Some(524288));
        }

        #[test]
        fn plain_object_returns_none() {
            let (mut index, class_id, buf) = make_int_index(100, 0, "size", 10);
            index
                .class_names_by_id
                .insert(class_id, "com.example.Foo".to_string());
            let raw = RawInstance {
                class_object_id: class_id,
                data: 42i32.to_be_bytes().to_vec(),
            };
            assert_eq!(collection_entry_count(&raw, &index, 8, &buf), None);
        }

        #[test]
        fn unknown_class_id_returns_none() {
            let index = PreciseIndex::new();
            let raw = RawInstance {
                class_object_id: 999,
                data: 42i32.to_be_bytes().to_vec(),
            };
            assert_eq!(collection_entry_count(&raw, &index, 8, &[]), None);
        }

        #[test]
        fn arraylist_with_size_field_returns_entry_count() {
            let (mut index, class_id, buf) = make_int_index(200, 0, "size", 10);
            index
                .class_names_by_id
                .insert(class_id, "java.util.ArrayList".to_string());
            let raw = RawInstance {
                class_object_id: class_id,
                data: 7i32.to_be_bytes().to_vec(),
            };
            assert_eq!(collection_entry_count(&raw, &index, 8, &buf), Some(7));
        }

        #[test]
        fn collection_detection_is_case_insensitive() {
            let (mut index, class_id, buf) = make_int_index(300, 0, "size", 10);
            index
                .class_names_by_id
                .insert(class_id, "java.util.hashmap".to_string());
            let raw = RawInstance {
                class_object_id: class_id,
                data: 3i32.to_be_bytes().to_vec(),
            };
            assert_eq!(collection_entry_count(&raw, &index, 8, &buf), Some(3));
        }

        #[test]
        fn negative_size_field_returns_none() {
            let (mut index, class_id, buf) = make_int_index(400, 0, "size", 10);
            index
                .class_names_by_id
                .insert(class_id, "java.util.HashMap".to_string());
            let raw = RawInstance {
                class_object_id: class_id,
                data: (-1i32).to_be_bytes().to_vec(),
            };
            assert_eq!(collection_entry_count(&raw, &index, 8, &buf), None);
        }
    }

    mod expand_object_tests {
        use std::io::Write as IoWrite;

        use hprof_parser::HprofTestBuilder;

        use super::*;
        use crate::engine::FieldValue;

        fn engine_from_bytes(bytes: &[u8]) -> Engine {
            let mut tmp = tempfile::NamedTempFile::new().unwrap();
            tmp.write_all(bytes).unwrap();
            tmp.flush().unwrap();
            let config = EngineConfig::default();
            Engine::from_file(tmp.path(), &config).unwrap()
        }

        #[test]
        fn expand_object_single_int_field_returns_correct_field_info() {
            let field_data = 7i32.to_be_bytes().to_vec();
            let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
                .add_string(1, "x")
                .add_class_dump(100, 0, 4, &[(1, 10u8)])
                .add_instance(0xABC, 0, 100, &field_data)
                .build();
            let engine = engine_from_bytes(&bytes);
            let fields = engine.expand_object(0xABC).expect("must find instance");
            assert_eq!(fields.len(), 1);
            assert_eq!(fields[0].name, "x");
            assert_eq!(fields[0].value, FieldValue::Int(7));
        }

        #[test]
        fn expand_object_super_sub_class_returns_fields_in_leaf_first_order() {
            // super class 50: field "a" (int)
            // sub class 100: field "b" (int), super=50
            // HotSpot writes leaf fields first in INSTANCE_DUMP
            let mut data = Vec::new();
            data.extend_from_slice(&2i32.to_be_bytes()); // b=2 (sub)
            data.extend_from_slice(&1i32.to_be_bytes()); // a=1 (super)

            let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
                .add_string(10, "a")
                .add_string(11, "b")
                .add_class_dump(50, 0, 4, &[(10, 10u8)])
                .add_class_dump(100, 50, 8, &[(11, 10u8)])
                .add_instance(0xABC, 0, 100, &data)
                .build();
            let engine = engine_from_bytes(&bytes);
            let fields = engine.expand_object(0xABC).expect("must find instance");
            assert_eq!(fields.len(), 2);
            assert_eq!(fields[0].name, "b");
            assert_eq!(fields[0].value, FieldValue::Int(2));
            assert_eq!(fields[1].name, "a");
            assert_eq!(fields[1].value, FieldValue::Int(1));
        }

        #[test]
        fn expand_object_unknown_id_returns_none() {
            let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
                .add_instance(0xABC, 0, 100, &[])
                .build();
            let engine = engine_from_bytes(&bytes);
            assert!(engine.expand_object(0xDEAD).is_none());
        }

        #[test]
        fn expand_object_enriches_object_ref_with_class_name() {
            // Parent object (0xABC) has one ObjectRef field pointing to child (0xDEAD).
            // child class_object_id=200 → LOAD_CLASS with name "java/util/ArrayList".
            let child_id: u64 = 0xDEAD;
            let field_data = child_id.to_be_bytes().to_vec();
            let child_data: Vec<u8> = vec![];
            let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
                .add_string(1, "child")
                .add_string(2, "java/util/ArrayList")
                .add_class(1, 200, 0, 2)
                .add_class_dump(100, 0, 8, &[(1, 2u8)])
                .add_class_dump(200, 0, 0, &[])
                .add_instance(0xABC, 0, 100, &field_data)
                .add_instance(0xDEAD, 0, 200, &child_data)
                .build();
            let engine = engine_from_bytes(&bytes);
            let fields = engine.expand_object(0xABC).expect("must find instance");
            assert_eq!(fields.len(), 1);
            if let FieldValue::ObjectRef { id, class_name, .. } = &fields[0].value {
                assert_eq!(*id, 0xDEAD);
                assert_eq!(class_name, "java.util.ArrayList");
            } else {
                panic!("expected ObjectRef");
            }
        }

        #[test]
        fn expand_object_string_field_without_array_has_no_inline_value() {
            // Parent (0xABC) has one ObjectRef field pointing to child (0xDEAD).
            // child class_object_id=1000, LOAD_CLASS with name "java/lang/String".
            // No backing array → inline_value is None.
            let child_id: u64 = 0xDEAD;
            let field_data = child_id.to_be_bytes().to_vec();
            let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
                .add_string(1, "strField")
                .add_string(2, "java/lang/String")
                .add_class(1, 1000, 0, 2)
                .add_class_dump(100, 0, 8, &[(1, 2u8)])
                .add_class_dump(1000, 0, 0, &[])
                .add_instance(0xABC, 0, 100, &field_data)
                .add_instance(0xDEAD, 0, 1000, &[])
                .build();
            let engine = engine_from_bytes(&bytes);
            let fields = engine.expand_object(0xABC).expect("must find instance");
            assert_eq!(fields.len(), 1);
            assert_eq!(
                fields[0].value,
                FieldValue::ObjectRef {
                    id: 0xDEAD,
                    class_name: "java.lang.String".to_string(),
                    entry_count: None,
                    inline_value: None,
                }
            );
        }

        #[test]
        fn expand_object_object_ref_with_unknown_child_id_uses_object_fallback() {
            // Child ID 0xDEAD is not in the file → fallback "Object"
            let child_id: u64 = 0xDEAD;
            let field_data = child_id.to_be_bytes().to_vec();
            let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
                .add_string(1, "child")
                .add_class_dump(100, 0, 8, &[(1, 2u8)])
                .add_instance(0xABC, 0, 100, &field_data)
                .build();
            let engine = engine_from_bytes(&bytes);
            let fields = engine.expand_object(0xABC).expect("must find instance");
            assert_eq!(fields.len(), 1);
            if let FieldValue::ObjectRef { class_name, .. } = &fields[0].value {
                assert_eq!(class_name, "Object");
            } else {
                panic!("expected ObjectRef");
            }
        }

        #[test]
        fn expand_object_object_ref_field_returns_object_ref_not_expanded() {
            let id: u64 = 0xDEAD;
            let field_data = id.to_be_bytes().to_vec();
            let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
                .add_string(1, "child")
                .add_class_dump(100, 0, 8, &[(1, 2u8)]) // type 2 = object ref
                .add_instance(0xABC, 0, 100, &field_data)
                .build();
            let engine = engine_from_bytes(&bytes);
            let fields = engine.expand_object(0xABC).expect("must find instance");
            assert_eq!(fields.len(), 1);
            // 0xDEAD has no class info → fallback "Object"
            assert_eq!(
                fields[0].value,
                FieldValue::ObjectRef {
                    id: 0xDEAD,
                    class_name: "Object".to_string(),
                    entry_count: None,
                    inline_value: None,
                }
            );
        }
    }

    mod truncate_inline_tests {
        use super::truncate_inline;

        #[test]
        fn short_ascii_string_returned_unchanged() {
            let s = "hello".to_string();
            assert_eq!(truncate_inline(s), "hello");
        }

        #[test]
        fn exactly_80_chars_returned_unchanged() {
            let s = "a".repeat(80);
            let result = truncate_inline(s.clone());
            assert_eq!(result, s);
        }

        #[test]
        fn over_80_ascii_chars_truncated_with_dotdot() {
            let s = "a".repeat(90);
            let result = truncate_inline(s);
            assert!(result.ends_with(".."));
            assert_eq!(result.chars().count(), 80); // 78 chars + ".."
        }

        #[test]
        fn multi_byte_utf8_does_not_panic_and_truncates_at_char_boundary() {
            // Each '中' is 3 UTF-8 bytes — byte-slicing would panic
            let s = "中".repeat(85);
            let result = truncate_inline(s);
            assert!(result.ends_with(".."));
            // Result must be valid UTF-8 (no panic = success, but also verify)
            assert!(std::str::from_utf8(result.as_bytes()).is_ok());
        }

        #[test]
        fn multi_byte_utf8_exactly_80_chars_returned_unchanged() {
            let s = "é".repeat(80); // 2 bytes each in UTF-8
            let result = truncate_inline(s.clone());
            assert_eq!(result, s);
        }
    }

    mod resolve_inline_value_tests {
        use std::io::Write as IoWrite;

        use hprof_parser::HprofTestBuilder;

        use super::*;
        use crate::engine::FieldValue;

        fn engine_from_bytes(bytes: &[u8]) -> Engine {
            let mut tmp = tempfile::NamedTempFile::new().unwrap();
            tmp.write_all(bytes).unwrap();
            tmp.flush().unwrap();
            Engine::from_file(tmp.path(), &EngineConfig::default()).unwrap()
        }

        /// Builds a parent object with one ObjectRef field pointing to a
        /// boxed-type child. Returns the engine and the child's class name.
        fn expand_boxed_child(
            class_name: &str,
            value_type_byte: u8,
            value_bytes: Vec<u8>,
        ) -> crate::engine::FieldValue {
            let value_bytes_len = value_bytes.len();
            let child_id: u64 = 0xBBBB;
            let field_data = child_id.to_be_bytes().to_vec();
            let class_name_slashes = class_name.replace('.', "/");
            let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
                .add_string(1, "field")
                .add_string(2, &class_name_slashes)
                .add_string(3, "value")
                .add_class(1, 200, 0, 2)
                .add_class_dump(100, 0, 8, &[(1, 2u8)])
                .add_class_dump(200, 0, value_bytes_len as u32, &[(3, value_type_byte)])
                .add_instance(0xAAAA, 0, 100, &field_data)
                .add_instance(child_id, 0, 200, &value_bytes)
                .build();
            let engine = engine_from_bytes(&bytes);
            let fields = engine.expand_object(0xAAAA).unwrap();
            fields.into_iter().next().unwrap().value
        }

        #[test]
        fn integer_field_shows_inline_value() {
            let v = expand_boxed_child("java.lang.Integer", 10, 42i32.to_be_bytes().to_vec());
            if let FieldValue::ObjectRef { inline_value, .. } = v {
                assert_eq!(inline_value.as_deref(), Some("42"));
            } else {
                panic!("expected ObjectRef");
            }
        }

        #[test]
        fn boolean_true_field_shows_inline_value() {
            let v = expand_boxed_child("java.lang.Boolean", 4, vec![1u8]);
            if let FieldValue::ObjectRef { inline_value, .. } = v {
                assert_eq!(inline_value.as_deref(), Some("true"));
            } else {
                panic!("expected ObjectRef");
            }
        }

        #[test]
        fn boolean_false_field_shows_inline_value() {
            let v = expand_boxed_child("java.lang.Boolean", 4, vec![0u8]);
            if let FieldValue::ObjectRef { inline_value, .. } = v {
                assert_eq!(inline_value.as_deref(), Some("false"));
            } else {
                panic!("expected ObjectRef");
            }
        }

        #[test]
        fn character_field_shows_inline_value() {
            let v = expand_boxed_child(
                "java.lang.Character",
                5,
                (b'A' as u16).to_be_bytes().to_vec(),
            );
            if let FieldValue::ObjectRef { inline_value, .. } = v {
                assert_eq!(inline_value.as_deref(), Some("'A'"));
            } else {
                panic!("expected ObjectRef");
            }
        }

        #[test]
        fn long_field_shows_inline_value() {
            let v = expand_boxed_child(
                "java.lang.Long",
                11,
                9_876_543_210i64.to_be_bytes().to_vec(),
            );
            if let FieldValue::ObjectRef { inline_value, .. } = v {
                assert_eq!(inline_value.as_deref(), Some("9876543210"));
            } else {
                panic!("expected ObjectRef");
            }
        }

        #[test]
        fn unknown_class_returns_no_inline_value() {
            let v = expand_boxed_child("com.example.Foo", 10, 1i32.to_be_bytes().to_vec());
            if let FieldValue::ObjectRef { inline_value, .. } = v {
                assert_eq!(inline_value, None);
            } else {
                panic!("expected ObjectRef");
            }
        }

        #[test]
        fn float_field_shows_inline_value() {
            let v = expand_boxed_child(
                "java.lang.Float",
                6,
                std::f32::consts::PI.to_be_bytes().to_vec(),
            );
            if let FieldValue::ObjectRef { inline_value, .. } = v {
                assert!(inline_value.is_some(), "expected Some for Float");
                let s = inline_value.unwrap();
                assert!(s.starts_with("3.14"), "expected float repr, got {s}");
            } else {
                panic!("expected ObjectRef");
            }
        }

        #[test]
        fn double_field_shows_inline_value() {
            let v = expand_boxed_child(
                "java.lang.Double",
                7,
                std::f64::consts::E.to_be_bytes().to_vec(),
            );
            if let FieldValue::ObjectRef { inline_value, .. } = v {
                assert!(inline_value.is_some(), "expected Some for Double");
                let s = inline_value.unwrap();
                assert!(s.starts_with("2.718"), "expected double repr, got {s}");
            } else {
                panic!("expected ObjectRef");
            }
        }

        #[test]
        fn byte_field_shows_inline_value() {
            let v = expand_boxed_child("java.lang.Byte", 8, vec![127u8]);
            if let FieldValue::ObjectRef { inline_value, .. } = v {
                assert_eq!(inline_value.as_deref(), Some("127"));
            } else {
                panic!("expected ObjectRef");
            }
        }

        #[test]
        fn short_field_shows_inline_value() {
            let v = expand_boxed_child("java.lang.Short", 9, (-1234i16).to_be_bytes().to_vec());
            if let FieldValue::ObjectRef { inline_value, .. } = v {
                assert_eq!(inline_value.as_deref(), Some("-1234"));
            } else {
                panic!("expected ObjectRef");
            }
        }

        #[test]
        fn boxed_type_without_value_field_returns_none() {
            // Integer class but no "value" field — only "dummy"
            let child_id: u64 = 0xBBBB;
            let field_data = child_id.to_be_bytes().to_vec();
            let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
                .add_string(1, "field")
                .add_string(2, "java/lang/Integer")
                .add_string(3, "dummy")
                .add_class(1, 200, 0, 2)
                .add_class_dump(100, 0, 8, &[(1, 2u8)])
                .add_class_dump(200, 0, 4, &[(3, 10u8)])
                .add_instance(0xAAAA, 0, 100, &field_data)
                .add_instance(child_id, 0, 200, &99i32.to_be_bytes())
                .build();
            let engine = engine_from_bytes(&bytes);
            let fields = engine.expand_object(0xAAAA).unwrap();
            if let FieldValue::ObjectRef { inline_value, .. } = &fields[0].value {
                assert_eq!(*inline_value, None, "no 'value' field means no inline");
            } else {
                panic!("expected ObjectRef");
            }
        }
    }

    mod builder_tests {
        use std::io::Write as IoWrite;

        use hprof_parser::HprofTestBuilder;

        use super::*;

        #[test]
        fn list_threads_returns_unknown_state_for_all_threads() {
            let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
                .add_string(10, "main")
                .add_string(11, "worker-1")
                .add_thread(1, 100, 0, 10, 0, 0)
                .add_thread(2, 101, 0, 11, 0, 0)
                .build();

            let mut tmp = tempfile::NamedTempFile::new().unwrap();
            tmp.write_all(&bytes).unwrap();
            tmp.flush().unwrap();

            let config = EngineConfig::default();
            let engine = Engine::from_file(tmp.path(), &config).unwrap();
            let threads = engine.list_threads();

            assert!(
                threads.iter().all(|t| t.state == ThreadState::Unknown),
                "all threads must report ThreadState::Unknown until Story 3.4"
            );
        }

        #[test]
        fn list_threads_returns_three_threads_with_resolved_names() {
            let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
                .add_string(10, "main")
                .add_string(11, "worker-1")
                .add_string(12, "worker-2")
                .add_thread(1, 100, 0, 10, 0, 0)
                .add_thread(2, 101, 0, 11, 0, 0)
                .add_thread(3, 102, 0, 12, 0, 0)
                .build();

            let mut tmp = tempfile::NamedTempFile::new().unwrap();
            tmp.write_all(&bytes).unwrap();
            tmp.flush().unwrap();

            let config = EngineConfig::default();
            let engine = Engine::from_file(tmp.path(), &config).unwrap();
            let threads = engine.list_threads();

            assert_eq!(threads.len(), 3);
            assert_eq!(threads[0].thread_serial, 1);
            assert_eq!(threads[0].name, "main");
            assert_eq!(threads[1].thread_serial, 2);
            assert_eq!(threads[1].name, "worker-1");
            assert_eq!(threads[2].thread_serial, 3);
            assert_eq!(threads[2].name, "worker-2");
        }

        #[test]
        fn list_threads_unknown_name_string_id_returns_thread_serial_fallback() {
            // Thread with name_string_id=99, but no string record with id=99.
            // Expect "Thread-{serial}" fallback, not "<unknown:99>".
            let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
                .add_thread(1, 100, 0, 99, 0, 0)
                .build();

            let mut tmp = tempfile::NamedTempFile::new().unwrap();
            tmp.write_all(&bytes).unwrap();
            tmp.flush().unwrap();

            let config = EngineConfig::default();
            let engine = Engine::from_file(tmp.path(), &config).unwrap();
            let threads = engine.list_threads();

            assert_eq!(threads.len(), 1);
            assert_eq!(threads[0].name, "Thread-1");
        }

        #[test]
        fn list_threads_synthetic_from_stack_trace_shows_thread_serial_name() {
            // File with no START_THREAD records but with a STACK_TRACE that
            // references thread_serial=2. A synthetic thread must appear with
            // name "Thread-2".
            let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
                .add_stack_trace(5, 2, &[])
                .build();

            let mut tmp = tempfile::NamedTempFile::new().unwrap();
            tmp.write_all(&bytes).unwrap();
            tmp.flush().unwrap();

            let config = EngineConfig::default();
            let engine = Engine::from_file(tmp.path(), &config).unwrap();
            let threads = engine.list_threads();

            assert_eq!(threads.len(), 1);
            assert_eq!(threads[0].thread_serial, 2);
            assert_eq!(threads[0].name, "Thread-2");
        }

        #[test]
        fn select_thread_returns_some_for_known_serial() {
            let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
                .add_string(10, "main")
                .add_thread(1, 100, 0, 10, 0, 0)
                .build();

            let mut tmp = tempfile::NamedTempFile::new().unwrap();
            tmp.write_all(&bytes).unwrap();
            tmp.flush().unwrap();

            let config = EngineConfig::default();
            let engine = Engine::from_file(tmp.path(), &config).unwrap();

            let found = engine.select_thread(1);
            assert!(found.is_some());
            let t = found.unwrap();
            assert_eq!(t.thread_serial, 1);
            assert_eq!(t.name, "main");

            assert!(engine.select_thread(999).is_none());
        }
    }

    mod thread_state_mapping {
        use super::super::*;

        #[test]
        fn status_zero_is_unknown() {
            assert_eq!(thread_state_from_status(0), ThreadState::Unknown);
        }

        #[test]
        fn status_runnable() {
            // JVMTI RUNNABLE = 0x0004
            assert_eq!(thread_state_from_status(0x0004), ThreadState::Runnable);
        }

        #[test]
        fn status_blocked() {
            // JVMTI BLOCKED_ON_MONITOR_ENTER = 0x0400
            assert_eq!(thread_state_from_status(0x0400), ThreadState::Blocked);
        }

        #[test]
        fn status_waiting() {
            // JVMTI WAITING_INDEFINITELY = 0x0010
            assert_eq!(thread_state_from_status(0x0010), ThreadState::Waiting);
        }

        #[test]
        fn status_timed_waiting() {
            // JVMTI WAITING_WITH_TIMEOUT = 0x0020
            assert_eq!(thread_state_from_status(0x0020), ThreadState::Waiting);
        }

        #[test]
        fn status_terminated_is_unknown() {
            // TERMINATED = 0x0002
            assert_eq!(thread_state_from_status(0x0002), ThreadState::Unknown);
        }

        #[test]
        fn status_new_is_unknown() {
            // NEW = 0x0001 (bit 0 only, no runnable bit)
            assert_eq!(thread_state_from_status(0x0001), ThreadState::Unknown);
        }

        #[test]
        fn runnable_takes_priority_over_other_bits() {
            // RUNNABLE bit set alongside others
            assert_eq!(thread_state_from_status(0x0005), ThreadState::Runnable);
        }
    }

    /// Smoke test on real jvisualvm dump — run manually with:
    /// `cargo test -p hprof-engine real_dump -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_dump_thread_states() {
        let path = std::path::Path::new("../../assets/heapdump-visualvm.hprof");
        if !path.exists() {
            eprintln!("skip: dump not found");
            return;
        }
        let config = EngineConfig::default();
        let engine = Engine::from_file(path, &config).unwrap();
        let threads = engine.list_threads();
        for t in &threads {
            eprintln!(
                "serial={:3} state={:?} name={}",
                t.thread_serial, t.state, t.name
            );
        }
        let has_non_unknown = threads.iter().any(|t| t.state != ThreadState::Unknown);
        assert!(
            has_non_unknown,
            "expected at least one non-Unknown thread state"
        );
    }

    #[test]
    fn memory_budget_default_uses_auto_calc() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&minimal_hprof_bytes()).unwrap();
        tmp.flush().unwrap();

        let config = EngineConfig::default();
        let engine = Engine::from_file(tmp.path(), &config).unwrap();
        assert!(engine.memory_budget() > 0, "auto-calc budget must be > 0");
    }

    #[test]
    fn memory_budget_explicit_override() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&minimal_hprof_bytes()).unwrap();
        tmp.flush().unwrap();

        let config = EngineConfig {
            budget_bytes: Some(1_000_000),
        };
        let engine = Engine::from_file(tmp.path(), &config).unwrap();
        assert_eq!(engine.memory_budget(), 1_000_000);
    }

    mod lru_eviction_tests {
        use std::io::Write as IoWrite;

        use hprof_parser::HprofTestBuilder;

        use super::*;

        /// Builds an engine with two distinct expandable
        /// objects (0xAAA, 0xBBB) and the given budget.
        fn engine_two_objects(
            budget: u64,
        ) -> Engine {
            let bytes =
                HprofTestBuilder::new(
                    "JAVA PROFILE 1.0.2",
                    8,
                )
                .add_string(1, "x")
                .add_string(2, "y")
                .add_class_dump(
                    100, 0, 4,
                    &[(1, 10u8)],
                )
                .add_class_dump(
                    200, 0, 4,
                    &[(2, 10u8)],
                )
                .add_instance(
                    0xAAA, 0, 100,
                    &7i32.to_be_bytes(),
                )
                .add_instance(
                    0xBBB, 0, 200,
                    &8i32.to_be_bytes(),
                )
                .build();
            let mut tmp =
                tempfile::NamedTempFile::new().unwrap();
            tmp.write_all(&bytes).unwrap();
            tmp.flush().unwrap();
            let config = EngineConfig {
                budget_bytes: Some(budget),
            };
            Engine::from_file(
                tmp.path(), &config,
            )
            .unwrap()
        }

        /// Builds an engine with four expandable objects.
        fn engine_four_objects(
            budget: u64,
        ) -> Engine {
            let bytes =
                HprofTestBuilder::new(
                    "JAVA PROFILE 1.0.2",
                    8,
                )
                .add_string(1, "a")
                .add_string(2, "b")
                .add_string(3, "c")
                .add_string(4, "d")
                .add_class_dump(
                    100, 0, 4,
                    &[(1, 10u8)],
                )
                .add_class_dump(
                    200, 0, 4,
                    &[(2, 10u8)],
                )
                .add_class_dump(
                    300, 0, 4,
                    &[(3, 10u8)],
                )
                .add_class_dump(
                    400, 0, 4,
                    &[(4, 10u8)],
                )
                .add_instance(
                    0xAAA, 0, 100,
                    &1i32.to_be_bytes(),
                )
                .add_instance(
                    0xBBB, 0, 200,
                    &2i32.to_be_bytes(),
                )
                .add_instance(
                    0xCCC, 0, 300,
                    &3i32.to_be_bytes(),
                )
                .add_instance(
                    0xDDD, 0, 400,
                    &4i32.to_be_bytes(),
                )
                .build();
            let mut tmp =
                tempfile::NamedTempFile::new().unwrap();
            tmp.write_all(&bytes).unwrap();
            tmp.flush().unwrap();
            let config = EngineConfig {
                budget_bytes: Some(budget),
            };
            Engine::from_file(
                tmp.path(), &config,
            )
            .unwrap()
        }

        #[test]
        fn expand_object_cached_does_not_double_count_memory()
        {
            let engine =
                engine_two_objects(10_000_000);
            engine
                .expand_object(0xAAA)
                .expect("first expand");
            let mem_after_first = engine.memory_used();
            engine
                .expand_object(0xAAA)
                .expect("second expand (cache hit)");
            let mem_after_second = engine.memory_used();
            assert_eq!(
                mem_after_first, mem_after_second,
                "cache hit must not increase memory_used"
            );
        }

        #[test]
        fn expand_object_with_tiny_budget_triggers_eviction()
        {
            let engine = engine_two_objects(1);
            engine
                .expand_object(0xAAA)
                .expect("expand A");
            engine
                .expand_object(0xBBB)
                .expect("expand B");
            assert!(
                engine.memory_used() > 0,
                "something must be tracked"
            );
            // Cache may be empty (eviction drained it)
            // or have 1 entry (last insert survived).
            // Either way, the loop terminated without
            // hanging.
        }

        #[test]
        fn expand_object_lru_order_respected() {
            // Large budget: no automatic eviction —
            // we control eviction manually via cache API.
            let engine =
                engine_four_objects(10_000_000);

            // Insert A, B, C (insertion order = LRU order)
            // After inserts: LRU → A < B < C ← MRU
            engine.expand_object(0xAAA).unwrap();
            engine.expand_object(0xBBB).unwrap();
            engine.expand_object(0xCCC).unwrap();
            assert_eq!(engine.object_cache.len(), 3);

            // Promote A to MRU via cache hit
            // New LRU order: B < C < A (MRU)
            let mem_before_a = engine.memory_used();
            engine.expand_object(0xAAA).unwrap();
            assert_eq!(
                engine.memory_used(),
                mem_before_a,
                "A promote: must be a cache hit"
            );

            // Manually evict LRU — must be B
            let b_evicted =
                engine.object_cache.evict_lru();
            assert!(
                b_evicted.is_some(),
                "first evict must return B's size"
            );

            // Manually evict LRU — must be C
            let c_evicted =
                engine.object_cache.evict_lru();
            assert!(
                c_evicted.is_some(),
                "second evict must return C's size"
            );

            // A (MRU) is the sole survivor
            assert_eq!(
                engine.object_cache.len(),
                1,
                "only A (MRU) must remain after two evictions"
            );

            // A is a cache hit → memory_used unchanged
            let mem_before = engine.memory_used();
            engine.expand_object(0xAAA).unwrap();
            assert_eq!(
                engine.memory_used(),
                mem_before,
                "A: cache hit must not increase memory_used"
            );

            // B was LRU and was evicted → cache miss →
            // re-parse from mmap → memory_used increases
            // (note: direct evict_lru didn't adjust counter,
            //  so add() on re-insert is still visible)
            let mem_before_b = engine.memory_used();
            engine.expand_object(0xBBB).unwrap();
            assert!(
                engine.memory_used() > mem_before_b,
                "B was LRU-evicted → re-expand must \
                 increase memory_used (cache miss)"
            );
        }

        #[test]
        fn expand_object_ac4_usage_below_target_after_eviction()
        {
            // Budget = 1 byte: baseline already exceeds
            // budget. After expand, either usage < 60%
            // or cache is empty (FM-2 behavior).
            let engine = engine_two_objects(1);
            engine.expand_object(0xAAA).unwrap();
            engine.expand_object(0xBBB).unwrap();
            let ratio = engine.memory_used() as f64
                / engine.memory_budget() as f64;
            let cache_empty =
                engine.object_cache.is_empty();
            assert!(
                ratio < 0.60 || cache_empty,
                "AC4: usage {ratio:.2} must be < 0.60 \
                 or cache must be empty"
            );
        }

        #[test]
        fn eviction_loop_terminates_when_cache_empty()
        {
            // Budget so small baseline alone exceeds
            // EVICTION_TARGET. expand_object must still
            // return Some and not hang.
            let engine = engine_two_objects(1);
            let result = engine.expand_object(0xAAA);
            assert!(
                result.is_some(),
                "must return fields even with tiny budget"
            );
        }

        #[test]
        fn re_parse_after_eviction_produces_identical_fields() {
            // Budget = 1 → every expand triggers immediate full eviction
            // A is evicted as soon as it is inserted (it becomes the sole
            // LRU entry and the eviction loop drains the cache).
            let engine = engine_two_objects(1);
            let fields_first =
                engine.expand_object(0xAAA).unwrap();
            assert!(
                engine.object_cache.is_empty(),
                "A must be evicted immediately with budget=1"
            );
            // Expand B to confirm eviction and internal state remain sane
            engine.expand_object(0xBBB).unwrap();
            assert!(
                engine.object_cache.is_empty(),
                "B must also be evicted immediately with budget=1"
            );
            // Re-expand A: must be a cache miss → re-parse from mmap
            let fields_second =
                engine.expand_object(0xAAA).unwrap();
            assert_eq!(
                fields_first, fields_second,
                "re-parse must produce byte-identical fields (AC2 / NFR8)"
            );
        }

        #[test]
        fn multi_cycle_no_panic_no_counter_overflow() {
            // Budget = 1 → each expand evicts all cached data.
            // 50 cycles of alternating A/B expansion must not panic
            // and must not overflow the MemoryCounter.
            let engine = engine_two_objects(1);
            for _ in 0..50 {
                let r_a = engine.expand_object(0xAAA);
                assert!(
                    r_a.is_some(),
                    "A must always return Some across all cycles"
                );
                let r_b = engine.expand_object(0xBBB);
                assert!(
                    r_b.is_some(),
                    "B must always return Some across all cycles"
                );
            }
            // usize::MAX / 2 is a conservative sentinel: real usage is
            // at most a few KB; any value above this indicates underflow.
            assert!(
                engine.memory_used() < usize::MAX / 2,
                "MemoryCounter must not underflow to usize::MAX"
            );
        }
    }
}
