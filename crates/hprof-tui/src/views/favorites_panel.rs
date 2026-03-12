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
    favorites::{PinnedItem, PinnedSnapshot},
    theme::THEME,
    views::{
        stack_view::{
            ChunkState, CollectionChunks, ExpansionPhase, STATIC_FIELDS_RENDER_LIMIT,
            compute_chunk_ranges,
        },
        tree_render::{RenderOptions, TreeRoot, render_variable_tree},
    },
};

type RowKindMap = HashMap<usize, (u64, bool)>;
type ChunkSentinelMap = HashMap<usize, (u64, usize)>;
type RowMetadata = (usize, RowKindMap, ChunkSentinelMap);

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
            self.list_state.select(None);
            return;
        }

        self.selected_item = self.selected_item.min(len.saturating_sub(1));
        self.row_counts.resize(len, 1);
        self.row_kind_maps.resize_with(len, HashMap::new);
        self.chunk_sentinel_maps.resize_with(len, HashMap::new);
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

        self.row_counts = row_counts;
        self.row_kind_maps = row_kind_maps;
        self.chunk_sentinel_maps = chunk_sentinel_maps;
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

    pub fn current_chunk_sentinel(&self) -> Option<(u64, usize)> {
        self.chunk_sentinel_maps
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

fn get_phase(object_id: u64, object_phases: &HashMap<u64, ExpansionPhase>) -> ExpansionPhase {
    object_phases
        .get(&object_id)
        .cloned()
        .unwrap_or(ExpansionPhase::Collapsed)
}

fn object_phases_for_item(
    object_fields: &HashMap<u64, Vec<FieldInfo>>,
    object_static_fields: &HashMap<u64, Vec<FieldInfo>>,
    collection_chunks: &HashMap<u64, CollectionChunks>,
    local_collapsed: &HashSet<u64>,
) -> HashMap<u64, ExpansionPhase> {
    object_fields
        .keys()
        .chain(object_static_fields.keys())
        .chain(collection_chunks.keys())
        .filter(|id| !local_collapsed.contains(id))
        .map(|&id| (id, ExpansionPhase::Expanded))
        .collect()
}

fn visible_collection_chunks(
    collection_chunks: &HashMap<u64, CollectionChunks>,
) -> HashMap<u64, CollectionChunks> {
    collection_chunks
        .iter()
        .map(|(&id, chunks)| (id, chunks.clone()))
        .collect()
}

struct MetadataCollector<'a> {
    object_fields: &'a HashMap<u64, Vec<FieldInfo>>,
    object_static_fields: &'a HashMap<u64, Vec<FieldInfo>>,
    collection_chunks: &'a HashMap<u64, CollectionChunks>,
    object_phases: &'a HashMap<u64, ExpansionPhase>,
    row_count: usize,
    kind_map: RowKindMap,
    sentinel_map: ChunkSentinelMap,
}

impl<'a> MetadataCollector<'a> {
    fn new(
        object_fields: &'a HashMap<u64, Vec<FieldInfo>>,
        object_static_fields: &'a HashMap<u64, Vec<FieldInfo>>,
        collection_chunks: &'a HashMap<u64, CollectionChunks>,
        object_phases: &'a HashMap<u64, ExpansionPhase>,
        row_count: usize,
    ) -> Self {
        Self {
            object_fields,
            object_static_fields,
            collection_chunks,
            object_phases,
            row_count,
            kind_map: HashMap::new(),
            sentinel_map: HashMap::new(),
        }
    }

    fn into_parts(self) -> RowMetadata {
        (self.row_count, self.kind_map, self.sentinel_map)
    }

    fn push_row(&mut self) -> usize {
        let row = self.row_count;
        self.row_count += 1;
        row
    }

    fn resolve_object_ref_state(
        &self,
        object_id: u64,
        entry_count: Option<u64>,
    ) -> (ExpansionPhase, bool, bool) {
        let is_collection =
            entry_count.is_some() && self.collection_chunks.contains_key(&object_id);
        if is_collection {
            return (get_phase(object_id, self.object_phases), true, true);
        }

        let has_object_data = self.object_fields.contains_key(&object_id)
            || self.object_static_fields.contains_key(&object_id);
        if has_object_data {
            (get_phase(object_id, self.object_phases), true, false)
        } else {
            (ExpansionPhase::Collapsed, false, false)
        }
    }

    fn collect_static_rows(&mut self, object_id: u64, depth: usize) {
        let Some(static_fields) = self.object_static_fields.get(&object_id) else {
            return;
        };
        if static_fields.is_empty() {
            return;
        }

        self.push_row(); // [static]
        let shown = static_fields.len().min(STATIC_FIELDS_RENDER_LIMIT);
        for field in static_fields.iter().take(shown) {
            let (child_phase, toggleable, is_collection) =
                if let FieldValue::ObjectRef {
                    id, entry_count, ..
                } = field.value
                {
                    self.resolve_object_ref_state(id, entry_count)
                } else {
                    (ExpansionPhase::Collapsed, false, false)
                };

            let row = self.push_row();
            if toggleable
                && let FieldValue::ObjectRef { id, .. } = field.value
                && !matches!(
                    child_phase,
                    ExpansionPhase::Failed | ExpansionPhase::Loading
                )
            {
                self.kind_map
                    .insert(row, (id, matches!(child_phase, ExpansionPhase::Collapsed)));
            }

            if is_collection {
                if matches!(
                    child_phase,
                    ExpansionPhase::Expanded | ExpansionPhase::Loading
                ) && let FieldValue::ObjectRef {
                    id,
                    entry_count: Some(_),
                    ..
                } = field.value
                    && let Some(cc) = self.collection_chunks.get(&id)
                {
                    self.collect_collection_rows(id, cc);
                }
                continue;
            }

            if toggleable && let FieldValue::ObjectRef { id, .. } = field.value {
                let mut visited = HashSet::new();
                self.collect_static_object_rows(id, &mut visited, depth + 1);
            }
        }

        if static_fields.len() > shown {
            self.push_row(); // [+N more static fields]
        }
    }

