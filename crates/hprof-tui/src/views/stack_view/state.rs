//! [`StackState`] — mutable state for the stack frame panel.

use std::collections::{HashMap, HashSet};

use hprof_engine::{FieldInfo, FieldValue, FrameInfo, VariableInfo, VariableValue};
use ratatui::{
    text::{Line, Span},
    widgets::{ListItem, ListState},
};

use crate::theme::THEME;

use super::expansion::ExpansionRegistry;
use super::format::{collect_descendants, compute_chunk_ranges, format_frame_label};
use super::types::{ChunkState, CollectionChunks, ExpansionPhase, StackCursor};

/// State for the stack frame panel.
pub struct StackState {
    // === Frames & Vars ===
    pub(super) frames: Vec<FrameInfo>,
    /// Vars per frame_id — populated on demand by `App` calling the engine.
    pub(super) vars: HashMap<u64, Vec<VariableInfo>>,
    pub(super) expanded: HashSet<u64>,
    // === Cursor & Navigation ===
    pub(super) cursor: StackCursor,
    pub(super) list_state: ListState,
    /// Visible height of the stack panel (set during render).
    pub(super) visible_height: u16,
    // === Expansion (delegated) ===
    pub(crate) expansion: ExpansionRegistry,
}

impl StackState {
    /// Creates a new state for the given frames. Selects first frame.
    pub fn new(frames: Vec<FrameInfo>) -> Self {
        let cursor = if frames.is_empty() {
            StackCursor::NoFrames
        } else {
            StackCursor::OnFrame(0)
        };
        let mut list_state = ListState::default();
        if !frames.is_empty() {
            list_state.select(Some(0));
        }
        Self {
            frames,
            vars: HashMap::new(),
            expanded: HashSet::new(),
            cursor,
            list_state,
            visible_height: 0,
            expansion: ExpansionRegistry::new(),
        }
    }

    /// Returns the frame_id currently selected, if any.
    pub fn selected_frame_id(&self) -> Option<u64> {
        match &self.cursor {
            StackCursor::NoFrames => None,
            StackCursor::OnFrame(fi) => self.frames.get(*fi).map(|f| f.frame_id),
            StackCursor::OnVar { frame_idx, .. }
            | StackCursor::OnObjectField { frame_idx, .. }
            | StackCursor::OnObjectLoadingNode { frame_idx, .. }
            | StackCursor::OnCyclicNode { frame_idx, .. }
            | StackCursor::OnChunkSection { frame_idx, .. }
            | StackCursor::OnCollectionEntry { frame_idx, .. }
            | StackCursor::OnCollectionEntryObjField { frame_idx, .. } => {
                self.frames.get(*frame_idx).map(|f| f.frame_id)
            }
        }
    }

    /// Returns the current cursor.
    pub fn cursor(&self) -> &StackCursor {
        &self.cursor
    }

    /// Returns the stack frames slice.
    pub(crate) fn frames(&self) -> &[FrameInfo] {
        &self.frames
    }

    /// Returns the vars map (keyed by `frame_id`).
    pub(crate) fn vars(&self) -> &HashMap<u64, Vec<VariableInfo>> {
        &self.vars
    }

    /// Returns the decoded object fields map.
    pub(crate) fn object_fields(&self) -> &HashMap<u64, Vec<FieldInfo>> {
        &self.expansion.object_fields
    }

    /// Returns the collection chunks map.
    pub(crate) fn collection_chunks_map(&self) -> &HashMap<u64, CollectionChunks> {
        &self.expansion.collection_chunks
    }

    /// Sets the cursor to `new_cursor` and syncs the
    /// ratatui list state.
    pub fn set_cursor(&mut self, new_cursor: StackCursor) {
        self.cursor = new_cursor;
        self.sync_list_state();
    }

    /// Returns the object_id if the cursor is on an `ObjectRef` var.
    pub fn selected_object_id(&self) -> Option<u64> {
        if let StackCursor::OnVar { frame_idx, var_idx } = self.cursor {
            let frame = self.frames.get(frame_idx)?;
            let vars = self.vars.get(&frame.frame_id)?;
            let var = vars.get(var_idx)?;
            if let VariableValue::ObjectRef { id, .. } = var.value {
                return Some(id);
            }
        }
        None
    }

