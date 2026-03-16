//! Favorites panel widget: renders pinned snapshots side-by-side.
//!
//! [`FavoritesPanel`] is a [`StatefulWidget`] that renders each [`PinnedItem`]
//! with a header and its frozen variable tree via [`render_variable_tree`].

use std::collections::{HashMap, HashSet};

use hprof_engine::{EntryInfo, FieldInfo, FieldValue, VariableInfo, VariableValue};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, ListState, StatefulWidget, Widget},
};

use crate::{
    favorites::{HideKey, PinnedItem, PinnedSnapshot},
    theme::THEME,
    views::{
        stack_view::{
            ChunkState, CollectionChunks, CollectionId, EntryIdx, ExpansionPhase, FieldIdx,
            FrameId, NavigationPath, NavigationPathBuilder, STATIC_FIELDS_RENDER_LIMIT,
            StaticFieldIdx, VarIdx, compute_chunk_ranges,
        },
        tree_render::{RenderOptions, TreeRoot, render_variable_tree},
    },
};

type RowKindMap = HashMap<usize, (u64, bool)>;
type ChunkSentinelMap = HashMap<usize, (u64, usize)>;
type FieldRowMap = HashMap<usize, (HideKey, bool)>;
type PathMap = Vec<Option<NavigationPath>>;
type RowMetadata = (usize, RowKindMap, ChunkSentinelMap, FieldRowMap, PathMap);

/// Render-time data for the favorites panel (non-mutable).
pub struct FavoritesPanel<'a> {
    /// Whether this panel has keyboard focus.
    pub focused: bool,
    /// Whether object IDs should be shown in rendered rows.
    pub show_object_ids: bool,
    /// Pinned items to display (borrowed from `App`).
    pub pinned: &'a [PinnedItem],
}

/// Mutable scroll state for the favorites panel.
#[derive(Default)]
pub struct FavoritesPanelState {
    /// Index of the selected pinned item (0..items_len).
    selected_item: usize,
    items_len: usize,
    /// Sub-row within the selected item (0 = header row).
    sub_row: usize,
    /// Total rendered rows per item.
    row_counts: Vec<usize>,
    /// Per-item row-kind map: sub_row -> (object_id, is_collapsed).
    row_kind_maps: Vec<RowKindMap>,
    /// Per-item chunk sentinel map: sub_row -> (collection_id, chunk_offset).
    chunk_sentinel_maps: Vec<ChunkSentinelMap>,
    /// Per-item field-row map: sub_row -> (HideKey, is_hidden).
    field_row_maps: Vec<FieldRowMap>,
    /// Per-item path map: sub_row -> NavigationPath for toggleable rows.
    path_maps: Vec<Vec<Option<crate::views::stack_view::NavigationPath>>>,
    /// ratatui list state — selected index is the absolute flat-row position.
    list_state: ListState,
}

impl FavoritesPanelState {
    /// Returns selected item index in `pinned`.
    pub fn selected_index(&self) -> usize {
        self.selected_item
    }

    /// Updates known item count for selection sync.
    pub fn set_items_len(&mut self, len: usize) {
        self.items_len = len;
        if len == 0 {
            self.selected_item = 0;
            self.sub_row = 0;
            self.row_counts.clear();
            self.row_kind_maps.clear();
            self.chunk_sentinel_maps.clear();
            self.field_row_maps.clear();
            self.path_maps.clear();
            self.list_state.select(None);
            return;
        }

        self.selected_item = self.selected_item.min(len.saturating_sub(1));
        self.row_counts.resize(len, 1);
        self.row_kind_maps.resize_with(len, HashMap::new);
        self.chunk_sentinel_maps.resize_with(len, HashMap::new);
        self.field_row_maps.resize_with(len, HashMap::new);
        self.path_maps.resize_with(len, Vec::new);
        self.clamp_sub_row();
    }

    /// Sets selected item index, or fully deselects when `None`.
    pub fn set_selected_index(&mut self, idx: Option<usize>) {
        if self.items_len == 0 {
            self.selected_item = 0;
            self.sub_row = 0;
            self.list_state.select(None);
            return;
        }

        match idx {
            Some(i) => {
                let clamped = i.min(self.items_len.saturating_sub(1));
                if clamped != self.selected_item {
                    self.selected_item = clamped;
                    self.sub_row = 0;
                }
                self.clamp_sub_row();
            }
            None => {
                self.selected_item = 0;
                self.sub_row = 0;
                self.list_state.select(None);
            }
        }
    }

