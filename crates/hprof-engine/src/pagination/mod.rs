//! Collection pagination: type dispatch and extractors.
//!
//! Resolves a page of entries from a Java collection
//! object by identifying its concrete type and delegating
//! to the appropriate extractor.

mod skip_index;

use hprof_parser::{HprofFile, ObjectArrayMeta, RawInstance};

use crate::engine::{CollectionPage, EntryInfo, FieldValue};
use crate::engine_impl::Engine;
use crate::resolver::decode_fields;

use skip_index::SkipCheckpoint;
pub(crate) use skip_index::SkipIndex;

/// Minimum number of uncached head IDs required to use
/// `batch_find_instances`. Below this threshold the fixed
/// overhead of rayon+segment grouping exceeds the gain.
const BATCH_THRESHOLD: usize = 16;

/// Returns a page from the collection identified by
/// `collection_id`, or `None` if unsupported/not found.
pub(crate) fn get_page(
    hfile: &HprofFile,
    collection_id: u64,
    offset: usize,
    limit: usize,
    skip_index: Option<&mut SkipIndex>,
) -> Option<CollectionPage> {
    // Try as Object[] or primitive[] first
    if let Some(page) = try_object_array(hfile, collection_id, offset, limit) {
        dbg_log!(
            "get_page(0x{:X}, {}, {}): Object[] → {} entries",
            collection_id,
            offset,
            limit,
            page.entries.len()
        );
        return Some(page);
    }
    if let Some(page) = try_prim_array(hfile, collection_id, offset, limit) {
        dbg_log!(
            "get_page(0x{:X}, {}, {}): prim[] → {} entries",
            collection_id,
            offset,
            limit,
            page.entries.len()
        );
        return Some(page);
    }

    // Resolve as instance and dispatch by class name
    let raw = match Engine::read_instance_public(hfile, collection_id) {
        Some(r) => r,
        None => {
            dbg_log!("get_page(0x{:X}): instance not found", collection_id);
            return None;
        }
    };
    let class_name = match hfile.index.class_names_by_id.get(&raw.class_object_id) {
        Some(cn) => cn,
        None => {
            dbg_log!(
                "get_page(0x{:X}): class_name missing \
                 for class_obj=0x{:X}",
                collection_id,
                raw.class_object_id
            );
            return None;
        }
    };
    let short = class_name.rsplit('.').next().unwrap_or(class_name.as_str());

    match_extractor(short, hfile, &raw, offset, limit, skip_index)
}

fn match_extractor(
    short_name: &str,
    hfile: &HprofFile,
    raw: &RawInstance,
    offset: usize,
    limit: usize,
    skip_index: Option<&mut SkipIndex>,
) -> Option<CollectionPage> {
    if short_name.eq_ignore_ascii_case("ArrayList")
        || short_name.eq_ignore_ascii_case("CopyOnWriteArrayList")
        || short_name.eq_ignore_ascii_case("Vector")
    {
        return extract_array_list(hfile, raw, offset, limit);
    }
    if short_name.eq_ignore_ascii_case("HashMap")
        || short_name.eq_ignore_ascii_case("LinkedHashMap")
    {
        return extract_hash_map(hfile, raw, offset, limit, false, skip_index);
    }
    if short_name.eq_ignore_ascii_case("ConcurrentHashMap") {
        // TODO(11.6-4.3d): HprofTestBuilder cannot construct a ConcurrentHashMap
        // (segments structure too complex). Batch pre-resolution is inherited via
        // the shared extract_hash_map code path (concurrent=true). Manual test
        // required to verify batch behaviour on a real dump with ≥16 entries.
        return extract_hash_map(hfile, raw, offset, limit, true, skip_index);
    }
    if short_name.eq_ignore_ascii_case("HashSet")
        || short_name.eq_ignore_ascii_case("LinkedHashSet")
    {
        return extract_hash_set(hfile, raw, offset, limit, skip_index);
    }
    if short_name.eq_ignore_ascii_case("LinkedList") {
        return extract_linked_list(hfile, raw, offset, limit, skip_index);
    }
    // Unsupported collection type
    None
}

