//! Shared variable-tree rendering for [`StackView`] and [`FavoritesPanel`].
//!
//! [`render_variable_tree`] produces a flat `Vec<ListItem>` from a tree root
//! without embedding cursor-selection styling — callers apply selection via
//! ratatui's `List::highlight_style` + `ListState`.

use std::collections::{HashMap, HashSet};

use hprof_engine::{EntryInfo, FieldInfo, FieldValue, VariableInfo, VariableValue};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::ListItem,
};

use crate::theme::THEME;

use super::stack_view::{
    ChunkState, CollectionChunks, ExpansionPhase, FAILED_LABEL_SEP, StackState,
    compute_chunk_ranges, field_value_style, format_entry_value_text, format_field_value_display,
    format_object_ref_collapsed,
};

/// Root of a variable tree to render.
pub(crate) enum TreeRoot<'a> {
    /// Render all variables of a stack frame.
    Frame { vars: &'a [VariableInfo] },
    /// Render the expanded fields of a single object.
    Subtree { root_id: u64 },
}

/// Rendering options that vary per panel.
pub(crate) struct RenderOptions {
    /// Whether object IDs should be shown in rendered labels.
    pub show_object_ids: bool,
    /// Whether rows without captured snapshot descendants should be
    /// marked as unavailable (`?`) instead of collapsed (`+`).
    pub snapshot_mode: bool,
}

/// Shared read-only context threaded through all render helpers.
struct RenderCtx<'a> {
    object_fields: &'a HashMap<u64, Vec<FieldInfo>>,
    object_static_fields: &'a HashMap<u64, Vec<FieldInfo>>,
    collection_chunks: &'a HashMap<u64, CollectionChunks>,
    object_phases: &'a HashMap<u64, ExpansionPhase>,
    object_errors: &'a HashMap<u64, String>,
    show_object_ids: bool,
    snapshot_mode: bool,
}

/// Renders a variable tree into a flat list of styled items.
///
/// No cursor is embedded — selection is handled by the caller via
/// `List::highlight_style` + `ListState`.
///
/// - `TreeRoot::Frame` produces the same item order as `flat_items()`'s
///   variable section, so `ListState` offsets remain correct for
///   `StackView`.
/// - `TreeRoot::Subtree` starts at indent `"  "` (two spaces) for use
///   in `FavoritesPanel`.
pub(crate) fn render_variable_tree(
    root: TreeRoot<'_>,
    object_fields: &HashMap<u64, Vec<FieldInfo>>,
    object_static_fields: &HashMap<u64, Vec<FieldInfo>>,
    collection_chunks: &HashMap<u64, CollectionChunks>,
    object_phases: &HashMap<u64, ExpansionPhase>,
    object_errors: &HashMap<u64, String>,
    options: RenderOptions,
) -> Vec<ListItem<'static>> {
    let ctx = RenderCtx {
        object_fields,
        object_static_fields,
        collection_chunks,
        object_phases,
        object_errors,
        show_object_ids: options.show_object_ids,
        snapshot_mode: options.snapshot_mode,
    };
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
                    append_var(var, "  ", &ctx, &mut items);
                }
            }
        }
        TreeRoot::Subtree { root_id } => {
            if let Some(chunks) = ctx.collection_chunks.get(&root_id) {
                append_collection_items(root_id, chunks, "  ", 0, &ctx, &mut items);
            } else {
                let mut visited = HashSet::new();
                append_fields_expanded(root_id, "  ", 0, &ctx, &mut visited, &mut items, false);
            }
        }
    }
    items
}

// ── low-level helpers ──────────────────────────────────────────────

fn object_ref_state(
    object_id: u64,
    entry_count: Option<u64>,
    ctx: &RenderCtx<'_>,
) -> (Option<ExpansionPhase>, bool) {
    if entry_count.is_some() && ctx.collection_chunks.contains_key(&object_id) {
        if ctx.snapshot_mode {
            return (Some(get_phase(object_id, ctx.object_phases)), false);
        }
        return (Some(ExpansionPhase::Expanded), false);
    }

    let has_snapshot_data = ctx.object_fields.contains_key(&object_id)
        || ctx.object_static_fields.contains_key(&object_id)
        || ctx.object_phases.contains_key(&object_id);

    if has_snapshot_data || !ctx.snapshot_mode {
        (Some(get_phase(object_id, ctx.object_phases)), false)
    } else {
        (None, true)
    }
}

