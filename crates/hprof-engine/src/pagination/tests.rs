//! Tests for pagination dispatch and extractors.
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
        .add_class_dump(1000, 0, 4 + id_size, &[(str_size, 10), (str_table, 2)])
        // Node class dump: key(Obj=2),
        // value(Obj=2), next(Obj=2)
        .add_class_dump(
            2000,
            0,
            id_size * 3,
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
        .add_class_dump(1000, 0, 4 + id_size, &[(str_size, 10), (str_ed, 2)])
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
            4 + id_size * 2,
            &[(str_size, 10), (str_first, 2), (str_last, 2)],
        )
        .add_class_dump(
            2000,
            0,
            id_size * 3,
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
        .add_class_dump(1000, 0, 4 + id_size, &[(str_size, 10), (str_table, 2)])
        .add_class_dump(
            2000,
            0,
            id_size * 3,
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
        .add_class_dump(2000, 0, 4 + id_size, &[(str_size, 10), (str_table, 2)])
        .add_class_dump(
            3000,
            0,
            id_size * 3,
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
        .add_class_dump(1000, 0, 4 + id_size, &[(str_size, 10), (str_table, 2)])
        .add_class_dump(
            2000,
            0,
            id_size * 3,
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
        .add_class_dump(1000, 0, 4 + id_size, &[(str_count, 10), (str_ed, 2)])
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

#[test]
fn id_to_field_value_for_object_array_id_sets_entry_count() {
    // Outer Object[] contains one element which is itself an inner Object[]
    let inner_id = 0xBB01u64;
    let outer_id = 0xBB02u64;
    let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
        .add_object_array(inner_id, 0, 0, &[0x01, 0x02, 0x03])
        .add_object_array(outer_id, 0, 0, &[inner_id])
        .build();
    let hfile = hfile_from_bytes(&bytes);
    let page = get_page(&hfile, outer_id, 0, 100).unwrap();
    assert_eq!(page.entries.len(), 1);
    match &page.entries[0].value {
        FieldValue::ObjectRef {
            class_name,
            entry_count,
            ..
        } => {
            assert_eq!(class_name, "Object[]");
            assert_eq!(*entry_count, Some(3));
        }
        other => panic!("expected ObjectRef, got {:?}", other),
    }
}

#[test]
fn id_to_field_value_for_prim_array_id_sets_entry_count() {
    // Outer Object[] contains one element which is an int[] of 5 elements
    let int_bytes: Vec<u8> = (0u32..5).flat_map(|n| n.to_be_bytes()).collect();
    let inner_id = 0xCC01u64;
    let outer_id = 0xCC02u64;
    let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
        .add_prim_array(inner_id, 0, 5, 10, &int_bytes)
        .add_object_array(outer_id, 0, 0, &[inner_id])
        .build();
    let hfile = hfile_from_bytes(&bytes);
    let page = get_page(&hfile, outer_id, 0, 100).unwrap();
    assert_eq!(page.entries.len(), 1);
    match &page.entries[0].value {
        FieldValue::ObjectRef {
            class_name,
            entry_count,
            ..
        } => {
            assert_eq!(class_name, "int[]");
            assert_eq!(*entry_count, Some(5));
        }
        other => panic!("expected ObjectRef, got {:?}", other),
    }
}

#[test]
fn get_page_with_arraylist_instance_id_returns_elements() {
    let id_size: u32 = 8;
    let str_size = 10u64;
    let str_ed = 11u64;
    let str_cn = 12u64;

    let mut data = Vec::new();
    data.extend_from_slice(&2i32.to_be_bytes()); // size=2
    data.extend_from_slice(&0x500u64.to_be_bytes()); // elementData=0x500

    let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", id_size)
        .add_string(str_size, "size")
        .add_string(str_ed, "elementData")
        .add_string(str_cn, "java/util/ArrayList")
        .add_class(1, 1000, 0, str_cn)
        .add_class_dump(1000, 0, 4 + id_size, &[(str_size, 10), (str_ed, 2)])
        .add_instance(0x100, 0, 1000, &data)
        .add_object_array(0x500, 0, 100, &[0x10, 0x20])
        .build();

    let hfile = hfile_from_bytes(&bytes);
    let page = get_page(&hfile, 0x100, 0, 100).unwrap();
    assert_eq!(page.total_count, 2);
    assert_eq!(page.entries.len(), 2);
}
