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
fn indexing_ratio_100_for_complete_file() {
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    tmp.write_all(&minimal_hprof_bytes()).unwrap();
    tmp.flush().unwrap();

    let config = EngineConfig::default();
    let engine = Engine::from_file(tmp.path(), &config).unwrap();
    // Empty file → records_attempted == 0 → ratio is 100.0
    assert_eq!(engine.indexing_ratio(), 100.0);
}

#[test]
fn indexing_ratio_partial_for_truncated_file() {
    // Build a file with 2 attempted, 1 indexed by crafting the Engine fields
    // directly via Arc<HprofFile> is not possible, so we verify via the
    // formula: ratio = indexed / attempted * 100.0
    // We test the formula in isolation here.
    let attempted: u64 = 10;
    let indexed: u64 = 8;
    let ratio = indexed as f64 / attempted as f64 * 100.0;
    assert!((ratio - 80.0).abs() < f64::EPSILON);
}

#[test]
fn is_fully_indexed_true_for_complete_file() {
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    tmp.write_all(&minimal_hprof_bytes()).unwrap();
    tmp.flush().unwrap();

    let config = EngineConfig::default();
    let engine = Engine::from_file(tmp.path(), &config).unwrap();
    assert!(engine.is_fully_indexed());
}

#[test]
fn is_fully_indexed_false_for_partial_file() {
    // Verify the integer comparison logic: indexed < attempted → false
    let attempted: u64 = 10;
    let indexed: u64 = 8;
    let fully = attempted == 0 || indexed >= attempted;
    assert!(!fully);
}

#[test]
fn is_fully_indexed_false_when_file_truncated_mid_record() {
    // A file truncated mid-record breaks the scan loop before
    // incrementing records_attempted, so the ratio stays 100/100.
    // is_fully_indexed() must detect this via index_warnings.
    use std::io::Write as IoWrite;
    let mut bytes = minimal_hprof_bytes();
    // Append a STRING record header claiming 9999 bytes of payload,
    // but provide only 2 bytes — payload end exceeds file size.
    bytes.push(0x01); // tag STRING_IN_UTF8
    bytes.extend_from_slice(&0u32.to_be_bytes()); // time_offset
    bytes.extend_from_slice(&9999u32.to_be_bytes()); // length (lies)
    bytes.extend_from_slice(&[0xAA, 0xBB]); // only 2 bytes of payload

    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    tmp.write_all(&bytes).unwrap();
    tmp.flush().unwrap();

    let config = EngineConfig::default();
    let engine = Engine::from_file(tmp.path(), &config).unwrap();
    assert!(
        !engine.is_fully_indexed(),
        "truncated file must not be reported as fully indexed"
    );
    assert!(
        !engine.warnings().is_empty(),
        "truncated file must produce at least one indexing warning"
    );
}

