//! Frame-variable dispatch — renders a single `VariableInfo`
//! with toggle indicator and recursive child expansion.

use std::collections::HashSet;

use hprof_engine::{VariableInfo, VariableValue};
use ratatui::style::Style;

use crate::theme::THEME;

use super::super::stack_view::{ExpansionPhase, format_object_ref_collapsed};
use super::RenderCtx;
use super::collection::append_collection_items;
use super::expansion::append_fields_expanded;
use super::helpers::{format_failed_label, object_ref_state, push_field_row};

pub(super) fn append_var(
    var: &VariableInfo,
    indent: &str,
    ctx: &RenderCtx<'_>,
    items: &mut Vec<ratatui::widgets::ListItem<'static>>,
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
    } else if let Some(p) = phase.clone() {
        match p {
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
    } else {
        let label = format_object_ref_collapsed(class_name, *entry_count, ctx.show_object_ids, *id);
        ("  ", format!("local variable: {label}"), Style::new())
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
