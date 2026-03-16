//! Tests for variable-tree rendering across all sub-modules.

use std::collections::{HashMap, HashSet};

use hprof_engine::{EntryInfo, FieldInfo, FieldValue, VariableInfo, VariableValue};
use ratatui::{Terminal, backend::TestBackend, widgets::ListItem};

use super::helpers::split_object_id_range;
use super::{RenderOptions, TreeRoot, render_variable_tree};
use crate::views::stack_view::ExpansionPhase;

fn render_items(items: Vec<ListItem<'static>>) -> String {
    use ratatui::widgets::List;
    let backend = TestBackend::new(80, items.len().max(1) as u16);
    let mut terminal = Terminal::new(backend).unwrap();
    let count = items.len().max(1) as u16;
    terminal
        .draw(|f| {
            let area = ratatui::layout::Rect::new(0, 0, 80, count);
            let list = List::new(items);
            f.render_widget(list, area);
        })
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol().to_string())
        .collect()
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

/// Basic frame rendering: no locals, null vars,
/// collapsed/expanded object refs.
mod frame_rendering {
    use super::*;

    #[test]
    fn frame_with_no_vars_renders_no_locals() {
        let items = render_variable_tree(
            TreeRoot::Frame {
                vars: &[],
                frame_id: 100,
            },
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            RenderOptions {
                show_object_ids: false,
                snapshot_mode: false,
                show_hidden: false,
            },
            None,
            None,
        );
        let text = render_items(items);
        assert!(text.contains("(no locals)"), "got: {text:?}");
    }

    #[test]
    fn frame_with_null_var_renders_null() {
        let vars = vec![make_var(0, 0)];
        let items = render_variable_tree(
            TreeRoot::Frame {
                vars: &vars,
                frame_id: 100,
            },
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            RenderOptions {
                show_object_ids: false,
                snapshot_mode: false,
                show_hidden: false,
            },
            None,
            None,
        );
        let text = render_items(items);
        assert!(text.contains("[0] null"), "got: {text:?}");
    }

    #[test]
    fn frame_with_collapsed_object_ref_shows_plus() {
        let vars = vec![make_var(0, 42)];
        let items = render_variable_tree(
            TreeRoot::Frame {
                vars: &vars,
                frame_id: 100,
            },
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            RenderOptions {
                show_object_ids: false,
                snapshot_mode: false,
                show_hidden: false,
            },
            None,
            None,
        );
        let text = render_items(items);
        assert!(text.contains("+"), "expected + toggle, got: {text:?}");
        assert!(text.contains("[0]"), "expected var index, got: {text:?}");
    }

    #[test]
    fn frame_with_expanded_object_ref_shows_minus_and_fields() {
        let vars = vec![make_var(0, 42)];
        let mut object_fields = HashMap::new();
        object_fields.insert(
            42u64,
            vec![FieldInfo {
                name: "count".to_string(),
                value: FieldValue::Int(7),
            }],
        );
        let mut object_phases = HashMap::new();
        object_phases.insert(42u64, ExpansionPhase::Expanded);

        let items = render_variable_tree(
            TreeRoot::Frame {
                vars: &vars,
                frame_id: 100,
            },
            &object_fields,
            &HashMap::new(),
            &HashMap::new(),
            &object_phases,
            &HashMap::new(),
            RenderOptions {
                show_object_ids: false,
                snapshot_mode: false,
                show_hidden: false,
            },
            None,
            None,
        );
        let text = render_items(items);
        assert!(text.contains("-"), "expected - toggle, got: {text:?}");
        assert!(text.contains("count"), "expected field name, got: {text:?}");
        assert!(text.contains("7"), "expected field value, got: {text:?}");
    }
}

/// Snapshot mode behaviour: `?` toggle for unavailable refs,
/// `+` for loaded collections.
mod snapshot_mode {
    use super::*;

    #[test]
    fn snapshot_mode_unavailable_var_shows_question_toggle() {
        let vars = vec![make_var(0, 42)];
        let items = render_variable_tree(
            TreeRoot::Frame {
                vars: &vars,
                frame_id: 100,
            },
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            RenderOptions {
                show_object_ids: false,
                snapshot_mode: true,
                show_hidden: false,
            },
            None,
            None,
        );
        let text = render_items(items);
        assert!(text.contains("?"), "expected ? toggle, got: {text:?}");
        assert!(
            !text.contains("+ [0]"),
            "did not expect + toggle, got: {text:?}"
        );
    }

