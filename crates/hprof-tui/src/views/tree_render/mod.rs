//! Shared variable-tree rendering for [`StackView`] and [`FavoritesPanel`].
//!
//! [`render_variable_tree`] produces a flat `Vec<ListItem>` from a tree root
//! without embedding cursor-selection styling — callers apply selection via
//! ratatui's `List::highlight_style` + `ListState`.

mod collection;
mod expansion;
mod helpers;
mod variable;

use std::collections::{HashMap, HashSet};

use hprof_engine::{FieldInfo, VariableInfo};
use ratatui::{
    text::{Line, Span},
    widgets::ListItem,
};

use crate::favorites::HideKey;
use crate::theme::THEME;

use super::stack_view::{CollectionChunks, ExpansionPhase};
use collection::append_collection_items;
use expansion::append_fields_expanded;
use variable::append_var;

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
    /// When `true`, hidden rows are rendered as `▪ [hidden: …]` placeholders
    /// so the user can navigate to them and restore individually.
    /// When `false` (default), hidden rows are absent from the output.
    pub show_hidden: bool,
}

/// Shared read-only context threaded through all render helpers.
pub(super) struct RenderCtx<'a> {
    object_fields: &'a HashMap<u64, Vec<FieldInfo>>,
    object_static_fields: &'a HashMap<u64, Vec<FieldInfo>>,
    collection_chunks: &'a HashMap<u64, CollectionChunks>,
    object_phases: &'a HashMap<u64, ExpansionPhase>,
    object_errors: &'a HashMap<u64, String>,
    show_object_ids: bool,
    snapshot_mode: bool,
    /// Row-level hide overlay — `None` means hiding not applicable (e.g. stack view).
    hidden_fields: Option<&'a HashSet<HideKey>>,
    /// Mirrors `RenderOptions::show_hidden`.
    show_hidden: bool,
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
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_variable_tree(
    root: TreeRoot<'_>,
    object_fields: &HashMap<u64, Vec<FieldInfo>>,
    object_static_fields: &HashMap<u64, Vec<FieldInfo>>,
    collection_chunks: &HashMap<u64, CollectionChunks>,
    object_phases: &HashMap<u64, ExpansionPhase>,
    object_errors: &HashMap<u64, String>,
    options: RenderOptions,
    hidden_fields: Option<&HashSet<HideKey>>,
) -> Vec<ListItem<'static>> {
    let ctx = RenderCtx {
        object_fields,
        object_static_fields,
        collection_chunks,
        object_phases,
        object_errors,
        show_object_ids: options.show_object_ids,
        snapshot_mode: options.snapshot_mode,
        hidden_fields,
        show_hidden: options.show_hidden,
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
                for (var_idx, var) in vars.iter().enumerate() {
                    let key = HideKey::Var(var_idx);
                    let is_hidden = ctx.hidden_fields.map(|s| s.contains(&key)).unwrap_or(false);
                    if is_hidden {
                        if ctx.show_hidden {
                            items.push(ListItem::new(Line::from(Span::styled(
                                format!("  \u{25AA} [hidden: var[{}]]", var_idx),
                                THEME.null_value,
                            ))));
                        }
                        continue;
                    }
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

#[cfg(test)]
mod tests;
