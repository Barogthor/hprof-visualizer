//! Favorites panel data model: pinning, snapshots, and snapshot construction.
//!
//! [`PinnedItem`] captures a frozen snapshot of a stack position at pin time.
//! [`snapshot_from_cursor`] builds a `PinnedItem` from the current `StackState`.

use std::collections::{HashMap, HashSet};

use hprof_engine::{FieldInfo, FieldValue, VariableInfo, VariableValue};

use crate::views::stack_view::{
    ChunkState, CollectionChunks, NavigationPath, PathSegment, RenderCursor, StackState, ThreadId,
    format_frame_label, format_frame_label_short,
};

/// Maximum number of object IDs captured in a single snapshot (across all
/// vars of a frame). Prevents unbounded memory use on deep object graphs.
const SNAPSHOT_OBJECT_LIMIT: usize = 500;

type SnapshotObjectFields = HashMap<u64, Vec<FieldInfo>>;
type SnapshotStaticFields = HashMap<u64, Vec<FieldInfo>>;
type SnapshotCollectionChunks = HashMap<u64, CollectionChunks>;
type SubtreeSnapshot = (
    SnapshotObjectFields,
    SnapshotStaticFields,
    SnapshotCollectionChunks,
    bool,
);

/// Structural position identifier used for toggle detection.
///
/// Two pins are the same if and only if their `PinKey` compares equal.
/// Equality uses `thread_id + nav_path` (not `thread_name`).
#[derive(Debug, Clone, Eq)]
pub struct PinKey {
    /// Thread that owns this pin (for thread selection during `g` navigation).
    pub thread_id: ThreadId,
    /// Display name of the owning thread.
    pub thread_name: String,
    /// Semantic position within the thread's stack.
    pub nav_path: NavigationPath,
}

impl PartialEq for PinKey {
    fn eq(&self, other: &Self) -> bool {
        self.thread_id == other.thread_id && self.nav_path == other.nav_path
    }
}

/// Frozen content captured at pin time.
pub enum PinnedSnapshot {
    /// Whole frame: all variables and their expanded object/collection trees.
    Frame {
        variables: Vec<VariableInfo>,
        object_fields: HashMap<u64, Vec<FieldInfo>>,
        object_static_fields: HashMap<u64, Vec<FieldInfo>>,
        collection_chunks: HashMap<u64, CollectionChunks>,
        truncated: bool,
    },
    /// A single expanded `ObjectRef` variable or field plus its subtree.
    Subtree {
        root_id: u64,
        object_fields: HashMap<u64, Vec<FieldInfo>>,
        object_static_fields: HashMap<u64, Vec<FieldInfo>>,
        collection_chunks: HashMap<u64, CollectionChunks>,
        truncated: bool,
    },
    /// An `ObjectRef` that was **not** expanded at pin time.
    UnexpandedRef { class_name: String, object_id: u64 },
    /// A primitive value or null.
    Primitive { value_label: String },
}

/// Identifies a renderable row that can be hidden within a pinned snapshot.
///
/// Only instance fields and Frame local variables are in scope.
/// Static fields are excluded for simplicity.
///
/// Used as key in [`PinnedItem::hidden_fields`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HideKey {
    /// A local variable row in a `Frame` snapshot: index in the `variables` vec.
    Var(usize),
    /// An instance field of an expanded object:
    /// (`parent_id`, `field_idx` in that object's `FieldInfo` vec).
    Field { parent_id: u64, field_idx: usize },
}

/// A single pinned item shown in the favorites panel.
pub struct PinnedItem {
    /// Name of the thread that owns this pin.
    pub thread_name: String,
    /// Label of the enclosing stack frame.
    pub frame_label: String,
    /// Short label: e.g. `"var[2]"` or `"var[0].cache.size"`.
    pub item_label: String,
    /// Frozen snapshot of the pinned data.
    pub snapshot: PinnedSnapshot,
    /// Objects or collection nodes collapsed by the user inside this snapshot.
    ///
    /// Default is empty: all captured nodes are expanded in the favorites view.
    pub local_collapsed: HashSet<u64>,
    /// Field/variable rows hidden by the user (`h` key). Hidden rows are
    /// replaced by a `▪ [hidden: …]` placeholder.
    pub hidden_fields: HashSet<HideKey>,
    /// Structural key used for toggle detection and de-duplication.
    pub key: PinKey,
}

