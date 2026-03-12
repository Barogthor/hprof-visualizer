use hprof_engine::{
    CollectionPage, EntryInfo, FieldInfo, FieldValue, FrameInfo, LineNumber, NavigationEngine,
    ThreadInfo, ThreadState, VariableInfo, VariableValue,
};
use std::collections::HashMap;

use super::*;

struct StubEngine {
    threads: Vec<ThreadInfo>,
    frames: Vec<FrameInfo>,
    frames_by_thread: HashMap<u32, Vec<FrameInfo>>,
    vars_by_frame: HashMap<u64, Vec<VariableInfo>>,
    expand_results: HashMap<u64, Option<Vec<FieldInfo>>>,
    class_by_object: HashMap<u64, u64>,
    static_by_class: HashMap<u64, Vec<FieldInfo>>,
}

impl StubEngine {
    fn with_threads(names: &[&str]) -> Self {
        Self {
            threads: names
                .iter()
                .enumerate()
                .map(|(i, &n)| ThreadInfo {
                    thread_serial: (i + 1) as u32,
                    name: n.to_string(),
                    state: ThreadState::Unknown,
                })
                .collect(),
            frames: vec![],
            frames_by_thread: HashMap::new(),
            vars_by_frame: HashMap::new(),
            expand_results: HashMap::new(),
            class_by_object: HashMap::new(),
            static_by_class: HashMap::new(),
        }
    }

    fn with_threads_and_frames(names: &[&str], frames: Vec<FrameInfo>) -> Self {
        let mut s = Self::with_threads(names);
        s.frames = frames;
        s
    }

    fn with_thread_specific_frames(names: &[&str], by_thread: &[(u32, Vec<FrameInfo>)]) -> Self {
        let mut s = Self::with_threads(names);
        s.frames_by_thread = by_thread
            .iter()
            .map(|(serial, frames)| (*serial, frames.clone()))
            .collect();
        s
    }

    fn with_vars(mut self, frame_id: u64, vars: Vec<VariableInfo>) -> Self {
        self.vars_by_frame.insert(frame_id, vars);
        self
    }

    fn with_expand(mut self, oid: u64, fields: Option<Vec<FieldInfo>>) -> Self {
        self.expand_results.insert(oid, fields);
        self
    }

    fn with_class_of(mut self, object_id: u64, class_id: u64) -> Self {
        self.class_by_object.insert(object_id, class_id);
        self
    }

    fn with_static_fields(mut self, class_id: u64, fields: Vec<FieldInfo>) -> Self {
        self.static_by_class.insert(class_id, fields);
        self
    }
}

impl NavigationEngine for StubEngine {
    fn warnings(&self) -> &[String] {
        &[]
    }
    fn list_threads(&self) -> Vec<ThreadInfo> {
        self.threads.clone()
    }
    fn select_thread(&self, serial: u32) -> Option<ThreadInfo> {
        self.threads
            .iter()
            .find(|t| t.thread_serial == serial)
            .cloned()
    }
    fn get_stack_frames(&self, thread_serial: u32) -> Vec<FrameInfo> {
        self.frames_by_thread
            .get(&thread_serial)
            .cloned()
            .unwrap_or_else(|| self.frames.clone())
    }
    fn get_local_variables(&self, frame_id: u64) -> Vec<VariableInfo> {
        self.vars_by_frame
            .get(&frame_id)
            .cloned()
            .unwrap_or_default()
    }
    fn expand_object(&self, oid: u64) -> Option<Vec<FieldInfo>> {
        if let Some(result) = self.expand_results.get(&oid) {
            return result.clone();
        }
        Some(vec![
            FieldInfo {
                name: "x".to_string(),
                value: FieldValue::Int(42),
            },
            FieldInfo {
                name: "child".to_string(),
                value: FieldValue::ObjectRef {
                    id: 999,
                    class_name: "Child".to_string(),
                    entry_count: None,
                    inline_value: None,
                },
            },
        ])
    }
    fn class_of_object(&self, object_id: u64) -> Option<u64> {
        self.class_by_object.get(&object_id).copied()
    }
    fn get_static_fields(&self, class_object_id: u64) -> Vec<FieldInfo> {
        self.static_by_class
            .get(&class_object_id)
            .cloned()
            .unwrap_or_default()
    }
    fn get_page(&self, collection_id: u64, offset: usize, limit: usize) -> Option<CollectionPage> {
        match collection_id {
            888 => {
                // Int entries
                let total: u64 = 250;
                let end = (offset + limit).min(total as usize);
                let entries = (offset..end)
                    .map(|i| EntryInfo {
                        index: i,
                        key: None,
                        value: FieldValue::Int(i as i32),
                    })
                    .collect();
                Some(CollectionPage {
                    entries,
                    total_count: total,
                    offset,
                    has_more: end < total as usize,
                })
            }
            889 => {
                // ObjectRef entries — used for AC10 expansion tests.
                // Entry 0 has value ObjectRef(id=700), entry 1 etc.
                let total: u64 = 3;
                let end = (offset + limit).min(total as usize);
                let entries = (offset..end)
                    .map(|i| EntryInfo {
                        index: i,
                        key: None,
                        value: FieldValue::ObjectRef {
                            id: 700 + i as u64,
                            class_name: "SomeItem".to_string(),
                            entry_count: None,
                            inline_value: None,
                        },
                    })
                    .collect();
                Some(CollectionPage {
                    entries,
                    total_count: total,
                    offset,
                    has_more: end < total as usize,
                })
            }
            890 => {
                // One nested collection entry: value ObjectRef(id=888)
                // with entry_count set so Enter dispatches StartCollection.
                let total: u64 = 1;
                let end = (offset + limit).min(total as usize);
                let entries = (offset..end)
                    .map(|i| EntryInfo {
                        index: i,
                        key: None,
                        value: FieldValue::ObjectRef {
                            id: 888,
                            class_name: "Object[]".to_string(),
                            entry_count: Some(250),
                            inline_value: None,
                        },
                    })
                    .collect();
                Some(CollectionPage {
                    entries,
                    total_count: total,
                    offset,
                    has_more: end < total as usize,
                })
            }
            _ => None,
        }
    }
    fn resolve_string(&self, _: u64) -> Option<String> {
        Some("value".to_string())
    }
    fn memory_used(&self) -> usize {
        0
    }
    fn memory_budget(&self) -> u64 {
        u64::MAX
    }
    fn indexing_ratio(&self) -> f64 {
        100.0
    }
    fn is_fully_indexed(&self) -> bool {
        true
    }
    fn skeleton_bytes(&self) -> usize {
        0
    }
}

fn make_frame(frame_id: u64) -> FrameInfo {
    FrameInfo {
        frame_id,
        method_name: "run".to_string(),
        class_name: "Thread".to_string(),
        source_file: "Thread.java".to_string(),
        line: LineNumber::Line(1),
        has_variables: false,
    }
}

fn make_obj_var(index: usize, object_id: u64) -> VariableInfo {
    VariableInfo {
        index,
        value: VariableValue::ObjectRef {
            id: object_id,
            class_name: "Object".to_string(),
            entry_count: None,
        },
    }
}

#[test]
fn app_new_builds_without_panic_with_zero_threads() {
    let engine = StubEngine::with_threads(&[]);
    let app = App::new(engine, "test.hprof".to_string());
    assert_eq!(app.focus, Focus::ThreadList);
    assert_eq!(app.thread_list.selected_serial(), None);
    assert_eq!(app.thread_count, 0);
}

#[test]
fn app_new_builds_without_panic_with_three_threads() {
    let engine = StubEngine::with_threads(&["main", "worker-1", "worker-2"]);
    let app = App::new(engine, "test.hprof".to_string());
    assert_eq!(app.thread_list.selected_serial(), Some(1));
    assert_eq!(app.thread_count, 3);
}

#[test]
fn handle_input_down_in_thread_list_updates_selection() {
    let engine = StubEngine::with_threads(&["main", "worker-1", "worker-2"]);
    let mut app = App::new(engine, "test.hprof".to_string());
    app.handle_input(InputEvent::Down);
    assert_eq!(app.thread_list.selected_serial(), Some(2));
}

#[test]
fn handle_input_search_activate_sets_search_active() {
    let engine = StubEngine::with_threads(&["main"]);
    let mut app = App::new(engine, "test.hprof".to_string());
    app.handle_input(InputEvent::SearchActivate);
    assert!(app.thread_list.is_search_active());
}

#[test]
fn handle_input_search_char_appends_to_filter_query() {
    let engine = StubEngine::with_threads(&["main", "worker"]);
    let mut app = App::new(engine, "test.hprof".to_string());
    app.handle_input(InputEvent::SearchActivate);
    app.handle_input(InputEvent::SearchChar('w'));
    app.handle_input(InputEvent::SearchChar('o'));
    assert_eq!(app.thread_list.filter(), "wo");
    assert_eq!(app.thread_list.filtered_count(), 1);
}

