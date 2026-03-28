//! Recursive field expansion and static-field rendering for
//! object trees.

use std::collections::HashSet;

use hprof_engine::{FieldInfo, FieldValue};
use ratatui::{
    text::{Line, Span},
    widgets::ListItem,
};

use crate::favorites::HideKey;
use crate::theme::THEME;

use super::super::stack_view::{
    ExpansionPhase, FieldIdx, NavigationPath, NavigationPathBuilder, STATIC_FIELDS_RENDER_LIMIT,
    StaticFieldIdx, field_value_style, format_field_value_display,
};
use super::RenderCtx;
use super::collection::append_collection_items;
use super::helpers::{format_failed_label, object_ref_state, push_field_row};

/// Computes phase / style / toggle for a single `FieldInfo`,
/// pushes the rendered row, and returns
/// `(child_phase, child_unavailable)` so the caller can decide
/// whether to recurse.
///
/// `path` is the current node's `NavigationPath` for path-based
/// phase lookup (`None` in snapshot mode).
pub(super) fn render_single_field(
    field: &FieldInfo,
    indent: &str,
    ctx: &RenderCtx<'_>,
    items: &mut Vec<ListItem<'static>>,
    static_ctx: bool,
    path: Option<&NavigationPath>,
) -> (Option<ExpansionPhase>, bool) {
    let (child_phase, child_unavailable) = if let FieldValue::ObjectRef {
        id, entry_count, ..
    } = &field.value
    {
        let (phase, unavail) = object_ref_state(*id, *entry_count, ctx, path);
        if static_ctx && unavail {
            (Some(ctx.phase_for(*id, path)), false)
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
    } else if matches!(child_phase, Some(ExpansionPhase::Loading)) {
        THEME.loading_indicator
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

/// Appends items for `object_id`'s instance fields and
/// (optionally) static fields at the given `indent`.
///
/// `depth` is used only for the recursion guard (max 16 levels).
/// `parent_path` is the path of the parent node for path-based
/// phase lookup (`None` in snapshot mode).
#[allow(clippy::too_many_arguments)]
pub(super) fn append_fields_expanded(
    object_id: u64,
    indent: &str,
    depth: usize,
    ctx: &RenderCtx<'_>,
    visited: &mut HashSet<u64>,
    items: &mut Vec<ListItem<'static>>,
    static_ctx: bool,
    parent_path: Option<NavigationPath>,
) {
    if depth >= 16 {
        return;
    }
    match ctx.phase_for(object_id, parent_path.as_ref()) {
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
                for (field_idx, field) in field_list.iter().enumerate() {
                    let hide_key = HideKey::Field {
                        parent_id: object_id,
                        field_idx,
                    };
                    let is_hidden = ctx
                        .hidden_fields
                        .map(|s| s.contains(&hide_key))
                        .unwrap_or(false);
                    if is_hidden {
                        if ctx.show_hidden {
                            items.push(ListItem::new(Line::from(Span::styled(
                                format!(
                                    "{indent}  \u{25AA} \
                                         [hidden: {}]",
                                    field.name
                                ),
                                THEME.null_value,
                            ))));
                        }
                        continue;
                    }

                    let child_path = parent_path.as_ref().map(|pp| {
                        NavigationPathBuilder::extend(pp.clone())
                            .field(FieldIdx(field_idx))
                            .build()
                    });

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
                            "{indent}  {}: \u{21BB} {} \
                             @ 0x{:X} [{label}]",
                            field.name, short, id
                        );
                        items.push(ListItem::new(Line::from(Span::styled(
                            text,
                            THEME.cyclic_ref,
                        ))));
                        continue;
                    }

                    let (child_phase, child_unavailable) = render_single_field(
                        field,
                        indent,
                        ctx,
                        items,
                        static_ctx,
                        child_path.as_ref(),
                    );
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
                                    child_path,
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
                                child_path,
                            );
                        }
                    }
                }
            }
            if !static_ctx {
                append_static_items(object_id, indent, depth, ctx, items, parent_path.as_ref());
            }
            visited.remove(&object_id);
        }
        ExpansionPhase::Failed => {}
    }
}

/// Appends a `[static fields]` header followed by static-field rows
/// for `object_id`. Caps output at `STATIC_FIELDS_RENDER_LIMIT`
/// and appends a `[+N more]` sentinel when fields are truncated.
pub(super) fn append_static_items(
    object_id: u64,
    indent: &str,
    depth: usize,
    ctx: &RenderCtx<'_>,
    items: &mut Vec<ListItem<'static>>,
    parent_path: Option<&NavigationPath>,
) {
    let Some(static_fields) = ctx.object_static_fields.get(&object_id) else {
        return;
    };
    if static_fields.is_empty() {
        return;
    }

    // Snapshot/favorites mode: no static section at all.
    if ctx.static_section_expanded.is_none() {
        return;
    }

    let total = static_fields.len();
    let expanded = ctx.is_static_section_expanded(parent_path);
    dbg_log!(
        "append_static_items(0x{:X}): total={} expanded={}",
        object_id,
        total,
        expanded,
    );

    let indicator = if expanded { "\u{25BE}" } else { "\u{25B8}" };
    items.push(ListItem::new(Line::from(Span::styled(
        format!("{indent}{indicator} [static fields] ({total})"),
        THEME.null_value,
    ))));

    if !expanded {
        return;
    }

    let shown = total.min(STATIC_FIELDS_RENDER_LIMIT);
    let hidden = total.saturating_sub(shown);

    let field_indent = format!("{indent}  ");
    let child_indent = format!("{indent}    ");
    for (si, field) in static_fields.iter().take(shown).enumerate() {
        let static_path = parent_path.map(|pp| {
            NavigationPathBuilder::extend(pp.clone())
                .static_field(StaticFieldIdx(si))
                .build()
        });

        let (child_phase, _) =
            render_single_field(field, &field_indent, ctx, items, true, static_path.as_ref());

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
                append_collection_items(id, cc, &child_indent, depth, ctx, items, static_path);
            }
            continue;
        }
        if let FieldValue::ObjectRef { id, .. } = field.value {
            let mut visited = HashSet::new();
            append_fields_expanded(
                id,
                &child_indent,
                depth,
                ctx,
                &mut visited,
                items,
                true,
                static_path,
            );
        }
    }

    if hidden > 0 {
        items.push(ListItem::new(Line::from(Span::styled(
            format!("{indent}  [+{hidden} more static fields]"),
            THEME.null_value,
        ))));
    }
}