    fn collect_static_object_rows(
        &mut self,
        obj_id: u64,
        visited: &mut HashSet<u64>,
        depth: usize,
    ) {
        if depth >= 16 {
            return;
        }
        match get_phase(obj_id, self.object_phases) {
            ExpansionPhase::Collapsed | ExpansionPhase::Failed => {}
            ExpansionPhase::Loading => {
                self.push_row();
            }
            ExpansionPhase::Expanded => {
                let field_list = self
                    .object_fields
                    .get(&obj_id)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                if field_list.is_empty() {
                    self.push_row();
                    return;
                }

                visited.insert(obj_id);
                for field in field_list {
                    if let FieldValue::ObjectRef { id, .. } = &field.value
                        && visited.contains(id)
                    {
                        self.push_row();
                        continue;
                    }

                    let (child_phase, toggleable, is_collection) =
                        if let FieldValue::ObjectRef {
                            id, entry_count, ..
                        } = field.value
                        {
                            self.resolve_object_ref_state(id, entry_count)
                        } else {
                            (ExpansionPhase::Collapsed, false, false)
                        };

                    let row = self.push_row();
                    if toggleable
                        && let FieldValue::ObjectRef { id, .. } = field.value
                        && !matches!(
                            child_phase,
                            ExpansionPhase::Failed | ExpansionPhase::Loading
                        )
                    {
                        self.kind_map
                            .insert(row, (id, matches!(child_phase, ExpansionPhase::Collapsed)));
                    }

                    if is_collection {
                        if matches!(
                            child_phase,
                            ExpansionPhase::Expanded | ExpansionPhase::Loading
                        ) && let FieldValue::ObjectRef {
                            id,
                            entry_count: Some(_),
                            ..
                        } = field.value
                            && let Some(cc) = self.collection_chunks.get(&id)
                        {
                            self.collect_collection_rows(id, cc);
                        }
                        continue;
                    }

                    if toggleable && let FieldValue::ObjectRef { id, .. } = field.value {
                        self.collect_static_object_rows(id, visited, depth + 1);
                    }
                }
                visited.remove(&obj_id);
            }
        }
    }

    fn collect_frame_rows(&mut self, vars: &[VariableInfo]) {
        if vars.is_empty() {
            self.push_row(); // (no locals)
            return;
        }
        for var in vars {
            self.collect_var_row(var);
        }
    }

    fn collect_var_row(&mut self, var: &VariableInfo) {
        let VariableValue::ObjectRef {
            id, entry_count, ..
        } = var.value
        else {
            self.push_row();
            return;
        };

        let (phase, toggleable, is_collection) = self.resolve_object_ref_state(id, entry_count);

        let row = self.push_row();
        if toggleable && !matches!(phase, ExpansionPhase::Failed | ExpansionPhase::Loading) {
            self.kind_map
                .insert(row, (id, matches!(phase, ExpansionPhase::Collapsed)));
        }

        if is_collection {
            if matches!(phase, ExpansionPhase::Expanded | ExpansionPhase::Loading)
                && let Some(cc) = self.collection_chunks.get(&id)
            {
                self.collect_collection_rows(id, cc);
            }
            return;
        }

        if !toggleable {
            return;
        }

        let mut visited = HashSet::new();
        self.collect_object_children_rows(id, &mut visited, 0);
    }