    pub(crate) fn update_row_metadata(
        &mut self,
        row_counts: Vec<usize>,
        row_kind_maps: Vec<RowKindMap>,
        chunk_sentinel_maps: Vec<ChunkSentinelMap>,
        field_row_maps: Vec<FieldRowMap>,
        path_maps: Vec<PathMap>,
    ) {
        debug_assert_eq!(
            row_counts.len(),
            self.items_len,
            "row_counts length mismatch"
        );
        debug_assert_eq!(
            row_kind_maps.len(),
            self.items_len,
            "row_kind_maps length mismatch"
        );
        debug_assert_eq!(
            chunk_sentinel_maps.len(),
            self.items_len,
            "chunk_sentinel_maps length mismatch"
        );
        debug_assert_eq!(
            field_row_maps.len(),
            self.items_len,
            "field_row_maps length mismatch"
        );
        debug_assert_eq!(path_maps.len(), self.items_len, "path_maps length mismatch");

        self.row_counts = row_counts;
        self.row_kind_maps = row_kind_maps;
        self.chunk_sentinel_maps = chunk_sentinel_maps;
        self.field_row_maps = field_row_maps;
        self.path_maps = path_maps;
        self.clamp_sub_row();
    }

    pub fn move_up(&mut self) {
        if self.items_len == 0 {
            return;
        }
        if self.sub_row > 0 {
            self.sub_row -= 1;
            return;
        }
        if self.selected_item > 0 {
            self.selected_item -= 1;
            self.sub_row = self
                .row_counts
                .get(self.selected_item)
                .copied()
                .unwrap_or(1)
                .saturating_sub(1);
        }
    }

    pub fn move_down(&mut self) {
        if self.items_len == 0 {
            return;
        }
        let rows = self
            .row_counts
            .get(self.selected_item)
            .copied()
            .unwrap_or(1);
        if self.sub_row + 1 < rows {
            self.sub_row += 1;
            return;
        }
        if self.selected_item + 1 < self.items_len {
            self.selected_item += 1;
            self.sub_row = 0;
        }
    }

    pub fn list_state_mut(&mut self) -> &mut ListState {
        &mut self.list_state
    }

    pub fn abs_row(&self) -> usize {
        if self.items_len == 0 || self.selected_item >= self.row_counts.len() {
            return 0;
        }
        self.row_counts[..self.selected_item].iter().sum::<usize>() + self.sub_row
    }

    pub fn current_toggleable_object(&self) -> Option<(u64, bool)> {
        self.row_kind_maps
            .get(self.selected_item)?
            .get(&self.sub_row)
            .copied()
    }

    /// Returns the `NavigationPath` for the row under the cursor,
    /// or `None` if on a non-toggleable row (header/separator).
    pub fn current_toggleable_path(&self) -> Option<&crate::views::stack_view::NavigationPath> {
        self.path_maps
            .get(self.selected_item)?
            .get(self.sub_row)?
            .as_ref()
    }

    pub fn current_chunk_sentinel(&self) -> Option<(u64, usize)> {
        self.chunk_sentinel_maps
            .get(self.selected_item)?
            .get(&self.sub_row)
            .copied()
    }

    /// Returns the `HideKey` and hidden status for the row currently under the
    /// cursor, or `None` if the cursor is on the header or a non-hideable row.
    pub fn field_key_at_cursor(&self) -> Option<(HideKey, bool)> {
        self.field_row_maps
            .get(self.selected_item)?
            .get(&self.sub_row)
            .copied()
    }

    pub(crate) fn clamp_sub_row(&mut self) {
        let max_sub_row = self
            .row_counts
            .get(self.selected_item)
            .copied()
            .unwrap_or(1)
            .saturating_sub(1);
        self.sub_row = self.sub_row.min(max_sub_row);
    }
}

fn object_phases_for_item(
    object_fields: &HashMap<u64, Vec<FieldInfo>>,
    object_static_fields: &HashMap<u64, Vec<FieldInfo>>,
    collection_chunks: &HashMap<u64, CollectionChunks>,
) -> HashMap<u64, ExpansionPhase> {
    object_fields
        .keys()
        .chain(object_static_fields.keys())
        .chain(collection_chunks.keys())
        .map(|&id| (id, ExpansionPhase::Expanded))
        .collect()
}

