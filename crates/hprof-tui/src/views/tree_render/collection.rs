//! Paginated collection item rendering with chunk navigation
//! and nested entry expansion.

use std::collections::HashSet;

use hprof_engine::{EntryInfo, FieldValue};
use ratatui::{
    text::{Line, Span},
    widgets::ListItem,
};

use crate::theme::THEME;

use super::super::stack_view::{
    ChunkState, CollectionChunks, CollectionId, EntryIdx, ExpansionPhase, NavigationPath,
    NavigationPathBuilder, StackState, compute_chunk_ranges, field_value_style,
    format_entry_value_text,
};
use super::RenderCtx;
use super::expansion::append_fields_expanded;
use super::helpers::{format_failed_label, object_ref_state};

/// Appends items for collection entries and chunk placeholders.
pub(super) fn append_collection_items(
    collection_id: u64,
    cc: &CollectionChunks,
    indent: &str,
    depth: usize,
    ctx: &RenderCtx<'_>,
    items: &mut Vec<ListItem<'static>>,
    parent_path: Option<NavigationPath>,
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
        parent_path,
    );
}

#[allow(clippy::too_many_arguments)]
fn append_collection_items_inner(
    collection_id: u64,
    cc: &CollectionChunks,
    indent: &str,
    depth: usize,
    ctx: &RenderCtx<'_>,
    items: &mut Vec<ListItem<'static>>,
    visited_collections: &mut HashSet<u64>,
    parent_path: Option<NavigationPath>,
) {
    if depth >= 16 {
        return;
    }
    if !visited_collections.insert(collection_id) {
        return;
    }

    if let Some(page) = &cc.eager_page {
        for entry in &page.entries {
            let entry_path = parent_path.as_ref().map(|pp| {
                NavigationPathBuilder::extend(pp.clone())
                    .collection_entry(CollectionId(collection_id), EntryIdx(entry.index))
                    .build()
            });
            append_collection_entry_item(
                collection_id,
                entry,
                indent,
                depth,
                ctx,
                items,
                visited_collections,
                entry_path,
            );
        }
    }

    let ranges = compute_chunk_ranges(cc.total_count);
    for (offset, limit) in &ranges {
        let end = offset + limit - 1;
        let chunk_state = cc.chunk_pages.get(offset);

        if ctx.snapshot_mode && !matches!(chunk_state, Some(ChunkState::Loaded(_))) {
            continue;
        }

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
                let entry_path = parent_path.as_ref().map(|pp| {
                    NavigationPathBuilder::extend(pp.clone())
                        .collection_entry(CollectionId(collection_id), EntryIdx(entry.index))
                        .build()
                });
                append_collection_entry_item(
                    collection_id,
                    entry,
                    indent,
                    depth,
                    ctx,
                    items,
                    visited_collections,
                    entry_path,
                );
            }
        }
    }

    visited_collections.remove(&collection_id);
}

#[allow(clippy::too_many_arguments)]
fn append_collection_entry_item(
    collection_id: u64,
    entry: &EntryInfo,
    indent: &str,
    depth: usize,
    ctx: &RenderCtx<'_>,
    items: &mut Vec<ListItem<'static>>,
    visited_collections: &mut HashSet<u64>,
    entry_path: Option<NavigationPath>,
) {
    let (value_phase, value_unavailable) = if let FieldValue::ObjectRef {
        id, entry_count, ..
    } = &entry.value
    {
        object_ref_state(*id, *entry_count, ctx, entry_path.as_ref())
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
                entry_path,
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
            entry_path,
        );
    }
}