    /// Returns the object_id if the cursor is on a loading/failed/empty pseudo-node.
    ///
    /// For root-level loading nodes (`field_path` empty) returns the root var's
    /// `ObjectRef` id. For nested loading nodes returns the nested object's id.
    pub fn selected_loading_object_id(&self) -> Option<u64> {
        if let StackCursor::OnObjectLoadingNode {
            frame_idx,
            var_idx,
            field_path,
        } = &self.cursor
        {
            let frame = self.frames.get(*frame_idx)?;
            let vars = self.vars.get(&frame.frame_id)?;
            let var = vars.get(*var_idx)?;
            if let VariableValue::ObjectRef { id: root_id, .. } = var.value {
                return Some(self.resolve_object_at_path(root_id, field_path));
            }
        }
        None
    }

    /// Walks `field_path` from `root_id` and returns the object_id that owns
    /// the field at the last path element. An empty path returns `root_id`.
    fn resolve_object_at_path(&self, root_id: u64, field_path: &[usize]) -> u64 {
        let mut current = root_id;
        for &step in field_path {
            if let Some(fields) = self.expansion.object_fields.get(&current)
                && let Some(field) = fields.get(step)
                && let FieldValue::ObjectRef { id, .. } = field.value
            {
                current = id;
            } else {
                break;
            }
        }
        current
    }

    /// Returns the `ObjectRef` id of the field under the cursor, if the cursor
    /// is `OnObjectField` and that field holds a `FieldValue::ObjectRef`. Used
    /// by `App` to start or stop nested expansion; the caller is responsible
    /// for checking the expansion phase.
    pub fn selected_field_ref_id(&self) -> Option<u64> {
        if let StackCursor::OnObjectField {
            frame_idx,
            var_idx,
            field_path,
        } = &self.cursor
        {
            let frame = self.frames.get(*frame_idx)?;
            let vars = self.vars.get(&frame.frame_id)?;
            let var = vars.get(*var_idx)?;
            if let VariableValue::ObjectRef { id: root_id, .. } = var.value {
                // Walk to parent object.
                let parent_path = &field_path[..field_path.len().saturating_sub(1)];
                let parent_id = self.resolve_object_at_path(root_id, parent_path);
                let field_idx = *field_path.last()?;
                let fields = self.expansion.object_fields.get(&parent_id)?;
                let field = fields.get(field_idx)?;
                if let FieldValue::ObjectRef { id, .. } = field.value {
                    return Some(id);
                }
            }
        }
        None
    }

    /// Returns `(object_id, entry_count)` for the field
    /// under cursor if it is an `ObjectRef` with a
    /// collection entry count.
    pub fn selected_field_collection_info(&self) -> Option<(u64, u64)> {
        if let StackCursor::OnObjectField {
            frame_idx,
            var_idx,
            ref field_path,
        } = self.cursor
        {
            let frame = self.frames.get(frame_idx)?;
            let vars = self.vars.get(&frame.frame_id)?;
            let var = vars.get(var_idx)?;
            if let VariableValue::ObjectRef { id: root_id, .. } = var.value {
                let parent_path = &field_path[..field_path.len().saturating_sub(1)];
                let parent_id = self.resolve_object_at_path(root_id, parent_path);
                let field_idx = *field_path.last()?;
                let fields = self.expansion.object_fields.get(&parent_id)?;
                let field = fields.get(field_idx)?;
                if let FieldValue::ObjectRef {
                    id,
                    entry_count: Some(ec),
                    ..
                } = field.value
                    && ec > 0
                {
                    return Some((id, ec));
                }
            }
        }
        None
    }

    /// Returns `Some(entry_count)` if the currently selected variable is a collection
    /// or array, `None` otherwise.
    pub fn selected_var_entry_count(&self) -> Option<u64> {
        let StackCursor::OnVar { frame_idx, var_idx } = self.cursor else {
            return None;
        };
        let frame = self.frames.get(frame_idx)?;
        let vars = self.vars.get(&frame.frame_id)?;
        let var = vars.get(var_idx)?;
        if let VariableValue::ObjectRef { entry_count, .. } = &var.value {
            *entry_count
        } else {
            None
        }
    }

