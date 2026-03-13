use hprof_engine::{
    CollectionPage, FieldInfo, FieldValue, FrameInfo, LineNumber, VariableInfo, VariableValue,
};
use ratatui::widgets::{List, ListItem};

use super::*;
use crate::theme::THEME;

fn make_frame(frame_id: u64) -> FrameInfo {
    FrameInfo {
        frame_id,
        method_name: format!("method{}", frame_id),
        class_name: format!("Class{}", frame_id),
        source_file: format!("Class{}.java", frame_id),
        line: LineNumber::Line(1),
        has_variables: false,
    }
}

fn make_var(index: usize, object_id: u64) -> VariableInfo {
    VariableInfo {
        index,
        value: if object_id == 0 {
            VariableValue::Null
        } else {
            VariableValue::ObjectRef {
                id: object_id,
                class_name: "Object".to_string(),
                entry_count: None,
            }
        },
    }
}

fn make_var_object_ref(index: usize, object_id: u64) -> VariableInfo {
    VariableInfo {
        index,
        value: VariableValue::ObjectRef {
            id: object_id,
            class_name: "Object".to_string(),
            entry_count: None,
        },
    }
}

// --- Helper constructors for RenderCursor ---

fn rc_frame(frame_id: u64) -> RenderCursor {
    RenderCursor::At(NavigationPathBuilder::frame_only(FrameId(frame_id)))
}

fn rc_var(frame_id: u64, var_idx: usize) -> RenderCursor {
    RenderCursor::At(NavigationPathBuilder::new(FrameId(frame_id), VarIdx(var_idx)).build())
}

fn rc_field(frame_id: u64, var_idx: usize, field_path: &[usize]) -> RenderCursor {
    let mut b = NavigationPathBuilder::new(FrameId(frame_id), VarIdx(var_idx));
    for &fi in field_path {
        b = b.field(FieldIdx(fi));
    }
    RenderCursor::At(b.build())
}

fn rc_static_field(
    frame_id: u64,
    var_idx: usize,
    field_path: &[usize],
    static_idx: usize,
) -> RenderCursor {
    let mut b = NavigationPathBuilder::new(FrameId(frame_id), VarIdx(var_idx));
    for &fi in field_path {
        b = b.field(FieldIdx(fi));
    }
    RenderCursor::At(b.static_field(StaticFieldIdx(static_idx)).build())
}

fn rc_loading(frame_id: u64, var_idx: usize, field_path: &[usize]) -> RenderCursor {
    let mut b = NavigationPathBuilder::new(FrameId(frame_id), VarIdx(var_idx));
    for &fi in field_path {
        b = b.field(FieldIdx(fi));
    }
    RenderCursor::LoadingNode(b.build())
}

fn rc_cyclic(frame_id: u64, var_idx: usize, field_path: &[usize]) -> RenderCursor {
    let mut b = NavigationPathBuilder::new(FrameId(frame_id), VarIdx(var_idx));
    for &fi in field_path {
        b = b.field(FieldIdx(fi));
    }
    RenderCursor::CyclicNode(b.build())
}

#[allow(dead_code)]
fn rc_section_header(frame_id: u64, var_idx: usize, field_path: &[usize]) -> RenderCursor {
    let mut b = NavigationPathBuilder::new(FrameId(frame_id), VarIdx(var_idx));
    for &fi in field_path {
        b = b.field(FieldIdx(fi));
    }
    RenderCursor::SectionHeader(b.build())
}

#[allow(dead_code)]
fn rc_overflow(frame_id: u64, var_idx: usize, field_path: &[usize]) -> RenderCursor {
    let mut b = NavigationPathBuilder::new(FrameId(frame_id), VarIdx(var_idx));
    for &fi in field_path {
        b = b.field(FieldIdx(fi));
    }
    RenderCursor::OverflowRow(b.build())
}

fn rc_coll_entry(
    frame_id: u64,
    var_idx: usize,
    field_path: &[usize],
    coll_id: u64,
    entry_idx: usize,
) -> RenderCursor {
    let mut b = NavigationPathBuilder::new(FrameId(frame_id), VarIdx(var_idx));
    for &fi in field_path {
        b = b.field(FieldIdx(fi));
    }
    RenderCursor::At(
        b.collection_entry(CollectionId(coll_id), EntryIdx(entry_idx))
            .build(),
    )
}

fn rc_coll_entry_field(
    frame_id: u64,
    var_idx: usize,
    field_path: &[usize],
    coll_id: u64,
    entry_idx: usize,
    obj_field_path: &[usize],
) -> RenderCursor {
    let mut b = NavigationPathBuilder::new(FrameId(frame_id), VarIdx(var_idx));
    for &fi in field_path {
        b = b.field(FieldIdx(fi));
    }
    b = b.collection_entry(CollectionId(coll_id), EntryIdx(entry_idx));
    for &fi in obj_field_path {
        b = b.field(FieldIdx(fi));
    }
    RenderCursor::At(b.build())
}

#[allow(dead_code)]
fn rc_chunk_section(
    frame_id: u64,
    var_idx: usize,
    field_path: &[usize],
    coll_id: u64,
    chunk_offset: usize,
) -> RenderCursor {
    let mut b = NavigationPathBuilder::new(FrameId(frame_id), VarIdx(var_idx));
    for &fi in field_path {
        b = b.field(FieldIdx(fi));
    }
    let path = b
        .collection_entry(CollectionId(coll_id), EntryIdx(chunk_offset))
        .build();
    RenderCursor::ChunkSection(path, ChunkOffset(chunk_offset))
}

#[allow(dead_code)]
fn rc_static_obj_field(
    frame_id: u64,
    var_idx: usize,
    field_path: &[usize],
    static_idx: usize,
    obj_field_path: &[usize],
) -> RenderCursor {
    let mut b = NavigationPathBuilder::new(FrameId(frame_id), VarIdx(var_idx));
    for &fi in field_path {
        b = b.field(FieldIdx(fi));
    }
    b = b.static_field(StaticFieldIdx(static_idx));
    for &fi in obj_field_path {
        b = b.field(FieldIdx(fi));
    }
    RenderCursor::At(b.build())
}

#[allow(dead_code)]
fn rc_coll_entry_static_field(
    frame_id: u64,
    var_idx: usize,
    field_path: &[usize],
    coll_id: u64,
    entry_idx: usize,
    obj_field_path: &[usize],
    static_idx: usize,
) -> RenderCursor {
    let mut b = NavigationPathBuilder::new(FrameId(frame_id), VarIdx(var_idx));
    for &fi in field_path {
        b = b.field(FieldIdx(fi));
    }
    b = b.collection_entry(CollectionId(coll_id), EntryIdx(entry_idx));
    for &fi in obj_field_path {
        b = b.field(FieldIdx(fi));
    }
    RenderCursor::At(b.static_field(StaticFieldIdx(static_idx)).build())
}

#[allow(dead_code, clippy::too_many_arguments)]
fn rc_coll_entry_static_obj_field(
    frame_id: u64,
    var_idx: usize,
    field_path: &[usize],
    coll_id: u64,
    entry_idx: usize,
    obj_field_path: &[usize],
    static_idx: usize,
    static_obj_path: &[usize],
) -> RenderCursor {
    let mut b = NavigationPathBuilder::new(FrameId(frame_id), VarIdx(var_idx));
    for &fi in field_path {
        b = b.field(FieldIdx(fi));
    }
    b = b.collection_entry(CollectionId(coll_id), EntryIdx(entry_idx));
    for &fi in obj_field_path {
        b = b.field(FieldIdx(fi));
    }
    b = b.static_field(StaticFieldIdx(static_idx));
    for &fi in static_obj_path {
        b = b.field(FieldIdx(fi));
    }
    RenderCursor::At(b.build())
}

fn path_field(frame_id: u64, var_idx: usize, field_path: &[usize]) -> NavigationPath {
    let mut b = NavigationPathBuilder::new(FrameId(frame_id), VarIdx(var_idx));
    for &fi in field_path {
        b = b.field(FieldIdx(fi));
    }
    b.build()
}

fn path_coll_entry(
    frame_id: u64,
    var_idx: usize,
    field_path: &[usize],
    coll_id: u64,
    entry_idx: usize,
) -> NavigationPath {
    let mut b = NavigationPathBuilder::new(FrameId(frame_id), VarIdx(var_idx));
    for &fi in field_path {
        b = b.field(FieldIdx(fi));
    }
    b.collection_entry(CollectionId(coll_id), EntryIdx(entry_idx))
        .build()
}

// ---------------------------------------------------------------------------
// Basic navigation tests
// ---------------------------------------------------------------------------

#[test]
fn new_with_three_frames_selects_frame_0() {
    let frames = vec![make_frame(1), make_frame(2), make_frame(3)];
    let state = StackState::new(frames);
    assert_eq!(state.cursor(), &rc_frame(1));
}

#[test]
fn move_down_on_three_frames_with_no_expanded_moves_to_frame_1() {
    let frames = vec![make_frame(1), make_frame(2), make_frame(3)];
    let mut state = StackState::new(frames);
    state.move_down();
    assert_eq!(state.cursor(), &rc_frame(2));
}

#[test]
fn move_up_at_frame_0_does_nothing() {
    let frames = vec![make_frame(1), make_frame(2), make_frame(3)];
    let mut state = StackState::new(frames);
    state.move_up();
    assert_eq!(state.cursor(), &rc_frame(1));
}

#[test]
fn toggle_expand_with_vars_then_move_down_moves_to_var_0() {
    let frames = vec![make_frame(10), make_frame(20)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var(0, 1), make_var(1, 2)];
    state.toggle_expand(10, vars);
    state.move_down();
    assert_eq!(state.cursor(), &rc_var(10, 0));
}

#[test]
fn move_down_past_last_var_of_expanded_frame_moves_to_next_frame() {
    let frames = vec![make_frame(10), make_frame(20)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var(0, 1)];
    state.toggle_expand(10, vars);
    state.move_down(); // → Var{10,0}
    state.move_down(); // → Frame(20)
    assert_eq!(state.cursor(), &rc_frame(20));
}

#[test]
fn toggle_expand_on_already_expanded_frame_collapses_it() {
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    state.toggle_expand(10, vec![make_var(0, 1)]);
    assert!(state.is_expanded(10));
    state.toggle_expand(10, vec![]);
    assert!(!state.is_expanded(10));
}

#[test]
fn toggle_expand_collapse_from_var_cursor_resets_to_frame_and_navigation_works() {
    let frames = vec![make_frame(10), make_frame(20)];
    let mut state = StackState::new(frames);
    state.toggle_expand(10, vec![make_var(0, 1)]);
    state.move_down(); // → rc_var(10,0)
    assert_eq!(state.cursor(), &rc_var(10, 0));
    state.toggle_expand(10, vec![]);
    assert_eq!(state.cursor(), &rc_frame(10));
    state.move_down();
    assert_eq!(state.cursor(), &rc_frame(20));
    state.move_up();
    assert_eq!(state.cursor(), &rc_frame(10));
}

#[test]
fn selected_frame_id_returns_correct_frame_id() {
    let frames = vec![make_frame(42), make_frame(99)];
    let state = StackState::new(frames);
    assert_eq!(state.selected_frame_id(), Some(42));
}

#[test]
fn format_frame_label_keeps_line_metadata_when_source_file_missing() {
    let frame = FrameInfo {
        frame_id: 1,
        method_name: "run".to_string(),
        class_name: "Thread".to_string(),
        source_file: String::new(),
        line: LineNumber::Native,
        has_variables: false,
    };
    assert_eq!(format_frame_label(&frame), "Thread.run() (native)");
}

#[test]
fn format_frame_label_with_source_file_and_line_number() {
    let frame = FrameInfo {
        frame_id: 1,
        method_name: "run".to_string(),
        class_name: "Thread".to_string(),
        source_file: "Thread.java".to_string(),
        line: LineNumber::Line(42),
        has_variables: false,
    };
    assert_eq!(format_frame_label(&frame), "Thread.run() [Thread.java:42]");
}

