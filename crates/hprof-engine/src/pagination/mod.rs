//! Collection pagination: type dispatch and extractors.
//!
//! Resolves a page of entries from a Java collection
//! object by identifying its concrete type and delegating
//! to the appropriate extractor.

use hprof_parser::{HprofFile, RawInstance};

use crate::engine::{CollectionPage, EntryInfo, FieldValue};
use crate::engine_impl::Engine;
use crate::resolver::decode_fields;

/// Returns a page from the collection identified by
/// `collection_id`, or `None` if unsupported/not found.
pub(crate) fn get_page(
    hfile: &HprofFile,
    collection_id: u64,
    offset: usize,
    limit: usize,
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

    match_extractor(short, hfile, &raw, offset, limit)
}

fn match_extractor(
    short_name: &str,
    hfile: &HprofFile,
    raw: &RawInstance,
    offset: usize,
    limit: usize,
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
        return extract_hash_map(hfile, raw, offset, limit, false);
    }
    if short_name.eq_ignore_ascii_case("ConcurrentHashMap") {
        return extract_hash_map(hfile, raw, offset, limit, true);
    }
    if short_name.eq_ignore_ascii_case("HashSet")
        || short_name.eq_ignore_ascii_case("LinkedHashSet")
    {
        return extract_hash_set(hfile, raw, offset, limit);
    }
    if short_name.eq_ignore_ascii_case("LinkedList") {
        return extract_linked_list(hfile, raw, offset, limit);
    }
    // Unsupported collection type
    None
}

/// Extracts a page from an `ObjectArrayDump` directly.
fn try_object_array(
    hfile: &HprofFile,
    array_id: u64,
    offset: usize,
    limit: usize,
) -> Option<CollectionPage> {
    let (_class_id, elements) = hfile.find_object_array(array_id)?;
    let total = elements.len() as u64;
    paginate_id_slice(&elements, total, offset, limit, hfile)
}

/// Extracts a page from a `PrimArrayDump` directly.
fn try_prim_array(
    hfile: &HprofFile,
    array_id: u64,
    offset: usize,
    limit: usize,
) -> Option<CollectionPage> {
    let (elem_type, bytes) = hfile.find_prim_array(array_id)?;
    let elem_size = prim_elem_size(elem_type)?;
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
    let (_class_id, elements) = hfile.find_object_array(arr_id)?;

    // Pass `total` (= size field) as the logical bound — paginate_id_slice
    // limits entries to [0, total), so capacity-padding slots are ignored.
    paginate_id_slice(&elements, total, offset, limit, hfile)
}

/// Walks `table` Node[] for HashMap / ConcurrentHashMap.
fn extract_hash_map(
    hfile: &HprofFile,
    raw: &RawInstance,
    offset: usize,
    limit: usize,
    concurrent: bool,
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

    // Walk non-null slots, following `next` chains
    let mut all_entries: Vec<(FieldValue, FieldValue)> = Vec::new();
    let target_count = offset + limit;
    let mut visited = std::collections::HashSet::new();

    for &slot_id in &table_elements {
        if slot_id == 0 {
            continue;
        }
        let mut node_id = slot_id;
        while node_id != 0 {
            if !visited.insert(node_id) {
                break; // cycle guard
            }
            if let Some(node_raw) = Engine::read_instance_public(hfile, node_id) {
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
            // Early exit: no need to walk past
            // offset + limit
            if all_entries.len() >= target_count {
                break;
            }
        }
        if all_entries.len() >= target_count {
            break;
        }
    }

    paginate_kv_entries(&all_entries, total, offset, limit)
}

/// HashSet delegates to its backing HashMap `map` field.
fn extract_hash_set(
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

    let map_id = fields.iter().find_map(|f| {
        if f.name == "map"
            && let FieldValue::ObjectRef { id, .. } = f.value
        {
            return Some(id);
        }
        None
    })?;

    let map_raw = Engine::read_instance_public(hfile, map_id)?;
    let page = extract_hash_map(hfile, &map_raw, offset, limit, false)?;

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
fn extract_linked_list(
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
    let target_count = offset + limit;
    let mut items: Vec<FieldValue> = Vec::new();
    let mut visited = std::collections::HashSet::new();

    while node_id != 0 && items.len() < target_count {
        if !visited.insert(node_id) {
            break; // cycle guard
        }
        if let Some(node_raw) = Engine::read_instance_public(hfile, node_id) {
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
    }

    let clamped_offset = (offset as u64).min(total) as usize;
    let page_items: Vec<EntryInfo> = items
        .into_iter()
        .skip(clamped_offset)
        .take(limit)
        .enumerate()
        .map(|(i, value)| EntryInfo {
            index: clamped_offset + i,
            key: None,
            value,
        })
        .collect();
    let actual_end = clamped_offset + page_items.len();

    Some(CollectionPage {
        entries: page_items,
        total_count: total,
        offset: clamped_offset,
        has_more: (actual_end as u64) < total,
    })
}

// --- Helpers ---

/// Paginates a slice of object IDs, resolving each
/// to a `FieldValue`.
fn paginate_id_slice(
    ids: &[u64],
    total: u64,
    offset: usize,
    limit: usize,
    hfile: &HprofFile,
) -> Option<CollectionPage> {
    let clamped_offset = (offset as u64).min(total) as usize;
    let remaining = (total - clamped_offset as u64) as usize;
    let actual_limit = limit.min(remaining);

    let mut entries = Vec::with_capacity(actual_limit);
    for i in 0..actual_limit {
        let idx = clamped_offset + i;
        let value = if idx < ids.len() {
            id_to_field_value(ids[idx], hfile)
        } else {
            FieldValue::Null
        };
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

/// Paginates key-value pairs collected from hash maps.
fn paginate_kv_entries(
    all: &[(FieldValue, FieldValue)],
    total: u64,
    offset: usize,
    limit: usize,
) -> Option<CollectionPage> {
    let clamped_offset = offset.min(all.len());
    let remaining = all.len() - clamped_offset;
    let actual_limit = limit.min(remaining);

    let entries: Vec<EntryInfo> = all[clamped_offset..clamped_offset + actual_limit]
        .iter()
        .enumerate()
        .map(|(i, (k, v))| EntryInfo {
            index: clamped_offset + i,
            key: Some(k.clone()),
            value: v.clone(),
        })
        .collect();
    let actual_end = clamped_offset + entries.len();

    Some(CollectionPage {
        entries,
        total_count: total,
        offset: clamped_offset,
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
    // Try Object[] array.
    if let Some((_cid, elems)) = hfile.find_object_array(id) {
        return FieldValue::ObjectRef {
            id,
            class_name: "Object[]".to_string(),
            entry_count: Some(elems.len() as u64),
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