    #[test]
    fn snapshot_mode_collapsed_collection_shows_plus_not_question() {
        use crate::views::stack_view::CollectionChunks;

        let vars = vec![make_var(0, 1)];
        let mut object_fields = HashMap::new();
        object_fields.insert(
            1u64,
            vec![FieldInfo {
                name: "items".to_string(),
                value: FieldValue::ObjectRef {
                    id: 200,
                    class_name: "java.util.ArrayList".to_string(),
                    entry_count: Some(2),
                    inline_value: None,
                },
            }],
        );

        let mut collection_chunks = HashMap::new();
        collection_chunks.insert(
            200u64,
            CollectionChunks {
                total_count: 2,
                eager_page: Some(hprof_engine::CollectionPage {
                    entries: vec![EntryInfo {
                        index: 0,
                        key: None,
                        value: FieldValue::Int(7),
                    }],
                    total_count: 2,
                    offset: 0,
                    has_more: false,
                }),
                chunk_pages: HashMap::new(),
            },
        );

        // Parent expanded; collection id absent =>
        // collapsed in snapshot mode.
        let mut object_phases = HashMap::new();
        object_phases.insert(1u64, ExpansionPhase::Expanded);

        let items = render_variable_tree(
            TreeRoot::Frame {
                vars: &vars,
                frame_id: 100,
            },
            &object_fields,
            &HashMap::new(),
            &collection_chunks,
            &object_phases,
            &HashMap::new(),
            RenderOptions {
                show_object_ids: false,
                snapshot_mode: true,
                show_hidden: false,
            },
            None,
            None,
        );
        let text = render_items(items);

        assert!(
            text.contains("items: ArrayList"),
            "expected collection row, got: {text:?}"
        );
        assert!(text.contains("+ items"), "expected + marker, got: {text:?}");
        assert!(
            !text.contains("? items"),
            "must not show ? marker, got: {text:?}"
        );
        assert!(
            !text.contains("[0] 7"),
            "collapsed collection should hide entries"
        );
    }
}

/// Object display: id visibility toggle, cyclic-ref guard,
/// failed-var error label.
mod object_display {
    use super::*;

    #[test]
    fn nested_object_field_respects_object_id_toggle() {
        let vars = vec![make_var(0, 42)];
        let mut object_fields = HashMap::new();
        object_fields.insert(
            42u64,
            vec![FieldInfo {
                name: "child".to_string(),
                value: FieldValue::ObjectRef {
                    id: 77,
                    class_name: "com.example.Child".to_string(),
                    entry_count: None,
                    inline_value: None,
                },
            }],
        );
        let mut object_phases = HashMap::new();
        object_phases.insert(42u64, ExpansionPhase::Expanded);

        let with_ids = render_variable_tree(
            TreeRoot::Frame {
                vars: &vars,
                frame_id: 100,
            },
            &object_fields,
            &HashMap::new(),
            &HashMap::new(),
            &object_phases,
            &HashMap::new(),
            RenderOptions {
                show_object_ids: true,
                snapshot_mode: false,
                show_hidden: false,
            },
            None,
            None,
        );
        let with_ids_text = render_items(with_ids);
        assert!(
            with_ids_text.contains("Child @ 0x4D"),
            "expected nested object id in field row, \
             got: {with_ids_text:?}"
        );

        let without_ids = render_variable_tree(
            TreeRoot::Frame {
                vars: &vars,
                frame_id: 100,
            },
            &object_fields,
            &HashMap::new(),
            &HashMap::new(),
            &object_phases,
            &HashMap::new(),
            RenderOptions {
                show_object_ids: false,
                snapshot_mode: false,
                show_hidden: false,
            },
            None,
            None,
        );
        let without_ids_text = render_items(without_ids);
        assert!(
            !without_ids_text.contains("@ 0x4D"),
            "expected no nested object id when toggle \
             is off, got: {without_ids_text:?}"
        );
    }