#[test]
fn new_with_empty_frames_returns_none_for_selected_frame_id() {
    let state = StackState::new(vec![]);
    assert_eq!(state.selected_frame_id(), None);
}

// --- Task 10: Object expansion phase tests ---

#[test]
fn set_expansion_loading_changes_phase_to_loading() {
    let mut state = StackState::new(vec![make_frame(1)]);
    state.set_expansion_loading(42);
    assert_eq!(state.expansion_state(42), ExpansionPhase::Loading);
}

#[test]
fn set_expansion_done_changes_phase_to_expanded() {
    let mut state = StackState::new(vec![make_frame(1)]);
    state.set_expansion_done(42, vec![]);
    assert_eq!(state.expansion_state(42), ExpansionPhase::Expanded);
}

#[test]
fn set_expansion_failed_changes_phase_to_failed() {
    let mut state = StackState::new(vec![make_frame(1)]);
    state.set_expansion_failed(42, "err".to_string());
    assert_eq!(state.expansion_state(42), ExpansionPhase::Failed);
}

#[test]
fn set_expansion_failed_recovers_cursor_from_loading_node_top_level() {
    let frames = vec![make_frame(10), make_frame(20)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var_object_ref(0, 99)];
    state.toggle_expand(10, vars);
    state.set_expansion_loading(99);
    state.move_down(); // → rc_var(10,0)
    state.move_down(); // → LoadingNode
    assert!(
        matches!(state.cursor(), RenderCursor::LoadingNode(_)),
        "precondition: cursor is on loading node"
    );
    state.set_expansion_failed(99, "err".to_string());
    assert_eq!(
        state.cursor(),
        &rc_var(10, 0),
        "cursor must snap to parent after failure"
    );
    state.move_down();
    assert_eq!(state.cursor(), &rc_frame(20));
}

#[test]
fn set_expansion_failed_recovers_cursor_from_loading_node_nested_field() {
    use hprof_engine::{FieldInfo, FieldValue};
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var_object_ref(0, 100)];
    state.toggle_expand(10, vars);
    let fields = vec![FieldInfo {
        name: "child".to_string(),
        value: FieldValue::ObjectRef {
            id: 200,
            class_name: "Child".to_string(),
            entry_count: None,
            inline_value: None,
        },
    }];
    state.set_expansion_done(100, fields);
    state.set_expansion_loading(200);
    state.move_down(); // → rc_var(10,0)
    state.move_down(); // → rc_field(10,0,[0])
    state.move_down(); // → LoadingNode
    assert!(
        matches!(state.cursor(), RenderCursor::LoadingNode(_)),
        "precondition: cursor is on nested loading node"
    );
    state.set_expansion_failed(200, "boom".to_string());
    assert_eq!(
        state.cursor(),
        &rc_field(10, 0, &[0]),
        "cursor must snap to parent field after nested failure"
    );
}

#[test]
fn cancel_expansion_on_loading_reverts_to_collapsed() {
    let mut state = StackState::new(vec![make_frame(1)]);
    state.set_expansion_loading(42);
    state.cancel_expansion(42);
    assert_eq!(state.expansion_state(42), ExpansionPhase::Collapsed);
}

#[test]
fn flat_items_loading_object_includes_loading_node() {
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var_object_ref(0, 99)];
    state.toggle_expand(10, vars);
    state.set_expansion_loading(99);
    let flat = state.flat_items();
    assert!(flat.contains(&rc_loading(10, 0, &[])));
}

#[test]
fn flat_items_expanded_with_two_fields_includes_two_object_field_nodes() {
    use hprof_engine::{FieldInfo, FieldValue};
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var_object_ref(0, 99)];
    state.toggle_expand(10, vars);
    let fields = vec![
        FieldInfo {
            name: "a".to_string(),
            value: FieldValue::Int(1),
        },
        FieldInfo {
            name: "b".to_string(),
            value: FieldValue::Int(2),
        },
    ];
    state.set_expansion_done(99, fields);
    let flat = state.flat_items();
    assert!(flat.contains(&rc_field(10, 0, &[0])));
    assert!(flat.contains(&rc_field(10, 0, &[1])));
}

#[test]
fn move_down_from_on_var_expanded_moves_to_first_object_field() {
    use hprof_engine::{FieldInfo, FieldValue};
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var_object_ref(0, 99)];
    state.toggle_expand(10, vars);
    let fields = vec![FieldInfo {
        name: "x".to_string(),
        value: FieldValue::Int(7),
    }];
    state.set_expansion_done(99, fields);
    state.move_down();
    assert_eq!(state.cursor(), &rc_var(10, 0));
    state.move_down();
    assert_eq!(state.cursor(), &rc_field(10, 0, &[0]));
}

#[test]
fn move_down_past_last_object_field_moves_to_next_frame() {
    use hprof_engine::{FieldInfo, FieldValue};
    let frames = vec![make_frame(10), make_frame(20)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var_object_ref(0, 99)];
    state.toggle_expand(10, vars);
    let fields = vec![FieldInfo {
        name: "x".to_string(),
        value: FieldValue::Int(7),
    }];
    state.set_expansion_done(99, fields);
    state.move_down(); // Frame → Var
    state.move_down(); // Var → Field
    state.move_down(); // Field → Frame(20)
    assert_eq!(state.cursor(), &rc_frame(20));
}

#[test]
fn selected_loading_object_id_on_loading_node_returns_object_id() {
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var_object_ref(0, 42)];
    state.toggle_expand(10, vars);
    state.set_expansion_loading(42);
    state.move_down(); // → rc_var(10,0)
    state.move_down(); // → LoadingNode
    assert_eq!(state.selected_loading_object_id(), Some(42));
}

// --- Task 4.5 / 5.5: depth-2 navigation and indentation tests ---

#[test]
fn flat_items_depth2_expansion_emits_correct_cursor_sequence() {
    use hprof_engine::{FieldInfo, FieldValue};
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var_object_ref(0, 100)];
    state.toggle_expand(10, vars);
    let fields_100 = vec![FieldInfo {
        name: "child".to_string(),
        value: FieldValue::ObjectRef {
            id: 200,
            class_name: "Foo".to_string(),
            entry_count: None,
            inline_value: None,
        },
    }];
    state.set_expansion_done(100, fields_100);
    let fields_200 = vec![FieldInfo {
        name: "val".to_string(),
        value: FieldValue::Int(7),
    }];
    state.set_expansion_done(200, fields_200);
    let flat = state.flat_items();
    assert!(flat.contains(&rc_field(10, 0, &[0])));
    assert!(flat.contains(&rc_field(10, 0, &[0, 0])));
}

#[test]
fn selected_field_ref_id_returns_object_ref_id_for_nested_field() {
    use hprof_engine::{FieldInfo, FieldValue};
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var_object_ref(0, 100)];
    state.toggle_expand(10, vars);
    let fields = vec![FieldInfo {
        name: "child".to_string(),
        value: FieldValue::ObjectRef {
            id: 200,
            class_name: "Bar".to_string(),
            entry_count: None,
            inline_value: None,
        },
    }];
    state.set_expansion_done(100, fields);
    state.set_cursor(rc_field(10, 0, &[0]));
    assert_eq!(state.selected_field_ref_id(), Some(200));
}

#[test]
fn selected_field_ref_id_returns_none_for_non_object_ref_field() {
    use hprof_engine::{FieldInfo, FieldValue};
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var_object_ref(0, 100)];
    state.toggle_expand(10, vars);
    let fields = vec![FieldInfo {
        name: "x".to_string(),
        value: FieldValue::Int(42),
    }];
    state.set_expansion_done(100, fields);
    state.set_cursor(rc_field(10, 0, &[0]));
    assert_eq!(state.selected_field_ref_id(), None);
}

// --- Task 7.4: recursive collapse tests ---

#[test]
fn collapse_object_recursive_removes_nested_expanded_child() {
    use hprof_engine::{FieldInfo, FieldValue};
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    let fields_100 = vec![FieldInfo {
        name: "child".to_string(),
        value: FieldValue::ObjectRef {
            id: 200,
            class_name: "Foo".to_string(),
            entry_count: None,
            inline_value: None,
        },
    }];
    state.set_expansion_done(100, fields_100);
    let fields_200 = vec![FieldInfo {
        name: "val".to_string(),
        value: FieldValue::Int(1),
    }];
    state.set_expansion_done(200, fields_200);
    assert_eq!(state.expansion_state(100), ExpansionPhase::Expanded);
    assert_eq!(state.expansion_state(200), ExpansionPhase::Expanded);
    state.collapse_object_recursive(100);
    assert_eq!(state.expansion_state(100), ExpansionPhase::Collapsed);
    assert_eq!(state.expansion_state(200), ExpansionPhase::Collapsed);
}

#[test]
fn collapse_object_recursive_cycle_guard_does_not_infinite_loop() {
    use hprof_engine::{FieldInfo, FieldValue};
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    let fields_100 = vec![FieldInfo {
        name: "c".to_string(),
        value: FieldValue::ObjectRef {
            id: 200,
            class_name: "A".to_string(),
            entry_count: None,
            inline_value: None,
        },
    }];
    state.set_expansion_done(100, fields_100);
    let fields_200 = vec![FieldInfo {
        name: "c".to_string(),
        value: FieldValue::ObjectRef {
            id: 100,
            class_name: "B".to_string(),
            entry_count: None,
            inline_value: None,
        },
    }];
    state.set_expansion_done(200, fields_200);
    state.collapse_object_recursive(100);
    assert_eq!(state.expansion_state(100), ExpansionPhase::Collapsed);
    assert_eq!(state.expansion_state(200), ExpansionPhase::Collapsed);
}

// --- Task 8.2: frame collapse clears nested expansion ---

#[test]
fn toggle_expand_collapse_frame_clears_nested_object_phases() {
    use hprof_engine::{FieldInfo, FieldValue};
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var_object_ref(0, 100)];
    state.toggle_expand(10, vars);
    let fields_100 = vec![FieldInfo {
        name: "x".to_string(),
        value: FieldValue::Int(1),
    }];
    state.set_expansion_done(100, fields_100);
    assert_eq!(state.expansion_state(100), ExpansionPhase::Expanded);
    state.toggle_expand(10, vec![]);
    assert!(state.expansion.object_phases.is_empty());
}

// --- Task 5.5: build_items indentation test ---

fn item_text(item: ListItem<'static>) -> String {
    use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};
    let area = Rect::new(0, 0, 120, 1);
    let mut buf = Buffer::empty(area);
    Widget::render(List::new(vec![item]), area, &mut buf);
    buf.content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>()
        .trim_end()
        .to_string()
}

#[test]
fn build_items_depth1_field_has_correct_indent() {
    use hprof_engine::{FieldInfo, FieldValue};
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var_object_ref(0, 99)];
    state.toggle_expand(10, vars);
    let fields = vec![FieldInfo {
        name: "count".to_string(),
        value: FieldValue::Int(5),
    }];
    state.set_expansion_done(99, fields);
    let items = state.build_items();
    assert_eq!(items.len(), 3);
    let text = item_text(items[2].clone());
    assert!(
        text.starts_with("      ") && !text.starts_with("        "),
        "depth-1 field must have 4+2 indent, got: {text:?}"
    );
}

#[test]
fn build_items_depth2_field_has_correct_indent() {
    use hprof_engine::{FieldInfo, FieldValue};
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var_object_ref(0, 99)];
    state.toggle_expand(10, vars);
    let fields_99 = vec![FieldInfo {
        name: "child".to_string(),
        value: FieldValue::ObjectRef {
            id: 200,
            class_name: "Foo".to_string(),
            entry_count: None,
            inline_value: None,
        },
    }];
    state.set_expansion_done(99, fields_99);
    let fields_200 = vec![FieldInfo {
        name: "val".to_string(),
        value: FieldValue::Int(7),
    }];
    state.set_expansion_done(200, fields_200);
    let items = state.build_items();
    assert_eq!(items.len(), 4);
    let depth1 = item_text(items[2].clone());
    assert!(
        depth1.starts_with("    - "),
        "depth-1 ObjectRef field must have toggle prefix, got: {depth1:?}"
    );
    let depth2 = item_text(items[3].clone());
    assert!(
        depth2.starts_with("        ") && !depth2.starts_with("          "),
        "depth-2 field must have 6+2 indent, got: {depth2:?}"
    );
}

