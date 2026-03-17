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

/// Pagination over `ObjectArrayDump` and `PrimArrayDump`, including edge cases
/// and `FieldValue::ObjectRef` enrichment for nested arrays.
mod array_pagination {
    use super::*;

    #[test]
    fn object_array_first_page() {
        let elements: Vec<u64> = (1..=5).collect();
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_object_array(0xA, 0, 100, &elements)
            .build();
        let hfile = hfile_from_bytes(&bytes);
        let page = get_page(&hfile, 0xA, 0, 3, None).unwrap();
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
        let page = get_page(&hfile, 0xA, 3, 3, None).unwrap();
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
        let page = get_page(&hfile, 0xA, 3, 1000, None).unwrap();
        assert_eq!(page.total_count, 5);
        assert_eq!(page.offset, 3);
        assert_eq!(page.entries.len(), 2);
        assert!(!page.has_more);
    }

    #[test]
    fn small_array_returns_all() {
        let elements: Vec<u64> = (1..=3).collect();
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_object_array(0xA, 0, 100, &elements)
            .build();
        let hfile = hfile_from_bytes(&bytes);
        let page = get_page(&hfile, 0xA, 0, 1000, None).unwrap();
        assert_eq!(page.total_count, 3);
        assert_eq!(page.entries.len(), 3);
        assert!(!page.has_more);
    }

    #[test]
    fn offset_beyond_bounds_returns_empty() {
        let elements: Vec<u64> = (1..=5).collect();
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_object_array(0xA, 0, 100, &elements)
            .build();
        let hfile = hfile_from_bytes(&bytes);
        let page = get_page(&hfile, 0xA, 100, 10, None).unwrap();
        assert_eq!(page.entries.len(), 0);
        assert!(!page.has_more);
    }

    #[test]
    fn has_more_flag_correct_at_boundary() {
        let elements: Vec<u64> = (1..=5).collect();
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_object_array(0xA, 0, 100, &elements)
            .build();
        let hfile = hfile_from_bytes(&bytes);

        // Exact fit: has_more = false
        let page = get_page(&hfile, 0xA, 0, 5, None).unwrap();
        assert_eq!(page.entries.len(), 5);
        assert!(!page.has_more);

        // One less: has_more = true
        let page = get_page(&hfile, 0xA, 0, 4, None).unwrap();
        assert_eq!(page.entries.len(), 4);
        assert!(page.has_more);
    }

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
        let page = get_page(&hfile, 0xB, 2, 3, None).unwrap();
        assert_eq!(page.total_count, 10);
        assert_eq!(page.offset, 2);
        assert_eq!(page.entries.len(), 3);
        assert!(page.has_more);
        assert_eq!(page.entries[0].value, FieldValue::Int(2));
        assert_eq!(page.entries[2].value, FieldValue::Int(4));
    }

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

    #[test]
    fn id_to_field_value_for_object_array_id_sets_entry_count() {
        let inner_id = 0xBB01u64;
        let outer_id = 0xBB02u64;
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_object_array(inner_id, 0, 0, &[0x01, 0x02, 0x03])
            .add_object_array(outer_id, 0, 0, &[inner_id])
            .build();
        let hfile = hfile_from_bytes(&bytes);
        let page = get_page(&hfile, outer_id, 0, 100, None).unwrap();
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
        let int_bytes: Vec<u8> = (0u32..5).flat_map(|n| n.to_be_bytes()).collect();
        let inner_id = 0xCC01u64;
        let outer_id = 0xCC02u64;
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_prim_array(inner_id, 0, 5, 10, &int_bytes)
            .add_object_array(outer_id, 0, 0, &[inner_id])
            .build();
        let hfile = hfile_from_bytes(&bytes);
        let page = get_page(&hfile, outer_id, 0, 100, None).unwrap();
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
}

/// Dispatcher behaviour when `get_page` cannot resolve an ID:
/// unknown ID, unsupported collection type, fully unknown class.
mod dispatch {
    use super::*;