fn get_phase(object_id: u64, object_phases: &HashMap<u64, ExpansionPhase>) -> ExpansionPhase {
    object_phases
        .get(&object_id)
        .cloned()
        .unwrap_or(ExpansionPhase::Collapsed)
}

fn split_object_id_range(text: &str) -> Option<(usize, usize)> {
    let start = text.find(" @ 0x")? + 1;
    let end = text[start..]
        .find(" = ")
        .map(|offset| start + offset)
        .unwrap_or(text.len());
    if end <= start + 5 {
        None
    } else {
        Some((start, end))
    }
}

fn spans_with_dimmed_object_id(text: String, base_style: Style) -> Vec<Span<'static>> {
    if let Some((start, end)) = split_object_id_range(&text) {
        let id_style = THEME.object_id_hint.add_modifier(Modifier::DIM);
        vec![
            Span::styled(text[..start].to_string(), base_style),
            Span::styled(text[start..end].to_string(), id_style),
            Span::styled(text[end..].to_string(), base_style),
        ]
    } else {
        vec![Span::styled(text, base_style)]
    }
}

/// Formats a failed object label as `ShortClass — error message`.
fn format_failed_label(class_name: &str, err: &str) -> String {
    let short = if class_name.is_empty() {
        "Object"
    } else {
        class_name.rsplit('.').next().unwrap_or(class_name)
    };
    format!("{short}{FAILED_LABEL_SEP}{err}")
}

/// Appends a single field row with toggle indicator and dimmed
/// object ID.
fn push_field_row(
    items: &mut Vec<ListItem<'static>>,
    indent: &str,
    toggle: &str,
    label: String,
    row_style: Style,
) {
    let toggle_style = if toggle.trim().is_empty() {
        row_style
    } else {
        THEME.expand_indicator
    };
    let mut row_spans = vec![
        Span::raw(indent.to_string()),
        Span::styled(toggle.to_string(), toggle_style),
    ];
    row_spans.extend(spans_with_dimmed_object_id(label, row_style));
    items.push(ListItem::new(Line::from(row_spans)));
}

/// Computes phase / style / toggle for a single `FieldInfo`, pushes
/// the rendered row, and returns `(child_phase, child_unavailable)`
/// so the caller can decide whether to recurse.
///
/// When `static_ctx` is true (static-field context), the
/// "unavailable" flag is suppressed — static fields are always fully
/// present when the class is loaded.
fn render_single_field(
    field: &FieldInfo,
    indent: &str,
    ctx: &RenderCtx<'_>,
    items: &mut Vec<ListItem<'static>>,
    static_ctx: bool,
) -> (Option<ExpansionPhase>, bool) {
    let (child_phase, child_unavailable) = if let FieldValue::ObjectRef {
        id, entry_count, ..
    } = &field.value
    {
        let (phase, unavail) = object_ref_state(*id, *entry_count, ctx);
        if static_ctx && unavail {
            (Some(get_phase(*id, ctx.object_phases)), false)
        } else {
            (phase, unavail)
        }
    } else {
        (None, false)
    };

    let row_style = if child_unavailable {
        THEME.null_value
    } else if matches!(child_phase, Some(ExpansionPhase::Failed)) {
        THEME.error_indicator
    } else {
        field_value_style(&field.value)
    };

    let val = if let (FieldValue::ObjectRef { id, class_name, .. }, Some(ExpansionPhase::Failed)) =
        (&field.value, &child_phase)
    {
        let err = ctx
            .object_errors
            .get(id)
            .map(|s| s.as_str())
            .unwrap_or("Failed to resolve object");
        format_failed_label(class_name, err)
    } else {
        format_field_value_display(&field.value, child_phase.as_ref(), ctx.show_object_ids)
    };

    let toggle = match (&child_phase, child_unavailable) {
        (_, true) => "? ",
        (Some(ExpansionPhase::Expanded), false) | (Some(ExpansionPhase::Loading), false) => "- ",
        (Some(ExpansionPhase::Failed), false) => "! ",
        (Some(ExpansionPhase::Collapsed), false) => "+ ",
        (None, false) => "  ",
    };

    push_field_row(
        items,
        indent,
        toggle,
        format!("{}: {val}", field.name),
        row_style,
    );

    (child_phase, child_unavailable)
}