    /// Returns `Some(entry_count)` if the currently selected collection entry is itself
    /// a collection or array, `None` otherwise.
    pub fn selected_collection_entry_count(&self) -> Option<u64> {
        let StackCursor::OnCollectionEntry {
            collection_id,
            entry_index,
            ..
        } = self.cursor
        else {
            return None;
        };
        let cc = self.expansion.collection_chunks.get(&collection_id)?;
        let entry = cc.find_entry(entry_index)?;
        if let FieldValue::ObjectRef { entry_count, .. } = &entry.value {
            *entry_count
        } else {
            None
        }
    }

    /// Returns `(collection_id, chunk_offset, chunk_limit)`
    /// if cursor is on a chunk section.
    pub fn selected_chunk_info(&self) -> Option<(u64, usize, usize)> {
        if let StackCursor::OnChunkSection {
            collection_id,
            chunk_offset,
            ..
        } = &self.cursor
        {
            let cc = self.expansion.collection_chunks.get(collection_id)?;
            let ranges = compute_chunk_ranges(cc.total_count);
            let limit = ranges
                .iter()
                .find(|(o, _)| *o == *chunk_offset)
                .map(|(_, l)| *l)?;
            return Some((*collection_id, *chunk_offset, limit));
        }
        None
    }

    /// Returns the `ObjectRef` id when cursor is `OnCollectionEntry`
    /// and that entry's value is an `ObjectRef`.
    pub fn selected_collection_entry_ref_id(&self) -> Option<u64> {
        if let StackCursor::OnCollectionEntry {
            collection_id,
            entry_index,
            ..
        } = &self.cursor
        {
            let cc = self.expansion.collection_chunks.get(collection_id)?;
            let entry = cc.find_entry(*entry_index)?;
            if let FieldValue::ObjectRef { id, .. } = &entry.value {
                return Some(*id);
            }
        }
        None
    }

    /// Returns the `ObjectRef` id when cursor is
    /// `OnCollectionEntryObjField` pointing to an `ObjectRef` field.
    pub fn selected_collection_entry_obj_field_ref_id(&self) -> Option<u64> {
        let field = self.collection_entry_obj_cursor_field()?;
        if let FieldValue::ObjectRef { id, .. } = field.value {
            return Some(id);
        }
        None
    }

    /// Resolves the field under the cursor when on
    /// `OnCollectionEntryObjField`.
    fn collection_entry_obj_cursor_field(&self) -> Option<&FieldInfo> {
        if let StackCursor::OnCollectionEntryObjField {
            collection_id,
            entry_index,
            obj_field_path,
            ..
        } = &self.cursor
        {
            let obj_root = {
                let cc = self.expansion.collection_chunks.get(collection_id)?;
                let entry = cc.find_entry(*entry_index)?;
                if let FieldValue::ObjectRef { id, .. } = &entry.value {
                    *id
                } else {
                    return None;
                }
            };
            let parent_path = &obj_field_path[..obj_field_path.len().saturating_sub(1)];
            let parent_id = self.resolve_object_at_path(obj_root, parent_path);
            let field_idx = *obj_field_path.last()?;
            let fields = self.expansion.object_fields.get(&parent_id)?;
            return fields.get(field_idx);
        }
        None
    }

    /// Returns the `ChunkState` for a specific chunk.
    pub fn chunk_state(&self, collection_id: u64, chunk_offset: usize) -> Option<&ChunkState> {
        self.expansion.chunk_state(collection_id, chunk_offset)
    }

    /// If cursor is inside a collection (entry or chunk
    /// section), returns the collection object ID and the
    /// `field_path` of the parent ObjectRef field so the
    /// cursor can be restored there.
    pub fn cursor_collection_id(&self) -> Option<(u64, StackCursor)> {
        match &self.cursor {
            StackCursor::OnCollectionEntry {
                frame_idx,
                var_idx,
                field_path,
                collection_id,
                ..
            }
            | StackCursor::OnChunkSection {
                frame_idx,
                var_idx,
                field_path,
                collection_id,
                ..
            }
            | StackCursor::OnCollectionEntryObjField {
                frame_idx,
                var_idx,
                field_path,
                collection_id,
                ..
            } => Some((
                *collection_id,
                StackCursor::OnObjectField {
                    frame_idx: *frame_idx,
                    var_idx: *var_idx,
                    field_path: field_path.clone(),
                },
            )),
            _ => None,
        }
    }