#[test]
fn build_items_failed_expansion_shows_error_inline_on_var_row() {
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var_object_ref(0, 99)];
    state.toggle_expand(10, vars);
    state.set_expansion_failed(99, "object not found".to_string());
    let items = state.build_items();
    assert_eq!(
        items.len(),
        2,
        "expect no child row for Failed — got {}",
        items.len()
    );
    let text = item_text(items[1].clone());
    assert!(
        text.contains("! "),
        "var row must contain '! ' prefix, got: {text:?}"
    );
    assert!(
        text.contains("object not found"),
        "var row must contain the stored error message, got: {text:?}"
    );
}

// --- Cyclic reference detection tests ---

#[test]
fn flat_items_self_ref_emits_cyclic_node() {
    use hprof_engine::{FieldInfo, FieldValue};
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var_object_ref(0, 100)];
    state.toggle_expand(10, vars);
    let fields = vec![FieldInfo {
        name: "self".to_string(),
        value: FieldValue::ObjectRef {
            id: 100,
            class_name: "Node".to_string(),
            entry_count: None,
            inline_value: None,
        },
    }];
    state.set_expansion_done(100, fields);
    let flat = state.flat_items();
    let cyclic_count = flat
        .iter()
        .filter(|c| matches!(c, RenderCursor::CyclicNode(_)))
        .count();
    assert_eq!(cyclic_count, 1);
    let deep_fields = flat
        .iter()
        .filter(|c| {
            if let RenderCursor::At(p) = c {
                let segs = p.segments();
                segs.len() > 3 && segs[2..].iter().all(|s| matches!(s, PathSegment::Field(_)))
            } else {
                false
            }
        })
        .count();
    assert_eq!(deep_fields, 0, "no recursive fields beyond depth 1");
}

#[test]
fn flat_items_multi_self_ref_emits_two_cyclic_nodes() {
    use hprof_engine::{FieldInfo, FieldValue};
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var_object_ref(0, 100)];
    state.toggle_expand(10, vars);
    let fields = vec![
        FieldInfo {
            name: "left".to_string(),
            value: FieldValue::ObjectRef {
                id: 100,
                class_name: "Node".to_string(),
                entry_count: None,
                inline_value: None,
            },
        },
        FieldInfo {
            name: "right".to_string(),
            value: FieldValue::ObjectRef {
                id: 100,
                class_name: "Node".to_string(),
                entry_count: None,
                inline_value: None,
            },
        },
    ];
    state.set_expansion_done(100, fields);
    let flat = state.flat_items();
    let cyclic_count = flat
        .iter()
        .filter(|c| matches!(c, RenderCursor::CyclicNode(_)))
        .count();
    assert_eq!(cyclic_count, 2);
}

#[test]
fn build_items_self_ref_renders_self_ref_marker() {
    use hprof_engine::{FieldInfo, FieldValue};
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var_object_ref(0, 100)];
    state.toggle_expand(10, vars);
    let fields = vec![FieldInfo {
        name: "me".to_string(),
        value: FieldValue::ObjectRef {
            id: 100,
            class_name: "java.lang.Thread".to_string(),
            entry_count: None,
            inline_value: None,
        },
    }];
    state.set_expansion_done(100, fields);
    let items = state.build_items();
    let text = item_text(items[2].clone());
    assert!(text.contains("\u{21BB}"), "must contain ↻, got: {text:?}");
    assert!(
        text.contains("[self-ref]"),
        "must contain [self-ref], got: {text:?}"
    );
    assert!(
        text.contains("Thread"),
        "must show short class name, got: {text:?}"
    );
    assert!(
        !text.contains("java.lang.Thread"),
        "must NOT show FQCN, got: {text:?}"
    );
}

#[test]
fn flat_items_indirect_cycle_emits_cyclic_node() {
    use hprof_engine::{FieldInfo, FieldValue};
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var_object_ref(0, 100)];
    state.toggle_expand(10, vars);
    let fields_a = vec![FieldInfo {
        name: "child".to_string(),
        value: FieldValue::ObjectRef {
            id: 200,
            class_name: "B".to_string(),
            entry_count: None,
            inline_value: None,
        },
    }];
    state.set_expansion_done(100, fields_a);
    let fields_b = vec![FieldInfo {
        name: "parent".to_string(),
        value: FieldValue::ObjectRef {
            id: 100,
            class_name: "A".to_string(),
            entry_count: None,
            inline_value: None,
        },
    }];
    state.set_expansion_done(200, fields_b);
    let flat = state.flat_items();
    let cyclic_count = flat
        .iter()
        .filter(|c| matches!(c, RenderCursor::CyclicNode(_)))
        .count();
    assert_eq!(cyclic_count, 1, "B's back-ref to A should be cyclic");
    let max_depth = flat
        .iter()
        .filter_map(|c| match c {
            RenderCursor::At(p) | RenderCursor::CyclicNode(p) => {
                let field_segs = p.segments().iter().skip(2).count();
                Some(field_segs)
            }
            _ => None,
        })
        .max()
        .unwrap_or(0);
    assert!(max_depth <= 3, "no deep recursion, max depth: {max_depth}");
}

#[test]
fn build_items_indirect_cycle_renders_cyclic_marker() {
    use hprof_engine::{FieldInfo, FieldValue};
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var_object_ref(0, 100)];
    state.toggle_expand(10, vars);
    let fields_a = vec![FieldInfo {
        name: "child".to_string(),
        value: FieldValue::ObjectRef {
            id: 200,
            class_name: "B".to_string(),
            entry_count: None,
            inline_value: None,
        },
    }];
    state.set_expansion_done(100, fields_a);
    let fields_b = vec![FieldInfo {
        name: "parent".to_string(),
        value: FieldValue::ObjectRef {
            id: 100,
            class_name: "A".to_string(),
            entry_count: None,
            inline_value: None,
        },
    }];
    state.set_expansion_done(200, fields_b);
    let items = state.build_items();
    let all_text: Vec<String> = items.into_iter().map(item_text).collect();
    let cyclic_line = all_text.iter().find(|t| t.contains("[cyclic]"));
    assert!(
        cyclic_line.is_some(),
        "must have [cyclic] marker, items: {all_text:?}"
    );
    let line = cyclic_line.unwrap();
    assert!(line.contains("\u{21BB}"), "must contain ↻, got: {line:?}");
    assert!(
        !line.contains("[self-ref]"),
        "indirect cycle must NOT show [self-ref]"
    );
}

#[test]
fn move_down_up_across_cyclic_node() {
    use hprof_engine::{FieldInfo, FieldValue};
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var_object_ref(0, 100)];
    state.toggle_expand(10, vars);
    let fields = vec![
        FieldInfo {
            name: "a".to_string(),
            value: FieldValue::Int(1),
        },
        FieldInfo {
            name: "b".to_string(),
            value: FieldValue::ObjectRef {
                id: 100,
                class_name: "Node".to_string(),
                entry_count: None,
                inline_value: None,
            },
        },
        FieldInfo {
            name: "c".to_string(),
            value: FieldValue::Int(3),
        },
    ];
    state.set_expansion_done(100, fields);
    state.move_down(); // Frame → Var
    state.move_down(); // Var → Field[0]
    assert_eq!(state.cursor(), &rc_field(10, 0, &[0]));
    state.move_down(); // Field[0] → CyclicNode[1]
    assert_eq!(state.cursor(), &rc_cyclic(10, 0, &[1]));
    state.move_down(); // CyclicNode[1] → Field[2]
    assert_eq!(state.cursor(), &rc_field(10, 0, &[2]));
    state.move_up(); // Field[2] → CyclicNode[1]
    assert_eq!(state.cursor(), &rc_cyclic(10, 0, &[1]));
    state.move_up(); // CyclicNode[1] → Field[0]
    assert_eq!(state.cursor(), &rc_field(10, 0, &[0]));
}

fn setup_collection_entry_self_ref_state() -> StackState {
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    state.toggle_expand(10, vec![make_var_object_ref(0, 99)]);
    state.set_expansion_done(
        99,
        vec![FieldInfo {
            name: "items".to_string(),
            value: FieldValue::ObjectRef {
                id: 200,
                class_name: "java.util.ArrayList".to_string(),
                entry_count: Some(1),
                inline_value: None,
            },
        }],
    );
    // collection 200 is at field[0] of var[0] (frame 10); set expansion_phases so it renders
    state
        .expansion
        .expansion_phases
        .insert(path_field(10, 0, &[0]), ExpansionPhase::Expanded);
    state.expansion.collection_chunks.insert(
        200,
        CollectionChunks {
            total_count: 1,
            eager_page: Some(CollectionPage {
                entries: vec![hprof_engine::EntryInfo {
                    index: 0,
                    key: None,
                    value: FieldValue::ObjectRef {
                        id: 700,
                        class_name: "Node".to_string(),
                        entry_count: None,
                        inline_value: None,
                    },
                }],
                total_count: 1,
                offset: 0,
                has_more: false,
            }),
            chunk_pages: std::collections::HashMap::new(),
        },
    );
    state.set_expansion_done(
        700,
        vec![FieldInfo {
            name: "self".to_string(),
            value: FieldValue::ObjectRef {
                id: 700,
                class_name: "Node".to_string(),
                entry_count: None,
                inline_value: None,
            },
        }],
    );
    state
}

#[test]
fn flat_items_collection_entry_self_ref_emits_terminal_obj_field_row() {
    let state = setup_collection_entry_self_ref_state();
    let flat = state.flat_items();
    // collection_id=200, entry_index=0, then field[0] is the self-ref
    let marker_rows = flat
        .iter()
        .filter(|c| {
            if let RenderCursor::At(p) = c {
                let segs = p.segments();
                segs.iter().any(|s| {
                    matches!(
                        s,
                        PathSegment::CollectionEntry(CollectionId(200), EntryIdx(0))
                    )
                }) && segs
                    .last()
                    .is_some_and(|s| matches!(s, PathSegment::Field(_)))
            } else {
                false
            }
        })
        .count();
    assert_eq!(marker_rows, 1, "cyclic entry field must emit one row");
    assert_eq!(state.flat_items().len(), state.build_items().len());
}

#[test]
fn build_items_collection_entry_self_ref_renders_marker() {
    let state = setup_collection_entry_self_ref_state();
    let all_text: Vec<String> = state.build_items().into_iter().map(item_text).collect();
    let marker = all_text
        .iter()
        .find(|t| t.contains("self: ") && t.contains("[self-ref]"));
    assert!(
        marker.is_some(),
        "must render cyclic marker row for collection entry object: {all_text:?}"
    );
    let line = marker.unwrap();
    assert!(line.contains("\u{21BB}"), "must contain ↻, got: {line:?}");
    assert!(
        line.contains("@ 0x2BC"),
        "must include object id, got: {line:?}"
    );
}

#[test]
fn selected_collection_entry_obj_field_ref_id_is_none_for_cyclic_row() {
    let mut state = setup_collection_entry_self_ref_state();
    // collection 200 at field[0] of var[0], entry[0], then obj_field[0]
    state.set_cursor(rc_coll_entry_field(10, 0, &[0], 200, 0, &[0]));
    assert_eq!(state.selected_collection_entry_obj_field_ref_id(), None);
    assert_eq!(
        state.selected_collection_entry_obj_field_collection_info(),
        None
    );
}