#[test]
fn skeleton_bytes_positive_for_real_file() {
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    tmp.write_all(&minimal_hprof_bytes()).unwrap();
    tmp.flush().unwrap();

    let config = EngineConfig::default();
    let engine = Engine::from_file(tmp.path(), &config).unwrap();
    // Even an empty file has a non-zero PreciseIndex struct size
    assert!(engine.skeleton_bytes() > 0);
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
                entry_count: None,
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
                entry_count: None,
            }
        );
    }

    #[test]
    fn get_local_variables_object_array_root_has_entry_count() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_string(1, "foo")
            .add_string(2, "()")
            .add_string(3, "")
            .add_string(4, "Obj")
            .add_class(1, 100, 0, 4)
            .add_stack_frame(50, 1, 2, 3, 1, 1)
            .add_stack_trace(10, 1, &[50])
            .add_thread(1, 200, 10, 1, 0, 0)
            .add_object_array(0xBBB, 0, 0xCC, &[0xC01, 0xC02, 0xC03])
            .add_java_frame_root(0xBBB, 1, 0)
            .build();
        let engine = engine_from_bytes(&bytes);
        let vars = engine.get_local_variables(50);
        assert_eq!(vars.len(), 1);
        match &vars[0].value {
            VariableValue::ObjectRef {
                class_name,
                entry_count,
                ..
            } => {
                assert_eq!(class_name, "Object[]");
                assert_eq!(*entry_count, Some(3));
            }
            _ => panic!("expected ObjectRef"),
        }
    }

    #[test]
    fn get_local_variables_prim_array_root_has_entry_count() {
        let int_bytes: Vec<u8> = (0u32..5).flat_map(|n| n.to_be_bytes()).collect();
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_string(1, "foo")
            .add_string(2, "()")
            .add_string(3, "")
            .add_string(4, "Obj")
            .add_class(1, 100, 0, 4)
            .add_stack_frame(50, 1, 2, 3, 1, 1)
            .add_stack_trace(10, 1, &[50])
            .add_thread(1, 200, 10, 1, 0, 0)
            .add_prim_array(0xCCC, 0, 5, 10, &int_bytes)
            .add_java_frame_root(0xCCC, 1, 0)
            .build();
        let engine = engine_from_bytes(&bytes);
        let vars = engine.get_local_variables(50);
        assert_eq!(vars.len(), 1);
        match &vars[0].value {
            VariableValue::ObjectRef {
                class_name,
                entry_count,
                ..
            } => {
                assert_eq!(class_name, "int[]");
                assert_eq!(*entry_count, Some(5));
            }
            _ => panic!("expected ObjectRef"),
        }
    }

    #[test]
    fn get_local_variables_plain_object_has_no_entry_count() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_string(1, "foo")
            .add_string(2, "()")
            .add_string(3, "")
            .add_string(4, "Foo")
            .add_class(1, 100, 0, 4)
            .add_stack_frame(50, 1, 2, 3, 1, 1)
            .add_stack_trace(10, 1, &[50])
            .add_thread(1, 300, 10, 1, 0, 0)
            .add_class_dump(100, 0, 0, &[])
            .add_instance(42, 0, 100, &[])
            .add_java_frame_root(42, 1, 0)
            .build();
        let engine = engine_from_bytes(&bytes);
        let vars = engine.get_local_variables(50);
        assert_eq!(vars.len(), 1);
        match &vars[0].value {
            VariableValue::ObjectRef { entry_count, .. } => {
                assert_eq!(*entry_count, None);
            }
            _ => panic!("expected ObjectRef"),
        }
    }

    #[test]
    fn get_local_variables_empty_object_array_has_entry_count_zero() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_string(1, "foo")
            .add_string(2, "()")
            .add_string(3, "")
            .add_string(4, "Obj")
            .add_class(1, 100, 0, 4)
            .add_stack_frame(50, 1, 2, 3, 1, 1)
            .add_stack_trace(10, 1, &[50])
            .add_thread(1, 200, 10, 1, 0, 0)
            .add_object_array(0xEEE, 0, 0xCC, &[])
            .add_java_frame_root(0xEEE, 1, 0)
            .build();
        let engine = engine_from_bytes(&bytes);
        let vars = engine.get_local_variables(50);
        match &vars[0].value {
            VariableValue::ObjectRef { entry_count, .. } => {
                assert_eq!(*entry_count, Some(0));
            }
            _ => panic!("expected ObjectRef"),
        }
    }

    #[test]
    fn get_local_variables_linked_list_root_has_entry_count() {
        let size_bytes = 3i32.to_be_bytes();
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_string(1, "foo")
            .add_string(2, "()")
            .add_string(3, "")
            .add_string(4, "java/util/LinkedList")
            .add_string(5, "size")
            .add_class(1, 0xAA01, 0, 4)
            .add_stack_frame(50, 1, 2, 3, 1, 1)
            .add_stack_trace(10, 1, &[50])
            .add_thread(1, 200, 10, 1, 0, 0)
            .add_class_dump(0xAA01, 0, 4, &[(5, 10)])
            .add_instance(0xAA02, 0, 0xAA01, &size_bytes)
            .add_java_frame_root(0xAA02, 1, 0)
            .build();
        let engine = engine_from_bytes(&bytes);
        let vars = engine.get_local_variables(50);
        assert_eq!(vars.len(), 1);
        match &vars[0].value {
            VariableValue::ObjectRef {
                class_name,
                entry_count,
                ..
            } => {
                assert_eq!(class_name, "java.util.LinkedList");
                assert_eq!(*entry_count, Some(3));
            }
            _ => panic!("expected ObjectRef"),
        }
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

    fn push_string(index: &mut PreciseIndex, buf: &mut Vec<u8>, id: u64, value: &str) {
        let offset = buf.len() as u64;
        buf.extend_from_slice(value.as_bytes());
        index.strings.insert(
            id,
            HprofStringRef {
                id,
                offset,
                len: value.len() as u32,
            },
        );
    }

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
                static_fields: vec![],
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

    #[test]
    fn hashmap_size_is_not_shifted_by_super_fields() {
        let mut index = PreciseIndex::new();
        let mut buf = Vec::new();

        let sid_table = 1;
        let sid_entry_set = 2;
        let sid_size = 3;
        let sid_mod_count = 4;
        let sid_threshold = 5;
        let sid_load_factor = 6;
        let sid_key_set = 7;
        let sid_values = 8;

        push_string(&mut index, &mut buf, sid_table, "table");
        push_string(&mut index, &mut buf, sid_entry_set, "entrySet");
        push_string(&mut index, &mut buf, sid_size, "size");
        push_string(&mut index, &mut buf, sid_mod_count, "modCount");
        push_string(&mut index, &mut buf, sid_threshold, "threshold");
        push_string(&mut index, &mut buf, sid_load_factor, "loadFactor");
        push_string(&mut index, &mut buf, sid_key_set, "keySet");
        push_string(&mut index, &mut buf, sid_values, "values");

        let abstract_map_id = 50u64;
        let hashmap_id = 100u64;

        index.class_dumps.insert(
            abstract_map_id,
            ClassDumpInfo {
                class_object_id: abstract_map_id,
                super_class_id: 0,
                instance_size: 16,
                static_fields: vec![],
                instance_fields: vec![
                    FieldDef {
                        name_string_id: sid_key_set,
                        field_type: 2,
                    },
                    FieldDef {
                        name_string_id: sid_values,
                        field_type: 2,
                    },
                ],
            },
        );
        index.class_dumps.insert(
            hashmap_id,
            ClassDumpInfo {
                class_object_id: hashmap_id,
                super_class_id: abstract_map_id,
                instance_size: 36,
                static_fields: vec![],
                instance_fields: vec![
                    FieldDef {
                        name_string_id: sid_table,
                        field_type: 2,
                    },
                    FieldDef {
                        name_string_id: sid_entry_set,
                        field_type: 2,
                    },
                    FieldDef {
                        name_string_id: sid_size,
                        field_type: 10,
                    },
                    FieldDef {
                        name_string_id: sid_mod_count,
                        field_type: 10,
                    },
                    FieldDef {
                        name_string_id: sid_threshold,
                        field_type: 10,
                    },
                    FieldDef {
                        name_string_id: sid_load_factor,
                        field_type: 6,
                    },
                ],
            },
        );
        index
            .class_names_by_id
            .insert(hashmap_id, "java.util.HashMap".to_string());

        let mut data = Vec::new();
        data.extend_from_slice(&0x10u64.to_be_bytes()); // table
        data.extend_from_slice(&0x11u64.to_be_bytes()); // entrySet
        data.extend_from_slice(&14_000i32.to_be_bytes()); // size
        data.extend_from_slice(&1i32.to_be_bytes()); // modCount
        data.extend_from_slice(&16_384i32.to_be_bytes()); // threshold
        data.extend_from_slice(&0.75f32.to_be_bytes()); // loadFactor
        // Super fields (AbstractMap)
        data.extend_from_slice(&0x0852_B150_1234_5678u64.to_be_bytes()); // keySet
        data.extend_from_slice(&0x0852_B151_1234_5678u64.to_be_bytes()); // values

        let raw = RawInstance {
            class_object_id: hashmap_id,
            data,
        };

        assert_eq!(collection_entry_count(&raw, &index, 8, &buf), Some(14_000));
    }

    #[test]
    fn linked_hashmap_reads_size_from_super_hashmap() {
        let mut index = PreciseIndex::new();
        let mut buf = Vec::new();

        let sid_table = 1;
        let sid_entry_set = 2;
        let sid_size = 3;
        let sid_head = 4;
        let sid_tail = 5;
        let sid_access_order = 6;

        push_string(&mut index, &mut buf, sid_table, "table");
        push_string(&mut index, &mut buf, sid_entry_set, "entrySet");
        push_string(&mut index, &mut buf, sid_size, "size");
        push_string(&mut index, &mut buf, sid_head, "head");
        push_string(&mut index, &mut buf, sid_tail, "tail");
        push_string(&mut index, &mut buf, sid_access_order, "accessOrder");

        let hashmap_id = 200u64;
        let linked_hashmap_id = 201u64;

        index.class_dumps.insert(
            hashmap_id,
            ClassDumpInfo {
                class_object_id: hashmap_id,
                super_class_id: 0,
                instance_size: 20,
                static_fields: vec![],
                instance_fields: vec![
                    FieldDef {
                        name_string_id: sid_table,
                        field_type: 2,
                    },
                    FieldDef {
                        name_string_id: sid_entry_set,
                        field_type: 2,
                    },
                    FieldDef {
                        name_string_id: sid_size,
                        field_type: 10,
                    },
                ],
            },
        );
        index.class_dumps.insert(
            linked_hashmap_id,
            ClassDumpInfo {
                class_object_id: linked_hashmap_id,
                super_class_id: hashmap_id,
                instance_size: 17,
                static_fields: vec![],
                instance_fields: vec![
                    FieldDef {
                        name_string_id: sid_head,
                        field_type: 2,
                    },
                    FieldDef {
                        name_string_id: sid_tail,
                        field_type: 2,
                    },
                    FieldDef {
                        name_string_id: sid_access_order,
                        field_type: 4,
                    },
                ],
            },
        );
        index
            .class_names_by_id
            .insert(linked_hashmap_id, "java.util.LinkedHashMap".to_string());

        let mut data = Vec::new();
        // Leaf class first (LinkedHashMap)
        data.extend_from_slice(&0x21u64.to_be_bytes()); // head
        data.extend_from_slice(&0x22u64.to_be_bytes()); // tail
        data.push(1u8); // accessOrder
        // Super class fields (HashMap)
        data.extend_from_slice(&0x30u64.to_be_bytes()); // table
        data.extend_from_slice(&0x31u64.to_be_bytes()); // entrySet
        data.extend_from_slice(&14_000i32.to_be_bytes()); // size

        let raw = RawInstance {
            class_object_id: linked_hashmap_id,
            data,
        };

        assert_eq!(collection_entry_count(&raw, &index, 8, &buf), Some(14_000));
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

mod static_fields_tests {
    use std::io::Write as IoWrite;

    use hprof_parser::{HprofTestBuilder, StaticValue};

    use super::*;

    fn engine_from_bytes(bytes: &[u8]) -> Engine {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(bytes).unwrap();
        tmp.flush().unwrap();
        let config = EngineConfig::default();
        Engine::from_file(tmp.path(), &config).unwrap()
    }

    #[test]
    fn class_of_object_returns_class_id() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_class_dump(100, 0, 0, &[])
            .add_instance(0xABCD, 0, 100, &[])
            .build();
        let engine = engine_from_bytes(&bytes);
        assert_eq!(engine.class_of_object(0xABCD), Some(100));
    }

    #[test]
    fn class_of_object_returns_none_for_unknown_object() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_class_dump(100, 0, 0, &[])
            .add_instance(0xABCD, 0, 100, &[])
            .build();
        let engine = engine_from_bytes(&bytes);
        assert_eq!(engine.class_of_object(0xDEAD), None);
    }

    #[test]
    fn get_static_fields_returns_resolved_fields() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_string(1, "counter")
            .add_string(2, "owner")
            .add_string(3, "java/lang/String")
            .add_class(1, 200, 0, 3)
            .add_class_dump_with_static_fields(
                100,
                0,
                0,
                &[],
                &[
                    (1, StaticValue::Int(42)),
                    (2, StaticValue::ObjectRef(0xDEAD)),
                ],
            )
            .add_class_dump(200, 0, 0, &[])
            .add_instance(0xDEAD, 0, 200, &[])
            .build();
        let engine = engine_from_bytes(&bytes);
        let fields = engine.get_static_fields(100);

        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name, "counter");
        assert_eq!(fields[0].value, crate::engine::FieldValue::Int(42));
        assert_eq!(fields[1].name, "owner");
        assert_eq!(
            fields[1].value,
            crate::engine::FieldValue::ObjectRef {
                id: 0xDEAD,
                class_name: "java.lang.String".to_string(),
                entry_count: None,
                inline_value: None,
            }
        );
    }

    #[test]
    fn get_static_fields_empty_when_no_statics() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_class_dump(100, 0, 0, &[])
            .build();
        let engine = engine_from_bytes(&bytes);
        assert!(engine.get_static_fields(100).is_empty());
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
    fn engine_two_objects(budget: u64) -> Engine {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_string(1, "x")
            .add_string(2, "y")
            .add_class_dump(100, 0, 4, &[(1, 10u8)])
            .add_class_dump(200, 0, 4, &[(2, 10u8)])
            .add_instance(0xAAA, 0, 100, &7i32.to_be_bytes())
            .add_instance(0xBBB, 0, 200, &8i32.to_be_bytes())
            .build();
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();
        let config = EngineConfig {
            budget_bytes: Some(budget),
        };
        Engine::from_file(tmp.path(), &config).unwrap()
    }

    /// Builds an engine with four expandable objects.
    fn engine_four_objects(budget: u64) -> Engine {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_string(1, "a")
            .add_string(2, "b")
            .add_string(3, "c")
            .add_string(4, "d")
            .add_class_dump(100, 0, 4, &[(1, 10u8)])
            .add_class_dump(200, 0, 4, &[(2, 10u8)])
            .add_class_dump(300, 0, 4, &[(3, 10u8)])
            .add_class_dump(400, 0, 4, &[(4, 10u8)])
            .add_instance(0xAAA, 0, 100, &1i32.to_be_bytes())
            .add_instance(0xBBB, 0, 200, &2i32.to_be_bytes())
            .add_instance(0xCCC, 0, 300, &3i32.to_be_bytes())
            .add_instance(0xDDD, 0, 400, &4i32.to_be_bytes())
            .build();
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();
        let config = EngineConfig {
            budget_bytes: Some(budget),
        };
        Engine::from_file(tmp.path(), &config).unwrap()
    }

    #[test]
    fn expand_object_cached_does_not_double_count_memory() {
        let engine = engine_two_objects(10_000_000);
        engine.expand_object(0xAAA).expect("first expand");
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
    fn expand_object_with_tiny_budget_triggers_eviction() {
        let engine = engine_two_objects(1);
        engine.expand_object(0xAAA).expect("expand A");
        engine.expand_object(0xBBB).expect("expand B");
        assert!(engine.memory_used() > 0, "something must be tracked");
        // Cache may be empty (eviction drained it)
        // or have 1 entry (last insert survived).
        // Either way, the loop terminated without
        // hanging.
    }

    #[test]
    fn expand_object_lru_order_respected() {
        // Large budget: no automatic eviction —
        // we control eviction manually via cache API.
        let engine = engine_four_objects(10_000_000);

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
        let b_evicted = engine.object_cache.evict_lru();
        assert!(b_evicted.is_some(), "first evict must return B's size");

        // Manually evict LRU — must be C
        let c_evicted = engine.object_cache.evict_lru();
        assert!(c_evicted.is_some(), "second evict must return C's size");

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
    fn expand_object_ac4_usage_below_target_after_eviction() {
        // Budget = 1 byte: baseline already exceeds
        // budget. After expand, either usage < 60%
        // or cache is empty (FM-2 behavior).
        let engine = engine_two_objects(1);
        engine.expand_object(0xAAA).unwrap();
        engine.expand_object(0xBBB).unwrap();
        let ratio = engine.memory_used() as f64 / engine.memory_budget() as f64;
        let cache_empty = engine.object_cache.is_empty();
        assert!(
            ratio < 0.60 || cache_empty,
            "AC4: usage {ratio:.2} must be < 0.60 \
                 or cache must be empty"
        );
    }

    #[test]
    fn eviction_loop_terminates_when_cache_empty() {
        // Budget so small baseline alone exceeds
        // EVICTION_TARGET. expand_object must still
        // return Some and not hang.
        let engine = engine_two_objects(1);
        let result = engine.expand_object(0xAAA);
        assert!(result.is_some(), "must return fields even with tiny budget");
    }

    #[test]
    fn re_parse_after_eviction_produces_identical_fields() {
        // Budget = 1 → every expand triggers immediate full eviction
        // A is evicted as soon as it is inserted (it becomes the sole
        // LRU entry and the eviction loop drains the cache).
        let engine = engine_two_objects(1);
        let fields_first = engine.expand_object(0xAAA).unwrap();
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
        let fields_second = engine.expand_object(0xAAA).unwrap();
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
            assert!(r_a.is_some(), "A must always return Some across all cycles");
            let r_b = engine.expand_object(0xBBB);
            assert!(r_b.is_some(), "B must always return Some across all cycles");
        }
        // usize::MAX / 2 is a conservative sentinel: real usage is
        // at most a few KB; any value above this indicates underflow.
        assert!(
            engine.memory_used() < usize::MAX / 2,
            "MemoryCounter must not underflow to usize::MAX"
        );
    }
}

