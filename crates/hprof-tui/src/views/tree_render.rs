//! Shared variable-tree rendering for [`StackView`] and [`FavoritesPanel`].
//!
//! [`render_variable_tree`] produces a flat `Vec<ListItem>` from a tree root
//! without embedding cursor-selection styling — callers apply selection via
//! ratatui's `List::highlight_style` + `ListState`.

use std::collections::{HashMap, HashSet};

use hprof_engine::{EntryInfo, FieldInfo, FieldValue, VariableInfo, VariableValue};
use ratatui::{
    style::Style,
    text::{Line, Span},
    widgets::ListItem,
};

use crate::theme::THEME;

use super::stack_view::{
    ChunkState, CollectionChunks, ExpansionPhase, FAILED_LABEL_SEP, StackState,
    compute_chunk_ranges, field_value_style, format_field_value_display,
};

/// Root of a variable tree to render.
pub(crate) enum TreeRoot<'a> {
    /// Render all variables of a stack frame.
    Frame { vars: &'a [VariableInfo] },
    /// Render the expanded fields of a single object.
    Subtree { root_id: u64 },
}

/// Renders a variable tree into a flat list of styled items.
///
/// No cursor is embedded — selection is handled by the caller via
/// `List::highlight_style` + `ListState`.
///
/// - `TreeRoot::Frame` produces the same item order as `flat_items()`'s
///   variable section, so `ListState` offsets remain correct for `StackView`.
/// - `TreeRoot::Subtree` starts at indent `"  "` (two spaces) for use in
///   `FavoritesPanel`.
pub(crate) fn render_variable_tree(
    root: TreeRoot<'_>,
    object_fields: &HashMap<u64, Vec<FieldInfo>>,
    collection_chunks: &HashMap<u64, CollectionChunks>,
    object_phases: &HashMap<u64, ExpansionPhase>,
    object_errors: &HashMap<u64, String>,
) -> Vec<ListItem<'static>> {
    let mut items = Vec::new();
    match root {
        TreeRoot::Frame { vars } => {
            if vars.is_empty() {
                items.push(ListItem::new(Line::from(Span::styled(
                    "  (no locals)",
                    THEME.null_value,
                ))));
            } else {
                for var in vars {
                    append_var(
                        var,
                        "  ",
                        object_fields,
                        collection_chunks,
                        object_phases,
                        object_errors,
                        &mut items,
                    );
                }
            }
        }
        TreeRoot::Subtree { root_id } => {
            let mut visited = HashSet::new();
            append_object_children(
                root_id,
                "  ",
                0,
                object_fields,
                collection_chunks,
                object_phases,
                object_errors,
                &mut visited,
                &mut items,
            );
        }
    }
    items
}

fn get_phase(object_id: u64, object_phases: &HashMap<u64, ExpansionPhase>) -> ExpansionPhase {
    object_phases
        .get(&object_id)
        .cloned()
        .unwrap_or(ExpansionPhase::Collapsed)
}