#[test]
fn flat_items_acyclic_tree_no_cyclic_nodes() {
    use hprof_engine::{FieldInfo, FieldValue};
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var_object_ref(0, 100)];
    state.toggle_expand(10, vars);
    let fields_a = vec![FieldInfo {
        name: "b".to_string(),
        value: FieldValue::ObjectRef {
            id: 200,
            class_name: "B".to_string(),
            entry_count: None,
            inline_value: None,
        },
    }];
    state.set_expansion_done(100, fields_a);
    let fields_b = vec![FieldInfo {
        name: "c".to_string(),
        value: FieldValue::ObjectRef {
            id: 300,
            class_name: "C".to_string(),
            entry_count: None,
            inline_value: None,
        },
    }];
    state.set_expansion_done(200, fields_b);
    let fields_c = vec![FieldInfo {
        name: "val".to_string(),
        value: FieldValue::Int(42),
    }];
    state.set_expansion_done(300, fields_c);
    let flat = state.flat_items();
    let cyclic_count = flat
        .iter()
        .filter(|c| matches!(c, RenderCursor::CyclicNode(_)))
        .count();
    assert_eq!(cyclic_count, 0, "acyclic tree must have zero cyclic nodes");
    assert!(flat.contains(&rc_field(10, 0, &[0])));
    assert!(flat.contains(&rc_field(10, 0, &[0, 0])));
    assert!(flat.contains(&rc_field(10, 0, &[0, 0, 0])));
}

#[test]
fn flat_items_diamond_shared_object_no_false_positive() {
    use hprof_engine::{FieldInfo, FieldValue};
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var_object_ref(0, 100)];
    state.toggle_expand(10, vars);
    let fields_a = vec![
        FieldInfo {
            name: "left".to_string(),
            value: FieldValue::ObjectRef {
                id: 300,
                class_name: "C".to_string(),
                entry_count: None,
                inline_value: None,
            },
        },
        FieldInfo {
            name: "right".to_string(),
            value: FieldValue::ObjectRef {
                id: 300,
                class_name: "C".to_string(),
                entry_count: None,
                inline_value: None,
            },
        },
    ];
    state.set_expansion_done(100, fields_a);
    let fields_c = vec![FieldInfo {
        name: "val".to_string(),
        value: FieldValue::Int(42),
    }];
    state.set_expansion_done(300, fields_c);
    let flat = state.flat_items();
    let cyclic_count = flat
        .iter()
        .filter(|c| matches!(c, RenderCursor::CyclicNode(_)))
        .count();
    assert_eq!(
        cyclic_count, 0,
        "diamond/shared object must not be a false positive"
    );
    assert!(flat.contains(&rc_field(10, 0, &[0, 0])));
    assert!(flat.contains(&rc_field(10, 0, &[1, 0])));
}

#[test]
fn collapse_cyclic_child_resyncs_cursor_to_var() {
    use hprof_engine::{FieldInfo, FieldValue};
    let frames = vec![make_frame(10), make_frame(20)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var_object_ref(0, 1000)];
    state.toggle_expand(10, vars);
    let thread_fields = vec![FieldInfo {
        name: "parkBlocker".to_string(),
        value: FieldValue::ObjectRef {
            id: 2000,
            class_name: "Coroutine".to_string(),
            entry_count: None,
            inline_value: None,
        },
    }];
    state.set_expansion_done(1000, thread_fields);
    let coroutine_fields = vec![FieldInfo {
        name: "blockedThread".to_string(),
        value: FieldValue::ObjectRef {
            id: 1000,
            class_name: "Thread".to_string(),
            entry_count: None,
            inline_value: None,
        },
    }];
    state.set_expansion_done(2000, coroutine_fields);
    state.set_cursor(rc_field(10, 0, &[0]));
    state.collapse_object_recursive(2000);
    let flat = state.flat_items();
    assert!(
        flat.contains(state.cursor()),
        "cursor must be in flat_items after collapse, got: {:?}",
        state.cursor(),
    );
    assert!(
        matches!(state.cursor(), RenderCursor::At(p) if p.segments().len() == 2),
        "cursor should fall back to OnVar, got: {:?}",
        state.cursor(),
    );
    state.move_down();
    assert_ne!(
        state.cursor(),
        &rc_var(10, 0),
        "move_down must move away from OnVar"
    );
}

#[test]
fn collapse_nested_non_recursive_preserves_parent() {
    use hprof_engine::{FieldInfo, FieldValue};
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var_object_ref(0, 1000)];
    state.toggle_expand(10, vars);
    let thread_fields = vec![FieldInfo {
        name: "parkBlocker".to_string(),
        value: FieldValue::ObjectRef {
            id: 2000,
            class_name: "Coroutine".to_string(),
            entry_count: None,
            inline_value: None,
        },
    }];
    state.set_expansion_done(1000, thread_fields);
    let coroutine_fields = vec![FieldInfo {
        name: "blockedThread".to_string(),
        value: FieldValue::ObjectRef {
            id: 1000,
            class_name: "Thread".to_string(),
            entry_count: None,
            inline_value: None,
        },
    }];
    state.set_expansion_done(2000, coroutine_fields);
    state.set_cursor(rc_field(10, 0, &[0]));
    state.collapse_object(2000);
    assert_eq!(
        state.expansion_state(1000),
        ExpansionPhase::Expanded,
        "parent must remain expanded"
    );
    assert_eq!(state.expansion_state(2000), ExpansionPhase::Collapsed);
    let flat = state.flat_items();
    assert!(flat.contains(state.cursor()), "cursor must still be valid");
    assert_eq!(state.cursor(), &rc_field(10, 0, &[0]));
}

#[test]
fn chunk_ranges_total_50_no_sections() {
    let ranges = compute_chunk_ranges(50);
    assert!(ranges.is_empty());
}

#[test]
fn chunk_ranges_total_100_no_sections() {
    let ranges = compute_chunk_ranges(100);
    assert!(ranges.is_empty());
}

#[test]
fn chunk_ranges_total_150() {
    let ranges = compute_chunk_ranges(150);
    assert_eq!(ranges, vec![(100, 50)]);
}

#[test]
fn chunk_ranges_total_500() {
    let ranges = compute_chunk_ranges(500);
    assert_eq!(
        ranges,
        vec![(100, 100), (200, 100), (300, 100), (400, 100),]
    );
}

#[test]
fn chunk_ranges_total_1000() {
    let ranges = compute_chunk_ranges(1000);
    assert_eq!(ranges.len(), 9);
    assert_eq!(ranges[0], (100, 100));
    assert_eq!(ranges[8], (900, 100));
}

#[test]
fn chunk_ranges_total_3000() {
    let ranges = compute_chunk_ranges(3000);
    assert_eq!(ranges.len(), 11);
    assert_eq!(ranges[0], (100, 100));
    assert_eq!(ranges[8], (900, 100));
    assert_eq!(ranges[9], (1000, 1000));
    assert_eq!(ranges[10], (2000, 1000));
}

#[test]
fn chunk_ranges_total_2348() {
    let ranges = compute_chunk_ranges(2348);
    assert_eq!(ranges.len(), 11);
    assert_eq!(ranges[9], (1000, 1000));
    assert_eq!(ranges[10], (2000, 348));
}

// --- Page navigation tests (frames 1..=30, index N → frame_id N+1) ---

#[test]
fn page_down_jumps_by_visible_height() {
    let frames: Vec<_> = (1..=30).map(make_frame).collect();
    let mut state = StackState::new(frames);
    state.set_visible_height(20);
    for _ in 0..5 {
        state.move_down();
    }
    assert_eq!(state.cursor(), &rc_frame(6));
    state.move_page_down();
    assert_eq!(state.cursor(), &rc_frame(26));
}

#[test]
fn page_up_jumps_by_visible_height() {
    let frames: Vec<_> = (1..=30).map(make_frame).collect();
    let mut state = StackState::new(frames);
    state.set_visible_height(20);
    for _ in 0..25 {
        state.move_down();
    }
    assert_eq!(state.cursor(), &rc_frame(26));
    state.move_page_up();
    assert_eq!(state.cursor(), &rc_frame(6));
}

#[test]
fn page_down_clamps_to_last_item() {
    let frames: Vec<_> = (1..=10).map(make_frame).collect();
    let mut state = StackState::new(frames);
    state.set_visible_height(20);
    state.move_page_down();
    assert_eq!(state.cursor(), &rc_frame(10));
}

#[test]
fn page_up_clamps_to_first_item() {
    let frames: Vec<_> = (1..=10).map(make_frame).collect();
    let mut state = StackState::new(frames);
    state.set_visible_height(20);
    for _ in 0..3 {
        state.move_down();
    }
    state.move_page_up();
    assert_eq!(state.cursor(), &rc_frame(1));
}

#[test]
fn value_style_null_returns_null_value() {
    assert_eq!(field_value_style(&FieldValue::Null), THEME.null_value);
}

#[test]
fn value_style_int_returns_primitive_value() {
    assert_eq!(
        field_value_style(&FieldValue::Int(42)),
        THEME.primitive_value
    );
}

#[test]
fn value_style_bool_returns_primitive_value() {
    assert_eq!(
        field_value_style(&FieldValue::Bool(true)),
        THEME.primitive_value
    );
}

#[test]
fn value_style_char_returns_string_value() {
    assert_eq!(
        field_value_style(&FieldValue::Char('x')),
        THEME.string_value
    );
}

#[test]
fn value_style_object_ref_with_inline_value_returns_string_value() {
    let v = FieldValue::ObjectRef {
        id: 1,
        class_name: "java.lang.String".to_string(),
        entry_count: None,
        inline_value: Some("hello".to_string()),
    };
    assert_eq!(field_value_style(&v), THEME.string_value);
}

#[test]
fn value_style_object_ref_without_inline_returns_default() {
    let v = FieldValue::ObjectRef {
        id: 1,
        class_name: "java.util.HashMap".to_string(),
        entry_count: None,
        inline_value: None,
    };
    assert_eq!(field_value_style(&v), ratatui::style::Style::new());
}

// --- Story 9.1: Failed-state tests (AC1–AC5) ---

fn rendered_fg_at(item: ListItem<'static>, col: u16) -> ratatui::style::Color {
    use ratatui::{buffer::Buffer, layout::Rect, style::Color, widgets::Widget};
    let area = Rect::new(0, 0, 120, 1);
    let mut buf = Buffer::empty(area);
    Widget::render(List::new(vec![item]), area, &mut buf);
    buf.cell((col, 0)).map(|c| c.fg).unwrap_or(Color::Reset)
}

#[test]
fn enter_on_failed_var_is_noop() {
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var_object_ref(0, 42)];
    state.toggle_expand(10, vars);
    state.set_expansion_failed(42, "object absent".to_string());
    assert_eq!(state.expansion_state(42), ExpansionPhase::Failed);
    let flat = state.flat_items();
    assert!(
        flat.contains(&rc_var(10, 0)),
        "Failed var must stay in flat_items: {flat:?}"
    );
    assert!(
        !flat
            .iter()
            .any(|c| matches!(c, RenderCursor::LoadingNode(_))),
        "no loading node must appear for a Failed object"
    );
}

#[test]
fn enter_on_failed_collection_entry_is_noop() {
    use hprof_engine::{CollectionPage, EntryInfo, FieldInfo, FieldValue};
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var_object_ref(0, 99)];
    state.toggle_expand(10, vars);
    state.set_expansion_done(
        99,
        vec![FieldInfo {
            name: "items".to_string(),
            value: FieldValue::ObjectRef {
                id: 200,
                class_name: "ArrayList".to_string(),
                entry_count: Some(1),
                inline_value: None,
            },
        }],
    );
    let entry_obj_id = 300u64;
    let eager_page = CollectionPage {
        entries: vec![EntryInfo {
            index: 0,
            key: None,
            value: FieldValue::ObjectRef {
                id: entry_obj_id,
                class_name: "String".to_string(),
                entry_count: None,
                inline_value: None,
            },
        }],
        total_count: 1,
        offset: 0,
        has_more: false,
    };
    state
        .expansion
        .expansion_phases
        .insert(path_field(10, 0, &[0]), ExpansionPhase::Expanded);
    state.expansion.collection_chunks.insert(
        200,
        CollectionChunks {
            total_count: 1,
            eager_page: Some(eager_page),
            chunk_pages: std::collections::HashMap::new(),
        },
    );
    state.set_expansion_failed(entry_obj_id, "not found".to_string());
    assert_eq!(state.expansion_state(entry_obj_id), ExpansionPhase::Failed);
    let flat = state.flat_items();
    assert!(
        flat.iter().any(|c| {
            if let RenderCursor::At(p) = c {
                matches!(
                    p.segments().last(),
                    Some(PathSegment::CollectionEntry(_, EntryIdx(0)))
                )
            } else {
                false
            }
        }),
        "collection entry must remain in flat_items: {flat:?}"
    );
}