    #[test]
    fn cyclic_object_ref_does_not_recurse_infinitely() {
        let vars = vec![make_var(0, 1)];
        let mut object_fields = HashMap::new();
        object_fields.insert(
            1u64,
            vec![FieldInfo {
                name: "self".to_string(),
                value: FieldValue::ObjectRef {
                    id: 1,
                    class_name: "Node".to_string(),
                    entry_count: None,
                    inline_value: None,
                },
            }],
        );
        let mut object_phases = HashMap::new();
        object_phases.insert(1u64, ExpansionPhase::Expanded);

        let items = render_variable_tree(
            TreeRoot::Frame {
                vars: &vars,
                frame_id: 100,
            },
            &object_fields,
            &HashMap::new(),
            &HashMap::new(),
            &object_phases,
            &HashMap::new(),
            RenderOptions {
                show_object_ids: false,
                snapshot_mode: false,
                show_hidden: false,
            },
            None,
            None,
        );
        let text = render_items(items);
        assert!(
            text.contains("self-ref") || text.contains("cyclic"),
            "expected cyclic marker, got: {text:?}"
        );
    }

    #[test]
    fn failed_var_label_uses_short_class_without_local_variable_prefix() {
        let vars = vec![make_var(0, 42)];
        let mut object_phases = HashMap::new();
        object_phases.insert(42u64, ExpansionPhase::Failed);
        let mut object_errors = HashMap::new();
        object_errors.insert(42u64, "boom".to_string());

        let items = render_variable_tree(
            TreeRoot::Frame {
                vars: &vars,
                frame_id: 100,
            },
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &object_phases,
            &object_errors,
            RenderOptions {
                show_object_ids: false,
                snapshot_mode: false,
                show_hidden: false,
            },
            None,
            None,
        );
        let text = render_items(items);
        assert!(text.contains("Object — boom"), "got: {text:?}");
        assert!(
            !text.contains("local variable:"),
            "failed label must not include local \
             variable prefix: {text:?}"
        );
    }
}

/// `TreeRoot::Subtree` rendering: field indentation,
/// collection entries, inline errors.
mod subtree_root {
    use super::*;

    #[test]
    fn subtree_root_renders_fields_at_two_space_indent() {
        let mut object_fields = HashMap::new();
        object_fields.insert(
            99u64,
            vec![FieldInfo {
                name: "x".to_string(),
                value: FieldValue::Int(42),
            }],
        );
        let mut object_phases = HashMap::new();
        object_phases.insert(99u64, ExpansionPhase::Expanded);

        let items = render_variable_tree(
            TreeRoot::Subtree { root_id: 99 },
            &object_fields,
            &HashMap::new(),
            &HashMap::new(),
            &object_phases,
            &HashMap::new(),
            RenderOptions {
                show_object_ids: false,
                snapshot_mode: false,
                show_hidden: false,
            },
            None,
            None,
        );
        let text = render_items(items);
        assert!(text.contains("x"), "expected field name, got: {text:?}");
        assert!(text.contains("42"), "expected field value, got: {text:?}");
    }

    #[test]
    fn subtree_root_collection_renders_entries_without_object_fields() {
        use crate::views::stack_view::{ChunkState, CollectionChunks};

        let mut chunks = HashMap::new();
        chunks.insert(
            77u64,
            CollectionChunks {
                total_count: 120,
                eager_page: Some(hprof_engine::CollectionPage {
                    entries: vec![EntryInfo {
                        index: 0,
                        key: None,
                        value: FieldValue::Int(7),
                    }],
                    total_count: 120,
                    offset: 0,
                    has_more: true,
                }),
                chunk_pages: HashMap::from([(100usize, ChunkState::Collapsed)]),
            },
        );

        let items = render_variable_tree(
            TreeRoot::Subtree { root_id: 77 },
            &HashMap::new(),
            &HashMap::new(),
            &chunks,
            &HashMap::new(),
            &HashMap::new(),
            RenderOptions {
                show_object_ids: false,
                snapshot_mode: true,
                show_hidden: false,
            },
            None,
            None,
        );
        let text = render_items(items);

        assert!(
            text.contains("[0] 7"),
            "expected eager entry, got: {text:?}"
        );
        // Unloaded chunk sentinels are hidden in snapshot
        // mode — snapshots are frozen and cannot fetch new
        // pages.
        assert!(
            !text.contains("[100...119]"),
            "unloaded chunk must be hidden in snapshot \
             mode: {text:?}"
        );
    }