/// Extracts a page from an `ObjectArrayDump` directly.
///
/// Uses O(1) positional reads via [`ObjectArrayMeta`]
/// instead of deserializing all elements.
fn try_object_array(
    hfile: &HprofFile,
    array_id: u64,
    offset: usize,
    limit: usize,
) -> Option<CollectionPage> {
    let meta = hfile.find_object_array_meta(array_id)?;
    paginate_object_array(&meta, meta.num_elements as u64, offset, limit, hfile)
}

/// Extracts a page from a `PrimArrayDump` directly.
fn try_prim_array(
    hfile: &HprofFile,
    array_id: u64,
    offset: usize,
    limit: usize,
) -> Option<CollectionPage> {
    let (elem_type, bytes) = hfile.find_prim_array(array_id)?;
    let Some(elem_size) = prim_elem_size(elem_type) else {
        return Some(CollectionPage {
            entries: vec![],
            total_count: 0,
            offset: 0,
            has_more: false,
        });
    };
    let total = (bytes.len() / elem_size) as u64;
    let clamped_offset = (offset as u64).min(total) as usize;
    let remaining = (total - clamped_offset as u64) as usize;
    let actual_limit = limit.min(remaining);

    let mut entries = Vec::with_capacity(actual_limit);
    for i in 0..actual_limit {
        let idx = clamped_offset + i;
        let byte_start = idx * elem_size;
        let slice = &bytes[byte_start..byte_start + elem_size];
        let value = decode_prim_value(elem_type, slice);
        entries.push(EntryInfo {
            index: idx,
            key: None,
            value,
        });
    }

    Some(CollectionPage {
        entries,
        total_count: total,
        offset: clamped_offset,
        has_more: (clamped_offset + actual_limit) < total as usize,
    })
}

/// Reads the `elementData` Object[] and `size` int,
/// then paginates.
fn extract_array_list(
    hfile: &HprofFile,
    raw: &RawInstance,
    offset: usize,
    limit: usize,
) -> Option<CollectionPage> {
    let fields = decode_fields(
        raw,
        &hfile.index,
        hfile.header.id_size,
        hfile.records_bytes(),
    );

    let mut size: Option<u64> = None;
    let mut element_data_id: Option<u64> = None;

    for f in &fields {
        if (f.name == "size" || f.name == "elementCount")
            && let FieldValue::Int(v) = f.value
        {
            size = Some(v.max(0) as u64);
        }
        if f.name == "elementData"
            && let FieldValue::ObjectRef { id, .. } = f.value
        {
            element_data_id = Some(id);
        }
    }

    let total = size?;
    let arr_id = element_data_id?;
    let meta = hfile.find_object_array_meta(arr_id)?;
    // Clamp total to the actual backing array capacity
    // to guard against corrupt dumps where size >
    // num_elements.
    let effective_total = total.min(meta.num_elements as u64);
    paginate_object_array(&meta, effective_total, offset, limit, hfile)
}