#[test]
fn failed_collection_entry_obj_no_phantom_cursor() {
    use hprof_engine::{CollectionPage, EntryInfo, FieldInfo, FieldValue};
    let frames = vec![make_frame(10), make_frame(20)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var_object_ref(0, 99)];
    state.toggle_expand(10, vars);
    state.set_expansion_done(
        99,
        vec![FieldInfo {
            name: "items".to_string(),
            value: FieldValue::ObjectRef {
                id: 200,
                class_name: "ArrayList".to_string(),
                entry_count: Some(1),
                inline_value: None,
            },
        }],
    );
    let eager_page = CollectionPage {
        entries: vec![EntryInfo {
            index: 0,
            key: None,
            value: FieldValue::ObjectRef {
                id: 300,
                class_name: "String".to_string(),
                entry_count: None,
                inline_value: None,
            },
        }],
        total_count: 1,
        offset: 0,
        has_more: false,
    };
    state
        .expansion
        .expansion_phases
        .insert(path_field(10, 0, &[0]), ExpansionPhase::Expanded);
    state.expansion.collection_chunks.insert(
        200,
        CollectionChunks {
            total_count: 1,
            eager_page: Some(eager_page),
            chunk_pages: std::collections::HashMap::new(),
        },
    );
    state.set_expansion_failed(300, "not found".to_string());
    assert_eq!(
        state.flat_items().len(),
        state.build_items().len(),
        "phantom cursor must not be emitted for Failed"
    );
    state.move_down(); // → Var
    state.move_down(); // → ObjField[0]
    state.move_down(); // → CollEntry{0}
    assert!(
        matches!(state.cursor(), RenderCursor::At(p)
            if matches!(p.segments().last(), Some(PathSegment::CollectionEntry(_, EntryIdx(0))))),
        "expected CollectionEntry, got {:?}",
        state.cursor()
    );
    state.move_down(); // must reach Frame(20)
    assert_eq!(state.cursor(), &rc_frame(20));
}

#[test]
fn failed_var_label_uses_stored_error_message() {
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var_object_ref(0, 99)];
    state.toggle_expand(10, vars);
    state.set_expansion_failed(99, "boom".to_string());
    let items = state.build_items();
    assert_eq!(
        items.len(),
        state.flat_items().len(),
        "build_items().len() must equal flat_items().len() (AC5)"
    );
    let text = item_text(items[1].clone());
    assert!(
        text.contains("! "),
        "var must show '! ' prefix, got: {text:?}"
    );
    assert!(
        text.contains("boom"),
        "var must contain stored error, got: {text:?}"
    );
}

#[test]
fn failed_var_style_is_error_indicator() {
    use ratatui::style::Color;
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var_object_ref(0, 99)];
    state.toggle_expand(10, vars);
    state.set_expansion_failed(99, "err".to_string());
    let items = state.build_items();
    let fg = rendered_fg_at(items[1].clone(), 4);
    assert_eq!(fg, Color::Red, "Failed var value must have Red fg");
}

#[test]
fn flat_items_build_items_equal_length_invariant() {
    use hprof_engine::{FieldInfo, FieldValue};
    {
        let state = StackState::new(vec![make_frame(1)]);
        assert_eq!(
            state.flat_items().len(),
            state.build_items().len(),
            "(b) collapsed"
        );
    }
    {
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 99)];
        state.toggle_expand(10, vars);
        state.set_expansion_failed(99, "err".to_string());
        assert_eq!(
            state.flat_items().len(),
            state.build_items().len(),
            "(c) var Failed"
        );
    }
    {
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 99)];
        state.toggle_expand(10, vars);
        state.set_expansion_done(
            99,
            vec![FieldInfo {
                name: "x".to_string(),
                value: FieldValue::Int(1),
            }],
        );
        assert_eq!(
            state.flat_items().len(),
            state.build_items().len(),
            "(d) expanded"
        );
    }
    {
        let frames = vec![make_frame(10), make_frame(20)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 100)];
        state.toggle_expand(10, vars);
        let nested_id = 200u64;
        state.set_expansion_done(
            100,
            vec![FieldInfo {
                name: "child".to_string(),
                value: FieldValue::ObjectRef {
                    id: nested_id,
                    class_name: "Foo".to_string(),
                    entry_count: None,
                    inline_value: None,
                },
            }],
        );
        state.set_expansion_failed(nested_id, "missing".to_string());
        assert_eq!(
            state.flat_items().len(),
            state.build_items().len(),
            "(f) nested Failed"
        );
    }
}

// --- selected_var_entry_count tests ---

#[test]
fn selected_var_entry_count_returns_some_when_on_var_with_entry_count() {
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    let vars = vec![VariableInfo {
        index: 0,
        value: VariableValue::ObjectRef {
            id: 0xA00,
            class_name: "Object[]".to_string(),
            entry_count: Some(42),
        },
    }];
    state.toggle_expand(10, vars);
    state.set_cursor(rc_var(10, 0));
    assert_eq!(state.selected_var_entry_count(), Some(42));
}

#[test]
fn selected_var_entry_count_returns_none_when_on_var_without_entry_count() {
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    let vars = vec![VariableInfo {
        index: 0,
        value: VariableValue::ObjectRef {
            id: 0xA01,
            class_name: "Foo".to_string(),
            entry_count: None,
        },
    }];
    state.toggle_expand(10, vars);
    state.set_cursor(rc_var(10, 0));
    assert_eq!(state.selected_var_entry_count(), None);
}

#[test]
fn selected_var_entry_count_returns_none_when_cursor_not_on_var() {
    let frames = vec![make_frame(10)];
    let state = StackState::new(frames);
    assert_eq!(state.selected_var_entry_count(), None);
}

#[test]
fn object_array_var_has_correct_entry_count_and_object_id() {
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    let vars = vec![VariableInfo {
        index: 0,
        value: VariableValue::ObjectRef {
            id: 0xA00,
            class_name: "Object[]".to_string(),
            entry_count: Some(3),
        },
    }];
    state.toggle_expand(10, vars);
    state.set_cursor(rc_var(10, 0));
    assert_eq!(state.selected_var_entry_count(), Some(3));
    assert_eq!(state.selected_object_id(), Some(0xA00));
}

// --- selected_collection_entry_count tests ---

#[test]
fn selected_collection_entry_count_returns_some_for_nested_array_entry() {
    let coll_id = 0xC011u64;
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    state.toggle_expand(10, vec![make_var_object_ref(0, 0xA00)]);
    state.expansion.collection_chunks.insert(
        coll_id,
        CollectionChunks {
            total_count: 1,
            eager_page: Some(CollectionPage {
                entries: vec![hprof_engine::EntryInfo {
                    index: 0,
                    key: None,
                    value: FieldValue::ObjectRef {
                        id: 0xBB01,
                        class_name: "Object[]".to_string(),
                        entry_count: Some(3),
                        inline_value: None,
                    },
                }],
                total_count: 1,
                offset: 0,
                has_more: false,
            }),
            chunk_pages: std::collections::HashMap::new(),
        },
    );
    // cursor: var[0] of frame 10, no field path, collection entry 0
    state.set_cursor(rc_coll_entry(10, 0, &[], coll_id, 0));
    assert_eq!(state.selected_collection_entry_count(), Some(3));
}

#[test]
fn selected_collection_entry_count_returns_none_when_entry_not_collection() {
    let coll_id = 0xC012u64;
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    state.toggle_expand(10, vec![make_var_object_ref(0, 0xA00)]);
    state.expansion.collection_chunks.insert(
        coll_id,
        CollectionChunks {
            total_count: 1,
            eager_page: Some(CollectionPage {
                entries: vec![hprof_engine::EntryInfo {
                    index: 0,
                    key: None,
                    value: FieldValue::ObjectRef {
                        id: 0xBB01,
                        class_name: "Foo".to_string(),
                        entry_count: None,
                        inline_value: None,
                    },
                }],
                total_count: 1,
                offset: 0,
                has_more: false,
            }),
            chunk_pages: std::collections::HashMap::new(),
        },
    );
    state.set_cursor(rc_coll_entry(10, 0, &[], coll_id, 0));
    assert_eq!(state.selected_collection_entry_count(), None);
}

#[test]
fn selected_collection_entry_count_returns_none_when_cursor_not_on_entry() {
    let frames = vec![make_frame(10)];
    let state = StackState::new(frames);
    assert_eq!(state.selected_collection_entry_count(), None);
}

#[test]
fn selected_collection_entry_obj_field_collection_info_returns_some_for_array_field() {
    let coll_id = 0xC100u64;
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    state.toggle_expand(10, vec![make_var_object_ref(0, 0xA00)]);
    state.expansion.collection_chunks.insert(
        coll_id,
        CollectionChunks {
            total_count: 1,
            eager_page: Some(CollectionPage {
                entries: vec![hprof_engine::EntryInfo {
                    index: 0,
                    key: None,
                    value: FieldValue::ObjectRef {
                        id: 0x700,
                        class_name: "Node".to_string(),
                        entry_count: None,
                        inline_value: None,
                    },
                }],
                total_count: 1,
                offset: 0,
                has_more: false,
            }),
            chunk_pages: std::collections::HashMap::new(),
        },
    );
    state.set_expansion_done(
        0x700,
        vec![FieldInfo {
            name: "arr".to_string(),
            value: FieldValue::ObjectRef {
                id: 0x888,
                class_name: "Object[]".to_string(),
                entry_count: Some(3),
                inline_value: None,
            },
        }],
    );
    state.set_cursor(rc_coll_entry_field(10, 0, &[], coll_id, 0, &[0]));
    assert_eq!(
        state.selected_collection_entry_obj_field_collection_info(),
        Some((0x888, 3))
    );
}

#[test]
fn selected_collection_entry_obj_field_collection_info_returns_none_without_entry_count() {
    let coll_id = 0xC101u64;
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    state.toggle_expand(10, vec![make_var_object_ref(0, 0xA00)]);
    state.expansion.collection_chunks.insert(
        coll_id,
        CollectionChunks {
            total_count: 1,
            eager_page: Some(CollectionPage {
                entries: vec![hprof_engine::EntryInfo {
                    index: 0,
                    key: None,
                    value: FieldValue::ObjectRef {
                        id: 0x701,
                        class_name: "Node".to_string(),
                        entry_count: None,
                        inline_value: None,
                    },
                }],
                total_count: 1,
                offset: 0,
                has_more: false,
            }),
            chunk_pages: std::collections::HashMap::new(),
        },
    );
    state.set_expansion_done(
        0x701,
        vec![FieldInfo {
            name: "child".to_string(),
            value: FieldValue::ObjectRef {
                id: 0x889,
                class_name: "Foo".to_string(),
                entry_count: None,
                inline_value: None,
            },
        }],
    );
    state.set_cursor(rc_coll_entry_field(10, 0, &[], coll_id, 0, &[0]));
    assert_eq!(
        state.selected_collection_entry_obj_field_collection_info(),
        None
    );
}

