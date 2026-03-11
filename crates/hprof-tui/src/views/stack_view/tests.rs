use hprof_engine::{FieldValue, FrameInfo, LineNumber, VariableInfo, VariableValue};

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

#[test]
fn new_with_three_frames_selects_frame_0() {
    let frames = vec![make_frame(1), make_frame(2), make_frame(3)];
    let state = StackState::new(frames);
    assert_eq!(state.cursor, StackCursor::OnFrame(0));
}

#[test]
fn move_down_on_three_frames_with_no_expanded_moves_to_frame_1() {
    let frames = vec![make_frame(1), make_frame(2), make_frame(3)];
    let mut state = StackState::new(frames);
    state.move_down();
    assert_eq!(state.cursor, StackCursor::OnFrame(1));
}

#[test]
fn move_up_at_frame_0_does_nothing() {
    let frames = vec![make_frame(1), make_frame(2), make_frame(3)];
    let mut state = StackState::new(frames);
    state.move_up();
    assert_eq!(state.cursor, StackCursor::OnFrame(0));
}

#[test]
fn toggle_expand_with_vars_then_move_down_moves_to_var_0() {
    let frames = vec![make_frame(10), make_frame(20)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var(0, 1), make_var(1, 2)];
    state.toggle_expand(10, vars);
    // cursor is still OnFrame(0), move_down should go to OnVar{frame_idx:0, var_idx:0}
    state.move_down();
    assert_eq!(
        state.cursor,
        StackCursor::OnVar {
            frame_idx: 0,
            var_idx: 0
        }
    );
}

#[test]
fn move_down_past_last_var_of_expanded_frame_moves_to_next_frame() {
    let frames = vec![make_frame(10), make_frame(20)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var(0, 1)];
    state.toggle_expand(10, vars);
    // flat: [Frame(0), Var{0,0}, Frame(1)]
    state.move_down(); // → Var{0,0}
    state.move_down(); // → Frame(1)
    assert_eq!(state.cursor, StackCursor::OnFrame(1));
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
    state.move_down(); // → OnVar{frame_idx:0, var_idx:0}
    assert_eq!(
        state.cursor,
        StackCursor::OnVar {
            frame_idx: 0,
            var_idx: 0
        }
    );
    // Collapse while cursor is on a var
    state.toggle_expand(10, vec![]);
    // Cursor must reset to the frame row
    assert_eq!(state.cursor, StackCursor::OnFrame(0));
    // Navigation must work: can move to the next frame
    state.move_down();
    assert_eq!(state.cursor, StackCursor::OnFrame(1));
    // And back
    state.move_up();
    assert_eq!(state.cursor, StackCursor::OnFrame(0));
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
    // Cursor was on OnObjectLoadingNode (var-level loading spinner)
    // when failure arrives — cursor must snap back to OnVar so
    // navigation is not stuck.
    let frames = vec![make_frame(10), make_frame(20)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var_object_ref(0, 99)];
    state.toggle_expand(10, vars);
    state.set_expansion_loading(99);
    // Simulate user pressing Down onto the loading spinner row.
    state.move_down(); // OnFrame(0) → OnVar{0,0}
    state.move_down(); // OnVar{0,0} → OnObjectLoadingNode{0,0,[]}
    assert!(
        matches!(state.cursor, StackCursor::OnObjectLoadingNode { .. }),
        "precondition: cursor is on loading node"
    );
    state.set_expansion_failed(99, "err".to_string());
    // Cursor must be recovered to OnVar, not orphaned.
    assert_eq!(
        state.cursor,
        StackCursor::OnVar {
            frame_idx: 0,
            var_idx: 0
        },
        "cursor must snap to parent OnVar after failure"
    );
    // Navigation must resume: Down from OnVar{0,0} must reach OnFrame(1).
    state.move_down();
    assert_eq!(state.cursor, StackCursor::OnFrame(1));
}