fn append_var(
    var: &VariableInfo,
    indent: &str,
    object_fields: &HashMap<u64, Vec<FieldInfo>>,
    collection_chunks: &HashMap<u64, CollectionChunks>,
    object_phases: &HashMap<u64, ExpansionPhase>,
    object_errors: &HashMap<u64, String>,
    items: &mut Vec<ListItem<'static>>,
) {
    let phase = if let VariableValue::ObjectRef { id, .. } = var.value {
        get_phase(id, object_phases)
    } else {
        ExpansionPhase::Collapsed
    };

    let (toggle, val_str, val_style): (&str, String, Style) = match (&var.value, &phase) {
        (VariableValue::Null, _) => ("  ", "null".to_string(), Style::new()),
        (VariableValue::ObjectRef { id, class_name, .. }, ExpansionPhase::Failed) => {
            let short = if class_name.is_empty() {
                "Object"
            } else {
                class_name.rsplit('.').next().unwrap_or(class_name)
            };
            let err = object_errors
                .get(id)
                .map(|s| s.as_str())
                .unwrap_or("Failed to resolve object");
            let label = format!("{short}{FAILED_LABEL_SEP}{err}");
            ("! ", label, THEME.error_indicator)
        }
        (VariableValue::ObjectRef { class_name, .. }, ExpansionPhase::Collapsed) => {
            ("+ ", format!("local variable: {class_name}"), Style::new())
        }
        (VariableValue::ObjectRef { class_name, .. }, _) => {
            ("- ", format!("local variable: {class_name}"), Style::new())
        }
    };

    let toggle_style = if toggle.trim().is_empty() {
        Style::new()
    } else {
        THEME.expand_indicator
    };

    items.push(ListItem::new(Line::from(vec![
        Span::raw(indent.to_string()),
        Span::styled(toggle.to_string(), toggle_style),
        Span::styled(format!("[{}] {val_str}", var.index), val_style),
    ])));

    if let VariableValue::ObjectRef { id, .. } = var.value {
        let mut visited = HashSet::new();
        append_object_children(
            id,
            &format!("{indent}  "),
            0,
            object_fields,
            collection_chunks,
            object_phases,
            object_errors,
            &mut visited,
            items,
        );
    }
}

/// Appends items for `object_id`'s children at the given `indent`.
///
/// `depth` is used only for the recursion guard (max 16 levels).
#[allow(clippy::too_many_arguments)]
fn append_object_children(
    object_id: u64,
    indent: &str,
    depth: usize,
    object_fields: &HashMap<u64, Vec<FieldInfo>>,
    collection_chunks: &HashMap<u64, CollectionChunks>,
    object_phases: &HashMap<u64, ExpansionPhase>,
    object_errors: &HashMap<u64, String>,
    visited: &mut HashSet<u64>,
    items: &mut Vec<ListItem<'static>>,
) {
    if depth >= 16 {
        return;
    }
    match get_phase(object_id, object_phases) {
        ExpansionPhase::Collapsed => {}
        ExpansionPhase::Loading => {
            items.push(ListItem::new(Line::from(Span::styled(
                format!("{indent}~ Loading..."),
                THEME.loading_indicator,
            ))));
        }
        ExpansionPhase::Expanded => {
            visited.insert(object_id);
            let field_list = object_fields
                .get(&object_id)
                .map(|f| f.as_slice())
                .unwrap_or(&[]);
            if field_list.is_empty() {
                items.push(ListItem::new(Line::from(Span::styled(
                    format!("{indent}(no fields)"),
                    THEME.null_value,
                ))));
            } else {
                for field in field_list {
                    if let FieldValue::ObjectRef { id, class_name, .. } = &field.value
                        && visited.contains(id)
                    {
                        let label = if *id == object_id {
                            "self-ref"
                        } else {
                            "cyclic"
                        };
                        let short = class_name.rsplit('.').next().unwrap_or(class_name);
                        let text = format!(
                            "{indent}  {}: \u{21BB} {} @ 0x{:X} [{label}]",
                            field.name, short, id
                        );
                        items.push(ListItem::new(Line::from(Span::styled(
                            text,
                            THEME.null_value,
                        ))));
                        continue;
                    }

                    let child_phase = if let FieldValue::ObjectRef { id, .. } = field.value {
                        Some(get_phase(id, object_phases))
                    } else {
                        None
                    };
                    let row_style = if matches!(child_phase, Some(ExpansionPhase::Failed)) {
                        THEME.error_indicator
                    } else {
                        field_value_style(&field.value)
                    };
                    let val = if let (
                        FieldValue::ObjectRef { id, class_name, .. },
                        Some(ExpansionPhase::Failed),
                    ) = (&field.value, &child_phase)
                    {
                        let err = object_errors
                            .get(id)
                            .map(|s| s.as_str())
                            .unwrap_or("Failed to resolve object");
                        let short = if class_name.is_empty() {
                            "Object"
                        } else {
                            class_name.rsplit('.').next().unwrap_or(class_name)
                        };
                        format!("{short}{FAILED_LABEL_SEP}{err}")
                    } else {
                        format_field_value_display(&field.value, child_phase.as_ref())
                    };
                    let toggle = match &child_phase {
                        Some(ExpansionPhase::Expanded) | Some(ExpansionPhase::Loading) => "- ",
                        Some(ExpansionPhase::Failed) => "! ",
                        Some(ExpansionPhase::Collapsed) => "+ ",
                        None => "  ",
                    };
                    let toggle_style = if toggle.trim().is_empty() {
                        row_style
                    } else {
                        THEME.expand_indicator
                    };
                    items.push(ListItem::new(Line::from(vec![
                        Span::raw(indent.to_string()),
                        Span::styled(toggle.to_string(), toggle_style),
                        Span::styled(format!("{}: {val}", field.name), row_style),
                    ])));

                    if let FieldValue::ObjectRef {
                        id,
                        entry_count: Some(_),
                        ..
                    } = field.value
                        && let Some(cc) = collection_chunks.get(&id)
                    {
                        append_collection_items(
                            cc,
                            &format!("{indent}  "),
                            object_fields,
                            collection_chunks,
                            object_phases,
                            object_errors,
                            items,
                        );
                        continue;
                    }
                    if let FieldValue::ObjectRef { id, .. } = field.value {
                        append_object_children(
                            id,
                            &format!("{indent}  "),
                            depth + 1,
                            object_fields,
                            collection_chunks,
                            object_phases,
                            object_errors,
                            visited,
                            items,
                        );
                    }
                }
            }
            visited.remove(&object_id);
        }
        ExpansionPhase::Failed => {
            // Error state is styled on the parent node — no child row emitted here.
        }
    }
}