struct MetadataCollector<'a> {
    object_fields: &'a HashMap<u64, Vec<FieldInfo>>,
    object_static_fields: &'a HashMap<u64, Vec<FieldInfo>>,
    collection_chunks: &'a HashMap<u64, CollectionChunks>,
    local_collapsed: &'a HashSet<NavigationPath>,
    hidden_fields: &'a HashSet<HideKey>,
    show_hidden: bool,
    row_count: usize,
    kind_map: RowKindMap,
    sentinel_map: ChunkSentinelMap,
    field_row_map: FieldRowMap,
    path_map: PathMap,
}

impl<'a> MetadataCollector<'a> {
    fn new(
        object_fields: &'a HashMap<u64, Vec<FieldInfo>>,
        object_static_fields: &'a HashMap<u64, Vec<FieldInfo>>,
        collection_chunks: &'a HashMap<u64, CollectionChunks>,
        local_collapsed: &'a HashSet<NavigationPath>,
        row_count: usize,
        hidden_fields: &'a HashSet<HideKey>,
        show_hidden: bool,
    ) -> Self {
        Self {
            object_fields,
            object_static_fields,
            collection_chunks,
            local_collapsed,
            hidden_fields,
            show_hidden,
            row_count,
            kind_map: HashMap::new(),
            sentinel_map: HashMap::new(),
            field_row_map: HashMap::new(),
            path_map: Vec::new(),
        }
    }

    fn into_parts(self) -> RowMetadata {
        (
            self.row_count,
            self.kind_map,
            self.sentinel_map,
            self.field_row_map,
            self.path_map,
        )
    }

    fn push_row(&mut self, path: Option<NavigationPath>) -> usize {
        let row = self.row_count;
        self.row_count += 1;
        self.path_map.push(path);
        row
    }

    fn phase_for_path(&self, object_id: u64, path: &NavigationPath) -> ExpansionPhase {
        if self.local_collapsed.contains(path) {
            return ExpansionPhase::Collapsed;
        }
        if self.has_data(object_id) {
            ExpansionPhase::Expanded
        } else {
            ExpansionPhase::Collapsed
        }
    }

    fn has_data(&self, object_id: u64) -> bool {
        self.object_fields.contains_key(&object_id)
            || self.object_static_fields.contains_key(&object_id)
            || self.collection_chunks.contains_key(&object_id)
    }

    fn collect_static_rows(&mut self, object_id: u64, parent_path: &NavigationPath, depth: usize) {
        let Some(static_fields) = self.object_static_fields.get(&object_id) else {
            return;
        };
        if static_fields.is_empty() {
            return;
        }

        self.push_row(None); // [static]
        let shown = static_fields.len().min(STATIC_FIELDS_RENDER_LIMIT);
        for (si, field) in static_fields.iter().take(shown).enumerate() {
            let static_path = NavigationPathBuilder::extend(parent_path.clone())
                .static_field(StaticFieldIdx(si))
                .build();

            let (child_phase, toggleable, is_collection) =
                if let FieldValue::ObjectRef {
                    id, entry_count, ..
                } = field.value
                {
                    self.resolve_at_path(id, entry_count, &static_path)
                } else {
                    (ExpansionPhase::Collapsed, false, false)
                };

            let row = self.push_row(if toggleable {
                Some(static_path.clone())
            } else {
                None
            });
            if toggleable && let FieldValue::ObjectRef { id, .. } = field.value {
                self.kind_map
                    .insert(row, (id, matches!(child_phase, ExpansionPhase::Collapsed)));
            }

            if is_collection {
                if matches!(child_phase, ExpansionPhase::Expanded)
                    && let FieldValue::ObjectRef {
                        id,
                        entry_count: Some(_),
                        ..
                    } = field.value
                    && let Some(cc) = self.collection_chunks.get(&id)
                {
                    self.collect_collection_rows(id, cc, &static_path);
                }
                continue;
            }

            if toggleable && let FieldValue::ObjectRef { id, .. } = field.value {
                let mut visited = HashSet::new();
                self.collect_static_object_rows(id, &static_path, &mut visited, depth + 1);
            }
        }

        if static_fields.len() > shown {
            self.push_row(None); // [+N more static fields]
        }
    }