    /// Returns the expansion phase for `object_id` (defaults to `Collapsed`).
    pub fn expansion_state(&self, object_id: u64) -> ExpansionPhase {
        self.expansion.expansion_state(object_id)
    }

    /// Marks an object as loading (called by App on expansion start).
    pub fn set_expansion_loading(&mut self, object_id: u64) {
        self.expansion.set_expansion_loading(object_id);
    }

    /// Marks an object expansion as complete with decoded fields.
    pub fn set_expansion_done(&mut self, object_id: u64, fields: Vec<FieldInfo>) {
        self.expansion.set_expansion_done(object_id, fields);
    }

    /// Marks an object expansion as failed with an error message.
    ///
    /// If the cursor was on the `OnObjectLoadingNode` for this object (the
    /// loading spinner), it is recovered to the parent node so navigation
    /// is not stuck after the failure.
    pub fn set_expansion_failed(&mut self, object_id: u64, error: String) {
        self.expansion.set_expansion_failed(object_id, error);
        if self.flat_index().is_none()
            && let StackCursor::OnObjectLoadingNode {
                frame_idx,
                var_idx,
                ref field_path,
            } = self.cursor.clone()
        {
            self.cursor = if field_path.is_empty() {
                StackCursor::OnVar { frame_idx, var_idx }
            } else {
                StackCursor::OnObjectField {
                    frame_idx,
                    var_idx,
                    field_path: field_path.clone(),
                }
            };
        }
        self.sync_list_state();
    }

    /// Cancels a loading expansion — reverts to `Collapsed`.
    pub fn cancel_expansion(&mut self, object_id: u64) {
        self.expansion.cancel_expansion(object_id);
    }

    /// Collapses an expanded object.
    pub fn collapse_object(&mut self, object_id: u64) {
        self.expansion.collapse_object(object_id);
    }

    /// Recursively collapses `object_id` and all nested
    /// expanded descendants.
    ///
    /// Uses a visited set to guard against cycles in corrupted
    /// heap metadata. After collapse, resyncs the cursor if it
    /// became orphaned.
    pub fn collapse_object_recursive(&mut self, object_id: u64) {
        let mut to_remove: Vec<u64> = Vec::new();
        let mut visited: HashSet<u64> = HashSet::new();
        collect_descendants(
            object_id,
            &self.expansion.object_fields,
            &mut visited,
            &mut to_remove,
        );
        for id in to_remove {
            self.collapse_object(id);
        }
        self.resync_cursor_after_collapse();
    }

    /// If the current cursor is no longer in the flat list (orphaned
    /// after a collapse that propagated through a cyclic back-ref),
    /// fall back to the parent `OnVar` or `OnFrame`.
    fn resync_cursor_after_collapse(&mut self) {
        let flat = self.flat_items();
        if flat.contains(&self.cursor) {
            return;
        }
        // Try falling back to OnVar or OnCollectionEntry
        match &self.cursor {
            StackCursor::OnObjectField {
                frame_idx, var_idx, ..
            }
            | StackCursor::OnCyclicNode {
                frame_idx, var_idx, ..
            }
            | StackCursor::OnObjectLoadingNode {
                frame_idx, var_idx, ..
            } => {
                let fallback = StackCursor::OnVar {
                    frame_idx: *frame_idx,
                    var_idx: *var_idx,
                };
                if flat.contains(&fallback) {
                    self.cursor = fallback;
                } else {
                    self.cursor = StackCursor::OnFrame(*frame_idx);
                }
            }
            StackCursor::OnCollectionEntryObjField {
                frame_idx,
                var_idx,
                field_path,
                collection_id,
                entry_index,
                ..
            } => {
                let fallback = StackCursor::OnCollectionEntry {
                    frame_idx: *frame_idx,
                    var_idx: *var_idx,
                    field_path: field_path.clone(),
                    collection_id: *collection_id,
                    entry_index: *entry_index,
                };
                if flat.contains(&fallback) {
                    self.cursor = fallback;
                } else {
                    self.cursor = StackCursor::OnFrame(*frame_idx);
                }
            }
            _ => {}
        }
        self.sync_list_state();
    }