/// Appends items for collection entries and chunk section placeholders.
#[allow(clippy::too_many_arguments)]
fn append_collection_items(
    cc: &CollectionChunks,
    indent: &str,
    object_fields: &HashMap<u64, Vec<FieldInfo>>,
    collection_chunks: &HashMap<u64, CollectionChunks>,
    object_phases: &HashMap<u64, ExpansionPhase>,
    object_errors: &HashMap<u64, String>,
    items: &mut Vec<ListItem<'static>>,
) {
    let entry_items = |entry: &EntryInfo, items: &mut Vec<ListItem<'static>>| {
        let value_phase = if let FieldValue::ObjectRef { id, .. } = &entry.value {
            Some(get_phase(*id, object_phases))
        } else {
            None
        };
        let text = if let (
            FieldValue::ObjectRef { id, class_name, .. },
            Some(ExpansionPhase::Failed),
        ) = (&entry.value, &value_phase)
        {
            let err = object_errors
                .get(id)
                .map(|s| s.as_str())
                .unwrap_or("Failed to resolve object");
            let short = if class_name.is_empty() {
                "Object"
            } else {
                class_name.rsplit('.').next().unwrap_or(class_name)
            };
            if let Some(key) = &entry.key {
                let k = super::stack_view::format_entry_value_text(key);
                format!(
                    "{indent}! [{}] {} => {}{FAILED_LABEL_SEP}{err}",
                    entry.index, k, short
                )
            } else {
                format!("{indent}! [{}] {}{FAILED_LABEL_SEP}{err}", entry.index, short)
            }
        } else {
            StackState::format_entry_line(entry, indent, value_phase.as_ref())
        };
        let row_style = if matches!(value_phase, Some(ExpansionPhase::Failed)) {
            THEME.error_indicator
        } else {
            field_value_style(&entry.value)
        };
        items.push(ListItem::new(Line::from(Span::styled(text, row_style))));
        if let FieldValue::ObjectRef { id, .. } = &entry.value {
            let mut visited = HashSet::new();
            append_collection_entry_obj(
                *id,
                &format!("{indent}  "),
                0,
                object_fields,
                collection_chunks,
                object_phases,
                object_errors,
                &mut visited,
                items,
            );
        }
    };

    if let Some(page) = &cc.eager_page {
        for entry in &page.entries {
            entry_items(entry, items);
        }
    }
    let ranges = compute_chunk_ranges(cc.total_count);
    for (offset, limit) in &ranges {
        let end = offset + limit - 1;
        let chunk_state = cc.chunk_pages.get(offset);
        let (toggle, label) = match chunk_state {
            Some(ChunkState::Loading) => ("~ ", format!("Loading [{offset}...{end}]")),
            Some(ChunkState::Loaded(_)) => ("- ", format!("[{offset}...{end}]")),
            _ => ("+ ", format!("[{offset}...{end}]")),
        };
        let text = format!("{indent}{toggle}{label}");
        let row_style = if matches!(chunk_state, Some(ChunkState::Loading)) {
            THEME.loading_indicator
        } else {
            THEME.expand_indicator
        };
        items.push(ListItem::new(Line::from(Span::styled(text, row_style))));
        if let Some(ChunkState::Loaded(page)) = chunk_state {
            for entry in &page.entries {
                entry_items(entry, items);
            }
        }
    }
}