    fn collect_static_object_rows(
        &mut self,
        obj_id: u64,
        path: &NavigationPath,
        visited: &mut HashSet<u64>,
        depth: usize,
    ) {
        if depth >= 16 {
            return;
        }
        match self.phase_for_path(obj_id, path) {
            ExpansionPhase::Collapsed | ExpansionPhase::Failed => {}
            ExpansionPhase::Loading => unreachable!("frozen snapshot"),
            ExpansionPhase::Expanded => {
                let field_list = self
                    .object_fields
                    .get(&obj_id)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                if field_list.is_empty() {
                    self.push_row(None);
                    return;
                }

                visited.insert(obj_id);
                for (fi, field) in field_list.iter().enumerate() {
                    if let FieldValue::ObjectRef { id, .. } = &field.value
                        && visited.contains(id)
                    {
                        self.push_row(None);
                        continue;
                    }

                    let child_path = NavigationPathBuilder::extend(path.clone())
                        .field(FieldIdx(fi))
                        .build();

                    let (child_phase, toggleable, is_collection) =
                        if let FieldValue::ObjectRef {
                            id, entry_count, ..
                        } = field.value
                        {
                            self.resolve_at_path(id, entry_count, &child_path)
                        } else {
                            (ExpansionPhase::Collapsed, false, false)
                        };

                    let row = self.push_row(if toggleable {
                        Some(child_path.clone())
                    } else {
                        None
                    });
                    if toggleable && let FieldValue::ObjectRef { id, .. } = field.value {
                        self.kind_map
                            .insert(row, (id, matches!(child_phase, ExpansionPhase::Collapsed)));
                    }

                    if is_collection {
                        if matches!(child_phase, ExpansionPhase::Expanded)
                            && let FieldValue::ObjectRef {
                                id,
                                entry_count: Some(_),
                                ..
                            } = field.value
                            && let Some(cc) = self.collection_chunks.get(&id)
                        {
                            self.collect_collection_rows(id, cc, &child_path);
                        }
                        continue;
                    }

                    if toggleable && let FieldValue::ObjectRef { id, .. } = field.value {
                        self.collect_static_object_rows(id, &child_path, visited, depth + 1);
                    }
                }
                visited.remove(&obj_id);
            }
        }
    }

    fn collect_frame_rows(&mut self, vars: &[VariableInfo], frame_id: u64) {
        if vars.is_empty() {
            self.push_row(None); // (no locals)
            return;
        }
        for (var_idx, var) in vars.iter().enumerate() {
            let key = HideKey::Var(var_idx);
            let is_hidden = self.hidden_fields.contains(&key);
            if is_hidden {
                if self.show_hidden {
                    let row = self.push_row(None);
                    self.field_row_map.insert(row, (key, true));
                }
                continue;
            }
            let var_row = self.row_count;
            self.field_row_map.insert(var_row, (key, false));
            let path = NavigationPathBuilder::new(FrameId(frame_id), VarIdx(var_idx)).build();
            self.collect_var_row(var, path);
        }
    }

    fn collect_var_row(&mut self, var: &VariableInfo, path: NavigationPath) {
        let VariableValue::ObjectRef {
            id, entry_count, ..
        } = var.value
        else {
            self.push_row(None);
            return;
        };

        let (phase, toggleable, is_collection) = self.resolve_at_path(id, entry_count, &path);

        let row = self.push_row(if toggleable { Some(path.clone()) } else { None });
        if toggleable {
            self.kind_map
                .insert(row, (id, matches!(phase, ExpansionPhase::Collapsed)));
        }

        if is_collection {
            if matches!(phase, ExpansionPhase::Expanded)
                && let Some(cc) = self.collection_chunks.get(&id)
            {
                self.collect_collection_rows(id, cc, &path);
            }
            return;
        }

        if !toggleable {
            return;
        }

        let mut visited = HashSet::new();
        self.collect_object_children_rows(id, &path, &mut visited, 0);
    }