/// Walks `table` Node[] for HashMap / ConcurrentHashMap.
///
/// When a `SkipIndex` is provided and `offset > 0`,
/// resumes from the nearest checkpoint instead of walking
/// from slot 0. Records new checkpoints during traversal.
fn extract_hash_map(
    hfile: &HprofFile,
    raw: &RawInstance,
    offset: usize,
    limit: usize,
    concurrent: bool,
    mut skip_index: Option<&mut SkipIndex>,
) -> Option<CollectionPage> {
    let fields = decode_fields(
        raw,
        &hfile.index,
        hfile.header.id_size,
        hfile.records_bytes(),
    );

    let mut size: Option<u64> = None;
    let mut table_id: Option<u64> = None;
    let val_field_name = if concurrent { "val" } else { "value" };

    for f in &fields {
        if matches!(f.name.as_str(), "size" | "elementCount" | "count")
            && let FieldValue::Int(v) = f.value
        {
            size = Some(v.max(0) as u64);
        }
        if f.name == "table"
            && let FieldValue::ObjectRef { id, .. } = f.value
        {
            table_id = Some(id);
        }
    }

    let total = size?;
    let table_arr_id = table_id?;
    let (_class_id, table_elements) = hfile.find_object_array(table_arr_id)?;

    // Determine start position via skip-index checkpoint
    let mut checkpoint_index: usize = 0;
    let mut start_slot: usize = 0;
    let mut resume_node_id: Option<u64> = None;

    if let Some(si) = skip_index.as_deref()
        && offset > 0
        && let Some((ci, cp)) = si.nearest_before(offset)
    {
        match cp {
            SkipCheckpoint::HashMapSlot {
                slot_index,
                node_id,
            } => {
                if *slot_index < table_elements.len() {
                    checkpoint_index = ci;
                    start_slot = *slot_index;
                    resume_node_id = Some(*node_id);
                }
            }
            _ => unreachable!(
                "unexpected checkpoint variant \
                 in extract_hash_map"
            ),
        }
    }

    // Batch pre-resolve uncached table head node IDs
    // (Story 11.6 Task 2.1)
    let uncached_heads: Vec<u64> = table_elements[start_slot..]
        .iter()
        .copied()
        .filter(|&id| id != 0 && !hfile.index.instance_offsets.contains(&id))
        .collect();
    if uncached_heads.len() >= BATCH_THRESHOLD {
        let batch = hfile.batch_find_instances(&uncached_heads);
        hfile.index.instance_offsets.insert_batch(&batch.offsets);
    }

    let adjusted_offset = offset - checkpoint_index;
    let target_count = adjusted_offset + limit;
    let mut all_entries: Vec<(FieldValue, FieldValue)> = Vec::new();
    let mut visited = std::collections::HashSet::new();

    for (slot_rel, &slot_id) in table_elements[start_slot..].iter().enumerate() {
        let current_slot = start_slot + slot_rel;
        if slot_id == 0 {
            continue;
        }
        let mut node_id = slot_id;

        // On the first slot of a checkpoint resume, walk
        // the chain to find the checkpoint node
        if let Some(target) = resume_node_id.take() {
            // Walk chain until we find the target node
            while node_id != 0 && node_id != target {
                if !visited.insert(node_id) {
                    break;
                }
                if let Some(nr) = Engine::read_instance_public(hfile, node_id) {
                    let nf = decode_fields(
                        &nr,
                        &hfile.index,
                        hfile.header.id_size,
                        hfile.records_bytes(),
                    );
                    node_id = nf
                        .iter()
                        .find(|f| f.name == "next")
                        .and_then(|f| match f.value {
                            FieldValue::ObjectRef { id, .. } => Some(id),
                            _ => None,
                        })
                        .unwrap_or(0);
                } else {
                    node_id = 0;
                }
            }
            if node_id == 0 {
                // Target node not found — fallback:
                // reset and walk from beginning
                return extract_hash_map_full(
                    hfile,
                    &table_elements,
                    total,
                    offset,
                    limit,
                    val_field_name,
                    skip_index,
                );
            }
            // node_id is now the checkpoint node; proceed
            // to collect from here (including this node)
        }

        while node_id != 0 {
            if !visited.insert(node_id) {
                break; // cycle guard
            }
            if let Some(node_raw) = Engine::read_instance_public(hfile, node_id) {
                // Record checkpoint before pushing entry
                if let Some(si) = skip_index.as_deref_mut() {
                    si.record(
                        checkpoint_index + all_entries.len(),
                        SkipCheckpoint::HashMapSlot {
                            slot_index: current_slot,
                            node_id,
                        },
                    );
                }

                let node_fields = decode_fields(
                    &node_raw,
                    &hfile.index,
                    hfile.header.id_size,
                    hfile.records_bytes(),
                );
                let key = node_fields
                    .iter()
                    .find(|f| f.name == "key")
                    .map(|f| f.value.clone())
                    .unwrap_or(FieldValue::Null);
                let value = node_fields
                    .iter()
                    .find(|f| f.name == val_field_name)
                    .map(|f| f.value.clone())
                    .unwrap_or(FieldValue::Null);
                let next_id = node_fields
                    .iter()
                    .find(|f| f.name == "next")
                    .and_then(|f| match f.value {
                        FieldValue::ObjectRef { id, .. } => Some(id),
                        _ => None,
                    })
                    .unwrap_or(0);
                all_entries.push((key, value));
                node_id = next_id;
            } else {
                break;
            }
            if all_entries.len() >= target_count {
                break;
            }
        }
        if all_entries.len() >= target_count {
            break;
        }
    }

    if all_entries.len() < target_count
        && let Some(si) = skip_index
    {
        si.mark_complete();
    }

    // Batch pre-resolve uncached key/value object IDs
    // for the visible page window (Story 11.6 Task 3.1)
    let kv_start = adjusted_offset;
    let kv_end = (adjusted_offset + limit).min(all_entries.len());
    if kv_start < kv_end {
        let obj_ids: Vec<u64> = all_entries[kv_start..kv_end]
            .iter()
            .flat_map(|(k, v)| [k, v])
            .filter_map(|fv| match fv {
                FieldValue::ObjectRef { id, .. } if *id != 0 => Some(*id),
                _ => None,
            })
            .filter(|id| !hfile.index.instance_offsets.contains(id))
            .collect();
        if !obj_ids.is_empty() {
            let batch = hfile.batch_find_instances(&obj_ids);
            hfile.index.instance_offsets.insert_batch(&batch.offsets);
        }
    }

    paginate_kv_entries(
        &all_entries,
        total,
        adjusted_offset,
        limit,
        checkpoint_index,
    )
}

