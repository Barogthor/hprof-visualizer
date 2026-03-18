//! Background pre-walk for HashMap/LinkedList collections.
//!
//! Spawns a thread that traverses an entire collection,
//! caching node offsets in `OffsetCache` and building
//! skip-index checkpoints incrementally. Deep page jumps
//! become fast once the walker has covered that range.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};

use rustc_hash::FxHashSet;

use hprof_parser::HprofFile;

use crate::engine::FieldValue;
use crate::engine_impl::Engine;
use crate::resolver::decode_fields;

use super::skip_index::{SKIP_INTERVAL, SkipCheckpoint};

/// Maximum number of concurrent background walkers.
pub(crate) const MAX_WALKERS: usize = 8;

/// Number of entries between channel batch sends.
const BATCH_SEND_INTERVAL: usize = 1000;

/// Messages sent from the background walker thread to
/// the engine via `mpsc::channel`.
pub(crate) enum WalkMessage {
    /// Batch of skip-index checkpoints discovered.
    /// Offsets are inserted directly into `OffsetCache`
    /// by the walker (RwLock, thread-safe).
    Batch {
        checkpoints: Vec<(usize, SkipCheckpoint)>,
    },
    /// Walk completed — all entries traversed.
    Complete,
}

/// Handle to a running background walker thread.
pub(crate) struct WalkerHandle {
    rx: Receiver<WalkMessage>,
    /// `pub(crate)` so `Engine::drop` and `cancel_walker`
    /// can call `join_handle.take()`.
    pub(crate) join_handle: Option<std::thread::JoinHandle<()>>,
    progress: Arc<AtomicUsize>,
    cancel: Arc<AtomicBool>,
}

impl WalkerHandle {
    /// Creates a new `WalkerHandle`.
    pub(crate) fn new(
        rx: Receiver<WalkMessage>,
        join_handle: std::thread::JoinHandle<()>,
        progress: Arc<AtomicUsize>,
        cancel: Arc<AtomicBool>,
    ) -> Self {
        Self {
            rx,
            join_handle: Some(join_handle),
            progress,
            cancel,
        }
    }

    /// Drains all pending messages from the channel.
    ///
    /// Returns `(messages, disconnected)` where
    /// `disconnected` is `true` when the sender has
    /// been dropped (walker finished or panicked).
    pub(crate) fn try_drain(&self) -> (Vec<WalkMessage>, bool) {
        let mut messages = Vec::new();
        loop {
            match self.rx.try_recv() {
                Ok(msg) => messages.push(msg),
                Err(TryRecvError::Empty) => {
                    return (messages, false);
                }
                Err(TryRecvError::Disconnected) => {
                    return (messages, true);
                }
            }
        }
    }

    /// Requests cancellation of the walker thread.
    pub(crate) fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Returns the current progress (entry count).
    pub(crate) fn progress(&self) -> usize {
        self.progress.load(Ordering::Relaxed)
    }

