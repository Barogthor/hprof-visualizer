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
            collection_id, offset, limit, page.entries.len()
        );
        return Some(page);
    }
    if let Some(page) = try_prim_array(hfile, collection_id, offset, limit) {
        dbg_log!(
            "get_page(0x{:X}, {}, {}): prim[] → {} entries",
            collection_id, offset, limit, page.entries.len()
        );
        return Some(page);
    }

    // Resolve as instance and dispatch by class name
    let raw = match Engine::read_instance_public(hfile, collection_id) {
        Some(r) => r,
        None => {
            dbg_log!(
                "get_page(0x{:X}): instance not found",
                collection_id
            );
            return None;
        }
    };
    let class_name = match hfile
        .index
        .class_names_by_id
        .get(&raw.class_object_id)
    {
        Some(cn) => cn,
        None => {
            dbg_log!(
                "get_page(0x{:X}): class_name missing \
                 for class_obj=0x{:X}",
                collection_id, raw.class_object_id
            );
            return None;
        }
    };
    let short = class_name
        .rsplit('.')
        .next()
        .unwrap_or(class_name.as_str());

    let result =
        match_extractor(short, hfile, &raw, offset, limit);
    match &result {
        Some(page) => dbg_log!(
            "get_page(0x{:X}): {} ({}) → {} entries",
            collection_id, class_name, short, page.entries.len()
        ),
        None => dbg_log!(
            "get_page(0x{:X}): {} ({}) → extractor None",
            collection_id, class_name, short
        ),
    }
    result
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
    let class_name = Engine::read_instance_public(hfile, id)
        .and_then(|raw| {
            hfile
                .index
                .class_names_by_id
                .get(&raw.class_object_id)
                .cloned()
        });
    dbg_log!(
        "id_to_field_value(0x{:X}): class={:?}",
        id, class_name
    );
    let class_name = class_name
        .unwrap_or_else(|| "Object".to_string());
    let inline_value =
        crate::engine_impl::resolve_inline_value(
            &class_name, hfile, id,
        );
    FieldValue::ObjectRef {
        id,
        class_name,
        entry_count: None,
        inline_value,
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
mod tests {
    use super::*;
    use hprof_parser::HprofTestBuilder;
    use std::io::Write;

    fn hfile_from_bytes(bytes: &[u8]) -> HprofFile {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(bytes).unwrap();
        tmp.flush().unwrap();
        HprofFile::from_path(tmp.path()).unwrap()
    }

    // 5.1: ObjectArrayDump pagination
    #[test]
    fn object_array_first_page() {
        let elements: Vec<u64> = (1..=5).collect();
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_object_array(0xA, 0, 100, &elements)
            .build();
        let hfile = hfile_from_bytes(&bytes);
        let page = get_page(&hfile, 0xA, 0, 3).unwrap();
        assert_eq!(page.total_count, 5);
        assert_eq!(page.offset, 0);
        assert_eq!(page.entries.len(), 3);
        assert!(page.has_more);
        assert_eq!(page.entries[0].index, 0);
        assert_eq!(page.entries[2].index, 2);
    }

    #[test]
    fn object_array_middle_page() {
        let elements: Vec<u64> = (1..=10).collect();
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_object_array(0xA, 0, 100, &elements)
            .build();
        let hfile = hfile_from_bytes(&bytes);
        let page = get_page(&hfile, 0xA, 3, 3).unwrap();
        assert_eq!(page.total_count, 10);
        assert_eq!(page.offset, 3);
        assert_eq!(page.entries.len(), 3);
        assert!(page.has_more);
        assert_eq!(page.entries[0].index, 3);
    }

    #[test]
    fn object_array_last_partial_page() {
        let elements: Vec<u64> = (1..=5).collect();
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_object_array(0xA, 0, 100, &elements)
            .build();
        let hfile = hfile_from_bytes(&bytes);
        let page = get_page(&hfile, 0xA, 3, 1000).unwrap();
        assert_eq!(page.total_count, 5);
        assert_eq!(page.offset, 3);
        assert_eq!(page.entries.len(), 2);
        assert!(!page.has_more);
    }

    // 5.2: Small array returns all entries
    #[test]
    fn small_array_returns_all() {
        let elements: Vec<u64> = (1..=3).collect();
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_object_array(0xA, 0, 100, &elements)
            .build();
        let hfile = hfile_from_bytes(&bytes);
        let page = get_page(&hfile, 0xA, 0, 1000).unwrap();
        assert_eq!(page.total_count, 3);
        assert_eq!(page.entries.len(), 3);
        assert!(!page.has_more);
    }

    // 5.3: Offset beyond bounds returns empty
    #[test]
    fn offset_beyond_bounds_returns_empty() {
        let elements: Vec<u64> = (1..=5).collect();
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_object_array(0xA, 0, 100, &elements)
            .build();
        let hfile = hfile_from_bytes(&bytes);
        let page = get_page(&hfile, 0xA, 100, 10).unwrap();
        assert_eq!(page.entries.len(), 0);
        assert!(!page.has_more);
    }

    // 5.4: HashMap with null slots skipped
    #[test]
    fn hashmap_null_slots_skipped() {
        // Build a HashMap with table containing
        // null (0) and real Node entries.
        //
        // HashMap class (id=1000):
        //   fields: size(Int), table(ObjectRef)
        // Node class (id=2000):
        //   fields: key(ObjectRef), value(ObjectRef),
        //           next(ObjectRef)
        //
        // Table array has [0, node1, 0, node2]
        // node1: key=0x10, value=0x20, next=0
        // node2: key=0x30, value=0x40, next=0
        let id_size: u32 = 8;

        // Strings
        let str_size = 10u64;
        let str_table = 11u64;
        let str_key = 12u64;
        let str_value = 13u64;
        let str_next = 14u64;
        let str_classname = 15u64;
        let str_nodename = 16u64;

        // HashMap instance data: size(Int=2) +
        // table(ObjectRef=0x500)
        let mut hm_data = Vec::new();
        hm_data.extend_from_slice(&2i32.to_be_bytes()); // size
        hm_data.extend_from_slice(&0x500u64.to_be_bytes()); // table

        // Node1 data: key(0x10) + value(0x20) +
        // next(0)
        let mut n1_data = Vec::new();
        n1_data.extend_from_slice(&0x10u64.to_be_bytes());
        n1_data.extend_from_slice(&0x20u64.to_be_bytes());
        n1_data.extend_from_slice(&0u64.to_be_bytes());

        // Node2 data: key(0x30) + value(0x40) +
        // next(0)
        let mut n2_data = Vec::new();
        n2_data.extend_from_slice(&0x30u64.to_be_bytes());
        n2_data.extend_from_slice(&0x40u64.to_be_bytes());
        n2_data.extend_from_slice(&0u64.to_be_bytes());

        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", id_size)
            .add_string(str_size, "size")
            .add_string(str_table, "table")
            .add_string(str_key, "key")
            .add_string(str_value, "value")
            .add_string(str_next, "next")
            .add_string(str_classname, "java/util/HashMap")
            .add_string(str_nodename, "java/util/HashMap$Node")
            .add_class(1, 1000, 0, str_classname)
            .add_class(2, 2000, 0, str_nodename)
            // HashMap class dump: size(Int=10),
            // table(Obj=2)
            .add_class_dump(
                1000,
                0,
                (4 + id_size) as u32,
                &[(str_size, 10), (str_table, 2)],
            )
            // Node class dump: key(Obj=2),
            // value(Obj=2), next(Obj=2)
            .add_class_dump(
                2000,
                0,
                (id_size * 3) as u32,
                &[(str_key, 2), (str_value, 2), (str_next, 2)],
            )
            // HashMap instance
            .add_instance(0x100, 0, 1000, &hm_data)
            // Table: [0, node1(0x200), 0, node2(0x300)]
            .add_object_array(0x500, 0, 2000, &[0, 0x200, 0, 0x300])
            // Node instances
            .add_instance(0x200, 0, 2000, &n1_data)
            .add_instance(0x300, 0, 2000, &n2_data)
            .build();

        let hfile = hfile_from_bytes(&bytes);
        let page = get_page(&hfile, 0x100, 0, 1000).unwrap();
        assert_eq!(page.total_count, 2);
        assert_eq!(page.entries.len(), 2);
        // Keys should be ObjectRef
        assert!(page.entries[0].key.is_some());
        assert!(page.entries[1].key.is_some());
    }

    // 5.5: ArrayList uses size not array capacity
    #[test]
    fn arraylist_uses_size_not_capacity() {
        // ArrayList with size=2 but elementData
        // capacity=4
        let id_size: u32 = 8;
        let str_size = 10u64;
        let str_ed = 11u64;
        let str_cn = 12u64;

        // Instance: size(Int=2) + elementData(Obj=0x500)
        let mut data = Vec::new();
        data.extend_from_slice(&2i32.to_be_bytes());
        data.extend_from_slice(&0x500u64.to_be_bytes());

        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", id_size)
            .add_string(str_size, "size")
            .add_string(str_ed, "elementData")
            .add_string(str_cn, "java/util/ArrayList")
            .add_class(1, 1000, 0, str_cn)
            .add_class_dump(
                1000,
                0,
                (4 + id_size) as u32,
                &[(str_size, 10), (str_ed, 2)],
            )
            .add_instance(0x100, 0, 1000, &data)
            // elementData: 4 slots but only 2 used
            .add_object_array(0x500, 0, 100, &[0x10, 0x20, 0, 0])
            .build();

        let hfile = hfile_from_bytes(&bytes);
        let page = get_page(&hfile, 0x100, 0, 1000).unwrap();
        // Must use size=2, not capacity=4
        assert_eq!(page.total_count, 2);
        assert_eq!(page.entries.len(), 2);
        assert!(!page.has_more);
    }

    // 5.6: Unsupported collection type returns None
    #[test]
    fn unsupported_collection_type_returns_none() {
        let id_size: u32 = 8;
        let str_cn = 10u64;
        let str_size = 11u64;

        let mut data = Vec::new();
        data.extend_from_slice(&5i32.to_be_bytes());

        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", id_size)
            .add_string(str_cn, "java/util/TreeMap")
            .add_string(str_size, "size")
            .add_class(1, 1000, 0, str_cn)
            .add_class_dump(1000, 0, 4, &[(str_size, 10)])
            .add_instance(0x100, 0, 1000, &data)
            .build();

        let hfile = hfile_from_bytes(&bytes);
        assert!(get_page(&hfile, 0x100, 0, 10).is_none());
    }

    // 5.7: Fully unknown type returns None
    #[test]
    fn unknown_type_returns_none() {
        let id_size: u32 = 8;
        let str_cn = 10u64;

        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", id_size)
            .add_string(str_cn, "com/example/MyClass")
            .add_class(1, 1000, 0, str_cn)
            .add_class_dump(1000, 0, 0, &[])
            .add_instance(0x100, 0, 1000, &[])
            .build();

        let hfile = hfile_from_bytes(&bytes);
        assert!(get_page(&hfile, 0x100, 0, 10).is_none());
    }

    // 5.8: has_more flag correctness
    #[test]
    fn has_more_flag_correct_at_boundary() {
        // 5 elements, request page of exactly 5
        let elements: Vec<u64> = (1..=5).collect();
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_object_array(0xA, 0, 100, &elements)
            .build();
        let hfile = hfile_from_bytes(&bytes);

        // Exact fit: has_more = false
        let page = get_page(&hfile, 0xA, 0, 5).unwrap();
        assert_eq!(page.entries.len(), 5);
        assert!(!page.has_more);

        // One less: has_more = true
        let page = get_page(&hfile, 0xA, 0, 4).unwrap();
        assert_eq!(page.entries.len(), 4);
        assert!(page.has_more);
    }

    // PrimArrayDump pagination
    #[test]
    fn prim_array_int_pagination() {
        let mut int_bytes = Vec::new();
        for i in 0i32..10 {
            int_bytes.extend_from_slice(&i.to_be_bytes());
        }
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_prim_array(0xB, 0, 10, 10, &int_bytes)
            .build();
        let hfile = hfile_from_bytes(&bytes);
        let page = get_page(&hfile, 0xB, 2, 3).unwrap();
        assert_eq!(page.total_count, 10);
        assert_eq!(page.offset, 2);
        assert_eq!(page.entries.len(), 3);
        assert!(page.has_more);
        assert_eq!(page.entries[0].value, FieldValue::Int(2));
        assert_eq!(page.entries[2].value, FieldValue::Int(4));
    }

    // Not found returns None
    #[test]
    fn nonexistent_id_returns_none() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_object_array(0xA, 0, 100, &[1])
            .build();
        let hfile = hfile_from_bytes(&bytes);
        assert!(get_page(&hfile, 0xDEAD, 0, 10).is_none());
    }

    // find_object_array test
    #[test]
    fn find_object_array_returns_elements() {
        let elements = vec![0x10u64, 0x20, 0x30];
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_object_array(0xA, 0, 100, &elements)
            .build();
        let hfile = hfile_from_bytes(&bytes);
        let (class_id, elems) = hfile.find_object_array(0xA).unwrap();
        assert_eq!(class_id, 100);
        assert_eq!(elems, elements);
    }

    #[test]
    fn find_object_array_unknown_returns_none() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_object_array(0xA, 0, 100, &[1])
            .build();
        let hfile = hfile_from_bytes(&bytes);
        assert!(hfile.find_object_array(0xBEEF).is_none());
    }

    // --- H1: LinkedList extractor tests ---

    /// Builds a 2-node LinkedList hprof fixture.
    ///
    /// Classes: LinkedList(id=1000), Node(id=2000).
    /// Instance 0x100: size=2, first=0x200, last=0x300
    /// Node 0x200: item=0x10, next=0x300, prev=0
    /// Node 0x300: item=0x20, next=0, prev=0x200
    fn build_linked_list_fixture() -> Vec<u8> {
        let id_size: u32 = 8;
        let str_size = 10u64;
        let str_first = 11u64;
        let str_last = 12u64;
        let str_item = 13u64;
        let str_next = 14u64;
        let str_prev = 15u64;
        let str_cn = 16u64;
        let str_node_cn = 17u64;

        let mut ll_data = Vec::new();
        ll_data.extend_from_slice(&2i32.to_be_bytes());
        ll_data.extend_from_slice(&0x200u64.to_be_bytes());
        ll_data.extend_from_slice(&0x300u64.to_be_bytes());

        let mut n1_data = Vec::new();
        n1_data.extend_from_slice(&0x10u64.to_be_bytes());
        n1_data.extend_from_slice(&0x300u64.to_be_bytes());
        n1_data.extend_from_slice(&0u64.to_be_bytes());

        let mut n2_data = Vec::new();
        n2_data.extend_from_slice(&0x20u64.to_be_bytes());
        n2_data.extend_from_slice(&0u64.to_be_bytes());
        n2_data.extend_from_slice(&0x200u64.to_be_bytes());

        HprofTestBuilder::new("JAVA PROFILE 1.0.2", id_size)
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
                (4 + id_size * 2) as u32,
                &[(str_size, 10), (str_first, 2), (str_last, 2)],
            )
            .add_class_dump(
                2000,
                0,
                (id_size * 3) as u32,
                &[(str_item, 2), (str_next, 2), (str_prev, 2)],
            )
            .add_instance(0x100, 0, 1000, &ll_data)
            .add_instance(0x200, 0, 2000, &n1_data)
            .add_instance(0x300, 0, 2000, &n2_data)
            .build()
    }

    #[test]
    fn linked_list_walks_chain() {
        let hfile = hfile_from_bytes(&build_linked_list_fixture());
        let page = get_page(&hfile, 0x100, 0, 1000).unwrap();
        assert_eq!(page.total_count, 2);
        assert_eq!(page.entries.len(), 2);
        assert!(!page.has_more);
        assert_eq!(page.entries[0].index, 0);
        assert_eq!(page.entries[1].index, 1);
        assert!(page.entries[0].key.is_none());
    }

    #[test]
    fn linked_list_offset_into_chain() {
        let hfile = hfile_from_bytes(&build_linked_list_fixture());
        let page = get_page(&hfile, 0x100, 1, 1000).unwrap();
        assert_eq!(page.total_count, 2);
        assert_eq!(page.entries.len(), 1);
        assert_eq!(page.entries[0].index, 1);
        assert!(!page.has_more);
    }

    // --- H2: ConcurrentHashMap uses "val" field name ---

    #[test]
    fn concurrent_hashmap_uses_val_field() {
        let id_size: u32 = 8;
        let str_size = 10u64;
        let str_table = 11u64;
        let str_key = 12u64;
        let str_val = 13u64; // "val" not "value"
        let str_next = 14u64;
        let str_cn = 15u64;
        let str_node_cn = 16u64;

        let mut chm_data = Vec::new();
        chm_data.extend_from_slice(&1i32.to_be_bytes()); // size = 1
        chm_data.extend_from_slice(&0x500u64.to_be_bytes()); // table

        let mut node_data = Vec::new();
        node_data.extend_from_slice(&0x10u64.to_be_bytes()); // key
        node_data.extend_from_slice(&0x20u64.to_be_bytes()); // val
        node_data.extend_from_slice(&0u64.to_be_bytes()); // next

        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", id_size)
            .add_string(str_size, "size")
            .add_string(str_table, "table")
            .add_string(str_key, "key")
            .add_string(str_val, "val")
            .add_string(str_next, "next")
            .add_string(str_cn, "java/util/concurrent/ConcurrentHashMap")
            .add_string(str_node_cn, "java/util/concurrent/ConcurrentHashMap$Node")
            .add_class(1, 1000, 0, str_cn)
            .add_class(2, 2000, 0, str_node_cn)
            .add_class_dump(
                1000,
                0,
                (4 + id_size) as u32,
                &[(str_size, 10), (str_table, 2)],
            )
            .add_class_dump(
                2000,
                0,
                (id_size * 3) as u32,
                &[(str_key, 2), (str_val, 2), (str_next, 2)],
            )
            .add_instance(0x100, 0, 1000, &chm_data)
            .add_object_array(0x500, 0, 2000, &[0x200])
            .add_instance(0x200, 0, 2000, &node_data)
            .build();

        let hfile = hfile_from_bytes(&bytes);
        let page = get_page(&hfile, 0x100, 0, 1000).unwrap();
        assert_eq!(page.total_count, 1);
        assert_eq!(page.entries.len(), 1);
        // Key and value must be resolved (non-null) via the "val" field
        assert!(matches!(
            page.entries[0].key,
            Some(FieldValue::ObjectRef { id: 0x10, .. })
        ));
        assert!(matches!(
            page.entries[0].value,
            FieldValue::ObjectRef { id: 0x20, .. }
        ));
    }

    // --- M2: HashSet returns keys only ---

    #[test]
    fn hashset_returns_keys_only() {
        let id_size: u32 = 8;
        let str_map = 10u64;
        let str_size = 11u64;
        let str_table = 12u64;
        let str_key = 13u64;
        let str_value = 14u64;
        let str_next = 15u64;
        let str_set_cn = 16u64;
        let str_map_cn = 17u64;
        let str_node_cn = 18u64;

        // HashSet instance: map=0x300
        let mut hs_data = Vec::new();
        hs_data.extend_from_slice(&0x300u64.to_be_bytes());

        // Backing HashMap instance: size=1, table=0x500
        let mut hm_data = Vec::new();
        hm_data.extend_from_slice(&1i32.to_be_bytes());
        hm_data.extend_from_slice(&0x500u64.to_be_bytes());

        // Node: key=0x10, value=0x20, next=0
        let mut node_data = Vec::new();
        node_data.extend_from_slice(&0x10u64.to_be_bytes());
        node_data.extend_from_slice(&0x20u64.to_be_bytes());
        node_data.extend_from_slice(&0u64.to_be_bytes());

        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", id_size)
            .add_string(str_map, "map")
            .add_string(str_size, "size")
            .add_string(str_table, "table")
            .add_string(str_key, "key")
            .add_string(str_value, "value")
            .add_string(str_next, "next")
            .add_string(str_set_cn, "java/util/HashSet")
            .add_string(str_map_cn, "java/util/HashMap")
            .add_string(str_node_cn, "java/util/HashMap$Node")
            .add_class(1, 1000, 0, str_set_cn)
            .add_class(2, 2000, 0, str_map_cn)
            .add_class(3, 3000, 0, str_node_cn)
            .add_class_dump(1000, 0, id_size, &[(str_map, 2)])
            .add_class_dump(
                2000,
                0,
                (4 + id_size) as u32,
                &[(str_size, 10), (str_table, 2)],
            )
            .add_class_dump(
                3000,
                0,
                (id_size * 3) as u32,
                &[(str_key, 2), (str_value, 2), (str_next, 2)],
            )
            .add_instance(0x100, 0, 1000, &hs_data)
            .add_instance(0x300, 0, 2000, &hm_data)
            .add_object_array(0x500, 0, 3000, &[0x200])
            .add_instance(0x200, 0, 3000, &node_data)
            .build();

        let hfile = hfile_from_bytes(&bytes);
        let page = get_page(&hfile, 0x100, 0, 1000).unwrap();
        assert_eq!(page.total_count, 1);
        assert_eq!(page.entries.len(), 1);
        // Set entry: no key, value = the HashMap key (0x10)
        assert!(page.entries[0].key.is_none());
        assert!(matches!(
            page.entries[0].value,
            FieldValue::ObjectRef { id: 0x10, .. }
        ));
    }

    // --- M2: LinkedHashMap delegates to HashMap extractor ---

    #[test]
    fn linkedhashmap_delegates_to_hashmap() {
        let id_size: u32 = 8;
        let str_size = 10u64;
        let str_table = 11u64;
        let str_key = 12u64;
        let str_value = 13u64;
        let str_next = 14u64;
        let str_cn = 15u64;
        let str_node_cn = 16u64;

        let mut lhm_data = Vec::new();
        lhm_data.extend_from_slice(&1i32.to_be_bytes());
        lhm_data.extend_from_slice(&0x500u64.to_be_bytes());

        let mut node_data = Vec::new();
        node_data.extend_from_slice(&0x10u64.to_be_bytes());
        node_data.extend_from_slice(&0x20u64.to_be_bytes());
        node_data.extend_from_slice(&0u64.to_be_bytes());

        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", id_size)
            .add_string(str_size, "size")
            .add_string(str_table, "table")
            .add_string(str_key, "key")
            .add_string(str_value, "value")
            .add_string(str_next, "next")
            .add_string(str_cn, "java/util/LinkedHashMap")
            .add_string(str_node_cn, "java/util/LinkedHashMap$Entry")
            .add_class(1, 1000, 0, str_cn)
            .add_class(2, 2000, 0, str_node_cn)
            .add_class_dump(
                1000,
                0,
                (4 + id_size) as u32,
                &[(str_size, 10), (str_table, 2)],
            )
            .add_class_dump(
                2000,
                0,
                (id_size * 3) as u32,
                &[(str_key, 2), (str_value, 2), (str_next, 2)],
            )
            .add_instance(0x100, 0, 1000, &lhm_data)
            .add_object_array(0x500, 0, 2000, &[0x200])
            .add_instance(0x200, 0, 2000, &node_data)
            .build();

        let hfile = hfile_from_bytes(&bytes);
        let page = get_page(&hfile, 0x100, 0, 1000).unwrap();
        assert_eq!(page.total_count, 1);
        assert_eq!(page.entries.len(), 1);
        assert!(page.entries[0].key.is_some());
    }

    // --- M2: Vector uses elementCount field (not size) ---

    #[test]
    fn vector_uses_elementcount_field() {
        let id_size: u32 = 8;
        let str_count = 10u64;
        let str_ed = 11u64;
        let str_cn = 12u64;

        // elementCount=2, elementData has capacity 4
        let mut data = Vec::new();
        data.extend_from_slice(&2i32.to_be_bytes());
        data.extend_from_slice(&0x500u64.to_be_bytes());

        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", id_size)
            .add_string(str_count, "elementCount")
            .add_string(str_ed, "elementData")
            .add_string(str_cn, "java/util/Vector")
            .add_class(1, 1000, 0, str_cn)
            .add_class_dump(
                1000,
                0,
                (4 + id_size) as u32,
                &[(str_count, 10), (str_ed, 2)],
            )
            .add_instance(0x100, 0, 1000, &data)
            .add_object_array(0x500, 0, 100, &[0x10, 0x20, 0, 0])
            .build();

        let hfile = hfile_from_bytes(&bytes);
        let page = get_page(&hfile, 0x100, 0, 1000).unwrap();
        // Must use elementCount=2, not capacity=4
        assert_eq!(page.total_count, 2);
        assert_eq!(page.entries.len(), 2);
        assert!(!page.has_more);
    }
}