    fn collect_object_children_rows(
        &mut self,
        object_id: u64,
        path: &NavigationPath,
        visited: &mut HashSet<u64>,
        depth: usize,
    ) {
        if depth >= 16 {
            return;
        }
        match self.phase_for_path(object_id, path) {
            ExpansionPhase::Collapsed | ExpansionPhase::Failed => {}
            ExpansionPhase::Loading => unreachable!("frozen snapshot"),
            ExpansionPhase::Expanded => {
                visited.insert(object_id);
                let field_list = self
                    .object_fields
                    .get(&object_id)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                if field_list.is_empty() {
                    self.push_row(None);
                } else {
                    for (field_idx, field) in field_list.iter().enumerate() {
                        let hide_key = HideKey::Field {
                            parent_id: object_id,
                            field_idx,
                        };
                        let is_hidden = self.hidden_fields.contains(&hide_key);
                        if is_hidden {
                            if self.show_hidden {
                                let row = self.push_row(None);
                                self.field_row_map.insert(row, (hide_key, true));
                            }
                            continue;
                        }

                        if let FieldValue::ObjectRef { id, .. } = &field.value
                            && visited.contains(id)
                        {
                            self.push_row(None);
                            continue;
                        }

                        let child_path = NavigationPathBuilder::extend(path.clone())
                            .field(FieldIdx(field_idx))
                            .build();

                        let (child_phase, toggleable, is_collection) =
                            if let FieldValue::ObjectRef {
                                id, entry_count, ..
                            } = field.value
                            {
                                self.resolve_at_path(id, entry_count, &child_path)
                            } else {
                                (ExpansionPhase::Collapsed, false, false)
                            };

                        let row = self.push_row(if toggleable {
                            Some(child_path.clone())
                        } else {
                            None
                        });
                        self.field_row_map.insert(row, (hide_key, false));
                        if toggleable && let FieldValue::ObjectRef { id, .. } = field.value {
                            self.kind_map.insert(
                                row,
                                (id, matches!(child_phase, ExpansionPhase::Collapsed)),
                            );
                        }

                        if is_collection {
                            if matches!(child_phase, ExpansionPhase::Expanded)
                                && let FieldValue::ObjectRef {
                                    id,
                                    entry_count: Some(_),
                                    ..
                                } = field.value
                                && let Some(cc) = self.collection_chunks.get(&id)
                            {
                                self.collect_collection_rows(id, cc, &child_path);
                            }
                            continue;
                        }

                        if toggleable && let FieldValue::ObjectRef { id, .. } = field.value {
                            self.collect_object_children_rows(id, &child_path, visited, depth + 1);
                        }
                    }
                }
                self.collect_static_rows(object_id, path, depth);
                visited.remove(&object_id);
            }
        }
    }

    fn collect_collection_rows(
        &mut self,
        collection_id: u64,
        cc: &CollectionChunks,
        parent_path: &NavigationPath,
    ) {
        if let Some(page) = &cc.eager_page {
            for entry in &page.entries {
                self.collect_collection_entry_row(collection_id, entry, parent_path);
            }
        }

        for (offset, _) in compute_chunk_ranges(cc.total_count) {
            if let Some(ChunkState::Loaded(page)) = cc.chunk_pages.get(&offset) {
                let _row = self.push_row(None);
                for entry in &page.entries {
                    self.collect_collection_entry_row(collection_id, entry, parent_path);
                }
            }
        }
    }

    fn collect_collection_entry_row(
        &mut self,
        collection_id: u64,
        entry: &EntryInfo,
        parent_path: &NavigationPath,
    ) {
        let entry_path = NavigationPathBuilder::extend(parent_path.clone())
            .collection_entry(CollectionId(collection_id), EntryIdx(entry.index))
            .build();

        if let FieldValue::ObjectRef {
            id, entry_count, ..
        } = &entry.value
        {
            let (phase, toggleable, is_collection) =
                self.resolve_at_path(*id, *entry_count, &entry_path);

            let row = self.push_row(if toggleable {
                Some(entry_path.clone())
            } else {
                None
            });
            if toggleable {
                self.kind_map
                    .insert(row, (*id, matches!(phase, ExpansionPhase::Collapsed)));
            }

            if is_collection {
                if matches!(phase, ExpansionPhase::Expanded)
                    && *id != collection_id
                    && let Some(nested) = self.collection_chunks.get(id)
                {
                    self.collect_collection_rows(*id, nested, &entry_path);
                }
                return;
            }

            if !toggleable {
                return;
            }

            let mut visited = HashSet::new();
            self.collect_collection_entry_obj_rows(*id, &entry_path, &mut visited, 0);
        } else {
            self.push_row(None);
        }
    }