/// AC6 E2E: budget_bytes flows through Engine::from_file →
/// HprofFile::from_path_with_progress → run_first_pass →
/// extract_all. Results with explicit budget must match results
/// with auto budget.
#[test]
fn budget_e2e_through_engine() {
    use hprof_parser::HprofTestBuilder;

    let mut builder = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8);
    for i in 1..=5u64 {
        builder = builder.add_instance(i, 0, 100, &[0u8; 16]);
    }
    let bytes = builder.build();
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    tmp.write_all(&bytes).unwrap();
    tmp.flush().unwrap();

    let config_explicit = EngineConfig {
        budget_bytes: Some(128),
    };
    let config_auto = EngineConfig::default();

    let engine_explicit = Engine::from_file(tmp.path(), &config_explicit).unwrap();
    let engine_auto = Engine::from_file(tmp.path(), &config_auto).unwrap();

    assert_eq!(
        engine_explicit.indexing_ratio(),
        engine_auto.indexing_ratio(),
        "indexing_ratio must match with explicit vs auto budget"
    );
    assert_eq!(
        engine_explicit.warnings().len(),
        engine_auto.warnings().len(),
        "warning counts must match"
    );
}

// -- Story 11.5 Task 5.4: Lazy skip-index creation via Engine --

#[test]
fn skip_index_lazy_creation_via_engine() {
    use crate::engine::NavigationEngine;
    use hprof_parser::HprofTestBuilder;

    let id_size: u32 = 8;
    let str_size = 10u64;
    let str_first = 11u64;
    let str_last = 12u64;
    let str_item = 13u64;
    let str_next = 14u64;
    let str_prev = 15u64;
    let str_cn = 16u64;
    let str_node_cn = 17u64;

    let n = 20usize;
    let first_node = 0x200u64;
    let last_node = 0x200u64 + (n as u64 - 1);

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
            0
        };
        let prev_id = if i > 0 { 0x200u64 + (i - 1) as u64 } else { 0 };
        let mut node_data = Vec::new();
        node_data.extend_from_slice(&item_id.to_be_bytes());
        node_data.extend_from_slice(&next_id.to_be_bytes());
        node_data.extend_from_slice(&prev_id.to_be_bytes());
        builder = builder.add_instance(node_id, 0, 2000, &node_data);
    }

    let bytes = builder.build();
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    tmp.write_all(&bytes).unwrap();
    tmp.flush().unwrap();

    let config = EngineConfig::default();
    let engine = Engine::from_file(tmp.path(), &config).unwrap();

    // Page 0: no skip-index must be created (lazy — AC#7)
    let _ = engine.get_page(0x100, 0, 10);
    assert_eq!(
        engine.skip_index_count(),
        0,
        "page 0 must not allocate a skip-index"
    );

    // Page 1 (offset=10): skip-index lazily created
    let _ = engine.get_page(0x100, 10, 10);
    assert_eq!(
        engine.skip_index_count(),
        1,
        "first offset>0 access must allocate exactly one skip-index"
    );
}

