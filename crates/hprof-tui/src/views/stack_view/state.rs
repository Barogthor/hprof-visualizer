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
    ChunkOffset, ChunkState, CollectionChunks, CollectionId, EntryIdx, ExpansionPhase, FieldIdx,
    FrameId, NavigationPath, NavigationPathBuilder, PathSegment, RenderCursor,
    STATIC_FIELDS_RENDER_LIMIT, StaticFieldIdx, VarIdx,
};

/// State for the stack frame panel.
pub struct StackState {
    // === Frames & Vars ===
    pub(super) frames: Vec<FrameInfo>,
    /// Vars per frame_id — populated on demand by `App` calling the engine.
    pub(super) vars: HashMap<u64, Vec<VariableInfo>>,
    pub(super) expanded: HashSet<u64>,
    // === Cursor & Navigation ===
    pub(super) nav: CursorState<RenderCursor>,
    // === Expansion (delegated) ===
    pub(crate) expansion: ExpansionRegistry,
}

impl StackState {
    /// Creates a new state for the given frames. Selects first frame.
    pub fn new(frames: Vec<FrameInfo>) -> Self {
        let cursor = if frames.is_empty() {
            RenderCursor::NoFrames
        } else {
            RenderCursor::At(NavigationPathBuilder::frame_only(FrameId(
                frames[0].frame_id,
            )))
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
        let path = self.cursor_path()?;
        let seg = path.segments().first()?;
        if let PathSegment::Frame(fid) = seg {
            Some(fid.0)
        } else {
            None
        }
    }

    /// Returns the current cursor.
    pub fn cursor(&self) -> &RenderCursor {
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

    /// Returns the decoded static fields map keyed by object id.
    pub(crate) fn object_static_fields(&self) -> &HashMap<u64, Vec<FieldInfo>> {
        &self.expansion.object_static_fields
    }

    /// Returns the collection chunks map.
    pub(crate) fn collection_chunks_map(&self) -> &HashMap<u64, CollectionChunks> {
        &self.expansion.collection_chunks
    }

    /// Sets the cursor to `new_cursor` and syncs the ratatui list state.
    pub fn set_cursor(&mut self, new_cursor: RenderCursor) {
        self.nav.set_cursor_and_sync(new_cursor, &self.flat_items());
    }

    // === Path helpers ===

    /// Extracts the `NavigationPath` from the current cursor, if any.
    fn cursor_path(&self) -> Option<&NavigationPath> {
        match self.nav.cursor() {
            RenderCursor::NoFrames => None,
            RenderCursor::At(p)
            | RenderCursor::LoadingNode(p)
            | RenderCursor::FailedNode(p)
            | RenderCursor::CyclicNode(p)
            | RenderCursor::SectionHeader(p)
            | RenderCursor::OverflowRow(p) => Some(p),
            RenderCursor::ChunkSection(p, _) => Some(p),
        }
    }

    /// Resolves the root object_id for a Var segment.
    fn var_object_id(&self, frame_id: u64, var_idx: usize) -> Option<u64> {
        let vars = self.vars.get(&frame_id)?;
        let var = vars.get(var_idx)?;
        if let VariableValue::ObjectRef { id, .. } = var.value {
            Some(id)
        } else {
            None
        }
    }

    /// Resolves the `object_id` at the end of `field_segs` starting from `root_id`.
    ///
    /// Walks field indices through `object_fields`.
    fn resolve_field_chain(&self, root_id: u64, field_segs: &[PathSegment]) -> Option<u64> {
        let mut current = root_id;
        for seg in field_segs {
            match seg {
                PathSegment::Field(fi) => {
                    let fields = self.expansion.object_fields.get(&current)?;
                    let field = fields.get(fi.0)?;
                    if let FieldValue::ObjectRef { id, .. } = field.value {
                        current = id;
                    } else {
                        return None;
                    }
                }
                PathSegment::CollectionEntry(cid, ei) => {
                    let cc = self.expansion.collection_chunks.get(&cid.0)?;
                    let entry = cc.find_entry(ei.0)?;
                    if let FieldValue::ObjectRef { id, .. } = &entry.value {
                        current = *id;
                    } else {
                        return None;
                    }
                }
                _ => return None,
            }
        }
        Some(current)
    }

    /// Given an `At(path)` cursor, resolves the object context:
    /// `(frame_id, root_object_id, segments_after_var)` where segments_after_var
    /// are all segments after the first two (Frame, Var).
    fn resolve_at_path_context<'a>(
        &self,
        path: &'a NavigationPath,
    ) -> Option<(u64, u64, &'a [PathSegment])> {
        let segs = path.segments();
        let frame_id = match segs.first()? {
            PathSegment::Frame(fid) => fid.0,
            _ => return None,
        };
        if segs.len() < 2 {
            return None;
        }
        let var_idx = match &segs[1] {
            PathSegment::Var(vi) => vi.0,
            _ => return None,
        };
        let root_id = self.var_object_id(frame_id, var_idx)?;
        Some((frame_id, root_id, &segs[2..]))
    }

    // === selected_* methods (derive from RenderCursor path) ===

    /// Returns the `object_id` if the cursor is on an `ObjectRef` var.
    pub fn selected_object_id(&self) -> Option<u64> {
        let RenderCursor::At(path) = self.nav.cursor() else {
            return None;
        };
        let segs = path.segments();
        if segs.len() != 2 {
            return None;
        }
        let (frame_id, root_id, _) = self.resolve_at_path_context(path)?;
        let _ = frame_id;
        Some(root_id)
    }

    /// Returns `Some(entry_count)` if the currently selected variable is a collection.
    pub fn selected_var_entry_count(&self) -> Option<u64> {
        let RenderCursor::At(path) = self.nav.cursor() else {
            return None;
        };
        let segs = path.segments();
        if segs.len() != 2 {
            return None;
        }
        let frame_id = match segs[0] {
            PathSegment::Frame(fid) => fid.0,
            _ => return None,
        };
        let var_idx = match segs[1] {
            PathSegment::Var(vi) => vi.0,
            _ => return None,
        };
        let vars = self.vars.get(&frame_id)?;
        let var = vars.get(var_idx)?;
        if let VariableValue::ObjectRef { entry_count, .. } = &var.value {
            *entry_count
        } else {
            None
        }
    }

    /// Returns the object_id if the cursor is on a loading/failed/empty pseudo-node.
    pub fn selected_loading_object_id(&self) -> Option<u64> {
        let path = match self.nav.cursor() {
            RenderCursor::LoadingNode(p) => p,
            RenderCursor::FailedNode(p) => p,
            _ => return None,
        };
        self.object_id_from_last_segment(path)
    }

    /// Resolves the object_id associated with the deepest object-reference segment.
    fn object_id_from_last_segment(&self, path: &NavigationPath) -> Option<u64> {
        let segs = path.segments();
        let frame_id = match segs.first()? {
            PathSegment::Frame(fid) => fid.0,
            _ => return None,
        };
        if segs.len() < 2 {
            return None;
        }
        let var_idx = match &segs[1] {
            PathSegment::Var(vi) => vi.0,
            _ => return None,
        };
        let root_id = self.var_object_id(frame_id, var_idx)?;
        if segs.len() == 2 {
            return Some(root_id);
        }
        // Walk up to the parent of the last segment to find the owning object.
        let parent_segs = &segs[2..segs.len().saturating_sub(1)];
        self.resolve_field_chain(root_id, parent_segs)
    }

    /// Returns `(object_id, entry_count)` if cursor is on an `ObjectRef` field
    /// pointing to a collection.
    pub fn selected_field_collection_info(&self) -> Option<(u64, u64)> {
        let RenderCursor::At(path) = self.nav.cursor() else {
            return None;
        };
        let segs = path.segments();
        let last = segs.last()?;
        let PathSegment::Field(fi) = last else {
            return None;
        };
        let (_, root_id, depth_segs) = self.resolve_at_path_context(path)?;
        let parent_segs = &depth_segs[..depth_segs.len().saturating_sub(1)];
        let parent_id = self
            .resolve_field_chain(root_id, parent_segs)
            .unwrap_or(root_id);
        let fields = self.expansion.object_fields.get(&parent_id)?;
        let field = fields.get(fi.0)?;
        if let FieldValue::ObjectRef {
            id,
            entry_count: Some(ec),
            ..
        } = field.value
            && ec > 0
        {
            Some((id, ec))
        } else {
            None
        }
    }

    /// Returns the `ObjectRef` id for the field under cursor if it is an `ObjectRef`.
    pub fn selected_field_ref_id(&self) -> Option<u64> {
        let RenderCursor::At(path) = self.nav.cursor() else {
            return None;
        };
        let segs = path.segments();
        let last = segs.last()?;
        let PathSegment::Field(fi) = last else {
            return None;
        };
        if self.is_cyclic_at_path(path) {
            return None;
        }
        let (_, root_id, depth_segs) = self.resolve_at_path_context(path)?;
        let parent_segs = &depth_segs[..depth_segs.len().saturating_sub(1)];
        let parent_id = self
            .resolve_field_chain(root_id, parent_segs)
            .unwrap_or(root_id);
        let fields = self.expansion.object_fields.get(&parent_id)?;
        let field = fields.get(fi.0)?;
        if let FieldValue::ObjectRef {
            id,
            entry_count: None,
            ..
        } = field.value
        {
            Some(id)
        } else if let FieldValue::ObjectRef {
            id,
            entry_count: Some(_),
            ..
        } = field.value
        {
            Some(id)
        } else {
            None
        }
    }

    /// Checks if a path ends in a cyclic reference.
    fn is_cyclic_at_path(&self, path: &NavigationPath) -> bool {
        matches!(self.nav.cursor(), RenderCursor::CyclicNode(p) if p == path)
    }

    /// Returns `(object_id, entry_count)` when on a static field pointing to collection.
    pub fn selected_static_field_collection_info(&self) -> Option<(u64, u64)> {
        let RenderCursor::At(path) = self.nav.cursor() else {
            return None;
        };
        self.static_field_value_info(path, |f| {
            if let FieldValue::ObjectRef {
                id,
                entry_count: Some(ec),
                ..
            } = f.value
                && ec > 0
            {
                Some((id, ec))
            } else {
                None
            }
        })
    }

    /// Returns the `ObjectRef` id when cursor is on a static field pointing to object.
    pub fn selected_static_field_ref_id(&self) -> Option<u64> {
        let RenderCursor::At(path) = self.nav.cursor() else {
            return None;
        };
        self.static_field_value_info(path, |f| {
            if let FieldValue::ObjectRef { id, .. } = f.value {
                Some(id)
            } else {
                None
            }
        })
    }

    /// Resolves a static field value for an `At(path)` where the last seg is `StaticField`.
    fn static_field_value_info<T>(
        &self,
        path: &NavigationPath,
        extract: impl Fn(&FieldInfo) -> Option<T>,
    ) -> Option<T> {
        let segs = path.segments();
        let last = segs.last()?;
        let PathSegment::StaticField(si) = last else {
            return None;
        };
        // Owner is the object at the parent path.
        let (_, root_id, depth_segs) = self.resolve_at_path_context(path)?;
        let parent_depth_segs = &depth_segs[..depth_segs.len().saturating_sub(1)];
        let owner_id = self
            .resolve_field_chain(root_id, parent_depth_segs)
            .unwrap_or(root_id);
        let static_fields = self.expansion.object_static_fields.get(&owner_id)?;
        let field = static_fields.get(si.0)?;
        extract(field)
    }

    /// Returns `(object_id, entry_count)` when on a static object sub-field.
    pub fn selected_static_obj_field_collection_info(&self) -> Option<(u64, u64)> {
        let RenderCursor::At(path) = self.nav.cursor() else {
            return None;
        };
        self.static_obj_field_info(path, |f| {
            if let FieldValue::ObjectRef {
                id,
                entry_count: Some(ec),
                ..
            } = f.value
                && id != 0
                && ec > 0
            {
                Some((id, ec))
            } else {
                None
            }
        })
    }

    /// Returns the `ObjectRef` id when on a static object sub-field.
    pub fn selected_static_obj_field_ref_id(&self) -> Option<u64> {
        let RenderCursor::At(path) = self.nav.cursor() else {
            return None;
        };
        self.static_obj_field_info(path, |f| {
            if let FieldValue::ObjectRef { id, .. } = f.value {
                Some(id)
            } else {
                None
            }
        })
    }

    /// Resolves the field info for `At(path)` in the static object subtree.
    ///
    /// Path layout: `[Frame, Var, ...fields..., StaticField(si), ...obj_fields...]`
    fn static_obj_field_info<T>(
        &self,
        path: &NavigationPath,
        extract: impl Fn(&FieldInfo) -> Option<T>,
    ) -> Option<T> {
        let segs = path.segments();
        // Find the StaticField segment and the last Field segment after it.
        let static_pos = segs
            .iter()
            .rposition(|s| matches!(s, PathSegment::StaticField(_)))?;
        let PathSegment::StaticField(si) = &segs[static_pos] else {
            return None;
        };

        // Object that owns the static field is at segs[..static_pos].
        let (_, root_id, depth_segs) = self.resolve_at_path_context(path)?;
        let pre_static = if static_pos >= 2 {
            &depth_segs[..static_pos - 2]
        } else {
            &[][..]
        };
        let static_owner_id = self
            .resolve_field_chain(root_id, pre_static)
            .unwrap_or(root_id);
        let static_fields = self.expansion.object_static_fields.get(&static_owner_id)?;
        let static_field = static_fields.get(si.0)?;
        let FieldValue::ObjectRef {
            id: static_root_id, ..
        } = static_field.value
        else {
            return None;
        };

        // Remaining path after the static field index.
        let obj_segs = &segs[static_pos + 1..];
        if obj_segs.is_empty() {
            return None;
        }
        let parent_segs = &obj_segs[..obj_segs.len().saturating_sub(1)];
        let parent_id = self
            .resolve_field_chain(static_root_id, parent_segs)
            .unwrap_or(static_root_id);
        let last = obj_segs.last()?;
        let PathSegment::Field(fi) = last else {
            return None;
        };
        let fields = self.expansion.object_fields.get(&parent_id)?;
        let field = fields.get(fi.0)?;
        extract(field)
    }

    /// Returns `Some(entry_count)` if cursor is on a collection entry that is
    /// itself a collection.
    pub fn selected_collection_entry_count(&self) -> Option<u64> {
        let RenderCursor::At(path) = self.nav.cursor() else {
            return None;
        };
        let last = path.segments().last()?;
        let PathSegment::CollectionEntry(cid, ei) = last else {
            return None;
        };
        let cc = self.expansion.collection_chunks.get(&cid.0)?;
        let entry = cc.find_entry(ei.0)?;
        if let FieldValue::ObjectRef { entry_count, .. } = &entry.value {
            *entry_count
        } else {
            None
        }
    }

    /// Returns `(collection_id, chunk_offset, chunk_limit)` if cursor is on a chunk section.
    pub fn selected_chunk_info(&self) -> Option<(u64, usize, usize)> {
        let RenderCursor::ChunkSection(path, offset) = self.nav.cursor() else {
            return None;
        };
        let last = path.segments().last()?;
        let PathSegment::CollectionEntry(cid, _) = last else {
            return None;
        };
        let cc = self.expansion.collection_chunks.get(&cid.0)?;
        let ranges = compute_chunk_ranges(cc.total_count);
        let limit = ranges
            .iter()
            .find(|(o, _)| *o == offset.0)
            .map(|(_, l)| *l)?;
        Some((cid.0, offset.0, limit))
    }

    /// Returns the `ObjectRef` id when cursor is on a `CollectionEntry` with ObjectRef.
    pub fn selected_collection_entry_ref_id(&self) -> Option<u64> {
        let RenderCursor::At(path) = self.nav.cursor() else {
            return None;
        };
        let last = path.segments().last()?;
        let PathSegment::CollectionEntry(cid, ei) = last else {
            return None;
        };
        let cc = self.expansion.collection_chunks.get(&cid.0)?;
        let entry = cc.find_entry(ei.0)?;
        if let FieldValue::ObjectRef {
            id,
            entry_count: None,
            ..
        } = &entry.value
        {
            Some(*id)
        } else if let FieldValue::ObjectRef { id, .. } = &entry.value {
            Some(*id)
        } else {
            None
        }
    }

    /// Returns `(object_id, entry_count)` when on a collection-entry object field
    /// pointing to a collection.
    pub fn selected_collection_entry_obj_field_collection_info(&self) -> Option<(u64, u64)> {
        let RenderCursor::At(path) = self.nav.cursor() else {
            return None;
        };
        self.coll_entry_obj_field_info(path, |f| {
            if let FieldValue::ObjectRef {
                id,
                entry_count: Some(ec),
                ..
            } = f.value
                && id != 0
                && ec > 0
            {
                Some((id, ec))
            } else {
                None
            }
        })
    }

    /// Returns the `ObjectRef` id when on a collection-entry object field.
    pub fn selected_collection_entry_obj_field_ref_id(&self) -> Option<u64> {
        let RenderCursor::At(path) = self.nav.cursor() else {
            return None;
        };
        self.coll_entry_obj_field_info(path, |f| {
            if let FieldValue::ObjectRef { id, .. } = f.value {
                // Detect cycle: field points to an ancestor object in this path.
                if self.coll_entry_path_contains_object(path, id) {
                    None
                } else {
                    Some(id)
                }
            } else {
                None
            }
        })
    }

    /// Returns true if `object_id` appears in any `CollectionEntry` or root Var
    /// ancestor of `path`, indicating a cycle.
    fn coll_entry_path_contains_object(&self, path: &NavigationPath, object_id: u64) -> bool {
        let segs = path.segments();
        // Check each CollectionEntry's entry object in the path.
        for seg in segs.iter() {
            if let PathSegment::CollectionEntry(cid, ei) = seg
                && let Some(cc) = self.expansion.collection_chunks.get(&cid.0)
                && let Some(entry) = cc.find_entry(ei.0)
                && let FieldValue::ObjectRef { id: eid, .. } = entry.value
                && eid == object_id
            {
                return true;
            }
        }
        false
    }

    /// Resolves field info for `At(path)` where the path goes through a
    /// `CollectionEntry` and then `Field` segments.
    ///
    /// Path layout: `[Frame, Var, ...fields..., CollectionEntry(cid, ei), ...obj_fields...]`
    fn coll_entry_obj_field_info<T>(
        &self,
        path: &NavigationPath,
        extract: impl Fn(&FieldInfo) -> Option<T>,
    ) -> Option<T> {
        let segs = path.segments();
        let entry_pos = segs
            .iter()
            .rposition(|s| matches!(s, PathSegment::CollectionEntry(_, _)))?;
        let PathSegment::CollectionEntry(cid, ei) = &segs[entry_pos] else {
            return None;
        };
        let cc = self.expansion.collection_chunks.get(&cid.0)?;
        let entry = cc.find_entry(ei.0)?;
        let FieldValue::ObjectRef { id: entry_root, .. } = &entry.value else {
            return None;
        };

        let obj_segs = &segs[entry_pos + 1..];
        if obj_segs.is_empty() {
            return None;
        }
        let parent_segs = &obj_segs[..obj_segs.len().saturating_sub(1)];
        let parent_id = self
            .resolve_field_chain(*entry_root, parent_segs)
            .unwrap_or(*entry_root);
        let last = obj_segs.last()?;
        let PathSegment::Field(fi) = last else {
            return None;
        };
        let fields = self.expansion.object_fields.get(&parent_id)?;
        let field = fields.get(fi.0)?;
        extract(field)
    }

    /// Returns `(object_id, entry_count)` when on a collection-entry static field.
    pub fn selected_collection_entry_static_field_collection_info(&self) -> Option<(u64, u64)> {
        let RenderCursor::At(path) = self.nav.cursor() else {
            return None;
        };
        self.coll_entry_static_field_info(path, |f| {
            if let FieldValue::ObjectRef {
                id,
                entry_count: Some(ec),
                ..
            } = f.value
                && id != 0
                && ec > 0
            {
                Some((id, ec))
            } else {
                None
            }
        })
    }

    /// Returns the `ObjectRef` id when on a collection-entry static field.
    pub fn selected_collection_entry_static_field_ref_id(&self) -> Option<u64> {
        let RenderCursor::At(path) = self.nav.cursor() else {
            return None;
        };
        self.coll_entry_static_field_info(path, |f| {
            if let FieldValue::ObjectRef { id, .. } = f.value {
                Some(id)
            } else {
                None
            }
        })
    }

    /// Resolves static field info for a path that goes through CollectionEntry + StaticField.
    fn coll_entry_static_field_info<T>(
        &self,
        path: &NavigationPath,
        extract: impl Fn(&FieldInfo) -> Option<T>,
    ) -> Option<T> {
        let segs = path.segments();
        let static_pos = segs
            .iter()
            .rposition(|s| matches!(s, PathSegment::StaticField(_)))?;
        let PathSegment::StaticField(si) = &segs[static_pos] else {
            return None;
        };

        // Find the CollectionEntry that precedes the StaticField.
        let entry_pos = segs[..static_pos]
            .iter()
            .rposition(|s| matches!(s, PathSegment::CollectionEntry(_, _)))?;
        let PathSegment::CollectionEntry(cid, ei) = &segs[entry_pos] else {
            return None;
        };
        let cc = self.expansion.collection_chunks.get(&cid.0)?;
        let entry = cc.find_entry(ei.0)?;
        let FieldValue::ObjectRef { id: entry_root, .. } = &entry.value else {
            return None;
        };

        // Walk from entry_root through obj_segs (Field segs between entry and static).
        let obj_segs = &segs[entry_pos + 1..static_pos];
        let owner_id = self
            .resolve_field_chain(*entry_root, obj_segs)
            .unwrap_or(*entry_root);
        let static_fields = self.expansion.object_static_fields.get(&owner_id)?;
        let field = static_fields.get(si.0)?;
        extract(field)
    }

    /// Returns `(object_id, entry_count)` when on a collection-entry static object sub-field.
    pub fn selected_collection_entry_static_obj_field_collection_info(&self) -> Option<(u64, u64)> {
        let RenderCursor::At(path) = self.nav.cursor() else {
            return None;
        };
        self.coll_entry_static_obj_field_info(path, |f| {
            if let FieldValue::ObjectRef {
                id,
                entry_count: Some(ec),
                ..
            } = f.value
                && id != 0
                && ec > 0
            {
                Some((id, ec))
            } else {
                None
            }
        })
    }

    /// Returns the `ObjectRef` id when on a collection-entry static object sub-field.
    pub fn selected_collection_entry_static_obj_field_ref_id(&self) -> Option<u64> {
        let RenderCursor::At(path) = self.nav.cursor() else {
            return None;
        };
        self.coll_entry_static_obj_field_info(path, |f| {
            if let FieldValue::ObjectRef { id, .. } = f.value {
                Some(id)
            } else {
                None
            }
        })
    }

    /// Resolves field info for a path through CollectionEntry + StaticField + Field.
    fn coll_entry_static_obj_field_info<T>(
        &self,
        path: &NavigationPath,
        extract: impl Fn(&FieldInfo) -> Option<T>,
    ) -> Option<T> {
        let segs = path.segments();
        let static_pos = segs
            .iter()
            .rposition(|s| matches!(s, PathSegment::StaticField(_)))?;
        let PathSegment::StaticField(si) = &segs[static_pos] else {
            return None;
        };

        // Find CollectionEntry before StaticField.
        let entry_pos = segs[..static_pos]
            .iter()
            .rposition(|s| matches!(s, PathSegment::CollectionEntry(_, _)))?;
        let PathSegment::CollectionEntry(cid, ei) = &segs[entry_pos] else {
            return None;
        };
        let cc = self.expansion.collection_chunks.get(&cid.0)?;
        let entry = cc.find_entry(ei.0)?;
        let FieldValue::ObjectRef { id: entry_root, .. } = &entry.value else {
            return None;
        };

        let obj_segs = &segs[entry_pos + 1..static_pos];
        let owner_id = self
            .resolve_field_chain(*entry_root, obj_segs)
            .unwrap_or(*entry_root);
        let static_fields = self.expansion.object_static_fields.get(&owner_id)?;
        let static_field = static_fields.get(si.0)?;
        let FieldValue::ObjectRef {
            id: static_root, ..
        } = static_field.value
        else {
            return None;
        };

        let static_obj_segs = &segs[static_pos + 1..];
        if static_obj_segs.is_empty() {
            return None;
        }
        let parent_segs = &static_obj_segs[..static_obj_segs.len().saturating_sub(1)];
        let parent_id = self
            .resolve_field_chain(static_root, parent_segs)
            .unwrap_or(static_root);
        let last = static_obj_segs.last()?;
        let PathSegment::Field(fi) = last else {
            return None;
        };
        let fields = self.expansion.object_fields.get(&parent_id)?;
        let field = fields.get(fi.0)?;
        extract(field)
    }

    /// Returns the [`ChunkState`] for a specific chunk.
    pub fn chunk_state(&self, collection_id: u64, chunk_offset: usize) -> Option<&ChunkState> {
        self.expansion.chunk_state(collection_id, chunk_offset)
    }

    /// Returns the logical parent cursor for the current position, or `None`
    /// if at the top level (Frame-only or NoFrames).
    pub fn parent_cursor(&self) -> Option<RenderCursor> {
        let path = self.cursor_path()?;
        let parent = path.parent()?;
        Some(RenderCursor::At(parent))
    }

    /// If cursor is inside a collection, returns `(collection_id, restore_cursor)`
    /// so the caller can collapse the collection and restore cursor position.
    pub fn cursor_collection_id(&self) -> Option<(u64, RenderCursor)> {
        let path = self.cursor_path()?;
        // Find innermost CollectionEntry in path (last one = deepest nesting level).
        let coll_seg_pos = path
            .segments()
            .iter()
            .rposition(|s| matches!(s, PathSegment::CollectionEntry(_, _)))?;
        let PathSegment::CollectionEntry(cid, _) = &path.segments()[coll_seg_pos] else {
            return None;
        };
        // Restore cursor is the path up to (not including) the CollectionEntry.
        let restore_path_segs = path.segments()[..coll_seg_pos].to_vec();
        let restore_cursor = if restore_path_segs.len() == 1 {
            // At frame level
            RenderCursor::At(NavigationPathBuilder::frame_only(
                if let PathSegment::Frame(fid) = &restore_path_segs[0] {
                    *fid
                } else {
                    return None;
                },
            ))
        } else {
            RenderCursor::At(NavigationPath::from_segments(restore_path_segs))
        };
        Some((cid.0, restore_cursor))
    }

    /// Returns the expansion phase for `path` (defaults to `Collapsed`).
    pub fn expansion_state_for_path(&self, path: &NavigationPath) -> ExpansionPhase {
        self.expansion.expansion_state(path)
    }

    /// Returns the expansion phase for an `object_id` (for app compatibility).
    pub fn expansion_state(&self, object_id: u64) -> ExpansionPhase {
        self.expansion
            .object_phases
            .get(&object_id)
            .cloned()
            .unwrap_or(ExpansionPhase::Collapsed)
    }

    /// Marks an object as loading (called by App on expansion start).
    pub fn set_expansion_loading(&mut self, object_id: u64) {
        self.expansion
            .object_phases
            .insert(object_id, ExpansionPhase::Loading);
    }

    /// Marks a path+object expansion as complete.
    pub fn set_expansion_done_at_path(
        &mut self,
        path: &NavigationPath,
        object_id: u64,
        fields: Vec<FieldInfo>,
    ) {
        self.expansion.set_expansion_done(path, object_id, fields);
        self.expansion.object_static_fields.remove(&object_id);
    }

    /// Marks an object expansion as complete with decoded fields.
    pub fn set_expansion_done(&mut self, object_id: u64, fields: Vec<FieldInfo>) {
        self.expansion.object_fields.insert(object_id, fields);
        self.expansion
            .object_phases
            .insert(object_id, ExpansionPhase::Expanded);
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
    pub fn set_expansion_failed(&mut self, object_id: u64, error: String) {
        self.expansion.object_errors.insert(object_id, error);
        self.expansion.object_static_fields.remove(&object_id);
        self.expansion
            .object_phases
            .insert(object_id, ExpansionPhase::Failed);
        // The cursor may be on a LoadingNode — recover it.
        let flat = self.flat_items();
        if !flat.iter().any(|c| c == self.nav.cursor()) {
            // Try At(same_path) first, then parent.
            if let Some(path) = self.cursor_path().cloned() {
                let same = RenderCursor::At(path);
                if flat.contains(&same) {
                    self.nav.set_cursor_and_sync(same, &flat);
                    return;
                }
            }
            if let Some(parent) = self.parent_cursor()
                && flat.contains(&parent)
            {
                self.nav.set_cursor_and_sync(parent, &flat);
                return;
            }
        }
        self.nav.sync(&flat);
    }

    /// Cancels a loading expansion — reverts to `Collapsed`.
    pub fn cancel_expansion(&mut self, object_id: u64) {
        self.expansion.collapse_object_by_id(object_id);
    }

    /// Collapses an expanded object.
    pub fn collapse_object(&mut self, object_id: u64) {
        self.expansion.collapse_object_by_id(object_id);
    }

    /// Recursively collapses `object_id` and all nested expanded descendants.
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

    /// If the current cursor is no longer in the flat list, fall back to parent.
    fn resync_cursor_after_collapse(&mut self) {
        let flat = self.flat_items();
        if flat.contains(self.nav.cursor()) {
            return;
        }
        if let Some(parent) = self.parent_cursor()
            && flat.contains(&parent)
        {
            self.nav.set_cursor_and_sync(parent, &flat);
            return;
        }
        // Fall back to first interactive item.
        if let Some(idx) = Self::first_interactive_index(&flat) {
            self.nav.set_cursor_and_sync(flat[idx].clone(), &flat);
        } else {
            self.nav.sync(&flat);
        }
    }

    /// Loads vars for `frame_id` into internal cache and toggles expand/collapse.
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
            // Reset cursor to the frame row when collapsing from inside a frame.
            if let Some(path) = self.cursor_path()
                && let Some(PathSegment::Frame(fid)) = path.segments().first().cloned()
                && fid.0 == frame_id
            {
                let frame_path = NavigationPathBuilder::frame_only(fid);
                self.nav
                    .set_cursor_and_sync(RenderCursor::At(frame_path), &self.flat_items());
                return;
            }
            self.nav.sync(&self.flat_items());
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

    fn is_non_interactive_cursor(cursor: &RenderCursor) -> bool {
        matches!(
            cursor,
            RenderCursor::SectionHeader(_) | RenderCursor::OverflowRow(_)
        )
    }

    fn first_interactive_index(flat: &[RenderCursor]) -> Option<usize> {
        flat.iter()
            .position(|c| !Self::is_non_interactive_cursor(c))
    }

    fn last_interactive_index(flat: &[RenderCursor]) -> Option<usize> {
        flat.iter()
            .rposition(|c| !Self::is_non_interactive_cursor(c))
    }

    fn next_interactive_index(flat: &[RenderCursor], current: usize) -> Option<usize> {
        ((current + 1)..flat.len()).find(|&idx| !Self::is_non_interactive_cursor(&flat[idx]))
    }

    fn prev_interactive_index(flat: &[RenderCursor], current: usize) -> Option<usize> {
        if current == 0 {
            return None;
        }
        (0..current)
            .rev()
            .find(|&idx| !Self::is_non_interactive_cursor(&flat[idx]))
    }

    fn snap_cursor_to_interactive(&mut self, flat: &[RenderCursor], prefer_down: bool) {
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
        if selected_idx >= new_offset + visible_height {
            *self.nav.list_state_mut().offset_mut() = selected_idx + 1 - visible_height;
        }
    }

    /// Scrolls the visible window down by one line without moving the selection cursor.
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
        let current_offset = self.nav.list_state().offset().min(max_offset);
        let new_offset = current_offset.saturating_add(1).min(max_offset);
        *self.nav.list_state_mut().offset_mut() = new_offset;
        if selected_idx < new_offset {
            *self.nav.list_state_mut().offset_mut() = selected_idx;
        }
    }

    /// Scrolls the visible window up by one page without moving the selection cursor.
    pub fn scroll_view_page_up(&mut self) {
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
        let new_offset = current_offset.saturating_sub(visible_height);
        *self.nav.list_state_mut().offset_mut() = new_offset;
        if selected_idx >= new_offset + visible_height {
            *self.nav.list_state_mut().offset_mut() = selected_idx + 1 - visible_height;
        }
    }

    /// Scrolls the visible window down by one page without moving the selection cursor.
    pub fn scroll_view_page_down(&mut self) {
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
        let current_offset = self.nav.list_state().offset().min(max_offset);
        let new_offset = current_offset
            .saturating_add(visible_height)
            .min(max_offset);
        *self.nav.list_state_mut().offset_mut() = new_offset;
        if selected_idx < new_offset {
            *self.nav.list_state_mut().offset_mut() = selected_idx;
        }
    }

    /// Centers the selected row in the visible window when possible.
    pub fn center_view_on_selection(&mut self) {
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
        let center_row = visible_height / 2;
        let centered_offset = selected_idx.saturating_sub(center_row).min(max_offset);
        *self.nav.list_state_mut().offset_mut() = centered_offset;
    }

    #[cfg(test)]
    pub fn list_state_offset_for_test(&self) -> usize {
        self.nav.list_state().offset()
    }

    #[cfg(test)]
    pub fn set_list_state_offset_for_test(&mut self, offset: usize) {
        *self.nav.list_state_mut().offset_mut() = offset;
    }

    // === flat_items() and emit_* ===

    /// Flattened ordered list of cursors matching the rendered list items.
    pub(crate) fn flat_items(&self) -> Vec<RenderCursor> {
        let mut out = Vec::new();
        for frame in &self.frames {
            let fid = FrameId(frame.frame_id);
            let frame_path = NavigationPathBuilder::frame_only(fid);
            out.push(RenderCursor::At(frame_path.clone()));
            if self.expanded.contains(&frame.frame_id) {
                let empty = vec![];
                let vars = self.vars.get(&frame.frame_id).unwrap_or(&empty);
                if vars.is_empty() {
                    let var_path = NavigationPathBuilder::new(fid, VarIdx(0)).build();
                    out.push(RenderCursor::At(var_path));
                } else {
                    for (vi, var) in vars.iter().enumerate() {
                        let var_path = NavigationPathBuilder::new(fid, VarIdx(vi)).build();
                        out.push(RenderCursor::At(var_path.clone()));
                        if let VariableValue::ObjectRef {
                            id: object_id,
                            entry_count,
                            ..
                        } = &var.value
                        {
                            if entry_count.is_some() {
                                // Gate on expansion_phases AND collection_chunks.
                                let phase = self
                                    .expansion
                                    .expansion_phases
                                    .get(&var_path)
                                    .cloned()
                                    .unwrap_or(ExpansionPhase::Collapsed);
                                if phase == ExpansionPhase::Expanded
                                    && let Some(cc) =
                                        self.expansion.collection_chunks.get(object_id)
                                {
                                    let mut vis = HashSet::new();
                                    self.emit_collection_children_inner(
                                        &var_path,
                                        CollectionId(*object_id),
                                        cc,
                                        &mut out,
                                        &mut vis,
                                    );
                                }
                                continue;
                            }
                            let mut visited = HashSet::new();
                            self.emit_object_children(*object_id, var_path, &mut visited, &mut out);
                        }
                    }
                }
            }
        }
        if out.is_empty() {
            out.push(RenderCursor::NoFrames);
        }
        out
    }

    /// Emits cursor nodes for the children of `object_id`.
    fn emit_object_children(
        &self,
        object_id: u64,
        parent_path: NavigationPath,
        visited: &mut HashSet<u64>,
        out: &mut Vec<RenderCursor>,
    ) {
        if parent_path.segments().len() >= 18 {
            return;
        }
        match self.expansion_state(object_id) {
            ExpansionPhase::Collapsed => {}
            ExpansionPhase::Loading => {
                out.push(RenderCursor::LoadingNode(parent_path));
            }
            ExpansionPhase::Expanded => {
                visited.insert(object_id);
                let fields = self.expansion.object_fields.get(&object_id);
                let field_count = fields.map(|f| f.len()).unwrap_or(0);
                if field_count == 0 {
                    out.push(RenderCursor::LoadingNode(parent_path.clone()));
                } else {
                    let field_list = fields.unwrap();
                    for (idx, field) in field_list.iter().enumerate() {
                        let child_path = NavigationPathBuilder::extend(parent_path.clone())
                            .field(FieldIdx(idx))
                            .build();
                        if let FieldValue::ObjectRef { id, .. } = field.value
                            && visited.contains(&id)
                        {
                            out.push(RenderCursor::CyclicNode(child_path));
                            continue;
                        }
                        out.push(RenderCursor::At(child_path.clone()));
                        if let FieldValue::ObjectRef {
                            id,
                            entry_count: Some(_),
                            ..
                        } = field.value
                        {
                            self.emit_collection_children(&child_path, CollectionId(id), out);
                            continue;
                        }
                        if let FieldValue::ObjectRef { id, .. } = field.value {
                            self.emit_object_children(id, child_path, visited, out);
                        }
                    }
                }
                self.emit_static_rows(&parent_path, object_id, out);
                visited.remove(&object_id);
            }
            ExpansionPhase::Failed => {}
        }
    }

    fn emit_static_rows(
        &self,
        parent_path: &NavigationPath,
        object_id: u64,
        out: &mut Vec<RenderCursor>,
    ) {
        let Some(static_fields) = self.expansion.object_static_fields.get(&object_id) else {
            return;
        };
        if static_fields.is_empty() {
            return;
        }
        out.push(RenderCursor::SectionHeader(parent_path.clone()));
        let shown = static_fields.len().min(STATIC_FIELDS_RENDER_LIMIT);
        for (si, field) in static_fields.iter().take(shown).enumerate() {
            let static_path = NavigationPathBuilder::extend(parent_path.clone())
                .static_field(StaticFieldIdx(si))
                .build();
            out.push(RenderCursor::At(static_path.clone()));
            if let FieldValue::ObjectRef {
                id,
                entry_count: Some(_),
                ..
            } = field.value
            {
                self.emit_collection_children(&static_path, CollectionId(id), out);
                continue;
            }
            if let FieldValue::ObjectRef { id, .. } = field.value {
                let mut visited = HashSet::new();
                self.emit_static_object_children(&static_path, id, &[], &mut visited, out);
            }
        }
        if static_fields.len() > STATIC_FIELDS_RENDER_LIMIT {
            out.push(RenderCursor::OverflowRow(parent_path.clone()));
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_static_object_children(
        &self,
        static_field_path: &NavigationPath,
        obj_id: u64,
        obj_path: &[usize],
        visited: &mut HashSet<u64>,
        out: &mut Vec<RenderCursor>,
    ) {
        if obj_path.len() >= 16 {
            return;
        }
        match self.expansion_state(obj_id) {
            ExpansionPhase::Collapsed => {}
            ExpansionPhase::Loading => {
                out.push(RenderCursor::LoadingNode(static_field_path.clone()));
            }
            ExpansionPhase::Failed => {}
            ExpansionPhase::Expanded => {
                let fields = self.expansion.object_fields.get(&obj_id);
                let field_count = fields.map(|f| f.len()).unwrap_or(0);
                if field_count == 0 {
                    out.push(RenderCursor::LoadingNode(static_field_path.clone()));
                } else {
                    visited.insert(obj_id);
                    let field_list = fields.unwrap();
                    for (idx, field) in field_list.iter().enumerate() {
                        let mut new_obj_path = obj_path.to_vec();
                        new_obj_path.push(idx);
                        let child_path = NavigationPathBuilder::extend(static_field_path.clone())
                            .field(FieldIdx(idx))
                            .build();
                        if let FieldValue::ObjectRef { id, .. } = field.value
                            && visited.contains(&id)
                        {
                            out.push(RenderCursor::CyclicNode(child_path));
                            continue;
                        }
                        out.push(RenderCursor::At(child_path.clone()));
                        if let FieldValue::ObjectRef {
                            id,
                            entry_count: Some(_),
                            ..
                        } = field.value
                        {
                            self.emit_collection_children(&child_path, CollectionId(id), out);
                            continue;
                        }
                        if let FieldValue::ObjectRef { id, .. } = field.value {
                            self.emit_static_object_children(
                                &child_path,
                                id,
                                &new_obj_path,
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

    /// Emits cursors for collection entries and chunk sections.
    ///
    /// Gates on `expansion_phases` for `parent_path` AND `collection_chunks`.
    ///
    /// `parent_path` is the row that owns the collection (a Var or Field row).
    /// Only emits if `expansion_phases[parent_path] == Expanded` AND chunks are loaded.
    fn emit_collection_children(
        &self,
        parent_path: &NavigationPath,
        collection_id: CollectionId,
        out: &mut Vec<RenderCursor>,
    ) {
        let phase = self
            .expansion
            .expansion_phases
            .get(parent_path)
            .cloned()
            .unwrap_or(ExpansionPhase::Collapsed);
        if phase != ExpansionPhase::Expanded {
            return;
        }
        let Some(cc) = self.expansion.collection_chunks.get(&collection_id.0) else {
            return;
        };
        let mut vis = HashSet::new();
        self.emit_collection_children_inner(parent_path, collection_id, cc, out, &mut vis);
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_collection_children_inner(
        &self,
        parent_path: &NavigationPath,
        collection_id: CollectionId,
        cc: &CollectionChunks,
        out: &mut Vec<RenderCursor>,
        visited_collections: &mut HashSet<u64>,
    ) {
        if !visited_collections.insert(collection_id.0) {
            return;
        }
        if let Some(page) = &cc.eager_page {
            for entry in &page.entries {
                self.emit_collection_entry_cursor(parent_path, collection_id, entry, out);
            }
        }
        let ranges = compute_chunk_ranges(cc.total_count);
        for (offset, _) in &ranges {
            let chunk_path = NavigationPathBuilder::extend(parent_path.clone())
                .collection_entry(collection_id, EntryIdx(*offset))
                .build();
            out.push(RenderCursor::ChunkSection(chunk_path, ChunkOffset(*offset)));
            if let Some(ChunkState::Loaded(page)) = cc.chunk_pages.get(offset) {
                for entry in &page.entries {
                    self.emit_collection_entry_cursor(parent_path, collection_id, entry, out);
                }
            }
        }
        visited_collections.remove(&collection_id.0);
    }

    fn emit_collection_entry_cursor(
        &self,
        parent_path: &NavigationPath,
        collection_id: CollectionId,
        entry: &hprof_engine::EntryInfo,
        out: &mut Vec<RenderCursor>,
    ) {
        let entry_path = NavigationPathBuilder::extend(parent_path.clone())
            .collection_entry(collection_id, EntryIdx(entry.index))
            .build();
        out.push(RenderCursor::At(entry_path.clone()));

        if let FieldValue::ObjectRef {
            id,
            entry_count: Some(_),
            ..
        } = &entry.value
            && *id != collection_id.0
        {
            self.emit_collection_children(&entry_path, CollectionId(*id), out);
            return;
        }

        if let FieldValue::ObjectRef { id, .. } = &entry.value {
            let mut visited = HashSet::new();
            self.emit_collection_entry_obj_children(&entry_path, *id, &[], &mut visited, out);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_collection_entry_obj_children(
        &self,
        entry_path: &NavigationPath,
        obj_id: u64,
        obj_path: &[usize],
        visited: &mut HashSet<u64>,
        out: &mut Vec<RenderCursor>,
    ) {
        if obj_path.len() >= 16 {
            return;
        }
        match self.expansion_state(obj_id) {
            ExpansionPhase::Collapsed => {}
            ExpansionPhase::Loading => {
                out.push(RenderCursor::LoadingNode(entry_path.clone()));
            }
            ExpansionPhase::Failed => {}
            ExpansionPhase::Expanded => {
                let fields = self.expansion.object_fields.get(&obj_id);
                let field_count = fields.map(|f| f.len()).unwrap_or(0);
                if field_count == 0 {
                    out.push(RenderCursor::LoadingNode(entry_path.clone()));
                } else {
                    visited.insert(obj_id);
                    let field_list = fields.unwrap();
                    for (idx, field) in field_list.iter().enumerate() {
                        let child_path = NavigationPathBuilder::extend(entry_path.clone())
                            .field(FieldIdx(idx))
                            .build();
                        let mut new_path = obj_path.to_vec();
                        new_path.push(idx);
                        // Cyclic refs are emitted as At (non-recursive) rather than CyclicNode
                        // so they remain navigable; cycle detection is in the accessor.
                        let is_cyclic = matches!(field.value, FieldValue::ObjectRef { id, .. }
                            if visited.contains(&id));
                        out.push(RenderCursor::At(child_path.clone()));
                        if is_cyclic {
                            continue;
                        }
                        if let FieldValue::ObjectRef {
                            id,
                            entry_count: Some(_),
                            ..
                        } = field.value
                        {
                            self.emit_collection_children(&child_path, CollectionId(id), out);
                            continue;
                        }
                        if let FieldValue::ObjectRef { id, .. } = field.value {
                            self.emit_collection_entry_obj_children(
                                &child_path,
                                id,
                                &new_path,
                                visited,
                                out,
                            );
                        }
                    }
                    visited.remove(&obj_id);
                }
                self.emit_collection_entry_static_rows(entry_path, obj_id, out);
            }
        }
    }

    fn emit_collection_entry_static_rows(
        &self,
        entry_path: &NavigationPath,
        object_id: u64,
        out: &mut Vec<RenderCursor>,
    ) {
        let Some(static_fields) = self.expansion.object_static_fields.get(&object_id) else {
            return;
        };
        if static_fields.is_empty() {
            return;
        }
        out.push(RenderCursor::SectionHeader(entry_path.clone()));
        let shown = static_fields.len().min(STATIC_FIELDS_RENDER_LIMIT);
        for (si, field) in static_fields.iter().take(shown).enumerate() {
            let static_path = NavigationPathBuilder::extend(entry_path.clone())
                .static_field(StaticFieldIdx(si))
                .build();
            out.push(RenderCursor::At(static_path.clone()));
            if let FieldValue::ObjectRef {
                id,
                entry_count: Some(_),
                ..
            } = field.value
            {
                self.emit_collection_children(&static_path, CollectionId(id), out);
                continue;
            }
            if let FieldValue::ObjectRef { id, .. } = field.value {
                let mut visited = HashSet::new();
                self.emit_coll_entry_static_object_children(
                    &static_path,
                    id,
                    &[],
                    &mut visited,
                    out,
                );
            }
        }
        if static_fields.len() > STATIC_FIELDS_RENDER_LIMIT {
            out.push(RenderCursor::OverflowRow(entry_path.clone()));
        }
    }

    fn emit_coll_entry_static_object_children(
        &self,
        static_path: &NavigationPath,
        obj_id: u64,
        obj_path: &[usize],
        visited: &mut HashSet<u64>,
        out: &mut Vec<RenderCursor>,
    ) {
        if obj_path.len() >= 16 {
            return;
        }
        match self.expansion_state(obj_id) {
            ExpansionPhase::Collapsed => {}
            ExpansionPhase::Loading => {
                out.push(RenderCursor::LoadingNode(static_path.clone()));
            }
            ExpansionPhase::Failed => {}
            ExpansionPhase::Expanded => {
                let fields = self.expansion.object_fields.get(&obj_id);
                let field_count = fields.map(|f| f.len()).unwrap_or(0);
                if field_count == 0 {
                    out.push(RenderCursor::LoadingNode(static_path.clone()));
                } else {
                    visited.insert(obj_id);
                    let field_list = fields.unwrap();
                    for (idx, field) in field_list.iter().enumerate() {
                        let child_path = NavigationPathBuilder::extend(static_path.clone())
                            .field(FieldIdx(idx))
                            .build();
                        let mut new_path = obj_path.to_vec();
                        new_path.push(idx);
                        if let FieldValue::ObjectRef { id, .. } = field.value
                            && visited.contains(&id)
                        {
                            out.push(RenderCursor::CyclicNode(child_path));
                            continue;
                        }
                        out.push(RenderCursor::At(child_path.clone()));
                        if let FieldValue::ObjectRef {
                            id,
                            entry_count: Some(_),
                            ..
                        } = field.value
                        {
                            self.emit_collection_children(&child_path, CollectionId(id), out);
                            continue;
                        }
                        if let FieldValue::ObjectRef { id, .. } = field.value {
                            self.emit_coll_entry_static_object_children(
                                &child_path,
                                id,
                                &new_path,
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

    // === Rendering ===

    /// Builds the list items for rendering.
    pub fn build_items(&self) -> Vec<ListItem<'static>> {
        self.build_items_with_object_ids(false)
    }

    /// Builds list items with optional object ID display.
    pub fn build_items_with_object_ids(&self, show_object_ids: bool) -> Vec<ListItem<'static>> {
        use super::super::tree_render::{RenderOptions, TreeRoot, render_variable_tree};
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
                    RenderOptions {
                        show_object_ids,
                        snapshot_mode: false,
                    },
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