    #[test]
    fn nonexistent_id_returns_none() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_object_array(0xA, 0, 100, &[1])
            .build();
        let hfile = hfile_from_bytes(&bytes);
        assert!(get_page(&hfile, 0xDEAD, 0, 10, None).is_none());
    }

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
        assert!(get_page(&hfile, 0x100, 0, 10, None).is_none());
    }

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
        assert!(get_page(&hfile, 0x100, 0, 10, None).is_none());
    }
}

/// Sequential collection extractors: `ArrayList`, `Vector`, `LinkedList`.
/// Verifies that logical size fields are used instead of backing-array capacity.
mod list_extractors {
    use super::*;

    #[test]
    fn arraylist_uses_size_not_capacity() {
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
        let page = get_page(&hfile, 0x100, 0, 1000, None).unwrap();
        // Must use size=2, not capacity=4
        assert_eq!(page.total_count, 2);
        assert_eq!(page.entries.len(), 2);
        assert!(!page.has_more);
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
        let page = get_page(&hfile, 0x100, 0, 100, None).unwrap();
        assert_eq!(page.total_count, 2);
        assert_eq!(page.entries.len(), 2);
    }

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
        let page = get_page(&hfile, 0x100, 0, 1000, None).unwrap();
        // Must use elementCount=2, not capacity=4
        assert_eq!(page.total_count, 2);
        assert_eq!(page.entries.len(), 2);
        assert!(!page.has_more);
    }

    #[test]
    fn linked_list_walks_chain() {
        let hfile = hfile_from_bytes(&build_linked_list_fixture());
        let page = get_page(&hfile, 0x100, 0, 1000, None).unwrap();
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
        let page = get_page(&hfile, 0x100, 1, 1000, None).unwrap();
        assert_eq!(page.total_count, 2);
        assert_eq!(page.entries.len(), 1);
        assert_eq!(page.entries[0].index, 1);
        assert!(!page.has_more);
    }
}

/// Hash-table collection extractors: `HashMap`, `ConcurrentHashMap`,
/// `LinkedHashMap`, `HashSet`.
mod map_extractors {
    use super::*;

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
        let page = get_page(&hfile, 0x100, 0, 1000, None).unwrap();
        assert_eq!(page.total_count, 2);
        assert_eq!(page.entries.len(), 2);
        // Keys should be ObjectRef
        assert!(page.entries[0].key.is_some());
        assert!(page.entries[1].key.is_some());
    }

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
        let page = get_page(&hfile, 0x100, 0, 1000, None).unwrap();
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
        let page = get_page(&hfile, 0x100, 0, 1000, None).unwrap();
        assert_eq!(page.total_count, 1);
        assert_eq!(page.entries.len(), 1);
        assert!(page.entries[0].key.is_some());
    }

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
        let page = get_page(&hfile, 0x100, 0, 1000, None).unwrap();
        assert_eq!(page.total_count, 1);
        assert_eq!(page.entries.len(), 1);
        // Set entry: no key, value = the HashMap key (0x10)
        assert!(page.entries[0].key.is_none());
        assert!(matches!(
            page.entries[0].value,
            FieldValue::ObjectRef { id: 0x10, .. }
        ));
    }

    /// Verifies that `get_page` on an Object[] with
    /// multiple instance references resolves them via
    /// O(1) positional reads (Story 11.4).
    #[test]
    fn get_page_object_array_resolves_instance_refs() {
        let id_size: u32 = 8;
        let str_cn = 50u64;

        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", id_size)
            .add_string(str_cn, "java/lang/Object")
            .add_class(1, 1000, 0, str_cn)
            .add_class_dump(1000, 0, 8, &[])
            .add_instance(0x10, 0, 1000, &[])
            .add_instance(0x20, 0, 1000, &[])
            .add_instance(0x30, 0, 1000, &[])
            .add_instance(0x40, 0, 1000, &[])
            .add_instance(0x50, 0, 1000, &[])
            .add_object_array(0xAA, 0, 1000, &[0x10, 0x20, 0x30, 0x40, 0x50])
            .build();

        let hfile = hfile_from_bytes(&bytes);

        let page = get_page(&hfile, 0xAA, 0, 5, None).unwrap();
        assert_eq!(page.total_count, 5);
        assert_eq!(page.entries.len(), 5);

        for (i, entry) in page.entries.iter().enumerate() {
            assert!(
                matches!(entry.value, FieldValue::ObjectRef { .. }),
                "entry {i} must be ObjectRef, got {:?}",
                entry.value
            );
        }
    }
}