struct DescendantsCollector<'a> {
    fields: &'a HashMap<u64, Vec<FieldInfo>>,
    collection_chunks: &'a HashMap<u64, CollectionChunks>,
    visited: &'a mut HashSet<u64>,
    out: &'a mut Vec<u64>,
    limit: usize,
}

impl DescendantsCollector<'_> {
    fn walk(&mut self, object_id: u64) -> bool {
        if self.visited.len() >= self.limit {
            return true;
        }
        if !self.visited.insert(object_id) {
            return false;
        }

        let mut truncated = self.walk_object_fields(object_id);
        if !truncated {
            truncated = self.walk_collection_pages(object_id);
        }

        self.out.push(object_id);
        truncated
    }

    fn walk_object_fields(&mut self, object_id: u64) -> bool {
        let Some(field_list) = self.fields.get(&object_id) else {
            return false;
        };

        for field in field_list {
            if let FieldValue::ObjectRef { id, .. } = field.value
                && self.walk_child(id)
            {
                return true;
            }
        }
        false
    }

    fn walk_collection_pages(&mut self, object_id: u64) -> bool {
        let Some(chunks) = self.collection_chunks.get(&object_id) else {
            return false;
        };

        if let Some(page) = &chunks.eager_page
            && self.walk_collection_entries(page.entries.as_slice())
        {
            return true;
        }

        for page_state in chunks.chunk_pages.values() {
            let ChunkState::Loaded(page) = page_state else {
                continue;
            };
            if self.walk_collection_entries(page.entries.as_slice()) {
                return true;
            }
        }

        false
    }

    fn walk_collection_entries(&mut self, entries: &[hprof_engine::EntryInfo]) -> bool {
        for entry in entries {
            if let FieldValue::ObjectRef { id, .. } = &entry.value
                && self.walk_child(*id)
            {
                return true;
            }
        }
        false
    }

    fn walk_child(&mut self, object_id: u64) -> bool {
        if self.walk(object_id) {
            return true;
        }
        self.visited.len() >= self.limit
    }
}

/// Builds a [`PinnedItem`] from the current cursor position, or `None` if the
/// cursor position cannot be pinned (e.g. loading/cyclic pseudo-nodes).
///
/// Only `RenderCursor::At(path)` produces a pin; all other variants return `None`.
pub fn snapshot_from_cursor(
    cursor: &RenderCursor,
    state: &StackState,
    thread_name: &str,
    thread_id: ThreadId,
) -> Option<PinnedItem> {
    let RenderCursor::At(path) = cursor else {
        return None;
    };
    PinnedItemFactory::new(state, thread_name, thread_id).build_from_path(path)
}

struct FrameCtx {
    frame_label: String,
}

struct PinnedItemFactory<'a> {
    state: &'a StackState,
    thread_name: &'a str,
    thread_id: ThreadId,
}

impl<'a> PinnedItemFactory<'a> {
    fn new(state: &'a StackState, thread_name: &'a str, thread_id: ThreadId) -> Self {
        Self {
            state,
            thread_name,
            thread_id,
        }
    }

    fn build_from_path(&self, path: &NavigationPath) -> Option<PinnedItem> {
        let segs = path.segments();
        let frame_id = match segs.first()? {
            PathSegment::Frame(fid) => fid.0,
            _ => return None,
        };

        let ctx = self.frame_context(frame_id)?;
        let item_label = build_label_from_path(self.state, path);

        let snapshot = self.snapshot_for_path(path, frame_id)?;

        Some(self.make_pinned_item(ctx.frame_label, item_label, snapshot, path.clone()))
    }

    /// Builds the snapshot for the position described by `path`.
    fn snapshot_for_path(&self, path: &NavigationPath, frame_id: u64) -> Option<PinnedSnapshot> {
        let segs = path.segments();

        if segs.len() == 1 {
            // Frame-only pin.
            return Some(self.snapshot_on_frame(frame_id));
        }

        let var_idx = match &segs[1] {
            PathSegment::Var(vi) => vi.0,
            _ => return None,
        };

        if segs.len() == 2 {
            // Var-level pin.
            let var = self.state.vars().get(&frame_id)?.get(var_idx)?;
            return Some(self.snapshot_for_variable_value(&var.value));
        }

        // Deeper path — walk to the leaf segment.
        let leaf = segs.last()?;
        match leaf {
            PathSegment::Field(fi) => {
                let parent_id = self.resolve_parent_object(path)?;
                let fields = self.state.object_fields().get(&parent_id)?;
                let field = fields.get(fi.0)?;
                Some(self.snapshot_for_field_value(&field.value))
            }
            PathSegment::StaticField(si) => {
                let owner_id = self.resolve_static_field_owner(path)?;
                let static_fields = self.state.object_static_fields().get(&owner_id)?;
                let field = static_fields.get(si.0)?;
                Some(self.snapshot_for_field_value(&field.value))
            }
            PathSegment::CollectionEntry(cid, ei) => {
                let cc = self.state.collection_chunks_map().get(&cid.0)?;
                let entry = cc.find_entry(ei.0)?;
                Some(self.snapshot_for_field_value(&entry.value))
            }
            _ => None,
        }
    }