    #[test]
    fn failed_collection_entry_shows_error_message_inline() {
        use crate::views::stack_view::{ChunkState, CollectionChunks};

        let vars = vec![make_var(0, 1)];
        let mut object_fields = HashMap::new();
        object_fields.insert(
            1u64,
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

        let mut collection_chunks = HashMap::new();
        collection_chunks.insert(
            200u64,
            CollectionChunks {
                total_count: 1,
                eager_page: Some(hprof_engine::CollectionPage {
                    entries: vec![EntryInfo {
                        index: 0,
                        key: None,
                        value: FieldValue::ObjectRef {
                            id: 300,
                            class_name: "java.lang.String".to_string(),
                            entry_count: None,
                            inline_value: None,
                        },
                    }],
                    total_count: 1,
                    offset: 0,
                    has_more: false,
                }),
                chunk_pages: HashMap::from([(100usize, ChunkState::Collapsed)]),
            },
        );

        let mut object_phases = HashMap::new();
        object_phases.insert(1u64, ExpansionPhase::Expanded);
        object_phases.insert(300u64, ExpansionPhase::Failed);

        let mut object_errors = HashMap::new();
        object_errors.insert(300u64, "entry missing".to_string());

        let items = render_variable_tree(
            TreeRoot::Frame {
                vars: &vars,
                frame_id: 100,
            },
            &object_fields,
            &HashMap::new(),
            &collection_chunks,
            &object_phases,
            &object_errors,
            RenderOptions {
                show_object_ids: false,
                snapshot_mode: false,
                show_hidden: false,
            },
            None,
            None,
        );
        let text = render_items(items);
        assert!(
            text.contains("! [0] String — entry missing"),
            "failed collection entry must include inline \
             error message, got: {text:?}"
        );
    }
}

/// Tests for hidden_fields overlay (Story 9.9).
mod hidden_fields_tests {
    use super::*;
    use crate::favorites::HideKey;

    fn empty_options() -> RenderOptions {
        RenderOptions {
            show_object_ids: false,
            snapshot_mode: true,
            show_hidden: false,
        }
    }

    #[test]
    fn render_variable_tree_hidden_var_shows_placeholder() {
        // show_hidden=true: hidden row renders as placeholder
        let vars = vec![VariableInfo {
            index: 0,
            value: VariableValue::Null,
        }];
        let mut hidden = HashSet::new();
        hidden.insert(HideKey::Var(0));
        let items = render_variable_tree(
            TreeRoot::Frame {
                vars: &vars,
                frame_id: 100,
            },
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            RenderOptions {
                show_object_ids: false,
                snapshot_mode: true,
                show_hidden: true,
            },
            Some(&hidden),
            None,
        );
        assert_eq!(items.len(), 1, "hidden var → 1 placeholder row");
        let text = render_items(items);
        assert!(text.contains("[hidden:"), "got: {text:?}");
    }

    #[test]
    fn render_variable_tree_hidden_var_absent_when_show_hidden_false() {
        // show_hidden=false (default): hidden row is
        // completely absent
        let vars = vec![VariableInfo {
            index: 0,
            value: VariableValue::Null,
        }];
        let mut hidden = HashSet::new();
        hidden.insert(HideKey::Var(0));
        let items = render_variable_tree(
            TreeRoot::Frame {
                vars: &vars,
                frame_id: 100,
            },
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            empty_options(),
            Some(&hidden),
            None,
        );
        assert_eq!(items.len(), 0, "hidden var with show_hidden=false → no row");
    }

    #[test]
    fn render_variable_tree_not_hidden_var_shows_normal() {
        let vars = vec![VariableInfo {
            index: 0,
            value: VariableValue::Null,
        }];
        let items = render_variable_tree(
            TreeRoot::Frame {
                vars: &vars,
                frame_id: 100,
            },
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            empty_options(),
            None,
            None,
        );
        let text = render_items(items);
        assert!(!text.contains("[hidden:"), "got: {text:?}");
    }