/// Story 11.4: O(1) object array pagination tests.
mod object_array_pagination {
    use super::*;

    #[test]
    fn try_object_array_mid_page() {
        let elements: Vec<u64> = (1..=10).collect();
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_object_array(0xA, 0, 100, &elements)
            .build();
        let hfile = hfile_from_bytes(&bytes);

        let page = get_page(&hfile, 0xA, 5, 3, None).unwrap();
        assert_eq!(page.entries.len(), 3);
        assert_eq!(page.offset, 5);
        assert_eq!(page.total_count, 10);
        assert!(page.has_more);
    }

    #[test]
    fn try_object_array_beyond_end() {
        let elements: Vec<u64> = (1..=10).collect();
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_object_array(0xA, 0, 100, &elements)
            .build();
        let hfile = hfile_from_bytes(&bytes);

        let page = get_page(&hfile, 0xA, 8, 100, None).unwrap();
        assert_eq!(page.entries.len(), 2);
        assert!(!page.has_more);
    }

    #[test]
    fn empty_object_array_paginate() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_object_array(0xA, 0, 100, &[])
            .build();
        let hfile = hfile_from_bytes(&bytes);

        let page = get_page(&hfile, 0xA, 0, 100, None).unwrap();
        assert_eq!(page.entries.len(), 0);
        assert_eq!(page.total_count, 0);
        assert!(!page.has_more);
    }

    #[test]
    fn arraylist_size_less_than_capacity() {
        let id_size: u32 = 8;
        let str_size = 10u64;
        let str_ed = 11u64;
        let str_cn = 12u64;

        // Instance: size(Int=3) + elementData(Obj=0x500)
        let mut data = Vec::new();
        data.extend_from_slice(&3i32.to_be_bytes());
        data.extend_from_slice(&0x500u64.to_be_bytes());

        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", id_size)
            .add_string(str_size, "size")
            .add_string(str_ed, "elementData")
            .add_string(str_cn, "java/util/ArrayList")
            .add_class(1, 1000, 0, str_cn)
            .add_class_dump(1000, 0, 4 + id_size, &[(str_size, 10), (str_ed, 2)])
            .add_instance(0x100, 0, 1000, &data)
            // Backing array: 10 slots, only 3 used
            .add_object_array(0x500, 0, 100, &[0x10, 0x20, 0x30, 0, 0, 0, 0, 0, 0, 0])
            .build();

        let hfile = hfile_from_bytes(&bytes);
        let page = get_page(&hfile, 0x100, 0, 100, None).unwrap();
        assert_eq!(page.total_count, 3);
        assert_eq!(page.entries.len(), 3);
        assert!(!page.has_more);
    }
}

/// Story 11.5: Skip-index integration tests for
/// variable-size collection pagination.
mod skip_index_integration {
    use super::*;
    use crate::pagination::skip_index::SkipIndex;