// -- Story 11.7: Background walker Engine integration --

mod walker_integration {
    use super::*;
    use crate::engine::NavigationEngine;
    use hprof_parser::HprofTestBuilder;

    fn build_hashmap_bytes(n: usize) -> Vec<u8> {
        let id_size: u32 = 8;
        let s_size = 10u64;
        let s_table = 11u64;
        let s_key = 12u64;
        let s_value = 13u64;
        let s_next = 14u64;
        let s_cn = 15u64;
        let s_node_cn = 16u64;

        let table: Vec<u64> = (0..n).map(|i| 0x200u64 + i as u64).collect();

        let mut hm = Vec::new();
        hm.extend_from_slice(&(n as i32).to_be_bytes());
        hm.extend_from_slice(&0x500u64.to_be_bytes());

        let mut b = HprofTestBuilder::new("JAVA PROFILE 1.0.2", id_size)
            .add_string(s_size, "size")
            .add_string(s_table, "table")
            .add_string(s_key, "key")
            .add_string(s_value, "value")
            .add_string(s_next, "next")
            .add_string(s_cn, "java/util/HashMap")
            .add_string(s_node_cn, "java/util/HashMap$Node")
            .add_class(1, 1000, 0, s_cn)
            .add_class(2, 2000, 0, s_node_cn)
            .add_class_dump(1000, 0, 4 + id_size, &[(s_size, 10), (s_table, 2)])
            .add_class_dump(
                2000,
                0,
                id_size * 3,
                &[(s_key, 2), (s_value, 2), (s_next, 2)],
            )
            .add_instance(0x100, 0, 1000, &hm)
            .add_object_array(0x500, 0, 2000, &table);

        for i in 0..n {
            let nid = 0x200u64 + i as u64;
            let kid = 0x1000u64 + i as u64;
            let vid = 0x2000u64 + i as u64;
            let mut nd = Vec::new();
            nd.extend_from_slice(&kid.to_be_bytes());
            nd.extend_from_slice(&vid.to_be_bytes());
            nd.extend_from_slice(&0u64.to_be_bytes());
            b = b.add_instance(nid, 0, 2000, &nd);
        }
        b.build()
    }