    fn collect_collection_entry_obj_rows(
        &mut self,
        obj_id: u64,
        path: &NavigationPath,
        visited: &mut HashSet<u64>,
        depth: usize,
    ) {
        if depth >= 16 {
            return;
        }
        match self.phase_for_path(obj_id, path) {
            ExpansionPhase::Collapsed | ExpansionPhase::Failed => {}
            ExpansionPhase::Loading => unreachable!("frozen snapshot"),
            ExpansionPhase::Expanded => {
                let field_list = self
                    .object_fields
                    .get(&obj_id)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                if field_list.is_empty() {
                    self.push_row(None);
                } else {
                    visited.insert(obj_id);
                    for (fi, field) in field_list.iter().enumerate() {
                        if let FieldValue::ObjectRef { id, .. } = &field.value
                            && visited.contains(id)
                        {
                            self.push_row(None);
                            continue;
                        }

                        let child_path = NavigationPathBuilder::extend(path.clone())
                            .field(FieldIdx(fi))
                            .build();

                        let (child_phase, toggleable, is_collection) =
                            if let FieldValue::ObjectRef {
                                id, entry_count, ..
                            } = field.value
                            {
                                self.resolve_at_path(id, entry_count, &child_path)
                            } else {
                                (ExpansionPhase::Collapsed, false, false)
                            };

                        let row = self.push_row(if toggleable {
                            Some(child_path.clone())
                        } else {
                            None
                        });
                        if toggleable && let FieldValue::ObjectRef { id, .. } = field.value {
                            self.kind_map.insert(
                                row,
                                (id, matches!(child_phase, ExpansionPhase::Collapsed)),
                            );
                        }

                        if is_collection {
                            if matches!(child_phase, ExpansionPhase::Expanded)
                                && let FieldValue::ObjectRef {
                                    id,
                                    entry_count: Some(_),
                                    ..
                                } = field.value
                                && let Some(cc) = self.collection_chunks.get(&id)
                            {
                                self.collect_collection_rows(id, cc, &child_path);
                            }
                            continue;
                        }

                        if toggleable && let FieldValue::ObjectRef { id, .. } = field.value {
                            self.collect_collection_entry_obj_rows(
                                id,
                                &child_path,
                                visited,
                                depth + 1,
                            );
                        }
                    }
                    visited.remove(&obj_id);
                }
                self.collect_static_rows(obj_id, path, depth);
            }
        }
    }

    /// Resolves phase, toggleable, and is_collection for an object at a path.
    fn resolve_at_path(
        &self,
        object_id: u64,
        entry_count: Option<u64>,
        path: &NavigationPath,
    ) -> (ExpansionPhase, bool, bool) {
        let is_collection =
            entry_count.is_some() && self.collection_chunks.contains_key(&object_id);
        if is_collection {
            let phase = self.phase_for_path(object_id, path);
            return (phase, true, true);
        }
        let has_data = self.object_fields.contains_key(&object_id)
            || self.object_static_fields.contains_key(&object_id);
        if has_data {
            let phase = self.phase_for_path(object_id, path);
            (phase, true, false)
        } else {
            (ExpansionPhase::Collapsed, false, false)
        }
    }
}