/// Full walk fallback for HashMap when checkpoint resume
/// fails.
fn extract_hash_map_full(
    hfile: &HprofFile,
    table_elements: &[u64],
    total: u64,
    offset: usize,
    limit: usize,
    val_field_name: &str,
    mut skip_index: Option<&mut SkipIndex>,
) -> Option<CollectionPage> {
    // Batch pre-resolve uncached table head node IDs
    // (Story 11.6 Task 2.1)
    let uncached_heads: Vec<u64> = table_elements
        .iter()
        .copied()
        .filter(|&id| id != 0 && !hfile.index.instance_offsets.contains(&id))
        .collect();
    if uncached_heads.len() >= BATCH_THRESHOLD {
        let batch = hfile.batch_find_instances(&uncached_heads);
        hfile.index.instance_offsets.insert_batch(&batch.offsets);
    }

    let target_count = offset + limit;
    let mut all_entries: Vec<(FieldValue, FieldValue)> = Vec::new();
    let mut visited = std::collections::HashSet::new();

    for (current_slot, &slot_id) in table_elements.iter().enumerate() {
        if slot_id == 0 {
            continue;
        }
        let mut node_id = slot_id;
        while node_id != 0 {
            if !visited.insert(node_id) {
                break;
            }
            if let Some(node_raw) = Engine::read_instance_public(hfile, node_id) {
                if let Some(si) = skip_index.as_deref_mut() {
                    si.record(
                        all_entries.len(),
                        SkipCheckpoint::HashMapSlot {
                            slot_index: current_slot,
                            node_id,
                        },
                    );
                }
                let node_fields = decode_fields(
                    &node_raw,
                    &hfile.index,
                    hfile.header.id_size,
                    hfile.records_bytes(),
                );
                let key = node_fields
                    .iter()
                    .find(|f| f.name == "key")
                    .map(|f| f.value.clone())
                    .unwrap_or(FieldValue::Null);
                let value = node_fields
                    .iter()
                    .find(|f| f.name == val_field_name)
                    .map(|f| f.value.clone())
                    .unwrap_or(FieldValue::Null);
                let next_id = node_fields
                    .iter()
                    .find(|f| f.name == "next")
                    .and_then(|f| match f.value {
                        FieldValue::ObjectRef { id, .. } => Some(id),
                        _ => None,
                    })
                    .unwrap_or(0);
                all_entries.push((key, value));
                node_id = next_id;
            } else {
                break;
            }
            if all_entries.len() >= target_count {
                break;
            }
        }
        if all_entries.len() >= target_count {
            break;
        }
    }

    if all_entries.len() < target_count
        && let Some(si) = skip_index
    {
        si.mark_complete();
    }

    // Batch pre-resolve uncached key/value object IDs
    // for the visible page window (Story 11.6 Task 3.1)
    let page_start = offset;
    let page_end = (offset + limit).min(all_entries.len());
    if page_start < page_end {
        let obj_ids: Vec<u64> = all_entries[page_start..page_end]
            .iter()
            .flat_map(|(k, v)| [k, v])
            .filter_map(|fv| match fv {
                FieldValue::ObjectRef { id, .. } if *id != 0 => Some(*id),
                _ => None,
            })
            .filter(|id| !hfile.index.instance_offsets.contains(id))
            .collect();
        if !obj_ids.is_empty() {
            let batch = hfile.batch_find_instances(&obj_ids);
            hfile.index.instance_offsets.insert_batch(&batch.offsets);
        }
    }

    paginate_kv_entries(&all_entries, total, offset, limit, 0)
}