#[test]
fn handle_input_escape_in_search_mode_clears_filter_and_deactivates() {
    let engine = StubEngine::with_threads(&["main", "worker"]);
    let mut app = App::new(engine, "test.hprof".to_string());
    app.handle_input(InputEvent::SearchActivate);
    app.handle_input(InputEvent::SearchChar('w'));
    app.handle_input(InputEvent::Escape);
    assert!(!app.thread_list.is_search_active());
    assert_eq!(app.thread_list.filtered_count(), 2);
}

#[test]
fn handle_input_enter_in_thread_list_loads_frames_and_transitions_to_stack_frames() {
    let frames = vec![make_frame(10), make_frame(20)];
    let engine = StubEngine::with_threads_and_frames(&["main"], frames);
    let mut app = App::new(engine, "test.hprof".to_string());
    app.handle_input(InputEvent::Enter);
    assert_eq!(app.focus, Focus::StackFrames);
    let ss = app.stack_state.as_ref().expect("stack_state must be Some");
    assert_eq!(ss.selected_frame_id(), Some(10));
}

#[test]
fn handle_input_up_down_in_stack_frames_moves_cursor() {
    let frames = vec![make_frame(10), make_frame(20), make_frame(30)];
    let engine = StubEngine::with_threads_and_frames(&["main"], frames);
    let mut app = App::new(engine, "test.hprof".to_string());
    app.handle_input(InputEvent::Enter); // → StackFrames
    app.handle_input(InputEvent::Down);
    assert_eq!(
        app.stack_state.as_ref().unwrap().selected_frame_id(),
        Some(20)
    );
    app.handle_input(InputEvent::Up);
    assert_eq!(
        app.stack_state.as_ref().unwrap().selected_frame_id(),
        Some(10)
    );
}

#[test]
fn handle_input_enter_in_stack_frames_expands_then_collapses() {
    let frames = vec![make_frame(10)];
    let engine = StubEngine::with_threads_and_frames(&["main"], frames);
    let mut app = App::new(engine, "test.hprof".to_string());
    app.handle_input(InputEvent::Enter); // → StackFrames, Enter on collapsed frame → expands
    app.handle_input(InputEvent::Enter);
    assert!(app.stack_state.as_ref().unwrap().is_expanded(10));
    // Enter on expanded frame → collapses
    app.handle_input(InputEvent::Enter);
    assert!(!app.stack_state.as_ref().unwrap().is_expanded(10));
}

#[test]
fn handle_input_escape_in_stack_frames_clears_state_and_returns_to_thread_list() {
    let frames = vec![make_frame(10)];
    let engine = StubEngine::with_threads_and_frames(&["main"], frames);
    let mut app = App::new(engine, "test.hprof".to_string());
    app.handle_input(InputEvent::Enter);
    assert_eq!(app.focus, Focus::StackFrames);
    app.handle_input(InputEvent::Escape);
    assert_eq!(app.focus, Focus::ThreadList);
    assert!(app.stack_state.is_none());
}

#[test]
fn handle_input_quit_returns_app_action_quit() {
    let engine = StubEngine::with_threads(&["main"]);
    let mut app = App::new(engine, "test.hprof".to_string());
    assert_eq!(app.handle_input(InputEvent::Quit), AppAction::Quit);
}

#[test]
fn app_new_captures_thread_count_without_repeated_list_calls() {
    let engine = StubEngine::with_threads(&["a", "b", "c"]);
    let app = App::new(engine, "x.hprof".to_string());
    assert_eq!(app.thread_count, 3);
}

#[test]
fn handle_input_enter_with_no_selected_thread_does_not_transition() {
    let engine = StubEngine::with_threads(&[]);
    let mut app = App::new(engine, "test.hprof".to_string());
    app.handle_input(InputEvent::Enter);
    assert_eq!(app.focus, Focus::ThreadList);
    assert!(app.stack_state.is_none());
}

#[test]
fn stack_state_is_none_on_construction() {
    let engine = StubEngine::with_threads(&["main"]);
    let app = App::new(engine, "test.hprof".to_string());
    assert!(app.stack_state.is_none());
}

#[test]
fn app_new_initializes_stack_preview_for_selected_thread() {
    let engine = StubEngine::with_thread_specific_frames(
        &["main", "worker"],
        &[(1, vec![make_frame(10)]), (2, vec![make_frame(20)])],
    );
    let app = App::new(engine, "test.hprof".to_string());
    assert_eq!(app.focus, Focus::ThreadList);
    assert!(app.stack_state.is_none());
    assert_eq!(app.preview_stack_state.selected_frame_id(), Some(10));
}

#[test]
fn moving_thread_selection_updates_stack_preview_without_enter() {
    let engine = StubEngine::with_thread_specific_frames(
        &["main", "worker"],
        &[(1, vec![make_frame(10)]), (2, vec![make_frame(20)])],
    );
    let mut app = App::new(engine, "test.hprof".to_string());
    app.handle_input(InputEvent::Down);
    assert_eq!(app.focus, Focus::ThreadList);
    assert!(app.stack_state.is_none());
    assert_eq!(app.preview_stack_state.selected_frame_id(), Some(20));
}

#[test]
fn variable_value_variants_accessible_via_hprof_engine() {
    let v = VariableValue::Null;
    assert_eq!(v, VariableValue::Null);
}

// --- Task 9: async expansion machinery tests ---

#[test]
fn start_object_expansion_registers_pending_but_no_loading_before_threshold() {
    let frames = vec![make_frame(10)];
    let vars = vec![make_obj_var(0, 42)];
    let engine = StubEngine::with_threads_and_frames(&["main"], frames).with_vars(10, vars);
    let mut app = App::new(engine, "test.hprof".to_string());
    // Enter StackFrames, expand frame 10, then move down to the ObjectRef var.
    app.handle_input(InputEvent::Enter); // → StackFrames, OnFrame(0)
    app.handle_input(InputEvent::Enter); // expand frame 10
    app.handle_input(InputEvent::Down); // → OnVar{0,0} (ObjectRef 42)
    app.handle_input(InputEvent::Enter); // start_object_expansion(42), loading indicator is NOT shown yet.
    assert!(
        app.pending_expansions.contains_key(&42),
        "pending expansion must be registered"
    );
    assert_eq!(
        app.stack_state.as_ref().unwrap().expansion_state(42),
        ExpansionPhase::Collapsed,
        "loading must not be shown before threshold"
    );
}

#[test]
fn poll_expansions_completes_and_moves_to_expanded() {
    let frames = vec![make_frame(10)];
    let vars = vec![make_obj_var(0, 42)];
    let engine = StubEngine::with_threads_and_frames(&["main"], frames).with_vars(10, vars);
    let mut app = App::new(engine, "test.hprof".to_string());
    app.handle_input(InputEvent::Enter); // → StackFrames
    app.handle_input(InputEvent::Enter); // expand frame 10
    app.handle_input(InputEvent::Down); // → OnVar{0,0}
    app.handle_input(InputEvent::Enter); // start expansion; poll until worker finishes.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !app.pending_expansions.is_empty() && std::time::Instant::now() < deadline {
        app.poll_expansions();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(
        app.stack_state.as_ref().unwrap().expansion_state(42),
        ExpansionPhase::Expanded
    );
}

// --- Task 6.4: nested expansion tests ---