    /// Loads vars for `frame_id` into internal cache and toggles expand/collapse.
    ///
    /// When collapsing: if cursor is on a var of this frame, it is reset to the
    /// frame row so navigation remains consistent. All expanded objects that
    /// belong to vars of this frame are recursively collapsed.
    pub fn toggle_expand(&mut self, frame_id: u64, vars: Vec<VariableInfo>) {
        if self.expanded.contains(&frame_id) {
            self.expanded.remove(&frame_id);
            // Recursively collapse any expanded objects in this frame's vars.
            if let Some(cached_vars) = self.vars.get(&frame_id) {
                let object_ids: Vec<u64> = cached_vars
                    .iter()
                    .filter_map(|v| {
                        if let VariableValue::ObjectRef { id, .. } = v.value {
                            Some(id)
                        } else {
                            None
                        }
                    })
                    .collect();
                for oid in object_ids {
                    self.collapse_object_recursive(oid);
                }
            }
            // Reset cursor to the frame row when collapsing from a var position.
            if let StackCursor::OnVar { frame_idx, .. }
            | StackCursor::OnObjectField { frame_idx, .. }
            | StackCursor::OnObjectLoadingNode { frame_idx, .. }
            | StackCursor::OnCyclicNode { frame_idx, .. }
            | StackCursor::OnChunkSection { frame_idx, .. }
            | StackCursor::OnCollectionEntry { frame_idx, .. }
            | StackCursor::OnCollectionEntryObjField { frame_idx, .. } = self.cursor
            {
                self.cursor = StackCursor::OnFrame(frame_idx);
            }
        } else {
            self.vars.insert(frame_id, vars);
            self.expanded.insert(frame_id);
        }
        self.sync_list_state();
    }

    /// Returns whether `frame_id` is currently expanded.
    pub fn is_expanded(&self, frame_id: u64) -> bool {
        self.expanded.contains(&frame_id)
    }

    /// Moves the cursor one step down.
    pub fn move_down(&mut self) {
        let flat = self.flat_items();
        if let Some(current) = flat.iter().position(|c| c == &self.cursor)
            && current + 1 < flat.len()
        {
            let next = current + 1;
            self.cursor = flat[next].clone();
            self.list_state.select(Some(next));
        }
    }

    /// Moves the cursor one step up.
    pub fn move_up(&mut self) {
        let flat = self.flat_items();
        if let Some(current) = flat.iter().position(|c| c == &self.cursor)
            && let Some(prev) = current.checked_sub(1)
        {
            self.cursor = flat[prev].clone();
            self.list_state.select(Some(prev));
        }
    }

    /// Sets the visible height (called during render).
    pub fn set_visible_height(&mut self, h: u16) {
        self.visible_height = h;
    }

    /// Moves the cursor forward by `visible_height` items.
    pub fn move_page_down(&mut self) {
        let flat = self.flat_items();
        if flat.is_empty() {
            return;
        }
        let current = flat.iter().position(|c| c == &self.cursor).unwrap_or(0);
        let target = (current + self.visible_height as usize).min(flat.len() - 1);
        self.cursor = flat[target].clone();
        self.list_state.select(Some(target));
    }

    /// Moves the cursor backward by `visible_height` items.
    pub fn move_page_up(&mut self) {
        let flat = self.flat_items();
        if flat.is_empty() {
            return;
        }
        let current = flat.iter().position(|c| c == &self.cursor).unwrap_or(0);
        let target = current.saturating_sub(self.visible_height as usize);
        self.cursor = flat[target].clone();
        self.list_state.select(Some(target));
    }

    /// Returns the flattened cursor index (position in the rendered list).
    fn flat_index(&self) -> Option<usize> {
        let flat = self.flat_items();
        flat.iter().position(|c| c == &self.cursor)
    }

    /// Flattened ordered list of cursors matching the rendered list items.
    pub(crate) fn flat_items(&self) -> Vec<StackCursor> {
        let mut out = Vec::new();
        for (fi, frame) in self.frames.iter().enumerate() {
            out.push(StackCursor::OnFrame(fi));
            if self.expanded.contains(&frame.frame_id) {
                let empty = vec![];
                let vars = self.vars.get(&frame.frame_id).unwrap_or(&empty);
                if vars.is_empty() {
                    out.push(StackCursor::OnVar {
                        frame_idx: fi,
                        var_idx: 0,
                    });
                } else {
                    for (vi, var) in vars.iter().enumerate() {
                        out.push(StackCursor::OnVar {
                            frame_idx: fi,
                            var_idx: vi,
                        });
                        if let VariableValue::ObjectRef { id: object_id, .. } = var.value {
                            let mut visited = HashSet::new();
                            self.emit_object_children(
                                fi,
                                vi,
                                object_id,
                                vec![],
                                &mut visited,
                                &mut out,
                            );
                        }
                    }
                }
            }
        }
        out
    }