// ── variable / field tree builders ─────────────────────────────────

fn append_var(
    var: &VariableInfo,
    indent: &str,
    ctx: &RenderCtx<'_>,
    items: &mut Vec<ListItem<'static>>,
) {
    let VariableValue::ObjectRef {
        id,
        class_name,
        entry_count,
    } = &var.value
    else {
        push_field_row(
            items,
            indent,
            "  ",
            format!("[{}] null", var.index),
            Style::new(),
        );
        return;
    };

    let (phase, unavailable) = object_ref_state(*id, *entry_count, ctx);

    let (toggle, val_str, val_style): (&str, String, Style) = if unavailable {
        let label = format_object_ref_collapsed(class_name, *entry_count, ctx.show_object_ids, *id);
        ("? ", format!("local variable: {label}"), THEME.null_value)
    } else {
        match phase.clone().unwrap_or(ExpansionPhase::Collapsed) {
            ExpansionPhase::Failed => {
                let err = ctx
                    .object_errors
                    .get(id)
                    .map(|s| s.as_str())
                    .unwrap_or("Failed to resolve object");
                (
                    "! ",
                    format_failed_label(class_name, err),
                    THEME.error_indicator,
                )
            }
            ExpansionPhase::Collapsed => {
                let label =
                    format_object_ref_collapsed(class_name, *entry_count, ctx.show_object_ids, *id);
                ("+ ", format!("local variable: {label}"), Style::new())
            }
            ExpansionPhase::Expanded | ExpansionPhase::Loading => {
                let label =
                    format_object_ref_collapsed(class_name, *entry_count, ctx.show_object_ids, *id);
                ("- ", format!("local variable: {label}"), Style::new())
            }
        }
    };

    push_field_row(
        items,
        indent,
        toggle,
        format!("[{}] {val_str}", var.index),
        val_style,
    );

    if unavailable {
        return;
    }
    if entry_count.is_some() {
        if matches!(
            phase,
            Some(ExpansionPhase::Expanded | ExpansionPhase::Loading)
        ) && let Some(cc) = ctx.collection_chunks.get(id)
        {
            append_collection_items(*id, cc, &format!("{indent}  "), 0, ctx, items);
        }
        return;
    }
    let mut visited = HashSet::new();
    append_fields_expanded(
        *id,
        &format!("{indent}  "),
        0,
        ctx,
        &mut visited,
        items,
        false,
    );
}

/// Appends items for `object_id`'s instance fields and (optionally)
/// static fields at the given `indent`.
///
/// `depth` is used only for the recursion guard (max 16 levels).
/// When `static_ctx` is true the `[static]` section is omitted and
/// the "unavailable" (`?`) toggle is suppressed — static fields are
/// always fully present when the class is loaded.
fn append_fields_expanded(
    object_id: u64,
    indent: &str,
    depth: usize,
    ctx: &RenderCtx<'_>,
    visited: &mut HashSet<u64>,
    items: &mut Vec<ListItem<'static>>,
    static_ctx: bool,
) {
    if depth >= 16 {
        return;
    }
    match get_phase(object_id, ctx.object_phases) {
        ExpansionPhase::Collapsed => {}
        ExpansionPhase::Loading => {
            items.push(ListItem::new(Line::from(Span::styled(
                format!("{indent}~ Loading..."),
                THEME.loading_indicator,
            ))));
        }
        ExpansionPhase::Expanded => {
            visited.insert(object_id);
            let field_list = ctx
                .object_fields
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
                            THEME.cyclic_ref,
                        ))));
                        continue;
                    }

                    let (child_phase, child_unavailable) =
                        render_single_field(field, indent, ctx, items, static_ctx);
                    if !child_unavailable {
                        if let FieldValue::ObjectRef {
                            id,
                            entry_count: Some(_),
                            ..
                        } = field.value
                            && let Some(cc) = ctx.collection_chunks.get(&id)
                        {
                            if matches!(
                                child_phase,
                                Some(ExpansionPhase::Expanded | ExpansionPhase::Loading)
                            ) {
                                append_collection_items(
                                    id,
                                    cc,
                                    &format!("{indent}  "),
                                    depth,
                                    ctx,
                                    items,
                                );
                            }
                        } else if let FieldValue::ObjectRef { id, .. } = field.value {
                            append_fields_expanded(
                                id,
                                &format!("{indent}  "),
                                depth + 1,
                                ctx,
                                visited,
                                items,
                                static_ctx,
                            );
                        }
                    }
                }
            }
            if !static_ctx {
                append_static_items(object_id, indent, depth, ctx, items);
            }
            visited.remove(&object_id);
        }
        ExpansionPhase::Failed => {}
    }
}