/// HashSet delegates to its backing HashMap `map` field.
fn extract_hash_set(
    hfile: &HprofFile,
    raw: &RawInstance,
    offset: usize,
    limit: usize,
    skip_index: Option<&mut SkipIndex>,
) -> Option<CollectionPage> {
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
    })?;

    let map_raw = Engine::read_instance_public(hfile, map_id)?;
    let page = extract_hash_map(hfile, &map_raw, offset, limit, false, skip_index)?;

    // Convert map entries to set entries (keys only)
    let entries = page
        .entries
        .into_iter()
        .map(|e| EntryInfo {
            index: e.index,
            key: None,
            value: e.key.unwrap_or(FieldValue::Null),
        })
        .collect();

    Some(CollectionPage {
        entries,
        total_count: page.total_count,
        offset: page.offset,
        has_more: page.has_more,
    })
}

/// Walks `first` → `next` chain for LinkedList.
///
/// When a `SkipIndex` is provided and `offset > 0`,
/// resumes from the nearest checkpoint instead of
/// walking from the head. Records new checkpoints
/// during traversal for future page requests.
fn extract_linked_list(
    hfile: &HprofFile,
    raw: &RawInstance,
    offset: usize,
    limit: usize,
    mut skip_index: Option<&mut SkipIndex>,
) -> Option<CollectionPage> {
    let fields = decode_fields(
        raw,
        &hfile.index,
        hfile.header.id_size,
        hfile.records_bytes(),
    );

    let mut size: Option<u64> = None;
    let mut first_id: Option<u64> = None;

    for f in &fields {
        if f.name == "size"
            && let FieldValue::Int(v) = f.value
        {
            size = Some(v.max(0) as u64);
        }
        if f.name == "first"
            && let FieldValue::ObjectRef { id, .. } = f.value
        {
            first_id = Some(id);
        }
    }

    let total = size?;
    let mut node_id = first_id.unwrap_or(0);

    // Determine start position via skip-index checkpoint
    let mut checkpoint_index: usize = 0;
    if let Some(si) = skip_index.as_deref()
        && offset > 0
        && let Some((ci, cp)) = si.nearest_before(offset)
    {
        match cp {
            SkipCheckpoint::LinkedListNode { node_id: cp_node } => {
                node_id = *cp_node;
                checkpoint_index = ci;
            }
            _ => unreachable!(
                "unexpected checkpoint variant \
                 in extract_linked_list"
            ),
        }
    }

    let adjusted_offset = offset - checkpoint_index;
    let target_count = adjusted_offset + limit;
    let max_iter = (total as usize).saturating_sub(checkpoint_index);
    let mut items: Vec<FieldValue> = Vec::new();
    let mut visited = std::collections::HashSet::new();
    let mut iterations: usize = 0;

    while node_id != 0 && items.len() < target_count {
        if iterations >= max_iter {
            break; // max iterations guard (Task 3.4)
        }
        if !visited.insert(node_id) {
            break; // cycle guard
        }
        if let Some(node_raw) = Engine::read_instance_public(hfile, node_id) {
            // Record checkpoint before pushing item
            if let Some(si) = skip_index.as_deref_mut() {
                si.record(
                    checkpoint_index + items.len(),
                    SkipCheckpoint::LinkedListNode { node_id },
                );
            }

            let node_fields = decode_fields(
                &node_raw,
                &hfile.index,
                hfile.header.id_size,
                hfile.records_bytes(),
            );
            let item = node_fields
                .iter()
                .find(|f| f.name == "item")
                .map(|f| f.value.clone())
                .unwrap_or(FieldValue::Null);
            items.push(item);
            node_id = node_fields
                .iter()
                .find(|f| f.name == "next")
                .and_then(|f| match f.value {
                    FieldValue::ObjectRef { id, .. } => Some(id),
                    _ => None,
                })
                .unwrap_or(0);
        } else {
            break;
        }
        iterations += 1;
    }

    // Mark complete if we reached the end of the chain
    if node_id == 0
        && let Some(si) = skip_index
    {
        si.mark_complete();
    }

    // Batch pre-resolve uncached item object IDs for
    // the visible page window (Story 11.6 Task 3.2)
    let clamped_offset = (adjusted_offset as u64).min(total) as usize;
    let ll_end = (clamped_offset + limit).min(items.len());
    if clamped_offset < ll_end {
        let obj_ids: Vec<u64> = items[clamped_offset..ll_end]
            .iter()
            .filter_map(|fv| match fv {
                FieldValue::ObjectRef { id, .. } if *id != 0 => Some(*id),
                _ => None,
            })
            .filter(|id| !hfile.index.instance_offsets.contains(id))
            .collect();
        if !obj_ids.is_empty() {
            let batch = hfile.batch_find_instances(&obj_ids);
            hfile.index.instance_offsets.insert_batch(&batch.offsets);
        }
    }

    let page_items: Vec<EntryInfo> = items
        .into_iter()
        .skip(clamped_offset)
        .take(limit)
        .enumerate()
        .map(|(i, value)| EntryInfo {
            index: checkpoint_index + clamped_offset + i,
            key: None,
            value,
        })
        .collect();
    let actual_end = checkpoint_index + clamped_offset + page_items.len();

    Some(CollectionPage {
        entries: page_items,
        total_count: total,
        offset: checkpoint_index + clamped_offset,
        has_more: (actual_end as u64) < total,
    })
}