/// Appends items for an object expanded from a collection entry value.
///
/// `depth` used for the recursion guard (max 16 levels).
#[allow(clippy::too_many_arguments)]
fn append_collection_entry_obj(
    obj_id: u64,
    indent: &str,
    depth: usize,
    object_fields: &HashMap<u64, Vec<FieldInfo>>,
    _collection_chunks: &HashMap<u64, CollectionChunks>,
    object_phases: &HashMap<u64, ExpansionPhase>,
    object_errors: &HashMap<u64, String>,
    visited: &mut HashSet<u64>,
    items: &mut Vec<ListItem<'static>>,
) {
    if depth >= 16 {
        return;
    }
    match get_phase(obj_id, object_phases) {
        ExpansionPhase::Collapsed => {}
        ExpansionPhase::Loading => {
            items.push(ListItem::new(Line::from(Span::styled(
                format!("{indent}~ Loading..."),
                THEME.loading_indicator,
            ))));
        }
        ExpansionPhase::Expanded => {
            let field_list = object_fields
                .get(&obj_id)
                .map(|f| f.as_slice())
                .unwrap_or(&[]);
            if field_list.is_empty() {
                items.push(ListItem::new(Line::from(Span::styled(
                    format!("{indent}(no fields)"),
                    THEME.null_value,
                ))));
            } else {
                visited.insert(obj_id);
                for (fidx, field) in field_list.iter().enumerate() {
                    if let FieldValue::ObjectRef { id, .. } = &field.value
                        && visited.contains(id)
                    {
                        continue; // cyclic — skip (non-navigable leaf)
                    }
                    let child_phase = if let FieldValue::ObjectRef { id, .. } = field.value {
                        Some(get_phase(id, object_phases))
                    } else {
                        None
                    };
                    let row_style = if matches!(child_phase, Some(ExpansionPhase::Failed)) {
                        THEME.error_indicator
                    } else {
                        field_value_style(&field.value)
                    };
                    let val = if let (
                        FieldValue::ObjectRef { id, class_name, .. },
                        Some(ExpansionPhase::Failed),
                    ) = (&field.value, &child_phase)
                    {
                        let err = object_errors
                            .get(id)
                            .map(|s| s.as_str())
                            .unwrap_or("Failed to resolve object");
                        let short = if class_name.is_empty() {
                            "Object"
                        } else {
                            class_name.rsplit('.').next().unwrap_or(class_name)
                        };
                        format!("{short}{FAILED_LABEL_SEP}{err}")
                    } else {
                        format_field_value_display(&field.value, child_phase.as_ref())
                    };
                    let toggle = match &child_phase {
                        Some(ExpansionPhase::Expanded) | Some(ExpansionPhase::Loading) => "- ",
                        Some(ExpansionPhase::Failed) => "! ",
                        Some(ExpansionPhase::Collapsed) => "+ ",
                        None => "  ",
                    };
                    let toggle_style = if toggle.trim().is_empty() {
                        row_style
                    } else {
                        THEME.expand_indicator
                    };
                    items.push(ListItem::new(Line::from(vec![
                        Span::raw(indent.to_string()),
                        Span::styled(toggle.to_string(), toggle_style),
                        Span::styled(format!("{}: {val}", field.name), row_style),
                    ])));
                    if let FieldValue::ObjectRef { id, .. } = field.value {
                        let _ = fidx; // suppress unused warning
                        append_collection_entry_obj(
                            id,
                            &format!("{indent}  "),
                            depth + 1,
                            object_fields,
                            _collection_chunks,
                            object_phases,
                            object_errors,
                            visited,
                            items,
                        );
                    }
                }
                visited.remove(&obj_id);
            }
        }
        ExpansionPhase::Failed => {
            // Error state is styled on the parent node — no child row emitted here.
        }
    }
}