fn append_static_items(
    object_id: u64,
    indent: &str,
    depth: usize,
    ctx: &RenderCtx<'_>,
    items: &mut Vec<ListItem<'static>>,
) {
    use super::stack_view::STATIC_FIELDS_RENDER_LIMIT;

    let Some(static_fields) = ctx.object_static_fields.get(&object_id) else {
        return;
    };
    if static_fields.is_empty() {
        return;
    }

    let shown = static_fields.len().min(STATIC_FIELDS_RENDER_LIMIT);
    let hidden = static_fields.len().saturating_sub(shown);
    dbg_log!(
        "append_static_items(0x{:X}): total={} shown={} hidden={}",
        object_id,
        static_fields.len(),
        shown,
        hidden
    );

    items.push(ListItem::new(Line::from(Span::styled(
        format!("{indent}[static]"),
        THEME.null_value,
    ))));

    let field_indent = format!("{indent}  ");
    let child_indent = format!("{indent}    ");
    for field in static_fields.iter().take(shown) {
        let (child_phase, _) = render_single_field(field, &field_indent, ctx, items, true);

        if let FieldValue::ObjectRef {
            id,
            entry_count: Some(_),
            ..
        } = field.value
            && let Some(cc) = ctx.collection_chunks.get(&id)
        {
            if matches!(
                child_phase,
                Some(ExpansionPhase::Expanded | ExpansionPhase::Loading)
            ) {
                append_collection_items(id, cc, &child_indent, depth, ctx, items);
            }
            continue;
        }
        if let FieldValue::ObjectRef { id, .. } = field.value {
            let mut visited = HashSet::new();
            append_fields_expanded(id, &child_indent, depth, ctx, &mut visited, items, true);
        }
    }

    if hidden > 0 {
        items.push(ListItem::new(Line::from(Span::styled(
            format!("{indent}  [+{hidden} more static fields]"),
            THEME.null_value,
        ))));
    }
}

// ── collection tree builders ───────────────────────────────────────

/// Appends items for collection entries and chunk placeholders.
fn append_collection_items(
    collection_id: u64,
    cc: &CollectionChunks,
    indent: &str,
    depth: usize,
    ctx: &RenderCtx<'_>,
    items: &mut Vec<ListItem<'static>>,
) {
    let mut visited_collections = HashSet::new();
    append_collection_items_inner(
        collection_id,
        cc,
        indent,
        depth,
        ctx,
        items,
        &mut visited_collections,
    );
}

fn append_collection_items_inner(
    collection_id: u64,
    cc: &CollectionChunks,
    indent: &str,
    depth: usize,
    ctx: &RenderCtx<'_>,
    items: &mut Vec<ListItem<'static>>,
    visited_collections: &mut HashSet<u64>,
) {
    if depth >= 16 {
        return;
    }
    if !visited_collections.insert(collection_id) {
        return;
    }

    if let Some(page) = &cc.eager_page {
        for entry in &page.entries {
            append_collection_entry_item(
                collection_id,
                entry,
                indent,
                depth,
                ctx,
                items,
                visited_collections,
            );
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
                append_collection_entry_item(
                    collection_id,
                    entry,
                    indent,
                    depth,
                    ctx,
                    items,
                    visited_collections,
                );
            }
        }
    }

    visited_collections.remove(&collection_id);
}

