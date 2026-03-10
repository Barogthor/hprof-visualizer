//! Stack frame panel: frame list with inline local variable tree.
//!
//! [`StackState`] manages frame selection and expand/collapse of local vars.
//! [`StackView`] is a [`StatefulWidget`] rendering the current state.

use std::collections::{HashMap, HashSet};

use hprof_engine::{
    CollectionPage, FieldInfo, FieldValue, FrameInfo, LineNumber, VariableInfo, VariableValue,
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, ListState, StatefulWidget, Widget},
};

use crate::theme;

/// Phase of an object expansion driven by `App`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpansionPhase {
    Collapsed,
    Loading,
    Expanded,
    Failed,
}

/// State of one chunk section in a paginated collection.
#[derive(Debug, Clone)]
pub enum ChunkState {
    /// Chunk not yet loaded — shows `+ [offset...end]`.
    Collapsed,
    /// Chunk load in progress — shows `~ Loading...`.
    Loading,
    /// Chunk loaded — shows entries inline.
    Loaded(CollectionPage),
}

/// State for one expanded collection in the tree.
#[derive(Debug, Clone)]
pub struct CollectionChunks {
    /// Total entry count of the collection.
    pub total_count: u64,
    /// First page (eagerly loaded, entries 0..100).
    pub eager_page: Option<CollectionPage>,
    /// Chunk sections keyed by chunk offset.
    pub chunk_pages: HashMap<usize, ChunkState>,
}

impl CollectionChunks {
    /// Finds the [`EntryInfo`] with the given `index` across all loaded
    /// pages (eager page and all loaded chunk pages).
    pub(crate) fn find_entry(&self, index: usize) -> Option<&hprof_engine::EntryInfo> {
        if let Some(page) = &self.eager_page
            && let Some(e) = page.entries.iter().find(|e| e.index == index)
        {
            return Some(e);
        }
        for state in self.chunk_pages.values() {
            if let ChunkState::Loaded(page) = state
                && let Some(e) = page.entries.iter().find(|e| e.index == index)
            {
                return Some(e);
            }
        }
        None
    }
}

/// Computes chunk ranges for a collection with
/// `total_count` entries.
///
/// Returns `(offset, limit)` pairs following the
/// 100/100/1000 chunking rules:
/// - `<= 100`: no sections (all eager)
/// - `101..=1000`: sections of 100
/// - `> 1000`: sections of 100 up to 1000, then
///   sections of 1000
pub fn compute_chunk_ranges(total_count: u64) -> Vec<(usize, usize)> {
    if total_count <= 100 {
        return vec![];
    }
    let total = total_count as usize;
    let mut ranges = Vec::new();
    // Sections of 100 from 100 up to min(1000, total)
    let boundary = total.min(1000);
    let mut offset = 100;
    while offset < boundary {
        let limit = (boundary - offset).min(100);
        ranges.push((offset, limit));
        offset += 100;
    }
    // Sections of 1000 from 1000 onward
    offset = 1000;
    while offset < total {
        let limit = (total - offset).min(1000);
        ranges.push((offset, limit));
        offset += 1000;
    }
    ranges
}

/// Cursor position within the frame+var tree.
///
/// `field_path` encodes depth from the root `ObjectRef` var:
/// - `[]` (empty) — loading/error node for the root var
/// - `[2]` — field index 2 of the root object (depth 1)
/// - `[2, 1]` — field index 1 within field 2's expanded object (depth 2)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StackCursor {
    NoFrames,
    OnFrame(usize),
    OnVar {
        frame_idx: usize,
        var_idx: usize,
    },
    /// Cursor on a specific field within an expanded object at any depth.
    OnObjectField {
        frame_idx: usize,
        var_idx: usize,
        /// Path of field indices from the root object to the current field.
        field_path: Vec<usize>,
    },
    /// Cursor on the loading/error pseudo-node for an expanding object.
    OnObjectLoadingNode {
        frame_idx: usize,
        var_idx: usize,
        /// Empty = root var's loading node. Non-empty = nested object's node.
        field_path: Vec<usize>,
    },
    /// Cursor on a cyclic reference marker (non-expandable leaf).
    OnCyclicNode {
        frame_idx: usize,
        var_idx: usize,
        field_path: Vec<usize>,
    },
    /// Cursor on a chunk section header inside a
    /// paginated collection.
    OnChunkSection {
        frame_idx: usize,
        var_idx: usize,
        field_path: Vec<usize>,
        collection_id: u64,
        chunk_offset: usize,
    },
    /// Cursor on one entry inside a paginated collection.
    OnCollectionEntry {
        frame_idx: usize,
        var_idx: usize,
        field_path: Vec<usize>,
        collection_id: u64,
        entry_index: usize,
    },
    /// Cursor on a field within an object expanded from a collection
    /// entry value. `obj_field_path` is empty for the loading/error
    /// node; non-empty encodes the field path within the entry object.
    OnCollectionEntryObjField {
        frame_idx: usize,
        var_idx: usize,
        /// Path to the collection's parent [`FieldValue::ObjectRef`] field.
        field_path: Vec<usize>,
        collection_id: u64,
        entry_index: usize,
        /// Path within the entry's root object.
        obj_field_path: Vec<usize>,
    },
}

/// State for the stack frame panel.
pub struct StackState {
    frames: Vec<FrameInfo>,
    /// Vars per frame_id — populated on demand by `App` calling the engine.
    vars: HashMap<u64, Vec<VariableInfo>>,
    expanded: HashSet<u64>,
    cursor: StackCursor,
    list_state: ListState,
    /// Per-object expansion phases (keyed by object_id).
    object_phases: HashMap<u64, ExpansionPhase>,
    /// Decoded fields for expanded objects.
    pub(crate) object_fields: HashMap<u64, Vec<FieldInfo>>,
    /// Error messages for failed expansions.
    object_errors: HashMap<u64, String>,
    /// Visible height of the stack panel (set during render).
    visible_height: u16,
    /// Per-collection paginated state (keyed by collection
    /// object ID).
    pub(crate) collection_chunks: HashMap<u64, CollectionChunks>,
}

/// Collects all descendant object IDs reachable from `root_id` in depth-first
/// post-order. Cycles are broken via `visited`.
fn collect_descendants(
    root_id: u64,
    fields: &HashMap<u64, Vec<FieldInfo>>,
    visited: &mut HashSet<u64>,
    out: &mut Vec<u64>,
) {
    if !visited.insert(root_id) {
        return;
    }
    if let Some(field_list) = fields.get(&root_id) {
        for f in field_list {
            if let FieldValue::ObjectRef { id, .. } = f.value {
                collect_descendants(id, fields, visited, out);
            }
        }
    }
    out.push(root_id);
}