    /// Builds a LinkedList with `n` nodes.
    ///
    /// Node IDs: 0x200, 0x201, …, 0x200 + (n-1).
    /// Item IDs: 0x10, 0x11, …
    fn build_linked_list_n(n: usize) -> Vec<u8> {
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

    /// Builds a HashMap with `n` entries, each in its own
    /// slot (no chaining). `pct_empty` controls the
    /// percentage of empty slots (0.0 = none, 0.5 = 50%).
    fn build_hashmap_n(n: usize, pct_empty: f64) -> Vec<u8> {
        let id_size: u32 = 8;
        let str_size = 10u64;
        let str_table = 11u64;
        let str_key = 12u64;
        let str_value = 13u64;
        let str_next = 14u64;
        let str_cn = 15u64;
        let str_node_cn = 16u64;

        // Calculate table size to achieve desired empty %
        let total_slots = if pct_empty > 0.0 {
            (n as f64 / (1.0 - pct_empty)).ceil() as usize
        } else {
            n
        };

        let mut table: Vec<u64> = vec![0; total_slots];
        // Place entries evenly across the table
        let step = if n > 0 { total_slots / n } else { 1 };
        for i in 0..n {
            let slot = (i * step).min(total_slots - 1);
            table[slot] = 0x200u64 + i as u64;
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
            let mut node_data = Vec::new();
            node_data.extend_from_slice(&key_id.to_be_bytes());
            node_data.extend_from_slice(&val_id.to_be_bytes());
            node_data.extend_from_slice(&0u64.to_be_bytes()); // next=0
            builder = builder.add_instance(node_id, 0, 2000, &node_data);
        }

        builder.build()
    }

    // -- Test 5.2: LinkedList skip-index integration --

    #[test]
    fn linked_list_skip_index_records_checkpoints() {
        let hfile = hfile_from_bytes(&build_linked_list_n(30));
        let mut si = SkipIndex::new(10);

        // Page 0: no skip-index needed
        let p0 = get_page(&hfile, 0x100, 0, 10, Some(&mut si)).unwrap();
        assert_eq!(p0.entries.len(), 10);
        assert_eq!(p0.offset, 0);

        // Page 2 (offset=20): walk from head, records
        // checkpoints at 0, 10, 20
        let p2 = get_page(&hfile, 0x100, 20, 10, Some(&mut si)).unwrap();
        assert_eq!(p2.entries.len(), 10);
        assert_eq!(p2.offset, 20);

        // Verify checkpoints recorded
        let (idx, _) = si.nearest_before(20).unwrap();
        assert_eq!(idx, 20);

        let (idx0, _) = si.nearest_before(5).unwrap();
        assert_eq!(idx0, 0);

        let (idx10, _) = si.nearest_before(15).unwrap();
        assert_eq!(idx10, 10);

        // Verify content matches full sequential traversal
        let full = get_page(&hfile, 0x100, 0, 30, None).unwrap();
        for (i, entry) in p2.entries.iter().enumerate() {
            assert_eq!(
                entry.value,
                full.entries[20 + i].value,
                "mismatch at index {}",
                20 + i
            );
        }
    }

    // -- Test 5.3: HashMap skip-index integration --

    #[test]
    fn hashmap_skip_index_records_checkpoints() {
        let hfile = hfile_from_bytes(&build_hashmap_n(30, 0.0));
        let mut si = SkipIndex::new(10);

        let p0 = get_page(&hfile, 0x100, 0, 10, Some(&mut si)).unwrap();
        assert_eq!(p0.entries.len(), 10);

        let p2 = get_page(&hfile, 0x100, 20, 10, Some(&mut si)).unwrap();
        assert_eq!(p2.entries.len(), 10);
        assert_eq!(p2.offset, 20);

        // Verify content matches full traversal
        let full = get_page(&hfile, 0x100, 0, 30, None).unwrap();
        for (i, entry) in p2.entries.iter().enumerate() {
            assert_eq!(
                entry.key,
                full.entries[20 + i].key,
                "key mismatch at index {}",
                20 + i
            );
        }
    }

    // -- Test 5.6: Empty LinkedList --

    #[test]
    fn empty_linked_list_with_skip_index() {
        let hfile = hfile_from_bytes(&build_linked_list_n(0));
        let mut si = SkipIndex::new(10);
        let page = get_page(&hfile, 0x100, 0, 10, Some(&mut si)).unwrap();
        assert_eq!(page.entries.len(), 0);
        assert_eq!(page.total_count, 0);
    }

    // -- Test 5.7: Offset beyond known checkpoints --

    #[test]
    fn offset_beyond_known_checkpoints_falls_back() {
        let hfile = hfile_from_bytes(&build_linked_list_n(30));
        let mut si = SkipIndex::new(10);

        // Build partial skip-index (up to entry 20)
        let _ = get_page(&hfile, 0x100, 20, 10, Some(&mut si));

        // Now request offset=25, which is beyond
        // checkpoint 20 but below 30 — should resume
        // from checkpoint 20
        let page = get_page(&hfile, 0x100, 25, 5, Some(&mut si)).unwrap();
        assert_eq!(page.entries.len(), 5);
        assert_eq!(page.offset, 25);

        // Verify content
        let full = get_page(&hfile, 0x100, 0, 30, None).unwrap();
        for (i, entry) in page.entries.iter().enumerate() {
            assert_eq!(
                entry.value,
                full.entries[25 + i].value,
                "mismatch at index {}",
                25 + i
            );
        }
    }

    // -- Test 5.9: HashMap with 50% empty slots --

    #[test]
    fn hashmap_50pct_empty_slots() {
        let hfile = hfile_from_bytes(&build_hashmap_n(30, 0.5));
        let mut si = SkipIndex::new(10);

        // First pass: build partial index
        let p2 = get_page(&hfile, 0x100, 20, 10, Some(&mut si)).unwrap();
        assert_eq!(p2.entries.len(), 10);
        assert_eq!(p2.offset, 20);

        // Verify resuming from checkpoint returns correct
        // entries
        let full = get_page(&hfile, 0x100, 0, 30, None).unwrap();
        for (i, entry) in p2.entries.iter().enumerate() {
            assert_eq!(
                entry.key,
                full.entries[20 + i].key,
                "key mismatch at index {}",
                20 + i
            );
        }
    }

    // -- Test 5.10: Partial skip-index extension --

    #[test]
    fn partial_skip_index_extension() {
        let hfile = hfile_from_bytes(&build_linked_list_n(50));
        let mut si = SkipIndex::new(10);

        // Call 1: page 0 → no skip-index changes
        // (no checkpoints recorded beyond what the
        // walk covers)
        let p0 = get_page(&hfile, 0x100, 0, 10, Some(&mut si)).unwrap();
        assert_eq!(p0.entries.len(), 10);
        // Skip-index must have checkpoint at 0 from the walk
        assert!(
            si.nearest_before(0).is_some(),
            "checkpoint 0 must be recorded after page 0 walk"
        );

        // Call 2: offset=20 → walk from head (or
        // checkpoint), records checkpoints at 0,10,20
        let p2 = get_page(&hfile, 0x100, 20, 10, Some(&mut si)).unwrap();
        assert_eq!(p2.entries.len(), 10);
        assert!(!si.is_complete());

        // Verify 3 checkpoints (0, 10, 20)
        assert!(si.nearest_before(5).is_some());
        assert!(si.nearest_before(15).is_some());
        assert!(si.nearest_before(25).is_some());

        // Call 3: offset=40 → resume from checkpoint 20,
        // records 30, 40
        let p4 = get_page(&hfile, 0x100, 40, 10, Some(&mut si)).unwrap();
        assert_eq!(p4.entries.len(), 10);
        assert_eq!(p4.offset, 40);

        // Verify 5 checkpoints (0, 10, 20, 30, 40)
        let (idx, _) = si.nearest_before(35).unwrap();
        assert_eq!(idx, 30);
        let (idx, _) = si.nearest_before(45).unwrap();
        assert_eq!(idx, 40);

        // Call 4: offset=40 again → reaches end,
        // mark_complete
        let p4b = get_page(&hfile, 0x100, 40, 10, Some(&mut si)).unwrap();
        assert_eq!(p4b.entries.len(), 10);
        assert!(si.is_complete());

        // Verify content matches full traversal
        let full = get_page(&hfile, 0x100, 0, 50, None).unwrap();
        for (i, entry) in p4.entries.iter().enumerate() {
            assert_eq!(
                entry.value,
                full.entries[40 + i].value,
                "mismatch at index {}",
                40 + i
            );
        }
    }

    // -- Test 5.8: LinkedList with cycle --

    /// Builds a LinkedList with `n` nodes where node at
    /// `cycle_from` links back to node at `cycle_to`.
    fn build_linked_list_with_cycle(n: usize, cycle_from: usize, cycle_to: usize) -> Vec<u8> {
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
        ll_data.extend_from_slice(&(n as i32).to_be_bytes());
        ll_data.extend_from_slice(&0x200u64.to_be_bytes()); // first
        ll_data.extend_from_slice(&(0x200u64 + (n - 1) as u64).to_be_bytes()); // last

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
            let next_id = if i == cycle_from {
                // Cycle: link back to cycle_to
                0x200u64 + cycle_to as u64
            } else if i + 1 < n {
                0x200u64 + (i + 1) as u64
            } else {
                0u64
            };
            let mut node_data = Vec::new();
            node_data.extend_from_slice(&item_id.to_be_bytes());
            node_data.extend_from_slice(&next_id.to_be_bytes());
            node_data.extend_from_slice(&0u64.to_be_bytes()); // prev
            builder = builder.add_instance(node_id, 0, 2000, &node_data);
        }

        builder.build()
    }