#[test]
fn flat_items_include_nested_collection_entries_for_multidimensional_arrays() {
    let outer_id = 0xD100u64;
    let inner_id = 0xD101u64;
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    state.toggle_expand(
        10,
        vec![VariableInfo {
            index: 0,
            value: VariableValue::ObjectRef {
                id: outer_id,
                class_name: "Object[]".to_string(),
                entry_count: Some(1),
            },
        }],
    );
    // outer_id is the var itself (collection var), so expansion_phases key = var path
    state.expansion.expansion_phases.insert(
        NavigationPathBuilder::new(FrameId(10), VarIdx(0)).build(),
        ExpansionPhase::Expanded,
    );
    state.expansion.collection_chunks.insert(
        outer_id,
        CollectionChunks {
            total_count: 1,
            eager_page: Some(CollectionPage {
                entries: vec![hprof_engine::EntryInfo {
                    index: 0,
                    key: None,
                    value: FieldValue::ObjectRef {
                        id: inner_id,
                        class_name: "Object[]".to_string(),
                        entry_count: Some(2),
                        inline_value: None,
                    },
                }],
                total_count: 1,
                offset: 0,
                has_more: false,
            }),
            chunk_pages: std::collections::HashMap::new(),
        },
    );
    // inner_id is at entry[0] of outer_id
    state.expansion.expansion_phases.insert(
        path_coll_entry(10, 0, &[], outer_id, 0),
        ExpansionPhase::Expanded,
    );
    state.expansion.collection_chunks.insert(
        inner_id,
        CollectionChunks {
            total_count: 2,
            eager_page: Some(CollectionPage {
                entries: vec![
                    hprof_engine::EntryInfo {
                        index: 0,
                        key: None,
                        value: FieldValue::Int(1),
                    },
                    hprof_engine::EntryInfo {
                        index: 1,
                        key: None,
                        value: FieldValue::Int(2),
                    },
                ],
                total_count: 2,
                offset: 0,
                has_more: false,
            }),
            chunk_pages: std::collections::HashMap::new(),
        },
    );
    let flat = state.flat_items();
    assert!(
        flat.iter().any(|c| {
            if let RenderCursor::At(p) = c {
                matches!(
                    p.segments().last(),
                    Some(PathSegment::CollectionEntry(CollectionId(oid), EntryIdx(0)))
                    if *oid == outer_id
                )
            } else {
                false
            }
        }),
        "outer collection entry must be emitted"
    );
    assert!(
        flat.iter().any(|c| {
            if let RenderCursor::At(p) = c {
                matches!(
                    p.segments().last(),
                    Some(PathSegment::CollectionEntry(CollectionId(iid), EntryIdx(0)))
                    if *iid == inner_id
                )
            } else {
                false
            }
        }),
        "nested collection entry must be emitted"
    );
}

// --- Story 9.3: parent_cursor() and Left/Right edge cases ---

#[test]
fn parent_cursor_on_frame_returns_none() {
    let frames = vec![make_frame(1)];
    let mut state = StackState::new(frames);
    state.set_cursor(rc_frame(1));
    assert_eq!(state.parent_cursor(), None);
}

#[test]
fn parent_cursor_on_var_returns_on_frame() {
    let frames = vec![make_frame(10), make_frame(20)];
    let mut state = StackState::new(frames);
    state.toggle_expand(10, vec![make_var_object_ref(0, 1)]);
    // set cursor on var[0] of frame 20 (frame_idx=1 in old notation)
    state.set_cursor(rc_var(20, 0));
    assert_eq!(state.parent_cursor(), Some(rc_frame(20)));
}

#[test]
fn parent_cursor_on_object_field_depth1_returns_on_var() {
    let frames = vec![make_frame(1)];
    let mut state = StackState::new(frames);
    state.set_cursor(rc_field(1, 2, &[3]));
    assert_eq!(state.parent_cursor(), Some(rc_var(1, 2)));
}

#[test]
fn parent_cursor_on_object_field_depth2_returns_shallower_field() {
    let frames = vec![make_frame(1)];
    let mut state = StackState::new(frames);
    state.set_cursor(rc_field(1, 0, &[1, 4]));
    assert_eq!(state.parent_cursor(), Some(rc_field(1, 0, &[1])));
}

#[test]
fn parent_cursor_on_collection_entry_with_field_path_returns_object_field() {
    let frames = vec![make_frame(1)];
    let mut state = StackState::new(frames);
    state.set_cursor(rc_coll_entry(1, 0, &[2], 0xA, 0));
    assert_eq!(state.parent_cursor(), Some(rc_field(1, 0, &[2])));
}

#[test]
fn parent_cursor_on_collection_entry_with_empty_field_path_returns_on_var() {
    let frames = vec![make_frame(1)];
    let mut state = StackState::new(frames);
    state.set_cursor(rc_coll_entry(1, 0, &[], 0xA, 0));
    assert_eq!(state.parent_cursor(), Some(rc_var(1, 0)));
}

#[test]
fn parent_cursor_on_collection_entry_obj_field_returns_collection_entry() {
    let frames = vec![make_frame(1)];
    let mut state = StackState::new(frames);
    state.set_cursor(rc_coll_entry_field(1, 0, &[], 0xA, 3, &[1]));
    assert_eq!(
        state.parent_cursor(),
        Some(rc_coll_entry(1, 0, &[], 0xA, 3))
    );
}

#[test]
fn parent_cursor_on_collection_entry_obj_field_depth2_returns_shallow_obj_field() {
    let frames = vec![make_frame(1)];
    let mut state = StackState::new(frames);
    state.set_cursor(rc_coll_entry_field(1, 0, &[], 0xA, 3, &[1, 3]));
    assert_eq!(
        state.parent_cursor(),
        Some(rc_coll_entry_field(1, 0, &[], 0xA, 3, &[1]))
    );
}

#[test]
fn left_on_non_expanded_var_navigates_to_frame() {
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    state.toggle_expand(10, vec![make_var_object_ref(0, 99)]);
    state.set_cursor(rc_var(10, 0));
    assert_eq!(state.parent_cursor(), Some(rc_frame(10)));
}

#[test]
fn left_on_non_expanded_frame_is_noop() {
    let frames = vec![make_frame(10)];
    let state = StackState::new(frames);
    assert!(!state.is_expanded(10));
    assert_eq!(state.parent_cursor(), None);
    assert_eq!(state.cursor(), &rc_frame(10));
}

#[test]
fn left_on_expanded_var_collapses_not_navigates() {
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    state.toggle_expand(10, vec![make_var_object_ref(0, 99)]);
    state.set_expansion_done(99, vec![]);
    state.set_cursor(rc_var(10, 0));
    assert_eq!(state.expansion_state(99), ExpansionPhase::Expanded);
    assert_eq!(state.parent_cursor(), Some(rc_frame(10)));
    assert_eq!(state.expansion_state(99), ExpansionPhase::Expanded);
}

#[test]
fn left_on_primitive_var_navigates_to_frame() {
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    state.toggle_expand(10, vec![make_var(0, 0)]);
    state.set_cursor(rc_var(10, 0));
    assert!(
        state.selected_object_id().is_none(),
        "Null var must have no object_id"
    );
    assert_eq!(state.parent_cursor(), Some(rc_frame(10)));
}

#[test]
fn right_on_collection_var_dispatches_start_collection() {
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    let collection_var = VariableInfo {
        index: 0,
        value: VariableValue::ObjectRef {
            id: 0xAB,
            class_name: "ArrayList".to_string(),
            entry_count: Some(5),
        },
    };
    state.toggle_expand(10, vec![collection_var]);
    state.set_cursor(rc_var(10, 0));
    assert_eq!(state.selected_var_entry_count(), Some(5));
    assert!(!state.expansion.collection_chunks.contains_key(&0xAB));
}

#[test]
fn cursor_collection_id_on_entry_with_field_path_returns_object_field_restore() {
    let frames = vec![make_frame(1)];
    let mut state = StackState::new(frames);
    state.set_cursor(rc_coll_entry(1, 0, &[2], 0xA, 0));
    let (cid, restore) = state.cursor_collection_id().expect("should return Some");
    assert_eq!(cid, 0xA);
    assert_eq!(restore, rc_field(1, 0, &[2]));
}

#[test]
fn left_from_collection_entry_inside_object_field_navigates_to_field_row() {
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    let obj_var = VariableInfo {
        index: 0,
        value: VariableValue::ObjectRef {
            id: 100,
            class_name: "MyObject".to_string(),
            entry_count: None,
        },
    };
    state.toggle_expand(10, vec![obj_var]);
    state.set_expansion_done(
        100,
        vec![
            FieldInfo {
                name: "count".to_string(),
                value: FieldValue::Int(42),
            },
            FieldInfo {
                name: "name".to_string(),
                value: FieldValue::ObjectRef {
                    id: 300,
                    class_name: "String".to_string(),
                    entry_count: None,
                    inline_value: None,
                },
            },
            FieldInfo {
                name: "items".to_string(),
                value: FieldValue::ObjectRef {
                    id: 200,
                    class_name: "ArrayList".to_string(),
                    entry_count: Some(1),
                    inline_value: None,
                },
            },
        ],
    );
    state
        .expansion
        .expansion_phases
        .insert(path_field(10, 0, &[2]), ExpansionPhase::Expanded);
    state.expansion.collection_chunks.insert(
        200,
        CollectionChunks {
            total_count: 1,
            eager_page: Some(CollectionPage {
                entries: vec![hprof_engine::EntryInfo {
                    index: 0,
                    key: None,
                    value: FieldValue::ObjectRef {
                        id: 500,
                        class_name: "Item".to_string(),
                        entry_count: None,
                        inline_value: None,
                    },
                }],
                total_count: 1,
                offset: 0,
                has_more: false,
            }),
            chunk_pages: std::collections::HashMap::new(),
        },
    );
    state.set_cursor(rc_coll_entry(10, 0, &[2], 200, 0));
    let flat = state.flat_items();
    let array_field_cursor = rc_field(10, 0, &[2]);
    assert!(
        flat.contains(&array_field_cursor),
        "flat_items must contain field[2]; got: {flat:?}"
    );
    assert_eq!(
        state.parent_cursor(),
        Some(array_field_cursor.clone()),
        "parent_cursor() from CollEntry(fp:[2]) must be field[2]"
    );
    state.set_cursor(array_field_cursor.clone());
    assert_eq!(state.cursor(), &array_field_cursor);
}

#[test]
fn parent_cursor_on_collection_entry_uses_path_parent_for_nested_collection() {
    // In the new model, parent_cursor() simply calls path.parent(). When cursor is
    // CollEntry(frame=1, var=0, coll=0xBB, entry=0) with no field prefix,
    // the parent is Var(frame=1, var=0).
    let frames = vec![make_frame(1)];
    let mut state = StackState::new(frames);
    state.set_cursor(rc_coll_entry(1, 0, &[], 0xBB, 0));
    assert_eq!(state.parent_cursor(), Some(rc_var(1, 0)));
}

#[test]
fn left_on_collection_entry_obj_field_with_open_collection_detects_collapse() {
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    state.toggle_expand(10, vec![make_var_object_ref(0, 0xAB)]);
    state.expansion.expansion_phases.insert(
        NavigationPathBuilder::new(FrameId(10), VarIdx(0)).build(),
        ExpansionPhase::Expanded,
    );
    state.expansion.collection_chunks.insert(
        0xAA,
        CollectionChunks {
            total_count: 1,
            eager_page: Some(CollectionPage {
                entries: vec![hprof_engine::EntryInfo {
                    index: 0,
                    key: None,
                    value: FieldValue::ObjectRef {
                        id: 200,
                        class_name: "CustomType".to_string(),
                        entry_count: None,
                        inline_value: None,
                    },
                }],
                total_count: 1,
                offset: 0,
                has_more: false,
            }),
            chunk_pages: std::collections::HashMap::new(),
        },
    );
    state.set_expansion_done(
        200,
        vec![
            FieldInfo {
                name: "x".to_string(),
                value: FieldValue::Int(1),
            },
            FieldInfo {
                name: "y".to_string(),
                value: FieldValue::Int(2),
            },
            FieldInfo {
                name: "items".to_string(),
                value: FieldValue::ObjectRef {
                    id: 300,
                    class_name: "int[]".to_string(),
                    entry_count: Some(5),
                    inline_value: None,
                },
            },
        ],
    );
    state.expansion.collection_chunks.insert(
        300,
        CollectionChunks {
            total_count: 5,
            eager_page: None,
            chunk_pages: std::collections::HashMap::new(),
        },
    );
    state.set_cursor(rc_coll_entry_field(10, 0, &[], 0xAA, 0, &[2]));
    let coll_info = state.selected_collection_entry_obj_field_collection_info();
    assert_eq!(
        coll_info,
        Some((300, 5)),
        "must detect inner array collection info"
    );
    assert!(state.expansion.collection_chunks.contains_key(&300));
}

