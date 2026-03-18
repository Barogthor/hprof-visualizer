//! Phase lookup, error display, and text formatting utilities
//! for tree rendering.

use std::collections::HashMap;

use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::ListItem,
};

use crate::theme::THEME;

use super::super::stack_view::{ExpansionPhase, FAILED_LABEL_SEP, NavigationPath};
use super::RenderCtx;

/// Determines the expansion phase and availability of an object
/// reference for rendering.
///
/// When `path` is `Some` and `ctx.expansion_phases` is available
/// (live stack view mode), phase is looked up by path. Otherwise
/// falls back to `ctx.object_phases` (snapshot mode).
pub(super) fn object_ref_state(
    object_id: u64,
    entry_count: Option<u64>,
    ctx: &RenderCtx<'_>,
    path: Option<&NavigationPath>,
) -> (Option<ExpansionPhase>, bool) {
    if entry_count == Some(0) {
        return (None, false);
    }
    if entry_count.is_some() && ctx.collection_chunks.contains_key(&object_id) {
        // In live mode with path-based phases, respect Loading state;
        // otherwise delegate to phase_for (snapshot/collapsed logic).
        if let (Some(ep), Some(p)) = (&ctx.expansion_phases, path) {
            let phase = ep.get(p).cloned().unwrap_or(ExpansionPhase::Expanded);
            return (Some(phase), false);
        }
        if ctx.snapshot_mode {
            return (Some(ctx.phase_for(object_id, path)), false);
        }
        return (Some(ExpansionPhase::Expanded), false);
    }

    let has_snapshot_data = ctx.object_fields.contains_key(&object_id)
        || ctx.object_static_fields.contains_key(&object_id)
        || ctx.object_phases.contains_key(&object_id);

    if has_snapshot_data || !ctx.snapshot_mode {
        (Some(ctx.phase_for(object_id, path)), false)
    } else {
        (None, true)
    }
}

pub(super) fn get_phase(
    object_id: u64,
    object_phases: &HashMap<u64, ExpansionPhase>,
) -> ExpansionPhase {
    object_phases
        .get(&object_id)
        .cloned()
        .unwrap_or(ExpansionPhase::Collapsed)
}

pub(super) fn split_object_id_range(text: &str) -> Option<(usize, usize)> {
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

pub(super) fn spans_with_dimmed_object_id(text: String, base_style: Style) -> Vec<Span<'static>> {
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

/// Formats a failed object label as
/// `ShortClass — error message`.
pub(super) fn format_failed_label(class_name: &str, err: &str) -> String {
    let short = if class_name.is_empty() {
        "Object"
    } else {
        class_name.rsplit('.').next().unwrap_or(class_name)
    };
    format!("{short}{FAILED_LABEL_SEP}{err}")
}

/// Appends a single field row with toggle indicator and dimmed
/// object ID.
pub(super) fn push_field_row(
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
