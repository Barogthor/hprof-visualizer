use hprof_engine::{
    CollectionPage, EntryInfo, FieldInfo, FieldValue, FrameInfo, LineNumber, NavigationEngine,
    ThreadInfo, ThreadState, VariableInfo, VariableValue,
};
use std::collections::HashMap;
use std::collections::HashSet;

use super::*;

fn cursor_ends_with_collection_entry(cursor: &RenderCursor) -> bool {
    matches!(cursor, RenderCursor::At(p)
        if matches!(p.segments().last(), Some(PathSegment::CollectionEntry(..))))
}

fn cursor_ends_with_field(cursor: &RenderCursor) -> bool {
    matches!(cursor, RenderCursor::At(p)
        if matches!(p.segments().last(), Some(PathSegment::Field(..))))
}

fn cursor_ends_with_static_field(cursor: &RenderCursor) -> bool {
    matches!(cursor, RenderCursor::At(p)
        if matches!(p.segments().last(), Some(PathSegment::StaticField(..))))
}

fn cursor_is_chunk_section(cursor: &RenderCursor) -> bool {
    matches!(cursor, RenderCursor::ChunkSection(..))
}

fn cursor_chunk_section_offset(cursor: &RenderCursor) -> Option<usize> {
    if let RenderCursor::ChunkSection(_, off) = cursor {
        Some(off.0)
    } else {
        None
    }
}

fn cursor_is_collection_entry_field(cursor: &RenderCursor) -> bool {
    if let RenderCursor::At(p) = cursor {
        let segs = p.segments();
        let last_is_field = matches!(segs.last(), Some(PathSegment::Field(..)));
        let has_coll_entry = segs
            .iter()
            .any(|s| matches!(s, PathSegment::CollectionEntry(..)));
        return last_is_field && has_coll_entry;
    }
    false
}

fn cursor_collection_entry_ids(cursor: &RenderCursor) -> Option<(u64, usize)> {
    if let RenderCursor::At(p) = cursor
        && let Some(PathSegment::CollectionEntry(cid, eidx)) = p.segments().last()
    {
        return Some((cid.0, eidx.0));
    }
    None
}

fn make_pin_key_var(
    thread_id: u32,
    thread_name: &str,
    frame_id: u64,
    var_idx: usize,
) -> crate::favorites::PinKey {
    crate::favorites::PinKey {
        thread_id: ThreadId(thread_id),
        thread_name: thread_name.to_string(),
        nav_path: NavigationPathBuilder::new(FrameId(frame_id), VarIdx(var_idx)).build(),
    }
}

fn make_pin_key_field(
    thread_id: u32,
    thread_name: &str,
    frame_id: u64,
    var_idx: usize,
    field_path: &[usize],
) -> crate::favorites::PinKey {
    let mut b = NavigationPathBuilder::new(FrameId(frame_id), VarIdx(var_idx));
    for &fi in field_path {
        b = b.field(FieldIdx(fi));
    }
    crate::favorites::PinKey {
        thread_id: ThreadId(thread_id),
        thread_name: thread_name.to_string(),
        nav_path: b.build(),
    }
}

fn make_pin_key_coll_entry(
    thread_id: u32,
    thread_name: &str,
    frame_id: u64,
    var_idx: usize,
    field_path: &[usize],
    collection_id: u64,
    entry_index: usize,
) -> crate::favorites::PinKey {
    let mut b = NavigationPathBuilder::new(FrameId(frame_id), VarIdx(var_idx));
    for &fi in field_path {
        b = b.field(FieldIdx(fi));
    }
    let b = b.collection_entry(CollectionId(collection_id), EntryIdx(entry_index));
    crate::favorites::PinKey {
        thread_id: ThreadId(thread_id),
        thread_name: thread_name.to_string(),
        nav_path: b.build(),
    }
}