#[cfg(test)]
mod tests {
    use hprof_engine::{FieldInfo, FieldValue, VariableInfo, VariableValue};
    use ratatui::{Terminal, backend::TestBackend};
    use std::collections::HashMap;

    use super::*;
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
                }
            },
        }
    }

    #[test]
    fn frame_with_no_vars_renders_no_locals() {
        let items = render_variable_tree(
            TreeRoot::Frame { vars: &[] },
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );
        let text = render_items(items);
        assert!(text.contains("(no locals)"), "got: {text:?}");
    }

    #[test]
    fn frame_with_null_var_renders_null() {
        let vars = vec![make_var(0, 0)];
        let items = render_variable_tree(
            TreeRoot::Frame { vars: &vars },
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );
        let text = render_items(items);
        assert!(text.contains("[0] null"), "got: {text:?}");
    }

    #[test]
    fn frame_with_collapsed_object_ref_shows_plus() {
        let vars = vec![make_var(0, 42)];
        let items = render_variable_tree(
            TreeRoot::Frame { vars: &vars },
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );
        let text = render_items(items);
        assert!(text.contains("+"), "expected + toggle, got: {text:?}");
        assert!(text.contains("[0]"), "expected var index, got: {text:?}");
    }

    #[test]
    fn failed_var_label_uses_short_class_without_local_variable_prefix() {
        let vars = vec![make_var(0, 42)];
        let mut object_phases = HashMap::new();
        object_phases.insert(42u64, ExpansionPhase::Failed);
        let mut object_errors = HashMap::new();
        object_errors.insert(42u64, "boom".to_string());

        let items = render_variable_tree(
            TreeRoot::Frame { vars: &vars },
            &HashMap::new(),
            &HashMap::new(),
            &object_phases,
            &object_errors,
        );
        let text = render_items(items);
        assert!(text.contains("Object — boom"), "got: {text:?}");
        assert!(
            !text.contains("local variable:"),
            "failed label must not include local variable prefix: {text:?}"
        );
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
            TreeRoot::Frame { vars: &vars },
            &object_fields,
            &HashMap::new(),
            &object_phases,
            &HashMap::new(),
        );
        let text = render_items(items);
        assert!(text.contains("-"), "expected - toggle, got: {text:?}");
        assert!(text.contains("count"), "expected field name, got: {text:?}");
        assert!(text.contains("7"), "expected field value, got: {text:?}");
    }

    #[test]
    fn cyclic_object_ref_does_not_recurse_infinitely() {
        let vars = vec![make_var(0, 1)];
        let mut object_fields = HashMap::new();
        // Object 1 has a field that references itself
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
            TreeRoot::Frame { vars: &vars },
            &object_fields,
            &HashMap::new(),
            &object_phases,
            &HashMap::new(),
        );
        let text = render_items(items);
        // Should render without panicking and show the cyclic marker
        assert!(
            text.contains("self-ref") || text.contains("cyclic"),
            "expected cyclic marker, got: {text:?}"
        );
    }

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
            &object_phases,
            &HashMap::new(),
        );
        let text = render_items(items);
        assert!(text.contains("x"), "expected field name, got: {text:?}");
        assert!(text.contains("42"), "expected field value, got: {text:?}");
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
            TreeRoot::Frame { vars: &vars },
            &object_fields,
            &collection_chunks,
            &object_phases,
            &object_errors,
        );
        let text = render_items(items);
        assert!(
            text.contains("! [0] String — entry missing"),
            "failed collection entry must include inline error message, got: {text:?}"
        );
    }
}