    #[test]
    fn render_variable_tree_hidden_field_suppresses_children() {
        let mut object_fields = HashMap::new();
        object_fields.insert(
            1u64,
            vec![FieldInfo {
                name: "child".to_string(),
                value: FieldValue::ObjectRef {
                    id: 2,
                    class_name: "Child".to_string(),
                    entry_count: None,
                    inline_value: None,
                },
            }],
        );
        object_fields.insert(
            2u64,
            vec![
                FieldInfo {
                    name: "x".to_string(),
                    value: FieldValue::Int(1),
                },
                FieldInfo {
                    name: "y".to_string(),
                    value: FieldValue::Int(2),
                },
            ],
        );
        let mut object_phases = HashMap::new();
        object_phases.insert(1u64, ExpansionPhase::Expanded);
        object_phases.insert(2u64, ExpansionPhase::Expanded);

        // Baseline: no hiding — 1 ObjectRef row +
        // 2 primitive child rows = 3
        let baseline = render_variable_tree(
            TreeRoot::Subtree { root_id: 1 },
            &object_fields,
            &HashMap::new(),
            &HashMap::new(),
            &object_phases,
            &HashMap::new(),
            empty_options(),
            None,
            None,
        );
        assert_eq!(baseline.len(), 3, "baseline: 1 field + 2 children");

        // With hide + show_hidden=true: 1 placeholder,
        // children suppressed
        let mut hidden = HashSet::new();
        hidden.insert(HideKey::Field {
            parent_id: 1,
            field_idx: 0,
        });
        let hidden_items = render_variable_tree(
            TreeRoot::Subtree { root_id: 1 },
            &object_fields,
            &HashMap::new(),
            &HashMap::new(),
            &object_phases,
            &HashMap::new(),
            RenderOptions {
                show_object_ids: false,
                snapshot_mode: true,
                show_hidden: true,
            },
            Some(&hidden),
            None,
        );
        assert_eq!(hidden_items.len(), 1, "hidden: only 1 placeholder row");
        let text = render_items(hidden_items);
        assert!(text.contains("[hidden:"), "got: {text:?}");

        // With hide + show_hidden=false: field and children
        // completely absent
        let hidden_absent = render_variable_tree(
            TreeRoot::Subtree { root_id: 1 },
            &object_fields,
            &HashMap::new(),
            &HashMap::new(),
            &object_phases,
            &HashMap::new(),
            empty_options(),
            Some(&hidden),
            None,
        );
        assert_eq!(hidden_absent.len(), 0, "show_hidden=false → no rows");
    }
}

/// Empty collections (entry_count == 0) render as leaf
/// without toggle.
mod empty_collections {
    use super::*;

    #[test]
    fn empty_collection_var_renders_without_toggle() {
        let vars = vec![VariableInfo {
            index: 0,
            value: VariableValue::ObjectRef {
                id: 0x50,
                class_name: "Object[]".to_string(),
                entry_count: Some(0),
            },
        }];
        let items = render_variable_tree(
            TreeRoot::Frame {
                vars: &vars,
                frame_id: 100,
            },
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            RenderOptions {
                show_object_ids: false,
                snapshot_mode: false,
                show_hidden: false,
            },
            None,
            None,
        );
        let text = render_items(items);
        assert!(
            text.contains("(empty)"),
            "expected '(empty)' label, got: {text:?}"
        );
        assert!(
            !text.contains('+'),
            "empty collection must not show + toggle: \
             {text:?}"
        );
    }

    #[test]
    fn empty_collection_field_renders_without_toggle() {
        let vars = vec![make_var(0, 42)];
        let mut object_fields = HashMap::new();
        object_fields.insert(
            42u64,
            vec![FieldInfo {
                name: "items".to_string(),
                value: FieldValue::ObjectRef {
                    id: 0x99,
                    class_name: "java.util.ArrayList".to_string(),
                    entry_count: Some(0),
                    inline_value: None,
                },
            }],
        );
        let mut object_phases = HashMap::new();
        object_phases.insert(42u64, ExpansionPhase::Expanded);

        let items = render_variable_tree(
            TreeRoot::Frame {
                vars: &vars,
                frame_id: 100,
            },
            &object_fields,
            &HashMap::new(),
            &HashMap::new(),
            &object_phases,
            &HashMap::new(),
            RenderOptions {
                show_object_ids: false,
                snapshot_mode: false,
                show_hidden: false,
            },
            None,
            None,
        );
        let text = render_items(items);
        assert!(
            text.contains("(empty)"),
            "expected '(empty)' label on field, got: \
             {text:?}"
        );
        assert!(
            !text.contains('+'),
            "empty collection field must not show + \
             toggle: {text:?}"
        );
    }
}

/// Unit tests for low-level helper functions.
mod helpers {
    use super::*;

    #[test]
    fn split_object_id_range_handles_inline_value_suffix() {
        let text = "Node @ 0x2A = \"abc\"";
        let (start, end) = split_object_id_range(text).unwrap();
        assert_eq!(&text[start..end], "@ 0x2A");
    }
}