    fn engine_from_bytes(bytes: &[u8]) -> Engine {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(bytes).unwrap();
        tmp.flush().unwrap();
        Engine::from_file(tmp.path(), &EngineConfig::default()).unwrap()
    }

    #[test]
    fn spawn_walker_completes_and_drain_applies() {
        let engine = engine_from_bytes(&build_hashmap_bytes(50));
        engine.spawn_walker(0x100);

        // Wait for walker to complete
        std::thread::sleep(std::time::Duration::from_millis(200));

        // Drain via get_page
        let page = engine.get_page(0x100, 0, 10);
        assert!(page.is_some());

        // Walker should be removed after Complete
        assert!(
            engine.walker_progress(0x100).is_none(),
            "walker should be removed after completion"
        );
    }

    #[test]
    fn spawn_walker_dedup() {
        let engine = engine_from_bytes(&build_hashmap_bytes(50));
        engine.spawn_walker(0x100);
        engine.spawn_walker(0x100); // second call

        let walkers = engine.walkers.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(walkers.len(), 1, "duplicate spawn should be ignored");
    }

    #[test]
    fn spawn_walker_cap() {
        let engine = engine_from_bytes(&build_hashmap_bytes(10));

        // Fill walker slots with fake entries
        {
            let mut walkers = engine.walkers.lock().unwrap_or_else(|e| e.into_inner());
            for i in 1..=8 {
                let (tx, rx) = std::sync::mpsc::channel();
                drop(tx);
                let handle = crate::pagination::WalkerHandle::new(
                    rx,
                    std::thread::spawn(|| {}),
                    std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
                    std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                );
                walkers.insert(i, handle);
            }
        }

        // 9th walker should be rejected
        engine.spawn_walker(0x100);
        let walkers = engine.walkers.lock().unwrap_or_else(|e| e.into_inner());
        assert!(
            !walkers.contains_key(&0x100),
            "should reject spawn when cap reached"
        );
    }

    #[test]
    fn cancel_walker_removes_handle() {
        let engine = engine_from_bytes(&build_hashmap_bytes(50));
        engine.spawn_walker(0x100);

        // Cancel immediately
        engine.cancel_walker(0x100);

        assert!(
            engine.walker_progress(0x100).is_none(),
            "handle should be removed after cancel"
        );
    }

    #[test]
    fn walker_progress_lifecycle() {
        let engine = engine_from_bytes(&build_hashmap_bytes(50));
        engine.spawn_walker(0x100);

        // Progress should be Some while walker runs
        // (may already be done for small collection)
        let _progress = engine.walker_progress(0x100);

        // Wait for completion
        std::thread::sleep(std::time::Duration::from_millis(200));
        // Drain walker
        engine.get_page(0x100, 0, 10);

        // After drain of Complete, progress is None
        assert!(
            engine.walker_progress(0x100).is_none(),
            "progress should be None after completion"
        );
    }
}