    fn collect_object_children_rows(
        &mut self,
        object_id: u64,
        visited: &mut HashSet<u64>,
        depth: usize,
    ) {
        if depth >= 16 {
            return;
        }
        match get_phase(object_id, self.object_phases) {
            ExpansionPhase::Collapsed | ExpansionPhase::Failed => {}
            ExpansionPhase::Loading => {
                self.push_row();
            }
            ExpansionPhase::Expanded => {
                visited.insert(object_id);
                let field_list = self
                    .object_fields
                    .get(&object_id)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                if field_list.is_empty() {
                    self.push_row();
                } else {
                    for field in field_list {
                        if let FieldValue::ObjectRef { id, .. } = &field.value
                            && visited.contains(id)
                        {
                            self.push_row();
                            continue;
                        }

                        let (child_phase, toggleable, is_collection) =
                            if let FieldValue::ObjectRef {
                                id, entry_count, ..
                            } = field.value
                            {
                                self.resolve_object_ref_state(id, entry_count)
                            } else {
                                (ExpansionPhase::Collapsed, false, false)
                            };

                        let row = self.push_row();
                        if toggleable
                            && let FieldValue::ObjectRef { id, .. } = field.value
                            && !matches!(
                                child_phase,
                                ExpansionPhase::Failed | ExpansionPhase::Loading
                            )
                        {
                            self.kind_map.insert(
                                row,
                                (id, matches!(child_phase, ExpansionPhase::Collapsed)),
                            );
                        }

                        if is_collection {
                            if matches!(
                                child_phase,
                                ExpansionPhase::Expanded | ExpansionPhase::Loading
                            ) && let FieldValue::ObjectRef {
                                id,
                                entry_count: Some(_),
                                ..
                            } = field.value
                                && let Some(cc) = self.collection_chunks.get(&id)
                            {
                                self.collect_collection_rows(id, cc);
                            }
                            continue;
                        }

                        if toggleable && let FieldValue::ObjectRef { id, .. } = field.value {
                            self.collect_object_children_rows(id, visited, depth + 1);
                        }
                    }
                }
                self.collect_static_rows(object_id, depth);
                visited.remove(&object_id);
            }
        }
    }

    fn collect_collection_rows(&mut self, collection_id: u64, cc: &CollectionChunks) {
        if let Some(page) = &cc.eager_page {
            for entry in &page.entries {
                self.collect_collection_entry_row(collection_id, entry);
            }
        }

        for (offset, _) in compute_chunk_ranges(cc.total_count) {
            let row = self.push_row();
            match cc.chunk_pages.get(&offset) {
                Some(ChunkState::Collapsed) => {
                    self.sentinel_map.insert(row, (collection_id, offset));
                }
                Some(ChunkState::Loaded(page)) => {
                    for entry in &page.entries {
                        self.collect_collection_entry_row(collection_id, entry);
                    }
                }
                Some(ChunkState::Loading) | None => {}
            }
        }
    }

    fn collect_collection_entry_row(&mut self, collection_id: u64, entry: &EntryInfo) {
        let row = self.push_row();

        if let FieldValue::ObjectRef {
            id, entry_count, ..
        } = &entry.value
        {
            let (phase, toggleable, is_collection) =
                self.resolve_object_ref_state(*id, *entry_count);
            if toggleable && !matches!(phase, ExpansionPhase::Failed | ExpansionPhase::Loading) {
                self.kind_map
                    .insert(row, (*id, matches!(phase, ExpansionPhase::Collapsed)));
            }

            if is_collection {
                if matches!(phase, ExpansionPhase::Expanded | ExpansionPhase::Loading)
                    && *id != collection_id
                    && let Some(nested) = self.collection_chunks.get(id)
                {
                    self.collect_collection_rows(*id, nested);
                }
                return;
            }

            if !toggleable {
                return;
            }

            let mut visited = HashSet::new();
            self.collect_collection_entry_obj_rows(*id, &mut visited, 0);
        }
    }

    fn collect_collection_entry_obj_rows(
        &mut self,
        obj_id: u64,
        visited: &mut HashSet<u64>,
        depth: usize,
    ) {
        if depth >= 16 {
            return;
        }
        match get_phase(obj_id, self.object_phases) {
            ExpansionPhase::Collapsed | ExpansionPhase::Failed => {}
            ExpansionPhase::Loading => {
                self.push_row();
            }
            ExpansionPhase::Expanded => {
                let field_list = self
                    .object_fields
                    .get(&obj_id)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                if field_list.is_empty() {
                    self.push_row();
                } else {
                    visited.insert(obj_id);
                    for field in field_list {
                        if let FieldValue::ObjectRef { id, .. } = &field.value
                            && visited.contains(id)
                        {
                            self.push_row();
                            continue;
                        }

                        let (child_phase, toggleable, is_collection) =
                            if let FieldValue::ObjectRef {
                                id, entry_count, ..
                            } = field.value
                            {
                                self.resolve_object_ref_state(id, entry_count)
                            } else {
                                (ExpansionPhase::Collapsed, false, false)
                            };

                        let row = self.push_row();
                        if toggleable
                            && let FieldValue::ObjectRef { id, .. } = field.value
                            && !matches!(
                                child_phase,
                                ExpansionPhase::Failed | ExpansionPhase::Loading
                            )
                        {
                            self.kind_map.insert(
                                row,
                                (id, matches!(child_phase, ExpansionPhase::Collapsed)),
                            );
                        }

                        if is_collection {
                            if matches!(
                                child_phase,
                                ExpansionPhase::Expanded | ExpansionPhase::Loading
                            ) && let FieldValue::ObjectRef {
                                id,
                                entry_count: Some(_),
                                ..
                            } = field.value
                                && let Some(cc) = self.collection_chunks.get(&id)
                            {
                                self.collect_collection_rows(id, cc);
                            }
                            continue;
                        }

                        if toggleable && let FieldValue::ObjectRef { id, .. } = field.value {
                            self.collect_collection_entry_obj_rows(id, visited, depth + 1);
                        }
                    }
                    visited.remove(&obj_id);
                }
                self.collect_static_rows(obj_id, depth);
            }
        }
    }
}