// --- Scroll view tests ---

#[test]
fn scroll_view_down_shifts_offset_without_moving_cursor() {
    let frames = (0..5).map(make_frame).collect();
    let mut state = StackState::new(frames);
    state.move_down(); // frame 1
    state.move_down(); // frame 2
    state.set_visible_height(3);
    state.set_list_state_offset_for_test(0);
    state.scroll_view_down();
    assert_eq!(state.list_state_offset_for_test(), 1);
    assert_eq!(state.selected_frame_id(), Some(2));
}

#[test]
fn scroll_view_up_shifts_offset_without_moving_cursor() {
    let frames = (0..5).map(make_frame).collect();
    let mut state = StackState::new(frames);
    state.move_down(); // frame 1
    state.set_visible_height(3);
    state.set_list_state_offset_for_test(1);
    state.scroll_view_up();
    assert_eq!(state.list_state_offset_for_test(), 0);
    assert_eq!(state.selected_frame_id(), Some(1));
}

#[test]
fn scroll_view_down_snaps_back_when_cursor_would_leave_viewport() {
    let frames = (0..5).map(make_frame).collect();
    let mut state = StackState::new(frames);
    state.set_visible_height(3);
    state.scroll_view_down();
    assert_eq!(state.list_state_offset_for_test(), 0);
    assert_eq!(state.selected_frame_id(), Some(0));
}

#[test]
fn scroll_view_up_snaps_when_cursor_at_bottom_of_viewport() {
    let frames = (0..5).map(make_frame).collect();
    let mut state = StackState::new(frames);
    state.move_down();
    state.move_down();
    state.move_down();
    state.move_down();
    state.set_visible_height(2);
    state.set_list_state_offset_for_test(3);
    state.scroll_view_up();
    assert_eq!(state.list_state_offset_for_test(), 3);
    assert_eq!(state.selected_frame_id(), Some(4));
}

#[test]
fn scroll_view_no_op_when_no_frames() {
    let mut state = StackState::new(vec![]);
    state.set_visible_height(5);
    state.scroll_view_up();
    state.scroll_view_down();
    assert_eq!(state.list_state_offset_for_test(), 0);
}

#[test]
fn scroll_view_no_op_when_visible_height_zero() {
    let frames = (0..5).map(make_frame).collect();
    let mut state = StackState::new(frames);
    state.move_down();
    state.move_down();
    state.set_visible_height(0);
    state.set_list_state_offset_for_test(0);
    state.scroll_view_up();
    state.scroll_view_down();
    assert_eq!(state.list_state_offset_for_test(), 0);
    assert_eq!(state.selected_frame_id(), Some(2));
}

#[test]
fn scroll_view_down_no_op_when_list_fits_in_viewport() {
    let frames = (0..3).map(make_frame).collect();
    let mut state = StackState::new(frames);
    state.move_down();
    state.set_visible_height(10);
    state.scroll_view_down();
    assert_eq!(state.list_state_offset_for_test(), 0);
    assert_eq!(state.selected_frame_id(), Some(1));
}

#[test]
fn scroll_view_down_clamps_stale_offset_before_increment() {
    let frames = (0..5).map(make_frame).collect();
    let mut state = StackState::new(frames);
    state.move_down();
    state.move_down();
    state.set_visible_height(3);
    state.set_list_state_offset_for_test(usize::MAX);
    state.scroll_view_down();
    assert_eq!(state.list_state_offset_for_test(), 2);
    assert_eq!(state.selected_frame_id(), Some(2));
}

#[test]
fn center_view_on_selection_places_cursor_near_middle() {
    let frames = (0..12).map(make_frame).collect();
    let mut state = StackState::new(frames);
    for _ in 0..7 {
        state.move_down();
    }
    state.set_visible_height(5);
    state.set_list_state_offset_for_test(0);
    state.center_view_on_selection();
    assert_eq!(state.list_state_offset_for_test(), 5);
    assert_eq!(state.selected_frame_id(), Some(7));
}

#[test]
fn center_view_on_selection_clamps_at_top() {
    let frames = (0..10).map(make_frame).collect();
    let mut state = StackState::new(frames);
    state.move_down();
    state.set_visible_height(5);
    state.set_list_state_offset_for_test(4);
    state.center_view_on_selection();
    assert_eq!(state.list_state_offset_for_test(), 0);
    assert_eq!(state.selected_frame_id(), Some(1));
}

#[test]
fn center_view_on_selection_clamps_at_bottom() {
    let frames = (0..10).map(make_frame).collect();
    let mut state = StackState::new(frames);
    for _ in 0..9 {
        state.move_down();
    }
    state.set_visible_height(5);
    state.set_list_state_offset_for_test(0);
    state.center_view_on_selection();
    assert_eq!(state.list_state_offset_for_test(), 5);
    assert_eq!(state.selected_frame_id(), Some(9));
}

#[test]
fn center_view_on_selection_no_op_when_visible_height_zero() {
    let frames = (0..5).map(make_frame).collect();
    let mut state = StackState::new(frames);
    state.move_down();
    state.move_down();
    state.set_visible_height(0);
    state.set_list_state_offset_for_test(1);
    state.center_view_on_selection();
    assert_eq!(state.list_state_offset_for_test(), 1);
    assert_eq!(state.selected_frame_id(), Some(2));
}

#[test]
fn scroll_view_page_down_shifts_offset_without_moving_cursor() {
    let frames = (0..12).map(make_frame).collect();
    let mut state = StackState::new(frames);
    for _ in 0..7 {
        state.move_down();
    }
    state.set_visible_height(4);
    state.set_list_state_offset_for_test(0);
    state.scroll_view_page_down();
    assert_eq!(state.list_state_offset_for_test(), 4);
    assert_eq!(state.selected_frame_id(), Some(7));
}

#[test]
fn scroll_view_page_up_shifts_offset_without_moving_cursor() {
    let frames = (0..12).map(make_frame).collect();
    let mut state = StackState::new(frames);
    for _ in 0..7 {
        state.move_down();
    }
    state.set_visible_height(4);
    state.set_list_state_offset_for_test(8);
    state.scroll_view_page_up();
    assert_eq!(state.list_state_offset_for_test(), 4);
    assert_eq!(state.selected_frame_id(), Some(7));
}

#[test]
fn scroll_view_page_down_snaps_back_when_cursor_would_leave_viewport() {
    let frames = (0..12).map(make_frame).collect();
    let mut state = StackState::new(frames);
    state.set_visible_height(4);
    state.set_list_state_offset_for_test(0);
    state.scroll_view_page_down();
    assert_eq!(state.list_state_offset_for_test(), 0);
    assert_eq!(state.selected_frame_id(), Some(0));
}

#[test]
fn scroll_view_page_up_snaps_when_cursor_at_bottom_edge() {
    let frames = (0..12).map(make_frame).collect();
    let mut state = StackState::new(frames);
    for _ in 0..11 {
        state.move_down();
    }
    state.set_visible_height(4);
    state.set_list_state_offset_for_test(8);
    state.scroll_view_page_up();
    assert_eq!(state.list_state_offset_for_test(), 8);
    assert_eq!(state.selected_frame_id(), Some(11));
}

// --- Static field rendering tests ---

#[test]
fn render_static_section_for_collection_entry_object() {
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    state.toggle_expand(10, vec![make_var_object_ref(0, 0xA00)]);
    state.set_expansion_done(
        0xA00,
        vec![FieldInfo {
            name: "items".to_string(),
            value: FieldValue::ObjectRef {
                id: 0xC00,
                class_name: "java.util.ArrayList".to_string(),
                entry_count: Some(1),
                inline_value: None,
            },
        }],
    );
    state
        .expansion
        .expansion_phases
        .insert(path_field(10, 0, &[0]), ExpansionPhase::Expanded);
    state.expansion.collection_chunks.insert(
        0xC00,
        CollectionChunks {
            total_count: 1,
            eager_page: Some(CollectionPage {
                entries: vec![hprof_engine::EntryInfo {
                    index: 0,
                    key: None,
                    value: FieldValue::ObjectRef {
                        id: 0x700,
                        class_name: "Node".to_string(),
                        entry_count: None,
                        inline_value: None,
                    },
                }],
                total_count: 1,
                offset: 0,
                has_more: false,
            }),
            chunk_pages: std::collections::HashMap::new(),
        },
    );
    state.set_expansion_done(
        0x700,
        vec![FieldInfo {
            name: "value".to_string(),
            value: FieldValue::Int(7),
        }],
    );
    state.set_static_fields(
        0x700,
        vec![FieldInfo {
            name: "STATIC_ONE".to_string(),
            value: FieldValue::Int(1),
        }],
    );
    let rendered: Vec<String> = state.build_items().into_iter().map(item_text).collect();
    assert!(
        rendered.iter().any(|l| l.contains("[static]")),
        "static header must be rendered: {rendered:?}"
    );
    assert!(
        rendered.iter().any(|l| l.contains("STATIC_ONE: 1")),
        "static field row must be rendered: {rendered:?}"
    );
    assert_eq!(state.flat_items().len(), state.build_items().len());
}

#[test]
fn render_collection_entry_static_helper_rows_not_navigable() {
    let frames = vec![make_frame(10), make_frame(20)];
    let mut state = StackState::new(frames);
    state.toggle_expand(10, vec![make_var_object_ref(0, 0xB00)]);
    state.set_expansion_done(
        0xB00,
        vec![FieldInfo {
            name: "items".to_string(),
            value: FieldValue::ObjectRef {
                id: 0xC10,
                class_name: "java.util.ArrayList".to_string(),
                entry_count: Some(1),
                inline_value: None,
            },
        }],
    );
    state
        .expansion
        .expansion_phases
        .insert(path_field(10, 0, &[0]), ExpansionPhase::Expanded);
    state.expansion.collection_chunks.insert(
        0xC10,
        CollectionChunks {
            total_count: 1,
            eager_page: Some(CollectionPage {
                entries: vec![hprof_engine::EntryInfo {
                    index: 0,
                    key: None,
                    value: FieldValue::ObjectRef {
                        id: 0x710,
                        class_name: "Node".to_string(),
                        entry_count: None,
                        inline_value: None,
                    },
                }],
                total_count: 1,
                offset: 0,
                has_more: false,
            }),
            chunk_pages: std::collections::HashMap::new(),
        },
    );
    state.set_expansion_done(
        0x710,
        vec![FieldInfo {
            name: "v".to_string(),
            value: FieldValue::Int(9),
        }],
    );
    let static_fields: Vec<FieldInfo> = (0..21)
        .map(|idx| FieldInfo {
            name: format!("S_{idx}"),
            value: FieldValue::Int(idx),
        })
        .collect();
    state.set_static_fields(0x710, static_fields);

    state.move_down(); // Frame(10) -> Var
    state.move_down(); // Var -> items field[0]
    state.move_down(); // items field -> entry[0]
    state.move_down(); // entry[0] -> entry obj field[0]
    // Should be on coll_entry_field: field[0] within entry[0] of coll 0xC10
    assert!(
        matches!(state.cursor(), RenderCursor::At(p)
            if p.segments().iter().any(|s| matches!(s, PathSegment::CollectionEntry(CollectionId(0xC10), EntryIdx(0))))
            && matches!(p.segments().last(), Some(PathSegment::Field(FieldIdx(0))))),
        "cursor must be on entry obj field[0], got: {:?}",
        state.cursor()
    );

    // Skip non-interactive [static] header
    state.move_down();
    // Should be on static field 0
    assert!(
        matches!(state.cursor(), RenderCursor::At(p)
            if p.segments().last().is_some_and(|s| matches!(s, PathSegment::StaticField(StaticFieldIdx(0))))),
        "cursor must be on static field 0, got: {:?}",
        state.cursor()
    );

    // Reach last rendered static field (idx 19)
    for _ in 0..19 {
        state.move_down();
    }
    assert!(
        matches!(state.cursor(), RenderCursor::At(p)
            if p.segments().last().is_some_and(|s| matches!(s, PathSegment::StaticField(StaticFieldIdx(19))))),
        "cursor must be on static field 19, got: {:?}",
        state.cursor()
    );

    // Skip non-interactive overflow row
    state.move_down();
    assert_eq!(state.cursor(), &rc_frame(20));

    // Moving up must also skip overflow and land back on static idx 19
    state.move_up();
    assert!(
        matches!(state.cursor(), RenderCursor::At(p)
            if p.segments().last().is_some_and(|s| matches!(s, PathSegment::StaticField(StaticFieldIdx(19))))),
        "cursor must be on static field 19 after move_up, got: {:?}",
        state.cursor()
    );
}