    fn snapshot_on_frame(&self, frame_id: u64) -> PinnedSnapshot {
        let vars = self
            .state
            .vars()
            .get(&frame_id)
            .cloned()
            .unwrap_or_default();

        let mut reachable = HashSet::new();
        let mut all_fields: SnapshotObjectFields = HashMap::new();
        let mut all_static_fields: SnapshotStaticFields = HashMap::new();
        let mut all_chunks: SnapshotCollectionChunks = HashMap::new();
        let mut any_truncated = false;

        for var in &vars {
            if reachable.len() >= SNAPSHOT_OBJECT_LIMIT {
                any_truncated = true;
                break;
            }
            if let VariableValue::ObjectRef { id, .. } = var.value {
                let (fields, static_fields, chunks, trunc) =
                    self.subtree_snapshot(id, &mut reachable);
                all_fields.extend(fields);
                all_static_fields.extend(static_fields);
                all_chunks.extend(chunks);
                if trunc {
                    any_truncated = true;
                }
            }
        }

        PinnedSnapshot::Frame {
            variables: vars,
            object_fields: all_fields,
            object_static_fields: all_static_fields,
            collection_chunks: all_chunks,
            truncated: any_truncated,
        }
    }

    /// Resolves the `object_id` of the parent object at the second-to-last segment.
    fn resolve_parent_object(&self, path: &NavigationPath) -> Option<u64> {
        let segs = path.segments();
        let frame_id = match segs.first()? {
            PathSegment::Frame(fid) => fid.0,
            _ => return None,
        };
        let var_idx = match segs.get(1)? {
            PathSegment::Var(vi) => vi.0,
            _ => return None,
        };
        let var = self.state.vars().get(&frame_id)?.get(var_idx)?;
        let root_id = match var.value {
            VariableValue::ObjectRef { id, .. } => id,
            _ => return None,
        };
        // Walk all segments except the last.
        let mut current = root_id;
        for seg in &segs[2..segs.len().saturating_sub(1)] {
            current = self.advance_object_id(current, seg)?;
        }
        Some(current)
    }

    /// Resolves the owner object_id for a `StaticField` leaf in `path`.
    fn resolve_static_field_owner(&self, path: &NavigationPath) -> Option<u64> {
        let segs = path.segments();
        let static_pos = segs
            .iter()
            .rposition(|s| matches!(s, PathSegment::StaticField(_)))?;
        let frame_id = match segs.first()? {
            PathSegment::Frame(fid) => fid.0,
            _ => return None,
        };
        let var_idx = match segs.get(1)? {
            PathSegment::Var(vi) => vi.0,
            _ => return None,
        };
        let var = self.state.vars().get(&frame_id)?.get(var_idx)?;
        let root_id = match var.value {
            VariableValue::ObjectRef { id, .. } => id,
            _ => return None,
        };
        let mut current = root_id;
        for seg in &segs[2..static_pos] {
            current = self.advance_object_id(current, seg)?;
        }
        Some(current)
    }