    /// Returns `true` if cancellation was requested.
    #[cfg(test)]
    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

/// Walks a collection in the background, building
/// skip-index checkpoints and caching offsets.
///
/// Runs on a spawned thread. Does NOT extract key/value
/// pairs — only resolves nodes to discover offsets and
/// build checkpoints. Significantly faster than the
/// page extractors.
pub(crate) fn walk_collection_background(
    hfile: Arc<HprofFile>,
    collection_id: u64,
    tx: Sender<WalkMessage>,
    progress: Arc<AtomicUsize>,
    cancel: Arc<AtomicBool>,
) {
    let Some(raw) = Engine::read_instance_public(&hfile, collection_id) else {
        let _ = tx.send(WalkMessage::Complete);
        return;
    };

    let class_name = match hfile.index.class_names_by_id.get(&raw.class_object_id) {
        Some(cn) => cn.clone(),
        None => {
            let _ = tx.send(WalkMessage::Complete);
            return;
        }
    };
    let short_name = class_name.rsplit('.').next().unwrap_or(class_name.as_str());

    match short_name {
        "HashMap" | "LinkedHashMap" => {
            walk_hash_map(&hfile, &raw, false, &tx, &progress, &cancel);
        }
        "ConcurrentHashMap" => {
            walk_hash_map(&hfile, &raw, true, &tx, &progress, &cancel);
        }
        "LinkedList" => {
            walk_linked_list(&hfile, &raw, &tx, &progress, &cancel);
        }
        "HashSet" | "LinkedHashSet" => {
            walk_hash_set(&hfile, &raw, &tx, &progress, &cancel);
        }
        _ => {
            // Unsupported collection type
        }
    }

    if !cancel.load(Ordering::Relaxed) {
        let _ = tx.send(WalkMessage::Complete);
    }
}

/// Extracts the `next` field ID from decoded fields.
fn extract_next_id(fields: &[crate::engine::FieldInfo]) -> u64 {
    fields
        .iter()
        .find(|f| f.name == "next")
        .and_then(|f| match f.value {
            FieldValue::ObjectRef { id, .. } => Some(id),
            _ => None,
        })
        .unwrap_or(0)
}

/// Background walk for HashMap / LinkedHashMap /
/// ConcurrentHashMap.
fn walk_hash_map(
    hfile: &HprofFile,
    raw: &hprof_parser::RawInstance,
    concurrent: bool,
    tx: &Sender<WalkMessage>,
    progress: &Arc<AtomicUsize>,
    cancel: &Arc<AtomicBool>,
) {
    let fields = decode_fields(
        raw,
        &hfile.index,
        hfile.header.id_size,
        hfile.records_bytes(),
    );

    let table_id = fields.iter().find_map(|f| {
        if f.name == "table"
            && let FieldValue::ObjectRef { id, .. } = f.value
        {
            return Some(id);
        }
        None
    });
    let Some(table_arr_id) = table_id else { return };
    if table_arr_id == 0 {
        return;
    }
    let Some((_class_id, table_elements)) = hfile.find_object_array(table_arr_id) else {
        return;
    };

    // Multi-level chain batching: batch-resolve all
    // non-zero table heads
    let uncached_heads: Vec<u64> = table_elements
        .iter()
        .copied()
        .filter(|&id| id != 0 && !hfile.index.instance_offsets.contains(&id))
        .collect();

    if !uncached_heads.is_empty() {
        let batch = hfile.batch_find_instances(&uncached_heads);
        hfile.index.instance_offsets.insert_batch(&batch.offsets);

        // For non-ConcurrentHashMap: decode each head's
        // `next` field to collect depth-1 node IDs
        if !concurrent {
            let mut depth1_ids: Vec<u64> = Vec::new();
            for head_raw in batch.instances.values() {
                let head_fields = decode_fields(
                    head_raw,
                    &hfile.index,
                    hfile.header.id_size,
                    hfile.records_bytes(),
                );
                let next_id = extract_next_id(&head_fields);
                if next_id != 0 && !hfile.index.instance_offsets.contains(&next_id) {
                    depth1_ids.push(next_id);
                }
            }
            if !depth1_ids.is_empty() {
                let batch2 = hfile.batch_find_instances(&depth1_ids);
                hfile.index.instance_offsets.insert_batch(&batch2.offsets);
            }
        }
    }

    // Walk the full collection
    let mut batch_buf: Vec<(usize, SkipCheckpoint)> = Vec::new();
    let mut entry_count: usize = 0;
    let mut visited = FxHashSet::default();

    for (slot_idx, &slot_id) in table_elements.iter().enumerate() {
        if slot_id == 0 {
            continue;
        }
        let mut node_id = slot_id;
        while node_id != 0 {
            if cancel.load(Ordering::Relaxed) {
                if !batch_buf.is_empty() {
                    let _ = tx.send(WalkMessage::Batch {
                        checkpoints: std::mem::take(&mut batch_buf),
                    });
                }
                return;
            }
            if !visited.insert(node_id) {
                break;
            }
            let Some(node_raw) = Engine::read_instance_public(hfile, node_id) else {
                break;
            };

            if entry_count.is_multiple_of(SKIP_INTERVAL) {
                batch_buf.push((
                    entry_count,
                    SkipCheckpoint::HashMapSlot {
                        slot_index: slot_idx,
                        node_id,
                    },
                ));
            }

            let node_fields = decode_fields(
                &node_raw,
                &hfile.index,
                hfile.header.id_size,
                hfile.records_bytes(),
            );
            node_id = extract_next_id(&node_fields);
            entry_count += 1;
            progress.store(entry_count, Ordering::Relaxed);

            if entry_count.is_multiple_of(BATCH_SEND_INTERVAL)
                && tx
                    .send(WalkMessage::Batch {
                        checkpoints: std::mem::take(&mut batch_buf),
                    })
                    .is_err()
            {
                return;
            }
        }
    }

    // Flush remaining checkpoints
    if !batch_buf.is_empty() {
        let _ = tx.send(WalkMessage::Batch {
            checkpoints: batch_buf,
        });
    }
}

/// Background walk for LinkedList.
fn walk_linked_list(
    hfile: &HprofFile,
    raw: &hprof_parser::RawInstance,
    tx: &Sender<WalkMessage>,
    progress: &Arc<AtomicUsize>,
    cancel: &Arc<AtomicBool>,
) {
    let fields = decode_fields(
        raw,
        &hfile.index,
        hfile.header.id_size,
        hfile.records_bytes(),
    );

    let mut node_id = fields
        .iter()
        .find_map(|f| {
            if f.name == "first"
                && let FieldValue::ObjectRef { id, .. } = f.value
            {
                return Some(id);
            }
            None
        })
        .unwrap_or(0);

    if node_id == 0 {
        return;
    }

    let mut batch_buf: Vec<(usize, SkipCheckpoint)> = Vec::new();
    let mut entry_count: usize = 0;
    let mut visited = FxHashSet::default();

    while node_id != 0 {
        if cancel.load(Ordering::Relaxed) {
            if !batch_buf.is_empty() {
                let _ = tx.send(WalkMessage::Batch {
                    checkpoints: std::mem::take(&mut batch_buf),
                });
            }
            return;
        }
        if !visited.insert(node_id) {
            break;
        }
        let Some(node_raw) = Engine::read_instance_public(hfile, node_id) else {
            break;
        };

        if entry_count.is_multiple_of(SKIP_INTERVAL) {
            batch_buf.push((entry_count, SkipCheckpoint::LinkedListNode { node_id }));
        }

        let node_fields = decode_fields(
            &node_raw,
            &hfile.index,
            hfile.header.id_size,
            hfile.records_bytes(),
        );
        node_id = extract_next_id(&node_fields);
        entry_count += 1;
        progress.store(entry_count, Ordering::Relaxed);

        if entry_count.is_multiple_of(BATCH_SEND_INTERVAL)
            && tx
                .send(WalkMessage::Batch {
                    checkpoints: std::mem::take(&mut batch_buf),
                })
                .is_err()
        {
            return;
        }
    }

    if !batch_buf.is_empty() {
        let _ = tx.send(WalkMessage::Batch {
            checkpoints: batch_buf,
        });
    }
}

/// HashSet/LinkedHashSet: delegate to HashMap walk via
/// the backing `map` field.
fn walk_hash_set(
    hfile: &HprofFile,
    raw: &hprof_parser::RawInstance,
    tx: &Sender<WalkMessage>,
    progress: &Arc<AtomicUsize>,
    cancel: &Arc<AtomicBool>,
) {
    let fields = decode_fields(
        raw,
        &hfile.index,
        hfile.header.id_size,
        hfile.records_bytes(),
    );

    let map_id = fields.iter().find_map(|f| {
        if f.name == "map"
            && let FieldValue::ObjectRef { id, .. } = f.value
        {
            return Some(id);
        }
        None
    });
    let Some(map_id) = map_id else { return };

    let Some(map_raw) = Engine::read_instance_public(hfile, map_id) else {
        return;
    };

    // Detect backing map's class to determine concurrent
    let map_class = hfile.index.class_names_by_id.get(&map_raw.class_object_id);
    let concurrent = map_class
        .map(|cn| cn.rsplit('.').next().unwrap_or(cn.as_str()) == "ConcurrentHashMap")
        .unwrap_or(false);

    walk_hash_map(hfile, &map_raw, concurrent, tx, progress, cancel);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    use hprof_parser::HprofTestBuilder;
    use std::io::Write;

    fn hfile_from_bytes(bytes: &[u8]) -> HprofFile {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(bytes).unwrap();
        tmp.flush().unwrap();
        HprofFile::from_path(tmp.path()).unwrap()
    }

    // -- Task 1 tests --

    #[test]
    fn walker_handle_try_drain_returns_pending() {
        let (tx, rx) = mpsc::channel();
        let progress = Arc::new(AtomicUsize::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let handle = WalkerHandle {
            rx,
            join_handle: None,
            progress,
            cancel,
        };

        tx.send(WalkMessage::Batch {
            checkpoints: vec![(0, SkipCheckpoint::LinkedListNode { node_id: 42 })],
        })
        .unwrap();
        tx.send(WalkMessage::Complete).unwrap();

        let (msgs, disconnected) = handle.try_drain();
        assert_eq!(msgs.len(), 2);
        assert!(!disconnected);
        assert!(matches!(msgs[0], WalkMessage::Batch { .. }));
        assert!(matches!(msgs[1], WalkMessage::Complete));
    }

    #[test]
    fn walker_handle_try_drain_disconnected() {
        let (tx, rx) = mpsc::channel();
        let progress = Arc::new(AtomicUsize::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let handle = WalkerHandle {
            rx,
            join_handle: None,
            progress,
            cancel,
        };

        tx.send(WalkMessage::Batch {
            checkpoints: vec![],
        })
        .unwrap();
        drop(tx);

        let (msgs, disconnected) = handle.try_drain();
        assert_eq!(msgs.len(), 1);
        assert!(disconnected);
    }

    #[test]
    fn walker_handle_cancel_sets_flag() {
        let (_tx, rx) = mpsc::channel::<WalkMessage>();
        let progress = Arc::new(AtomicUsize::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let handle = WalkerHandle {
            rx,
            join_handle: None,
            progress,
            cancel,
        };

        assert!(!handle.is_cancelled());
        handle.cancel();
        assert!(handle.is_cancelled());
    }

    #[test]
    fn walker_handle_progress() {
        let (_tx, rx) = mpsc::channel::<WalkMessage>();
        let progress = Arc::new(AtomicUsize::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let handle = WalkerHandle {
            rx,
            join_handle: None,
            progress: Arc::clone(&progress),
            cancel,
        };

        assert_eq!(handle.progress(), 0);
        progress.store(42, Ordering::Relaxed);
        assert_eq!(handle.progress(), 42);
    }

    // -- Test fixture builders --

    /// Builds a HashMap with `n` entries. When
    /// `chain_depth > 1`, groups entries into
    /// chains of that depth (fewer table slots).
    fn build_hashmap(n: usize, chain_depth: usize) -> Vec<u8> {
        let id_size: u32 = 8;
        let str_size = 10u64;
        let str_table = 11u64;
        let str_key = 12u64;
        let str_value = 13u64;
        let str_next = 14u64;
        let str_cn = 15u64;
        let str_node_cn = 16u64;

        let depth = chain_depth.max(1);
        let num_chains = n.div_ceil(depth);
        let mut table: Vec<u64> = Vec::with_capacity(num_chains);
        // Each chain starts at node_id for entry
        // i*depth
        for c in 0..num_chains {
            table.push(0x200u64 + (c * depth) as u64);
        }

        let mut hm_data = Vec::new();
        hm_data.extend_from_slice(&(n as i32).to_be_bytes());
        hm_data.extend_from_slice(&0x500u64.to_be_bytes());

        let mut builder = HprofTestBuilder::new("JAVA PROFILE 1.0.2", id_size)
            .add_string(str_size, "size")
            .add_string(str_table, "table")
            .add_string(str_key, "key")
            .add_string(str_value, "value")
            .add_string(str_next, "next")
            .add_string(str_cn, "java/util/HashMap")
            .add_string(str_node_cn, "java/util/HashMap$Node")
            .add_class(1, 1000, 0, str_cn)
            .add_class(2, 2000, 0, str_node_cn)
            .add_class_dump(1000, 0, 4 + id_size, &[(str_size, 10), (str_table, 2)])
            .add_class_dump(
                2000,
                0,
                id_size * 3,
                &[(str_key, 2), (str_value, 2), (str_next, 2)],
            )
            .add_instance(0x100, 0, 1000, &hm_data)
            .add_object_array(0x500, 0, 2000, &table);

        for i in 0..n {
            let node_id = 0x200u64 + i as u64;
            let key_id = 0x1000u64 + i as u64;
            let val_id = 0x2000u64 + i as u64;
            // Next node: same chain if not last
            let chain_pos = i % depth;
            let next_id = if chain_pos + 1 < depth && i + 1 < n {
                0x200u64 + (i + 1) as u64
            } else {
                0u64
            };
            let mut node_data = Vec::new();
            node_data.extend_from_slice(&key_id.to_be_bytes());
            node_data.extend_from_slice(&val_id.to_be_bytes());
            node_data.extend_from_slice(&next_id.to_be_bytes());
            builder = builder.add_instance(node_id, 0, 2000, &node_data);
        }

        builder.build()
    }

    fn build_linked_list(n: usize) -> Vec<u8> {
        let id_size: u32 = 8;
        let str_size = 10u64;
        let str_first = 11u64;
        let str_last = 12u64;
        let str_item = 13u64;
        let str_next = 14u64;
        let str_prev = 15u64;
        let str_cn = 16u64;
        let str_node_cn = 17u64;

        let first_node = if n > 0 { 0x200u64 } else { 0 };
        let last_node = if n > 0 { 0x200u64 + (n as u64 - 1) } else { 0 };

        let mut ll_data = Vec::new();
        ll_data.extend_from_slice(&(n as i32).to_be_bytes());
        ll_data.extend_from_slice(&first_node.to_be_bytes());
        ll_data.extend_from_slice(&last_node.to_be_bytes());

        let mut builder = HprofTestBuilder::new("JAVA PROFILE 1.0.2", id_size)
            .add_string(str_size, "size")
            .add_string(str_first, "first")
            .add_string(str_last, "last")
            .add_string(str_item, "item")
            .add_string(str_next, "next")
            .add_string(str_prev, "prev")
            .add_string(str_cn, "java/util/LinkedList")
            .add_string(str_node_cn, "java/util/LinkedList$Node")
            .add_class(1, 1000, 0, str_cn)
            .add_class(2, 2000, 0, str_node_cn)
            .add_class_dump(
                1000,
                0,
                4 + id_size * 2,
                &[(str_size, 10), (str_first, 2), (str_last, 2)],
            )
            .add_class_dump(
                2000,
                0,
                id_size * 3,
                &[(str_item, 2), (str_next, 2), (str_prev, 2)],
            )
            .add_instance(0x100, 0, 1000, &ll_data);

        for i in 0..n {
            let node_id = 0x200u64 + i as u64;
            let item_id = 0x10u64 + i as u64;
            let next_id = if i + 1 < n {
                0x200u64 + (i + 1) as u64
            } else {
                0u64
            };
            let prev_id = if i > 0 {
                0x200u64 + (i - 1) as u64
            } else {
                0u64
            };
            let mut node_data = Vec::new();
            node_data.extend_from_slice(&item_id.to_be_bytes());
            node_data.extend_from_slice(&next_id.to_be_bytes());
            node_data.extend_from_slice(&prev_id.to_be_bytes());
            builder = builder.add_instance(node_id, 0, 2000, &node_data);
        }

        builder.build()
    }

    /// Collects all checkpoints from walk messages.
    fn collect_checkpoints(msgs: &[WalkMessage]) -> Vec<(usize, SkipCheckpoint)> {
        let mut cps = Vec::new();
        for msg in msgs {
            if let WalkMessage::Batch { checkpoints } = msg {
                cps.extend(checkpoints.iter().cloned());
            }
        }
        cps
    }

    /// Runs walk_collection_background and returns
    /// all messages + final progress.
    fn run_walk(hfile: &HprofFile, collection_id: u64) -> (Vec<WalkMessage>, usize) {
        let (tx, rx) = mpsc::channel();
        let progress = Arc::new(AtomicUsize::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let hfile_arc = unsafe {
            // SAFETY: HprofFile is behind an Arc in
            // prod; in tests we create a fake Arc
            // that shares the lifetime. The walk
            // function only reads from hfile.
            Arc::from_raw(hfile as *const HprofFile)
        };
        walk_collection_background(
            Arc::clone(&hfile_arc),
            collection_id,
            tx,
            Arc::clone(&progress),
            cancel,
        );
        // Prevent double-free: forget the Arc
        std::mem::forget(hfile_arc);
        let mut msgs = Vec::new();
        while let Ok(msg) = rx.recv() {
            msgs.push(msg);
        }
        let final_progress = progress.load(Ordering::Relaxed);
        (msgs, final_progress)
    }

    // -- Task 2 tests --

    #[test]
    fn walk_hashmap_50_entries() {
        let hfile = hfile_from_bytes(&build_hashmap(50, 1));
        let (msgs, progress) = run_walk(&hfile, 0x100);

        // Last message should be Complete
        assert!(matches!(msgs.last().unwrap(), WalkMessage::Complete));

        let cps = collect_checkpoints(&msgs);
        assert_eq!(progress, 50);

        // Checkpoint at entry 0
        assert!(
            cps.iter().any(|(idx, _)| *idx == 0),
            "should have checkpoint at entry 0"
        );
    }

    #[test]
    fn walk_hashmap_cancel_midway() {
        let hfile = hfile_from_bytes(&build_hashmap(200, 1));
        let (tx, rx) = mpsc::channel();
        let progress = Arc::new(AtomicUsize::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        // Cancel after a tiny delay
        cancel.store(true, Ordering::Relaxed);

        let hfile_arc = unsafe { Arc::from_raw(&hfile as *const HprofFile) };
        walk_collection_background(
            Arc::clone(&hfile_arc),
            0x100,
            tx,
            Arc::clone(&progress),
            Arc::clone(&cancel),
        );
        std::mem::forget(hfile_arc);

        let mut msgs = Vec::new();
        while let Ok(msg) = rx.recv() {
            msgs.push(msg);
        }
        // Should NOT have Complete (cancelled)
        assert!(
            !msgs.iter().any(|m| matches!(m, WalkMessage::Complete)),
            "cancelled walk should not send Complete"
        );
    }

    #[test]
    fn walk_linked_list_30_entries() {
        let hfile = hfile_from_bytes(&build_linked_list(30));
        let (msgs, progress) = run_walk(&hfile, 0x100);

        assert!(matches!(msgs.last().unwrap(), WalkMessage::Complete));
        assert_eq!(progress, 30);

        let cps = collect_checkpoints(&msgs);
        assert!(
            cps.iter().any(|(idx, _)| *idx == 0),
            "should have checkpoint at entry 0"
        );
    }

    #[test]
    fn walk_hashmap_depth2_chains() {
        // 50 entries in chains of depth 2
        let hfile = hfile_from_bytes(&build_hashmap(50, 2));
        let (msgs, progress) = run_walk(&hfile, 0x100);

        assert!(matches!(msgs.last().unwrap(), WalkMessage::Complete));
        assert_eq!(progress, 50);
    }

    #[test]
    fn walk_hashmap_counting_invariant() {
        // Create HashMap with 200 entries (> 1
        // SKIP_INTERVAL)
        let hfile = hfile_from_bytes(&build_hashmap(200, 1));

        // Full sequential walk (ground truth)
        let full = super::super::get_page(&hfile, 0x100, 0, 200, None).unwrap();
        assert_eq!(full.entries.len(), 200);

        // Walk with background walker
        let (msgs, _) = run_walk(&hfile, 0x100);
        let cps = collect_checkpoints(&msgs);

        // Apply checkpoints to a fresh SkipIndex
        let mut si = super::super::skip_index::SkipIndex::new(SKIP_INTERVAL);
        for (idx, cp) in &cps {
            si.record(*idx, cp.clone());
        }

        // Paginate WITH skip-index from
        // checkpoint 100
        let resumed = super::super::get_page(&hfile, 0x100, 100, 100, Some(&mut si)).unwrap();

        // Verify entries match ground truth
        for (i, entry) in resumed.entries.iter().enumerate() {
            assert_eq!(
                entry.key,
                full.entries[100 + i].key,
                "key mismatch at position {}",
                100 + i
            );
        }
    }

    #[test]
    fn walk_hashmap_cyclic_chain_terminates() {
        let id_size: u32 = 8;
        // Build a HashMap where node's next points
        // back to itself (cycle)
        let str_size = 10u64;
        let str_table = 11u64;
        let str_key = 12u64;
        let str_value = 13u64;
        let str_next = 14u64;
        let str_cn = 15u64;
        let str_node_cn = 16u64;

        let table = vec![0x200u64]; // one slot

        let mut hm_data = Vec::new();
        hm_data.extend_from_slice(&1i32.to_be_bytes());
        hm_data.extend_from_slice(&0x500u64.to_be_bytes());

        // Node 0x200: next = 0x200 (cycle!)
        let mut node_data = Vec::new();
        node_data.extend_from_slice(&0x1000u64.to_be_bytes()); // key
        node_data.extend_from_slice(&0x2000u64.to_be_bytes()); // value
        node_data.extend_from_slice(&0x200u64.to_be_bytes()); // next = self

        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", id_size)
            .add_string(str_size, "size")
            .add_string(str_table, "table")
            .add_string(str_key, "key")
            .add_string(str_value, "value")
            .add_string(str_next, "next")
            .add_string(str_cn, "java/util/HashMap")
            .add_string(str_node_cn, "java/util/HashMap$Node")
            .add_class(1, 1000, 0, str_cn)
            .add_class(2, 2000, 0, str_node_cn)
            .add_class_dump(1000, 0, 4 + id_size, &[(str_size, 10), (str_table, 2)])
            .add_class_dump(
                2000,
                0,
                id_size * 3,
                &[(str_key, 2), (str_value, 2), (str_next, 2)],
            )
            .add_instance(0x100, 0, 1000, &hm_data)
            .add_object_array(0x500, 0, 2000, &table)
            .add_instance(0x200, 0, 2000, &node_data)
            .build();

        let hfile = hfile_from_bytes(&bytes);
        let (msgs, progress) = run_walk(&hfile, 0x100);

        // Walker terminates despite cycle
        assert!(matches!(msgs.last().unwrap(), WalkMessage::Complete));
        // Only 1 entry (cycle broken by visited guard)
        assert_eq!(progress, 1);
    }

    #[test]
    fn walk_empty_hashmap() {
        let hfile = hfile_from_bytes(&build_hashmap(0, 1));
        let (msgs, progress) = run_walk(&hfile, 0x100);

        assert!(matches!(msgs.last().unwrap(), WalkMessage::Complete));
        assert_eq!(progress, 0);
    }
}