fn format_frame_label(frame: &FrameInfo) -> String {
    let line_label = match &frame.line {
        LineNumber::Line(n) => format!(":{}", n),
        LineNumber::NoInfo => String::new(),
        LineNumber::Unknown => " (?)".to_string(),
        LineNumber::Compiled => " (compiled)".to_string(),
        LineNumber::Native => " (native)".to_string(),
    };
    let location = if frame.source_file.is_empty() {
        line_label
    } else {
        format!(" [{}{}]", frame.source_file, line_label)
    };
    format!("{}.{}(){}", frame.class_name, frame.method_name, location)
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
            object_phases: HashMap::new(),
            object_fields: HashMap::new(),
            object_errors: HashMap::new(),
            visible_height: 0,
            collection_chunks: HashMap::new(),
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
            if let Some(fields) = self.object_fields.get(&current)
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
                let fields = self.object_fields.get(&parent_id)?;
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
                let fields = self.object_fields.get(&parent_id)?;
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

    /// Returns `(collection_id, chunk_offset, chunk_limit)`
    /// if cursor is on a chunk section.
    pub fn selected_chunk_info(&self) -> Option<(u64, usize, usize)> {
        if let StackCursor::OnChunkSection {
            collection_id,
            chunk_offset,
            ..
        } = &self.cursor
        {
            let cc = self.collection_chunks.get(collection_id)?;
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
            let cc = self.collection_chunks.get(collection_id)?;
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
                let cc = self.collection_chunks.get(collection_id)?;
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
            let fields = self.object_fields.get(&parent_id)?;
            return fields.get(field_idx);
        }
        None
    }

    /// Returns the `ChunkState` for a specific chunk.
    pub fn chunk_state(&self, collection_id: u64, chunk_offset: usize) -> Option<&ChunkState> {
        self.collection_chunks
            .get(&collection_id)?
            .chunk_pages
            .get(&chunk_offset)
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
        self.object_phases
            .get(&object_id)
            .cloned()
            .unwrap_or(ExpansionPhase::Collapsed)
    }

    /// Marks an object as loading (called by App on expansion start).
    pub fn set_expansion_loading(&mut self, object_id: u64) {
        self.object_phases
            .insert(object_id, ExpansionPhase::Loading);
    }

    /// Marks an object expansion as complete with decoded fields.
    pub fn set_expansion_done(&mut self, object_id: u64, fields: Vec<FieldInfo>) {
        self.object_fields.insert(object_id, fields);
        self.object_phases
            .insert(object_id, ExpansionPhase::Expanded);
    }

    /// Marks an object expansion as failed with an error message.
    pub fn set_expansion_failed(&mut self, object_id: u64, error: String) {
        self.object_errors.insert(object_id, error);
        self.object_phases.insert(object_id, ExpansionPhase::Failed);
    }

    /// Cancels a loading expansion — reverts to `Collapsed`.
    pub fn cancel_expansion(&mut self, object_id: u64) {
        self.object_phases.remove(&object_id);
        self.object_fields.remove(&object_id);
        self.object_errors.remove(&object_id);
    }

    /// Collapses an expanded object.
    pub fn collapse_object(&mut self, object_id: u64) {
        self.object_phases.remove(&object_id);
        self.object_fields.remove(&object_id);
        self.object_errors.remove(&object_id);
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
        collect_descendants(object_id, &self.object_fields, &mut visited, &mut to_remove);
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
    fn flat_items(&self) -> Vec<StackCursor> {
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
                let fields = self.object_fields.get(&object_id);
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
                            && let Some(cc) = self.collection_chunks.get(&id)
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
                out.push(StackCursor::OnObjectLoadingNode {
                    frame_idx: fi,
                    var_idx: vi,
                    field_path: parent_path,
                });
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
            ExpansionPhase::Loading | ExpansionPhase::Failed => {
                out.push(StackCursor::OnCollectionEntryObjField {
                    frame_idx: fi,
                    var_idx: vi,
                    field_path: field_path.to_vec(),
                    collection_id,
                    entry_index,
                    obj_field_path: obj_path.to_vec(),
                });
            }
            ExpansionPhase::Expanded => {
                let fields = self.object_fields.get(&obj_id);
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

    /// Formats a collapsed [`FieldValue::ObjectRef`] as `ClassName [>]`
    /// or `ClassName (N entries) [>]` for collections.
    fn format_object_ref_collapsed(class_name: &str, entry_count: Option<u64>) -> String {
        let display_name = if class_name.is_empty() {
            "Object"
        } else {
            class_name
        };
        let short = display_name.rsplit('.').next().unwrap_or(display_name);
        match entry_count {
            Some(n) => format!("{short} ({n} entries)"),
            None => short.to_string(),
        }
    }

    /// Formats a [`FieldValue`] for display in field rows.
    fn format_field_value(v: &FieldValue, phase: Option<&ExpansionPhase>) -> String {
        match v {
            FieldValue::Null => "null".to_string(),
            FieldValue::ObjectRef {
                class_name,
                entry_count,
                inline_value,
                ..
            } => {
                let base = match phase {
                    Some(ExpansionPhase::Expanded) | Some(ExpansionPhase::Loading) => {
                        let display_name = if class_name.is_empty() {
                            "Object"
                        } else {
                            class_name
                        };
                        display_name
                            .rsplit('.')
                            .next()
                            .unwrap_or(display_name)
                            .to_string()
                    }
                    _ => Self::format_object_ref_collapsed(class_name, *entry_count),
                };
                match inline_value {
                    Some(v) => format!("{base} = {v}"),
                    None => base,
                }
            }
            FieldValue::Bool(b) => b.to_string(),
            FieldValue::Char(c) => format!("'{c}'"),
            FieldValue::Byte(n) => n.to_string(),
            FieldValue::Short(n) => n.to_string(),
            FieldValue::Int(n) => n.to_string(),
            FieldValue::Long(n) => n.to_string(),
            FieldValue::Float(f) => format!("{f}"),
            FieldValue::Double(d) => format!("{d}"),
        }
    }

    /// Formats a `FieldValue` for inline display in collection entries.
    ///
    /// `value_phase` is used for `ObjectRef` values: when present and
    /// `Expanded` / `Loading`, the toggle shows `-`; otherwise `+`.
    fn format_entry_value(v: &FieldValue) -> String {
        match v {
            FieldValue::Null => "null".to_string(),
            FieldValue::ObjectRef {
                class_name,
                entry_count,
                inline_value,
                ..
            } => {
                let display_name = if class_name.is_empty() {
                    "Object"
                } else {
                    class_name
                };
                let short = display_name.rsplit('.').next().unwrap_or(display_name);
                let base = match entry_count {
                    Some(n) => format!("{short} ({n} entries)"),
                    None => short.to_string(),
                };
                match inline_value {
                    Some(v) => format!("{base} = {v}"),
                    None => base,
                }
            }
            FieldValue::Bool(b) => b.to_string(),
            FieldValue::Char(c) => format!("'{c}'"),
            FieldValue::Byte(n) => n.to_string(),
            FieldValue::Short(n) => n.to_string(),
            FieldValue::Int(n) => n.to_string(),
            FieldValue::Long(n) => n.to_string(),
            FieldValue::Float(f) => format!("{f}"),
            FieldValue::Double(d) => format!("{d}"),
        }
    }

    /// Renders collection entries and chunk sections.
    #[allow(clippy::too_many_arguments)]
    fn build_collection_items(
        &self,
        fi: usize,
        vi: usize,
        field_path: &[usize],
        collection_id: u64,
        cc: &CollectionChunks,
        parent_indent: &str,
        items: &mut Vec<ListItem<'static>>,
    ) {
        let indent = format!("{parent_indent}  ");
        let render_entry =
            |entry: &hprof_engine::EntryInfo, items: &mut Vec<ListItem<'static>>, sel: bool| {
                let value_phase = if let FieldValue::ObjectRef { id, .. } = &entry.value {
                    Some(self.expansion_state(*id))
                } else {
                    None
                };
                let text = Self::format_entry_line(entry, &indent, value_phase.as_ref());
                let s = if sel {
                    theme::SELECTED
                } else {
                    ratatui::style::Style::default()
                };
                items.push(ListItem::new(Line::from(Span::styled(text, s))));
                // Render expanded entry ObjectRef children.
                if let FieldValue::ObjectRef { id, .. } = &entry.value {
                    let mut visited = HashSet::new();
                    self.build_collection_entry_obj_items(
                        fi,
                        vi,
                        field_path,
                        collection_id,
                        entry.index,
                        *id,
                        &[],
                        &indent,
                        &mut visited,
                        items,
                    );
                }
            };
        // Eager page entries.
        if let Some(page) = &cc.eager_page {
            for entry in &page.entries {
                let sel = matches!(
                    &self.cursor,
                    StackCursor::OnCollectionEntry {
                        collection_id: cid,
                        entry_index: ei,
                        ..
                    }
                    if *cid == collection_id && *ei == entry.index
                );
                render_entry(entry, items, sel);
            }
        }
        // Chunk sections.
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
            let sel = matches!(
                &self.cursor,
                StackCursor::OnChunkSection {
                    collection_id: cid,
                    chunk_offset: co,
                    ..
                }
                if *cid == collection_id && *co == *offset
            );
            let s = if sel {
                theme::SELECTED
            } else {
                theme::SEARCH_HINT
            };
            items.push(ListItem::new(Line::from(Span::styled(text, s))));
            // Loaded chunk entries.
            if let Some(ChunkState::Loaded(page)) = chunk_state {
                for entry in &page.entries {
                    let sel = matches!(
                        &self.cursor,
                        StackCursor::OnCollectionEntry {
                            collection_id: cid,
                            entry_index: ei,
                            ..
                        }
                        if *cid == collection_id && *ei == entry.index
                    );
                    render_entry(entry, items, sel);
                }
            }
        }
    }

    /// Renders fields of an object expanded from a collection entry value.
    #[allow(clippy::too_many_arguments)]
    fn build_collection_entry_obj_items(
        &self,
        _fi: usize,
        _vi: usize,
        _field_path: &[usize],
        collection_id: u64,
        entry_index: usize,
        obj_id: u64,
        obj_path: &[usize],
        parent_indent: &str,
        visited: &mut HashSet<u64>,
        items: &mut Vec<ListItem<'static>>,
    ) {
        if obj_path.len() >= 16 {
            return;
        }
        let indent = format!("{parent_indent}  ");
        match self.expansion_state(obj_id) {
            ExpansionPhase::Collapsed => {}
            ExpansionPhase::Loading => {
                let sel = matches!(&self.cursor,
                    StackCursor::OnCollectionEntryObjField {
                        collection_id: cid,
                        entry_index: ei,
                        obj_field_path,
                        ..
                    }
                    if *cid == collection_id
                        && *ei == entry_index
                        && *obj_field_path == obj_path
                );
                let s = if sel {
                    theme::SELECTED
                } else {
                    theme::SEARCH_HINT
                };
                items.push(ListItem::new(Line::from(Span::styled(
                    format!("{indent}~ Loading..."),
                    s,
                ))));
            }
            ExpansionPhase::Expanded => {
                visited.insert(obj_id);
                let empty: Vec<FieldInfo> = vec![];
                let field_list = self
                    .object_fields
                    .get(&obj_id)
                    .map(|f| f.as_slice())
                    .unwrap_or(empty.as_slice());
                if field_list.is_empty() {
                    let sel = matches!(&self.cursor,
                        StackCursor::OnCollectionEntryObjField {
                            collection_id: cid,
                            entry_index: ei,
                            obj_field_path,
                            ..
                        }
                        if *cid == collection_id
                            && *ei == entry_index
                            && *obj_field_path == obj_path
                    );
                    let s = if sel {
                        theme::SELECTED
                    } else {
                        theme::SEARCH_HINT
                    };
                    items.push(ListItem::new(Line::from(Span::styled(
                        format!("{indent}(no fields)"),
                        s,
                    ))));
                } else {
                    for (fidx, field) in field_list.iter().enumerate() {
                        let mut child_path = obj_path.to_vec();
                        child_path.push(fidx);
                        let child_phase = if let FieldValue::ObjectRef { id, .. } = field.value {
                            Some(self.expansion_state(id))
                        } else {
                            None
                        };
                        let cycle = if let FieldValue::ObjectRef { id, .. } = &field.value {
                            visited.contains(id)
                        } else {
                            false
                        };
                        let sel = !cycle
                            && matches!(&self.cursor,
                                StackCursor::OnCollectionEntryObjField {
                                    collection_id: cid,
                                    entry_index: ei,
                                    obj_field_path,
                                    ..
                                }
                                if *cid == collection_id
                                    && *ei == entry_index
                                    && *obj_field_path == child_path
                            );
                        let s = if sel {
                            theme::SELECTED
                        } else if cycle {
                            theme::SEARCH_HINT
                        } else {
                            ratatui::style::Style::default()
                        };
                        let toggle =
                            if cycle {
                                "  "
                            } else {
                                match &child_phase {
                                    Some(ExpansionPhase::Expanded)
                                    | Some(ExpansionPhase::Loading) => "- ",
                                    Some(ExpansionPhase::Collapsed)
                                    | Some(ExpansionPhase::Failed) => "+ ",
                                    None => "  ",
                                }
                            };
                        let val = Self::format_field_value(&field.value, child_phase.as_ref());
                        let text = format!("{indent}{toggle}{}: {}", field.name, val);
                        items.push(ListItem::new(Line::from(Span::styled(text, s))));
                        if !cycle
                            && let FieldValue::ObjectRef { id, .. } = field.value
                        {
                            self.build_collection_entry_obj_items(
                                _fi,
                                _vi,
                                _field_path,
                                collection_id,
                                entry_index,
                                id,
                                &child_path,
                                &indent,
                                visited,
                                items,
                            );
                        }
                    }
                }
                visited.remove(&obj_id);
            }
            ExpansionPhase::Failed => {
                let err = self
                    .object_errors
                    .get(&obj_id)
                    .cloned()
                    .unwrap_or_else(|| "Failed to resolve object".to_string());
                let sel = matches!(&self.cursor,
                    StackCursor::OnCollectionEntryObjField {
                        collection_id: cid,
                        entry_index: ei,
                        obj_field_path,
                        ..
                    }
                    if *cid == collection_id
                        && *ei == entry_index
                        && *obj_field_path == obj_path
                );
                let s = if sel {
                    theme::SELECTED
                } else {
                    theme::SEARCH_HINT
                };
                items.push(ListItem::new(Line::from(Span::styled(
                    format!("{indent}! {err}"),
                    s,
                ))));
            }
        }
    }

    /// Formats one collection entry as a display line.
    ///
    /// `value_phase` controls the expand toggle for `ObjectRef` values:
    /// pass the current [`ExpansionPhase`] of the entry's value object
    /// so that `+` / `-` is rendered correctly.
    pub(crate) fn format_entry_line(
        entry: &hprof_engine::EntryInfo,
        indent: &str,
        value_phase: Option<&ExpansionPhase>,
    ) -> String {
        let toggle = match value_phase {
            Some(ExpansionPhase::Expanded) | Some(ExpansionPhase::Loading) => "- ",
            Some(ExpansionPhase::Collapsed) | Some(ExpansionPhase::Failed) => "+ ",
            None => "  ",
        };
        let val = Self::format_entry_value(&entry.value);
        if let Some(key) = &entry.key {
            let k = Self::format_entry_value(key);
            format!("{indent}{toggle}[{}] {} => {}", entry.index, k, val)
        } else {
            format!("{indent}{toggle}[{}] {}", entry.index, val)
        }
    }

    /// Builds the list items for rendering.
    pub fn build_items(&self) -> Vec<ListItem<'static>> {
        let mut items = Vec::new();
        for (fi, frame) in self.frames.iter().enumerate() {
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
            let is_selected = matches!(&self.cursor, StackCursor::OnFrame(i) if *i == fi)
                || matches!(&self.cursor,
                    StackCursor::OnVar { frame_idx, .. }
                    | StackCursor::OnObjectField { frame_idx, .. }
                    | StackCursor::OnObjectLoadingNode { frame_idx, .. }
                    | StackCursor::OnCyclicNode { frame_idx, .. }
                    | StackCursor::OnChunkSection { frame_idx, .. }
                    | StackCursor::OnCollectionEntry { frame_idx, .. }
                    | StackCursor::OnCollectionEntryObjField { frame_idx, .. }
                    if *frame_idx == fi);
            let style = if is_selected {
                theme::SELECTED
            } else {
                ratatui::style::Style::default()
            };
            items.push(ListItem::new(Line::from(Span::styled(text, style))));

            if self.expanded.contains(&frame.frame_id) {
                let empty = vec![];
                let vars = self.vars.get(&frame.frame_id).unwrap_or(&empty);
                if vars.is_empty() {
                    let var_style = if matches!(&self.cursor,
                        StackCursor::OnVar { frame_idx, .. } if *frame_idx == fi)
                    {
                        theme::SELECTED
                    } else {
                        theme::SEARCH_HINT
                    };
                    items.push(ListItem::new(Line::from(Span::styled(
                        "  (no locals)",
                        var_style,
                    ))));
                } else {
                    for (vi, var) in vars.iter().enumerate() {
                        let phase = if let VariableValue::ObjectRef { id, .. } = var.value {
                            self.expansion_state(id)
                        } else {
                            ExpansionPhase::Collapsed
                        };

                        let (toggle, val_str) = match (&var.value, &phase) {
                            (VariableValue::Null, _) => ("  ", "null".to_string()),
                            (
                                VariableValue::ObjectRef { class_name, .. },
                                ExpansionPhase::Collapsed,
                            ) => ("+ ", format!("local variable: {}", class_name)),
                            (VariableValue::ObjectRef { class_name, .. }, _) => {
                                ("- ", format!("local variable: {}", class_name))
                            }
                        };
                        let var_text = format!("  {toggle}[{}] {val_str}", var.index,);
                        let var_selected = matches!(&self.cursor,
                            StackCursor::OnVar { frame_idx: ffi, var_idx: vvi }
                            if *ffi == fi && *vvi == vi);
                        let var_style = if var_selected {
                            theme::SELECTED
                        } else {
                            ratatui::style::Style::default()
                        };
                        items.push(ListItem::new(Line::from(Span::styled(var_text, var_style))));

                        if let VariableValue::ObjectRef { id: object_id, .. } = var.value {
                            let mut visited = HashSet::new();
                            self.build_object_items(
                                fi,
                                vi,
                                object_id,
                                &[],
                                &mut visited,
                                &mut items,
                            );
                        }
                    }
                }
            }
        }
        if items.is_empty() {
            items.push(ListItem::new(Line::from(Span::styled(
                "(no frames)",
                theme::SEARCH_HINT,
            ))));
        }
        items
    }

    /// Recursively appends list items for `object_id` at `parent_path`.
    ///
    /// Indentation = `2 + 2 * (parent_path.len() + 1)` spaces for field rows.
    /// `visited` tracks the ancestor chain for cycle detection.
    fn build_object_items(
        &self,
        fi: usize,
        vi: usize,
        object_id: u64,
        parent_path: &[usize],
        visited: &mut HashSet<u64>,
        items: &mut Vec<ListItem<'static>>,
    ) {
        // Guard against runaway recursion.
        if parent_path.len() >= 16 {
            return;
        }
        // Depth: root fields at depth 1 → 4 spaces, depth 2 → 6 spaces, etc.
        let indent = " ".repeat(2 + 2 * (parent_path.len() + 1));
        let phase = self.expansion_state(object_id);
        match phase {
            ExpansionPhase::Collapsed => {}
            ExpansionPhase::Loading => {
                let cur_path: Vec<usize> = parent_path.to_vec();
                let selected = matches!(&self.cursor,
                    StackCursor::OnObjectLoadingNode { frame_idx: ffi, var_idx: vvi, field_path }
                    if *ffi == fi && *vvi == vi && *field_path == cur_path);
                let s = if selected {
                    theme::SELECTED
                } else {
                    theme::SEARCH_HINT
                };
                items.push(ListItem::new(Line::from(Span::styled(
                    format!("{indent}~ Loading..."),
                    s,
                ))));
            }
            ExpansionPhase::Expanded => {
                visited.insert(object_id);
                let empty: Vec<FieldInfo> = vec![];
                let field_list = self
                    .object_fields
                    .get(&object_id)
                    .map(|f| f.as_slice())
                    .unwrap_or(empty.as_slice());
                if field_list.is_empty() {
                    let cur_path: Vec<usize> = parent_path.to_vec();
                    let selected = matches!(&self.cursor,
                        StackCursor::OnObjectLoadingNode { frame_idx: ffi, var_idx: vvi, field_path }
                        if *ffi == fi && *vvi == vi && *field_path == cur_path);
                    let s = if selected {
                        theme::SELECTED
                    } else {
                        theme::SEARCH_HINT
                    };
                    items.push(ListItem::new(Line::from(Span::styled(
                        format!("{indent}(no fields)"),
                        s,
                    ))));
                } else {
                    for (fidx, field) in field_list.iter().enumerate() {
                        let mut child_path = parent_path.to_vec();
                        child_path.push(fidx);

                        // Cycle detection for ObjectRef fields
                        if let FieldValue::ObjectRef { id, class_name, .. } = &field.value
                            && visited.contains(id)
                        {
                            let label = if *id == object_id {
                                "self-ref"
                            } else {
                                "cyclic"
                            };
                            let short = class_name.rsplit('.').next().unwrap_or(class_name);
                            let marker = format!("\u{21BB} {} @ 0x{:X} [{}]", short, id, label,);
                            let text = format!("{indent}  {}: {}", field.name, marker,);
                            let sel = matches!(
                                &self.cursor,
                                StackCursor::OnCyclicNode {
                                    frame_idx: ffi,
                                    var_idx: vvi,
                                    field_path,
                                }
                                if *ffi == fi
                                    && *vvi == vi
                                    && *field_path == child_path
                            );
                            let s = if sel {
                                theme::SELECTED
                            } else {
                                theme::SEARCH_HINT
                            };
                            items.push(ListItem::new(Line::from(Span::styled(text, s))));
                            continue;
                        }

                        let selected = matches!(&self.cursor,
                            StackCursor::OnObjectField { frame_idx: ffi, var_idx: vvi, field_path }
                            if *ffi == fi && *vvi == vi && *field_path == child_path);
                        let child_phase = if let FieldValue::ObjectRef { id, .. } = field.value {
                            Some(self.expansion_state(id))
                        } else {
                            None
                        };
                        let s = if selected {
                            theme::SELECTED
                        } else {
                            ratatui::style::Style::default()
                        };
                        let val = Self::format_field_value(&field.value, child_phase.as_ref());
                        let toggle = match &child_phase {
                            Some(ExpansionPhase::Expanded) | Some(ExpansionPhase::Loading) => "- ",
                            Some(ExpansionPhase::Collapsed) | Some(ExpansionPhase::Failed) => "+ ",
                            None => "  ",
                        };
                        let text = format!("{indent}{toggle}{}: {}", field.name, val,);
                        items.push(ListItem::new(Line::from(Span::styled(text, s))));

                        // Collection rendering.
                        if let FieldValue::ObjectRef {
                            id,
                            entry_count: Some(_),
                            ..
                        } = field.value
                            && let Some(cc) = self.collection_chunks.get(&id)
                        {
                            self.build_collection_items(
                                fi,
                                vi,
                                &child_path,
                                id,
                                cc,
                                &indent,
                                items,
                            );
                            continue;
                        }
                        if let FieldValue::ObjectRef { id, .. } = field.value {
                            self.build_object_items(fi, vi, id, &child_path, visited, items);
                        }
                    }
                }
                visited.remove(&object_id);
            }
            ExpansionPhase::Failed => {
                let cur_path: Vec<usize> = parent_path.to_vec();
                let msg = self
                    .object_errors
                    .get(&object_id)
                    .cloned()
                    .unwrap_or_else(|| "Failed to resolve object".to_string());
                let selected = matches!(&self.cursor,
                    StackCursor::OnObjectLoadingNode { frame_idx: ffi, var_idx: vvi, field_path }
                    if *ffi == fi && *vvi == vi && *field_path == cur_path);
                let s = if selected {
                    theme::SELECTED
                } else {
                    theme::SEARCH_HINT
                };
                items.push(ListItem::new(Line::from(Span::styled(
                    format!("{indent}! {msg}"),
                    s,
                ))));
            }
        }
    }
}