    #[test]
    fn linked_list_cycle_full_traversal() {
        // 15 nodes, cycle: node 12 → node 5
        let hfile = hfile_from_bytes(&build_linked_list_with_cycle(15, 12, 5));
        let page = get_page(&hfile, 0x100, 0, 100, None).unwrap();
        // visited detects cycle at node 5 revisit
        // → stops at 13 entries (0..12 inclusive)
        assert!(page.entries.len() <= 15);
        assert!(page.entries.len() >= 13);
    }

    #[test]
    fn linked_list_cycle_resumed_walk_max_iter_guard() {
        // 15 nodes, cycle: node 12 → node 5
        let hfile = hfile_from_bytes(&build_linked_list_with_cycle(15, 12, 5));
        let mut si = SkipIndex::new(10);

        // First walk: offset=0, limit=10 → records
        // checkpoints at 0, 10
        let p0 = get_page(&hfile, 0x100, 0, 10, Some(&mut si)).unwrap();
        assert_eq!(p0.entries.len(), 10);

        // Resumed walk from checkpoint 10:
        // max_iter = 15 - 10 = 5
        // Walk: node 10 → 11 → 12 → 5(cycle) → 6 → …
        // max_iter guard breaks after 5 iterations
        let p1 = get_page(&hfile, 0x100, 10, 10, Some(&mut si)).unwrap();
        // Should get at most 5 entries (max_iter guard)
        assert!(
            p1.entries.len() <= 5,
            "expected ≤ 5 entries due to max_iter guard, \
             got {}",
            p1.entries.len()
        );
    }

    // -- Test 6.5: Skip-index activation smoke test --

    #[test]
    fn skip_index_activation_smoke_test() {
        let hfile = hfile_from_bytes(&build_linked_list_n(30));
        let mut si = SkipIndex::new(10);

        // Build skip-index via offset=20
        let _ = get_page(&hfile, 0x100, 20, 10, Some(&mut si));

        // Verify the skip-index was actually populated
        let result = si.nearest_before(20);
        assert!(result.is_some(), "skip-index should have a checkpoint ≤ 20");
        let (idx, _) = result.unwrap();
        assert_eq!(idx, 20, "nearest checkpoint before 20 should be 20");
    }
}