fn collect_row_metadata(item: &PinnedItem) -> RowMetadata {
    let mut row_count = 1; // Header row.
    let mut kind_map = HashMap::new();
    let mut sentinel_map = HashMap::new();
    let mut field_row_map = FieldRowMap::new();
    let mut path_map = PathMap::new();

    match &item.snapshot {
        PinnedSnapshot::Frame {
            variables,
            object_fields,
            object_static_fields,
            collection_chunks,
            truncated,
        } => {
            let start_count = row_count + usize::from(*truncated);
            let object_phases =
                object_phases_for_item(object_fields, object_static_fields, collection_chunks);
            let mut collector = MetadataCollector::new(
                object_fields,
                object_static_fields,
                collection_chunks,
                &item.local_collapsed,
                start_count,
                &item.hidden_fields,
                item.show_hidden,
            );
            collector.collect_frame_rows(variables, 0);
            let (rc, km, sm, fm, mut pm) = collector.into_parts();
            // Prefix path_map with None entries for header + truncated
            // so that path_map[sub_row] aligns with kind_map[sub_row].
            let mut aligned = vec![None; start_count];
            aligned.append(&mut pm);
            row_count = rc;
            kind_map = km;
            sentinel_map = sm;
            field_row_map = fm;
            path_map = aligned;

            debug_assert_eq!(
                row_count,
                render_variable_tree(
                    TreeRoot::Frame {
                        vars: variables,
                        frame_id: 0
                    },
                    object_fields,
                    object_static_fields,
                    collection_chunks,
                    &object_phases,
                    &HashMap::new(),
                    RenderOptions {
                        show_object_ids: false,
                        snapshot_mode: true,
                        show_hidden: item.show_hidden,
                    },
                    Some(&item.hidden_fields),
                    None,
                    Some(&item.local_collapsed),
                )
                .len()
                    + 1
                    + usize::from(*truncated),
                "row count mismatch for item {}",
                item.item_label
            );
        }
        PinnedSnapshot::Subtree {
            root_id,
            object_fields,
            object_static_fields,
            collection_chunks,
            truncated,
        } => {
            let start_count = row_count + usize::from(*truncated);
            let object_phases =
                object_phases_for_item(object_fields, object_static_fields, collection_chunks);
            // Synthetic root path for subtree snapshots.
            let root_path = NavigationPathBuilder::new(FrameId(*root_id), VarIdx(0)).build();
            let mut collector = MetadataCollector::new(
                object_fields,
                object_static_fields,
                collection_chunks,
                &item.local_collapsed,
                start_count,
                &item.hidden_fields,
                item.show_hidden,
            );
            if let Some(root_chunks) = collection_chunks.get(root_id) {
                collector.collect_collection_rows(*root_id, root_chunks, &root_path);
            } else {
                let mut visited = HashSet::new();
                collector.collect_object_children_rows(*root_id, &root_path, &mut visited, 0);
            }
            let (rc, km, sm, fm, mut pm) = collector.into_parts();
            let mut aligned = vec![None; start_count];
            aligned.append(&mut pm);
            row_count = rc;
            kind_map = km;
            sentinel_map = sm;
            field_row_map = fm;
            path_map = aligned;

            debug_assert_eq!(
                row_count,
                render_variable_tree(
                    TreeRoot::Subtree { root_id: *root_id },
                    object_fields,
                    object_static_fields,
                    collection_chunks,
                    &object_phases,
                    &HashMap::new(),
                    RenderOptions {
                        show_object_ids: false,
                        snapshot_mode: true,
                        show_hidden: item.show_hidden,
                    },
                    Some(&item.hidden_fields),
                    None,
                    Some(&item.local_collapsed),
                )
                .len()
                    + 1
                    + usize::from(*truncated),
                "row count mismatch for item {}",
                item.item_label
            );
        }
        PinnedSnapshot::Primitive { .. } | PinnedSnapshot::UnexpandedRef { .. } => {
            row_count += 1;
            path_map.push(None); // header
            path_map.push(None); // content
        }
    }

    row_count += 1; // Separator row.
    path_map.push(None); // Separator has no path.
    (row_count, kind_map, sentinel_map, field_row_map, path_map)
}