#[test]
fn set_expansion_failed_recovers_cursor_from_loading_node_nested_field() {
    // Same as above but for a nested field object (field_path non-empty).
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
    // Expand the nested field object.
    state.set_expansion_loading(200);
    // Navigate: Frame → Var → ObjectField{[0]} → OnObjectLoadingNode{[0]}
    state.move_down(); // → OnVar{0,0}
    state.move_down(); // → OnObjectField{0,0,[0]}
    state.move_down(); // → OnObjectLoadingNode{0,0,[0]}
    assert!(
        matches!(state.cursor, StackCursor::OnObjectLoadingNode { .. }),
        "precondition: cursor is on nested loading node"
    );
    state.set_expansion_failed(200, "boom".to_string());
    // Cursor must recover to the parent OnObjectField.
    assert_eq!(
        state.cursor,
        StackCursor::OnObjectField {
            frame_idx: 0,
            var_idx: 0,
            field_path: vec![0],
        },
        "cursor must snap to parent OnObjectField after nested failure"
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
    assert!(flat.contains(&StackCursor::OnObjectLoadingNode {
        frame_idx: 0,
        var_idx: 0,
        field_path: vec![],
    }));
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
    assert!(flat.contains(&StackCursor::OnObjectField {
        frame_idx: 0,
        var_idx: 0,
        field_path: vec![0],
    }));
    assert!(flat.contains(&StackCursor::OnObjectField {
        frame_idx: 0,
        var_idx: 0,
        field_path: vec![1],
    }));
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
    // cursor is OnFrame(0), move down → OnVar{0,0}, move down → OnObjectField{0,0,[0]}
    state.move_down();
    assert_eq!(
        state.cursor,
        StackCursor::OnVar {
            frame_idx: 0,
            var_idx: 0
        }
    );
    state.move_down();
    assert_eq!(
        state.cursor,
        StackCursor::OnObjectField {
            frame_idx: 0,
            var_idx: 0,
            field_path: vec![0],
        }
    );
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
    // flat: [Frame(0), Var{0,0}, Field{0,0,0}, Frame(1)]
    state.move_down(); // Frame → Var
    state.move_down(); // Var → Field
    state.move_down(); // Field → Frame(1)
    assert_eq!(state.cursor, StackCursor::OnFrame(1));
}

#[test]
fn selected_loading_object_id_on_loading_node_returns_object_id() {
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var_object_ref(0, 42)];
    state.toggle_expand(10, vars);
    state.set_expansion_loading(42);
    // move to the loading node
    state.move_down(); // → OnVar{0,0}
    state.move_down(); // → OnObjectLoadingNode{0,0,field_path:[]}
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
    // Root object 100 has one ObjectRef field pointing to object 200.
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
    // Object 200 has one Int field.
    let fields_200 = vec![FieldInfo {
        name: "val".to_string(),
        value: FieldValue::Int(7),
    }];
    state.set_expansion_done(200, fields_200);

    let flat = state.flat_items();
    // Expected: Frame(0), Var{0,0}, Field{0,0,[0]}, Field{0,0,[0,0]}
    assert!(flat.contains(&StackCursor::OnObjectField {
        frame_idx: 0,
        var_idx: 0,
        field_path: vec![0],
    }));
    assert!(flat.contains(&StackCursor::OnObjectField {
        frame_idx: 0,
        var_idx: 0,
        field_path: vec![0, 0],
    }));
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
    // Navigate to the field at path [0]
    state.cursor = StackCursor::OnObjectField {
        frame_idx: 0,
        var_idx: 0,
        field_path: vec![0],
    };
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
    state.cursor = StackCursor::OnObjectField {
        frame_idx: 0,
        var_idx: 0,
        field_path: vec![0],
    };
    assert_eq!(state.selected_field_ref_id(), None);
}

// --- Task 7.4: recursive collapse tests ---