// --- Helpers ---

/// Paginates an object array via O(1) positional reads.
///
/// `total` is the logical element count. In
/// `try_object_array` it equals `meta.num_elements`; in
/// `extract_array_list` it comes from the ArrayList `size`
/// field and may be less than `meta.num_elements`.
///
/// Uses batch pre-resolution (Story 11.2) to cache
/// instance offsets before resolving individual elements,
/// avoiding per-element segment scans.
fn paginate_object_array(
    meta: &ObjectArrayMeta,
    total: u64,
    offset: usize,
    limit: usize,
    hfile: &HprofFile,
) -> Option<CollectionPage> {
    let clamped = (offset as u64).min(total) as usize;
    let remaining = (total - clamped as u64) as usize;
    let actual = limit.min(remaining);

    // Step 1: Read all page IDs once via O(1) positional access.
    // id=0 represents a null reference; stored at position i for
    // index preservation. Failed reads (should not occur within
    // bounds) also map to 0.
    let element_ids: Vec<u64> = (0..actual)
        .map(|i| {
            let idx = (clamped + i) as u32;
            hfile
                .read_object_array_element(meta, idx)
                .unwrap_or_else(|| {
                    dbg_log!(
                        "read_object_array_element \
                     returned None at idx {}",
                        idx
                    );
                    0
                })
        })
        .collect();

    // Step 2: Batch pre-resolve uncached non-null instance IDs.
    let uncached: Vec<u64> = element_ids
        .iter()
        .copied()
        .filter(|&id| id != 0 && !hfile.index.instance_offsets.contains(&id))
        .collect();

    if !uncached.is_empty() {
        let batch = hfile.batch_find_instances(&uncached);
        hfile.index.instance_offsets.insert_batch(&batch.offsets);
    }

    // Step 3: Resolve all elements (batch-found IDs now
    // hit O(1) offset path; no second mmap read needed).
    let entries: Vec<EntryInfo> = element_ids
        .into_iter()
        .enumerate()
        .map(|(i, id)| EntryInfo {
            index: clamped + i,
            key: None,
            value: id_to_field_value(id, hfile),
        })
        .collect();

    Some(CollectionPage {
        entries,
        total_count: total,
        offset: clamped,
        has_more: (clamped + actual) < total as usize,
    })
}