#[allow(clippy::too_many_arguments)]
fn make_pin_key_coll_entry_field(
    thread_id: u32,
    thread_name: &str,
    frame_id: u64,
    var_idx: usize,
    field_path: &[usize],
    collection_id: u64,
    entry_index: usize,
    obj_field_path: &[usize],
) -> crate::favorites::PinKey {
    let mut b = NavigationPathBuilder::new(FrameId(frame_id), VarIdx(var_idx));
    for &fi in field_path {
        b = b.field(FieldIdx(fi));
    }
    let mut b = b.collection_entry(CollectionId(collection_id), EntryIdx(entry_index));
    for &fi in obj_field_path {
        b = b.field(FieldIdx(fi));
    }
    crate::favorites::PinKey {
        thread_id: ThreadId(thread_id),
        thread_name: thread_name.to_string(),
        nav_path: b.build(),
    }
}

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
            // 1 entry: ObjectRef with entry_count == 0 (empty collection inside collection)
            891 => {
                let total: u64 = 1;
                let end = (offset + limit).min(total as usize);
                let entries = (offset..end)
                    .map(|i| EntryInfo {
                        index: i,
                        key: None,
                        value: FieldValue::ObjectRef {
                            id: 777,
                            class_name: "ArrayList".to_string(),
                            entry_count: Some(0),
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

fn make_favorite_item(thread_name: &str, frame_id: u64) -> crate::favorites::PinnedItem {
    make_favorite_item_with_tid(1, thread_name, frame_id)
}

fn make_favorite_item_with_tid(
    thread_id: u32,
    thread_name: &str,
    frame_id: u64,
) -> crate::favorites::PinnedItem {
    crate::favorites::PinnedItem {
        thread_name: thread_name.to_string(),
        frame_label: "Thread.run()".to_string(),
        item_label: "var[0]".to_string(),
        snapshot: crate::favorites::PinnedSnapshot::Primitive {
            value_label: "42".to_string(),
        },
        local_collapsed: HashSet::new(),
        hidden_fields: HashSet::new(),
        show_hidden: false,
        key: make_pin_key_var(thread_id, thread_name, frame_id, 0),
    }
}

fn poll_all_expansions_top(app: &mut App<StubEngine>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !app.pending_expansions.is_empty() && std::time::Instant::now() < deadline {
        app.poll_expansions();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// Drives the full async navigation loop until `pending_navigation` is None.
///
/// Mirrors the `run_loop` poll sequence: expansions → pages → Continue resume.
fn poll_navigation_to_completion(app: &mut App<StubEngine>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while app.pending_navigation.is_some() && std::time::Instant::now() < deadline {
        app.poll_expansions();
        app.poll_pages();
        if app
            .pending_navigation
            .as_ref()
            .is_some_and(|p| p.awaited == AwaitedResource::Continue)
            && let Some(pending) = app.pending_navigation.take()
        {
            app.resume_pending_navigation(pending);
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

fn make_field_favorite_item(
    thread_name: &str,
    frame_id: u64,
    var_idx: usize,
    field_path: Vec<usize>,
) -> crate::favorites::PinnedItem {
    crate::favorites::PinnedItem {
        thread_name: thread_name.to_string(),
        frame_label: "Thread.run()".to_string(),
        item_label: "var[0].field".to_string(),
        snapshot: crate::favorites::PinnedSnapshot::Primitive {
            value_label: "42".to_string(),
        },
        local_collapsed: HashSet::new(),
        hidden_fields: HashSet::new(),
        show_hidden: false,
        key: make_pin_key_field(1, thread_name, frame_id, var_idx, &field_path),
    }
}

mod construction {
    //! Tests that `App::new` initialises correctly with zero or several threads.
    use super::*;

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
}

mod thread_navigation {
    //! Tests for thread-list navigation: movement, enter, back, search filter (activation,
    //! char input, backspace, Esc single/double, filter persistence).
    use super::*;

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
    fn thread_list_search_bar_visible_when_filter_active_not_in_input_mode() {
        let engine = StubEngine::with_threads(&["main", "worker"]);
        let mut app = App::new(engine, "test.hprof".to_string());

        app.handle_input(InputEvent::SearchActivate);
        app.handle_input(InputEvent::SearchChar('w'));
        app.handle_input(InputEvent::Escape);

        assert!(!app.thread_list.filter().is_empty());
        assert!(!app.thread_list.is_search_active());
    }

    #[test]
    fn search_backspace_uses_pop_for_utf8_safety() {
        let engine = StubEngine::with_threads(&["main", "worker"]);
        let mut app = App::new(engine, "test.hprof".to_string());

        app.handle_input(InputEvent::SearchActivate);
        app.handle_input(InputEvent::SearchChar('é'));
        app.handle_input(InputEvent::SearchBackspace);

        assert_eq!(app.thread_list.filter(), "");
    }

    #[test]
    fn thread_list_esc_in_search_mode_preserves_filter() {
        let engine = StubEngine::with_threads(&["main", "worker"]);
        let mut app = App::new(engine, "test.hprof".to_string());
        app.handle_input(InputEvent::SearchActivate);
        app.handle_input(InputEvent::SearchChar('w'));
        app.handle_input(InputEvent::Escape);
        assert!(!app.thread_list.is_search_active());
        assert_eq!(app.thread_list.filter(), "w");
        assert_eq!(app.thread_list.filtered_count(), 1);
    }

    #[test]
    fn thread_list_second_esc_clears_filter() {
        let engine = StubEngine::with_threads(&["main", "worker"]);
        let mut app = App::new(engine, "test.hprof".to_string());
        app.handle_input(InputEvent::SearchActivate);
        app.handle_input(InputEvent::SearchChar('w'));
        app.handle_input(InputEvent::Escape);

        app.handle_input(InputEvent::Escape);

        assert!(!app.thread_list.is_search_active());
        assert_eq!(app.thread_list.filter(), "");
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
    fn thread_list_enter_in_search_mode_deactivates_input_keeps_filter() {
        let frames = vec![make_frame(10)];
        let engine = StubEngine::with_threads_and_frames(&["main", "worker"], frames);
        let mut app = App::new(engine, "test.hprof".to_string());

        app.handle_input(InputEvent::SearchActivate);
        app.handle_input(InputEvent::SearchChar('w'));
        app.handle_input(InputEvent::Enter);

        assert_eq!(app.focus, Focus::StackFrames);
        assert!(!app.thread_list.is_search_active());
        assert_eq!(app.thread_list.filter(), "w");
    }

    #[test]
    fn thread_list_esc_routing_does_not_clear_filter_from_other_focus() {
        let frames = vec![make_frame(10)];
        let engine = StubEngine::with_threads_and_frames(&["main", "worker"], frames);
        let mut app = App::new(engine, "test.hprof".to_string());

        app.handle_input(InputEvent::SearchActivate);
        app.handle_input(InputEvent::SearchChar('w'));
        app.handle_input(InputEvent::Escape);
        assert_eq!(app.thread_list.filter(), "w");

        app.handle_input(InputEvent::Enter);
        assert_eq!(app.focus, Focus::StackFrames);

        app.handle_input(InputEvent::Escape);

        assert_eq!(app.focus, Focus::ThreadList);
        assert_eq!(app.thread_list.filter(), "w");
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
    fn esc_from_stack_frames_to_thread_list_preserves_filter() {
        let frames = vec![make_frame(10)];
        let engine = StubEngine::with_threads_and_frames(&["main", "worker"], frames);
        let mut app = App::new(engine, "test.hprof".to_string());

        app.handle_input(InputEvent::SearchActivate);
        app.handle_input(InputEvent::SearchChar('w'));
        app.handle_input(InputEvent::Escape);
        app.handle_input(InputEvent::Enter);
        assert_eq!(app.focus, Focus::StackFrames);

        app.handle_input(InputEvent::Escape);

        assert_eq!(app.focus, Focus::ThreadList);
        assert_eq!(app.thread_list.filter(), "w");
    }
}

mod stack_navigation {
    //! Tests for stack-frame navigation: Up/Down, Enter expand/collapse, Esc back,
    //! initial `stack_state`, and automatic preview update on thread change.
    use super::*;

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
}

mod object_expansion {
    //! Tests for async object expansion: pending/loading/expanded states, nested expansion,
    //! recursive collapse, static fields, and Esc cancellation on a loading node.
    use super::*;

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
            app.pending_expansions.values().any(|pe| pe.object_id == 42),
            "pending expansion must be registered"
        );
        let p = NavigationPathBuilder::new(FrameId(10), VarIdx(0)).build();
        assert_eq!(
            app.stack_state
                .as_ref()
                .unwrap()
                .expansion_state_for_path(&p),
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
        let p = NavigationPathBuilder::new(FrameId(10), VarIdx(0)).build();
        assert_eq!(
            app.stack_state
                .as_ref()
                .unwrap()
                .expansion_state_for_path(&p),
            ExpansionPhase::Expanded
        );
    }

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
        let p42 = NavigationPathBuilder::new(FrameId(10), VarIdx(0)).build();
        assert_eq!(
            app.stack_state
                .as_ref()
                .unwrap()
                .expansion_state_for_path(&p42),
            ExpansionPhase::Expanded
        );
        // Navigate down to the "child" field (index 1 in flat list: field_path=[1])
        app.handle_input(InputEvent::Down); // → OnObjectField{0,0,[0]} (field "x")
        app.handle_input(InputEvent::Down); // → OnObjectField{0,0,[1]} (field "child" = ObjectRef 999)
        app.handle_input(InputEvent::Enter); // start nested expansion of 999; loading not shown before threshold.
        assert!(
            app.pending_expansions
                .values()
                .any(|pe| pe.object_id == 999),
            "pending expansion for 999 must be registered"
        );
        let p999 = NavigationPathBuilder::new(FrameId(10), VarIdx(0))
            .field(FieldIdx(1))
            .build();
        assert_ne!(
            app.stack_state
                .as_ref()
                .unwrap()
                .expansion_state_for_path(&p999),
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
        let p = NavigationPathBuilder::new(FrameId(10), VarIdx(0)).build();
        assert_eq!(
            app.stack_state
                .as_ref()
                .unwrap()
                .expansion_state_for_path(&p),
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
        let p999 = NavigationPathBuilder::new(FrameId(10), VarIdx(0))
            .field(FieldIdx(1))
            .build();
        assert_eq!(
            app.stack_state
                .as_ref()
                .unwrap()
                .expansion_state_for_path(&p999),
            ExpansionPhase::Expanded
        );
        // Enter again on the same field → CollapseNestedObj(999)
        app.handle_input(InputEvent::Enter);
        assert_eq!(
            app.stack_state
                .as_ref()
                .unwrap()
                .expansion_state_for_path(&p999),
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
        assert!(
            cursor_ends_with_static_field(app.stack_state.as_ref().unwrap().cursor()),
            "expected static field cursor, got {:?}",
            app.stack_state.as_ref().unwrap().cursor()
        );

        app.handle_input(InputEvent::Enter); // expand static object ref 777
        assert!(
            app.pending_expansions
                .values()
                .any(|pe| pe.object_id == 777),
            "pending expansion for static object 777 must be registered"
        );

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !app.pending_expansions.is_empty() && std::time::Instant::now() < deadline {
            app.poll_expansions();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }

        let p777 = NavigationPathBuilder::new(FrameId(10), VarIdx(0))
            .static_field(StaticFieldIdx(0))
            .build();
        assert_eq!(
            app.stack_state
                .as_ref()
                .unwrap()
                .expansion_state_for_path(&p777),
            ExpansionPhase::Expanded
        );

        app.handle_input(InputEvent::Enter); // collapse static object ref 777
        assert_eq!(
            app.stack_state
                .as_ref()
                .unwrap()
                .expansion_state_for_path(&p777),
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
        let exp_path = NavigationPathBuilder::new(FrameId(10), VarIdx(0)).build();
        app.pending_expansions.insert(
            exp_path.clone(),
            PendingExpansion {
                rx,
                object_id: 42,
                path: exp_path,
                started: Instant::now() - EXPANSION_LOADING_THRESHOLD - Duration::from_millis(10),
                loading_shown: false,
            },
        );
        // Poll once — this triggers the Loading state deterministically.
        app.poll_expansions();
        let p = NavigationPathBuilder::new(FrameId(10), VarIdx(0)).build();
        assert_eq!(
            app.stack_state
                .as_ref()
                .unwrap()
                .expansion_state_for_path(&p),
            ExpansionPhase::Loading
        );
        app.handle_input(InputEvent::Down); // → OnObjectLoadingNode{0,0}
        app.handle_input(InputEvent::Escape); // cancel expansion (not go-back)
        assert_eq!(
            app.stack_state
                .as_ref()
                .unwrap()
                .expansion_state_for_path(&p),
            ExpansionPhase::Collapsed
        );
        // Focus must remain in StackFrames.
        assert_eq!(app.focus, Focus::StackFrames);
    }
}

mod collection_paging {
    //! Tests for collection and array pagination: chunk layout, page loading, loading
    //! indicator, Esc, Left/Right in collection entries, ObjectRef expansion, and nested collections.
    use super::*;

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
            app.poll_pages();
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
            !app.pending_expansions
                .values()
                .any(|pe| pe.object_id == 888),
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
        assert_eq!(
            cursor_chunk_section_offset(ss.cursor()),
            Some(100),
            "expected chunk section at offset 100, got {:?}",
            ss.cursor()
        );
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
                parent_path: None,
            },
        );
        // Before polling: loading_shown must be false.
        assert!(
            !app.pending_pages.get(&(888, 0)).unwrap().loading_shown,
            "before poll, loading_shown must be false"
        );
        // One poll — threshold exceeded → loading_shown set.
        // Collections at offset 0 use ChunkState, not
        // ExpansionPhase, for the loading indicator.
        app.poll_pages();
        assert!(
            app.pending_pages.get(&(888, 0)).unwrap().loading_shown,
            "after threshold, loading_shown must be true"
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
        assert!(
            cursor_ends_with_collection_entry(ss.cursor()),
            "expected collection entry cursor, got {:?}",
            ss.cursor()
        );
        // Escape → collapse collection.
        app.handle_input(InputEvent::Escape);
        let ss = app.stack_state.as_ref().unwrap();
        assert!(
            !ss.expansion.collection_chunks.contains_key(&888),
            "collection should be removed"
        );
        // Cursor returns to the collection field.
        assert!(
            cursor_ends_with_field(ss.cursor()),
            "expected object field cursor, got {:?}",
            ss.cursor()
        );
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
            cursor_ends_with_collection_entry(ss.cursor()),
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
            &RenderCursor::At(NavigationPathBuilder::new(FrameId(10), VarIdx(0)).build()),
            "escape from var-opened collection must restore var cursor"
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
            !app.pending_expansions
                .values()
                .any(|pe| pe.object_id == 888),
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
            cursor_is_chunk_section(ss.cursor()),
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
        assert!(
            cursor_ends_with_field(ss.cursor()),
            "expected object field cursor, got {:?}",
            ss.cursor()
        );
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
            app.poll_pages();
            app.poll_expansions();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let ss = app.stack_state.as_ref().unwrap();
        // Collection chunks should be gone.
        assert!(!ss.expansion.collection_chunks.contains_key(&777));
        // Should have fallen back to expand_object →
        // expansion state should be Expanded.
        let p777 = NavigationPathBuilder::new(FrameId(10), VarIdx(0))
            .field(FieldIdx(0))
            .build();
        assert_eq!(ss.expansion_state_for_path(&p777), ExpansionPhase::Expanded,);
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
        let line = StackState::format_entry_line(&map_entry, "  ", None, false);
        assert!(line.contains("[5] 42 => 100"), "map entry format: {}", line);
        // List entry.
        let list_entry = EntryInfo {
            index: 3,
            key: None,
            value: FieldValue::Int(77),
        };
        let line = StackState::format_entry_line(&list_entry, "  ", None, false);
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
            false,
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
            false,
        );
        assert!(
            line_expanded.contains("- [0]") && line_expanded.contains("String"),
            "ObjectRef expanded should show '- [0] ...': {}",
            line_expanded
        );
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
            matches!(cursor_collection_entry_ids(ss.cursor()), Some((_, 0))),
            "should be on entry 0, got {:?}",
            ss.cursor()
        );
        // The entry rendering is verified through format_entry_line above;
        // here we verify Enter triggers start_object_expansion.
        app.handle_input(InputEvent::Enter);
        // pending_expansions should contain ObjectRef id 700 (entry 0's value).
        assert!(
            app.pending_expansions
                .values()
                .any(|pe| pe.object_id == 700),
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
            cursor_is_collection_entry_field(ss.cursor()),
            "after expanding entry 0, down should reach collection entry obj field, \
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
                cursor_is_collection_entry_field(ss.cursor()),
                "expected collection entry obj field before opening nested collection, got {:?}",
                ss.cursor()
            );
        }

        app.handle_input(InputEvent::Enter); // must StartCollection(888), not StartEntryObj(888)
        assert!(
            app.pending_pages.contains_key(&(888, 0)),
            "nested collection field must trigger collection paging"
        );
        assert!(
            !app.pending_expansions
                .values()
                .any(|pe| pe.object_id == 888),
            "nested collection field must not call expand_object on collection id"
        );

        poll_all_pages(&mut app);
        app.handle_input(InputEvent::Down); // -> first nested collection entry
        {
            let ss = app.stack_state.as_ref().unwrap();
            assert!(
                matches!(cursor_collection_entry_ids(ss.cursor()), Some((888, 0))),
                "expected first nested collection entry (cid=888, idx=0), got {:?}",
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
                cursor_is_collection_entry_field(ss.cursor()),
                "escape from nested collection should restore collection entry obj field, got {:?}",
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
                matches!(cursor_collection_entry_ids(ss.cursor()), Some((890, 0))),
                "expected entry 0 on outer collection (cid=890), got {:?}",
                ss.cursor()
            );
        }

        app.handle_input(InputEvent::Enter); // must StartCollection(888), not StartEntryObj(888)
        assert!(
            app.pending_pages.contains_key(&(888, 0)),
            "nested collection entry must trigger collection paging"
        );
        assert!(
            !app.pending_expansions
                .values()
                .any(|pe| pe.object_id == 888),
            "nested collection entry must not call expand_object on collection id"
        );

        poll_all_pages(&mut app);
        app.handle_input(InputEvent::Down); // -> first entry of nested collection 888
        {
            let ss = app.stack_state.as_ref().unwrap();
            assert!(
                matches!(cursor_collection_entry_ids(ss.cursor()), Some((888, 0))),
                "expected first nested collection entry (cid=888, idx=0), got {:?}",
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
                matches!(cursor_collection_entry_ids(ss.cursor()), Some((890, 0))),
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
            !app.pending_expansions
                .values()
                .any(|pe| pe.object_id == 888),
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
            !app.pending_expansions
                .values()
                .any(|pe| pe.object_id == 888),
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
                cursor_ends_with_collection_entry(ss.cursor()),
                "expected collection entry before Left, got {:?}",
                ss.cursor()
            );
        }

        app.handle_input(InputEvent::Left);
        let ss = app.stack_state.as_ref().unwrap();
        assert_eq!(
            ss.cursor(),
            &RenderCursor::At(NavigationPathBuilder::new(FrameId(10), VarIdx(0)).build()),
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
                cursor_is_collection_entry_field(ss.cursor()),
                "expected entry object field before Left, got {:?}",
                ss.cursor()
            );
        }

        app.handle_input(InputEvent::Left);
        let ss = app.stack_state.as_ref().unwrap();
        assert!(
            matches!(cursor_collection_entry_ids(ss.cursor()), Some((889, 0))),
            "Left on primitive entry object field must navigate to parent entry, got {:?}",
            ss.cursor()
        );
    }

    #[test]
    fn enter_on_empty_collection_var_is_noop() {
        let mut app = make_var_collection_app(0);
        app.handle_input(InputEvent::Enter); // → StackFrames
        app.handle_input(InputEvent::Enter); // expand frame
        app.handle_input(InputEvent::Down); // → var row (Object[0])
        app.handle_input(InputEvent::Enter); // should be no-op

        assert!(
            app.pending_expansions.is_empty(),
            "Enter on Object[0] must not start object expansion"
        );
        assert!(
            app.pending_pages.is_empty(),
            "Enter on Object[0] must not start collection page load"
        );
    }

    #[test]
    fn right_on_empty_collection_var_is_noop() {
        let mut app = make_var_collection_app(0);
        app.handle_input(InputEvent::Enter); // → StackFrames
        app.handle_input(InputEvent::Enter); // expand frame
        app.handle_input(InputEvent::Down); // → var row (Object[0])
        app.handle_input(InputEvent::Right); // should be no-op

        assert!(
            app.pending_expansions.is_empty(),
            "Right on Object[0] must not start object expansion"
        );
        assert!(
            app.pending_pages.is_empty(),
            "Right on Object[0] must not start collection page load"
        );
    }

    #[test]
    fn enter_on_empty_collection_field_is_noop() {
        let mut app = make_collection_app(0);
        nav_to_collection_field(&mut app);
        app.handle_input(InputEvent::Enter); // should be no-op

        assert!(
            app.pending_expansions.is_empty(),
            "Enter on field Object[0] must not start expansion"
        );
        assert!(
            app.pending_pages.is_empty(),
            "Enter on field Object[0] must not start page load"
        );
    }

    fn make_static_empty_collection_app() -> App<StubEngine> {
        let frames = vec![{
            let mut f = make_frame(10);
            f.has_variables = true;
            f
        }];
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
                    name: "EMPTY".to_string(),
                    value: FieldValue::ObjectRef {
                        id: 777,
                        class_name: "ArrayList".to_string(),
                        entry_count: Some(0),
                        inline_value: None,
                    },
                }],
            );
        App::new(engine, "test.hprof".to_string())
    }

    fn nav_to_static_empty_collection(app: &mut App<StubEngine>) {
        app.handle_input(InputEvent::Enter); // → StackFrames
        app.handle_input(InputEvent::Enter); // expand frame
        app.handle_input(InputEvent::Down); // → OnVar
        app.handle_input(InputEvent::Enter); // expand object 42
        poll_all_expansions(app);
        app.handle_input(InputEvent::Down); // → x field
        app.handle_input(InputEvent::Down); // → EMPTY static field
    }

    #[test]
    fn enter_on_empty_collection_static_field_is_noop() {
        let mut app = make_static_empty_collection_app();
        nav_to_static_empty_collection(&mut app);
        assert!(
            cursor_ends_with_static_field(app.stack_state.as_ref().unwrap().cursor()),
            "expected static field cursor, got {:?}",
            app.stack_state.as_ref().unwrap().cursor()
        );
        app.handle_input(InputEvent::Enter); // should be no-op

        assert!(
            app.pending_expansions.is_empty(),
            "Enter on empty static collection must not start expansion"
        );
        assert!(
            app.pending_pages.is_empty(),
            "Enter on empty static collection must not start page load"
        );
    }

    #[test]
    fn right_on_empty_collection_static_field_is_noop() {
        let mut app = make_static_empty_collection_app();
        nav_to_static_empty_collection(&mut app);
        app.handle_input(InputEvent::Right); // should be no-op

        assert!(
            app.pending_expansions.is_empty(),
            "Right on empty static collection must not start expansion"
        );
        assert!(
            app.pending_pages.is_empty(),
            "Right on empty static collection must not start page load"
        );
    }

    fn make_empty_entry_collection_app() -> App<StubEngine> {
        let frames = vec![{
            let mut f = make_frame(10);
            f.has_variables = true;
            f
        }];
        let vars = vec![make_obj_var(0, 42)];
        // Object 42 has field items: collection 891 (1 entry with entry_count == 0)
        let expand_fields = Some(vec![FieldInfo {
            name: "items".to_string(),
            value: FieldValue::ObjectRef {
                id: 891,
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

    fn nav_to_empty_collection_entry(app: &mut App<StubEngine>) {
        app.handle_input(InputEvent::Enter); // → StackFrames
        app.handle_input(InputEvent::Enter); // expand frame
        app.handle_input(InputEvent::Down); // → OnVar
        app.handle_input(InputEvent::Enter); // expand object 42
        poll_all_expansions(app);
        app.handle_input(InputEvent::Down); // → items field
        app.handle_input(InputEvent::Enter); // open collection 891
        poll_all_pages(app);
        app.handle_input(InputEvent::Down); // → entry [0] (empty ObjectRef)
    }

    #[test]
    fn enter_on_empty_collection_entry_is_noop() {
        let mut app = make_empty_entry_collection_app();
        nav_to_empty_collection_entry(&mut app);
        app.handle_input(InputEvent::Enter); // should be no-op

        assert!(
            app.pending_expansions.is_empty(),
            "Enter on empty collection entry must not start expansion"
        );
        assert!(
            app.pending_pages.is_empty(),
            "Enter on empty collection entry must not start page load"
        );
    }

    #[test]
    fn right_on_empty_collection_entry_is_noop() {
        let mut app = make_empty_entry_collection_app();
        nav_to_empty_collection_entry(&mut app);
        app.handle_input(InputEvent::Right); // should be no-op

        assert!(
            app.pending_expansions.is_empty(),
            "Right on empty collection entry must not start expansion"
        );
        assert!(
            app.pending_pages.is_empty(),
            "Right on empty collection entry must not start page load"
        );
    }
}

mod camera {
    //! Tests for camera controls: Page Up/Down, scroll without moving cursor, centering,
    //! and no-ops in thread list or search mode.
    use super::*;

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
        assert_eq!(
            *state.cursor(),
            RenderCursor::At(NavigationPathBuilder::frame_only(FrameId(6)))
        );
        state.move_page_down();
        assert_eq!(
            *state.cursor(),
            RenderCursor::At(NavigationPathBuilder::frame_only(FrameId(16)))
        );
        state.move_page_up();
        assert_eq!(
            *state.cursor(),
            RenderCursor::At(NavigationPathBuilder::frame_only(FrameId(6)))
        );
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
}

mod loading_and_warnings {
    //! Tests for loading indicator threshold, warning emission on expansion failure or
    //! disconnected channel, and memory log formatting.
    use super::*;

    #[test]
    fn loading_indicator_not_shown_before_threshold() {
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
        let p = NavigationPathBuilder::new(FrameId(10), VarIdx(0)).build();
        assert_eq!(
            app.stack_state
                .as_ref()
                .unwrap()
                .expansion_state_for_path(&p),
            ExpansionPhase::Expanded,
            "fast expansion must complete as Expanded \
             without ever showing Loading"
        );
    }

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
        let exp_path = NavigationPathBuilder::new(FrameId(100), VarIdx(0)).build();
        app.pending_expansions.insert(
            exp_path.clone(),
            PendingExpansion {
                rx,
                object_id: 77,
                path: exp_path,
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
    fn loading_indicator_shown_if_not_yet_complete_after_threshold() {
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
        let exp_path = NavigationPathBuilder::new(FrameId(10), VarIdx(0)).build();
        app.pending_expansions.insert(
            exp_path.clone(),
            PendingExpansion {
                rx,
                object_id: 99,
                path: exp_path.clone(),
                started: Instant::now() - EXPANSION_LOADING_THRESHOLD - Duration::from_millis(10),
                loading_shown: false,
            },
        );
        // Poll once — threshold exceeded, loading_shown transitions to true.
        app.poll_expansions();
        // Verify loading_shown was set.
        let pe = app.pending_expansions.get(&exp_path).unwrap();
        assert!(
            pe.loading_shown,
            "loading_shown must be set after threshold"
        );
    }
}

mod favorites {
    //! Tests for the favorites panel: toggle visibility/focus, help overlay, navigate-to-source
    //! (exact thread, field, collection entry, stale path, nested entries, duplicate name resolution),
    //! and snapshot page-limit enforcement.
    use super::*;

    #[test]
    fn hidden_favorites_panel_forces_focus_back_to_previous_panel() {
        use crate::favorites::{PinnedItem, PinnedSnapshot};
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
            local_collapsed: HashSet::new(),
            hidden_fields: HashSet::new(),
            show_hidden: false,
            key: make_pin_key_var(1, "main", 1, 0),
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
    fn tab_from_favorites_cycles_to_thread_list() {
        use crate::favorites::{PinnedItem, PinnedSnapshot};

        let engine = StubEngine::with_threads(&["main"]);
        let mut app = App::new(engine, "test.hprof".to_string());
        app.pinned.push(PinnedItem {
            thread_name: "main".to_string(),
            frame_label: "Thread.run()".to_string(),
            item_label: "var[0]".to_string(),
            snapshot: PinnedSnapshot::Primitive {
                value_label: "42".to_string(),
            },
            local_collapsed: HashSet::new(),
            hidden_fields: HashSet::new(),
            show_hidden: false,
            key: make_pin_key_var(1, "main", 1, 0),
        });
        app.last_area_width = MIN_WIDTH_FAVORITES_PANEL;
        app.focus = Focus::Favorites;

        app.handle_input(InputEvent::Tab);
        assert_eq!(app.focus, Focus::ThreadList);
    }

    #[test]
    fn favorites_navigate_to_source_empty_list_no_panic() {
        let engine = StubEngine::with_threads(&["main"]);
        let mut app = App::new(engine, "test.hprof".to_string());
        app.focus = Focus::Favorites;

        let action = app.handle_input(InputEvent::NavigateToSource);

        assert_eq!(action, AppAction::Continue);
        assert_eq!(app.focus, Focus::Favorites);
        assert!(app.stack_state.is_none());
    }

    #[test]
    fn favorites_navigate_to_source_zero_match_emits_warning() {
        let engine = StubEngine::with_threads(&["main"]);
        let mut app = App::new(engine, "test.hprof".to_string());
        // thread_id=2 — does not exist in this engine (only thread serial=1)
        app.pinned
            .push(make_favorite_item_with_tid(2, "worker", 10));
        app.sync_favorites_selection();
        app.focus = Focus::Favorites;

        app.handle_input(InputEvent::NavigateToSource);

        assert_eq!(
            app.ui_status.as_deref(),
            Some("Thread 'worker' no longer found")
        );
        assert_eq!(app.focus, Focus::Favorites);
        assert!(app.stack_state.is_none());
    }

    #[test]
    fn favorites_navigate_to_source_selects_correct_thread() {
        let engine = StubEngine::with_thread_specific_frames(
            &["main", "worker"],
            &[(1, vec![make_frame(11)]), (2, vec![make_frame(22)])],
        );
        let mut app = App::new(engine, "test.hprof".to_string());
        // "worker" is thread serial=2 in this engine
        app.pinned
            .push(make_favorite_item_with_tid(2, "worker", 22));
        app.sync_favorites_selection();
        app.focus = Focus::Favorites;

        app.handle_input(InputEvent::NavigateToSource);

        assert_eq!(app.focus, Focus::StackFrames);
        assert!(app.ui_status.is_none());
        assert_eq!(
            app.stack_state.as_ref().unwrap().selected_frame_id(),
            Some(22)
        );
        assert_eq!(app.thread_list.selected_serial(), Some(2));
    }

    #[test]
    fn favorites_navigate_to_source_positions_on_field_when_possible() {
        let frames = vec![make_frame(10)];
        let vars = vec![make_obj_var(0, 42)];
        let engine = StubEngine::with_threads_and_frames(&["main"], frames).with_vars(10, vars);
        let mut app = App::new(engine, "test.hprof".to_string());
        app.pinned
            .push(make_field_favorite_item("main", 10, 0, vec![1]));
        app.sync_favorites_selection();
        app.focus = Focus::Favorites;

        app.handle_input(InputEvent::NavigateToSource);
        poll_navigation_to_completion(&mut app);

        assert_eq!(app.focus, Focus::StackFrames);
        assert_eq!(
            app.stack_state.as_ref().unwrap().cursor(),
            &RenderCursor::At(
                NavigationPathBuilder::new(FrameId(10), VarIdx(0))
                    .field(FieldIdx(1))
                    .build()
            )
        );
    }

    #[test]
    fn favorites_navigate_to_source_positions_on_collection_entry() {
        use crate::favorites::{PinnedItem, PinnedSnapshot};

        let frames = vec![make_frame(10)];
        let vars = vec![make_obj_var(0, 42)];
        let engine = StubEngine::with_threads_and_frames(&["main"], frames)
            .with_vars(10, vars)
            .with_expand(
                42,
                Some(vec![FieldInfo {
                    name: "items".to_string(),
                    value: FieldValue::ObjectRef {
                        id: 889,
                        class_name: "Object[]".to_string(),
                        entry_count: Some(3),
                        inline_value: None,
                    },
                }]),
            );
        let mut app = App::new(engine, "test.hprof".to_string());
        app.pinned.push(PinnedItem {
            thread_name: "main".to_string(),
            frame_label: "Thread.run()".to_string(),
            item_label: "var[0][1]".to_string(),
            snapshot: PinnedSnapshot::Primitive {
                value_label: "dummy".to_string(),
            },
            local_collapsed: HashSet::new(),
            hidden_fields: HashSet::new(),
            show_hidden: false,
            key: make_pin_key_coll_entry(1, "main", 10, 0, &[0], 889, 1),
        });
        app.sync_favorites_selection();
        app.focus = Focus::Favorites;

        app.handle_input(InputEvent::NavigateToSource);
        poll_navigation_to_completion(&mut app);

        assert_eq!(app.focus, Focus::StackFrames);
        assert_eq!(
            app.stack_state.as_ref().unwrap().cursor(),
            &RenderCursor::At(
                NavigationPathBuilder::new(FrameId(10), VarIdx(0))
                    .field(FieldIdx(0))
                    .collection_entry(CollectionId(889), EntryIdx(1))
                    .build()
            )
        );
    }

    #[test]
    fn favorites_navigate_to_source_positions_on_collection_entry_obj_field() {
        use crate::favorites::{PinnedItem, PinnedSnapshot};

        let frames = vec![make_frame(10)];
        let vars = vec![make_obj_var(0, 42)];
        let engine = StubEngine::with_threads_and_frames(&["main"], frames)
            .with_vars(10, vars)
            .with_expand(
                42,
                Some(vec![FieldInfo {
                    name: "items".to_string(),
                    value: FieldValue::ObjectRef {
                        id: 889,
                        class_name: "Object[]".to_string(),
                        entry_count: Some(3),
                        inline_value: None,
                    },
                }]),
            );
        let mut app = App::new(engine, "test.hprof".to_string());
        app.pinned.push(PinnedItem {
            thread_name: "main".to_string(),
            frame_label: "Thread.run()".to_string(),
            item_label: "var[0][1].child".to_string(),
            snapshot: PinnedSnapshot::Primitive {
                value_label: "dummy".to_string(),
            },
            local_collapsed: HashSet::new(),
            hidden_fields: HashSet::new(),
            show_hidden: false,
            key: make_pin_key_coll_entry_field(1, "main", 10, 0, &[0], 889, 1, &[1]),
        });
        app.sync_favorites_selection();
        app.focus = Focus::Favorites;

        app.handle_input(InputEvent::NavigateToSource);
        poll_navigation_to_completion(&mut app);

        assert_eq!(app.focus, Focus::StackFrames);
        assert_eq!(
            app.stack_state.as_ref().unwrap().cursor(),
            &RenderCursor::At(
                NavigationPathBuilder::new(FrameId(10), VarIdx(0))
                    .field(FieldIdx(0))
                    .collection_entry(CollectionId(889), EntryIdx(1))
                    .field(FieldIdx(1))
                    .build()
            )
        );
    }

    #[test]
    fn favorites_navigate_to_source_collection_entry_with_stale_path_uses_semantic_match() {
        use crate::favorites::{PinnedItem, PinnedSnapshot};

        let frames = vec![make_frame(10)];
        let vars = vec![VariableInfo {
            index: 0,
            value: VariableValue::ObjectRef {
                id: 889,
                class_name: "Object[]".to_string(),
                entry_count: Some(3),
            },
        }];
        let engine = StubEngine::with_threads_and_frames(&["main"], frames).with_vars(10, vars);
        let mut app = App::new(engine, "test.hprof".to_string());
        app.pinned.push(PinnedItem {
            thread_name: "main".to_string(),
            frame_label: "Thread.run()".to_string(),
            item_label: "var[0][1]".to_string(),
            snapshot: PinnedSnapshot::Primitive {
                value_label: "dummy".to_string(),
            },
            local_collapsed: HashSet::new(),
            hidden_fields: HashSet::new(),
            show_hidden: false,
            // Var at index 0 is directly the collection (no field hops).
            key: make_pin_key_coll_entry(1, "main", 10, 0, &[], 889, 1),
        });
        app.sync_favorites_selection();
        app.focus = Focus::Favorites;

        app.handle_input(InputEvent::NavigateToSource);
        poll_navigation_to_completion(&mut app);

        assert_eq!(app.focus, Focus::StackFrames);
        assert_eq!(
            app.stack_state.as_ref().unwrap().cursor(),
            &RenderCursor::At(
                NavigationPathBuilder::new(FrameId(10), VarIdx(0))
                    .collection_entry(CollectionId(889), EntryIdx(1))
                    .build()
            )
        );
    }

    #[test]
    fn favorites_navigate_to_source_nested_collection_entry_uses_restore_cursor_chain() {
        use crate::favorites::{PinnedItem, PinnedSnapshot};

        let frames = vec![make_frame(10)];
        let vars = vec![VariableInfo {
            index: 0,
            value: VariableValue::ObjectRef {
                id: 889,
                class_name: "Object[]".to_string(),
                entry_count: Some(3),
            },
        }];
        let engine = StubEngine::with_threads_and_frames(&["main"], frames)
            .with_vars(10, vars)
            .with_expand(
                701,
                Some(vec![FieldInfo {
                    name: "inner".to_string(),
                    value: FieldValue::ObjectRef {
                        id: 890,
                        class_name: "Object[]".to_string(),
                        entry_count: Some(1),
                        inline_value: None,
                    },
                }]),
            );
        let mut app = App::new(engine, "test.hprof".to_string());
        app.pinned.push(PinnedItem {
            thread_name: "main".to_string(),
            frame_label: "Thread.run()".to_string(),
            item_label: "var[0][1].inner[0]".to_string(),
            snapshot: PinnedSnapshot::Primitive {
                value_label: "dummy".to_string(),
            },
            local_collapsed: HashSet::new(),
            hidden_fields: HashSet::new(),
            show_hidden: false,
            // var[0][1].inner (field 0 of entry 1's object) is collection 890, entry 0.
            key: crate::favorites::PinKey {
                thread_id: ThreadId(1),
                thread_name: "main".to_string(),
                nav_path: NavigationPathBuilder::new(FrameId(10), VarIdx(0))
                    .collection_entry(CollectionId(889), EntryIdx(1))
                    .field(FieldIdx(0))
                    .collection_entry(CollectionId(890), EntryIdx(0))
                    .build(),
            },
        });
        app.sync_favorites_selection();
        app.focus = Focus::Favorites;

        app.handle_input(InputEvent::NavigateToSource);
        poll_navigation_to_completion(&mut app);

        assert_eq!(app.focus, Focus::StackFrames);
        assert_eq!(
            app.stack_state.as_ref().unwrap().cursor(),
            &RenderCursor::At(
                NavigationPathBuilder::new(FrameId(10), VarIdx(0))
                    .collection_entry(CollectionId(889), EntryIdx(1))
                    .field(FieldIdx(0))
                    .collection_entry(CollectionId(890), EntryIdx(0))
                    .build()
            )
        );
    }

    #[test]
    fn favorites_navigate_to_source_navigates_by_thread_id_when_names_duplicate() {
        // Two threads with the same name "dup"; navigation uses thread_id (serial),
        // not thread_name, so there is no ambiguity.
        let engine = StubEngine::with_thread_specific_frames(
            &["dup", "dup"],
            &[(1, vec![make_frame(11)]), (2, vec![make_frame(22)])],
        );
        let mut app = App::new(engine, "test.hprof".to_string());
        // thread_id=1 → "dup" serial=1, frame_id=11
        app.pinned.push(make_favorite_item("dup", 11));
        app.sync_favorites_selection();
        app.focus = Focus::Favorites;

        app.handle_input(InputEvent::NavigateToSource);

        assert_eq!(app.focus, Focus::StackFrames);
        assert!(
            app.ui_status.is_none(),
            "no warning expected for thread_id-based navigation"
        );
        assert_eq!(
            app.stack_state.as_ref().unwrap().selected_frame_id(),
            Some(11),
            "should navigate to frame_id=11 (thread serial=1)"
        );
    }

    #[test]
    fn favorites_navigate_to_source_frame_positioning_found() {
        let engine = StubEngine::with_thread_specific_frames(
            &["main"],
            &[(1, vec![make_frame(10), make_frame(20)])],
        );
        let mut app = App::new(engine, "test.hprof".to_string());
        app.pinned.push(make_favorite_item("main", 20));
        app.sync_favorites_selection();
        app.focus = Focus::Favorites;

        app.handle_input(InputEvent::NavigateToSource);

        assert_eq!(app.focus, Focus::StackFrames);
        assert_eq!(
            app.stack_state.as_ref().unwrap().selected_frame_id(),
            Some(20)
        );
    }

    #[test]
    fn favorites_f_last_item_empty_panel_focus() {
        let frames = vec![make_frame(10)];
        let engine = StubEngine::with_threads_and_frames(&["main"], frames);
        let mut app = App::new(engine, "test.hprof".to_string());
        app.handle_input(InputEvent::Enter);
        assert_eq!(app.focus, Focus::StackFrames);
        app.pinned.push(make_favorite_item("main", 10));
        app.sync_favorites_selection();
        app.focus = Focus::Favorites;

        app.handle_input(InputEvent::ToggleFavorite);
        assert_eq!(app.focus, Focus::StackFrames);

        let engine = StubEngine::with_threads(&["main"]);
        let mut app = App::new(engine, "test.hprof".to_string());
        app.pinned.push(make_favorite_item("main", 10));
        app.sync_favorites_selection();
        app.focus = Focus::Favorites;

        app.handle_input(InputEvent::ToggleFavorite);
        assert_eq!(app.focus, Focus::ThreadList);
    }

    #[test]
    fn snapshot_chunk_page_limit_respected() {
        use crate::favorites::{PinnedItem, PinnedSnapshot};
        use crate::views::stack_view::{ChunkState, CollectionChunks};

        let engine = StubEngine::with_threads(&["main"]);
        let mut app = App::new(engine, "test.hprof".to_string());

        let collection_id = 0x55;
        let mut chunk_pages = HashMap::new();
        for i in 0..SNAPSHOT_CHUNK_PAGE_LIMIT {
            let offset = 100 * (i + 1);
            chunk_pages.insert(
                offset,
                ChunkState::Loaded(CollectionPage {
                    entries: vec![EntryInfo {
                        index: offset,
                        key: None,
                        value: FieldValue::Int(offset as i32),
                    }],
                    total_count: 10_000,
                    offset,
                    has_more: true,
                }),
            );
        }

        app.pinned.push(PinnedItem {
            thread_name: "main".to_string(),
            frame_label: "Thread.run()".to_string(),
            item_label: "var[0]".to_string(),
            snapshot: PinnedSnapshot::Subtree {
                root_id: 1,
                object_fields: HashMap::new(),
                object_static_fields: HashMap::new(),
                collection_chunks: HashMap::from([(
                    collection_id,
                    CollectionChunks {
                        total_count: 10_000,
                        eager_page: None,
                        chunk_pages,
                    },
                )]),
                truncated: false,
            },
            local_collapsed: HashSet::new(),
            hidden_fields: HashSet::new(),
            show_hidden: false,
            key: make_pin_key_var(1, "main", 1, 0),
        });

        let new_offset = 9_999usize;
        let (tx, rx) = mpsc::channel();
        tx.send(Some(CollectionPage {
            entries: vec![EntryInfo {
                index: new_offset,
                key: None,
                value: FieldValue::Int(1),
            }],
            total_count: 10_000,
            offset: new_offset,
            has_more: false,
        }))
        .unwrap();
        app.pending_pinned_pages.insert(
            (0, collection_id, new_offset),
            PendingPage {
                rx,
                started: Instant::now(),
                loading_shown: false,
                parent_path: None,
            },
        );

        app.poll_pages();

        let PinnedSnapshot::Subtree {
            collection_chunks, ..
        } = &app.pinned[0].snapshot
        else {
            panic!("expected subtree snapshot");
        };
        let cc = collection_chunks
            .get(&collection_id)
            .expect("collection must exist in pinned snapshot");
        assert_eq!(cc.chunk_pages.len(), SNAPSHOT_CHUNK_PAGE_LIMIT);
        assert!(
            !cc.chunk_pages.contains_key(&new_offset),
            "chunk beyond snapshot page cap must not be inserted"
        );
    }

    fn make_collection_pinned_item(collection_id: u64, total_count: u64) -> PinnedItem {
        use crate::views::stack_view::CollectionChunks;

        let eager_entries: Vec<EntryInfo> = (0..total_count.min(100))
            .map(|i| EntryInfo {
                index: i as usize,
                key: None,
                value: FieldValue::Int(i as i32),
            })
            .collect();
        let eager_page = Some(CollectionPage {
            entries: eager_entries,
            total_count,
            offset: 0,
            has_more: total_count > 100,
        });
        PinnedItem {
            thread_name: "main".to_string(),
            frame_label: "Thread.run()".to_string(),
            item_label: "var[0]".to_string(),
            snapshot: PinnedSnapshot::Subtree {
                root_id: collection_id,
                object_fields: HashMap::new(),
                object_static_fields: HashMap::new(),
                collection_chunks: HashMap::from([(
                    collection_id,
                    CollectionChunks {
                        total_count,
                        eager_page,
                        chunk_pages: HashMap::new(),
                    },
                )]),
                truncated: false,
            },
            local_collapsed: HashSet::new(),
            hidden_fields: HashSet::new(),
            show_hidden: false,
            key: make_pin_key_var(1, "main", 1, 0),
        }
    }

    /// Render the app to populate favorites panel state
    /// (row_kind_maps, chunk_sentinel_maps, etc.).
    fn render_app(app: &mut App<StubEngine>) {
        use ratatui::{Terminal, backend::TestBackend};
        let backend = TestBackend::new(200, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| app.render(f)).unwrap();
    }

    #[test]
    fn favorites_snapshot_hides_unloaded_chunk_sentinels() {
        let engine = StubEngine::with_threads(&["main"]);
        let mut app = App::new(engine, "test.hprof".to_string());

        // 250 entries → eager page has [0..100], chunks [100..200]
        // and [200..250] are NOT loaded in the snapshot.
        app.pinned.push(make_collection_pinned_item(0xAA, 250));
        app.sync_favorites_selection();
        app.focus = Focus::Favorites;

        render_app(&mut app);

        // Navigate through all rows — no sentinel should appear.
        for _ in 0..120 {
            app.handle_input(InputEvent::Down);
        }
        assert!(
            app.favorites_list_state.current_chunk_sentinel().is_none(),
            "unloaded chunk sentinels must be hidden in snapshot mode"
        );
        assert!(
            app.pending_pinned_pages.is_empty(),
            "no prefetch should occur in snapshot mode"
        );
    }

    // 6.16
    #[test]
    fn handle_favorites_input_h_noop_when_no_pinned_items() {
        let engine = StubEngine::with_threads(&["main"]);
        let mut app = App::new(engine, "test.hprof".to_string());
        app.focus = Focus::Favorites;

        // No panic expected; pinned remains empty.
        app.handle_input(InputEvent::SearchChar('h'));
        assert!(app.pinned.is_empty());
    }
}

mod focus {
    //! Tests for focus management: Tab cycling between panels, key routing in search mode,
    //! Quit from various contexts, and object-ID toggle.
    use super::*;

    #[test]
    fn handle_input_quit_returns_app_action_quit() {
        let engine = StubEngine::with_threads(&["main"]);
        let mut app = App::new(engine, "test.hprof".to_string());
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
    fn toggle_object_ids_noop_outside_stack_frames_focus() {
        let engine = StubEngine::with_threads(&["main"]);
        let mut app = App::new(engine, "test.hprof".to_string());
        assert!(!app.show_object_ids);

        app.handle_input(InputEvent::ToggleObjectIds);

        assert!(!app.show_object_ids);
    }

    #[test]
    fn toggle_object_ids_in_stack_frames_focus_toggles_flag() {
        let frames = vec![make_frame(10)];
        let engine = StubEngine::with_threads_and_frames(&["main"], frames);
        let mut app = App::new(engine, "test.hprof".to_string());
        app.handle_input(InputEvent::Enter);
        assert_eq!(app.focus, Focus::StackFrames);
        assert!(!app.show_object_ids);

        app.handle_input(InputEvent::ToggleObjectIds);
        assert!(app.show_object_ids);

        app.handle_input(InputEvent::ToggleObjectIds);
        assert!(!app.show_object_ids);
    }
}

mod async_navigation {
    //! Task 5 tests (5.1–5.12) for story 9-11: async go-to-pin and scroll-to-target.
    use super::*;

    fn poll_all_expansions(app: &mut App<StubEngine>) {
        poll_all_expansions_top(app);
    }

    // -------------------------------------------------------------------------
    // 5.1 — navigate_to_path defers (non-blocking) when expansion is required.
    // -------------------------------------------------------------------------
    #[test]
    fn navigate_to_path_defers_on_unexpanded_object() {
        let frames = vec![make_frame(10)];
        let vars = vec![make_obj_var(0, 42)];
        let engine = StubEngine::with_threads_and_frames(&["main"], frames).with_vars(10, vars);
        let mut app = App::new(engine, "test.hprof".to_string());
        app.pinned
            .push(make_field_favorite_item("main", 10, 0, vec![1]));
        app.sync_favorites_selection();
        app.focus = Focus::Favorites;

        // NavigateToSource should defer immediately (object 42 not yet expanded).
        app.handle_input(InputEvent::NavigateToSource);

        // Walk deferred — pending_navigation is set and spinner is active.
        assert!(
            app.pending_navigation.is_some(),
            "pending_navigation must be set on deferral"
        );
        assert_eq!(
            app.spinner_state,
            SpinnerState::NavigatingToPin,
            "spinner must show NavigatingToPin during async wait"
        );
        let awaited = &app.pending_navigation.as_ref().unwrap().awaited;
        assert_eq!(
            *awaited,
            AwaitedResource::ObjectExpansion(42),
            "must await expansion of object 42"
        );
    }

    // -------------------------------------------------------------------------
    // 5.2 — after full walk, cursor_index matches the target in flat_items.
    // -------------------------------------------------------------------------
    #[test]
    fn cursor_index_matches_target_after_full_walk() {
        let frames = vec![make_frame(10)];
        let vars = vec![make_obj_var(0, 42)];
        let engine = StubEngine::with_threads_and_frames(&["main"], frames).with_vars(10, vars);
        let mut app = App::new(engine, "test.hprof".to_string());
        app.pinned
            .push(make_field_favorite_item("main", 10, 0, vec![1]));
        app.sync_favorites_selection();
        app.focus = Focus::Favorites;

        app.handle_input(InputEvent::NavigateToSource);
        poll_navigation_to_completion(&mut app);

        let ss = app.stack_state.as_ref().unwrap();
        let target = RenderCursor::At(
            NavigationPathBuilder::new(FrameId(10), VarIdx(0))
                .field(FieldIdx(1))
                .build(),
        );
        let flat = ss.flat_items();
        let idx = flat.iter().position(|c| c == &target);
        assert!(
            idx.is_some(),
            "target cursor must appear in flat_items after walk"
        );
        let cursor_idx = flat.iter().position(|c| c == ss.cursor());
        assert_eq!(cursor_idx, idx, "cursor_index must match target position");
    }

    // -------------------------------------------------------------------------
    // 5.3 — App-level scroll: cursor placed in upper third after navigation.
    // -------------------------------------------------------------------------
    #[test]
    fn scroll_positions_cursor_in_upper_third_after_navigation() {
        let frames = vec![make_frame(10)];
        let vars = vec![make_obj_var(0, 42)];
        let engine = StubEngine::with_threads_and_frames(&["main"], frames).with_vars(10, vars);
        let mut app = App::new(engine, "test.hprof".to_string());
        // Open the stack first from ThreadList focus so stack_state is Some.
        app.handle_input(InputEvent::Enter); // ThreadList → opens stack
        // Set visible height before navigation (persists since stack won't be recreated).
        if let Some(s) = &mut app.stack_state {
            s.set_visible_height(3);
        }

        app.pinned
            .push(make_field_favorite_item("main", 10, 0, vec![1]));
        app.sync_favorites_selection();
        app.focus = Focus::Favorites;

        app.handle_input(InputEvent::NavigateToSource);
        poll_navigation_to_completion(&mut app);

        // After navigation, scroll offset must be consistent with upper-third rule.
        // With visible_height=3, upper_third=1. Offset = target_idx - 1 (clamped).
        if let Some(s) = app.stack_state.as_ref() {
            let flat = s.flat_items();
            let target = RenderCursor::At(
                NavigationPathBuilder::new(FrameId(10), VarIdx(0))
                    .field(FieldIdx(1))
                    .build(),
            );
            if let Some(target_idx) = flat.iter().position(|c| c == &target) {
                let offset = s.list_state_offset_for_test();
                let expected = target_idx
                    .saturating_sub(1)
                    .min(flat.len().saturating_sub(3));
                assert_eq!(
                    offset,
                    expected,
                    "offset must place cursor in upper third (target at {target_idx}, \
                    flat.len={})",
                    flat.len()
                );
            } else {
                panic!("target Field(1) not found in flat_items after nav; flat={flat:?}");
            }
        }
    }

    // -------------------------------------------------------------------------
    // 5.4 — Escape during pending navigation clears state; cursor stays.
    // -------------------------------------------------------------------------
    #[test]
    fn escape_during_pending_nav_clears_state_cursor_stays() {
        let frames = vec![make_frame(10)];
        let vars = vec![make_obj_var(0, 42)];
        let engine = StubEngine::with_threads_and_frames(&["main"], frames).with_vars(10, vars);
        let mut app = App::new(engine, "test.hprof".to_string());
        app.pinned
            .push(make_field_favorite_item("main", 10, 0, vec![1]));
        app.sync_favorites_selection();
        app.focus = Focus::Favorites;

        app.handle_input(InputEvent::NavigateToSource);
        assert!(
            app.pending_navigation.is_some(),
            "must be pending before Esc"
        );

        // Escape cancels navigation.
        app.handle_input(InputEvent::Escape);

        assert!(
            app.pending_navigation.is_none(),
            "pending_navigation must be cleared"
        );
        assert_eq!(
            app.spinner_state,
            SpinnerState::Idle,
            "spinner must be idle"
        );

        // Cursor stays at last resolved step (Var(0)).
        let ss = app.stack_state.as_ref().unwrap();
        let cursor = ss.cursor();
        assert!(
            matches!(
                cursor,
                RenderCursor::At(p) if matches!(
                    p.segments().last(),
                    Some(PathSegment::Var(_))
                )
            ),
            "cursor must remain at last resolved step (Var), got: {cursor:?}"
        );
    }

    // -------------------------------------------------------------------------
    // 5.5 — Walk completes in one pass when all steps are already cached.
    // -------------------------------------------------------------------------
    #[test]
    fn walk_completes_in_one_pass_when_steps_cached() {
        let frames = vec![make_frame(10)];
        let vars = vec![make_obj_var(0, 42)];
        let engine = StubEngine::with_threads_and_frames(&["main"], frames).with_vars(10, vars);
        let mut app = App::new(engine, "test.hprof".to_string());

        // Navigate to StackFrames to open stack, then pre-expand object 42.
        app.handle_input(InputEvent::Enter);
        app.handle_input(InputEvent::Enter); // expand frame
        let exp_path_42 = NavigationPathBuilder::new(FrameId(10), VarIdx(0)).build();
        app.start_object_expansion(42, exp_path_42);
        poll_all_expansions(&mut app);

        // Object 42 is now Expanded — walk should complete without deferral.
        app.pinned
            .push(make_field_favorite_item("main", 10, 0, vec![1]));
        app.sync_favorites_selection();
        app.focus = Focus::Favorites;

        app.handle_input(InputEvent::NavigateToSource);

        // No pending navigation — walk completed synchronously.
        assert!(
            app.pending_navigation.is_none(),
            "no pending_navigation when all steps are cached"
        );
        assert_eq!(
            app.spinner_state,
            SpinnerState::Idle,
            "spinner must be idle after sync completion"
        );
        let ss = app.stack_state.as_ref().unwrap();
        assert_eq!(
            ss.cursor(),
            &RenderCursor::At(
                NavigationPathBuilder::new(FrameId(10), VarIdx(0))
                    .field(FieldIdx(1))
                    .build()
            ),
            "cursor must be at Field(1)"
        );
    }

    // -------------------------------------------------------------------------
    // 5.6 — In-frame continuation cap: walk yields after 10 consecutive steps.
    // -------------------------------------------------------------------------
    #[test]
    fn in_frame_cap_yields_after_10_steps() {
        let frames = vec![make_frame(10)];
        let vars = vec![make_obj_var(0, 42)];
        // StubEngine default: any object → [x:Int, child:ObjectRef(999)].
        // object 999 → field 1 = ObjectRef(999) again (self-referential via default).
        let engine = StubEngine::with_threads_and_frames(&["main"], frames).with_vars(10, vars);
        let mut app = App::new(engine, "test.hprof".to_string());

        // Open stack + pre-expand 42 and 999 so all Field hops are cached.
        app.handle_input(InputEvent::Enter);
        app.handle_input(InputEvent::Enter); // expand frame 10
        let exp_path_42 = NavigationPathBuilder::new(FrameId(10), VarIdx(0)).build();
        app.start_object_expansion(42, exp_path_42);
        poll_all_expansions(&mut app);
        let exp_path_999 = NavigationPathBuilder::new(FrameId(10), VarIdx(0))
            .field(FieldIdx(1))
            .build();
        app.start_object_expansion(999, exp_path_999);
        poll_all_expansions(&mut app);
        // Path-based expansion: 999 must be marked Expanded
        // at each intermediate depth so the walk can proceed
        // without deferring for async expansion.
        {
            let ss = app.stack_state.as_mut().unwrap();
            for depth in 2..=8 {
                let mut b = NavigationPathBuilder::new(FrameId(10), VarIdx(0));
                for _ in 0..depth {
                    b = b.field(FieldIdx(1));
                }
                ss.expansion
                    .expansion_phases
                    .insert(b.build(), ExpansionPhase::Expanded);
            }
        }

        // Build path: Frame(10) → Var(0) → Field(1) × 9 = 11 segments total.
        // Steps: Frame=1, Var=2, then 9 Fields → step_count=10 before Field[9].
        // At segment index 10 (Field[9]): step_count=10 ≥ 10 → yield with Continue.
        let nav_path = NavigationPathBuilder::new(FrameId(10), VarIdx(0))
            .field(FieldIdx(1))
            .field(FieldIdx(1))
            .field(FieldIdx(1))
            .field(FieldIdx(1))
            .field(FieldIdx(1))
            .field(FieldIdx(1))
            .field(FieldIdx(1))
            .field(FieldIdx(1))
            .field(FieldIdx(1))
            .build();
        let pin_key = crate::favorites::PinKey {
            thread_id: ThreadId(1),
            thread_name: "main".to_string(),
            nav_path,
        };
        app.pinned.push(crate::favorites::PinnedItem {
            thread_name: "main".to_string(),
            frame_label: "Thread.run()".to_string(),
            item_label: "deep".to_string(),
            snapshot: crate::favorites::PinnedSnapshot::Primitive {
                value_label: "x".to_string(),
            },
            local_collapsed: HashSet::new(),
            hidden_fields: HashSet::new(),
            show_hidden: false,
            key: pin_key,
        });
        app.sync_favorites_selection();
        app.focus = Focus::Favorites;

        app.handle_input(InputEvent::NavigateToSource);

        // Should have yielded with Continue (not async expansion/page).
        let pending = app.pending_navigation.as_ref();
        assert!(pending.is_some(), "must yield after 10 steps");
        assert_eq!(
            pending.unwrap().awaited,
            AwaitedResource::Continue,
            "must yield with Continue variant after in-frame cap"
        );
    }

    // -------------------------------------------------------------------------
    // 5.7 — Stale context triggers retry with original_path.
    // -------------------------------------------------------------------------
    #[test]
    fn stale_prereq_triggers_retry_with_original_path() {
        let frames = vec![make_frame(10)];
        let vars = vec![make_obj_var(0, 42)];
        // Default engine: 42→[x,child:999], 999→[x,child:999]
        let engine = StubEngine::with_threads_and_frames(&["main"], frames).with_vars(10, vars);
        let mut app = App::new(engine, "test.hprof".to_string());

        // Pin: Frame(10) → Var(0) → Field(1) → Field(0)
        // Field(1) on 42 = child(999); Field(0) on 999 = x(Int)
        let pin_key = crate::favorites::PinKey {
            thread_id: ThreadId(1),
            thread_name: "main".to_string(),
            nav_path: NavigationPathBuilder::new(FrameId(10), VarIdx(0))
                .field(FieldIdx(1))
                .field(FieldIdx(0))
                .build(),
        };
        app.pinned.push(crate::favorites::PinnedItem {
            thread_name: "main".to_string(),
            frame_label: "Thread.run()".to_string(),
            item_label: "var[0].child.x".to_string(),
            snapshot: crate::favorites::PinnedSnapshot::Primitive {
                value_label: "0".to_string(),
            },
            local_collapsed: HashSet::new(),
            hidden_fields: HashSet::new(),
            show_hidden: false,
            key: pin_key,
        });
        app.sync_favorites_selection();
        app.focus = Focus::Favorites;

        // Step 1: NavigateToSource → defers waiting for ObjectExpansion(42).
        app.handle_input(InputEvent::NavigateToSource);
        assert!(
            matches!(
                app.pending_navigation.as_ref().map(|p| &p.awaited),
                Some(AwaitedResource::ObjectExpansion(42))
            ),
            "must await expansion of 42"
        );

        // Step 2: 42 expands → resume → processes Field(1), prereq_expanded=[42],
        // then hits Field(0) on 999 → defers waiting for ObjectExpansion(999).
        // Poll only until 42's expansion completes (not 999).
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while app.pending_expansions.values().any(|pe| pe.object_id == 42)
            && std::time::Instant::now() < deadline
        {
            app.poll_expansions();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }

        assert!(
            matches!(
                app.pending_navigation.as_ref().map(|p| &p.awaited),
                Some(AwaitedResource::ObjectExpansion(999))
            ),
            "must await expansion of 999 at second step; pending={:?}",
            app.pending_navigation.as_ref().map(|p| &p.awaited)
        );
        // prereq_expanded must contain path for 42.
        let p42 = NavigationPathBuilder::new(FrameId(10), VarIdx(0)).build();
        assert!(
            app.pending_navigation
                .as_ref()
                .unwrap()
                .prereq_expanded
                .contains(&p42),
            "prereq_expanded must include path for 42"
        );

        // Step 3: Simulate stale context — collapse object 42.
        app.stack_state.as_mut().unwrap().collapse_object(&p42);

        // Step 4: 999 expands → resume detects 42 is no longer Expanded → stale.
        poll_all_expansions(&mut app);

        assert_eq!(
            app.ui_status.as_deref(),
            Some("Pin context changed, retrying..."),
            "must show stale context message"
        );
    }

    // -------------------------------------------------------------------------
    // 5.8 — Async expansion failure terminates walk and shows error.
    // -------------------------------------------------------------------------
    #[test]
    fn async_expansion_failure_clears_nav_and_shows_error() {
        let frames = vec![make_frame(10)];
        let vars = vec![make_obj_var(0, 42)];
        let engine = StubEngine::with_threads_and_frames(&["main"], frames)
            .with_vars(10, vars)
            .with_expand(42, None); // force expansion failure
        let mut app = App::new(engine, "test.hprof".to_string());
        app.pinned
            .push(make_field_favorite_item("main", 10, 0, vec![1]));
        app.sync_favorites_selection();
        app.focus = Focus::Favorites;

        app.handle_input(InputEvent::NavigateToSource);
        assert!(app.pending_navigation.is_some(), "must defer before poll");

        poll_all_expansions(&mut app);

        assert!(
            app.pending_navigation.is_none(),
            "pending_navigation must be cleared on failure"
        );
        assert_eq!(
            app.spinner_state,
            SpinnerState::Idle,
            "spinner must be off after failure"
        );
        assert!(
            app.ui_status
                .as_deref()
                .is_some_and(|s| s.contains("Failed to navigate")),
            "ui_status must contain failure message; got: {:?}",
            app.ui_status
        );
    }

    // -------------------------------------------------------------------------
    // 5.9 — Spam g cancels old navigation and starts new one (RT-A1).
    // -------------------------------------------------------------------------
    #[test]
    fn second_navigate_to_source_cancels_first() {
        let frames = vec![make_frame(10)];
        let vars = vec![make_obj_var(0, 42)];
        let engine = StubEngine::with_threads_and_frames(&["main"], frames).with_vars(10, vars);
        let mut app = App::new(engine, "test.hprof".to_string());

        let path1 = NavigationPathBuilder::new(FrameId(10), VarIdx(0))
            .field(FieldIdx(0))
            .build();
        let path2 = NavigationPathBuilder::new(FrameId(10), VarIdx(0))
            .field(FieldIdx(1))
            .build();

        let key1 = crate::favorites::PinKey {
            thread_id: ThreadId(1),
            thread_name: "main".to_string(),
            nav_path: path1,
        };
        let key2 = crate::favorites::PinKey {
            thread_id: ThreadId(1),
            thread_name: "main".to_string(),
            nav_path: path2.clone(),
        };

        let make_item = |k: crate::favorites::PinKey| crate::favorites::PinnedItem {
            thread_name: "main".to_string(),
            frame_label: "Thread.run()".to_string(),
            item_label: "pin".to_string(),
            snapshot: crate::favorites::PinnedSnapshot::Primitive {
                value_label: "v".to_string(),
            },
            local_collapsed: HashSet::new(),
            hidden_fields: HashSet::new(),
            show_hidden: false,
            key: k,
        };

        app.pinned.push(make_item(key1));
        app.pinned.push(make_item(key2));
        app.sync_favorites_selection();
        app.focus = Focus::Favorites;

        // Navigate to pin1 → defers.
        app.handle_input(InputEvent::NavigateToSource);
        let first_pending = app
            .pending_navigation
            .as_ref()
            .map(|p| p.original_path.clone());
        assert!(first_pending.is_some(), "must be pending after first nav");

        // Select pin2 and navigate again → must cancel pin1 nav.
        // After NavigateToSource, focus=StackFrames; restore Favorites to move its cursor.
        app.focus = Focus::Favorites;
        app.handle_input(InputEvent::Down); // select pin2 in favorites
        app.handle_input(InputEvent::NavigateToSource);

        let second_pending = app.pending_navigation.as_ref();
        assert!(second_pending.is_some(), "must be pending after second nav");
        assert_ne!(
            second_pending.map(|p| p.original_path.clone()),
            first_pending,
            "second nav must use path2, not path1"
        );
        assert_eq!(
            second_pending.unwrap().original_path,
            path2,
            "second nav must use path2"
        );
    }

    // -------------------------------------------------------------------------
    // 5.10 — Empty remaining_path on resume → walk treated as complete.
    // -------------------------------------------------------------------------
    #[test]
    fn single_deferred_segment_completes_on_resume() {
        // Path: Frame(10) → Var(0) → CollectionEntry(889, 1)
        // Var(0) is directly a collection — CollectionEntry is the last segment.
        let frames = vec![make_frame(10)];
        let vars = vec![VariableInfo {
            index: 0,
            value: VariableValue::ObjectRef {
                id: 889,
                class_name: "Object[]".to_string(),
                entry_count: Some(3),
            },
        }];
        let engine = StubEngine::with_threads_and_frames(&["main"], frames).with_vars(10, vars);
        let mut app = App::new(engine, "test.hprof".to_string());

        let pin_key = crate::favorites::PinKey {
            thread_id: ThreadId(1),
            thread_name: "main".to_string(),
            nav_path: NavigationPathBuilder::new(FrameId(10), VarIdx(0))
                .collection_entry(CollectionId(889), EntryIdx(1))
                .build(),
        };
        app.pinned.push(crate::favorites::PinnedItem {
            thread_name: "main".to_string(),
            frame_label: "Thread.run()".to_string(),
            item_label: "var[0][1]".to_string(),
            snapshot: crate::favorites::PinnedSnapshot::Primitive {
                value_label: "v".to_string(),
            },
            local_collapsed: HashSet::new(),
            hidden_fields: HashSet::new(),
            show_hidden: false,
            key: pin_key,
        });
        app.sync_favorites_selection();
        app.focus = Focus::Favorites;

        app.handle_input(InputEvent::NavigateToSource);

        // CollectionEntry(889,1) is the only deferred segment → remaining_path=[CE].
        assert!(
            matches!(
                app.pending_navigation.as_ref().map(|p| &p.awaited),
                Some(AwaitedResource::CollectionPage(889))
            ),
            "must await collection page for 889"
        );
        assert_eq!(
            app.pending_navigation
                .as_ref()
                .unwrap()
                .remaining_path
                .len(),
            1,
            "remaining_path must hold exactly the CollectionEntry segment"
        );

        // Poll until the page loads and walk resumes.
        poll_navigation_to_completion(&mut app);

        assert!(
            app.pending_navigation.is_none(),
            "pending_navigation must be None after walk completes"
        );
        assert_eq!(
            app.stack_state.as_ref().unwrap().cursor(),
            &RenderCursor::At(
                NavigationPathBuilder::new(FrameId(10), VarIdx(0))
                    .collection_entry(CollectionId(889), EntryIdx(1))
                    .build()
            ),
            "cursor must land on collection entry"
        );
    }

    // -------------------------------------------------------------------------
    // 5.11 — Poll resume uses pending thread_id, not selected_serial.
    // -------------------------------------------------------------------------
    #[test]
    fn poll_resume_uses_pending_thread_id_not_selection() {
        // Two threads: main (serial=1, frame=10), worker (serial=2, frame=20).
        let frames1 = vec![make_frame(10)];
        let frames2 = vec![make_frame(20)];
        let engine = StubEngine::with_thread_specific_frames(
            &["main", "worker"],
            &[(1, frames1), (2, frames2)],
        )
        .with_vars(10, vec![make_obj_var(0, 42)])
        .with_vars(20, vec![make_obj_var(0, 55)]);

        let mut app = App::new(engine, "test.hprof".to_string());
        app.pinned.push(crate::favorites::PinnedItem {
            thread_name: "main".to_string(),
            frame_label: "Thread.run()".to_string(),
            item_label: "pin".to_string(),
            snapshot: crate::favorites::PinnedSnapshot::Primitive {
                value_label: "v".to_string(),
            },
            local_collapsed: HashSet::new(),
            hidden_fields: HashSet::new(),
            show_hidden: false,
            key: crate::favorites::PinKey {
                thread_id: ThreadId(1),
                thread_name: "main".to_string(),
                nav_path: NavigationPathBuilder::new(FrameId(10), VarIdx(0))
                    .field(FieldIdx(1))
                    .build(),
            },
        });
        app.sync_favorites_selection();
        app.focus = Focus::Favorites;

        // NavigateToSource for "main" thread → defers on ObjectExpansion(42).
        app.handle_input(InputEvent::NavigateToSource);
        assert!(
            app.pending_navigation
                .as_ref()
                .is_some_and(|p| p.thread_id == 1),
            "pending thread_id must be 1 (main)"
        );

        // Switch selected thread to "worker" (serial=2).
        app.handle_input(InputEvent::Escape); // back to ThreadList (if needed)
        app.focus = Focus::ThreadList;
        app.handle_input(InputEvent::Down); // select worker
        assert_eq!(app.thread_list.selected_serial(), Some(2));

        // Poll: expansion of 42 completes; resume must navigate to main's stack.
        poll_navigation_to_completion(&mut app);

        assert!(
            app.pending_navigation.is_none(),
            "walk must complete for main thread"
        );
        // Stack state should show main's frame (frame_id=10), not worker's (20).
        let ss = app.stack_state.as_ref().unwrap();
        assert!(
            ss.frames().iter().any(|f| f.frame_id == 10),
            "stack must show main thread frames after resume"
        );
    }

    // -------------------------------------------------------------------------
    // 5.12 — Integration: App + PinnedItem + NavigateToSource + viewport offset.
    // -------------------------------------------------------------------------
    #[test]
    fn integration_navigate_to_source_positions_cursor_and_scroll() {
        let frames = vec![make_frame(10)];
        let vars = vec![make_obj_var(0, 42)];
        let engine = StubEngine::with_threads_and_frames(&["main"], frames).with_vars(10, vars);
        let mut app = App::new(engine, "test.hprof".to_string());

        // Set small visible height so scroll offset is non-trivial.
        app.handle_input(InputEvent::Enter);
        app.handle_input(InputEvent::Enter); // expand frame
        if let Some(s) = &mut app.stack_state {
            s.set_visible_height(3);
        }
        app.focus = Focus::Favorites;

        app.pinned
            .push(make_field_favorite_item("main", 10, 0, vec![1]));
        app.sync_favorites_selection();

        app.handle_input(InputEvent::NavigateToSource);
        poll_navigation_to_completion(&mut app);

        let ss = app.stack_state.as_ref().unwrap();
        let target = RenderCursor::At(
            NavigationPathBuilder::new(FrameId(10), VarIdx(0))
                .field(FieldIdx(1))
                .build(),
        );
        assert_eq!(ss.cursor(), &target, "cursor must be at Field(1)");
        assert!(
            app.pending_navigation.is_none(),
            "no pending navigation after completion"
        );
        assert_eq!(app.spinner_state, SpinnerState::Idle, "spinner must be off");

        // Scroll offset: target is in flat_items at some index, offset ≤ index.
        let flat = ss.flat_items();
        if let Some(idx) = flat.iter().position(|c| c == &target) {
            let offset = ss.list_state_offset_for_test();
            assert!(
                offset <= idx,
                "offset {offset} must be ≤ target index {idx}"
            );
        }
    }
}

// =========================================================
// Story 11.1 — Loading indicator for slow operations
// =========================================================

mod spinner_state_tests {
    use super::*;
    use std::time::{Duration, Instant};

    // --- Task 1.3: threshold timing ---

    #[test]
    fn operation_under_threshold_never_shows_loading() {
        let engine = StubEngine::with_threads(&["main"]);
        let mut app = App::new(engine, "t.hprof".into());
        let (tx, rx) = mpsc::channel();
        let path = NavigationPathBuilder::frame_only(FrameId(0));
        app.pending_expansions.insert(
            path.clone(),
            PendingExpansion {
                rx,
                object_id: 1,
                path: path.clone(),
                started: Instant::now(),
                loading_shown: false,
            },
        );
        // Immediately resolve — under 200ms.
        tx.send(Some(vec![])).unwrap();
        app.poll_expansions();
        // Expansion removed, loading_shown was never set.
        assert!(
            app.pending_expansions.is_empty(),
            "expansion must be consumed"
        );
    }

    #[test]
    fn operation_over_threshold_shows_loading() {
        let engine = StubEngine::with_threads(&["main"]);
        let mut app = App::new(engine, "t.hprof".into());
        let (_tx, rx) = mpsc::channel::<Option<Vec<FieldInfo>>>();
        let path = NavigationPathBuilder::frame_only(FrameId(0));
        app.pending_expansions.insert(
            path.clone(),
            PendingExpansion {
                rx,
                object_id: 1,
                path: path.clone(),
                started: Instant::now() - EXPANSION_LOADING_THRESHOLD - Duration::from_millis(10),
                loading_shown: false,
            },
        );
        app.poll_expansions();
        let pe = app.pending_expansions.get(&path).unwrap();
        assert!(
            pe.loading_shown,
            "loading_shown must be true after threshold"
        );
    }

    // --- Task 2.8: two concurrent pending operations ---

    #[test]
    fn spinner_stays_when_one_of_two_operations_completes() {
        let engine = StubEngine::with_threads(&["main"]);
        let mut app = App::new(engine, "t.hprof".into());

        let (tx1, rx1) = mpsc::channel();
        let path1 = NavigationPathBuilder::frame_only(FrameId(0));
        app.pending_expansions.insert(
            path1.clone(),
            PendingExpansion {
                rx: rx1,
                object_id: 10,
                path: path1.clone(),
                started: Instant::now() - EXPANSION_LOADING_THRESHOLD - Duration::from_millis(10),
                loading_shown: true,
            },
        );

        let (_tx2, rx2) = mpsc::channel::<Option<Vec<FieldInfo>>>();
        let path2 = NavigationPathBuilder::new(FrameId(0), VarIdx(0)).build();
        app.pending_expansions.insert(
            path2.clone(),
            PendingExpansion {
                rx: rx2,
                object_id: 20,
                path: path2.clone(),
                started: Instant::now() - EXPANSION_LOADING_THRESHOLD - Duration::from_millis(10),
                loading_shown: true,
            },
        );

        // Complete first operation.
        tx1.send(Some(vec![])).unwrap();
        app.poll_expansions();
        app.update_spinner_state();

        assert_eq!(
            app.spinner_state,
            SpinnerState::Resolving,
            "spinner must stay Resolving while second op pending"
        );

        // Now complete the second.
        // tx2 was dropped (not sent), so it will be Disconnected.
        // Actually let's just drop the tx to disconnect:
        drop(_tx2);
        app.poll_expansions();
        // After minimum display time, spinner should clear.
        // Force loading_until to past.
        app.loading_until = Some(Instant::now() - Duration::from_millis(1));
        app.update_spinner_state();

        assert_eq!(
            app.spinner_state,
            SpinnerState::Idle,
            "spinner must be Idle when all ops complete"
        );
    }

    // --- Task 2.9: SpinnerState priority ---

    #[test]
    fn navigating_to_pin_takes_priority_over_resolving() {
        let engine = StubEngine::with_threads(&["main"]);
        let mut app = App::new(engine, "t.hprof".into());

        // Simulate pending navigation.
        app.pending_navigation = Some(PendingNavigation {
            remaining_path: vec![],
            original_path: NavigationPathBuilder::frame_only(FrameId(0)),
            thread_id: 1,
            awaited: AwaitedResource::ObjectExpansion(99),
            prereq_expanded: vec![],
        });

        // Simulate a pending expansion with loading_shown.
        let (_tx, rx) = mpsc::channel::<Option<Vec<FieldInfo>>>();
        let path = NavigationPathBuilder::frame_only(FrameId(0));
        app.pending_expansions.insert(
            path.clone(),
            PendingExpansion {
                rx,
                object_id: 99,
                path: path.clone(),
                started: Instant::now() - EXPANSION_LOADING_THRESHOLD - Duration::from_millis(10),
                loading_shown: true,
            },
        );

        app.update_spinner_state();
        assert_eq!(
            app.spinner_state,
            SpinnerState::NavigatingToPin,
            "NavigatingToPin must win over Resolving"
        );

        // Remove pending navigation.
        app.pending_navigation = None;
        app.update_spinner_state();
        assert_eq!(
            app.spinner_state,
            SpinnerState::Resolving,
            "without nav, should be Resolving"
        );
    }

    // --- Task 2.10: spinner animation ---

    #[test]
    fn spinner_tick_divides_by_four_for_frame_index() {
        // tick=0  → frame 0
        // tick=3  → frame 0
        // tick=4  → frame 1
        // tick=39 → frame 9
        // tick=40 → frame 0 (wraps)
        let tick_0: u8 = 0;
        assert_eq!((tick_0 / 4) as usize % 10, 0);
        assert_eq!((3u8 / 4) as usize % 10, 0);
        assert_eq!((4u8 / 4) as usize % 10, 1);
        assert_eq!((39u8 / 4) as usize % 10, 9);
        assert_eq!((40u8 / 4) as usize % 10, 0);
    }

    #[test]
    fn spinner_tick_increments_when_not_idle() {
        let engine = StubEngine::with_threads(&["main"]);
        let mut app = App::new(engine, "t.hprof".into());
        assert_eq!(app.spinner_tick, 0);

        // Idle — tick should not increment in render.
        // We can't call render() in tests (no terminal), but
        // we can verify the guard logic directly:
        assert_eq!(app.spinner_state, SpinnerState::Idle);

        // Set non-idle.
        app.spinner_state = SpinnerState::Resolving;
        // Simulate what render does:
        if app.spinner_state != SpinnerState::Idle {
            app.spinner_tick = app.spinner_tick.wrapping_add(1);
        }
        assert_eq!(app.spinner_tick, 1);

        // Set back to Idle — should not increment.
        app.spinner_state = SpinnerState::Idle;
        if app.spinner_state != SpinnerState::Idle {
            app.spinner_tick = app.spinner_tick.wrapping_add(1);
        }
        assert_eq!(app.spinner_tick, 1, "tick must not change when Idle");
    }

    // --- Task 2.11: Escape behaviour ---

    #[test]
    fn escape_during_nav_removes_awaited_expansion() {
        let engine = StubEngine::with_threads_and_frames(&["main"], vec![make_frame(0)]);
        let mut app = App::new(engine, "t.hprof".into());
        app.open_stack_for_selected_thread(1);

        app.pending_navigation = Some(PendingNavigation {
            remaining_path: vec![],
            original_path: NavigationPathBuilder::frame_only(FrameId(0)),
            thread_id: 1,
            awaited: AwaitedResource::ObjectExpansion(42),
            prereq_expanded: vec![],
        });
        app.spinner_state = SpinnerState::NavigatingToPin;

        // Awaited expansion (oid=42) — will be removed on Escape.
        let (_tx1, rx1) = mpsc::channel::<Option<Vec<FieldInfo>>>();
        let path1 = NavigationPathBuilder::frame_only(FrameId(0));
        app.pending_expansions.insert(
            path1.clone(),
            PendingExpansion {
                rx: rx1,
                object_id: 42,
                path: path1.clone(),
                started: Instant::now() - EXPANSION_LOADING_THRESHOLD - Duration::from_millis(10),
                loading_shown: true,
            },
        );

        app.handle_stack_frames_input(InputEvent::Escape);

        assert!(app.pending_navigation.is_none());
        assert!(
            !app.pending_expansions.values().any(|pe| pe.object_id == 42),
            "awaited expansion must be removed on cancel"
        );
        // No other pending ops → Idle.
        assert_eq!(app.spinner_state, SpinnerState::Idle);
    }

    #[test]
    fn escape_during_nav_with_unrelated_ops_transitions_to_resolving() {
        let engine = StubEngine::with_threads_and_frames(&["main"], vec![make_frame(0)]);
        let mut app = App::new(engine, "t.hprof".into());
        app.open_stack_for_selected_thread(1);

        app.pending_navigation = Some(PendingNavigation {
            remaining_path: vec![],
            original_path: NavigationPathBuilder::frame_only(FrameId(0)),
            thread_id: 1,
            awaited: AwaitedResource::ObjectExpansion(42),
            prereq_expanded: vec![],
        });
        app.spinner_state = SpinnerState::NavigatingToPin;

        // Awaited expansion (removed on Escape).
        let (_tx1, rx1) = mpsc::channel::<Option<Vec<FieldInfo>>>();
        let path1 = NavigationPathBuilder::frame_only(FrameId(0));
        app.pending_expansions.insert(
            path1.clone(),
            PendingExpansion {
                rx: rx1,
                object_id: 42,
                path: path1.clone(),
                started: Instant::now() - EXPANSION_LOADING_THRESHOLD - Duration::from_millis(10),
                loading_shown: true,
            },
        );

        // Unrelated expansion (oid=99) — survives Escape.
        let (_tx2, rx2) = mpsc::channel::<Option<Vec<FieldInfo>>>();
        let path2 = NavigationPathBuilder::new(FrameId(0), VarIdx(0)).build();
        app.pending_expansions.insert(
            path2.clone(),
            PendingExpansion {
                rx: rx2,
                object_id: 99,
                path: path2.clone(),
                started: Instant::now() - EXPANSION_LOADING_THRESHOLD - Duration::from_millis(10),
                loading_shown: true,
            },
        );

        app.handle_stack_frames_input(InputEvent::Escape);

        assert!(app.pending_navigation.is_none());
        assert!(
            !app.pending_expansions.values().any(|pe| pe.object_id == 42),
            "awaited expansion must be removed"
        );
        assert!(
            app.pending_expansions.values().any(|pe| pe.object_id == 99),
            "unrelated expansion must survive"
        );
        assert_eq!(
            app.spinner_state,
            SpinnerState::Resolving,
            "unrelated pending op → Resolving"
        );
    }

    #[test]
    fn escape_during_nav_without_pending_ops_goes_idle() {
        let engine = StubEngine::with_threads_and_frames(&["main"], vec![make_frame(0)]);
        let mut app = App::new(engine, "t.hprof".into());
        app.open_stack_for_selected_thread(1);

        app.pending_navigation = Some(PendingNavigation {
            remaining_path: vec![],
            original_path: NavigationPathBuilder::frame_only(FrameId(0)),
            thread_id: 1,
            awaited: AwaitedResource::ObjectExpansion(42),
            prereq_expanded: vec![],
        });
        app.spinner_state = SpinnerState::NavigatingToPin;

        app.handle_stack_frames_input(InputEvent::Escape);

        assert_eq!(
            app.spinner_state,
            SpinnerState::Idle,
            "must be Idle when no pending ops"
        );
    }

    #[test]
    fn escape_during_resolving_stays_resolving() {
        let engine = StubEngine::with_threads_and_frames(&["main"], vec![make_frame(0)]);
        let mut app = App::new(engine, "t.hprof".into());
        app.open_stack_for_selected_thread(1);

        app.spinner_state = SpinnerState::Resolving;
        // No pending_navigation — Escape won't trigger nav cancel.
        // It will try to collapse collection / go back to thread list.
        let before = app.spinner_state;
        app.handle_stack_frames_input(InputEvent::Escape);
        // Escape did not clear the spinner (background resolution).
        // Note: Escape may change focus, but spinner_state is
        // unchanged (no code path sets it to Idle for Resolving).
        // The only thing that clears Resolving is
        // update_spinner_state when all ops complete.
        // After Escape with no pending_navigation, the handler
        // falls through to collection collapse / focus change.
        // spinner_state is NOT touched.
        assert_eq!(
            app.spinner_state, before,
            "Resolving must not be cleared by Escape"
        );
    }

    // --- Task 4.1: fast operation never shows loading ---

    #[test]
    fn fast_operation_loading_shown_stays_false() {
        let engine = StubEngine::with_threads(&["main"]);
        let mut app = App::new(engine, "t.hprof".into());
        let (tx, rx) = mpsc::channel();
        let path = NavigationPathBuilder::frame_only(FrameId(0));
        app.pending_expansions.insert(
            path.clone(),
            PendingExpansion {
                rx,
                object_id: 1,
                path: path.clone(),
                started: Instant::now(),
                loading_shown: false,
            },
        );
        // Resolve immediately.
        tx.send(Some(vec![])).unwrap();
        // Before poll, loading_shown is false.
        assert!(
            !app.pending_expansions.get(&path).unwrap().loading_shown,
            "loading_shown must start false"
        );
        app.poll_expansions();
        // After poll, expansion is removed (resolved).
        assert!(app.pending_expansions.is_empty());
    }

    // --- Task 4.3 + 4.4: minimum display time ---

    #[test]
    fn minimum_spinner_duration_prevents_early_clear() {
        let engine = StubEngine::with_threads(&["main"]);
        let mut app = App::new(engine, "t.hprof".into());

        // Simulate: spinner just turned on.
        app.spinner_state = SpinnerState::Idle;
        app.loading_until = None;

        // Add a pending expansion that has loading_shown.
        let (_tx, rx) = mpsc::channel::<Option<Vec<FieldInfo>>>();
        let path = NavigationPathBuilder::frame_only(FrameId(0));
        app.pending_expansions.insert(
            path.clone(),
            PendingExpansion {
                rx,
                object_id: 1,
                path: path.clone(),
                started: Instant::now() - EXPANSION_LOADING_THRESHOLD - Duration::from_millis(10),
                loading_shown: true,
            },
        );

        // First update: Idle → Resolving, arms timer.
        app.update_spinner_state();
        assert_eq!(app.spinner_state, SpinnerState::Resolving);
        assert!(app.loading_until.is_some());

        // Now remove the pending expansion (operation completed).
        app.pending_expansions.clear();

        // Update again — timer not expired yet, spinner stays.
        app.update_spinner_state();
        assert_ne!(
            app.spinner_state,
            SpinnerState::Idle,
            "spinner must stay visible during minimum display"
        );

        // Force timer to past.
        app.loading_until = Some(Instant::now() - Duration::from_millis(1));
        app.update_spinner_state();
        assert_eq!(
            app.spinner_state,
            SpinnerState::Idle,
            "spinner must clear after minimum display expires"
        );
        assert!(
            app.loading_until.is_none(),
            "loading_until must be reset to None"
        );
    }

    #[test]
    fn navigating_to_pin_also_arms_minimum_display_timer() {
        let engine = StubEngine::with_threads(&["main"]);
        let mut app = App::new(engine, "t.hprof".into());
        app.spinner_state = SpinnerState::Idle;
        app.loading_until = None;

        // Simulate pending navigation.
        app.pending_navigation = Some(PendingNavigation {
            remaining_path: vec![],
            original_path: NavigationPathBuilder::frame_only(FrameId(0)),
            thread_id: 1,
            awaited: AwaitedResource::ObjectExpansion(1),
            prereq_expanded: vec![],
        });

        app.update_spinner_state();
        assert_eq!(app.spinner_state, SpinnerState::NavigatingToPin);
        assert!(
            app.loading_until.is_some(),
            "timer must arm for NavigatingToPin too"
        );
    }

    #[test]
    fn resolving_to_navigating_does_not_reset_timer() {
        let engine = StubEngine::with_threads(&["main"]);
        let mut app = App::new(engine, "t.hprof".into());

        // Start in Resolving state with timer armed.
        let (_tx, rx) = mpsc::channel::<Option<Vec<FieldInfo>>>();
        let path = NavigationPathBuilder::frame_only(FrameId(0));
        app.pending_expansions.insert(
            path.clone(),
            PendingExpansion {
                rx,
                object_id: 1,
                path: path.clone(),
                started: Instant::now() - EXPANSION_LOADING_THRESHOLD - Duration::from_millis(10),
                loading_shown: true,
            },
        );
        app.spinner_state = SpinnerState::Idle;
        app.update_spinner_state();
        assert_eq!(app.spinner_state, SpinnerState::Resolving);
        let timer_1 = app.loading_until;

        // Now add a pending navigation → transition to NavigatingToPin.
        app.pending_navigation = Some(PendingNavigation {
            remaining_path: vec![],
            original_path: NavigationPathBuilder::frame_only(FrameId(0)),
            thread_id: 1,
            awaited: AwaitedResource::ObjectExpansion(1),
            prereq_expanded: vec![],
        });
        app.update_spinner_state();
        assert_eq!(app.spinner_state, SpinnerState::NavigatingToPin);
        // Timer must NOT have been reset.
        assert_eq!(
            app.loading_until, timer_1,
            "timer must not reset on Resolving → NavigatingToPin"
        );
    }

    // --- Task 2.12: StatusBar renders each SpinnerState ---

    #[test]
    fn status_bar_renders_resolving_spinner() {
        use crate::views::status_bar::{SPINNER_CHARS, StatusBar};
        use ratatui::{Terminal, backend::TestBackend};
        let bar = StatusBar {
            filename: "t.hprof",
            thread_count: 1,
            selected: None,
            warning_count: 0,
            file_indexed_pct: None,
            last_warning: None,
            pinned_hidden_count: 0,
            spinner_state: SpinnerState::Resolving,
            spinner_tick: 0,
            walker_info: None,
        };
        let backend = TestBackend::new(200, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| f.render_widget(bar, f.area())).unwrap();
        let content: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol().to_string())
            .collect();
        assert!(
            content.contains("Resolving"),
            "Resolving text must appear; got: {content:?}"
        );
        let expected_char = SPINNER_CHARS[0];
        assert!(
            content.contains(expected_char),
            "spinner char must appear; got: {content:?}"
        );
    }

    #[test]
    fn status_bar_idle_shows_no_spinner_text() {
        use crate::views::status_bar::StatusBar;
        use ratatui::{Terminal, backend::TestBackend};
        let bar = StatusBar {
            filename: "t.hprof",
            thread_count: 1,
            selected: None,
            warning_count: 0,
            file_indexed_pct: None,
            last_warning: None,
            pinned_hidden_count: 0,
            spinner_state: SpinnerState::Idle,
            spinner_tick: 0,
            walker_info: None,
        };
        let backend = TestBackend::new(200, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| f.render_widget(bar, f.area())).unwrap();
        let content: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol().to_string())
            .collect();
        assert!(
            !content.contains("Resolving") && !content.contains("Navigating to pin"),
            "no spinner text when Idle; got: {content:?}"
        );
    }
}
