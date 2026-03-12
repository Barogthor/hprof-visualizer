//! [`StackState`] — mutable state for the stack frame panel.

use std::collections::{HashMap, HashSet};

use hprof_engine::{FieldInfo, FieldValue, FrameInfo, VariableInfo, VariableValue};
use ratatui::{
    text::{Line, Span},
    widgets::{ListItem, ListState},
};

use crate::theme::THEME;
use crate::views::cursor::CursorState;

use super::expansion::ExpansionRegistry;
use super::format::{collect_descendants, compute_chunk_ranges, format_frame_label};
use super::types::{
    ChunkState, CollectionChunks, ExpansionPhase, STATIC_FIELDS_RENDER_LIMIT, StackCursor,
};

/// State for the stack frame panel.
pub struct StackState {
    // === Frames & Vars ===
    pub(super) frames: Vec<FrameInfo>,
    /// Vars per frame_id — populated on demand by `App` calling the engine.
    pub(super) vars: HashMap<u64, Vec<VariableInfo>>,
    pub(super) expanded: HashSet<u64>,
    // === Cursor & Navigation ===
    pub(super) nav: CursorState<StackCursor>,
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
        let mut out = Self {
            frames,
            vars: HashMap::new(),
            expanded: HashSet::new(),
            nav: CursorState::new(cursor),
            expansion: ExpansionRegistry::new(),
        };
        let flat = out.flat_items();
        out.nav.sync(&flat);
        out
    }

    /// Returns the frame_id currently selected, if any.
    pub fn selected_frame_id(&self) -> Option<u64> {
        match self.nav.cursor() {
            StackCursor::NoFrames => None,
            StackCursor::OnFrame(fi) => self.frames.get(*fi).map(|f| f.frame_id),
            StackCursor::OnVar { frame_idx, .. }
            | StackCursor::OnObjectField { frame_idx, .. }
            | StackCursor::OnObjectLoadingNode { frame_idx, .. }
            | StackCursor::OnCyclicNode { frame_idx, .. }
            | StackCursor::OnStaticSectionHeader { frame_idx, .. }
            | StackCursor::OnStaticField { frame_idx, .. }
            | StackCursor::OnStaticOverflowRow { frame_idx, .. }
            | StackCursor::OnStaticObjectField { frame_idx, .. }
            | StackCursor::OnChunkSection { frame_idx, .. }
            | StackCursor::OnCollectionEntry { frame_idx, .. }
            | StackCursor::OnCollectionEntryObjField { frame_idx, .. }
            | StackCursor::OnCollectionEntryStaticSectionHeader { frame_idx, .. }
            | StackCursor::OnCollectionEntryStaticField { frame_idx, .. }
            | StackCursor::OnCollectionEntryStaticOverflowRow { frame_idx, .. }
            | StackCursor::OnCollectionEntryStaticObjectField { frame_idx, .. } => {
                self.frames.get(*frame_idx).map(|f| f.frame_id)
            }
        }
    }

    /// Returns the current cursor.
    pub fn cursor(&self) -> &StackCursor {
        self.nav.cursor()
    }

    /// Returns mutable access to ratatui list state for rendering.
    pub fn list_state_mut(&mut self) -> &mut ListState {
        self.nav.list_state_mut()
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
        self.nav.set_cursor_and_sync(new_cursor, &self.flat_items());
    }

    /// Returns the object_id if the cursor is on an `ObjectRef` var.
    pub fn selected_object_id(&self) -> Option<u64> {
        if let StackCursor::OnVar { frame_idx, var_idx } = self.nav.cursor() {
            let frame = self.frames.get(*frame_idx)?;
            let vars = self.vars.get(&frame.frame_id)?;
            let var = vars.get(*var_idx)?;
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
        } = self.nav.cursor()
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
        } = self.nav.cursor()
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
            field_path,
        } = self.nav.cursor()
        {
            let frame = self.frames.get(*frame_idx)?;
            let vars = self.vars.get(&frame.frame_id)?;
            let var = vars.get(*var_idx)?;
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

    fn static_owner_object_id(
        &self,
        frame_idx: usize,
        var_idx: usize,
        field_path: &[usize],
    ) -> Option<u64> {
        let frame = self.frames.get(frame_idx)?;
        let vars = self.vars.get(&frame.frame_id)?;
        let var = vars.get(var_idx)?;
        let VariableValue::ObjectRef { id: root_id, .. } = var.value else {
            return None;
        };
        if field_path.is_empty() {
            Some(root_id)
        } else {
            Some(self.resolve_object_at_path(root_id, field_path))
        }
    }

    fn static_field_for(
        &self,
        frame_idx: usize,
        var_idx: usize,
        field_path: &[usize],
        static_idx: usize,
    ) -> Option<&FieldInfo> {
        let owner_id = self.static_owner_object_id(frame_idx, var_idx, field_path)?;
        let static_fields = self.expansion.object_static_fields.get(&owner_id)?;
        static_fields.get(static_idx)
    }

    fn static_field_under_cursor(&self) -> Option<&FieldInfo> {
        if let StackCursor::OnStaticField {
            frame_idx,
            var_idx,
            field_path,
            static_idx,
        } = self.nav.cursor()
        {
            return self.static_field_for(*frame_idx, *var_idx, field_path, *static_idx);
        }
        None
    }

    /// Returns the `ObjectRef` id when cursor is `OnStaticField`
    /// and that static field value is an `ObjectRef`.
    pub fn selected_static_field_ref_id(&self) -> Option<u64> {
        let field = self.static_field_under_cursor()?;
        if let FieldValue::ObjectRef { id, .. } = field.value {
            return Some(id);
        }
        None
    }

    /// Returns `(object_id, entry_count)` when cursor is `OnStaticField`
    /// pointing to a collection/array `ObjectRef` value.
    pub fn selected_static_field_collection_info(&self) -> Option<(u64, u64)> {
        let field = self.static_field_under_cursor()?;
        if let FieldValue::ObjectRef {
            id,
            entry_count: Some(ec),
            ..
        } = field.value
            && ec > 0
        {
            return Some((id, ec));
        }
        None
    }

    /// Resolves the field under cursor when on `OnStaticObjectField`.
    ///
    /// Returns `None` for cyclic terminal rows so Enter is a no-op.
    fn static_obj_cursor_field(&self) -> Option<&FieldInfo> {
        if let StackCursor::OnStaticObjectField {
            frame_idx,
            var_idx,
            field_path,
            static_idx,
            obj_field_path,
        } = self.nav.cursor()
        {
            let static_root_id = {
                let field = self.static_field_for(*frame_idx, *var_idx, field_path, *static_idx)?;
                if let FieldValue::ObjectRef { id, .. } = field.value {
                    id
                } else {
                    return None;
                }
            };

            let mut ancestor_ids = HashSet::new();
            ancestor_ids.insert(static_root_id);

            let parent_path = &obj_field_path[..obj_field_path.len().saturating_sub(1)];
            let mut parent_id = static_root_id;
            for &step in parent_path {
                let parent_fields = self.expansion.object_fields.get(&parent_id)?;
                let parent_field = parent_fields.get(step)?;
                if let FieldValue::ObjectRef { id, .. } = parent_field.value {
                    parent_id = id;
                    ancestor_ids.insert(parent_id);
                } else {
                    return None;
                }
            }

            let field_idx = *obj_field_path.last()?;
            let fields = self.expansion.object_fields.get(&parent_id)?;
            let field = fields.get(field_idx)?;
            if let FieldValue::ObjectRef { id, .. } = field.value
                && ancestor_ids.contains(&id)
            {
                return None;
            }
            return Some(field);
        }
        None
    }

    /// Returns the `ObjectRef` id when cursor is `OnStaticObjectField`
    /// pointing to an `ObjectRef` field.
    pub fn selected_static_obj_field_ref_id(&self) -> Option<u64> {
        let field = self.static_obj_cursor_field()?;
        if let FieldValue::ObjectRef { id, .. } = field.value {
            return Some(id);
        }
        None
    }

    /// Returns `(object_id, entry_count)` when cursor is `OnStaticObjectField`
    /// pointing to a collection/array field.
    pub fn selected_static_obj_field_collection_info(&self) -> Option<(u64, u64)> {
        let field = self.static_obj_cursor_field()?;
        if let FieldValue::ObjectRef {
            id,
            entry_count: Some(ec),
            ..
        } = field.value
            && ec > 0
        {
            return Some((id, ec));
        }
        None
    }

    fn collection_entry_object_id(&self, collection_id: u64, entry_index: usize) -> Option<u64> {
        let cc = self.expansion.collection_chunks.get(&collection_id)?;
        let entry = cc.find_entry(entry_index)?;
        if let FieldValue::ObjectRef { id, .. } = &entry.value {
            Some(*id)
        } else {
            None
        }
    }

    fn resolve_collection_entry_object_at_path(
        &self,
        collection_id: u64,
        entry_index: usize,
        obj_field_path: &[usize],
    ) -> Option<u64> {
        let mut current = self.collection_entry_object_id(collection_id, entry_index)?;
        for &step in obj_field_path {
            let fields = self.expansion.object_fields.get(&current)?;
            let field = fields.get(step)?;
            if let FieldValue::ObjectRef { id, .. } = field.value {
                current = id;
            } else {
                return None;
            }
        }
        Some(current)
    }

    fn collection_entry_static_field_for(
        &self,
        collection_id: u64,
        entry_index: usize,
        obj_field_path: &[usize],
        static_idx: usize,
    ) -> Option<&FieldInfo> {
        let owner_id = self.resolve_collection_entry_object_at_path(
            collection_id,
            entry_index,
            obj_field_path,
        )?;
        let static_fields = self.expansion.object_static_fields.get(&owner_id)?;
        static_fields.get(static_idx)
    }

    /// Returns the `ObjectRef` id when cursor is
    /// `OnCollectionEntryStaticField` and that static value is an `ObjectRef`.
    pub fn selected_collection_entry_static_field_ref_id(&self) -> Option<u64> {
        if let StackCursor::OnCollectionEntryStaticField {
            collection_id,
            entry_index,
            obj_field_path,
            static_idx,
            ..
        } = self.nav.cursor()
        {
            let field = self.collection_entry_static_field_for(
                *collection_id,
                *entry_index,
                obj_field_path,
                *static_idx,
            )?;
            if let FieldValue::ObjectRef { id, .. } = field.value {
                return Some(id);
            }
        }
        None
    }

    /// Returns `(object_id, entry_count)` when cursor is
    /// `OnCollectionEntryStaticField` pointing to a collection/array static
    /// value.
    pub fn selected_collection_entry_static_field_collection_info(&self) -> Option<(u64, u64)> {
        if let StackCursor::OnCollectionEntryStaticField {
            collection_id,
            entry_index,
            obj_field_path,
            static_idx,
            ..
        } = self.nav.cursor()
        {
            let field = self.collection_entry_static_field_for(
                *collection_id,
                *entry_index,
                obj_field_path,
                *static_idx,
            )?;
            if let FieldValue::ObjectRef {
                id,
                entry_count: Some(ec),
                ..
            } = field.value
            {
                if id == 0 || ec == 0 {
                    return None;
                }
                return Some((id, ec));
            }
        }
        None
    }

    /// Resolves the field under cursor when on
    /// `OnCollectionEntryStaticObjectField`.
    ///
    /// Returns `None` for cyclic terminal rows so Enter is a no-op.
    fn collection_entry_static_obj_cursor_field(&self) -> Option<&FieldInfo> {
        if let StackCursor::OnCollectionEntryStaticObjectField {
            collection_id,
            entry_index,
            obj_field_path,
            static_idx,
            static_obj_field_path,
            ..
        } = self.nav.cursor()
        {
            let static_root_id = {
                let field = self.collection_entry_static_field_for(
                    *collection_id,
                    *entry_index,
                    obj_field_path,
                    *static_idx,
                )?;
                if let FieldValue::ObjectRef { id, .. } = field.value {
                    id
                } else {
                    return None;
                }
            };

            let mut ancestor_ids = HashSet::new();
            ancestor_ids.insert(static_root_id);

            let parent_path =
                &static_obj_field_path[..static_obj_field_path.len().saturating_sub(1)];
            let mut parent_id = static_root_id;
            for &step in parent_path {
                let parent_fields = self.expansion.object_fields.get(&parent_id)?;
                let parent_field = parent_fields.get(step)?;
                if let FieldValue::ObjectRef { id, .. } = parent_field.value {
                    parent_id = id;
                    ancestor_ids.insert(parent_id);
                } else {
                    return None;
                }
            }

            let field_idx = *static_obj_field_path.last()?;
            let fields = self.expansion.object_fields.get(&parent_id)?;
            let field = fields.get(field_idx)?;
            if let FieldValue::ObjectRef { id, .. } = field.value
                && ancestor_ids.contains(&id)
            {
                return None;
            }
            return Some(field);
        }
        None
    }

    /// Returns the `ObjectRef` id when cursor is
    /// `OnCollectionEntryStaticObjectField` and the field is an `ObjectRef`.
    pub fn selected_collection_entry_static_obj_field_ref_id(&self) -> Option<u64> {
        let field = self.collection_entry_static_obj_cursor_field()?;
        if let FieldValue::ObjectRef { id, .. } = field.value {
            return Some(id);
        }
        None
    }

    /// Returns `(object_id, entry_count)` when cursor is
    /// `OnCollectionEntryStaticObjectField` and the field is a
    /// collection/array `ObjectRef`.
    pub fn selected_collection_entry_static_obj_field_collection_info(&self) -> Option<(u64, u64)> {
        let field = self.collection_entry_static_obj_cursor_field()?;
        if let FieldValue::ObjectRef {
            id,
            entry_count: Some(ec),
            ..
        } = field.value
        {
            if id == 0 || ec == 0 {
                return None;
            }
            return Some((id, ec));
        }
        None
    }

    /// Returns `Some(entry_count)` if the currently selected variable is a collection
    /// or array, `None` otherwise.
    pub fn selected_var_entry_count(&self) -> Option<u64> {
        let StackCursor::OnVar { frame_idx, var_idx } = self.nav.cursor() else {
            return None;
        };
        let frame = self.frames.get(*frame_idx)?;
        let vars = self.vars.get(&frame.frame_id)?;
        let var = vars.get(*var_idx)?;
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
        } = self.nav.cursor()
        else {
            return None;
        };
        let cc = self.expansion.collection_chunks.get(collection_id)?;
        let entry = cc.find_entry(*entry_index)?;
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
        } = self.nav.cursor()
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
        } = self.nav.cursor()
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

    /// Returns `(collection_id, entry_count)` when the cursor is
    /// `OnCollectionEntryObjField` pointing to a collection/array field.
    pub fn selected_collection_entry_obj_field_collection_info(&self) -> Option<(u64, u64)> {
        let field = self.collection_entry_obj_cursor_field()?;
        if let FieldValue::ObjectRef {
            id,
            entry_count: Some(ec),
            ..
        } = field.value
        {
            if id == 0 {
                return None;
            }
            return Some((id, ec));
        }
        None
    }

    /// Resolves the field under the cursor when on
    /// `OnCollectionEntryObjField`.
    ///
    /// Returns `None` for cyclic terminal rows so Enter is a no-op.
    fn collection_entry_obj_cursor_field(&self) -> Option<&FieldInfo> {
        if let StackCursor::OnCollectionEntryObjField {
            collection_id,
            entry_index,
            obj_field_path,
            ..
        } = self.nav.cursor()
        {
            let root_id = {
                let cc = self.expansion.collection_chunks.get(collection_id)?;
                let entry = cc.find_entry(*entry_index)?;
                if let FieldValue::ObjectRef { id, .. } = &entry.value {
                    *id
                } else {
                    return None;
                }
            };

            let mut ancestor_ids = HashSet::new();
            ancestor_ids.insert(root_id);

            let parent_path = &obj_field_path[..obj_field_path.len().saturating_sub(1)];
            let mut parent_id = root_id;
            for &step in parent_path {
                let parent_fields = self.expansion.object_fields.get(&parent_id)?;
                let parent_field = parent_fields.get(step)?;
                if let FieldValue::ObjectRef { id, .. } = parent_field.value {
                    parent_id = id;
                    ancestor_ids.insert(parent_id);
                } else {
                    return None;
                }
            }

            let field_idx = *obj_field_path.last()?;
            let fields = self.expansion.object_fields.get(&parent_id)?;
            let field = fields.get(field_idx)?;
            if let FieldValue::ObjectRef { id, .. } = field.value
                && ancestor_ids.contains(&id)
            {
                return None;
            }
            return Some(field);
        }
        None
    }

    /// Returns the `ChunkState` for a specific chunk.
    pub fn chunk_state(&self, collection_id: u64, chunk_offset: usize) -> Option<&ChunkState> {
        self.expansion.chunk_state(collection_id, chunk_offset)
    }

    /// Returns the logical parent cursor for the current position, or `None`
    /// if at the top level (`OnFrame` or `NoFrames`).
    ///
    /// Does NOT modify state — only computes where the cursor should go.
    ///
    /// Parent relationships:
    /// - `OnVar` → `OnFrame`
    /// - `OnObjectField { path: [x] }` → `OnVar`
    /// - `OnObjectField { path: [x, y, ...] }` → `OnObjectField` with last element dropped
    /// - `OnObjectLoadingNode` / `OnCyclicNode` → same rule as `OnObjectField`
    /// - `OnStaticSectionHeader` / `OnStaticField` / `OnStaticOverflowRow` →
    ///   same rule as `OnObjectField`
    /// - `OnStaticObjectField { obj_field_path: [x] }` → `OnStaticField`
    /// - `OnStaticObjectField { obj_field_path: [x, y, ...] }` → truncate
    ///   `obj_field_path`
    /// - `OnCollectionEntry { field_path: [] }` → `OnVar`
    /// - `OnCollectionEntry { field_path: [x, ...] }` → `OnObjectField { field_path }`
    /// - `OnChunkSection` → same rule as `OnCollectionEntry`
    /// - `OnCollectionEntryObjField { obj_field_path: [x] }` → `OnCollectionEntry`
    /// - `OnCollectionEntryObjField { obj_field_path: [x, y, ...] }` → truncate
    ///   `obj_field_path`
    /// - `OnCollectionEntryStaticObjectField { static_obj_field_path: [x] }`
    ///   → `OnCollectionEntryStaticField`
    /// - `OnCollectionEntryStaticObjectField { static_obj_field_path: [x, y, ...] }`
    ///   → truncate `static_obj_field_path`
    pub fn parent_cursor(&self) -> Option<StackCursor> {
        match &self.nav.cursor().clone() {
            StackCursor::NoFrames | StackCursor::OnFrame(_) => None,
            StackCursor::OnVar { frame_idx, .. } => Some(StackCursor::OnFrame(*frame_idx)),
            StackCursor::OnObjectField {
                frame_idx,
                var_idx,
                field_path,
            }
            | StackCursor::OnObjectLoadingNode {
                frame_idx,
                var_idx,
                field_path,
            }
            | StackCursor::OnCyclicNode {
                frame_idx,
                var_idx,
                field_path,
            } => {
                // len() == 0: edge case (story 9.2). Both 0 and 1 map to OnVar.
                if field_path.len() <= 1 {
                    Some(StackCursor::OnVar {
                        frame_idx: *frame_idx,
                        var_idx: *var_idx,
                    })
                } else {
                    let parent_path = field_path[..field_path.len() - 1].to_vec();
                    Some(StackCursor::OnObjectField {
                        frame_idx: *frame_idx,
                        var_idx: *var_idx,
                        field_path: parent_path,
                    })
                }
            }
            StackCursor::OnStaticSectionHeader {
                frame_idx,
                var_idx,
                field_path,
            }
            | StackCursor::OnStaticField {
                frame_idx,
                var_idx,
                field_path,
                ..
            }
            | StackCursor::OnStaticOverflowRow {
                frame_idx,
                var_idx,
                field_path,
            } => {
                if field_path.is_empty() {
                    Some(StackCursor::OnVar {
                        frame_idx: *frame_idx,
                        var_idx: *var_idx,
                    })
                } else {
                    Some(StackCursor::OnObjectField {
                        frame_idx: *frame_idx,
                        var_idx: *var_idx,
                        field_path: field_path.clone(),
                    })
                }
            }
            StackCursor::OnStaticObjectField {
                frame_idx,
                var_idx,
                field_path,
                static_idx,
                obj_field_path,
            } => {
                if obj_field_path.len() <= 1 {
                    Some(StackCursor::OnStaticField {
                        frame_idx: *frame_idx,
                        var_idx: *var_idx,
                        field_path: field_path.clone(),
                        static_idx: *static_idx,
                    })
                } else {
                    let parent_obj_path = obj_field_path[..obj_field_path.len() - 1].to_vec();
                    Some(StackCursor::OnStaticObjectField {
                        frame_idx: *frame_idx,
                        var_idx: *var_idx,
                        field_path: field_path.clone(),
                        static_idx: *static_idx,
                        obj_field_path: parent_obj_path,
                    })
                }
            }
            StackCursor::OnChunkSection {
                frame_idx,
                var_idx,
                field_path,
                collection_id,
                ..
            }
            | StackCursor::OnCollectionEntry {
                frame_idx,
                var_idx,
                field_path,
                collection_id,
                ..
            } => {
                if let Some(restore) = self.expansion.collection_restore_cursors.get(collection_id)
                {
                    return Some(restore.clone());
                }
                if field_path.is_empty() {
                    Some(StackCursor::OnVar {
                        frame_idx: *frame_idx,
                        var_idx: *var_idx,
                    })
                } else {
                    Some(StackCursor::OnObjectField {
                        frame_idx: *frame_idx,
                        var_idx: *var_idx,
                        field_path: field_path.clone(),
                    })
                }
            }
            StackCursor::OnCollectionEntryObjField {
                frame_idx,
                var_idx,
                field_path,
                collection_id,
                entry_index,
                obj_field_path,
            } => {
                if obj_field_path.len() <= 1 {
                    Some(StackCursor::OnCollectionEntry {
                        frame_idx: *frame_idx,
                        var_idx: *var_idx,
                        field_path: field_path.clone(),
                        collection_id: *collection_id,
                        entry_index: *entry_index,
                    })
                } else {
                    let parent_obj_path = obj_field_path[..obj_field_path.len() - 1].to_vec();
                    Some(StackCursor::OnCollectionEntryObjField {
                        frame_idx: *frame_idx,
                        var_idx: *var_idx,
                        field_path: field_path.clone(),
                        collection_id: *collection_id,
                        entry_index: *entry_index,
                        obj_field_path: parent_obj_path,
                    })
                }
            }
            StackCursor::OnCollectionEntryStaticSectionHeader {
                frame_idx,
                var_idx,
                field_path,
                collection_id,
                entry_index,
                obj_field_path,
            }
            | StackCursor::OnCollectionEntryStaticField {
                frame_idx,
                var_idx,
                field_path,
                collection_id,
                entry_index,
                obj_field_path,
                ..
            }
            | StackCursor::OnCollectionEntryStaticOverflowRow {
                frame_idx,
                var_idx,
                field_path,
                collection_id,
                entry_index,
                obj_field_path,
            } => {
                if obj_field_path.is_empty() {
                    Some(StackCursor::OnCollectionEntry {
                        frame_idx: *frame_idx,
                        var_idx: *var_idx,
                        field_path: field_path.clone(),
                        collection_id: *collection_id,
                        entry_index: *entry_index,
                    })
                } else {
                    Some(StackCursor::OnCollectionEntryObjField {
                        frame_idx: *frame_idx,
                        var_idx: *var_idx,
                        field_path: field_path.clone(),
                        collection_id: *collection_id,
                        entry_index: *entry_index,
                        obj_field_path: obj_field_path.clone(),
                    })
                }
            }
            StackCursor::OnCollectionEntryStaticObjectField {
                frame_idx,
                var_idx,
                field_path,
                collection_id,
                entry_index,
                obj_field_path,
                static_idx,
                static_obj_field_path,
            } => {
                if static_obj_field_path.len() <= 1 {
                    Some(StackCursor::OnCollectionEntryStaticField {
                        frame_idx: *frame_idx,
                        var_idx: *var_idx,
                        field_path: field_path.clone(),
                        collection_id: *collection_id,
                        entry_index: *entry_index,
                        obj_field_path: obj_field_path.clone(),
                        static_idx: *static_idx,
                    })
                } else {
                    let parent_obj_path =
                        static_obj_field_path[..static_obj_field_path.len() - 1].to_vec();
                    Some(StackCursor::OnCollectionEntryStaticObjectField {
                        frame_idx: *frame_idx,
                        var_idx: *var_idx,
                        field_path: field_path.clone(),
                        collection_id: *collection_id,
                        entry_index: *entry_index,
                        obj_field_path: obj_field_path.clone(),
                        static_idx: *static_idx,
                        static_obj_field_path: parent_obj_path,
                    })
                }
            }
        }
    }

    /// If cursor is inside a collection (entry or chunk
    /// section), returns the collection object ID and the
    /// `field_path` of the parent ObjectRef field so the
    /// cursor can be restored there.
    pub fn cursor_collection_id(&self) -> Option<(u64, StackCursor)> {
        match self.nav.cursor() {
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
            }
            | StackCursor::OnCollectionEntryStaticSectionHeader {
                frame_idx,
                var_idx,
                field_path,
                collection_id,
                ..
            }
            | StackCursor::OnCollectionEntryStaticField {
                frame_idx,
                var_idx,
                field_path,
                collection_id,
                ..
            }
            | StackCursor::OnCollectionEntryStaticOverflowRow {
                frame_idx,
                var_idx,
                field_path,
                collection_id,
                ..
            }
            | StackCursor::OnCollectionEntryStaticObjectField {
                frame_idx,
                var_idx,
                field_path,
                collection_id,
                ..
            } => {
                if let Some(restore_cursor) =
                    self.expansion.collection_restore_cursors.get(collection_id)
                {
                    return Some((*collection_id, restore_cursor.clone()));
                }
                let restore_cursor = if field_path.is_empty() {
                    StackCursor::OnVar {
                        frame_idx: *frame_idx,
                        var_idx: *var_idx,
                    }
                } else {
                    StackCursor::OnObjectField {
                        frame_idx: *frame_idx,
                        var_idx: *var_idx,
                        field_path: field_path.clone(),
                    }
                };
                Some((*collection_id, restore_cursor))
            }
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
        self.expansion.object_static_fields.remove(&object_id);
    }

    /// Stores resolved static fields for an expanded object.
    pub fn set_static_fields(&mut self, object_id: u64, fields: Vec<FieldInfo>) {
        dbg_log!(
            "set_static_fields(0x{:X}): incoming_count={}",
            object_id,
            fields.len()
        );
        if fields.is_empty() {
            self.expansion.object_static_fields.remove(&object_id);
            dbg_log!("set_static_fields(0x{:X}): removed", object_id);
        } else {
            self.expansion
                .object_static_fields
                .insert(object_id, fields);
            dbg_log!("set_static_fields(0x{:X}): stored", object_id);
        }
        self.nav.sync(&self.flat_items());
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
            } = self.nav.cursor().clone()
        {
            let fallback = if field_path.is_empty() {
                StackCursor::OnVar { frame_idx, var_idx }
            } else {
                StackCursor::OnObjectField {
                    frame_idx,
                    var_idx,
                    field_path: field_path.clone(),
                }
            };
            self.nav.set_cursor_and_sync(fallback, &self.flat_items());
        } else {
            self.nav.sync(&self.flat_items());
        }
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
        if flat.contains(self.nav.cursor()) {
            return;
        }
        let mut fallback: Option<StackCursor> = None;
        // Try falling back to OnVar or OnCollectionEntry
        match self.nav.cursor() {
            StackCursor::OnObjectField {
                frame_idx, var_idx, ..
            }
            | StackCursor::OnCyclicNode {
                frame_idx, var_idx, ..
            }
            | StackCursor::OnObjectLoadingNode {
                frame_idx, var_idx, ..
            }
            | StackCursor::OnStaticSectionHeader {
                frame_idx, var_idx, ..
            }
            | StackCursor::OnStaticField {
                frame_idx, var_idx, ..
            }
            | StackCursor::OnStaticOverflowRow {
                frame_idx, var_idx, ..
            } => {
                let candidate = StackCursor::OnVar {
                    frame_idx: *frame_idx,
                    var_idx: *var_idx,
                };
                if flat.contains(&candidate) {
                    fallback = Some(candidate);
                } else {
                    fallback = Some(StackCursor::OnFrame(*frame_idx));
                }
            }
            StackCursor::OnStaticObjectField {
                frame_idx,
                var_idx,
                field_path,
                static_idx,
                ..
            } => {
                let candidate = StackCursor::OnStaticField {
                    frame_idx: *frame_idx,
                    var_idx: *var_idx,
                    field_path: field_path.clone(),
                    static_idx: *static_idx,
                };
                if flat.contains(&candidate) {
                    fallback = Some(candidate);
                } else {
                    fallback = Some(StackCursor::OnFrame(*frame_idx));
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
                let candidate = StackCursor::OnCollectionEntry {
                    frame_idx: *frame_idx,
                    var_idx: *var_idx,
                    field_path: field_path.clone(),
                    collection_id: *collection_id,
                    entry_index: *entry_index,
                };
                if flat.contains(&candidate) {
                    fallback = Some(candidate);
                } else {
                    fallback = Some(StackCursor::OnFrame(*frame_idx));
                }
            }
            StackCursor::OnCollectionEntryStaticSectionHeader {
                frame_idx,
                var_idx,
                field_path,
                collection_id,
                entry_index,
                obj_field_path,
            }
            | StackCursor::OnCollectionEntryStaticField {
                frame_idx,
                var_idx,
                field_path,
                collection_id,
                entry_index,
                obj_field_path,
                ..
            }
            | StackCursor::OnCollectionEntryStaticOverflowRow {
                frame_idx,
                var_idx,
                field_path,
                collection_id,
                entry_index,
                obj_field_path,
            } => {
                let candidate = if obj_field_path.is_empty() {
                    StackCursor::OnCollectionEntry {
                        frame_idx: *frame_idx,
                        var_idx: *var_idx,
                        field_path: field_path.clone(),
                        collection_id: *collection_id,
                        entry_index: *entry_index,
                    }
                } else {
                    StackCursor::OnCollectionEntryObjField {
                        frame_idx: *frame_idx,
                        var_idx: *var_idx,
                        field_path: field_path.clone(),
                        collection_id: *collection_id,
                        entry_index: *entry_index,
                        obj_field_path: obj_field_path.clone(),
                    }
                };
                if flat.contains(&candidate) {
                    fallback = Some(candidate);
                } else {
                    fallback = Some(StackCursor::OnFrame(*frame_idx));
                }
            }
            StackCursor::OnCollectionEntryStaticObjectField {
                frame_idx,
                var_idx,
                field_path,
                collection_id,
                entry_index,
                obj_field_path,
                static_idx,
                ..
            } => {
                let candidate = StackCursor::OnCollectionEntryStaticField {
                    frame_idx: *frame_idx,
                    var_idx: *var_idx,
                    field_path: field_path.clone(),
                    collection_id: *collection_id,
                    entry_index: *entry_index,
                    obj_field_path: obj_field_path.clone(),
                    static_idx: *static_idx,
                };
                if flat.contains(&candidate) {
                    fallback = Some(candidate);
                } else {
                    fallback = Some(StackCursor::OnFrame(*frame_idx));
                }
            }
            _ => {}
        }
        if let Some(cursor) = fallback {
            self.nav.set_cursor_and_sync(cursor, &self.flat_items());
        } else {
            self.nav.sync(&self.flat_items());
        }
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
            | StackCursor::OnStaticSectionHeader { frame_idx, .. }
            | StackCursor::OnStaticField { frame_idx, .. }
            | StackCursor::OnStaticOverflowRow { frame_idx, .. }
            | StackCursor::OnStaticObjectField { frame_idx, .. }
            | StackCursor::OnChunkSection { frame_idx, .. }
            | StackCursor::OnCollectionEntry { frame_idx, .. }
            | StackCursor::OnCollectionEntryObjField { frame_idx, .. }
            | StackCursor::OnCollectionEntryStaticSectionHeader { frame_idx, .. }
            | StackCursor::OnCollectionEntryStaticField { frame_idx, .. }
            | StackCursor::OnCollectionEntryStaticOverflowRow { frame_idx, .. }
            | StackCursor::OnCollectionEntryStaticObjectField { frame_idx, .. } =
                self.nav.cursor()
            {
                self.nav
                    .set_cursor_and_sync(StackCursor::OnFrame(*frame_idx), &self.flat_items());
            } else {
                self.nav.sync(&self.flat_items());
            }
        } else {
            self.vars.insert(frame_id, vars);
            self.expanded.insert(frame_id);
            self.nav.sync(&self.flat_items());
        }
    }

    /// Returns whether `frame_id` is currently expanded.
    pub fn is_expanded(&self, frame_id: u64) -> bool {
        self.expanded.contains(&frame_id)
    }

    fn is_non_interactive_cursor(cursor: &StackCursor) -> bool {
        matches!(
            cursor,
            StackCursor::OnStaticSectionHeader { .. }
                | StackCursor::OnStaticOverflowRow { .. }
                | StackCursor::OnCollectionEntryStaticSectionHeader { .. }
                | StackCursor::OnCollectionEntryStaticOverflowRow { .. }
        )
    }

    fn first_interactive_index(flat: &[StackCursor]) -> Option<usize> {
        flat.iter()
            .position(|c| !Self::is_non_interactive_cursor(c))
    }

    fn last_interactive_index(flat: &[StackCursor]) -> Option<usize> {
        flat.iter()
            .rposition(|c| !Self::is_non_interactive_cursor(c))
    }

    fn next_interactive_index(flat: &[StackCursor], current: usize) -> Option<usize> {
        ((current + 1)..flat.len()).find(|&idx| !Self::is_non_interactive_cursor(&flat[idx]))
    }

    fn prev_interactive_index(flat: &[StackCursor], current: usize) -> Option<usize> {
        if current == 0 {
            return None;
        }
        (0..current)
            .rev()
            .find(|&idx| !Self::is_non_interactive_cursor(&flat[idx]))
    }

    fn snap_cursor_to_interactive(&mut self, flat: &[StackCursor], prefer_down: bool) {
        let Some(current_idx) = flat.iter().position(|c| c == self.nav.cursor()) else {
            if let Some(idx) = Self::first_interactive_index(flat) {
                self.nav.set_cursor_and_sync(flat[idx].clone(), flat);
            }
            return;
        };
        if !Self::is_non_interactive_cursor(&flat[current_idx]) {
            return;
        }

        let preferred = if prefer_down {
            Self::next_interactive_index(flat, current_idx)
        } else {
            Self::prev_interactive_index(flat, current_idx)
        };
        let fallback = if prefer_down {
            Self::prev_interactive_index(flat, current_idx)
        } else {
            Self::next_interactive_index(flat, current_idx)
        };

        if let Some(idx) = preferred.or(fallback) {
            self.nav.set_cursor_and_sync(flat[idx].clone(), flat);
        }
    }

    /// Moves the cursor one step down.
    pub fn move_down(&mut self) {
        let flat = self.flat_items();
        if flat.is_empty() {
            return;
        }
        let current_idx = flat
            .iter()
            .position(|c| c == self.nav.cursor())
            .or_else(|| Self::first_interactive_index(&flat))
            .unwrap_or(0);
        let target_idx = Self::next_interactive_index(&flat, current_idx).unwrap_or(current_idx);
        self.nav
            .set_cursor_and_sync(flat[target_idx].clone(), &flat);
    }

    /// Moves the cursor one step up.
    pub fn move_up(&mut self) {
        let flat = self.flat_items();
        if flat.is_empty() {
            return;
        }
        let current_idx = flat
            .iter()
            .position(|c| c == self.nav.cursor())
            .or_else(|| Self::first_interactive_index(&flat))
            .unwrap_or(0);
        let target_idx = Self::prev_interactive_index(&flat, current_idx).unwrap_or(current_idx);
        self.nav
            .set_cursor_and_sync(flat[target_idx].clone(), &flat);
    }

    /// Moves the cursor to the first item.
    pub fn move_home(&mut self) {
        let flat = self.flat_items();
        if let Some(idx) = Self::first_interactive_index(&flat) {
            self.nav.set_cursor_and_sync(flat[idx].clone(), &flat);
        }
    }

    /// Moves the cursor to the last item.
    pub fn move_end(&mut self) {
        let flat = self.flat_items();
        if let Some(idx) = Self::last_interactive_index(&flat) {
            self.nav.set_cursor_and_sync(flat[idx].clone(), &flat);
        }
    }

    /// Sets the visible height (called during render).
    pub fn set_visible_height(&mut self, h: usize) {
        self.nav.set_visible_height(h);
    }

    /// Moves the cursor forward by `visible_height` items.
    pub fn move_page_down(&mut self) {
        let flat = self.flat_items();
        self.nav.move_page_down(&flat);
        self.snap_cursor_to_interactive(&flat, true);
    }

    /// Moves the cursor backward by `visible_height` items.
    pub fn move_page_up(&mut self) {
        let flat = self.flat_items();
        self.nav.move_page_up(&flat);
        self.snap_cursor_to_interactive(&flat, false);
    }

    /// Scrolls the visible window up by one line without moving the selection cursor.
    ///
    /// If the cursor would go off the bottom of the viewport after scrolling, the camera
    /// snaps so the cursor is at the last visible row.
    pub fn scroll_view_up(&mut self) {
        let visible_height = self.nav.visible_height();
        if visible_height == 0 {
            return;
        }
        let flat = self.flat_items();
        let item_count = flat.len();
        let cursor = self.nav.cursor().clone();
        let Some(selected_idx) = flat.iter().position(|c| c == &cursor) else {
            return;
        };
        let max_offset = item_count.saturating_sub(visible_height);
        let current_offset = self.nav.list_state().offset().min(max_offset);
        let new_offset = current_offset.saturating_sub(1);
        *self.nav.list_state_mut().offset_mut() = new_offset;
        // Snap back: cursor below viewport after scrolling up.
        // Safety: underflow impossible — snap only fires when
        //   selected_idx >= new_offset + visible_height
        //   which implies selected_idx + 1 >= visible_height + 1 > visible_height,
        //   so selected_idx + 1 - visible_height >= 1 (no usize underflow).
        if selected_idx >= new_offset + visible_height {
            *self.nav.list_state_mut().offset_mut() = selected_idx + 1 - visible_height;
        }
    }

    /// Scrolls the visible window down by one line without moving the selection cursor.
    ///
    /// If the cursor would go off the top of the viewport after scrolling, the camera
    /// snaps so the cursor is at the first visible row.
    pub fn scroll_view_down(&mut self) {
        let visible_height = self.nav.visible_height();
        if visible_height == 0 {
            return;
        }
        let flat = self.flat_items();
        let item_count = flat.len();
        if item_count == 0 {
            return;
        }
        let cursor = self.nav.cursor().clone();
        let Some(selected_idx) = flat.iter().position(|c| c == &cursor) else {
            return;
        };
        let max_offset = item_count.saturating_sub(visible_height);
        // Clamp stale offsets first, then advance by one line.
        let current_offset = self.nav.list_state().offset().min(max_offset);
        let new_offset = current_offset.saturating_add(1).min(max_offset);
        *self.nav.list_state_mut().offset_mut() = new_offset;
        // Snap back: cursor above viewport after scrolling down.
        if selected_idx < new_offset {
            *self.nav.list_state_mut().offset_mut() = selected_idx;
        }
    }

    #[cfg(test)]
    pub fn list_state_offset_for_test(&self) -> usize {
        self.nav.list_state().offset()
    }

    #[cfg(test)]
    pub fn set_list_state_offset_for_test(&mut self, offset: usize) {
        *self.nav.list_state_mut().offset_mut() = offset;
    }

    /// Returns the flattened cursor index (position in the rendered list).
    fn flat_index(&self) -> Option<usize> {
        let flat = self.flat_items();
        flat.iter().position(|c| c == self.nav.cursor())
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
                        if let VariableValue::ObjectRef {
                            id: object_id,
                            entry_count,
                            ..
                        } = &var.value
                        {
                            if entry_count.is_some()
                                && let Some(cc) = self.expansion.collection_chunks.get(object_id)
                            {
                                self.emit_collection_children(
                                    fi,
                                    vi,
                                    &[],
                                    *object_id,
                                    cc,
                                    &mut out,
                                );
                                continue;
                            }
                            let mut visited = HashSet::new();
                            self.emit_object_children(
                                fi,
                                vi,
                                *object_id,
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
                self.emit_static_rows(fi, vi, &parent_path, object_id, out);
                visited.remove(&object_id);
            }
            ExpansionPhase::Failed => {
                // Error state is styled on the parent node — no child row emitted here.
            }
        }
    }

    fn emit_static_rows(
        &self,
        fi: usize,
        vi: usize,
        parent_path: &[usize],
        object_id: u64,
        out: &mut Vec<StackCursor>,
    ) {
        let Some(static_fields) = self.expansion.object_static_fields.get(&object_id) else {
            return;
        };
        if static_fields.is_empty() {
            return;
        }

        out.push(StackCursor::OnStaticSectionHeader {
            frame_idx: fi,
            var_idx: vi,
            field_path: parent_path.to_vec(),
        });

        let shown = static_fields.len().min(STATIC_FIELDS_RENDER_LIMIT);
        for (static_idx, field) in static_fields.iter().take(shown).enumerate() {
            out.push(StackCursor::OnStaticField {
                frame_idx: fi,
                var_idx: vi,
                field_path: parent_path.to_vec(),
                static_idx,
            });

            if let FieldValue::ObjectRef {
                id,
                entry_count: Some(_),
                ..
            } = field.value
                && let Some(cc) = self.expansion.collection_chunks.get(&id)
            {
                self.emit_collection_children(fi, vi, parent_path, id, cc, out);
                continue;
            }

            if let FieldValue::ObjectRef { id, .. } = field.value {
                let mut visited = HashSet::new();
                self.emit_static_object_children(
                    fi,
                    vi,
                    parent_path,
                    static_idx,
                    id,
                    &[],
                    &mut visited,
                    out,
                );
            }
        }

        if static_fields.len() > STATIC_FIELDS_RENDER_LIMIT {
            out.push(StackCursor::OnStaticOverflowRow {
                frame_idx: fi,
                var_idx: vi,
                field_path: parent_path.to_vec(),
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_static_object_children(
        &self,
        fi: usize,
        vi: usize,
        field_path: &[usize],
        static_idx: usize,
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
                out.push(StackCursor::OnStaticObjectField {
                    frame_idx: fi,
                    var_idx: vi,
                    field_path: field_path.to_vec(),
                    static_idx,
                    obj_field_path: obj_path.to_vec(),
                });
            }
            ExpansionPhase::Failed => {
                // Error state is styled on the parent static row — no child cursor emitted here.
            }
            ExpansionPhase::Expanded => {
                let fields = self.expansion.object_fields.get(&obj_id);
                let field_count = fields.map(|f| f.len()).unwrap_or(0);
                if field_count == 0 {
                    out.push(StackCursor::OnStaticObjectField {
                        frame_idx: fi,
                        var_idx: vi,
                        field_path: field_path.to_vec(),
                        static_idx,
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
                            out.push(StackCursor::OnStaticObjectField {
                                frame_idx: fi,
                                var_idx: vi,
                                field_path: field_path.to_vec(),
                                static_idx,
                                obj_field_path: path,
                            });
                            continue;
                        }

                        out.push(StackCursor::OnStaticObjectField {
                            frame_idx: fi,
                            var_idx: vi,
                            field_path: field_path.to_vec(),
                            static_idx,
                            obj_field_path: path.clone(),
                        });

                        if let FieldValue::ObjectRef {
                            id,
                            entry_count: Some(_),
                            ..
                        } = field.value
                            && let Some(cc) = self.expansion.collection_chunks.get(&id)
                        {
                            self.emit_collection_children(fi, vi, field_path, id, cc, out);
                            continue;
                        }

                        if let FieldValue::ObjectRef { id, .. } = field.value {
                            self.emit_static_object_children(
                                fi, vi, field_path, static_idx, id, &path, visited, out,
                            );
                        }
                    }
                    visited.remove(&obj_id);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_collection_entry_static_rows(
        &self,
        fi: usize,
        vi: usize,
        field_path: &[usize],
        collection_id: u64,
        entry_index: usize,
        obj_field_path: &[usize],
        object_id: u64,
        out: &mut Vec<StackCursor>,
    ) {
        let Some(static_fields) = self.expansion.object_static_fields.get(&object_id) else {
            return;
        };
        if static_fields.is_empty() {
            return;
        }

        out.push(StackCursor::OnCollectionEntryStaticSectionHeader {
            frame_idx: fi,
            var_idx: vi,
            field_path: field_path.to_vec(),
            collection_id,
            entry_index,
            obj_field_path: obj_field_path.to_vec(),
        });

        let shown = static_fields.len().min(STATIC_FIELDS_RENDER_LIMIT);
        for (static_idx, field) in static_fields.iter().take(shown).enumerate() {
            out.push(StackCursor::OnCollectionEntryStaticField {
                frame_idx: fi,
                var_idx: vi,
                field_path: field_path.to_vec(),
                collection_id,
                entry_index,
                obj_field_path: obj_field_path.to_vec(),
                static_idx,
            });

            if let FieldValue::ObjectRef {
                id,
                entry_count: Some(_),
                ..
            } = field.value
                && let Some(cc) = self.expansion.collection_chunks.get(&id)
            {
                self.emit_collection_children(fi, vi, field_path, id, cc, out);
                continue;
            }

            if let FieldValue::ObjectRef { id, .. } = field.value {
                let mut visited = HashSet::new();
                self.emit_collection_entry_static_object_children(
                    fi,
                    vi,
                    field_path,
                    collection_id,
                    entry_index,
                    obj_field_path,
                    static_idx,
                    id,
                    &[],
                    &mut visited,
                    out,
                );
            }
        }

        if static_fields.len() > STATIC_FIELDS_RENDER_LIMIT {
            out.push(StackCursor::OnCollectionEntryStaticOverflowRow {
                frame_idx: fi,
                var_idx: vi,
                field_path: field_path.to_vec(),
                collection_id,
                entry_index,
                obj_field_path: obj_field_path.to_vec(),
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_collection_entry_static_object_children(
        &self,
        fi: usize,
        vi: usize,
        field_path: &[usize],
        collection_id: u64,
        entry_index: usize,
        obj_field_path: &[usize],
        static_idx: usize,
        obj_id: u64,
        static_obj_path: &[usize],
        visited: &mut HashSet<u64>,
        out: &mut Vec<StackCursor>,
    ) {
        if static_obj_path.len() >= 16 {
            return;
        }

        match self.expansion_state(obj_id) {
            ExpansionPhase::Collapsed => {}
            ExpansionPhase::Loading => {
                out.push(StackCursor::OnCollectionEntryStaticObjectField {
                    frame_idx: fi,
                    var_idx: vi,
                    field_path: field_path.to_vec(),
                    collection_id,
                    entry_index,
                    obj_field_path: obj_field_path.to_vec(),
                    static_idx,
                    static_obj_field_path: static_obj_path.to_vec(),
                });
            }
            ExpansionPhase::Failed => {
                // Error state is styled on the parent static row — no child cursor emitted here.
            }
            ExpansionPhase::Expanded => {
                let fields = self.expansion.object_fields.get(&obj_id);
                let field_count = fields.map(|f| f.len()).unwrap_or(0);
                if field_count == 0 {
                    out.push(StackCursor::OnCollectionEntryStaticObjectField {
                        frame_idx: fi,
                        var_idx: vi,
                        field_path: field_path.to_vec(),
                        collection_id,
                        entry_index,
                        obj_field_path: obj_field_path.to_vec(),
                        static_idx,
                        static_obj_field_path: static_obj_path.to_vec(),
                    });
                } else {
                    visited.insert(obj_id);
                    let field_list = fields.unwrap();
                    for (idx, field) in field_list.iter().enumerate() {
                        let mut path = static_obj_path.to_vec();
                        path.push(idx);

                        if let FieldValue::ObjectRef { id, .. } = field.value
                            && visited.contains(&id)
                        {
                            out.push(StackCursor::OnCollectionEntryStaticObjectField {
                                frame_idx: fi,
                                var_idx: vi,
                                field_path: field_path.to_vec(),
                                collection_id,
                                entry_index,
                                obj_field_path: obj_field_path.to_vec(),
                                static_idx,
                                static_obj_field_path: path,
                            });
                            continue;
                        }

                        out.push(StackCursor::OnCollectionEntryStaticObjectField {
                            frame_idx: fi,
                            var_idx: vi,
                            field_path: field_path.to_vec(),
                            collection_id,
                            entry_index,
                            obj_field_path: obj_field_path.to_vec(),
                            static_idx,
                            static_obj_field_path: path.clone(),
                        });

                        if let FieldValue::ObjectRef {
                            id,
                            entry_count: Some(_),
                            ..
                        } = field.value
                            && let Some(cc) = self.expansion.collection_chunks.get(&id)
                        {
                            self.emit_collection_children(fi, vi, field_path, id, cc, out);
                            continue;
                        }

                        if let FieldValue::ObjectRef { id, .. } = field.value {
                            self.emit_collection_entry_static_object_children(
                                fi,
                                vi,
                                field_path,
                                collection_id,
                                entry_index,
                                obj_field_path,
                                static_idx,
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
        let mut visited_collections = HashSet::new();
        self.emit_collection_children_inner(
            fi,
            vi,
            field_path,
            collection_id,
            cc,
            out,
            &mut visited_collections,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_collection_children_inner(
        &self,
        fi: usize,
        vi: usize,
        field_path: &[usize],
        collection_id: u64,
        cc: &CollectionChunks,
        out: &mut Vec<StackCursor>,
        visited_collections: &mut HashSet<u64>,
    ) {
        if !visited_collections.insert(collection_id) {
            return;
        }

        if let Some(page) = &cc.eager_page {
            for entry in &page.entries {
                self.emit_collection_entry_cursor(
                    fi,
                    vi,
                    field_path,
                    collection_id,
                    entry,
                    out,
                    visited_collections,
                );
            }
        }

        let ranges = compute_chunk_ranges(cc.total_count);
        for (offset, _) in &ranges {
            out.push(StackCursor::OnChunkSection {
                frame_idx: fi,
                var_idx: vi,
                field_path: field_path.to_vec(),
                collection_id,
                chunk_offset: *offset,
            });
            if let Some(ChunkState::Loaded(page)) = cc.chunk_pages.get(offset) {
                for entry in &page.entries {
                    self.emit_collection_entry_cursor(
                        fi,
                        vi,
                        field_path,
                        collection_id,
                        entry,
                        out,
                        visited_collections,
                    );
                }
            }
        }

        visited_collections.remove(&collection_id);
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_collection_entry_cursor(
        &self,
        fi: usize,
        vi: usize,
        field_path: &[usize],
        collection_id: u64,
        entry: &hprof_engine::EntryInfo,
        out: &mut Vec<StackCursor>,
        visited_collections: &mut HashSet<u64>,
    ) {
        out.push(StackCursor::OnCollectionEntry {
            frame_idx: fi,
            var_idx: vi,
            field_path: field_path.to_vec(),
            collection_id,
            entry_index: entry.index,
        });

        if let FieldValue::ObjectRef {
            id,
            entry_count: Some(_),
            ..
        } = &entry.value
            && *id != collection_id
            && let Some(nested) = self.expansion.collection_chunks.get(id)
        {
            self.emit_collection_children_inner(
                fi,
                vi,
                field_path,
                *id,
                nested,
                out,
                visited_collections,
            );
            return;
        }

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
                            // Cyclic — emit terminal leaf row.
                            out.push(StackCursor::OnCollectionEntryObjField {
                                frame_idx: fi,
                                var_idx: vi,
                                field_path: field_path.to_vec(),
                                collection_id,
                                entry_index,
                                obj_field_path: path,
                            });
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
                        if let FieldValue::ObjectRef {
                            id,
                            entry_count: Some(_),
                            ..
                        } = field.value
                            && id != collection_id
                            && let Some(cc) = self.expansion.collection_chunks.get(&id)
                        {
                            self.emit_collection_children(fi, vi, field_path, id, cc, out);
                            continue;
                        }
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
                self.emit_collection_entry_static_rows(
                    fi,
                    vi,
                    field_path,
                    collection_id,
                    entry_index,
                    obj_path,
                    obj_id,
                    out,
                );
            }
        }
    }

    // === Rendering ===
    /// Builds the list items for rendering.
    ///
    /// Frame headers are plain items; variable-tree rows are produced by
    /// [`render_variable_tree`] (no per-item cursor styling — selection is
    /// applied by ratatui's `List` via [`Self::list_state_mut`]).
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
                    &self.expansion.object_static_fields,
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