    fn advance_object_id(&self, current: u64, seg: &PathSegment) -> Option<u64> {
        match seg {
            PathSegment::Field(fi) => {
                let fields = self.state.object_fields().get(&current)?;
                match fields.get(fi.0)?.value {
                    FieldValue::ObjectRef { id, .. } => Some(id),
                    _ => None,
                }
            }
            PathSegment::CollectionEntry(cid, ei) => {
                let cc = self.state.collection_chunks_map().get(&cid.0)?;
                let entry = cc.find_entry(ei.0)?;
                match &entry.value {
                    FieldValue::ObjectRef { id, .. } => Some(*id),
                    _ => None,
                }
            }
            PathSegment::StaticField(si) => {
                let static_fields = self.state.object_static_fields().get(&current)?;
                match static_fields.get(si.0)?.value {
                    FieldValue::ObjectRef { id, .. } => Some(id),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Walks the reachable object graph from `root_id`, cloning fields and
    /// collection chunks into fresh maps.
    fn subtree_snapshot(&self, root_id: u64, reachable: &mut HashSet<u64>) -> SubtreeSnapshot {
        let mut desc_order: Vec<u64> = Vec::new();
        let truncated = self.collect_descendants_limited(
            root_id,
            reachable,
            &mut desc_order,
            SNAPSHOT_OBJECT_LIMIT,
        ) || reachable.len() >= SNAPSHOT_OBJECT_LIMIT;

        let mut snap_fields: SnapshotObjectFields = HashMap::new();
        let mut snap_static_fields: SnapshotStaticFields = HashMap::new();
        let mut snap_chunks: SnapshotCollectionChunks = HashMap::new();

        for &id in &desc_order {
            if let Some(fields) = self.state.object_fields().get(&id) {
                snap_fields.insert(id, fields.clone());
            }
            if let Some(fields) = self.state.object_static_fields().get(&id) {
                snap_static_fields.insert(id, fields.clone());
            }
            if let Some(cc) = self.state.collection_chunks_map().get(&id) {
                snap_chunks.insert(id, self.freeze_collection_chunks(cc));
            }
        }
        (snap_fields, snap_static_fields, snap_chunks, truncated)
    }

    fn collect_descendants_limited(
        &self,
        root_id: u64,
        visited: &mut HashSet<u64>,
        out: &mut Vec<u64>,
        limit: usize,
    ) -> bool {
        let mut collector = DescendantsCollector {
            fields: self.state.object_fields(),
            collection_chunks: self.state.collection_chunks_map(),
            visited,
            out,
            limit,
        };
        collector.walk(root_id)
    }

    fn freeze_collection_chunks(&self, chunks: &CollectionChunks) -> CollectionChunks {
        let mut chunk_pages = HashMap::with_capacity(chunks.chunk_pages.len());
        for (offset, state) in &chunks.chunk_pages {
            let frozen = match state {
                ChunkState::Collapsed => ChunkState::Collapsed,
                ChunkState::Loading => ChunkState::Collapsed,
                ChunkState::Loaded(page) => ChunkState::Loaded(page.clone()),
            };
            chunk_pages.insert(*offset, frozen);
        }

        CollectionChunks {
            total_count: chunks.total_count,
            eager_page: chunks.eager_page.clone(),
            chunk_pages,
        }
    }

    fn frame_context(&self, frame_id: u64) -> Option<FrameCtx> {
        let frame = self
            .state
            .frames()
            .iter()
            .find(|f| f.frame_id == frame_id)?;
        Some(FrameCtx {
            frame_label: format_frame_label_short(frame),
        })
    }

    fn object_has_snapshot_data(&self, object_id: u64) -> bool {
        self.state.object_fields().contains_key(&object_id)
            || self.state.object_static_fields().contains_key(&object_id)
            || self.state.collection_chunks_map().contains_key(&object_id)
    }

    fn snapshot_for_object_ref(&self, object_id: u64, class_name: &str) -> PinnedSnapshot {
        if self.object_has_snapshot_data(object_id) {
            let mut reachable = HashSet::new();
            let (fields, static_fields, chunks, truncated) =
                self.subtree_snapshot(object_id, &mut reachable);
            PinnedSnapshot::Subtree {
                root_id: object_id,
                object_fields: fields,
                object_static_fields: static_fields,
                collection_chunks: chunks,
                truncated,
            }
        } else {
            PinnedSnapshot::UnexpandedRef {
                class_name: class_name.to_string(),
                object_id,
            }
        }
    }

    fn snapshot_for_variable_value(&self, value: &VariableValue) -> PinnedSnapshot {
        match value {
            VariableValue::ObjectRef { id, class_name, .. } => {
                self.snapshot_for_object_ref(*id, class_name)
            }
            VariableValue::Null => PinnedSnapshot::Primitive {
                value_label: "null".to_string(),
            },
        }
    }

    fn snapshot_for_field_value(&self, value: &FieldValue) -> PinnedSnapshot {
        match value {
            FieldValue::ObjectRef { id, class_name, .. } => {
                self.snapshot_for_object_ref(*id, class_name)
            }
            FieldValue::Null => PinnedSnapshot::Primitive {
                value_label: "null".to_string(),
            },
            other => PinnedSnapshot::Primitive {
                value_label: format_primitive_field_value(other),
            },
        }
    }

    fn make_pinned_item(
        &self,
        frame_label: String,
        item_label: String,
        snapshot: PinnedSnapshot,
        nav_path: NavigationPath,
    ) -> PinnedItem {
        PinnedItem {
            thread_name: self.thread_name.to_string(),
            frame_label,
            item_label,
            snapshot,
            local_collapsed: HashSet::new(),
            hidden_fields: HashSet::new(),
            key: PinKey {
                thread_id: self.thread_id,
                thread_name: self.thread_name.to_string(),
                nav_path,
            },
        }
    }
}

/// Builds a human-readable label for a `NavigationPath`, e.g. `"var[0].cache.size"`.
///
/// Walks segments left-to-right:
/// - `Var(idx)` → `"var[{idx}]"`
/// - `Field(idx)` → field name from `object_fields`, or `"field[{idx}]"` fallback
/// - `StaticField(idx)` → static field name from `object_static_fields`, or `"static[{idx}]"`
/// - `CollectionEntry(_, entry_idx)` → `"[{entry_idx}]"` appended as a suffix
fn build_label_from_path(state: &StackState, path: &NavigationPath) -> String {
    let segs = path.segments();
    let frame_id = match segs.first() {
        Some(PathSegment::Frame(fid)) => fid.0,
        _ => return String::new(),
    };
    if segs.len() < 2 {
        // Frame-only path: label is the frame label.
        if let Some(frame) = state.frames().iter().find(|f| f.frame_id == frame_id) {
            return format_frame_label(frame);
        }
        return String::new();
    }

    let var_idx = match &segs[1] {
        PathSegment::Var(vi) => vi.0,
        _ => return String::new(),
    };

    let mut parts: Vec<String> = vec![format!("var[{var_idx}]")];
    let mut current_id: Option<u64> = state
        .vars()
        .get(&frame_id)
        .and_then(|vars| vars.get(var_idx))
        .and_then(|var| {
            if let VariableValue::ObjectRef { id, .. } = var.value {
                Some(id)
            } else {
                None
            }
        });

    for seg in &segs[2..] {
        match seg {
            PathSegment::Field(fi) => {
                let (name, next_id) = if let Some(cid) = current_id {
                    let fields = state.object_fields().get(&cid);
                    let field = fields.and_then(|f| f.get(fi.0));
                    let name = field
                        .map(|f| f.name.clone())
                        .unwrap_or_else(|| format!("field[{}]", fi.0));
                    let next = field.and_then(|f| {
                        if let FieldValue::ObjectRef { id, .. } = f.value {
                            Some(id)
                        } else {
                            None
                        }
                    });
                    (name, next)
                } else {
                    (format!("field[{}]", fi.0), None)
                };
                parts.push(name);
                current_id = next_id;
            }
            PathSegment::StaticField(si) => {
                let (name, next_id) = if let Some(cid) = current_id {
                    let static_fields = state.object_static_fields().get(&cid);
                    let field = static_fields.and_then(|f| f.get(si.0));
                    let name = field
                        .map(|f| f.name.clone())
                        .unwrap_or_else(|| format!("static[{}]", si.0));
                    let next = field.and_then(|f| {
                        if let FieldValue::ObjectRef { id, .. } = f.value {
                            Some(id)
                        } else {
                            None
                        }
                    });
                    (name, next)
                } else {
                    (format!("static[{}]", si.0), None)
                };
                parts.push(name);
                current_id = next_id;
            }
            PathSegment::CollectionEntry(cid, ei) => {
                // Append as bracketed suffix on the last part.
                if let Some(last) = parts.last_mut() {
                    last.push_str(&format!("[{}]", ei.0));
                } else {
                    parts.push(format!("[{}]", ei.0));
                }
                // Advance current_id to the entry's object if available.
                current_id = state
                    .collection_chunks_map()
                    .get(&cid.0)
                    .and_then(|cc| cc.find_entry(ei.0))
                    .and_then(|e| {
                        if let FieldValue::ObjectRef { id, .. } = &e.value {
                            Some(*id)
                        } else {
                            None
                        }
                    });
            }
            _ => {}
        }
    }

    parts.join(".")
}

fn format_primitive_field_value(v: &FieldValue) -> String {
    match v {
        FieldValue::Bool(b) => b.to_string(),
        FieldValue::Char(c) => format!("'{c}'"),
        FieldValue::Byte(n) => n.to_string(),
        FieldValue::Short(n) => n.to_string(),
        FieldValue::Int(n) => n.to_string(),
        FieldValue::Long(n) => n.to_string(),
        FieldValue::Float(f) => format!("{f}"),
        FieldValue::Double(d) => format!("{d}"),
        FieldValue::Null => "null".to_string(),
        FieldValue::ObjectRef { .. } => unreachable!("handled separately"),
    }
}

#[cfg(test)]
mod tests {
    use hprof_engine::{FieldInfo, FieldValue, FrameInfo, LineNumber, VariableInfo, VariableValue};

    use super::*;
    use crate::views::stack_view::{FieldIdx, FrameId, NavigationPathBuilder, StackState, VarIdx};

    // ── Task 6.1–6.5: HideKey and hidden_fields ─────────────────────────────

    #[test]
    fn hide_key_var_and_field_are_distinct() {
        let var = HideKey::Var(0);
        let field = HideKey::Field {
            parent_id: 0,
            field_idx: 0,
        };
        assert_ne!(var, field);
        let mut set = HashSet::new();
        set.insert(var);
        set.insert(field);
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn snapshot_from_cursor_initializes_hidden_fields_empty() {
        let state = make_state_with_frame(
            1,
            vec![VariableInfo {
                index: 0,
                value: VariableValue::Null,
            }],
        );
        let cursor = make_cursor_at(1);
        let item = snapshot_from_cursor(&cursor, &state, "main", thread_id()).unwrap();
        assert!(item.hidden_fields.is_empty());
    }

    #[test]
    fn pinned_item_hidden_fields_toggle_hides_and_restores() {
        let state = make_state_with_frame(
            1,
            vec![VariableInfo {
                index: 0,
                value: VariableValue::Null,
            }],
        );
        let cursor = make_cursor_at(1);
        let mut item = snapshot_from_cursor(&cursor, &state, "main", thread_id()).unwrap();
        item.hidden_fields.insert(HideKey::Var(0));
        assert!(item.hidden_fields.contains(&HideKey::Var(0)));
        item.hidden_fields.remove(&HideKey::Var(0));
        assert!(!item.hidden_fields.contains(&HideKey::Var(0)));
    }

    #[test]
    fn pinned_item_hidden_fields_reset_clears_multiple() {
        let state = make_state_with_frame(
            1,
            vec![VariableInfo {
                index: 0,
                value: VariableValue::Null,
            }],
        );
        let cursor = make_cursor_at(1);
        let mut item = snapshot_from_cursor(&cursor, &state, "main", thread_id()).unwrap();
        item.hidden_fields.insert(HideKey::Var(0));
        item.hidden_fields.insert(HideKey::Field {
            parent_id: 1,
            field_idx: 0,
        });
        item.hidden_fields.clear();
        assert!(item.hidden_fields.is_empty());
    }

    #[test]
    fn pinned_item_hidden_fields_reset_noop_when_empty() {
        let state = make_state_with_frame(
            1,
            vec![VariableInfo {
                index: 0,
                value: VariableValue::Null,
            }],
        );
        let cursor = make_cursor_at(1);
        let mut item = snapshot_from_cursor(&cursor, &state, "main", thread_id()).unwrap();
        assert!(item.hidden_fields.is_empty());
        item.hidden_fields.clear(); // no-op
        assert!(item.hidden_fields.is_empty());
    }

    fn make_frame(frame_id: u64) -> FrameInfo {
        FrameInfo {
            frame_id,
            method_name: "method".to_string(),
            class_name: "Class".to_string(),
            source_file: "Class.java".to_string(),
            line: LineNumber::Line(1),
            has_variables: true,
        }
    }

    fn make_state_with_frame(frame_id: u64, vars: Vec<VariableInfo>) -> StackState {
        let mut state = StackState::new(vec![make_frame(frame_id)]);
        state.toggle_expand(frame_id, vars);
        state
    }

    fn make_cursor_at(frame_id: u64) -> RenderCursor {
        RenderCursor::At(NavigationPathBuilder::frame_only(FrameId(frame_id)))
    }

    fn make_cursor_var(frame_id: u64, var_idx: usize) -> RenderCursor {
        RenderCursor::At(NavigationPathBuilder::new(FrameId(frame_id), VarIdx(var_idx)).build())
    }

    fn thread_id() -> ThreadId {
        ThreadId(1)
    }

    #[test]
    fn pin_key_same_thread_and_path_are_equal() {
        let path1 = NavigationPathBuilder::frame_only(FrameId(42));
        let path2 = NavigationPathBuilder::frame_only(FrameId(42));
        let k1 = PinKey {
            thread_id: ThreadId(1),
            thread_name: "Thread-1".to_string(),
            nav_path: path1,
        };
        let k2 = PinKey {
            thread_id: ThreadId(1),
            thread_name: "Thread-1-renamed".to_string(), // different name, same id
            nav_path: path2,
        };
        assert_eq!(k1, k2);
    }

    #[test]
    fn pin_key_different_thread_id_not_equal() {
        let path1 = NavigationPathBuilder::frame_only(FrameId(42));
        let path2 = NavigationPathBuilder::frame_only(FrameId(42));
        let k1 = PinKey {
            thread_id: ThreadId(1),
            thread_name: "main".to_string(),
            nav_path: path1,
        };
        let k2 = PinKey {
            thread_id: ThreadId(2),
            thread_name: "main".to_string(),
            nav_path: path2,
        };
        assert_ne!(k1, k2);
    }

    #[test]
    fn snapshot_on_frame_cursor_produces_frame_snapshot() {
        let state = make_state_with_frame(
            1,
            vec![VariableInfo {
                index: 0,
                value: VariableValue::Null,
            }],
        );
        let cursor = make_cursor_at(1);
        let item = snapshot_from_cursor(&cursor, &state, "main", thread_id()).unwrap();
        assert!(matches!(item.snapshot, PinnedSnapshot::Frame { .. }));
        assert_eq!(item.thread_name, "main");
        assert_eq!(item.key.thread_id, thread_id());
    }

    #[test]
    fn snapshot_on_var_null_produces_primitive() {
        let state = make_state_with_frame(
            1,
            vec![VariableInfo {
                index: 0,
                value: VariableValue::Null,
            }],
        );
        let cursor = make_cursor_var(1, 0);
        let item = snapshot_from_cursor(&cursor, &state, "main", thread_id()).unwrap();
        assert!(
            matches!(&item.snapshot, PinnedSnapshot::Primitive { value_label } if value_label == "null")
        );
    }

    #[test]
    fn snapshot_on_var_unexpanded_objectref_produces_unexpanded_ref() {
        let state = make_state_with_frame(
            1,
            vec![VariableInfo {
                index: 0,
                value: VariableValue::ObjectRef {
                    id: 99,
                    class_name: "java.util.ArrayList".to_string(),
                    entry_count: None,
                },
            }],
        );
        let cursor = make_cursor_var(1, 0);
        let item = snapshot_from_cursor(&cursor, &state, "main", thread_id()).unwrap();
        assert!(matches!(
            &item.snapshot,
            PinnedSnapshot::UnexpandedRef { object_id: 99, .. }
        ));
    }

    #[test]
    fn snapshot_on_var_expanded_objectref_produces_subtree() {
        let mut state = make_state_with_frame(
            1,
            vec![VariableInfo {
                index: 0,
                value: VariableValue::ObjectRef {
                    id: 42,
                    class_name: "Foo".to_string(),
                    entry_count: None,
                },
            }],
        );
        state.set_expansion_done(
            42,
            vec![FieldInfo {
                name: "x".to_string(),
                value: FieldValue::Int(1),
            }],
        );
        let cursor = make_cursor_var(1, 0);
        let item = snapshot_from_cursor(&cursor, &state, "main", thread_id()).unwrap();
        assert!(matches!(
            &item.snapshot,
            PinnedSnapshot::Subtree { root_id: 42, .. }
        ));
    }

    #[test]
    fn snapshot_on_object_field_resolves_field_and_builds_correct_key() {
        let mut state = make_state_with_frame(
            1,
            vec![VariableInfo {
                index: 0,
                value: VariableValue::ObjectRef {
                    id: 10,
                    class_name: "Outer".to_string(),
                    entry_count: None,
                },
            }],
        );
        state.set_expansion_done(
            10,
            vec![FieldInfo {
                name: "count".to_string(),
                value: FieldValue::Int(5),
            }],
        );
        let path = NavigationPathBuilder::new(FrameId(1), VarIdx(0))
            .field(FieldIdx(0))
            .build();
        let cursor = RenderCursor::At(path);
        let item = snapshot_from_cursor(&cursor, &state, "main", thread_id()).unwrap();
        assert!(
            matches!(&item.snapshot, PinnedSnapshot::Primitive { value_label } if value_label == "5")
        );
        assert!(
            item.item_label.contains("count"),
            "expected field name in label, got: {}",
            item.item_label
        );
    }

    #[test]
    fn snapshot_on_cyclic_node_returns_none() {
        let state = StackState::new(vec![make_frame(1)]);
        let path = NavigationPathBuilder::new(FrameId(1), VarIdx(0))
            .field(FieldIdx(0))
            .build();
        let cursor = RenderCursor::CyclicNode(path);
        assert!(snapshot_from_cursor(&cursor, &state, "main", thread_id()).is_none());
    }

    #[test]
    fn snapshot_on_loading_node_returns_none() {
        let state = StackState::new(vec![make_frame(1)]);
        let path = NavigationPathBuilder::new(FrameId(1), VarIdx(0)).build();
        let cursor = RenderCursor::LoadingNode(path);
        assert!(snapshot_from_cursor(&cursor, &state, "main", thread_id()).is_none());
    }

    #[test]
    fn snapshot_respects_object_limit_truncation() {
        let n = SNAPSHOT_OBJECT_LIMIT + 10;
        // Build a long chain: object 0 -> object 1 -> ... -> object n-1
        let mut state = make_state_with_frame(
            1,
            vec![VariableInfo {
                index: 0,
                value: VariableValue::ObjectRef {
                    id: 0,
                    class_name: "Node".to_string(),
                    entry_count: None,
                },
            }],
        );
        for i in 0..n {
            let next_id = (i + 1) as u64;
            state.set_expansion_done(
                i as u64,
                vec![FieldInfo {
                    name: "next".to_string(),
                    value: FieldValue::ObjectRef {
                        id: next_id,
                        class_name: "Node".to_string(),
                        entry_count: None,
                        inline_value: None,
                    },
                }],
            );
        }
        let cursor = make_cursor_at(1);
        let item = snapshot_from_cursor(&cursor, &state, "main", thread_id()).unwrap();
        match &item.snapshot {
            PinnedSnapshot::Frame {
                object_fields,
                truncated,
                ..
            } => {
                assert!(*truncated, "expected truncated snapshot");
                assert!(
                    object_fields.len() <= SNAPSHOT_OBJECT_LIMIT,
                    "snapshot must cap object_fields at {}, got {}",
                    SNAPSHOT_OBJECT_LIMIT,
                    object_fields.len()
                );
            }
            _ => panic!("expected frame snapshot"),
        }
    }

    #[test]
    fn snapshot_freezes_loading_chunks_to_collapsed() {
        use crate::views::stack_view::{ChunkState, CollectionChunks};

        let mut state = make_state_with_frame(
            1,
            vec![VariableInfo {
                index: 0,
                value: VariableValue::ObjectRef {
                    id: 10,
                    class_name: "Root".to_string(),
                    entry_count: None,
                },
            }],
        );
        state.set_expansion_done(
            10,
            vec![FieldInfo {
                name: "items".to_string(),
                value: FieldValue::ObjectRef {
                    id: 20,
                    class_name: "ArrayList".to_string(),
                    entry_count: Some(200),
                    inline_value: None,
                },
            }],
        );
        state.expansion.collection_chunks.insert(
            20,
            CollectionChunks {
                total_count: 200,
                eager_page: None,
                chunk_pages: {
                    let mut m = HashMap::new();
                    m.insert(100usize, ChunkState::Loading);
                    m
                },
            },
        );

        let cursor = make_cursor_var(1, 0);
        let item = snapshot_from_cursor(&cursor, &state, "main", thread_id()).unwrap();

        match item.snapshot {
            PinnedSnapshot::Subtree {
                collection_chunks, ..
            } => {
                let state = collection_chunks
                    .get(&20)
                    .and_then(|cc| cc.chunk_pages.get(&100));
                assert!(
                    matches!(state, Some(ChunkState::Collapsed)),
                    "loading chunk must be frozen to collapsed"
                );
            }
            _ => panic!("expected subtree snapshot"),
        }
    }
}