impl StatefulWidget for FavoritesPanel<'_> {
    type State = FavoritesPanelState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        state.set_items_len(self.pinned.len());
        let mut all_row_counts = Vec::with_capacity(self.pinned.len());
        let mut all_row_kind_maps = Vec::with_capacity(self.pinned.len());
        let mut all_chunk_sentinel_maps = Vec::with_capacity(self.pinned.len());
        let mut all_field_row_maps = Vec::with_capacity(self.pinned.len());
        let mut all_path_maps = Vec::with_capacity(self.pinned.len());
        for item in self.pinned {
            let (rc, km, sm, fm, pm) = collect_row_metadata(item);
            all_row_counts.push(rc);
            all_row_kind_maps.push(km);
            all_chunk_sentinel_maps.push(sm);
            all_field_row_maps.push(fm);
            all_path_maps.push(pm);
        }
        state.update_row_metadata(
            all_row_counts,
            all_row_kind_maps,
            all_chunk_sentinel_maps,
            all_field_row_maps,
            all_path_maps,
        );

        let border_style = if self.focused {
            THEME.border_focused
        } else {
            THEME.border_unfocused
        };
        let title = format!("Favorites [{}]", self.pinned.len());
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Plain)
            .border_style(border_style)
            .title(title)
            .title_bottom(Line::from(Span::raw("[f] unpin")));
        let inner = block.inner(area);
        block.render(area, buf);

        let mut items: Vec<ListItem<'static>> = Vec::new();

        for item in self.pinned {
            let header_text = match &item.snapshot {
                PinnedSnapshot::Frame { .. } => {
                    format!("[F] {} · {}", item.thread_name, item.frame_label)
                }
                _ => format!(
                    "[V] {} · {} › {}",
                    item.thread_name, item.frame_label, item.item_label
                ),
            };
            items.push(ListItem::new(Line::from(Span::styled(
                header_text,
                THEME.status_bar_bg,
            ))));

            match &item.snapshot {
                PinnedSnapshot::Frame {
                    variables,
                    object_fields,
                    object_static_fields,
                    collection_chunks,
                    truncated,
                } => {
                    if *truncated {
                        items.push(ListItem::new(Line::from(Span::styled(
                            "  [!] snapshot partiel — trop d'objets",
                            THEME.error_indicator,
                        ))));
                    }
                    let object_phases = object_phases_for_item(
                        object_fields,
                        object_static_fields,
                        collection_chunks,
                    );
                    let tree = render_variable_tree(
                        TreeRoot::Frame {
                            vars: variables,
                            frame_id: 0,
                        },
                        object_fields,
                        object_static_fields,
                        collection_chunks,
                        &object_phases,
                        &HashMap::new(),
                        RenderOptions {
                            show_object_ids: self.show_object_ids,
                            snapshot_mode: true,
                            show_hidden: item.show_hidden,
                        },
                        Some(&item.hidden_fields),
                        None,
                        Some(&item.local_collapsed),
                    );
                    items.extend(tree);
                }
                PinnedSnapshot::Subtree {
                    root_id,
                    object_fields,
                    object_static_fields,
                    collection_chunks,
                    truncated,
                } => {
                    if *truncated {
                        items.push(ListItem::new(Line::from(Span::styled(
                            "  [!] snapshot partiel — trop d'objets",
                            THEME.error_indicator,
                        ))));
                    }
                    let object_phases = object_phases_for_item(
                        object_fields,
                        object_static_fields,
                        collection_chunks,
                    );
                    let tree = render_variable_tree(
                        TreeRoot::Subtree { root_id: *root_id },
                        object_fields,
                        object_static_fields,
                        collection_chunks,
                        &object_phases,
                        &HashMap::new(),
                        RenderOptions {
                            show_object_ids: self.show_object_ids,
                            snapshot_mode: true,
                            show_hidden: item.show_hidden,
                        },
                        Some(&item.hidden_fields),
                        None,
                        Some(&item.local_collapsed),
                    );
                    items.extend(tree);
                }
                PinnedSnapshot::UnexpandedRef {
                    class_name,
                    object_id,
                } => {
                    let short = class_name.rsplit('.').next().unwrap_or(class_name);
                    let label = if *object_id == 0 {
                        format!("  {short} [not expanded]")
                    } else {
                        format!("  {short} @ 0x{object_id:X} [not expanded]")
                    };
                    items.push(ListItem::new(Line::from(Span::raw(label))));
                }
                PinnedSnapshot::Primitive { value_label } => {
                    items.push(ListItem::new(Line::from(Span::raw(format!(
                        "  {value_label}"
                    )))));
                }
            }

            items.push(ListItem::new(Line::from("")));
        }

        if items.is_empty() {
            items.push(ListItem::new(Line::from(Span::styled(
                "(no favorites)",
                THEME.null_value,
            ))));
        }

        if !self.pinned.is_empty() {
            state.list_state.select(Some(state.abs_row()));
        } else {
            state.list_state.select(None);
        }

        let list = List::new(items).highlight_style(THEME.selection_bg);
        StatefulWidget::render(list, inner, buf, state.list_state_mut());
    }
}

#[cfg(test)]
mod tests;