#[test]
fn collapse_object_recursive_removes_nested_expanded_child() {
    use hprof_engine::{FieldInfo, FieldValue};
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    // Expand root 100 → child 200
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
    // Artificial cycle: 100 → 200 → 100 (corrupted heap)
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
    // Must terminate without stack overflow
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
    // Expand object 100 (nested)
    let fields_100 = vec![FieldInfo {
        name: "x".to_string(),
        value: FieldValue::Int(1),
    }];
    state.set_expansion_done(100, fields_100);
    assert_eq!(state.expansion_state(100), ExpansionPhase::Expanded);
    // Collapse the frame
    state.toggle_expand(10, vec![]);
    // object_phases must be cleaned up
    assert!(state.object_phases.is_empty());
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
    // items[0] = frame, items[1] = var, items[2] = field
    assert_eq!(items.len(), 3);
    let text = item_text(items[2].clone());
    // 4-space indent + 2-char toggle prefix ("  " for primitives)
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
    // items[0]=frame, [1]=var, [2]=depth-1 field, [3]=depth-2 field
    assert_eq!(items.len(), 4);
    let depth1 = item_text(items[2].clone());
    // 4-space indent + 2-char toggle ("- " for expanded ObjectRef)
    assert!(
        depth1.starts_with("    - "),
        "depth-1 ObjectRef field must have toggle prefix, got: {depth1:?}"
    );
    let depth2 = item_text(items[3].clone());
    // 6-space indent + 2-char toggle ("  " for primitive)
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
    // items[0]=frame, [1]=var with inline error — no orphan child row (AC4)
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
        .filter(|c| matches!(c, StackCursor::OnCyclicNode { .. }))
        .count();
    assert_eq!(cyclic_count, 1);
    let deep_fields = flat
        .iter()
        .filter(|c| {
            matches!(
                c,
                StackCursor::OnObjectField {
                    field_path, ..
                } if field_path.len() > 1
            )
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
        .filter(|c| matches!(c, StackCursor::OnCyclicNode { .. }))
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
    // A(100) → B(200)
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
    // B(200) → A(100) (back-reference)
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
        .filter(|c| matches!(c, StackCursor::OnCyclicNode { .. }))
        .count();
    assert_eq!(cyclic_count, 1, "B's back-ref to A should be cyclic");
    // Should not recurse 16 levels deep
    let max_depth = flat
        .iter()
        .filter_map(|c| match c {
            StackCursor::OnObjectField { field_path, .. } => Some(field_path.len()),
            StackCursor::OnCyclicNode { field_path, .. } => Some(field_path.len()),
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
    // flat: Frame(0), Var{0,0}, Field[0](Int),
    //       CyclicNode[1](self-ref), Field[2](Int)
    state.move_down(); // Frame → Var
    state.move_down(); // Var → Field[0]
    assert!(matches!(
        &state.cursor,
        StackCursor::OnObjectField { field_path, .. }
        if *field_path == vec![0]
    ));
    state.move_down(); // Field[0] → CyclicNode[1]
    assert!(matches!(
        &state.cursor,
        StackCursor::OnCyclicNode { field_path, .. }
        if *field_path == vec![1]
    ));
    state.move_down(); // CyclicNode[1] → Field[2]
    assert!(matches!(
        &state.cursor,
        StackCursor::OnObjectField { field_path, .. }
        if *field_path == vec![2]
    ));
    // Now go back up
    state.move_up(); // Field[2] → CyclicNode[1]
    assert!(matches!(
        &state.cursor,
        StackCursor::OnCyclicNode { field_path, .. }
        if *field_path == vec![1]
    ));
    state.move_up(); // CyclicNode[1] → Field[0]
    assert!(matches!(
        &state.cursor,
        StackCursor::OnObjectField { field_path, .. }
        if *field_path == vec![0]
    ));
}

#[test]
fn flat_items_acyclic_tree_no_cyclic_nodes() {
    use hprof_engine::{FieldInfo, FieldValue};
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var_object_ref(0, 100)];
    state.toggle_expand(10, vars);
    // A(100) → B(200) → C(300), no cycles
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
        .filter(|c| matches!(c, StackCursor::OnCyclicNode { .. }))
        .count();
    assert_eq!(cyclic_count, 0, "acyclic tree must have zero cyclic nodes");
    // Should have fields at depths 1, 2, 3
    assert!(flat.contains(&StackCursor::OnObjectField {
        frame_idx: 0,
        var_idx: 0,
        field_path: vec![0],
    }));
    assert!(flat.contains(&StackCursor::OnObjectField {
        frame_idx: 0,
        var_idx: 0,
        field_path: vec![0, 0],
    }));
    assert!(flat.contains(&StackCursor::OnObjectField {
        frame_idx: 0,
        var_idx: 0,
        field_path: vec![0, 0, 0],
    }));
}

#[test]
fn flat_items_diamond_shared_object_no_false_positive() {
    use hprof_engine::{FieldInfo, FieldValue};
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var_object_ref(0, 100)];
    state.toggle_expand(10, vars);
    // A(100) has two fields both pointing to C(300)
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
    // C is shared but NOT an ancestor — no cyclic nodes
    let cyclic_count = flat
        .iter()
        .filter(|c| matches!(c, StackCursor::OnCyclicNode { .. }))
        .count();
    assert_eq!(
        cyclic_count, 0,
        "diamond/shared object must not be a false positive"
    );
    // C's field should appear under both left and right
    assert!(flat.contains(&StackCursor::OnObjectField {
        frame_idx: 0,
        var_idx: 0,
        field_path: vec![0, 0],
    }));
    assert!(flat.contains(&StackCursor::OnObjectField {
        frame_idx: 0,
        var_idx: 0,
        field_path: vec![1, 0],
    }));
}

#[test]
fn collapse_cyclic_child_resyncs_cursor_to_var() {
    use hprof_engine::{FieldInfo, FieldValue};
    let frames = vec![make_frame(10), make_frame(20)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var_object_ref(0, 1000)];
    state.toggle_expand(10, vars);
    // Thread(1000) → parkBlocker field → Coroutine(2000)
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
    // Coroutine(2000) → blockedThread → Thread(1000) (cycle)
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

    // Navigate to parkBlocker field (path [0])
    state.cursor = StackCursor::OnObjectField {
        frame_idx: 0,
        var_idx: 0,
        field_path: vec![0],
    };

    // Collapse the nested Coroutine object.
    // collect_descendants(2000) follows back-ref to 1000,
    // collapsing BOTH objects. Cursor becomes orphaned.
    state.collapse_object_recursive(2000);

    // Cursor must have been resynced — not stuck
    let flat = state.flat_items();
    assert!(
        flat.contains(&state.cursor),
        "cursor must be in flat_items after collapse, got: {:?}",
        state.cursor,
    );
    // Should have fallen back to OnVar
    assert!(
        matches!(
            &state.cursor,
            StackCursor::OnVar {
                frame_idx: 0,
                var_idx: 0,
            }
        ),
        "cursor should fall back to OnVar, got: {:?}",
        state.cursor,
    );

    // Navigation must work again
    state.move_down();
    assert_ne!(
        state.cursor,
        StackCursor::OnVar {
            frame_idx: 0,
            var_idx: 0,
        },
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
    // Thread(1000) → parkBlocker → Coroutine(2000)
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
    // Coroutine(2000) → blockedThread → Thread(1000)
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

    // Cursor on parkBlocker field
    state.cursor = StackCursor::OnObjectField {
        frame_idx: 0,
        var_idx: 0,
        field_path: vec![0],
    };

    // Non-recursive collapse (CollapseNestedObj path):
    // only collapses 2000, NOT 1000.
    state.collapse_object(2000);

    // Thread(1000) must still be expanded
    assert_eq!(
        state.expansion_state(1000),
        ExpansionPhase::Expanded,
        "parent object must remain expanded"
    );
    // Coroutine(2000) must be collapsed
    assert_eq!(state.expansion_state(2000), ExpansionPhase::Collapsed,);
    // Cursor stays on the parkBlocker field
    let flat = state.flat_items();
    assert!(flat.contains(&state.cursor), "cursor must still be valid");
    assert!(matches!(
        &state.cursor,
        StackCursor::OnObjectField {
            field_path, ..
        } if *field_path == vec![0]
    ));
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
    // 9 sections of 100 (100..999) + 2 of 1000
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

#[test]
fn page_down_jumps_by_visible_height() {
    // 30 frames, cursor at frame 5, height 20 → frame 25
    let frames: Vec<_> = (1..=30).map(make_frame).collect();
    let mut state = StackState::new(frames);
    state.set_visible_height(20);
    // Move cursor to frame 5
    for _ in 0..5 {
        state.move_down();
    }
    assert_eq!(state.cursor, StackCursor::OnFrame(5));
    state.move_page_down();
    assert_eq!(state.cursor, StackCursor::OnFrame(25));
}

#[test]
fn page_up_jumps_by_visible_height() {
    // 30 frames, cursor at frame 25, height 20 → frame 5
    let frames: Vec<_> = (1..=30).map(make_frame).collect();
    let mut state = StackState::new(frames);
    state.set_visible_height(20);
    for _ in 0..25 {
        state.move_down();
    }
    assert_eq!(state.cursor, StackCursor::OnFrame(25));
    state.move_page_up();
    assert_eq!(state.cursor, StackCursor::OnFrame(5));
}

#[test]
fn page_down_clamps_to_last_item() {
    let frames: Vec<_> = (1..=10).map(make_frame).collect();
    let mut state = StackState::new(frames);
    state.set_visible_height(20);
    state.move_page_down();
    assert_eq!(state.cursor, StackCursor::OnFrame(9));
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
    assert_eq!(state.cursor, StackCursor::OnFrame(0));
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

/// AC1 / AC3: Enter on Failed var is a no-op — cursor stays on OnVar, no child cursor.
#[test]
fn enter_on_failed_var_is_noop() {
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var_object_ref(0, 42)];
    state.toggle_expand(10, vars);
    state.set_expansion_failed(42, "object absent".to_string());
    assert_eq!(state.expansion_state(42), ExpansionPhase::Failed);
    let flat = state.flat_items();
    // Cursor can land on the Failed var (AC3).
    assert!(
        flat.contains(&StackCursor::OnVar {
            frame_idx: 0,
            var_idx: 0
        }),
        "Failed var must stay in flat_items: {flat:?}"
    );
    // No child cursor emitted for the Failed object (AC4).
    assert!(
        !flat
            .iter()
            .any(|c| matches!(c, StackCursor::OnObjectLoadingNode { .. })),
        "no loading node must appear for a Failed object"
    );
}

/// AC1 / AC3: Enter on Failed collection entry is a no-op.
///
/// Sets up: var → Expanded object → field (collection) → entry (ObjectRef Failed).
#[test]
fn enter_on_failed_collection_entry_is_noop() {
    use hprof_engine::{CollectionPage, EntryInfo, FieldInfo, FieldValue};
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    // Var → obj 99 (Expanded, has a collection field id=200)
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
    // Expand collection 200 with one entry whose value ObjectRef id=300 is Failed.
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
    state.collection_chunks.insert(
        200,
        CollectionChunks {
            total_count: 1,
            eager_page: Some(eager_page),
            chunk_pages: std::collections::HashMap::new(),
        },
    );
    state.set_expansion_failed(entry_obj_id, "not found".to_string());
    // expansion_state must stay Failed.
    assert_eq!(state.expansion_state(entry_obj_id), ExpansionPhase::Failed);
    let flat = state.flat_items();
    // OnCollectionEntry cursor must be present (AC3).
    assert!(
        flat.iter()
            .any(|c| matches!(c, StackCursor::OnCollectionEntry { entry_index: 0, .. })),
        "collection entry must remain in flat_items: {flat:?}"
    );
}

/// Navigation: a Failed collection entry object must not emit a phantom
/// cursor — flat_items and build_items must stay equal length and
/// move_down from the entry row must reach the next frame, not stall.
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
    state.collection_chunks.insert(
        200,
        CollectionChunks {
            total_count: 1,
            eager_page: Some(eager_page),
            chunk_pages: std::collections::HashMap::new(),
        },
    );
    state.set_expansion_failed(300, "not found".to_string());

    // AC5: no phantom cursor — lengths must be equal.
    assert_eq!(
        state.flat_items().len(),
        state.build_items().len(),
        "phantom OnCollectionEntryObjField must not be emitted for Failed"
    );

    // Navigate to the collection entry row, then Down must reach Frame(1).
    // flat: [Frame(0), Var{0,0}, ObjField{[0]}, CollEntry{0}, Frame(1)]
    state.move_down(); // → Var{0,0}
    state.move_down(); // → ObjField{[0]}
    state.move_down(); // → CollEntry{entry_index=0}
    assert!(
        matches!(
            state.cursor,
            StackCursor::OnCollectionEntry { entry_index: 0, .. }
        ),
        "expected OnCollectionEntry, got {:?}",
        state.cursor
    );
    state.move_down(); // must skip phantom and reach Frame(1)
    assert_eq!(
        state.cursor,
        StackCursor::OnFrame(1),
        "move_down from Failed collection entry must reach next frame"
    );
}

/// AC2 / AC4 / AC5: Failed var label uses stored error, no extra child row.
#[test]
fn failed_var_label_uses_stored_error_message() {
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var_object_ref(0, 99)];
    state.toggle_expand(10, vars);
    state.set_expansion_failed(99, "boom".to_string());
    let items = state.build_items();
    // items[0] = frame, items[1] = var — no orphan child row (AC4).
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

/// AC2: Failed var row uses THEME.error_indicator (Red fg).
#[test]
fn failed_var_style_is_error_indicator() {
    use ratatui::style::Color;
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    let vars = vec![make_var_object_ref(0, 99)];
    state.toggle_expand(10, vars);
    state.set_expansion_failed(99, "err".to_string());
    let items = state.build_items();
    // items[1] is the var row.
    // Layout: 2-space indent + 2-char toggle "! " + value text starting at col 4.
    let fg = rendered_fg_at(items[1].clone(), 4);
    assert_eq!(fg, Color::Red, "Failed var value must have Red fg");
}

/// AC5: flat_items().len() == build_items().len() across multiple configurations.
#[test]
fn flat_items_build_items_equal_length_invariant() {
    use hprof_engine::{FieldInfo, FieldValue};

    // (b) one frame collapsed
    {
        let state = StackState::new(vec![make_frame(1)]);
        assert_eq!(
            state.flat_items().len(),
            state.build_items().len(),
            "(b) collapsed"
        );
    }
    // (c) one frame expanded, var Failed
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
    // (d) one frame expanded, var Expanded with fields
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
            "(d) expanded with fields"
        );
    }
    // (f) two frames — one collapsed, one expanded with a Failed nested field
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
            "(f) nested Failed field"
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
    state.set_cursor(StackCursor::OnVar {
        frame_idx: 0,
        var_idx: 0,
    });
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
    state.set_cursor(StackCursor::OnVar {
        frame_idx: 0,
        var_idx: 0,
    });
    assert_eq!(state.selected_var_entry_count(), None);
}

#[test]
fn selected_var_entry_count_returns_none_when_cursor_not_on_var() {
    let frames = vec![make_frame(10)];
    let state = StackState::new(frames);
    // cursor is OnFrame(0)
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
    state.set_cursor(StackCursor::OnVar {
        frame_idx: 0,
        var_idx: 0,
    });
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
    state.collection_chunks.insert(
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
    state.set_cursor(StackCursor::OnCollectionEntry {
        collection_id: coll_id,
        entry_index: 0,
        frame_idx: 0,
        var_idx: 0,
        field_path: vec![],
    });
    assert_eq!(state.selected_collection_entry_count(), Some(3));
}

#[test]
fn selected_collection_entry_count_returns_none_when_entry_not_collection() {
    let coll_id = 0xC012u64;
    let frames = vec![make_frame(10)];
    let mut state = StackState::new(frames);
    state.toggle_expand(10, vec![make_var_object_ref(0, 0xA00)]);
    state.collection_chunks.insert(
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
    state.set_cursor(StackCursor::OnCollectionEntry {
        collection_id: coll_id,
        entry_index: 0,
        frame_idx: 0,
        var_idx: 0,
        field_path: vec![],
    });
    assert_eq!(state.selected_collection_entry_count(), None);
}

#[test]
fn selected_collection_entry_count_returns_none_when_cursor_not_on_entry() {
    let frames = vec![make_frame(10)];
    let state = StackState::new(frames);
    assert_eq!(state.selected_collection_entry_count(), None);
}