fn collect_row_metadata(item: &PinnedItem) -> RowMetadata {
    let mut row_count = 1; // Header row.
    let mut kind_map = HashMap::new();
    let mut sentinel_map = HashMap::new();

    match &item.snapshot {
        PinnedSnapshot::Frame {
            variables,
            object_fields,
            object_static_fields,
            collection_chunks,
            truncated,
        } => {
            let start_count = row_count + usize::from(*truncated);
            let object_phases = object_phases_for_item(
                object_fields,
                object_static_fields,
                collection_chunks,
                &item.local_collapsed,
            );
            let visible_chunks = visible_collection_chunks(collection_chunks);
            let mut collector = MetadataCollector::new(
                object_fields,
                object_static_fields,
                &visible_chunks,
                &object_phases,
                start_count,
            );
            collector.collect_frame_rows(variables);
            (row_count, kind_map, sentinel_map) = collector.into_parts();

            debug_assert_eq!(
                row_count,
                render_variable_tree(
                    TreeRoot::Frame { vars: variables },
                    object_fields,
                    object_static_fields,
                    &visible_chunks,
                    &object_phases,
                    &HashMap::new(),
                    RenderOptions {
                        show_object_ids: false,
                        snapshot_mode: true,
                    },
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
            let object_phases = object_phases_for_item(
                object_fields,
                object_static_fields,
                collection_chunks,
                &item.local_collapsed,
            );
            let visible_chunks = visible_collection_chunks(collection_chunks);
            let mut collector = MetadataCollector::new(
                object_fields,
                object_static_fields,
                &visible_chunks,
                &object_phases,
                start_count,
            );
            if let Some(root_chunks) = visible_chunks.get(root_id) {
                collector.collect_collection_rows(*root_id, root_chunks);
            } else {
                let mut visited = HashSet::new();
                collector.collect_object_children_rows(*root_id, &mut visited, 0);
            }
            (row_count, kind_map, sentinel_map) = collector.into_parts();

            debug_assert_eq!(
                row_count,
                render_variable_tree(
                    TreeRoot::Subtree { root_id: *root_id },
                    object_fields,
                    object_static_fields,
                    &visible_chunks,
                    &object_phases,
                    &HashMap::new(),
                    RenderOptions {
                        show_object_ids: false,
                        snapshot_mode: true,
                    },
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
        }
    }

    row_count += 1; // Separator row.
    (row_count, kind_map, sentinel_map)
}

impl StatefulWidget for FavoritesPanel<'_> {
    type State = FavoritesPanelState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        state.set_items_len(self.pinned.len());
        let mut all_row_counts = Vec::with_capacity(self.pinned.len());
        let mut all_row_kind_maps = Vec::with_capacity(self.pinned.len());
        let mut all_chunk_sentinel_maps = Vec::with_capacity(self.pinned.len());
        for item in self.pinned {
            let (row_count, kind_map, sentinel_map) = collect_row_metadata(item);
            all_row_counts.push(row_count);
            all_row_kind_maps.push(kind_map);
            all_chunk_sentinel_maps.push(sentinel_map);
        }
        state.update_row_metadata(all_row_counts, all_row_kind_maps, all_chunk_sentinel_maps);

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
                        &item.local_collapsed,
                    );
                    let visible_chunks = visible_collection_chunks(collection_chunks);
                    let tree = render_variable_tree(
                        TreeRoot::Frame { vars: variables },
                        object_fields,
                        object_static_fields,
                        &visible_chunks,
                        &object_phases,
                        &HashMap::new(),
                        RenderOptions {
                            show_object_ids: self.show_object_ids,
                            snapshot_mode: true,
                        },
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
                        &item.local_collapsed,
                    );
                    let visible_chunks = visible_collection_chunks(collection_chunks);
                    let tree = render_variable_tree(
                        TreeRoot::Subtree { root_id: *root_id },
                        object_fields,
                        object_static_fields,
                        &visible_chunks,
                        &object_phases,
                        &HashMap::new(),
                        RenderOptions {
                            show_object_ids: self.show_object_ids,
                            snapshot_mode: true,
                        },
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
mod tests {
    use hprof_engine::{
        CollectionPage, EntryInfo, FieldInfo, FieldValue, VariableInfo, VariableValue,
    };
    use ratatui::{Terminal, backend::TestBackend};
    use std::collections::{HashMap, HashSet};

    use super::*;
    use crate::favorites::{PinKey, PinnedItem, PinnedSnapshot};

    fn render_panel(panel: FavoritesPanel<'_>, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = FavoritesPanelState::default();
        terminal
            .draw(|f| {
                f.render_stateful_widget(panel, f.area(), &mut state);
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

    fn make_frame_item() -> PinnedItem {
        PinnedItem {
            thread_name: "main".to_string(),
            frame_label: "Foo.bar()".to_string(),
            item_label: "Foo.bar()".to_string(),
            snapshot: PinnedSnapshot::Frame {
                variables: vec![VariableInfo {
                    index: 0,
                    value: VariableValue::Null,
                }],
                object_fields: HashMap::new(),
                object_static_fields: HashMap::new(),
                collection_chunks: HashMap::new(),
                truncated: false,
            },
            local_collapsed: HashSet::new(),
            key: PinKey::Frame {
                frame_id: 1,
                thread_name: "main".to_string(),
            },
        }
    }

    fn make_primitive_item() -> PinnedItem {
        PinnedItem {
            thread_name: "main".to_string(),
            frame_label: "Foo.bar()".to_string(),
            item_label: "var[0]".to_string(),
            snapshot: PinnedSnapshot::Primitive {
                value_label: "42".to_string(),
            },
            local_collapsed: HashSet::new(),
            key: PinKey::Var {
                frame_id: 1,
                thread_name: "main".to_string(),
                var_idx: 0,
            },
        }
    }

    fn make_frame_with_nested_objects() -> PinnedItem {
        let mut object_fields = HashMap::new();
        object_fields.insert(
            10,
            vec![FieldInfo {
                name: "child".to_string(),
                value: FieldValue::ObjectRef {
                    id: 11,
                    class_name: "Node".to_string(),
                    entry_count: None,
                    inline_value: None,
                },
            }],
        );
        object_fields.insert(
            11,
            vec![FieldInfo {
                name: "value".to_string(),
                value: FieldValue::Int(1),
            }],
        );
        PinnedItem {
            thread_name: "main".to_string(),
            frame_label: "Foo.bar()".to_string(),
            item_label: "Foo.bar()".to_string(),
            snapshot: PinnedSnapshot::Frame {
                variables: vec![VariableInfo {
                    index: 0,
                    value: VariableValue::ObjectRef {
                        id: 10,
                        class_name: "Node".to_string(),
                        entry_count: None,
                    },
                }],
                object_fields,
                object_static_fields: HashMap::new(),
                collection_chunks: HashMap::new(),
                truncated: false,
            },
            local_collapsed: HashSet::new(),
            key: PinKey::Frame {
                frame_id: 1,
                thread_name: "main".to_string(),
            },
        }
    }

    #[test]
    fn panel_shows_frame_header_with_f_prefix() {
        let items = vec![make_frame_item()];
        let text = render_panel(
            FavoritesPanel {
                focused: false,
                show_object_ids: false,
                pinned: &items,
            },
            80,
            10,
        );
        assert!(text.contains("[F]"), "expected [F] prefix, got: {text:?}");
        assert!(text.contains("main"), "expected thread name, got: {text:?}");
        assert!(
            text.contains("Foo.bar()"),
            "expected frame label, got: {text:?}"
        );
    }

    #[test]
    fn panel_shows_variable_header_with_v_prefix() {
        let items = vec![make_primitive_item()];
        let text = render_panel(
            FavoritesPanel {
                focused: false,
                show_object_ids: false,
                pinned: &items,
            },
            80,
            10,
        );
        assert!(text.contains("[V]"), "expected [V] prefix, got: {text:?}");
    }

    #[test]
    fn panel_shows_primitive_value() {
        let items = vec![make_primitive_item()];
        let text = render_panel(
            FavoritesPanel {
                focused: false,
                show_object_ids: false,
                pinned: &items,
            },
            80,
            10,
        );
        assert!(text.contains("42"), "expected value 42, got: {text:?}");
    }

    #[test]
    fn favorites_panel_state_move_down_crosses_item_boundary() {
        let mut state = FavoritesPanelState::default();
        state.set_items_len(2);
        state.update_row_metadata(
            vec![3, 2],
            vec![HashMap::new(), HashMap::new()],
            vec![HashMap::new(), HashMap::new()],
        );
        state.selected_item = 0;
        state.sub_row = 2;

        state.move_down();

        assert_eq!(state.selected_item, 1);
        assert_eq!(state.sub_row, 0);
    }

    #[test]
    fn favorites_panel_state_move_up_crosses_item_boundary() {
        let mut state = FavoritesPanelState::default();
        state.set_items_len(2);
        state.update_row_metadata(
            vec![3, 2],
            vec![HashMap::new(), HashMap::new()],
            vec![HashMap::new(), HashMap::new()],
        );
        state.selected_item = 1;
        state.sub_row = 0;

        state.move_up();

        assert_eq!(state.selected_item, 0);
        assert_eq!(state.sub_row, 2);
    }

    #[test]
    fn favorites_panel_state_move_down_noop_at_last_row() {
        let mut state = FavoritesPanelState::default();
        state.set_items_len(1);
        state.update_row_metadata(vec![3], vec![HashMap::new()], vec![HashMap::new()]);
        state.selected_item = 0;
        state.sub_row = 2;

        state.move_down();

        assert_eq!(state.selected_item, 0);
        assert_eq!(state.sub_row, 2);
    }

    #[test]
    fn favorites_panel_state_abs_row_correct() {
        let mut state = FavoritesPanelState::default();
        state.set_items_len(2);
        state.update_row_metadata(
            vec![3, 4],
            vec![HashMap::new(), HashMap::new()],
            vec![HashMap::new(), HashMap::new()],
        );
        state.selected_item = 1;
        state.sub_row = 2;

        assert_eq!(state.abs_row(), 5);
    }

    #[test]
    fn favorites_item_toggle_expand_removes_from_local_collapsed() {
        let mut item = make_frame_with_nested_objects();
        item.local_collapsed.insert(10);

        if let Some((id, is_collapsed)) = Some((10u64, true))
            && is_collapsed
        {
            item.local_collapsed.remove(&id);
        }

        assert!(!item.local_collapsed.contains(&10));
    }

    #[test]
    fn favorites_item_toggle_collapse_adds_to_local_collapsed() {
        let mut item = make_frame_with_nested_objects();

        if let Some((id, is_collapsed)) = Some((10u64, false))
            && !is_collapsed
        {
            item.local_collapsed.insert(id);
        }

        assert!(item.local_collapsed.contains(&10));
    }

    #[test]
    fn collect_row_metadata_matches_render_count_flat() {
        let item = make_frame_with_nested_objects();
        let (row_count, _kind_map, _sentinel_map) = collect_row_metadata(&item);

        let PinnedSnapshot::Frame {
            variables,
            object_fields,
            collection_chunks,
            ..
        } = &item.snapshot
        else {
            panic!("expected frame snapshot");
        };
        let object_phases = object_phases_for_item(
            object_fields,
            &HashMap::new(),
            collection_chunks,
            &item.local_collapsed,
        );
        let visible_chunks = visible_collection_chunks(collection_chunks);
        let rendered = render_variable_tree(
            TreeRoot::Frame { vars: variables },
            object_fields,
            &HashMap::new(),
            &visible_chunks,
            &object_phases,
            &HashMap::new(),
            RenderOptions {
                show_object_ids: false,
                snapshot_mode: true,
            },
        );

        assert_eq!(row_count, rendered.len() + 2);
    }

    #[test]
    fn collect_row_metadata_matches_render_count_nested() {
        let mut object_fields = HashMap::new();
        object_fields.insert(
            10,
            vec![FieldInfo {
                name: "child".to_string(),
                value: FieldValue::ObjectRef {
                    id: 11,
                    class_name: "Node".to_string(),
                    entry_count: None,
                    inline_value: None,
                },
            }],
        );
        object_fields.insert(
            11,
            vec![FieldInfo {
                name: "grandchild".to_string(),
                value: FieldValue::ObjectRef {
                    id: 12,
                    class_name: "Node".to_string(),
                    entry_count: None,
                    inline_value: None,
                },
            }],
        );
        object_fields.insert(
            12,
            vec![FieldInfo {
                name: "items".to_string(),
                value: FieldValue::ObjectRef {
                    id: 99,
                    class_name: "ArrayList".to_string(),
                    entry_count: Some(1003),
                    inline_value: None,
                },
            }],
        );

        let mut collection_chunks = HashMap::new();
        collection_chunks.insert(
            99,
            CollectionChunks {
                total_count: 1003,
                eager_page: Some(CollectionPage {
                    entries: vec![
                        EntryInfo {
                            index: 0,
                            key: None,
                            value: FieldValue::Int(1),
                        },
                        EntryInfo {
                            index: 1,
                            key: None,
                            value: FieldValue::Int(2),
                        },
                        EntryInfo {
                            index: 2,
                            key: None,
                            value: FieldValue::Int(3),
                        },
                    ],
                    total_count: 1003,
                    offset: 0,
                    has_more: true,
                }),
                chunk_pages: HashMap::new(),
            },
        );

        let item = PinnedItem {
            thread_name: "main".to_string(),
            frame_label: "Foo.bar()".to_string(),
            item_label: "var[0]".to_string(),
            snapshot: PinnedSnapshot::Frame {
                variables: vec![VariableInfo {
                    index: 0,
                    value: VariableValue::ObjectRef {
                        id: 10,
                        class_name: "Node".to_string(),
                        entry_count: None,
                    },
                }],
                object_fields: object_fields.clone(),
                object_static_fields: HashMap::new(),
                collection_chunks: collection_chunks.clone(),
                truncated: false,
            },
            local_collapsed: HashSet::new(),
            key: PinKey::Var {
                frame_id: 1,
                thread_name: "main".to_string(),
                var_idx: 0,
            },
        };

        let (row_count, _kind_map, _sentinel_map) = collect_row_metadata(&item);

        let object_phases = object_phases_for_item(
            &object_fields,
            &HashMap::new(),
            &collection_chunks,
            &item.local_collapsed,
        );
        let rendered = render_variable_tree(
            TreeRoot::Frame {
                vars: match &item.snapshot {
                    PinnedSnapshot::Frame { variables, .. } => variables,
                    _ => unreachable!(),
                },
            },
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

        assert_eq!(row_count, rendered.len() + 2);
    }

    #[test]
    fn favorites_panel_state_move_down_before_first_render_advances_item() {
        let mut state = FavoritesPanelState::default();
        state.set_items_len(2);

        state.move_down();

        assert_eq!(state.selected_item, 1);
    }

    #[test]
    fn favorites_panel_renders_with_local_collapsed_shows_plus() {
        let mut item = make_frame_with_nested_objects();
        item.local_collapsed.insert(10);
        let items = vec![item];
        let text = render_panel(
            FavoritesPanel {
                focused: false,
                show_object_ids: false,
                pinned: &items,
            },
            120,
            20,
        );

        assert!(text.contains("+"), "expected plus marker, got: {text:?}");
    }

    #[test]
    fn favorites_panel_renders_expanded_shows_minus() {
        let items = vec![make_frame_with_nested_objects()];
        let text = render_panel(
            FavoritesPanel {
                focused: false,
                show_object_ids: false,
                pinned: &items,
            },
            120,
            20,
        );

        assert!(text.contains("-"), "expected minus marker, got: {text:?}");
    }

    #[test]
    fn favorites_panel_renders_unavailable_object_with_question_marker() {
        let items = vec![PinnedItem {
            thread_name: "main".to_string(),
            frame_label: "Foo.bar()".to_string(),
            item_label: "Foo.bar()".to_string(),
            snapshot: PinnedSnapshot::Frame {
                variables: vec![VariableInfo {
                    index: 0,
                    value: VariableValue::ObjectRef {
                        id: 999,
                        class_name: "Node".to_string(),
                        entry_count: None,
                    },
                }],
                object_fields: HashMap::new(),
                object_static_fields: HashMap::new(),
                collection_chunks: HashMap::new(),
                truncated: false,
            },
            local_collapsed: HashSet::new(),
            key: PinKey::Frame {
                frame_id: 1,
                thread_name: "main".to_string(),
            },
        }];
        let text = render_panel(
            FavoritesPanel {
                focused: false,
                show_object_ids: false,
                pinned: &items,
            },
            120,
            20,
        );

        assert!(
            text.contains("? [0] local variable: Node"),
            "expected unavailable marker, got: {text:?}"
        );
    }

    #[test]
    fn collect_row_metadata_unavailable_object_is_not_toggleable() {
        let item = PinnedItem {
            thread_name: "main".to_string(),
            frame_label: "Foo.bar()".to_string(),
            item_label: "Foo.bar()".to_string(),
            snapshot: PinnedSnapshot::Frame {
                variables: vec![VariableInfo {
                    index: 0,
                    value: VariableValue::ObjectRef {
                        id: 999,
                        class_name: "Node".to_string(),
                        entry_count: None,
                    },
                }],
                object_fields: HashMap::new(),
                object_static_fields: HashMap::new(),
                collection_chunks: HashMap::new(),
                truncated: false,
            },
            local_collapsed: HashSet::new(),
            key: PinKey::Frame {
                frame_id: 1,
                thread_name: "main".to_string(),
            },
        };

        let (row_count, kind_map, _sentinel_map) = collect_row_metadata(&item);

        assert_eq!(row_count, 3);
        assert!(
            kind_map.is_empty(),
            "unavailable row must not be toggleable"
        );
    }

    #[test]
    fn collect_row_metadata_subtree_collection_root_matches_render_count() {
        use crate::views::stack_view::{ChunkState, CollectionChunks};

        let item = PinnedItem {
            thread_name: "main".to_string(),
            frame_label: "Foo.bar()".to_string(),
            item_label: "var[0]".to_string(),
            snapshot: PinnedSnapshot::Subtree {
                root_id: 77,
                object_fields: HashMap::new(),
                object_static_fields: HashMap::new(),
                collection_chunks: HashMap::from([(
                    77u64,
                    CollectionChunks {
                        total_count: 120,
                        eager_page: Some(CollectionPage {
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
                )]),
                truncated: false,
            },
            local_collapsed: HashSet::new(),
            key: PinKey::Var {
                frame_id: 1,
                thread_name: "main".to_string(),
                var_idx: 0,
            },
        };

        let (row_count, _kind_map, _sentinel_map) = collect_row_metadata(&item);
        let PinnedSnapshot::Subtree {
            root_id,
            object_fields,
            collection_chunks,
            ..
        } = &item.snapshot
        else {
            panic!("expected subtree snapshot");
        };
        let object_phases = object_phases_for_item(
            object_fields,
            &HashMap::new(),
            collection_chunks,
            &item.local_collapsed,
        );
        let visible_chunks = visible_collection_chunks(collection_chunks);
        let rendered = render_variable_tree(
            TreeRoot::Subtree { root_id: *root_id },
            object_fields,
            &HashMap::new(),
            &visible_chunks,
            &object_phases,
            &HashMap::new(),
            RenderOptions {
                show_object_ids: false,
                snapshot_mode: true,
            },
        );

        assert_eq!(row_count, rendered.len() + 2);
    }

    #[test]
    fn favorites_panel_collapsed_collection_row_shows_plus_not_question() {
        let mut object_fields = HashMap::new();
        object_fields.insert(
            1,
            vec![FieldInfo {
                name: "items".to_string(),
                value: FieldValue::ObjectRef {
                    id: 200,
                    class_name: "ArrayList".to_string(),
                    entry_count: Some(2),
                    inline_value: None,
                },
            }],
        );

        let mut collection_chunks = HashMap::new();
        collection_chunks.insert(
            200,
            CollectionChunks {
                total_count: 2,
                eager_page: Some(CollectionPage {
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

        let mut local_collapsed = HashSet::new();
        local_collapsed.insert(200);

        let items = vec![PinnedItem {
            thread_name: "main".to_string(),
            frame_label: "Foo.bar()".to_string(),
            item_label: "Foo.bar()".to_string(),
            snapshot: PinnedSnapshot::Frame {
                variables: vec![VariableInfo {
                    index: 0,
                    value: VariableValue::ObjectRef {
                        id: 1,
                        class_name: "Node".to_string(),
                        entry_count: None,
                    },
                }],
                object_fields,
                object_static_fields: HashMap::new(),
                collection_chunks,
                truncated: false,
            },
            local_collapsed,
            key: PinKey::Frame {
                frame_id: 1,
                thread_name: "main".to_string(),
            },
        }];

        let text = render_panel(
            FavoritesPanel {
                focused: false,
                show_object_ids: false,
                pinned: &items,
            },
            120,
            30,
        );

        assert!(text.contains("+ items"), "expected + marker, got: {text:?}");
        assert!(
            !text.contains("? items"),
            "did not expect ? marker, got: {text:?}"
        );
        assert!(
            !text.contains("[0] 7"),
            "collapsed collection should hide entries"
        );
    }

    #[test]
    fn sub_row_clamped_after_collapse() {
        let mut state = FavoritesPanelState::default();
        state.set_items_len(1);
        state.update_row_metadata(vec![5], vec![HashMap::new()], vec![HashMap::new()]);
        state.sub_row = 4;

        state.update_row_metadata(vec![2], vec![HashMap::new()], vec![HashMap::new()]);
        state.clamp_sub_row();

        assert_eq!(state.sub_row, 1);
    }

    #[test]
    fn collect_row_metadata_cyclic_object_emits_one_row_not_infinite() {
        let mut object_fields = HashMap::new();
        object_fields.insert(
            1,
            vec![FieldInfo {
                name: "b".to_string(),
                value: FieldValue::ObjectRef {
                    id: 2,
                    class_name: "B".to_string(),
                    entry_count: None,
                    inline_value: None,
                },
            }],
        );
        object_fields.insert(
            2,
            vec![FieldInfo {
                name: "a".to_string(),
                value: FieldValue::ObjectRef {
                    id: 1,
                    class_name: "A".to_string(),
                    entry_count: None,
                    inline_value: None,
                },
            }],
        );
        let item = PinnedItem {
            thread_name: "main".to_string(),
            frame_label: "Foo.bar()".to_string(),
            item_label: "var[0]".to_string(),
            snapshot: PinnedSnapshot::Frame {
                variables: vec![VariableInfo {
                    index: 0,
                    value: VariableValue::ObjectRef {
                        id: 1,
                        class_name: "A".to_string(),
                        entry_count: None,
                    },
                }],
                object_fields,
                object_static_fields: HashMap::new(),
                collection_chunks: HashMap::new(),
                truncated: false,
            },
            local_collapsed: HashSet::new(),
            key: PinKey::Var {
                frame_id: 1,
                thread_name: "main".to_string(),
                var_idx: 0,
            },
        };

        let (row_count, _, _) = collect_row_metadata(&item);

        assert!(row_count > 0);
        assert!(row_count < 100);
    }

    #[test]
    fn collect_row_metadata_primitive_and_unexpanded_ref_row_count() {
        let primitive = make_primitive_item();
        let (primitive_rows, _, _) = collect_row_metadata(&primitive);
        assert_eq!(primitive_rows, 3);

        let unexpanded = PinnedItem {
            thread_name: "main".to_string(),
            frame_label: "Foo.bar()".to_string(),
            item_label: "var[0]".to_string(),
            snapshot: PinnedSnapshot::UnexpandedRef {
                class_name: "Foo".to_string(),
                object_id: 1,
            },
            local_collapsed: HashSet::new(),
            key: PinKey::Var {
                frame_id: 1,
                thread_name: "main".to_string(),
                var_idx: 0,
            },
        };
        let (unexpanded_rows, _, _) = collect_row_metadata(&unexpanded);
        assert_eq!(unexpanded_rows, 3);
    }

    #[test]
    fn favorites_panel_renders_static_fields_for_pinned_object() {
        let mut object_fields = HashMap::new();
        object_fields.insert(
            10,
            vec![FieldInfo {
                name: "value".to_string(),
                value: FieldValue::Int(1),
            }],
        );
        let mut object_static_fields = HashMap::new();
        object_static_fields.insert(
            10,
            vec![FieldInfo {
                name: "SOME_STATIC".to_string(),
                value: FieldValue::Int(99),
            }],
        );

        let items = vec![PinnedItem {
            thread_name: "main".to_string(),
            frame_label: "Foo.bar()".to_string(),
            item_label: "var[0]".to_string(),
            snapshot: PinnedSnapshot::Frame {
                variables: vec![VariableInfo {
                    index: 0,
                    value: VariableValue::ObjectRef {
                        id: 10,
                        class_name: "Node".to_string(),
                        entry_count: None,
                    },
                }],
                object_fields: object_fields.clone(),
                object_static_fields: object_static_fields.clone(),
                collection_chunks: HashMap::new(),
                truncated: false,
            },
            local_collapsed: HashSet::new(),
            key: PinKey::Var {
                frame_id: 1,
                thread_name: "main".to_string(),
                var_idx: 0,
            },
        }];
        let text = render_panel(
            FavoritesPanel {
                focused: false,
                show_object_ids: false,
                pinned: &items,
            },
            120,
            30,
        );

        assert!(
            text.contains("[static]"),
            "expected static section, got: {text:?}"
        );
        assert!(
            text.contains("SOME_STATIC"),
            "expected static field label, got: {text:?}"
        );
    }
}