fn append_collection_entry_item(
    collection_id: u64,
    entry: &EntryInfo,
    indent: &str,
    depth: usize,
    ctx: &RenderCtx<'_>,
    items: &mut Vec<ListItem<'static>>,
    visited_collections: &mut HashSet<u64>,
) {
    let (value_phase, value_unavailable) = if let FieldValue::ObjectRef {
        id, entry_count, ..
    } = &entry.value
    {
        object_ref_state(*id, *entry_count, ctx)
    } else {
        (None, false)
    };
    let text = if let (FieldValue::ObjectRef { id, class_name, .. }, Some(ExpansionPhase::Failed)) =
        (&entry.value, &value_phase)
    {
        let err = ctx
            .object_errors
            .get(id)
            .map(|s| s.as_str())
            .unwrap_or("Failed to resolve object");
        let failed = format_failed_label(class_name, err);
        if let Some(key) = &entry.key {
            let k = format_entry_value_text(key, false);
            format!("{indent}! [{}] {} => {failed}", entry.index, k)
        } else {
            format!("{indent}! [{}] {failed}", entry.index)
        }
    } else if value_unavailable {
        let val = format_entry_value_text(&entry.value, ctx.show_object_ids);
        if let Some(key) = &entry.key {
            let k = format_entry_value_text(key, false);
            format!("{indent}? [{}] {} => {val}", entry.index, k)
        } else {
            format!("{indent}? [{}] {val}", entry.index)
        }
    } else {
        StackState::format_entry_line(entry, indent, value_phase.as_ref(), ctx.show_object_ids)
    };
    let row_style = if value_unavailable {
        THEME.null_value
    } else if matches!(value_phase, Some(ExpansionPhase::Failed)) {
        THEME.error_indicator
    } else {
        field_value_style(&entry.value)
    };
    items.push(ListItem::new(Line::from(Span::styled(text, row_style))));

    if !value_unavailable
        && let FieldValue::ObjectRef {
            id,
            entry_count: Some(_),
            ..
        } = &entry.value
        && *id != collection_id
        && let Some(nested) = ctx.collection_chunks.get(id)
    {
        if matches!(
            value_phase,
            Some(ExpansionPhase::Expanded | ExpansionPhase::Loading)
        ) {
            append_collection_items_inner(
                *id,
                nested,
                &format!("{indent}  "),
                depth,
                ctx,
                items,
                visited_collections,
            );
        }
        return;
    }

    if !value_unavailable && let FieldValue::ObjectRef { id, .. } = &entry.value {
        let mut visited = HashSet::new();
        append_fields_expanded(
            *id,
            &format!("{indent}  "),
            depth,
            ctx,
            &mut visited,
            items,
            false,
        );
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
                    entry_count: None,
                }
            },
        }
    }

    /// Basic frame rendering: no locals, null vars, collapsed/expanded object refs.
    mod frame_rendering {
        use super::*;

        #[test]
        fn frame_with_no_vars_renders_no_locals() {
            let items = render_variable_tree(
                TreeRoot::Frame { vars: &[] },
                &HashMap::new(),
                &HashMap::new(),
                &HashMap::new(),
                &HashMap::new(),
                &HashMap::new(),
                RenderOptions {
                    show_object_ids: false,
                    snapshot_mode: false,
                },
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
                &HashMap::new(),
                RenderOptions {
                    show_object_ids: false,
                    snapshot_mode: false,
                },
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
                &HashMap::new(),
                RenderOptions {
                    show_object_ids: false,
                    snapshot_mode: false,
                },
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
                TreeRoot::Frame { vars: &vars },
                &object_fields,
                &HashMap::new(),
                &HashMap::new(),
                &object_phases,
                &HashMap::new(),
                RenderOptions {
                    show_object_ids: false,
                    snapshot_mode: false,
                },
            );
            let text = render_items(items);
            assert!(text.contains("-"), "expected - toggle, got: {text:?}");
            assert!(text.contains("count"), "expected field name, got: {text:?}");
            assert!(text.contains("7"), "expected field value, got: {text:?}");
        }
    }

    /// Snapshot mode behaviour: `?` toggle for unavailable refs, `+` for loaded collections.
    mod snapshot_mode {
        use super::*;

        #[test]
        fn snapshot_mode_unavailable_var_shows_question_toggle() {
            let vars = vec![make_var(0, 42)];
            let items = render_variable_tree(
                TreeRoot::Frame { vars: &vars },
                &HashMap::new(),
                &HashMap::new(),
                &HashMap::new(),
                &HashMap::new(),
                &HashMap::new(),
                RenderOptions {
                    show_object_ids: false,
                    snapshot_mode: true,
                },
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

            // Parent expanded; collection id absent => collapsed in snapshot mode.
            let mut object_phases = HashMap::new();
            object_phases.insert(1u64, ExpansionPhase::Expanded);

            let items = render_variable_tree(
                TreeRoot::Frame { vars: &vars },
                &object_fields,
                &HashMap::new(),
                &collection_chunks,
                &object_phases,
                &HashMap::new(),
                RenderOptions {
                    show_object_ids: false,
                    snapshot_mode: true,
                },
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

    /// Object display: id visibility toggle, cyclic-ref guard, failed-var error label.
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
                TreeRoot::Frame { vars: &vars },
                &object_fields,
                &HashMap::new(),
                &HashMap::new(),
                &object_phases,
                &HashMap::new(),
                RenderOptions {
                    show_object_ids: true,
                    snapshot_mode: false,
                },
            );
            let with_ids_text = render_items(with_ids);
            assert!(
                with_ids_text.contains("Child @ 0x4D"),
                "expected nested object id in field row, got: {with_ids_text:?}"
            );

            let without_ids = render_variable_tree(
                TreeRoot::Frame { vars: &vars },
                &object_fields,
                &HashMap::new(),
                &HashMap::new(),
                &object_phases,
                &HashMap::new(),
                RenderOptions {
                    show_object_ids: false,
                    snapshot_mode: false,
                },
            );
            let without_ids_text = render_items(without_ids);
            assert!(
                !without_ids_text.contains("@ 0x4D"),
                "expected no nested object id when toggle is off, got: {without_ids_text:?}"
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
                TreeRoot::Frame { vars: &vars },
                &object_fields,
                &HashMap::new(),
                &HashMap::new(),
                &object_phases,
                &HashMap::new(),
                RenderOptions {
                    show_object_ids: false,
                    snapshot_mode: false,
                },
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
                TreeRoot::Frame { vars: &vars },
                &HashMap::new(),
                &HashMap::new(),
                &HashMap::new(),
                &object_phases,
                &object_errors,
                RenderOptions {
                    show_object_ids: false,
                    snapshot_mode: false,
                },
            );
            let text = render_items(items);
            assert!(text.contains("Object — boom"), "got: {text:?}");
            assert!(
                !text.contains("local variable:"),
                "failed label must not include local variable prefix: {text:?}"
            );
        }
    }

    /// `TreeRoot::Subtree` rendering: field indentation, collection entries, inline errors.
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
                },
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
                },
            );
            let text = render_items(items);

            assert!(
                text.contains("[0] 7"),
                "expected eager entry, got: {text:?}"
            );
            assert!(
                text.contains("+ [100...119]"),
                "expected chunk sentinel row, got: {text:?}"
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
                TreeRoot::Frame { vars: &vars },
                &object_fields,
                &HashMap::new(),
                &collection_chunks,
                &object_phases,
                &object_errors,
                RenderOptions {
                    show_object_ids: false,
                    snapshot_mode: false,
                },
            );
            let text = render_items(items);
            assert!(
                text.contains("! [0] String — entry missing"),
                "failed collection entry must include inline error message, got: {text:?}"
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
}