/// Stateful widget for the stack frame panel.
pub struct StackView {
    /// Whether this panel has keyboard focus.
    pub focused: bool,
}

impl StatefulWidget for StackView {
    type State = StackState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let border_style = if self.focused {
            theme::BORDER_FOCUSED
        } else {
            theme::BORDER_UNFOCUSED
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Plain)
            .border_style(border_style)
            .title("Stack Frames  [Enter] expand  [Esc] back");
        let inner = block.inner(area);
        block.render(area, buf);

        let items = state.build_items();
        let list = List::new(items)
            .highlight_style(ratatui::style::Style::default().add_modifier(Modifier::BOLD));
        StatefulWidget::render(list, inner, buf, &mut state.list_state);
    }
}

#[cfg(test)]
mod tests {
    use hprof_engine::{FrameInfo, LineNumber, VariableInfo, VariableValue};

    use super::*;

    fn make_frame(frame_id: u64) -> FrameInfo {
        FrameInfo {
            frame_id,
            method_name: format!("method{}", frame_id),
            class_name: format!("Class{}", frame_id),
            source_file: format!("Class{}.java", frame_id),
            line: LineNumber::Line(1),
            has_variables: false,
        }
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
    fn new_with_three_frames_selects_frame_0() {
        let frames = vec![make_frame(1), make_frame(2), make_frame(3)];
        let state = StackState::new(frames);
        assert_eq!(state.cursor, StackCursor::OnFrame(0));
    }

    #[test]
    fn move_down_on_three_frames_with_no_expanded_moves_to_frame_1() {
        let frames = vec![make_frame(1), make_frame(2), make_frame(3)];
        let mut state = StackState::new(frames);
        state.move_down();
        assert_eq!(state.cursor, StackCursor::OnFrame(1));
    }

    #[test]
    fn move_up_at_frame_0_does_nothing() {
        let frames = vec![make_frame(1), make_frame(2), make_frame(3)];
        let mut state = StackState::new(frames);
        state.move_up();
        assert_eq!(state.cursor, StackCursor::OnFrame(0));
    }

    #[test]
    fn toggle_expand_with_vars_then_move_down_moves_to_var_0() {
        let frames = vec![make_frame(10), make_frame(20)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var(0, 1), make_var(1, 2)];
        state.toggle_expand(10, vars);
        // cursor is still OnFrame(0), move_down should go to OnVar{frame_idx:0, var_idx:0}
        state.move_down();
        assert_eq!(
            state.cursor,
            StackCursor::OnVar {
                frame_idx: 0,
                var_idx: 0
            }
        );
    }

    #[test]
    fn move_down_past_last_var_of_expanded_frame_moves_to_next_frame() {
        let frames = vec![make_frame(10), make_frame(20)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var(0, 1)];
        state.toggle_expand(10, vars);
        // flat: [Frame(0), Var{0,0}, Frame(1)]
        state.move_down(); // → Var{0,0}
        state.move_down(); // → Frame(1)
        assert_eq!(state.cursor, StackCursor::OnFrame(1));
    }

    #[test]
    fn toggle_expand_on_already_expanded_frame_collapses_it() {
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        state.toggle_expand(10, vec![make_var(0, 1)]);
        assert!(state.is_expanded(10));
        state.toggle_expand(10, vec![]);
        assert!(!state.is_expanded(10));
    }

    #[test]
    fn toggle_expand_collapse_from_var_cursor_resets_to_frame_and_navigation_works() {
        let frames = vec![make_frame(10), make_frame(20)];
        let mut state = StackState::new(frames);
        state.toggle_expand(10, vec![make_var(0, 1)]);
        state.move_down(); // → OnVar{frame_idx:0, var_idx:0}
        assert_eq!(
            state.cursor,
            StackCursor::OnVar {
                frame_idx: 0,
                var_idx: 0
            }
        );
        // Collapse while cursor is on a var
        state.toggle_expand(10, vec![]);
        // Cursor must reset to the frame row
        assert_eq!(state.cursor, StackCursor::OnFrame(0));
        // Navigation must work: can move to the next frame
        state.move_down();
        assert_eq!(state.cursor, StackCursor::OnFrame(1));
        // And back
        state.move_up();
        assert_eq!(state.cursor, StackCursor::OnFrame(0));
    }

    #[test]
    fn selected_frame_id_returns_correct_frame_id() {
        let frames = vec![make_frame(42), make_frame(99)];
        let state = StackState::new(frames);
        assert_eq!(state.selected_frame_id(), Some(42));
    }

    #[test]
    fn format_frame_label_keeps_line_metadata_when_source_file_missing() {
        let frame = FrameInfo {
            frame_id: 1,
            method_name: "run".to_string(),
            class_name: "Thread".to_string(),
            source_file: String::new(),
            line: LineNumber::Native,
            has_variables: false,
        };
        assert_eq!(format_frame_label(&frame), "Thread.run() (native)");
    }

    #[test]
    fn format_frame_label_with_source_file_and_line_number() {
        let frame = FrameInfo {
            frame_id: 1,
            method_name: "run".to_string(),
            class_name: "Thread".to_string(),
            source_file: "Thread.java".to_string(),
            line: LineNumber::Line(42),
            has_variables: false,
        };
        assert_eq!(format_frame_label(&frame), "Thread.run() [Thread.java:42]");
    }

    #[test]
    fn new_with_empty_frames_returns_none_for_selected_frame_id() {
        let state = StackState::new(vec![]);
        assert_eq!(state.selected_frame_id(), None);
    }

    // --- Task 10: Object expansion phase tests ---

    fn make_var_object_ref(index: usize, object_id: u64) -> VariableInfo {
        VariableInfo {
            index,
            value: VariableValue::ObjectRef {
                id: object_id,
                class_name: "Object".to_string(),
            },
        }
    }

    #[test]
    fn set_expansion_loading_changes_phase_to_loading() {
        let mut state = StackState::new(vec![make_frame(1)]);
        state.set_expansion_loading(42);
        assert_eq!(state.expansion_state(42), ExpansionPhase::Loading);
    }

    #[test]
    fn set_expansion_done_changes_phase_to_expanded() {
        let mut state = StackState::new(vec![make_frame(1)]);
        state.set_expansion_done(42, vec![]);
        assert_eq!(state.expansion_state(42), ExpansionPhase::Expanded);
    }

    #[test]
    fn set_expansion_failed_changes_phase_to_failed() {
        let mut state = StackState::new(vec![make_frame(1)]);
        state.set_expansion_failed(42, "err".to_string());
        assert_eq!(state.expansion_state(42), ExpansionPhase::Failed);
    }

    #[test]
    fn cancel_expansion_on_loading_reverts_to_collapsed() {
        let mut state = StackState::new(vec![make_frame(1)]);
        state.set_expansion_loading(42);
        state.cancel_expansion(42);
        assert_eq!(state.expansion_state(42), ExpansionPhase::Collapsed);
    }

    #[test]
    fn flat_items_loading_object_includes_loading_node() {
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 99)];
        state.toggle_expand(10, vars);
        state.set_expansion_loading(99);
        let flat = state.flat_items();
        assert!(flat.contains(&StackCursor::OnObjectLoadingNode {
            frame_idx: 0,
            var_idx: 0,
            field_path: vec![],
        }));
    }

    #[test]
    fn flat_items_expanded_with_two_fields_includes_two_object_field_nodes() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 99)];
        state.toggle_expand(10, vars);
        let fields = vec![
            FieldInfo {
                name: "a".to_string(),
                value: FieldValue::Int(1),
            },
            FieldInfo {
                name: "b".to_string(),
                value: FieldValue::Int(2),
            },
        ];
        state.set_expansion_done(99, fields);
        let flat = state.flat_items();
        assert!(flat.contains(&StackCursor::OnObjectField {
            frame_idx: 0,
            var_idx: 0,
            field_path: vec![0],
        }));
        assert!(flat.contains(&StackCursor::OnObjectField {
            frame_idx: 0,
            var_idx: 0,
            field_path: vec![1],
        }));
    }

    #[test]
    fn move_down_from_on_var_expanded_moves_to_first_object_field() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 99)];
        state.toggle_expand(10, vars);
        let fields = vec![FieldInfo {
            name: "x".to_string(),
            value: FieldValue::Int(7),
        }];
        state.set_expansion_done(99, fields);
        // cursor is OnFrame(0), move down → OnVar{0,0}, move down → OnObjectField{0,0,[0]}
        state.move_down();
        assert_eq!(
            state.cursor,
            StackCursor::OnVar {
                frame_idx: 0,
                var_idx: 0
            }
        );
        state.move_down();
        assert_eq!(
            state.cursor,
            StackCursor::OnObjectField {
                frame_idx: 0,
                var_idx: 0,
                field_path: vec![0],
            }
        );
    }

    #[test]
    fn move_down_past_last_object_field_moves_to_next_frame() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10), make_frame(20)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 99)];
        state.toggle_expand(10, vars);
        let fields = vec![FieldInfo {
            name: "x".to_string(),
            value: FieldValue::Int(7),
        }];
        state.set_expansion_done(99, fields);
        // flat: [Frame(0), Var{0,0}, Field{0,0,0}, Frame(1)]
        state.move_down(); // Frame → Var
        state.move_down(); // Var → Field
        state.move_down(); // Field → Frame(1)
        assert_eq!(state.cursor, StackCursor::OnFrame(1));
    }

    #[test]
    fn selected_loading_object_id_on_loading_node_returns_object_id() {
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 42)];
        state.toggle_expand(10, vars);
        state.set_expansion_loading(42);
        // move to the loading node
        state.move_down(); // → OnVar{0,0}
        state.move_down(); // → OnObjectLoadingNode{0,0,field_path:[]}
        assert_eq!(state.selected_loading_object_id(), Some(42));
    }

    // --- Task 4.5 / 5.5: depth-2 navigation and indentation tests ---

    #[test]
    fn flat_items_depth2_expansion_emits_correct_cursor_sequence() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 100)];
        state.toggle_expand(10, vars);
        // Root object 100 has one ObjectRef field pointing to object 200.
        let fields_100 = vec![FieldInfo {
            name: "child".to_string(),
            value: FieldValue::ObjectRef {
                id: 200,
                class_name: "Foo".to_string(),
                entry_count: None,
                inline_value: None,
            },
        }];
        state.set_expansion_done(100, fields_100);
        // Object 200 has one Int field.
        let fields_200 = vec![FieldInfo {
            name: "val".to_string(),
            value: FieldValue::Int(7),
        }];
        state.set_expansion_done(200, fields_200);

        let flat = state.flat_items();
        // Expected: Frame(0), Var{0,0}, Field{0,0,[0]}, Field{0,0,[0,0]}
        assert!(flat.contains(&StackCursor::OnObjectField {
            frame_idx: 0,
            var_idx: 0,
            field_path: vec![0],
        }));
        assert!(flat.contains(&StackCursor::OnObjectField {
            frame_idx: 0,
            var_idx: 0,
            field_path: vec![0, 0],
        }));
    }

    #[test]
    fn selected_field_ref_id_returns_object_ref_id_for_nested_field() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 100)];
        state.toggle_expand(10, vars);
        let fields = vec![FieldInfo {
            name: "child".to_string(),
            value: FieldValue::ObjectRef {
                id: 200,
                class_name: "Bar".to_string(),
                entry_count: None,
                inline_value: None,
            },
        }];
        state.set_expansion_done(100, fields);
        // Navigate to the field at path [0]
        state.cursor = StackCursor::OnObjectField {
            frame_idx: 0,
            var_idx: 0,
            field_path: vec![0],
        };
        assert_eq!(state.selected_field_ref_id(), Some(200));
    }

    #[test]
    fn selected_field_ref_id_returns_none_for_non_object_ref_field() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 100)];
        state.toggle_expand(10, vars);
        let fields = vec![FieldInfo {
            name: "x".to_string(),
            value: FieldValue::Int(42),
        }];
        state.set_expansion_done(100, fields);
        state.cursor = StackCursor::OnObjectField {
            frame_idx: 0,
            var_idx: 0,
            field_path: vec![0],
        };
        assert_eq!(state.selected_field_ref_id(), None);
    }

    // --- Task 7.4: recursive collapse tests ---

    #[test]
    fn collapse_object_recursive_removes_nested_expanded_child() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        // Expand root 100 → child 200
        let fields_100 = vec![FieldInfo {
            name: "child".to_string(),
            value: FieldValue::ObjectRef {
                id: 200,
                class_name: "Foo".to_string(),
                entry_count: None,
                inline_value: None,
            },
        }];
        state.set_expansion_done(100, fields_100);
        let fields_200 = vec![FieldInfo {
            name: "val".to_string(),
            value: FieldValue::Int(1),
        }];
        state.set_expansion_done(200, fields_200);

        assert_eq!(state.expansion_state(100), ExpansionPhase::Expanded);
        assert_eq!(state.expansion_state(200), ExpansionPhase::Expanded);

        state.collapse_object_recursive(100);

        assert_eq!(state.expansion_state(100), ExpansionPhase::Collapsed);
        assert_eq!(state.expansion_state(200), ExpansionPhase::Collapsed);
    }

    #[test]
    fn collapse_object_recursive_cycle_guard_does_not_infinite_loop() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        // Artificial cycle: 100 → 200 → 100 (corrupted heap)
        let fields_100 = vec![FieldInfo {
            name: "c".to_string(),
            value: FieldValue::ObjectRef {
                id: 200,
                class_name: "A".to_string(),
                entry_count: None,
                inline_value: None,
            },
        }];
        state.set_expansion_done(100, fields_100);
        let fields_200 = vec![FieldInfo {
            name: "c".to_string(),
            value: FieldValue::ObjectRef {
                id: 100,
                class_name: "B".to_string(),
                entry_count: None,
                inline_value: None,
            },
        }];
        state.set_expansion_done(200, fields_200);
        // Must terminate without stack overflow
        state.collapse_object_recursive(100);
        assert_eq!(state.expansion_state(100), ExpansionPhase::Collapsed);
        assert_eq!(state.expansion_state(200), ExpansionPhase::Collapsed);
    }

    // --- Task 8.2: frame collapse clears nested expansion ---

    #[test]
    fn toggle_expand_collapse_frame_clears_nested_object_phases() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 100)];
        state.toggle_expand(10, vars);
        // Expand object 100 (nested)
        let fields_100 = vec![FieldInfo {
            name: "x".to_string(),
            value: FieldValue::Int(1),
        }];
        state.set_expansion_done(100, fields_100);
        assert_eq!(state.expansion_state(100), ExpansionPhase::Expanded);
        // Collapse the frame
        state.toggle_expand(10, vec![]);
        // object_phases must be cleaned up
        assert!(state.object_phases.is_empty());
    }

    // --- Task 5.5: build_items indentation test ---

    fn item_text(item: ListItem<'static>) -> String {
        use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};
        let area = Rect::new(0, 0, 120, 1);
        let mut buf = Buffer::empty(area);
        Widget::render(List::new(vec![item]), area, &mut buf);
        buf.content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    #[test]
    fn build_items_depth1_field_has_correct_indent() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 99)];
        state.toggle_expand(10, vars);
        let fields = vec![FieldInfo {
            name: "count".to_string(),
            value: FieldValue::Int(5),
        }];
        state.set_expansion_done(99, fields);
        let items = state.build_items();
        // items[0] = frame, items[1] = var, items[2] = field
        assert_eq!(items.len(), 3);
        let text = item_text(items[2].clone());
        // 4-space indent + 2-char toggle prefix ("  " for primitives)
        assert!(
            text.starts_with("      ") && !text.starts_with("        "),
            "depth-1 field must have 4+2 indent, got: {text:?}"
        );
    }

    #[test]
    fn build_items_depth2_field_has_correct_indent() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 99)];
        state.toggle_expand(10, vars);
        let fields_99 = vec![FieldInfo {
            name: "child".to_string(),
            value: FieldValue::ObjectRef {
                id: 200,
                class_name: "Foo".to_string(),
                entry_count: None,
                inline_value: None,
            },
        }];
        state.set_expansion_done(99, fields_99);
        let fields_200 = vec![FieldInfo {
            name: "val".to_string(),
            value: FieldValue::Int(7),
        }];
        state.set_expansion_done(200, fields_200);
        let items = state.build_items();
        // items[0]=frame, [1]=var, [2]=depth-1 field, [3]=depth-2 field
        assert_eq!(items.len(), 4);
        let depth1 = item_text(items[2].clone());
        // 4-space indent + 2-char toggle ("- " for expanded ObjectRef)
        assert!(
            depth1.starts_with("    - "),
            "depth-1 ObjectRef field must have toggle prefix, got: {depth1:?}"
        );
        let depth2 = item_text(items[3].clone());
        // 6-space indent + 2-char toggle ("  " for primitive)
        assert!(
            depth2.starts_with("        ") && !depth2.starts_with("          "),
            "depth-2 field must have 6+2 indent, got: {depth2:?}"
        );
    }

    #[test]
    fn build_items_failed_expansion_shows_error_message_with_correct_indent() {
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 99)];
        state.toggle_expand(10, vars);
        state.set_expansion_failed(99, "Failed to resolve object".to_string());
        let items = state.build_items();
        // items[0]=frame, [1]=var, [2]=error node at depth 1 (4 spaces)
        assert_eq!(items.len(), 3);
        let text = item_text(items[2].clone());
        assert!(
            text.starts_with("    "),
            "error node must have 4-space indent, got: {text:?}"
        );
        assert!(
            text.contains("! Failed to resolve object"),
            "error node must contain error message, got: {text:?}"
        );
    }

    // --- Cyclic reference detection tests ---

    #[test]
    fn flat_items_self_ref_emits_cyclic_node() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 100)];
        state.toggle_expand(10, vars);
        let fields = vec![FieldInfo {
            name: "self".to_string(),
            value: FieldValue::ObjectRef {
                id: 100,
                class_name: "Node".to_string(),
                entry_count: None,
                inline_value: None,
            },
        }];
        state.set_expansion_done(100, fields);
        let flat = state.flat_items();
        let cyclic_count = flat
            .iter()
            .filter(|c| matches!(c, StackCursor::OnCyclicNode { .. }))
            .count();
        assert_eq!(cyclic_count, 1);
        let deep_fields = flat
            .iter()
            .filter(|c| {
                matches!(
                    c,
                    StackCursor::OnObjectField {
                        field_path, ..
                    } if field_path.len() > 1
                )
            })
            .count();
        assert_eq!(deep_fields, 0, "no recursive fields beyond depth 1");
    }

    #[test]
    fn flat_items_multi_self_ref_emits_two_cyclic_nodes() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 100)];
        state.toggle_expand(10, vars);
        let fields = vec![
            FieldInfo {
                name: "left".to_string(),
                value: FieldValue::ObjectRef {
                    id: 100,
                    class_name: "Node".to_string(),
                    entry_count: None,
                    inline_value: None,
                },
            },
            FieldInfo {
                name: "right".to_string(),
                value: FieldValue::ObjectRef {
                    id: 100,
                    class_name: "Node".to_string(),
                    entry_count: None,
                    inline_value: None,
                },
            },
        ];
        state.set_expansion_done(100, fields);
        let flat = state.flat_items();
        let cyclic_count = flat
            .iter()
            .filter(|c| matches!(c, StackCursor::OnCyclicNode { .. }))
            .count();
        assert_eq!(cyclic_count, 2);
    }

    #[test]
    fn build_items_self_ref_renders_self_ref_marker() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 100)];
        state.toggle_expand(10, vars);
        let fields = vec![FieldInfo {
            name: "me".to_string(),
            value: FieldValue::ObjectRef {
                id: 100,
                class_name: "java.lang.Thread".to_string(),
                entry_count: None,
                inline_value: None,
            },
        }];
        state.set_expansion_done(100, fields);
        let items = state.build_items();
        let text = item_text(items[2].clone());
        assert!(text.contains("\u{21BB}"), "must contain ↻, got: {text:?}");
        assert!(
            text.contains("[self-ref]"),
            "must contain [self-ref], got: {text:?}"
        );
        assert!(
            text.contains("Thread"),
            "must show short class name, got: {text:?}"
        );
        assert!(
            !text.contains("java.lang.Thread"),
            "must NOT show FQCN, got: {text:?}"
        );
    }

    #[test]
    fn flat_items_indirect_cycle_emits_cyclic_node() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 100)];
        state.toggle_expand(10, vars);
        // A(100) → B(200)
        let fields_a = vec![FieldInfo {
            name: "child".to_string(),
            value: FieldValue::ObjectRef {
                id: 200,
                class_name: "B".to_string(),
                entry_count: None,
                inline_value: None,
            },
        }];
        state.set_expansion_done(100, fields_a);
        // B(200) → A(100) (back-reference)
        let fields_b = vec![FieldInfo {
            name: "parent".to_string(),
            value: FieldValue::ObjectRef {
                id: 100,
                class_name: "A".to_string(),
                entry_count: None,
                inline_value: None,
            },
        }];
        state.set_expansion_done(200, fields_b);
        let flat = state.flat_items();
        let cyclic_count = flat
            .iter()
            .filter(|c| matches!(c, StackCursor::OnCyclicNode { .. }))
            .count();
        assert_eq!(cyclic_count, 1, "B's back-ref to A should be cyclic");
        // Should not recurse 16 levels deep
        let max_depth = flat
            .iter()
            .filter_map(|c| match c {
                StackCursor::OnObjectField { field_path, .. } => Some(field_path.len()),
                StackCursor::OnCyclicNode { field_path, .. } => Some(field_path.len()),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        assert!(max_depth <= 3, "no deep recursion, max depth: {max_depth}");
    }

    #[test]
    fn build_items_indirect_cycle_renders_cyclic_marker() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 100)];
        state.toggle_expand(10, vars);
        let fields_a = vec![FieldInfo {
            name: "child".to_string(),
            value: FieldValue::ObjectRef {
                id: 200,
                class_name: "B".to_string(),
                entry_count: None,
                inline_value: None,
            },
        }];
        state.set_expansion_done(100, fields_a);
        let fields_b = vec![FieldInfo {
            name: "parent".to_string(),
            value: FieldValue::ObjectRef {
                id: 100,
                class_name: "A".to_string(),
                entry_count: None,
                inline_value: None,
            },
        }];
        state.set_expansion_done(200, fields_b);
        let items = state.build_items();
        let all_text: Vec<String> = items.into_iter().map(item_text).collect();
        let cyclic_line = all_text.iter().find(|t| t.contains("[cyclic]"));
        assert!(
            cyclic_line.is_some(),
            "must have [cyclic] marker, items: {all_text:?}"
        );
        let line = cyclic_line.unwrap();
        assert!(line.contains("\u{21BB}"), "must contain ↻, got: {line:?}");
        assert!(
            !line.contains("[self-ref]"),
            "indirect cycle must NOT show [self-ref]"
        );
    }

    #[test]
    fn move_down_up_across_cyclic_node() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 100)];
        state.toggle_expand(10, vars);
        let fields = vec![
            FieldInfo {
                name: "a".to_string(),
                value: FieldValue::Int(1),
            },
            FieldInfo {
                name: "b".to_string(),
                value: FieldValue::ObjectRef {
                    id: 100,
                    class_name: "Node".to_string(),
                    entry_count: None,
                    inline_value: None,
                },
            },
            FieldInfo {
                name: "c".to_string(),
                value: FieldValue::Int(3),
            },
        ];
        state.set_expansion_done(100, fields);
        // flat: Frame(0), Var{0,0}, Field[0](Int),
        //       CyclicNode[1](self-ref), Field[2](Int)
        state.move_down(); // Frame → Var
        state.move_down(); // Var → Field[0]
        assert!(matches!(
            &state.cursor,
            StackCursor::OnObjectField { field_path, .. }
            if *field_path == vec![0]
        ));
        state.move_down(); // Field[0] → CyclicNode[1]
        assert!(matches!(
            &state.cursor,
            StackCursor::OnCyclicNode { field_path, .. }
            if *field_path == vec![1]
        ));
        state.move_down(); // CyclicNode[1] → Field[2]
        assert!(matches!(
            &state.cursor,
            StackCursor::OnObjectField { field_path, .. }
            if *field_path == vec![2]
        ));
        // Now go back up
        state.move_up(); // Field[2] → CyclicNode[1]
        assert!(matches!(
            &state.cursor,
            StackCursor::OnCyclicNode { field_path, .. }
            if *field_path == vec![1]
        ));
        state.move_up(); // CyclicNode[1] → Field[0]
        assert!(matches!(
            &state.cursor,
            StackCursor::OnObjectField { field_path, .. }
            if *field_path == vec![0]
        ));
    }

    #[test]
    fn flat_items_acyclic_tree_no_cyclic_nodes() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 100)];
        state.toggle_expand(10, vars);
        // A(100) → B(200) → C(300), no cycles
        let fields_a = vec![FieldInfo {
            name: "b".to_string(),
            value: FieldValue::ObjectRef {
                id: 200,
                class_name: "B".to_string(),
                entry_count: None,
                inline_value: None,
            },
        }];
        state.set_expansion_done(100, fields_a);
        let fields_b = vec![FieldInfo {
            name: "c".to_string(),
            value: FieldValue::ObjectRef {
                id: 300,
                class_name: "C".to_string(),
                entry_count: None,
                inline_value: None,
            },
        }];
        state.set_expansion_done(200, fields_b);
        let fields_c = vec![FieldInfo {
            name: "val".to_string(),
            value: FieldValue::Int(42),
        }];
        state.set_expansion_done(300, fields_c);
        let flat = state.flat_items();
        let cyclic_count = flat
            .iter()
            .filter(|c| matches!(c, StackCursor::OnCyclicNode { .. }))
            .count();
        assert_eq!(cyclic_count, 0, "acyclic tree must have zero cyclic nodes");
        // Should have fields at depths 1, 2, 3
        assert!(flat.contains(&StackCursor::OnObjectField {
            frame_idx: 0,
            var_idx: 0,
            field_path: vec![0],
        }));
        assert!(flat.contains(&StackCursor::OnObjectField {
            frame_idx: 0,
            var_idx: 0,
            field_path: vec![0, 0],
        }));
        assert!(flat.contains(&StackCursor::OnObjectField {
            frame_idx: 0,
            var_idx: 0,
            field_path: vec![0, 0, 0],
        }));
    }

    #[test]
    fn flat_items_diamond_shared_object_no_false_positive() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 100)];
        state.toggle_expand(10, vars);
        // A(100) has two fields both pointing to C(300)
        let fields_a = vec![
            FieldInfo {
                name: "left".to_string(),
                value: FieldValue::ObjectRef {
                    id: 300,
                    class_name: "C".to_string(),
                    entry_count: None,
                    inline_value: None,
                },
            },
            FieldInfo {
                name: "right".to_string(),
                value: FieldValue::ObjectRef {
                    id: 300,
                    class_name: "C".to_string(),
                    entry_count: None,
                    inline_value: None,
                },
            },
        ];
        state.set_expansion_done(100, fields_a);
        let fields_c = vec![FieldInfo {
            name: "val".to_string(),
            value: FieldValue::Int(42),
        }];
        state.set_expansion_done(300, fields_c);
        let flat = state.flat_items();
        // C is shared but NOT an ancestor — no cyclic nodes
        let cyclic_count = flat
            .iter()
            .filter(|c| matches!(c, StackCursor::OnCyclicNode { .. }))
            .count();
        assert_eq!(
            cyclic_count, 0,
            "diamond/shared object must not be a false positive"
        );
        // C's field should appear under both left and right
        assert!(flat.contains(&StackCursor::OnObjectField {
            frame_idx: 0,
            var_idx: 0,
            field_path: vec![0, 0],
        }));
        assert!(flat.contains(&StackCursor::OnObjectField {
            frame_idx: 0,
            var_idx: 0,
            field_path: vec![1, 0],
        }));
    }

    #[test]
    fn collapse_cyclic_child_resyncs_cursor_to_var() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10), make_frame(20)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 1000)];
        state.toggle_expand(10, vars);
        // Thread(1000) → parkBlocker field → Coroutine(2000)
        let thread_fields = vec![FieldInfo {
            name: "parkBlocker".to_string(),
            value: FieldValue::ObjectRef {
                id: 2000,
                class_name: "Coroutine".to_string(),
                entry_count: None,
                inline_value: None,
            },
        }];
        state.set_expansion_done(1000, thread_fields);
        // Coroutine(2000) → blockedThread → Thread(1000) (cycle)
        let coroutine_fields = vec![FieldInfo {
            name: "blockedThread".to_string(),
            value: FieldValue::ObjectRef {
                id: 1000,
                class_name: "Thread".to_string(),
                entry_count: None,
                inline_value: None,
            },
        }];
        state.set_expansion_done(2000, coroutine_fields);

        // Navigate to parkBlocker field (path [0])
        state.cursor = StackCursor::OnObjectField {
            frame_idx: 0,
            var_idx: 0,
            field_path: vec![0],
        };

        // Collapse the nested Coroutine object.
        // collect_descendants(2000) follows back-ref to 1000,
        // collapsing BOTH objects. Cursor becomes orphaned.
        state.collapse_object_recursive(2000);

        // Cursor must have been resynced — not stuck
        let flat = state.flat_items();
        assert!(
            flat.contains(&state.cursor),
            "cursor must be in flat_items after collapse, got: {:?}",
            state.cursor,
        );
        // Should have fallen back to OnVar
        assert!(
            matches!(
                &state.cursor,
                StackCursor::OnVar {
                    frame_idx: 0,
                    var_idx: 0,
                }
            ),
            "cursor should fall back to OnVar, got: {:?}",
            state.cursor,
        );

        // Navigation must work again
        state.move_down();
        assert_ne!(
            state.cursor,
            StackCursor::OnVar {
                frame_idx: 0,
                var_idx: 0,
            },
            "move_down must move away from OnVar"
        );
    }

    #[test]
    fn collapse_nested_non_recursive_preserves_parent() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 1000)];
        state.toggle_expand(10, vars);
        // Thread(1000) → parkBlocker → Coroutine(2000)
        let thread_fields = vec![FieldInfo {
            name: "parkBlocker".to_string(),
            value: FieldValue::ObjectRef {
                id: 2000,
                class_name: "Coroutine".to_string(),
                entry_count: None,
                inline_value: None,
            },
        }];
        state.set_expansion_done(1000, thread_fields);
        // Coroutine(2000) → blockedThread → Thread(1000)
        let coroutine_fields = vec![FieldInfo {
            name: "blockedThread".to_string(),
            value: FieldValue::ObjectRef {
                id: 1000,
                class_name: "Thread".to_string(),
                entry_count: None,
                inline_value: None,
            },
        }];
        state.set_expansion_done(2000, coroutine_fields);

        // Cursor on parkBlocker field
        state.cursor = StackCursor::OnObjectField {
            frame_idx: 0,
            var_idx: 0,
            field_path: vec![0],
        };

        // Non-recursive collapse (CollapseNestedObj path):
        // only collapses 2000, NOT 1000.
        state.collapse_object(2000);

        // Thread(1000) must still be expanded
        assert_eq!(
            state.expansion_state(1000),
            ExpansionPhase::Expanded,
            "parent object must remain expanded"
        );
        // Coroutine(2000) must be collapsed
        assert_eq!(state.expansion_state(2000), ExpansionPhase::Collapsed,);
        // Cursor stays on the parkBlocker field
        let flat = state.flat_items();
        assert!(flat.contains(&state.cursor), "cursor must still be valid");
        assert!(matches!(
            &state.cursor,
            StackCursor::OnObjectField {
                field_path, ..
            } if *field_path == vec![0]
        ));
    }

    #[test]
    fn chunk_ranges_total_50_no_sections() {
        let ranges = compute_chunk_ranges(50);
        assert!(ranges.is_empty());
    }

    #[test]
    fn chunk_ranges_total_100_no_sections() {
        let ranges = compute_chunk_ranges(100);
        assert!(ranges.is_empty());
    }

    #[test]
    fn chunk_ranges_total_150() {
        let ranges = compute_chunk_ranges(150);
        assert_eq!(ranges, vec![(100, 50)]);
    }

    #[test]
    fn chunk_ranges_total_500() {
        let ranges = compute_chunk_ranges(500);
        assert_eq!(
            ranges,
            vec![(100, 100), (200, 100), (300, 100), (400, 100),]
        );
    }

    #[test]
    fn chunk_ranges_total_1000() {
        let ranges = compute_chunk_ranges(1000);
        assert_eq!(ranges.len(), 9);
        assert_eq!(ranges[0], (100, 100));
        assert_eq!(ranges[8], (900, 100));
    }

    #[test]
    fn chunk_ranges_total_3000() {
        let ranges = compute_chunk_ranges(3000);
        // 9 sections of 100 (100..999) + 2 of 1000
        assert_eq!(ranges.len(), 11);
        assert_eq!(ranges[0], (100, 100));
        assert_eq!(ranges[8], (900, 100));
        assert_eq!(ranges[9], (1000, 1000));
        assert_eq!(ranges[10], (2000, 1000));
    }

    #[test]
    fn chunk_ranges_total_2348() {
        let ranges = compute_chunk_ranges(2348);
        assert_eq!(ranges.len(), 11);
        assert_eq!(ranges[9], (1000, 1000));
        assert_eq!(ranges[10], (2000, 348));
    }

    #[test]
    fn page_down_jumps_by_visible_height() {
        // 30 frames, cursor at frame 5, height 20 → frame 25
        let frames: Vec<_> = (1..=30).map(make_frame).collect();
        let mut state = StackState::new(frames);
        state.set_visible_height(20);
        // Move cursor to frame 5
        for _ in 0..5 {
            state.move_down();
        }
        assert_eq!(state.cursor, StackCursor::OnFrame(5));
        state.move_page_down();
        assert_eq!(state.cursor, StackCursor::OnFrame(25));
    }

    #[test]
    fn page_up_jumps_by_visible_height() {
        // 30 frames, cursor at frame 25, height 20 → frame 5
        let frames: Vec<_> = (1..=30).map(make_frame).collect();
        let mut state = StackState::new(frames);
        state.set_visible_height(20);
        for _ in 0..25 {
            state.move_down();
        }
        assert_eq!(state.cursor, StackCursor::OnFrame(25));
        state.move_page_up();
        assert_eq!(state.cursor, StackCursor::OnFrame(5));
    }

    #[test]
    fn page_down_clamps_to_last_item() {
        let frames: Vec<_> = (1..=10).map(make_frame).collect();
        let mut state = StackState::new(frames);
        state.set_visible_height(20);
        state.move_page_down();
        assert_eq!(state.cursor, StackCursor::OnFrame(9));
    }

    #[test]
    fn page_up_clamps_to_first_item() {
        let frames: Vec<_> = (1..=10).map(make_frame).collect();
        let mut state = StackState::new(frames);
        state.set_visible_height(20);
        for _ in 0..3 {
            state.move_down();
        }
        state.move_page_up();
        assert_eq!(state.cursor, StackCursor::OnFrame(0));
    }
}