#[test]
fn render_static_section_separator_not_navigable() {
    let frames = vec![make_frame(10), make_frame(20)];
    let mut state = StackState::new(frames);
    state.toggle_expand(10, vec![make_var_object_ref(0, 0xA00)]);
    state.set_expansion_done(
        0xA00,
        vec![FieldInfo {
            name: "instance".to_string(),
            value: FieldValue::Int(1),
        }],
    );
    state.set_static_fields(
        0xA00,
        vec![FieldInfo {
            name: "STATIC_ONE".to_string(),
            value: FieldValue::Int(42),
        }],
    );

    state.move_down(); // Frame(10) -> Var
    state.move_down(); // Var -> instance field[0]
    assert_eq!(state.cursor(), &rc_field(10, 0, &[0]));

    // Must skip non-interactive [static] header
    state.move_down();
    assert!(
        matches!(state.cursor(), RenderCursor::At(p)
            if matches!(p.segments().last(), Some(PathSegment::StaticField(StaticFieldIdx(0))))),
        "cursor must be on static field 0, got: {:?}",
        state.cursor()
    );

    // Must also skip header when moving back up
    state.move_up();
    assert_eq!(state.cursor(), &rc_field(10, 0, &[0]));
}

#[test]
fn render_static_overflow_row_not_navigable() {
    let frames = vec![make_frame(10), make_frame(20)];
    let mut state = StackState::new(frames);
    state.toggle_expand(10, vec![make_var_object_ref(0, 0xB00)]);
    state.set_expansion_done(
        0xB00,
        vec![FieldInfo {
            name: "instance".to_string(),
            value: FieldValue::Int(1),
        }],
    );
    let static_fields: Vec<FieldInfo> = (0..21)
        .map(|idx| FieldInfo {
            name: format!("STATIC_{idx}"),
            value: FieldValue::Int(idx),
        })
        .collect();
    state.set_static_fields(0xB00, static_fields);

    state.move_down(); // Frame(10) -> Var
    state.move_down(); // Var -> instance field
    state.move_down(); // instance field -> static[0], skipping [static]
    for _ in 0..19 {
        state.move_down();
    }
    assert!(
        matches!(state.cursor(), RenderCursor::At(p)
            if matches!(p.segments().last(), Some(PathSegment::StaticField(StaticFieldIdx(19))))),
        "cursor must be on static field 19, got: {:?}",
        state.cursor()
    );

    // Must skip non-interactive overflow row to next interactive row
    state.move_down();
    assert_eq!(state.cursor(), &rc_frame(20));

    // And skip overflow row when navigating upward
    state.move_up();
    assert!(
        matches!(state.cursor(), RenderCursor::At(p)
            if matches!(p.segments().last(), Some(PathSegment::StaticField(StaticFieldIdx(19))))),
        "cursor must be on static field 19 after move_up, got: {:?}",
        state.cursor()
    );
}

#[test]
fn static_object_field_rows_are_emitted_and_navigable() {
    let frames = vec![make_frame(10), make_frame(20)];
    let mut state = StackState::new(frames);
    state.toggle_expand(10, vec![make_var_object_ref(0, 0xA00)]);
    state.set_expansion_done(
        0xA00,
        vec![FieldInfo {
            name: "instance".to_string(),
            value: FieldValue::Int(1),
        }],
    );
    state.set_static_fields(
        0xA00,
        vec![FieldInfo {
            name: "S_CHILD".to_string(),
            value: FieldValue::ObjectRef {
                id: 0xB00,
                class_name: "Child".to_string(),
                entry_count: None,
                inline_value: None,
            },
        }],
    );
    state.set_expansion_done(
        0xB00,
        vec![FieldInfo {
            name: "leaf".to_string(),
            value: FieldValue::Int(7),
        }],
    );

    state.move_down(); // Frame(10) -> Var
    state.move_down(); // Var -> instance field
    state.move_down(); // instance field -> static field (skip [static] header)
    assert!(
        matches!(state.cursor(), RenderCursor::At(p)
            if matches!(p.segments().last(), Some(PathSegment::StaticField(StaticFieldIdx(0))))),
        "cursor must be on static field 0, got: {:?}",
        state.cursor()
    );

    state.move_down(); // static field -> static object child field
    assert!(
        matches!(state.cursor(), RenderCursor::At(p)
            if matches!(p.segments().last(), Some(PathSegment::Field(FieldIdx(0))))
            && p.segments().iter().any(|s| matches!(s, PathSegment::StaticField(_)))),
        "cursor must be on static obj field[0], got: {:?}",
        state.cursor()
    );

    // parent_cursor of static obj field[0] = static field[0]
    assert_eq!(state.parent_cursor(), Some(rc_static_field(10, 0, &[], 0)));

    let rendered: Vec<String> = state.build_items().into_iter().map(item_text).collect();
    assert!(
        rendered.iter().any(|l| l.contains("leaf: 7")),
        "expanded static object children must be rendered: {rendered:?}"
    );
    assert_eq!(state.flat_items().len(), state.build_items().len());
}

#[test]
fn collection_entry_static_object_field_rows_are_emitted() {
    let frames = vec![make_frame(10), make_frame(20)];
    let mut state = StackState::new(frames);
    state.toggle_expand(10, vec![make_var_object_ref(0, 0xC00)]);
    state.set_expansion_done(
        0xC00,
        vec![FieldInfo {
            name: "items".to_string(),
            value: FieldValue::ObjectRef {
                id: 0xD00,
                class_name: "java.util.ArrayList".to_string(),
                entry_count: Some(1),
                inline_value: None,
            },
        }],
    );
    state
        .expansion
        .expansion_phases
        .insert(path_field(10, 0, &[0]), ExpansionPhase::Expanded);
    state.expansion.collection_chunks.insert(
        0xD00,
        CollectionChunks {
            total_count: 1,
            eager_page: Some(CollectionPage {
                entries: vec![hprof_engine::EntryInfo {
                    index: 0,
                    key: None,
                    value: FieldValue::ObjectRef {
                        id: 0x710,
                        class_name: "Node".to_string(),
                        entry_count: None,
                        inline_value: None,
                    },
                }],
                total_count: 1,
                offset: 0,
                has_more: false,
            }),
            chunk_pages: std::collections::HashMap::new(),
        },
    );
    state.set_expansion_done(
        0x710,
        vec![FieldInfo {
            name: "v".to_string(),
            value: FieldValue::Int(9),
        }],
    );
    state.set_static_fields(
        0x710,
        vec![FieldInfo {
            name: "S_CHILD".to_string(),
            value: FieldValue::ObjectRef {
                id: 0x720,
                class_name: "Child".to_string(),
                entry_count: None,
                inline_value: None,
            },
        }],
    );
    state.set_expansion_done(
        0x720,
        vec![FieldInfo {
            name: "x".to_string(),
            value: FieldValue::Int(3),
        }],
    );

    state.move_down(); // Frame(10) -> Var
    state.move_down(); // Var -> items field[0]
    state.move_down(); // items field -> entry[0]
    state.move_down(); // entry[0] -> entry obj field[0]
    state.move_down(); // -> static field (skipping [static] header)
    assert!(
        matches!(state.cursor(), RenderCursor::At(p)
            if p.segments().last().is_some_and(|s| matches!(s, PathSegment::StaticField(StaticFieldIdx(0))))),
        "cursor must be on static field 0, got: {:?}",
        state.cursor()
    );

    state.move_down(); // -> static child field
    assert!(
        matches!(state.cursor(), RenderCursor::At(p)
            if matches!(p.segments().last(), Some(PathSegment::Field(FieldIdx(0))))
            && p.segments().iter().any(|s| matches!(s, PathSegment::StaticField(_)))),
        "cursor must be on static obj child field[0], got: {:?}",
        state.cursor()
    );

    let rendered: Vec<String> = state.build_items().into_iter().map(item_text).collect();
    assert!(
        rendered.iter().any(|l| l.contains("x: 3")),
        "expanded collection-entry static object children must be rendered: {rendered:?}"
    );
    assert_eq!(state.flat_items().len(), state.build_items().len());
}

// === NavigationPath unit tests (Task 4) ===

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

fn nav_hash(path: &NavigationPath) -> u64 {
    let mut h = DefaultHasher::new();
    path.hash(&mut h);
    h.finish()
}

#[test]
fn nav_path_two_builders_same_logical_path_are_eq_and_same_hash() {
    let fid = FrameId(10);
    let vid = VarIdx(2);
    let fi = FieldIdx(3);
    let path_a = NavigationPathBuilder::new(fid, vid).field(fi).build();
    let path_b = NavigationPathBuilder::new(fid, vid).field(fi).build();
    assert_eq!(path_a, path_b);
    assert_eq!(nav_hash(&path_a), nav_hash(&path_b));
}

#[test]
#[should_panic]
fn nav_path_build_panics_when_frame_at_position_2() {
    let bad = NavigationPath::from_raw(vec![
        PathSegment::Frame(FrameId(1)),
        PathSegment::Var(VarIdx(0)),
        PathSegment::Frame(FrameId(2)),
    ]);
    let _b = NavigationPathBuilder::extend(bad).build();
}

#[test]
#[should_panic]
fn nav_path_build_panics_when_var_at_position_2() {
    let bad = NavigationPath::from_raw(vec![
        PathSegment::Frame(FrameId(1)),
        PathSegment::Var(VarIdx(0)),
        PathSegment::Var(VarIdx(1)),
    ]);
    let _b = NavigationPathBuilder::extend(bad).build();
}

#[test]
fn nav_path_parent_returns_none_on_frame_only() {
    let path = NavigationPathBuilder::frame_only(FrameId(42));
    assert_eq!(path.parent(), None);
}

#[test]
fn nav_path_parent_returns_frame_only_on_depth_2() {
    let path = NavigationPathBuilder::new(FrameId(1), VarIdx(0)).build();
    let parent = path.parent().expect("depth-2 must have parent");
    assert_eq!(parent, NavigationPathBuilder::frame_only(FrameId(1)));
}

#[test]
fn nav_path_parent_truncates_at_depth_3_plus() {
    let path = NavigationPathBuilder::new(FrameId(1), VarIdx(0))
        .field(FieldIdx(2))
        .build();
    let parent = path.parent().expect("depth-3 must have parent");
    let expected = NavigationPathBuilder::new(FrameId(1), VarIdx(0)).build();
    assert_eq!(parent, expected);
}

#[test]
fn nav_path_frame_only_builds_valid_depth1_path() {
    let path = NavigationPathBuilder::frame_only(FrameId(7));
    assert_eq!(path.segments().len(), 1);
    assert!(matches!(path.segments()[0], PathSegment::Frame(FrameId(7))));
}

// --- Task 18: instance-scoped expansion ---

#[test]
fn expansion_at_path_a_does_not_affect_path_b() {
    use hprof_engine::{FieldInfo, FieldValue};
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    let vars = vec![
        make_var_object_ref(0, 100),
        make_var_object_ref(1, 100), // same object_id
    ];
    state.toggle_expand(10, vars);

    let path_a = NavigationPathBuilder::new(FrameId(10), VarIdx(0)).build();
    let path_b = NavigationPathBuilder::new(FrameId(10), VarIdx(1)).build();

    let fields = vec![FieldInfo {
        name: "x".to_string(),
        value: FieldValue::Int(1),
    }];
    state.set_expansion_done_at_path(&path_a, 100, fields);

    assert_eq!(
        state.expansion_state_for_path(&path_b),
        ExpansionPhase::Collapsed,
        "expansion at path A must not affect path B"
    );
}