    /// Emits cursor nodes for the children of `object_id` at `parent_path`.
    ///
    /// Guards against runaway recursion: stops at depth 16.
    /// `visited` tracks the ancestor chain for cycle detection.
    fn emit_object_children(
        &self,
        fi: usize,
        vi: usize,
        object_id: u64,
        parent_path: Vec<usize>,
        visited: &mut HashSet<u64>,
        out: &mut Vec<StackCursor>,
    ) {
        if parent_path.len() >= 16 {
            return;
        }
        match self.expansion_state(object_id) {
            ExpansionPhase::Collapsed => {}
            ExpansionPhase::Loading => {
                out.push(StackCursor::OnObjectLoadingNode {
                    frame_idx: fi,
                    var_idx: vi,
                    field_path: parent_path,
                });
            }
            ExpansionPhase::Expanded => {
                visited.insert(object_id);
                let fields = self.expansion.object_fields.get(&object_id);
                let field_count = fields.map(|f| f.len()).unwrap_or(0);
                if field_count == 0 {
                    out.push(StackCursor::OnObjectLoadingNode {
                        frame_idx: fi,
                        var_idx: vi,
                        field_path: parent_path.clone(),
                    });
                } else {
                    let field_list = fields.unwrap();
                    for (idx, field) in field_list.iter().enumerate() {
                        let mut path = parent_path.clone();
                        path.push(idx);
                        if let FieldValue::ObjectRef { id, .. } = field.value
                            && visited.contains(&id)
                        {
                            out.push(StackCursor::OnCyclicNode {
                                frame_idx: fi,
                                var_idx: vi,
                                field_path: path,
                            });
                            continue;
                        }
                        out.push(StackCursor::OnObjectField {
                            frame_idx: fi,
                            var_idx: vi,
                            field_path: path.clone(),
                        });
                        // Check for collection expansion.
                        if let FieldValue::ObjectRef {
                            id,
                            entry_count: Some(_),
                            ..
                        } = field.value
                            && let Some(cc) = self.expansion.collection_chunks.get(&id)
                        {
                            self.emit_collection_children(fi, vi, &path, id, cc, out);
                            continue;
                        }
                        if let FieldValue::ObjectRef { id, .. } = field.value {
                            self.emit_object_children(fi, vi, id, path, visited, out);
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

    /// Emits cursors for collection entries and chunk
    /// sections.
    fn emit_collection_children(
        &self,
        fi: usize,
        vi: usize,
        field_path: &[usize],
        collection_id: u64,
        cc: &CollectionChunks,
        out: &mut Vec<StackCursor>,
    ) {
        let emit_entry = |entry: &hprof_engine::EntryInfo, out: &mut Vec<StackCursor>| {
            out.push(StackCursor::OnCollectionEntry {
                frame_idx: fi,
                var_idx: vi,
                field_path: field_path.to_vec(),
                collection_id,
                entry_index: entry.index,
            });
            // If this entry's value is an expanded ObjectRef, emit its
            // fields as OnCollectionEntryObjField cursors.
            if let FieldValue::ObjectRef { id, .. } = &entry.value {
                let mut visited = HashSet::new();
                self.emit_collection_entry_obj_children(
                    fi,
                    vi,
                    field_path,
                    collection_id,
                    entry.index,
                    *id,
                    &[],
                    &mut visited,
                    out,
                );
            }
        };
        // Eager page entries.
        if let Some(page) = &cc.eager_page {
            for entry in &page.entries {
                emit_entry(entry, out);
            }
        }
        // Chunk sections in offset order.
        let ranges = compute_chunk_ranges(cc.total_count);
        for (offset, _) in &ranges {
            out.push(StackCursor::OnChunkSection {
                frame_idx: fi,
                var_idx: vi,
                field_path: field_path.to_vec(),
                collection_id,
                chunk_offset: *offset,
            });
            // If loaded, emit entries.
            if let Some(ChunkState::Loaded(page)) = cc.chunk_pages.get(offset) {
                for entry in &page.entries {
                    emit_entry(entry, out);
                }
            }
        }
    }

    /// Emits [`StackCursor::OnCollectionEntryObjField`] nodes for
    /// the fields of an object expanded from a collection entry value.
    ///
    /// Guards against runaway recursion: stops at depth 16.
    #[allow(clippy::too_many_arguments)]
    fn emit_collection_entry_obj_children(
        &self,
        fi: usize,
        vi: usize,
        field_path: &[usize],
        collection_id: u64,
        entry_index: usize,
        obj_id: u64,
        obj_path: &[usize],
        visited: &mut HashSet<u64>,
        out: &mut Vec<StackCursor>,
    ) {
        if obj_path.len() >= 16 {
            return;
        }
        match self.expansion_state(obj_id) {
            ExpansionPhase::Collapsed => {}
            ExpansionPhase::Loading => {
                out.push(StackCursor::OnCollectionEntryObjField {
                    frame_idx: fi,
                    var_idx: vi,
                    field_path: field_path.to_vec(),
                    collection_id,
                    entry_index,
                    obj_field_path: obj_path.to_vec(),
                });
            }
            ExpansionPhase::Failed => {
                // Error state is styled on the parent entry row — no child cursor emitted here.
            }
            ExpansionPhase::Expanded => {
                let fields = self.expansion.object_fields.get(&obj_id);
                let field_count = fields.map(|f| f.len()).unwrap_or(0);
                if field_count == 0 {
                    out.push(StackCursor::OnCollectionEntryObjField {
                        frame_idx: fi,
                        var_idx: vi,
                        field_path: field_path.to_vec(),
                        collection_id,
                        entry_index,
                        obj_field_path: obj_path.to_vec(),
                    });
                } else {
                    visited.insert(obj_id);
                    let field_list = fields.unwrap();
                    for (idx, field) in field_list.iter().enumerate() {
                        let mut path = obj_path.to_vec();
                        path.push(idx);
                        if let FieldValue::ObjectRef { id, .. } = field.value
                            && visited.contains(&id)
                        {
                            // Cyclic — emit as non-navigable leaf (no cursor)
                            continue;
                        }
                        out.push(StackCursor::OnCollectionEntryObjField {
                            frame_idx: fi,
                            var_idx: vi,
                            field_path: field_path.to_vec(),
                            collection_id,
                            entry_index,
                            obj_field_path: path.clone(),
                        });
                        if let FieldValue::ObjectRef { id, .. } = field.value {
                            self.emit_collection_entry_obj_children(
                                fi,
                                vi,
                                field_path,
                                collection_id,
                                entry_index,
                                id,
                                &path,
                                visited,
                                out,
                            );
                        }
                    }
                    visited.remove(&obj_id);
                }
            }
        }
    }

    fn sync_list_state(&mut self) {
        let idx = self.flat_index();
        self.list_state.select(idx);
    }

    // === Rendering ===
    /// Builds the list items for rendering.
    ///
    /// Frame headers are plain items; variable-tree rows are produced by
    /// [`render_variable_tree`] (no per-item cursor styling — selection is
    /// applied by ratatui's `List` via [`Self::list_state`]).
    pub fn build_items(&self) -> Vec<ListItem<'static>> {
        use super::super::tree_render::{TreeRoot, render_variable_tree};
        let mut items = Vec::new();
        for frame in &self.frames {
            let label = format_frame_label(frame);
            let is_expanded = self.expanded.contains(&frame.frame_id);
            let toggle = if !frame.has_variables {
                "  "
            } else if is_expanded {
                "- "
            } else {
                "+ "
            };
            let text = format!("{toggle}{label}");
            items.push(ListItem::new(Line::from(text)));

            if is_expanded {
                let empty = vec![];
                let vars = self.vars.get(&frame.frame_id).unwrap_or(&empty);
                let tree_items = render_variable_tree(
                    TreeRoot::Frame { vars },
                    &self.expansion.object_fields,
                    &self.expansion.collection_chunks,
                    &self.expansion.object_phases,
                    &self.expansion.object_errors,
                );
                items.extend(tree_items);
            }
        }
        if items.is_empty() {
            items.push(ListItem::new(Line::from(Span::styled(
                "(no frames)",
                THEME.null_value,
            ))));
        }
        items
    }

}