/// Paginates key-value pairs collected from hash maps.
///
/// `base_index` is the logical index of the first element in `all`
/// (non-zero when resuming from a skip-index checkpoint).
fn paginate_kv_entries(
    all: &[(FieldValue, FieldValue)],
    total: u64,
    offset: usize,
    limit: usize,
    base_index: usize,
) -> Option<CollectionPage> {
    let clamped_offset = offset.min(all.len());
    let remaining = all.len() - clamped_offset;
    let actual_limit = limit.min(remaining);

    let entries: Vec<EntryInfo> = all[clamped_offset..clamped_offset + actual_limit]
        .iter()
        .enumerate()
        .map(|(i, (k, v))| EntryInfo {
            index: base_index + clamped_offset + i,
            key: Some(k.clone()),
            value: v.clone(),
        })
        .collect();
    let actual_end = base_index + clamped_offset + entries.len();

    Some(CollectionPage {
        entries,
        total_count: total,
        offset: base_index + clamped_offset,
        has_more: (actual_end as u64) < total,
    })
}

/// Converts an object ID to a `FieldValue`.
fn id_to_field_value(id: u64, hfile: &HprofFile) -> FieldValue {
    if id == 0 {
        return FieldValue::Null;
    }
    // Try instance first (covers all regular objects and collections).
    if let Some(raw) = Engine::read_instance_public(hfile, id) {
        let class_name = hfile
            .index
            .class_names_by_id
            .get(&raw.class_object_id)
            .cloned()
            .unwrap_or_else(|| "Object".to_string());
        dbg_log!("id_to_field_value(0x{:X}): class={:?}", id, class_name);
        let entry_count = crate::engine_impl::collection_entry_count(
            &raw,
            &hfile.index,
            hfile.header.id_size,
            hfile.records_bytes(),
        );
        let inline_value = crate::engine_impl::resolve_inline_value(&class_name, hfile, id);
        return FieldValue::ObjectRef {
            id,
            class_name,
            entry_count,
            inline_value,
        };
    }
    // Try Object[] array (metadata only — no element read).
    if let Some(meta) = hfile.find_object_array_meta(id) {
        return FieldValue::ObjectRef {
            id,
            class_name: "Object[]".to_string(),
            entry_count: Some(meta.num_elements as u64),
            inline_value: None,
        };
    }
    // Try primitive array.
    if let Some((etype, bytes)) = hfile.find_prim_array(id) {
        let type_name = crate::engine_impl::prim_array_type_name(etype);
        let esz = crate::engine_impl::field_byte_size(etype, hfile.header.id_size);
        let cnt = if esz > 0 { bytes.len() / esz } else { 0 };
        return FieldValue::ObjectRef {
            id,
            class_name: format!("{type_name}[]"),
            entry_count: Some(cnt as u64),
            inline_value: None,
        };
    }
    // Unknown ID.
    FieldValue::ObjectRef {
        id,
        class_name: "Object".to_string(),
        entry_count: None,
        inline_value: None,
    }
}

/// Returns element byte size for a primitive array type.
fn prim_elem_size(elem_type: u8) -> Option<usize> {
    match elem_type {
        4 => Some(1),  // boolean
        5 => Some(2),  // char
        6 => Some(4),  // float
        7 => Some(8),  // double
        8 => Some(1),  // byte
        9 => Some(2),  // short
        10 => Some(4), // int
        11 => Some(8), // long
        _ => None,
    }
}

/// Decodes a single primitive value from raw bytes.
fn decode_prim_value(elem_type: u8, bytes: &[u8]) -> FieldValue {
    use byteorder::{BigEndian, ReadBytesExt};
    use std::io::Cursor;
    let mut c = Cursor::new(bytes);
    match elem_type {
        4 => FieldValue::Bool(c.read_u8().unwrap_or(0) != 0),
        5 => {
            let code = c.read_u16::<BigEndian>().unwrap_or(0);
            let ch = char::from_u32(code as u32).unwrap_or(char::REPLACEMENT_CHARACTER);
            FieldValue::Char(ch)
        }
        6 => FieldValue::Float(c.read_f32::<BigEndian>().unwrap_or(0.0)),
        7 => FieldValue::Double(c.read_f64::<BigEndian>().unwrap_or(0.0)),
        8 => FieldValue::Byte(c.read_i8().unwrap_or(0)),
        9 => FieldValue::Short(c.read_i16::<BigEndian>().unwrap_or(0)),
        10 => FieldValue::Int(c.read_i32::<BigEndian>().unwrap_or(0)),
        11 => FieldValue::Long(c.read_i64::<BigEndian>().unwrap_or(0)),
        _ => FieldValue::Null,
    }
}

#[cfg(test)]
mod tests;