#[test]
fn enter_on_nested_object_field_starts_expansion() {
    // StubEngine.expand_object returns a field "child" ObjectRef(999)
    let frames = vec![make_frame(10)];
    let vars = vec![make_obj_var(0, 42)];
    let engine = StubEngine::with_threads_and_frames(&["main"], frames).with_vars(10, vars);
    let mut app = App::new(engine, "test.hprof".to_string());
    app.handle_input(InputEvent::Enter); // → StackFrames
    app.handle_input(InputEvent::Enter); // expand frame 10
    app.handle_input(InputEvent::Down); // → OnVar{0,0}
    app.handle_input(InputEvent::Enter); // start expansion of object 42, then poll until complete
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !app.pending_expansions.is_empty() && std::time::Instant::now() < deadline {
        app.poll_expansions();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    // Object 42 expanded, has "child" field (ObjectRef 999) at index 1
    assert_eq!(
        app.stack_state.as_ref().unwrap().expansion_state(42),
        ExpansionPhase::Expanded
    );
    // Navigate down to the "child" field (index 1 in flat list: field_path=[1])
    app.handle_input(InputEvent::Down); // → OnObjectField{0,0,[0]} (field "x")
    app.handle_input(InputEvent::Down); // → OnObjectField{0,0,[1]} (field "child" = ObjectRef 999)
    app.handle_input(InputEvent::Enter); // start nested expansion of 999; loading not shown before threshold.
    assert!(
        app.pending_expansions.contains_key(&999),
        "pending expansion for 999 must be registered"
    );
    assert_ne!(
        app.stack_state.as_ref().unwrap().expansion_state(999),
        ExpansionPhase::Loading,
        "loading must not be shown before threshold"
    );
}

#[test]
fn collapse_object_recursive_called_on_enter_for_expanded_root_obj() {
    let frames = vec![make_frame(10)];
    let vars = vec![make_obj_var(0, 42)];
    let engine = StubEngine::with_threads_and_frames(&["main"], frames).with_vars(10, vars);
    let mut app = App::new(engine, "test.hprof".to_string());
    app.handle_input(InputEvent::Enter); // → StackFrames
    app.handle_input(InputEvent::Enter); // expand frame 10
    app.handle_input(InputEvent::Down); // → OnVar{0,0}
    app.handle_input(InputEvent::Enter); // start expansion
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !app.pending_expansions.is_empty() && std::time::Instant::now() < deadline {
        app.poll_expansions();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    // Now collapse via Enter on OnVar (expanded state)
    app.handle_input(InputEvent::Enter); // CollapseObj(42)
    assert_eq!(
        app.stack_state.as_ref().unwrap().expansion_state(42),
        ExpansionPhase::Collapsed
    );
}

#[test]
fn enter_twice_on_nested_object_field_collapses_it() {
    // StubEngine.expand_object always returns [x:Int, child:ObjectRef(999)]
    let frames = vec![make_frame(10)];
    let vars = vec![make_obj_var(0, 42)];
    let engine = StubEngine::with_threads_and_frames(&["main"], frames).with_vars(10, vars);
    let mut app = App::new(engine, "test.hprof".to_string());
    app.handle_input(InputEvent::Enter); // → StackFrames
    app.handle_input(InputEvent::Enter); // expand frame 10
    app.handle_input(InputEvent::Down); // → OnVar{0,0}
    app.handle_input(InputEvent::Enter); // start expansion of object 42
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !app.pending_expansions.is_empty() && std::time::Instant::now() < deadline {
        app.poll_expansions();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    // Navigate to "child" field at path [1] (ObjectRef 999)
    app.handle_input(InputEvent::Down); // → OnObjectField{0,0,[0]} ("x")
    app.handle_input(InputEvent::Down); // → OnObjectField{0,0,[1]} ("child" = 999)
    app.handle_input(InputEvent::Enter); // start nested expansion of 999
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !app.pending_expansions.is_empty() && std::time::Instant::now() < deadline {
        app.poll_expansions();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(
        app.stack_state.as_ref().unwrap().expansion_state(999),
        ExpansionPhase::Expanded
    );
    // Enter again on the same field → CollapseNestedObj(999)
    app.handle_input(InputEvent::Enter);
    assert_eq!(
        app.stack_state.as_ref().unwrap().expansion_state(999),
        ExpansionPhase::Collapsed
    );
}

#[test]
fn enter_on_static_object_field_starts_and_collapses_expansion() {
    let frames = vec![make_frame(10)];
    let vars = vec![make_obj_var(0, 42)];
    let engine = StubEngine::with_threads_and_frames(&["main"], frames)
        .with_vars(10, vars)
        .with_expand(
            42,
            Some(vec![FieldInfo {
                name: "x".to_string(),
                value: FieldValue::Int(1),
            }]),
        )
        .with_class_of(42, 500)
        .with_static_fields(
            500,
            vec![FieldInfo {
                name: "S_CHILD".to_string(),
                value: FieldValue::ObjectRef {
                    id: 777,
                    class_name: "Child".to_string(),
                    entry_count: None,
                    inline_value: None,
                },
            }],
        )
        .with_expand(
            777,
            Some(vec![FieldInfo {
                name: "leaf".to_string(),
                value: FieldValue::Int(9),
            }]),
        );
    let mut app = App::new(engine, "test.hprof".to_string());

    app.handle_input(InputEvent::Enter); // -> StackFrames
    app.handle_input(InputEvent::Enter); // expand frame 10
    app.handle_input(InputEvent::Down); // -> OnVar{0,0}
    app.handle_input(InputEvent::Enter); // expand object 42

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !app.pending_expansions.is_empty() && std::time::Instant::now() < deadline {
        app.poll_expansions();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }

    // OnObjectField([0]) then OnStaticField([0]).
    app.handle_input(InputEvent::Down);
    app.handle_input(InputEvent::Down);
    assert!(matches!(
        app.stack_state.as_ref().unwrap().cursor(),
        StackCursor::OnStaticField { static_idx: 0, .. }
    ));

    app.handle_input(InputEvent::Enter); // expand static object ref 777
    assert!(
        app.pending_expansions.contains_key(&777),
        "pending expansion for static object 777 must be registered"
    );

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !app.pending_expansions.is_empty() && std::time::Instant::now() < deadline {
        app.poll_expansions();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }

    assert_eq!(
        app.stack_state.as_ref().unwrap().expansion_state(777),
        ExpansionPhase::Expanded
    );

    app.handle_input(InputEvent::Enter); // collapse static object ref 777
    assert_eq!(
        app.stack_state.as_ref().unwrap().expansion_state(777),
        ExpansionPhase::Collapsed
    );
}

#[test]
fn escape_on_loading_node_cancels_expansion_without_leaving_stack_frames() {
    let frames = vec![make_frame(10)];
    let vars = vec![make_obj_var(0, 42)];
    let engine = StubEngine::with_threads_and_frames(&["main"], frames).with_vars(10, vars);
    let mut app = App::new(engine, "test.hprof".to_string());
    app.handle_input(InputEvent::Enter); // → StackFrames
    app.handle_input(InputEvent::Enter); // expand frame 10
    app.handle_input(InputEvent::Down); // → OnVar{0,0}
    // Inject a still-pending expansion to avoid races with fast worker completion.
    let (_tx, rx) = mpsc::channel::<Option<Vec<FieldInfo>>>();
    app.pending_expansions.insert(
        42,
        PendingExpansion {
            rx,
            started: Instant::now() - EXPANSION_LOADING_THRESHOLD - Duration::from_millis(10),
            loading_shown: false,
        },
    );
    // Poll once — this triggers the Loading state deterministically.
    app.poll_expansions();
    assert_eq!(
        app.stack_state.as_ref().unwrap().expansion_state(42),
        ExpansionPhase::Loading
    );
    app.handle_input(InputEvent::Down); // → OnObjectLoadingNode{0,0}
    app.handle_input(InputEvent::Escape); // cancel expansion (not go-back)
    assert_eq!(
        app.stack_state.as_ref().unwrap().expansion_state(42),
        ExpansionPhase::Collapsed
    );
    // Focus must remain in StackFrames.
    assert_eq!(app.focus, Focus::StackFrames);
}

// --- Collection pagination tests ---

/// Builds an App with a frame that has one var
/// (object 42) whose expand returns a field "items"
/// = ObjectRef(888, ArrayList, entry_count=ec).
/// StubEngine.get_page(888, ..) returns test entries.
fn make_var_collection_app(ec: u64) -> App<StubEngine> {
    let frames = vec![{
        let mut f = make_frame(10);
        f.has_variables = true;
        f
    }];
    let vars = vec![VariableInfo {
        index: 0,
        value: VariableValue::ObjectRef {
            id: 888,
            class_name: "Object[]".to_string(),
            entry_count: Some(ec),
        },
    }];
    let engine = StubEngine::with_threads_and_frames(&["main"], frames).with_vars(10, vars);
    App::new(engine, "test.hprof".to_string())
}

/// Builds an App with a frame that has one var
/// (object 42) whose expand returns a field "items"
/// = ObjectRef(888, ArrayList, entry_count=ec).
/// StubEngine.get_page(888, ..) returns test entries.
fn make_collection_app(ec: u64) -> App<StubEngine> {
    let frames = vec![{
        let mut f = make_frame(10);
        f.has_variables = true;
        f
    }];
    let vars = vec![make_obj_var(0, 42)];
    let expand_fields = Some(vec![FieldInfo {
        name: "items".to_string(),
        value: FieldValue::ObjectRef {
            id: 888,
            class_name: "java.util.ArrayList".to_string(),
            entry_count: Some(ec),
            inline_value: None,
        },
    }]);
    let engine = StubEngine::with_threads_and_frames(&["main"], frames)
        .with_vars(10, vars)
        .with_expand(42, expand_fields);
    App::new(engine, "test.hprof".to_string())
}

/// Navigate from thread list into the collection
/// field. Returns app positioned on the "items"
/// ObjectField.
fn nav_to_collection_field(app: &mut App<StubEngine>) {
    app.handle_input(InputEvent::Enter); // StackFrames
    app.handle_input(InputEvent::Enter); // expand frame
    app.handle_input(InputEvent::Down); // → OnVar
    app.handle_input(InputEvent::Enter); // expand obj 42
    poll_all_expansions(app);
    app.handle_input(InputEvent::Down); // → items field
}

fn poll_all_pages(app: &mut App<StubEngine>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !app.pending_pages.is_empty() && std::time::Instant::now() < deadline {
        let _fallbacks = app.poll_pages();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

fn poll_all_expansions(app: &mut App<StubEngine>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !app.pending_expansions.is_empty() && std::time::Instant::now() < deadline {
        app.poll_expansions();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[test]
fn collection_enter_triggers_get_page_not_expand() {
    let mut app = make_collection_app(250);
    nav_to_collection_field(&mut app);
    // Enter on collection field → StartCollection
    app.handle_input(InputEvent::Enter);
    // Should have pending_pages, not pending_expansions
    // for collection 888.
    assert!(
        !app.pending_pages.is_empty(),
        "collection load should be pending"
    );
    assert!(
        !app.pending_expansions.contains_key(&888),
        "should NOT use expand_object for collection"
    );
}

#[test]
fn collection_small_no_chunk_sections() {
    let mut app = make_collection_app(50);
    nav_to_collection_field(&mut app);
    app.handle_input(InputEvent::Enter);
    poll_all_pages(&mut app);
    let ss = app.stack_state.as_ref().unwrap();
    let cc = ss.expansion.collection_chunks.get(&888).unwrap();
    assert!(cc.eager_page.is_some());
    assert_eq!(cc.eager_page.as_ref().unwrap().entries.len(), 50);
    assert!(
        cc.chunk_pages.is_empty(),
        "<= 100 entries → no chunk sections"
    );
}

#[test]
fn collection_250_chunk_layout() {
    let mut app = make_collection_app(250);
    nav_to_collection_field(&mut app);
    app.handle_input(InputEvent::Enter);
    poll_all_pages(&mut app);
    let ss = app.stack_state.as_ref().unwrap();
    let cc = ss.expansion.collection_chunks.get(&888).unwrap();
    // Eager page: 100 entries.
    assert_eq!(cc.eager_page.as_ref().unwrap().entries.len(), 100);
    // Chunk sections: [100..199], [200..249].
    let ranges = compute_chunk_ranges(250);
    assert_eq!(ranges.len(), 2);
    assert_eq!(ranges[0], (100, 100));
    assert_eq!(ranges[1], (200, 50));
    // All chunk sections start as Collapsed.
    for (offset, _) in &ranges {
        assert!(matches!(
            cc.chunk_pages.get(offset),
            Some(ChunkState::Collapsed)
        ));
    }
}

#[test]
fn collection_3000_chunk_layout() {
    let ranges = compute_chunk_ranges(3000);
    // 9 sections of 100 + 2 sections of 1000
    assert_eq!(ranges.len(), 11);
    assert_eq!(ranges[0], (100, 100));
    assert_eq!(ranges[8], (900, 100));
    assert_eq!(ranges[9], (1000, 1000));
    assert_eq!(ranges[10], (2000, 1000));
}

#[test]
fn collection_2348_last_chunk_truncated() {
    let ranges = compute_chunk_ranges(2348);
    let last = ranges.last().unwrap();
    assert_eq!(*last, (2000, 348));
}

#[test]
fn chunk_section_enter_loads_correct_range() {
    let mut app = make_collection_app(250);
    nav_to_collection_field(&mut app);
    app.handle_input(InputEvent::Enter);
    poll_all_pages(&mut app);
    // Navigate down past eager entries to first chunk
    // section: 100 entries + 1 to pass them.
    for _ in 0..101 {
        app.handle_input(InputEvent::Down);
    }
    let ss = app.stack_state.as_ref().unwrap();
    // Should be on first chunk section [100..199].
    assert!(matches!(
        ss.cursor(),
        StackCursor::OnChunkSection {
            chunk_offset: 100,
            ..
        }
    ));
    // Enter on chunk → LoadChunk(888, 100, 100).
    app.handle_input(InputEvent::Enter);
    assert!(
        app.pending_pages.contains_key(&(888, 100)),
        "chunk load should be pending"
    );
    poll_all_pages(&mut app);
    let ss = app.stack_state.as_ref().unwrap();
    let cc = ss.expansion.collection_chunks.get(&888).unwrap();
    assert!(matches!(
        cc.chunk_pages.get(&100),
        Some(ChunkState::Loaded(_))
    ));
}

#[test]
fn chunk_loading_indicator() {
    let mut app = make_collection_app(250);
    nav_to_collection_field(&mut app);
    app.handle_input(InputEvent::Enter);
    poll_all_pages(&mut app);
    // Navigate to first chunk.
    for _ in 0..101 {
        app.handle_input(InputEvent::Down);
    }
    app.handle_input(InputEvent::Enter);
    // Before threshold, chunk is not in Loading state (still Collapsed or absent).
    {
        let ss = app.stack_state.as_ref().unwrap();
        let cc = ss.expansion.collection_chunks.get(&888).unwrap();
        assert!(
            !matches!(cc.chunk_pages.get(&100), Some(ChunkState::Loading)),
            "chunk must NOT be Loading before threshold"
        );
    }
    // Simulate threshold elapsed.
    if let Some(pp) = app.pending_pages.get_mut(&(888, 100)) {
        pp.started = Instant::now() - EXPANSION_LOADING_THRESHOLD - Duration::from_millis(10);
    }
    // Poll once — threshold triggers ChunkState::Loading.
    app.poll_pages();
    let ss = app.stack_state.as_ref().unwrap();
    let cc = ss.expansion.collection_chunks.get(&888).unwrap();
    assert!(
        matches!(cc.chunk_pages.get(&100), Some(ChunkState::Loading)),
        "chunk must be Loading after threshold"
    );
}

#[test]
fn first_collection_page_shows_loading_indicator_after_threshold() {
    let mut app = make_collection_app(250);
    nav_to_collection_field(&mut app);
    // Manually inject a PendingPage with an unsent channel so try_recv()
    // returns Empty (no real thread spawned that would return immediately).
    let (_tx, rx) = mpsc::channel::<Option<CollectionPage>>();
    app.pending_pages.insert(
        (888, 0),
        PendingPage {
            rx,
            started: Instant::now() - EXPANSION_LOADING_THRESHOLD - Duration::from_millis(10),
            loading_shown: false,
        },
    );
    // Before polling: expansion_state must not be Loading yet.
    {
        let ss = app.stack_state.as_ref().unwrap();
        assert_eq!(
            ss.expansion_state(888),
            ExpansionPhase::Collapsed,
            "before poll, collection must not show loading"
        );
    }
    // One poll — threshold exceeded → set_expansion_loading(888).
    app.poll_pages();
    let ss = app.stack_state.as_ref().unwrap();
    assert_eq!(
        ss.expansion_state(888),
        ExpansionPhase::Loading,
        "after threshold, eager page load must show loading"
    );
}

#[test]
fn escape_collapses_collection() {
    let mut app = make_collection_app(250);
    nav_to_collection_field(&mut app);
    app.handle_input(InputEvent::Enter);
    poll_all_pages(&mut app);
    // Move into collection entries.
    app.handle_input(InputEvent::Down);
    let ss = app.stack_state.as_ref().unwrap();
    assert!(matches!(ss.cursor(), StackCursor::OnCollectionEntry { .. }));
    // Escape → collapse collection.
    app.handle_input(InputEvent::Escape);
    let ss = app.stack_state.as_ref().unwrap();
    assert!(
        !ss.expansion.collection_chunks.contains_key(&888),
        "collection should be removed"
    );
    // Cursor returns to the collection field.
    assert!(matches!(ss.cursor(), StackCursor::OnObjectField { .. }));
    // Focus stays in StackFrames.
    assert_eq!(app.focus, Focus::StackFrames);
}

#[test]
fn escape_from_collection_opened_on_var_restores_on_var_cursor() {
    let mut app = make_var_collection_app(250);
    app.handle_input(InputEvent::Enter); // StackFrames
    app.handle_input(InputEvent::Enter); // expand frame
    app.handle_input(InputEvent::Down); // -> OnVar{0,0}
    app.handle_input(InputEvent::Enter); // open collection 888 from var
    poll_all_pages(&mut app);
    assert!(
        app.stack_state
            .as_ref()
            .unwrap()
            .expansion
            .collection_chunks
            .contains_key(&888),
        "collection 888 should be loaded before testing escape"
    );

    app.handle_input(InputEvent::Down); // -> first collection entry
    let ss = app.stack_state.as_ref().unwrap();
    assert!(
        matches!(ss.cursor(), StackCursor::OnCollectionEntry { .. }),
        "expected collection entry cursor before escape, got {:?}",
        ss.cursor()
    );

    app.handle_input(InputEvent::Escape);
    let ss = app.stack_state.as_ref().unwrap();
    assert!(
        !ss.expansion.collection_chunks.contains_key(&888),
        "collection should be removed"
    );
    assert_eq!(
        ss.cursor(),
        &StackCursor::OnVar {
            frame_idx: 0,
            var_idx: 0
        },
        "escape from var-opened collection must restore OnVar"
    );
}

#[test]
fn var_prim_array_triggers_collection_paging_not_expand() {
    // Regression: var with entry_count=Some(5) and class_name="int[]"
    // must dispatch StartCollection (pending_pages), not expand_object.
    let frames = vec![{
        let mut f = make_frame(10);
        f.has_variables = true;
        f
    }];
    let vars = vec![VariableInfo {
        index: 0,
        value: VariableValue::ObjectRef {
            id: 888,
            class_name: "int[]".to_string(),
            entry_count: Some(5),
        },
    }];
    let engine = StubEngine::with_threads_and_frames(&["main"], frames).with_vars(10, vars);
    let mut app = App::new(engine, "test.hprof".to_string());
    app.handle_input(InputEvent::Enter); // → StackFrames
    app.handle_input(InputEvent::Enter); // expand frame
    app.handle_input(InputEvent::Down); // → OnVar{0,0}
    app.handle_input(InputEvent::Enter); // must start collection paging, not expand_object
    assert!(
        app.pending_pages.contains_key(&(888, 0)),
        "prim array var with entry_count must trigger collection paging"
    );
    assert!(
        !app.pending_expansions.contains_key(&888),
        "prim array var must not call expand_object"
    );
    poll_all_pages(&mut app);
    let ss = app.stack_state.as_ref().unwrap();
    assert!(
        ss.expansion.collection_chunks.contains_key(&888),
        "collection chunks must be present after polling"
    );
}

#[test]
fn escape_from_chunk_section_collapses_collection() {
    let mut app = make_collection_app(250);
    nav_to_collection_field(&mut app);
    app.handle_input(InputEvent::Enter);
    poll_all_pages(&mut app);
    // Navigate to first chunk section (past 100 eager entries + 1 entry node).
    for _ in 0..101 {
        app.handle_input(InputEvent::Down);
    }
    let ss = app.stack_state.as_ref().unwrap();
    assert!(
        matches!(ss.cursor(), StackCursor::OnChunkSection { .. }),
        "should be on chunk section, got {:?}",
        ss.cursor()
    );
    // Escape from chunk section should collapse the collection.
    app.handle_input(InputEvent::Escape);
    let ss = app.stack_state.as_ref().unwrap();
    assert!(
        !ss.expansion.collection_chunks.contains_key(&888),
        "collection should be removed after escape from chunk section"
    );
    assert!(matches!(ss.cursor(), StackCursor::OnObjectField { .. }));
}

#[test]
fn unsupported_type_falls_back_to_expand_object() {
    // Use collection ID 777 which StubEngine.get_page
    // returns None for.
    let frames = vec![{
        let mut f = make_frame(10);
        f.has_variables = true;
        f
    }];
    let vars = vec![make_obj_var(0, 42)];
    let expand_fields = Some(vec![FieldInfo {
        name: "tree".to_string(),
        value: FieldValue::ObjectRef {
            id: 777,
            class_name: "java.util.TreeMap".to_string(),
            entry_count: Some(50),
            inline_value: None,
        },
    }]);
    let engine = StubEngine::with_threads_and_frames(&["main"], frames)
        .with_vars(10, vars)
        .with_expand(42, expand_fields);
    let mut app = App::new(engine, "test.hprof".to_string());
    nav_to_collection_field(&mut app);
    app.handle_input(InputEvent::Enter);
    // poll_pages will get None and fall back to
    // expand_object.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while (!app.pending_pages.is_empty() || !app.pending_expansions.is_empty())
        && std::time::Instant::now() < deadline
    {
        let fallbacks = app.poll_pages();
        for cid in fallbacks {
            app.start_object_expansion(cid);
        }
        app.poll_expansions();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let ss = app.stack_state.as_ref().unwrap();
    // Collection chunks should be gone.
    assert!(!ss.expansion.collection_chunks.contains_key(&777));
    // Should have fallen back to expand_object →
    // expansion state should be Expanded.
    assert_eq!(ss.expansion_state(777), ExpansionPhase::Expanded);
}

#[test]
fn re_enter_on_loaded_chunk_toggles_collapse() {
    let mut app = make_collection_app(250);
    nav_to_collection_field(&mut app);
    app.handle_input(InputEvent::Enter);
    poll_all_pages(&mut app);
    // Navigate to first chunk, load it.
    for _ in 0..101 {
        app.handle_input(InputEvent::Down);
    }
    app.handle_input(InputEvent::Enter);
    poll_all_pages(&mut app);
    // Chunk is now Loaded.
    let ss = app.stack_state.as_ref().unwrap();
    assert!(matches!(
        ss.expansion
            .collection_chunks
            .get(&888)
            .unwrap()
            .chunk_pages
            .get(&100),
        Some(ChunkState::Loaded(_))
    ));
    // Enter again → ToggleChunk → Collapsed.
    app.handle_input(InputEvent::Enter);
    let ss = app.stack_state.as_ref().unwrap();
    assert!(matches!(
        ss.expansion
            .collection_chunks
            .get(&888)
            .unwrap()
            .chunk_pages
            .get(&100),
        Some(ChunkState::Collapsed)
    ));
}

#[test]
fn entry_rendering_map_vs_list_format() {
    use crate::views::stack_view::StackState;
    // Verify format_entry_line for map entry.
    let map_entry = EntryInfo {
        index: 5,
        key: Some(FieldValue::Int(42)),
        value: FieldValue::Int(100),
    };
    let line = StackState::format_entry_line(&map_entry, "  ", None);
    assert!(line.contains("[5] 42 => 100"), "map entry format: {}", line);
    // List entry.
    let list_entry = EntryInfo {
        index: 3,
        key: None,
        value: FieldValue::Int(77),
    };
    let line = StackState::format_entry_line(&list_entry, "  ", None);
    assert!(line.contains("[3] 77"), "list entry format: {}", line);
    assert!(
        !line.contains("=>"),
        "list entry should not have =>: {}",
        line
    );
    // ObjectRef value shows "+" toggle when collapsed.
    let obj_entry = EntryInfo {
        index: 0,
        key: None,
        value: FieldValue::ObjectRef {
            id: 999,
            class_name: "java.lang.String".to_string(),
            entry_count: None,
            inline_value: None,
        },
    };
    let line_collapsed = StackState::format_entry_line(
        &obj_entry,
        "  ",
        Some(&crate::views::stack_view::ExpansionPhase::Collapsed),
    );
    assert!(
        line_collapsed.contains("+ [0]") && line_collapsed.contains("String"),
        "ObjectRef collapsed should show '+ [0] ...': {}",
        line_collapsed
    );
    let line_expanded = StackState::format_entry_line(
        &obj_entry,
        "  ",
        Some(&crate::views::stack_view::ExpansionPhase::Expanded),
    );
    assert!(
        line_expanded.contains("- [0]") && line_expanded.contains("String"),
        "ObjectRef expanded should show '- [0] ...': {}",
        line_expanded
    );
}

/// Builds an App with collection 889 (entries are ObjectRef values).
fn make_obj_entry_collection_app() -> App<StubEngine> {
    let frames = vec![{
        let mut f = make_frame(10);
        f.has_variables = true;
        f
    }];
    let vars = vec![make_obj_var(0, 42)];
    let expand_fields = Some(vec![FieldInfo {
        name: "items".to_string(),
        value: FieldValue::ObjectRef {
            id: 889,
            class_name: "java.util.ArrayList".to_string(),
            entry_count: Some(3),
            inline_value: None,
        },
    }]);
    let engine = StubEngine::with_threads_and_frames(&["main"], frames)
        .with_vars(10, vars)
        .with_expand(42, expand_fields);
    App::new(engine, "test.hprof".to_string())
}

/// Builds an App where a collection entry object (id=700) contains an
/// `ObjectRef` field that is itself a collection (id=888).
fn make_obj_entry_array_field_collection_app() -> App<StubEngine> {
    let frames = vec![{
        let mut f = make_frame(10);
        f.has_variables = true;
        f
    }];
    let vars = vec![make_obj_var(0, 42)];
    let expand_fields = Some(vec![FieldInfo {
        name: "items".to_string(),
        value: FieldValue::ObjectRef {
            id: 889,
            class_name: "java.util.ArrayList".to_string(),
            entry_count: Some(3),
            inline_value: None,
        },
    }]);
    let entry_obj_fields = Some(vec![FieldInfo {
        name: "arr".to_string(),
        value: FieldValue::ObjectRef {
            id: 888,
            class_name: "Object[]".to_string(),
            entry_count: Some(250),
            inline_value: None,
        },
    }]);
    let engine = StubEngine::with_threads_and_frames(&["main"], frames)
        .with_vars(10, vars)
        .with_expand(42, expand_fields)
        .with_expand(700, entry_obj_fields);
    App::new(engine, "test.hprof".to_string())
}

/// Builds an App where the top-level collection contains an entry that is
/// itself a collection (`Object[]`) and must open via `StartCollection`.
fn make_collection_with_nested_collection_entries_app() -> App<StubEngine> {
    let frames = vec![{
        let mut f = make_frame(10);
        f.has_variables = true;
        f
    }];
    let vars = vec![make_obj_var(0, 42)];
    let expand_fields = Some(vec![FieldInfo {
        name: "items".to_string(),
        value: FieldValue::ObjectRef {
            id: 890,
            class_name: "java.util.ArrayList".to_string(),
            entry_count: Some(1),
            inline_value: None,
        },
    }]);
    let engine = StubEngine::with_threads_and_frames(&["main"], frames)
        .with_vars(10, vars)
        .with_expand(42, expand_fields);
    App::new(engine, "test.hprof".to_string())
}

#[test]
fn collection_entry_objectref_shows_plus_prefix() {
    let mut app = make_obj_entry_collection_app();
    // Navigate to collection field "items" (collection 889).
    let frames = vec![{
        let mut f = make_frame(10);
        f.has_variables = true;
        f
    }];
    let _ = frames; // already built in app
    nav_to_collection_field(&mut app);
    app.handle_input(InputEvent::Enter); // start collection load
    poll_all_pages(&mut app);
    // Navigate down to first entry (index 0).
    app.handle_input(InputEvent::Down);
    let ss = app.stack_state.as_ref().unwrap();
    assert!(
        matches!(
            ss.cursor(),
            StackCursor::OnCollectionEntry { entry_index: 0, .. }
        ),
        "should be on entry 0, got {:?}",
        ss.cursor()
    );
    // The entry rendering is verified through format_entry_line above;
    // here we verify Enter triggers start_object_expansion.
    app.handle_input(InputEvent::Enter);
    // pending_expansions should contain ObjectRef id 700 (entry 0's value).
    assert!(
        app.pending_expansions.contains_key(&700),
        "entering on ObjectRef entry should start expansion of id 700"
    );
}

#[test]
fn collection_entry_objectref_expanded_fields_appear_in_tree() {
    let mut app = make_obj_entry_collection_app();
    nav_to_collection_field(&mut app);
    app.handle_input(InputEvent::Enter);
    poll_all_pages(&mut app);
    // Navigate to entry 0 and expand it.
    app.handle_input(InputEvent::Down);
    app.handle_input(InputEvent::Enter); // start expand of id=700, then poll until done.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !app.pending_expansions.is_empty() && std::time::Instant::now() < deadline {
        app.poll_expansions();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    // Navigate down — should enter OnCollectionEntryObjField.
    app.handle_input(InputEvent::Down);
    let ss = app.stack_state.as_ref().unwrap();
    assert!(
        matches!(
            ss.cursor(),
            StackCursor::OnCollectionEntryObjField { entry_index: 0, .. }
        ),
        "after expanding entry 0, down should reach OnCollectionEntryObjField, \
             got {:?}",
        ss.cursor()
    );
}

#[test]
fn collection_entry_object_field_collection_opens_without_failed_resolve() {
    let mut app = make_obj_entry_array_field_collection_app();
    nav_to_collection_field(&mut app);
    app.handle_input(InputEvent::Enter); // open collection 889
    poll_all_pages(&mut app);

    app.handle_input(InputEvent::Down); // -> OnCollectionEntry{collection_id:889, entry_index:0}
    app.handle_input(InputEvent::Enter); // expand entry object id=700
    poll_all_expansions(&mut app);

    app.handle_input(InputEvent::Down); // -> OnCollectionEntryObjField (arr)
    {
        let ss = app.stack_state.as_ref().unwrap();
        assert!(
            matches!(ss.cursor(), StackCursor::OnCollectionEntryObjField { .. }),
            "expected OnCollectionEntryObjField before opening nested collection, got {:?}",
            ss.cursor()
        );
    }

    app.handle_input(InputEvent::Enter); // must StartCollection(888), not StartEntryObj(888)
    assert!(
        app.pending_pages.contains_key(&(888, 0)),
        "nested collection field must trigger collection paging"
    );
    assert!(
        !app.pending_expansions.contains_key(&888),
        "nested collection field must not call expand_object on collection id"
    );

    poll_all_pages(&mut app);
    app.handle_input(InputEvent::Down); // -> first nested collection entry
    {
        let ss = app.stack_state.as_ref().unwrap();
        assert!(
            matches!(
                ss.cursor(),
                StackCursor::OnCollectionEntry {
                    collection_id: 888,
                    entry_index: 0,
                    ..
                }
            ),
            "expected first nested collection entry, got {:?}",
            ss.cursor()
        );
    }

    app.handle_input(InputEvent::Escape);
    {
        let ss = app.stack_state.as_ref().unwrap();
        assert!(
            !ss.expansion.collection_chunks.contains_key(&888),
            "nested collection should be collapsed on escape"
        );
        assert!(
            matches!(ss.cursor(), StackCursor::OnCollectionEntryObjField { .. }),
            "escape from nested collection should restore OnCollectionEntryObjField, got {:?}",
            ss.cursor()
        );
    }
}

#[test]
fn nested_collection_entry_object_array_opens_and_renders_children() {
    let mut app = make_collection_with_nested_collection_entries_app();
    nav_to_collection_field(&mut app);
    app.handle_input(InputEvent::Enter); // open collection 890
    poll_all_pages(&mut app);

    app.handle_input(InputEvent::Down); // -> entry 0 of collection 890 (value is Object[] id=888)
    {
        let ss = app.stack_state.as_ref().unwrap();
        assert!(
            matches!(
                ss.cursor(),
                StackCursor::OnCollectionEntry {
                    collection_id: 890,
                    entry_index: 0,
                    ..
                }
            ),
            "expected entry 0 on outer collection, got {:?}",
            ss.cursor()
        );
    }

    app.handle_input(InputEvent::Enter); // must StartCollection(888), not StartEntryObj(888)
    assert!(
        app.pending_pages.contains_key(&(888, 0)),
        "nested collection entry must trigger collection paging"
    );
    assert!(
        !app.pending_expansions.contains_key(&888),
        "nested collection entry must not call expand_object on collection id"
    );

    poll_all_pages(&mut app);
    app.handle_input(InputEvent::Down); // -> first entry of nested collection 888
    {
        let ss = app.stack_state.as_ref().unwrap();
        assert!(
            matches!(
                ss.cursor(),
                StackCursor::OnCollectionEntry {
                    collection_id: 888,
                    entry_index: 0,
                    ..
                }
            ),
            "expected first nested collection entry, got {:?}",
            ss.cursor()
        );
    }

    app.handle_input(InputEvent::Escape);
    {
        let ss = app.stack_state.as_ref().unwrap();
        assert!(
            !ss.expansion.collection_chunks.contains_key(&888),
            "nested collection should be collapsed on escape"
        );
        assert!(
            matches!(
                ss.cursor(),
                StackCursor::OnCollectionEntry {
                    collection_id: 890,
                    entry_index: 0,
                    ..
                }
            ),
            "escape from nested collection should restore outer collection entry, got {:?}",
            ss.cursor()
        );
    }
}

#[test]
fn right_on_nested_collection_entry_starts_collection_paging() {
    let mut app = make_collection_with_nested_collection_entries_app();
    nav_to_collection_field(&mut app);
    app.handle_input(InputEvent::Enter); // open collection 890
    poll_all_pages(&mut app);

    app.handle_input(InputEvent::Down); // -> entry 0 of collection 890 (value Object[] id=888)
    app.handle_input(InputEvent::Right); // must mirror Enter and StartCollection(888)

    assert!(
        app.pending_pages.contains_key(&(888, 0)),
        "Right on nested collection entry must trigger collection paging"
    );
    assert!(
        !app.pending_expansions.contains_key(&888),
        "Right on nested collection entry must not call expand_object on collection id"
    );
}

#[test]
fn right_on_collection_entry_object_field_collection_starts_collection_paging() {
    let mut app = make_obj_entry_array_field_collection_app();
    nav_to_collection_field(&mut app);
    app.handle_input(InputEvent::Enter); // open collection 889
    poll_all_pages(&mut app);

    app.handle_input(InputEvent::Down); // -> OnCollectionEntry{collection_id:889, entry_index:0}
    app.handle_input(InputEvent::Enter); // expand entry object id=700
    poll_all_expansions(&mut app);

    app.handle_input(InputEvent::Down); // -> OnCollectionEntryObjField (arr)
    app.handle_input(InputEvent::Right); // must mirror Enter and StartCollection(888)

    assert!(
        app.pending_pages.contains_key(&(888, 0)),
        "Right on collection-entry object field must trigger collection paging"
    );
    assert!(
        !app.pending_expansions.contains_key(&888),
        "Right on collection-entry object field must not call expand_object on collection id"
    );
}

#[test]
fn left_on_primitive_collection_entry_navigates_to_parent_var() {
    let mut app = make_var_collection_app(250);
    app.handle_input(InputEvent::Enter); // StackFrames
    app.handle_input(InputEvent::Enter); // expand frame
    app.handle_input(InputEvent::Down); // -> OnVar{0,0}
    app.handle_input(InputEvent::Enter); // open collection 888 from var
    poll_all_pages(&mut app);

    app.handle_input(InputEvent::Down); // -> first collection entry (primitive Int)
    {
        let ss = app.stack_state.as_ref().unwrap();
        assert!(
            matches!(ss.cursor(), StackCursor::OnCollectionEntry { .. }),
            "expected collection entry before Left, got {:?}",
            ss.cursor()
        );
    }

    app.handle_input(InputEvent::Left);
    let ss = app.stack_state.as_ref().unwrap();
    assert_eq!(
        ss.cursor(),
        &StackCursor::OnVar {
            frame_idx: 0,
            var_idx: 0
        },
        "Left on primitive collection entry must navigate to parent var"
    );
}

#[test]
fn left_on_primitive_collection_entry_object_field_navigates_to_parent_entry() {
    let mut app = make_obj_entry_collection_app();
    nav_to_collection_field(&mut app);
    app.handle_input(InputEvent::Enter); // open collection 889
    poll_all_pages(&mut app);

    app.handle_input(InputEvent::Down); // -> entry 0 (ObjectRef id=700)
    app.handle_input(InputEvent::Enter); // expand 700
    poll_all_expansions(&mut app);

    app.handle_input(InputEvent::Down); // -> first entry object field (x:Int)
    {
        let ss = app.stack_state.as_ref().unwrap();
        assert!(
            matches!(ss.cursor(), StackCursor::OnCollectionEntryObjField { .. }),
            "expected entry object field before Left, got {:?}",
            ss.cursor()
        );
    }

    app.handle_input(InputEvent::Left);
    let ss = app.stack_state.as_ref().unwrap();
    assert!(
        matches!(
            ss.cursor(),
            StackCursor::OnCollectionEntry {
                collection_id: 889,
                entry_index: 0,
                ..
            }
        ),
        "Left on primitive entry object field must navigate to parent entry, got {:?}",
        ss.cursor()
    );
}

#[test]
fn page_up_down_scrolls_tree_by_visible_height() {
    // This is a general tree scroll test, not
    // collection-specific.
    use crate::views::stack_view::StackState;
    let frames: Vec<_> = (1..=30).map(make_frame).collect();
    let mut state = StackState::new(frames);
    state.set_visible_height(10);
    // Move to frame 5.
    for _ in 0..5 {
        state.move_down();
    }
    assert_eq!(*state.cursor(), StackCursor::OnFrame(5));
    state.move_page_down();
    assert_eq!(*state.cursor(), StackCursor::OnFrame(15));
    state.move_page_up();
    assert_eq!(*state.cursor(), StackCursor::OnFrame(5));
}

#[test]
fn camera_scroll_in_stack_frames_shifts_offset_without_moving_cursor() {
    let frames: Vec<_> = (0..5).map(make_frame).collect();
    let engine = StubEngine::with_threads_and_frames(&["main"], frames);
    let mut app = App::new(engine, "test.hprof".to_string());

    app.handle_input(InputEvent::Enter); // -> StackFrames
    app.handle_input(InputEvent::Down); // -> frame 1
    app.handle_input(InputEvent::Down); // -> frame 2

    {
        let ss = app
            .stack_state
            .as_mut()
            .expect("stack_state must be present in stack focus");
        ss.set_visible_height(3);
        ss.set_list_state_offset_for_test(0);
    }

    app.handle_input(InputEvent::CameraScrollDown);

    let ss = app.stack_state.as_ref().unwrap();
    assert_eq!(ss.list_state_offset_for_test(), 1);
    assert_eq!(ss.selected_frame_id(), Some(2));
    assert_eq!(app.focus, Focus::StackFrames);
}

#[test]
fn camera_scroll_in_thread_list_is_noop() {
    let engine = StubEngine::with_threads(&["main", "worker"]);
    let mut app = App::new(engine, "test.hprof".to_string());
    let before = app.thread_list.selected_serial();

    app.handle_input(InputEvent::CameraScrollDown);
    app.handle_input(InputEvent::CameraScrollUp);

    assert_eq!(app.focus, Focus::ThreadList);
    assert_eq!(app.thread_list.selected_serial(), before);
    assert!(app.stack_state.is_none());
}

#[test]
fn camera_scroll_in_search_mode_is_noop_and_keeps_filter() {
    let engine = StubEngine::with_threads(&["main", "worker"]);
    let mut app = App::new(engine, "test.hprof".to_string());

    app.handle_input(InputEvent::SearchActivate);
    app.handle_input(InputEvent::SearchChar('w'));
    let before_selected = app.thread_list.selected_serial();

    app.handle_input(InputEvent::CameraScrollDown);

    assert_eq!(app.focus, Focus::ThreadList);
    assert!(app.thread_list.is_search_active());
    assert_eq!(app.thread_list.filter(), "w");
    assert_eq!(app.thread_list.selected_serial(), before_selected);
}

#[test]
fn camera_center_in_stack_frames_centers_view_without_moving_cursor() {
    let frames: Vec<_> = (0..8).map(make_frame).collect();
    let engine = StubEngine::with_threads_and_frames(&["main"], frames);
    let mut app = App::new(engine, "test.hprof".to_string());

    app.handle_input(InputEvent::Enter); // -> StackFrames
    app.handle_input(InputEvent::Down); // -> frame 1
    app.handle_input(InputEvent::Down); // -> frame 2
    app.handle_input(InputEvent::Down); // -> frame 3

    {
        let ss = app
            .stack_state
            .as_mut()
            .expect("stack_state must be present in stack focus");
        ss.set_visible_height(5);
        ss.set_list_state_offset_for_test(0);
    }

    app.handle_input(InputEvent::CameraCenterSelection);

    let ss = app.stack_state.as_ref().unwrap();
    // selected(3), visible_height(5): center row index = 2 => offset = 1.
    assert_eq!(ss.list_state_offset_for_test(), 1);
    assert_eq!(ss.selected_frame_id(), Some(3));
    assert_eq!(app.focus, Focus::StackFrames);
}

#[test]
fn camera_center_in_thread_list_is_noop() {
    let engine = StubEngine::with_threads(&["main", "worker"]);
    let mut app = App::new(engine, "test.hprof".to_string());
    let before = app.thread_list.selected_serial();

    app.handle_input(InputEvent::CameraCenterSelection);

    assert_eq!(app.focus, Focus::ThreadList);
    assert_eq!(app.thread_list.selected_serial(), before);
    assert!(app.stack_state.is_none());
}

#[test]
fn camera_page_scroll_in_stack_frames_shifts_offset_without_moving_cursor() {
    let frames: Vec<_> = (0..12).map(make_frame).collect();
    let engine = StubEngine::with_threads_and_frames(&["main"], frames);
    let mut app = App::new(engine, "test.hprof".to_string());

    app.handle_input(InputEvent::Enter); // -> StackFrames
    for _ in 0..7 {
        app.handle_input(InputEvent::Down);
    }

    {
        let ss = app
            .stack_state
            .as_mut()
            .expect("stack_state must be present in stack focus");
        ss.set_visible_height(4);
        ss.set_list_state_offset_for_test(0);
    }

    app.handle_input(InputEvent::CameraPageDown);

    let ss = app.stack_state.as_ref().unwrap();
    assert_eq!(ss.list_state_offset_for_test(), 4);
    assert_eq!(ss.selected_frame_id(), Some(7));
    assert_eq!(app.focus, Focus::StackFrames);
}

#[test]
fn camera_page_scroll_in_thread_list_is_noop() {
    let engine = StubEngine::with_threads(&["main", "worker"]);
    let mut app = App::new(engine, "test.hprof".to_string());
    let before = app.thread_list.selected_serial();

    app.handle_input(InputEvent::CameraPageDown);
    app.handle_input(InputEvent::CameraPageUp);

    assert_eq!(app.focus, Focus::ThreadList);
    assert_eq!(app.thread_list.selected_serial(), before);
    assert!(app.stack_state.is_none());
}

// --- Task 4: loading indicator threshold tests ---

#[test]
fn loading_indicator_not_shown_before_1_second() {
    let frames = vec![make_frame(10)];
    let vars = vec![make_obj_var(0, 42)];
    let engine = StubEngine::with_threads_and_frames(&["main"], frames).with_vars(10, vars);
    let mut app = App::new(engine, "test.hprof".to_string());
    app.handle_input(InputEvent::Enter);
    app.handle_input(InputEvent::Enter);
    app.handle_input(InputEvent::Down);
    app.handle_input(InputEvent::Enter); // start_object_expansion(42) — completes fast.
    // Poll once without sleeping — StubEngine responds immediately.
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !app.pending_expansions.is_empty() && std::time::Instant::now() < deadline {
        app.poll_expansions();
        std::thread::sleep(Duration::from_millis(1));
    }
    // Expansion completed without ever setting Loading state.
    assert_eq!(
        app.stack_state.as_ref().unwrap().expansion_state(42),
        ExpansionPhase::Expanded,
        "fast expansion must complete as Expanded without ever showing Loading"
    );
}

// --- Task 5: WarningLog wiring tests ---

#[test]
fn failed_expansion_adds_warning_to_log() {
    let frames = vec![make_frame(10)];
    let vars = vec![make_obj_var(0, 55)];
    let engine = StubEngine::with_threads_and_frames(&["main"], frames)
        .with_vars(10, vars)
        .with_expand(55, None); // force None → unresolvable
    let mut app = App::new(engine, "test.hprof".to_string());
    app.handle_input(InputEvent::Enter);
    app.handle_input(InputEvent::Enter);
    app.handle_input(InputEvent::Down);
    app.handle_input(InputEvent::Enter); // start expansion of 55, then poll for result.
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !app.pending_expansions.is_empty() && std::time::Instant::now() < deadline {
        app.poll_expansions();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(app.warnings.count(), 1);
    assert!(
        app.warnings.last().unwrap_or("").contains("0x37"),
        "warning must reference the object id; got: {:?}",
        app.warnings.last()
    );
}

#[test]
fn disconnected_expansion_adds_warning_to_log() {
    let frames = vec![make_frame(10)];
    let vars = vec![make_obj_var(0, 77)];
    let engine = StubEngine::with_threads_and_frames(&["main"], frames).with_vars(10, vars);
    let mut app = App::new(engine, "test.hprof".to_string());
    // Manually inject a disconnected pending expansion (tx dropped immediately).
    let (tx, rx) = mpsc::channel::<Option<Vec<FieldInfo>>>();
    drop(tx); // disconnect
    app.pending_expansions.insert(
        77,
        PendingExpansion {
            rx,
            started: Instant::now(),
            loading_shown: false,
        },
    );
    app.poll_expansions();
    assert_eq!(app.warnings.count(), 1);
    assert!(
        app.warnings.last().unwrap_or("").contains("0x4D"),
        "warning must reference the object id 0x4D (77); got: {:?}",
        app.warnings.last()
    );
}

// --- Task 6: format_memory_log tests ---

#[test]
fn format_memory_log_produces_correct_output() {
    let s = format_memory_log(42 * 1024 * 1024, 512 * 1_048_576, 38 * 1024 * 1024);
    assert_eq!(
        s,
        "[memory] cache 42 MB / 512 MB budget | skeleton 38 MB (non-evictable)"
    );
}

#[test]
fn format_memory_log_rounds_down_to_mb() {
    // 1.9 MB → 1 MB (integer division rounds down)
    let s = format_memory_log(1024 * 1024 + 900_000, 1_048_576, 0);
    assert!(s.contains("cache 1 MB"), "expected round-down; got: {s}");
    assert!(s.contains("skeleton 0 MB"), "got: {s}");
}

#[test]
fn loading_indicator_shown_if_not_yet_complete_after_1_second() {
    // Use a slow channel: create the receiver manually without sending a result.
    let frames = vec![make_frame(10)];
    let vars = vec![make_obj_var(0, 99)];
    let engine = StubEngine::with_threads_and_frames(&["main"], frames).with_vars(10, vars);
    let mut app = App::new(engine, "test.hprof".to_string());
    app.handle_input(InputEvent::Enter);
    app.handle_input(InputEvent::Enter);
    app.handle_input(InputEvent::Down);
    // Manually insert a PendingExpansion with an unsent channel and past started time.
    let (_tx, rx) = mpsc::channel::<Option<Vec<FieldInfo>>>();
    app.pending_expansions.insert(
        99,
        PendingExpansion {
            rx,
            started: Instant::now() - EXPANSION_LOADING_THRESHOLD - Duration::from_millis(10),
            loading_shown: false,
        },
    );
    // Poll once — threshold exceeded, loading_shown transitions to true.
    app.poll_expansions();
    // Verify loading_shown was set.
    let pe = app.pending_expansions.get(&99).unwrap();
    assert!(
        pe.loading_shown,
        "loading_shown must be set after threshold"
    );
}

#[test]
fn hidden_favorites_panel_forces_focus_back_to_previous_panel() {
    use crate::favorites::{PinKey, PinnedItem, PinnedSnapshot};
    use ratatui::{Terminal, backend::TestBackend};

    let engine = StubEngine::with_threads(&["main"]);
    let mut app = App::new(engine, "test.hprof".to_string());
    app.pinned.push(PinnedItem {
        thread_name: "main".to_string(),
        frame_label: "Thread.run()".to_string(),
        item_label: "var[0]".to_string(),
        snapshot: PinnedSnapshot::Primitive {
            value_label: "42".to_string(),
        },
        key: PinKey::Var {
            frame_id: 1,
            thread_name: "main".to_string(),
            var_idx: 0,
        },
    });
    app.sync_favorites_selection();
    app.prev_focus = Focus::StackFrames;
    app.focus = Focus::Favorites;

    let backend = TestBackend::new(100, 20); // < MIN_WIDTH_FAVORITES_PANEL
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| app.render(f)).unwrap();

    assert_eq!(app.last_area_width, 100);
    assert_eq!(
        app.focus,
        Focus::StackFrames,
        "focus must return to previous panel when favorites is hidden"
    );
}

#[test]
fn toggle_help_sets_show_help_true() {
    let engine = StubEngine::with_threads(&["main"]);
    let mut app = App::new(engine, "test.hprof".to_string());
    assert!(!app.show_help);
    let action = app.handle_input(InputEvent::ToggleHelp);
    assert_eq!(action, AppAction::Continue);
    assert!(app.show_help);
}

#[test]
fn toggle_help_twice_sets_show_help_false() {
    let engine = StubEngine::with_threads(&["main"]);
    let mut app = App::new(engine, "test.hprof".to_string());
    app.handle_input(InputEvent::ToggleHelp);
    app.handle_input(InputEvent::ToggleHelp);
    assert!(!app.show_help);
}

#[test]
fn up_still_routes_when_show_help_is_true() {
    let engine = StubEngine::with_threads(&["main", "worker"]);
    let mut app = App::new(engine, "test.hprof".to_string());
    app.handle_input(InputEvent::Down); // selection moves to worker
    app.show_help = true;
    app.handle_input(InputEvent::Up); // selection moves back to main
    assert_eq!(app.thread_list.selected_serial(), Some(1));
}

#[test]
fn quit_returns_app_action_quit_when_show_help_is_true() {
    let engine = StubEngine::with_threads(&["main"]);
    let mut app = App::new(engine, "test.hprof".to_string());
    app.show_help = true;
    assert_eq!(app.handle_input(InputEvent::Quit), AppAction::Quit);
}

#[test]
fn tab_from_thread_list_with_no_stack_state_is_noop() {
    let engine = StubEngine::with_threads(&["main"]);
    let mut app = App::new(engine, "test.hprof".to_string());
    assert_eq!(app.focus, Focus::ThreadList);
    app.handle_input(InputEvent::Tab);
    assert_eq!(app.focus, Focus::ThreadList);
}

#[test]
fn tab_from_thread_list_with_stack_state_moves_to_stack_frames() {
    let frames = vec![make_frame(10)];
    let engine = StubEngine::with_threads_and_frames(&["main"], frames);
    let mut app = App::new(engine, "test.hprof".to_string());
    app.handle_input(InputEvent::Enter); // → StackFrames, stack_state = Some(...)
    app.focus = Focus::ThreadList; // simulate returning to thread list
    app.handle_input(InputEvent::Tab);
    assert_eq!(app.focus, Focus::StackFrames);
}

#[test]
fn tab_from_stack_frames_returns_to_thread_list() {
    let frames = vec![make_frame(10)];
    let engine = StubEngine::with_threads_and_frames(&["main"], frames);
    let mut app = App::new(engine, "test.hprof".to_string());
    app.handle_input(InputEvent::Enter); // → StackFrames
    assert_eq!(app.focus, Focus::StackFrames);
    app.handle_input(InputEvent::Tab);
    assert_eq!(app.focus, Focus::ThreadList);
}

#[test]
fn search_char_s_in_non_search_mode_activates_search() {
    let engine = StubEngine::with_threads(&["main"]);
    let mut app = App::new(engine, "test.hprof".to_string());
    assert!(!app.thread_list.is_search_active());
    app.handle_input(InputEvent::SearchChar('s'));
    assert!(app.thread_list.is_search_active());
}

#[test]
fn quit_from_thread_list_with_search_active_returns_quit() {
    let engine = StubEngine::with_threads(&["main"]);
    let mut app = App::new(engine, "test.hprof".to_string());
    app.handle_input(InputEvent::SearchActivate);
    assert!(app.thread_list.is_search_active());
    assert_eq!(app.handle_input(InputEvent::Quit), AppAction::Quit);
}

#[test]
fn quit_from_stack_frames_returns_quit() {
    let frames = vec![make_frame(10)];
    let engine = StubEngine::with_threads_and_frames(&["main"], frames);
    let mut app = App::new(engine, "test.hprof".to_string());
    app.handle_input(InputEvent::Enter); // → StackFrames
    assert_eq!(app.handle_input(InputEvent::Quit), AppAction::Quit);
}

#[test]
fn tab_from_thread_list_with_search_active_moves_to_stack_frames() {
    let frames = vec![make_frame(10)];
    let engine = StubEngine::with_threads_and_frames(&["main"], frames);
    let mut app = App::new(engine, "test.hprof".to_string());
    app.handle_input(InputEvent::Enter); // → StackFrames
    app.focus = Focus::ThreadList;
    app.handle_input(InputEvent::SearchActivate);
    assert!(app.thread_list.is_search_active());

    app.handle_input(InputEvent::Tab);
    assert_eq!(app.focus, Focus::StackFrames);
}

#[test]
fn tab_from_favorites_cycles_to_thread_list() {
    use crate::favorites::{PinKey, PinnedItem, PinnedSnapshot};

    let engine = StubEngine::with_threads(&["main"]);
    let mut app = App::new(engine, "test.hprof".to_string());
    app.pinned.push(PinnedItem {
        thread_name: "main".to_string(),
        frame_label: "Thread.run()".to_string(),
        item_label: "var[0]".to_string(),
        snapshot: PinnedSnapshot::Primitive {
            value_label: "42".to_string(),
        },
        key: PinKey::Var {
            frame_id: 1,
            thread_name: "main".to_string(),
            var_idx: 0,
        },
    });
    app.last_area_width = MIN_WIDTH_FAVORITES_PANEL;
    app.focus = Focus::Favorites;

    app.handle_input(InputEvent::Tab);
    assert_eq!(app.focus, Focus::ThreadList);
}
