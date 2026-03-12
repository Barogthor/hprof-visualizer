//! Favorites panel data model: pinning, snapshots, and snapshot construction.
//!
//! [`PinnedItem`] captures a frozen snapshot of a stack position at pin time.
//! [`snapshot_from_cursor`] builds a `PinnedItem` from the current `StackState`.

use std::collections::{HashMap, HashSet};

use hprof_engine::{FieldInfo, FieldValue, VariableInfo, VariableValue};

use crate::views::stack_view::{
    ChunkState, CollectionChunks, StackCursor, StackState, format_frame_label,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PinKey {
    /// A whole stack frame.
    Frame { frame_id: u64, thread_name: String },
    /// A single variable inside a frame.
    Var {
        frame_id: u64,
        thread_name: String,
        var_idx: usize,
    },
    /// A field deep inside an object tree.
    Field {
        frame_id: u64,
        thread_name: String,
        var_idx: usize,
        field_path: Vec<usize>,
    },
    /// One entry inside a collection/array.
    CollectionEntry {
        frame_id: u64,
        thread_name: String,
        var_idx: usize,
        field_path: Vec<usize>,
        collection_id: u64,
        entry_index: usize,
    },
    /// One field inside an object expanded from a collection entry.
    CollectionEntryField {
        frame_id: u64,
        thread_name: String,
        var_idx: usize,
        field_path: Vec<usize>,
        collection_id: u64,
        entry_index: usize,
        obj_field_path: Vec<usize>,
    },
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
    /// Structural key used for toggle detection and de-duplication.
    pub key: PinKey,
}

/// Walks the reachable object graph from `root_id`, cloning fields and
/// collection chunks into fresh maps.
///
/// `reachable` is shared across all vars of a frame so the global
/// `SNAPSHOT_OBJECT_LIMIT` applies across the entire frame snapshot.
///
/// Returns
/// `(object_fields, object_static_fields, collection_chunks, truncated)`.
pub(crate) fn subtree_snapshot(
    root_id: u64,
    state: &StackState,
    reachable: &mut HashSet<u64>,
) -> SubtreeSnapshot {
    let mut desc_order: Vec<u64> = Vec::new();
    let truncated = collect_descendants_limited(
        root_id,
        state.object_fields(),
        state.collection_chunks_map(),
        reachable,
        &mut desc_order,
        SNAPSHOT_OBJECT_LIMIT,
    ) || reachable.len() >= SNAPSHOT_OBJECT_LIMIT;

    let mut snap_fields: SnapshotObjectFields = HashMap::new();
    let mut snap_static_fields: SnapshotStaticFields = HashMap::new();
    let mut snap_chunks: SnapshotCollectionChunks = HashMap::new();

    for &id in &desc_order {
        if let Some(fields) = state.object_fields().get(&id) {
            snap_fields.insert(id, fields.clone());
        }
        if let Some(fields) = state.object_static_fields().get(&id) {
            snap_static_fields.insert(id, fields.clone());
        }
        if let Some(cc) = state.collection_chunks_map().get(&id) {
            snap_chunks.insert(id, freeze_collection_chunks(cc));
        }
    }
    (snap_fields, snap_static_fields, snap_chunks, truncated)
}

fn collect_descendants_limited(
    root_id: u64,
    fields: &HashMap<u64, Vec<FieldInfo>>,
    collection_chunks: &HashMap<u64, CollectionChunks>,
    visited: &mut HashSet<u64>,
    out: &mut Vec<u64>,
    limit: usize,
) -> bool {
    if visited.len() >= limit {
        return true;
    }
    if !visited.insert(root_id) {
        return false;
    }

    let mut truncated = false;
    if let Some(field_list) = fields.get(&root_id) {
        for f in field_list {
            if let FieldValue::ObjectRef { id, .. } = f.value {
                if collect_descendants_limited(id, fields, collection_chunks, visited, out, limit) {
                    truncated = true;
                    break;
                }
                if visited.len() >= limit {
                    truncated = true;
                    break;
                }
            }
        }
    }

    if !truncated
        && let Some(chunks) = collection_chunks.get(&root_id)
        && let Some(page) = &chunks.eager_page
    {
        for entry in &page.entries {
            if let FieldValue::ObjectRef { id, .. } = &entry.value {
                if collect_descendants_limited(*id, fields, collection_chunks, visited, out, limit)
                {
                    truncated = true;
                    break;
                }
                if visited.len() >= limit {
                    truncated = true;
                    break;
                }
            }
        }
    }

    if !truncated && let Some(chunks) = collection_chunks.get(&root_id) {
        for page_state in chunks.chunk_pages.values() {
            let ChunkState::Loaded(page) = page_state else {
                continue;
            };
            for entry in &page.entries {
                if let FieldValue::ObjectRef { id, .. } = &entry.value {
                    if collect_descendants_limited(
                        *id,
                        fields,
                        collection_chunks,
                        visited,
                        out,
                        limit,
                    ) {
                        truncated = true;
                        break;
                    }
                    if visited.len() >= limit {
                        truncated = true;
                        break;
                    }
                }
            }
            if truncated {
                break;
            }
        }
    }

    out.push(root_id);
    truncated
}

fn freeze_collection_chunks(chunks: &CollectionChunks) -> CollectionChunks {
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

/// Builds a [`PinnedItem`] from the current cursor position, or `None` if the
/// cursor position cannot be pinned (e.g. loading/cyclic pseudo-nodes).
pub fn snapshot_from_cursor(
    cursor: &StackCursor,
    state: &StackState,
    thread_name: &str,
) -> Option<PinnedItem> {
    PinnedItemFactory::new(state, thread_name).build_from_cursor(cursor)
}

struct FrameCtx {
    frame_id: u64,
    frame_label: String,
}

struct PinnedItemFactory<'a> {
    state: &'a StackState,
    thread_name: &'a str,
}

impl<'a> PinnedItemFactory<'a> {
    fn new(state: &'a StackState, thread_name: &'a str) -> Self {
        Self { state, thread_name }
    }

    fn build_from_cursor(&self, cursor: &StackCursor) -> Option<PinnedItem> {
        match cursor {
            StackCursor::OnFrame(frame_idx) => self.snapshot_on_frame(*frame_idx),
            StackCursor::OnVar { frame_idx, var_idx } => self.snapshot_on_var(*frame_idx, *var_idx),
            StackCursor::OnObjectField {
                frame_idx,
                var_idx,
                field_path,
            } => self.snapshot_on_object_field(*frame_idx, *var_idx, field_path),
            StackCursor::OnCollectionEntry {
                frame_idx,
                var_idx,
                field_path,
                collection_id,
                entry_index,
            } => self.snapshot_on_collection_entry(
                *frame_idx,
                *var_idx,
                field_path,
                *collection_id,
                *entry_index,
            ),
            StackCursor::OnCollectionEntryObjField {
                frame_idx,
                var_idx,
                field_path,
                collection_id,
                entry_index,
                obj_field_path,
            } => self.snapshot_on_collection_entry_field(
                *frame_idx,
                *var_idx,
                field_path,
                *collection_id,
                *entry_index,
                obj_field_path,
            ),

            // Not pinnable.
            StackCursor::NoFrames
            | StackCursor::OnObjectLoadingNode { .. }
            | StackCursor::OnCyclicNode { .. }
            | StackCursor::OnStaticSectionHeader { .. }
            | StackCursor::OnStaticField { .. }
            | StackCursor::OnStaticOverflowRow { .. }
            | StackCursor::OnStaticObjectField { .. }
            | StackCursor::OnCollectionEntryStaticSectionHeader { .. }
            | StackCursor::OnCollectionEntryStaticField { .. }
            | StackCursor::OnCollectionEntryStaticOverflowRow { .. }
            | StackCursor::OnCollectionEntryStaticObjectField { .. }
            | StackCursor::OnChunkSection { .. } => None,
        }
    }

    fn snapshot_on_frame(&self, frame_idx: usize) -> Option<PinnedItem> {
        let ctx = self.frame_context(frame_idx)?;
        let vars = self
            .state
            .vars()
            .get(&ctx.frame_id)
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
                    subtree_snapshot(id, self.state, &mut reachable);
                all_fields.extend(fields);
                all_static_fields.extend(static_fields);
                all_chunks.extend(chunks);
                if trunc {
                    any_truncated = true;
                }
            }
        }

        Some(self.make_pinned_item(
            ctx.frame_label.clone(),
            ctx.frame_label,
            PinnedSnapshot::Frame {
                variables: vars,
                object_fields: all_fields,
                object_static_fields: all_static_fields,
                collection_chunks: all_chunks,
                truncated: any_truncated,
            },
            PinKey::Frame {
                frame_id: ctx.frame_id,
                thread_name: self.thread_name.to_string(),
            },
        ))
    }

    fn snapshot_on_var(&self, frame_idx: usize, var_idx: usize) -> Option<PinnedItem> {
        let ctx = self.frame_context(frame_idx)?;
        let var = self.state.vars().get(&ctx.frame_id)?.get(var_idx)?;
        let item_label = format!("var[{var_idx}]");
        let snapshot = self.snapshot_for_variable_value(&var.value);

        Some(self.make_pinned_item(
            ctx.frame_label,
            item_label,
            snapshot,
            PinKey::Var {
                frame_id: ctx.frame_id,
                thread_name: self.thread_name.to_string(),
                var_idx,
            },
        ))
    }

    fn snapshot_on_object_field(
        &self,
        frame_idx: usize,
        var_idx: usize,
        field_path: &[usize],
    ) -> Option<PinnedItem> {
        let ctx = self.frame_context(frame_idx)?;
        let var = self.state.vars().get(&ctx.frame_id)?.get(var_idx)?;
        let VariableValue::ObjectRef { id: root_id, .. } = var.value else {
            return None;
        };

        let leaf_field = self.resolve_leaf_object_field(root_id, field_path)?;
        let item_label = build_field_path_label(self.state, root_id, var_idx, field_path);
        let snapshot = self.snapshot_for_field_value(&leaf_field.value);

        Some(self.make_pinned_item(
            ctx.frame_label,
            item_label,
            snapshot,
            PinKey::Field {
                frame_id: ctx.frame_id,
                thread_name: self.thread_name.to_string(),
                var_idx,
                field_path: field_path.to_vec(),
            },
        ))
    }

    fn snapshot_on_collection_entry(
        &self,
        frame_idx: usize,
        var_idx: usize,
        field_path: &[usize],
        collection_id: u64,
        entry_index: usize,
    ) -> Option<PinnedItem> {
        let ctx = self.frame_context(frame_idx)?;
        let cc = self.state.collection_chunks_map().get(&collection_id)?;
        let entry = cc.find_entry(entry_index)?;
        let item_label = build_collection_entry_label(
            self.state,
            ctx.frame_id,
            var_idx,
            field_path,
            entry_index,
        );
        let snapshot = self.snapshot_for_field_value(&entry.value);

        Some(self.make_pinned_item(
            ctx.frame_label,
            item_label,
            snapshot,
            PinKey::CollectionEntry {
                frame_id: ctx.frame_id,
                thread_name: self.thread_name.to_string(),
                var_idx,
                field_path: field_path.to_vec(),
                collection_id,
                entry_index,
            },
        ))
    }

    fn snapshot_on_collection_entry_field(
        &self,
        frame_idx: usize,
        var_idx: usize,
        field_path: &[usize],
        collection_id: u64,
        entry_index: usize,
        obj_field_path: &[usize],
    ) -> Option<PinnedItem> {
        if obj_field_path.is_empty() {
            return None;
        }

        let ctx = self.frame_context(frame_idx)?;
        let cc = self.state.collection_chunks_map().get(&collection_id)?;
        let entry = cc.find_entry(entry_index)?;
        let FieldValue::ObjectRef { id: root_id, .. } = &entry.value else {
            return None;
        };

        let leaf_field = self.resolve_leaf_object_field(*root_id, obj_field_path)?;
        let item_label = build_collection_entry_obj_field_label(
            self.state,
            ctx.frame_id,
            var_idx,
            field_path,
            entry_index,
            *root_id,
            obj_field_path,
        );
        let snapshot = self.snapshot_for_field_value(&leaf_field.value);

        Some(self.make_pinned_item(
            ctx.frame_label,
            item_label,
            snapshot,
            PinKey::CollectionEntryField {
                frame_id: ctx.frame_id,
                thread_name: self.thread_name.to_string(),
                var_idx,
                field_path: field_path.to_vec(),
                collection_id,
                entry_index,
                obj_field_path: obj_field_path.to_vec(),
            },
        ))
    }

    fn frame_context(&self, frame_idx: usize) -> Option<FrameCtx> {
        let frame = self.state.frames().get(frame_idx)?;
        Some(FrameCtx {
            frame_id: frame.frame_id,
            frame_label: format_frame_label(frame),
        })
    }

    fn resolve_leaf_object_field(&self, root_id: u64, path: &[usize]) -> Option<&FieldInfo> {
        let mut current_id = root_id;
        for &field_idx in &path[..path.len().saturating_sub(1)] {
            let fields = self.state.object_fields().get(&current_id)?;
            match fields.get(field_idx)?.value {
                FieldValue::ObjectRef { id, .. } => current_id = id,
                _ => return None,
            }
        }
        let leaf_field_idx = *path.last()?;
        let leaf_fields = self.state.object_fields().get(&current_id)?;
        leaf_fields.get(leaf_field_idx)
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
                subtree_snapshot(object_id, self.state, &mut reachable);
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
        key: PinKey,
    ) -> PinnedItem {
        PinnedItem {
            thread_name: self.thread_name.to_string(),
            frame_label,
            item_label,
            snapshot,
            local_collapsed: HashSet::new(),
            key,
        }
    }
}

/// Builds a human-readable label for a field path, e.g. `"var[0].cache.size"`.
fn build_field_path_label(
    state: &StackState,
    root_id: u64,
    var_idx: usize,
    field_path: &[usize],
) -> String {
    let mut parts = vec![format!("var[{var_idx}]")];
    let mut current_id = root_id;
    for &idx in field_path {
        let name = state
            .object_fields()
            .get(&current_id)
            .and_then(|fields| fields.get(idx))
            .map(|f| f.name.clone())
            .unwrap_or_else(|| format!("field[{idx}]"));

        // Advance current_id for next level.
        if let Some(next_id) = state
            .object_fields()
            .get(&current_id)
            .and_then(|fields| fields.get(idx))
            .and_then(|f| {
                if let FieldValue::ObjectRef { id, .. } = f.value {
                    Some(id)
                } else {
                    None
                }
            })
        {
            current_id = next_id;
        }
        parts.push(name);
    }
    parts.join(".")
}

fn build_collection_entry_label(
    state: &StackState,
    frame_id: u64,
    var_idx: usize,
    field_path: &[usize],
    entry_index: usize,
) -> String {
    let base = if field_path.is_empty() {
        format!("var[{var_idx}]")
    } else {
        let root_id = state
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
        if let Some(root_id) = root_id {
            build_field_path_label(state, root_id, var_idx, field_path)
        } else {
            format!("var[{var_idx}]")
        }
    };
    format!("{base}[{entry_index}]")
}

fn build_collection_entry_obj_field_label(
    state: &StackState,
    frame_id: u64,
    var_idx: usize,
    field_path: &[usize],
    entry_index: usize,
    root_id: u64,
    obj_field_path: &[usize],
) -> String {
    let mut label = build_collection_entry_label(state, frame_id, var_idx, field_path, entry_index);
    let mut current_id = root_id;
    for &idx in obj_field_path {
        let field_name = state
            .object_fields()
            .get(&current_id)
            .and_then(|fields| fields.get(idx))
            .map(|f| {
                if let FieldValue::ObjectRef { id, .. } = f.value {
                    current_id = id;
                }
                f.name.clone()
            })
            .unwrap_or_else(|| format!("#{idx}"));
        label.push('.');
        label.push_str(&field_name);
    }
    label
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
    use hprof_engine::{
        CollectionPage, EntryInfo, FieldInfo, FieldValue, FrameInfo, LineNumber, VariableInfo,
        VariableValue,
    };

    use super::*;
    use crate::views::stack_view::StackState;

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

    #[test]
    fn pin_key_same_frame_id_different_thread_name_are_not_equal() {
        let k1 = PinKey::Frame {
            frame_id: 42,
            thread_name: "Thread-1".to_string(),
        };
        let k2 = PinKey::Frame {
            frame_id: 42,
            thread_name: "Thread-2".to_string(),
        };
        assert_ne!(k1, k2);
    }

    #[test]
    fn pin_key_same_values_are_equal() {
        let k1 = PinKey::Var {
            frame_id: 10,
            thread_name: "main".to_string(),
            var_idx: 2,
        };
        let k2 = PinKey::Var {
            frame_id: 10,
            thread_name: "main".to_string(),
            var_idx: 2,
        };
        assert_eq!(k1, k2);
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
        let cursor = StackCursor::OnFrame(0);
        let item = snapshot_from_cursor(&cursor, &state, "main").unwrap();
        assert!(matches!(item.snapshot, PinnedSnapshot::Frame { .. }));
        assert_eq!(item.thread_name, "main");
        assert!(matches!(item.key, PinKey::Frame { frame_id: 1, .. }));
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
        let cursor = StackCursor::OnVar {
            frame_idx: 0,
            var_idx: 0,
        };
        let item = snapshot_from_cursor(&cursor, &state, "main").unwrap();
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
        let cursor = StackCursor::OnVar {
            frame_idx: 0,
            var_idx: 0,
        };
        let item = snapshot_from_cursor(&cursor, &state, "main").unwrap();
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
        let cursor = StackCursor::OnVar {
            frame_idx: 0,
            var_idx: 0,
        };
        let item = snapshot_from_cursor(&cursor, &state, "main").unwrap();
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
        let cursor = StackCursor::OnObjectField {
            frame_idx: 0,
            var_idx: 0,
            field_path: vec![0],
        };
        let item = snapshot_from_cursor(&cursor, &state, "main").unwrap();
        assert!(
            matches!(&item.snapshot, PinnedSnapshot::Primitive { value_label } if value_label == "5")
        );
        assert!(matches!(
            &item.key,
            PinKey::Field {
                var_idx: 0,
                field_path,
                ..
            }
            if field_path == &[0usize]
        ));
        assert!(
            item.item_label.contains("count"),
            "expected field name in label, got: {}",
            item.item_label
        );
    }

    #[test]
    fn snapshot_on_cyclic_node_returns_none() {
        let state = StackState::new(vec![make_frame(1)]);
        let cursor = StackCursor::OnCyclicNode {
            frame_idx: 0,
            var_idx: 0,
            field_path: vec![0],
        };
        assert!(snapshot_from_cursor(&cursor, &state, "main").is_none());
    }

    #[test]
    fn snapshot_on_loading_node_returns_none() {
        let state = StackState::new(vec![make_frame(1)]);
        let cursor = StackCursor::OnObjectLoadingNode {
            frame_idx: 0,
            var_idx: 0,
            field_path: vec![],
        };
        assert!(snapshot_from_cursor(&cursor, &state, "main").is_none());
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
        let cursor = StackCursor::OnFrame(0);
        let item = snapshot_from_cursor(&cursor, &state, "main").unwrap();
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

        let cursor = StackCursor::OnVar {
            frame_idx: 0,
            var_idx: 0,
        };
        let item = snapshot_from_cursor(&cursor, &state, "main").unwrap();

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

    #[test]
    fn snapshot_on_var_with_collection_chunks_only_produces_subtree() {
        use crate::views::stack_view::CollectionChunks;

        let mut state = make_state_with_frame(
            1,
            vec![VariableInfo {
                index: 0,
                value: VariableValue::ObjectRef {
                    id: 20,
                    class_name: "ArrayList".to_string(),
                    entry_count: Some(120),
                },
            }],
        );
        state.expansion.collection_chunks.insert(
            20,
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
                chunk_pages: HashMap::new(),
            },
        );

        let cursor = StackCursor::OnVar {
            frame_idx: 0,
            var_idx: 0,
        };
        let item = snapshot_from_cursor(&cursor, &state, "main").unwrap();

        match item.snapshot {
            PinnedSnapshot::Subtree {
                root_id,
                object_fields,
                collection_chunks,
                ..
            } => {
                assert_eq!(root_id, 20);
                assert!(
                    object_fields.is_empty(),
                    "collection-only snapshot should not fabricate object fields"
                );
                assert!(
                    collection_chunks.contains_key(&20),
                    "collection chunks should be captured when available"
                );
            }
            _ => panic!("expected subtree snapshot for collection-only root"),
        }
    }

    #[test]
    fn snapshot_on_var_collection_entry_expanded_object_is_captured() {
        use crate::views::stack_view::CollectionChunks;

        let mut state = make_state_with_frame(
            1,
            vec![VariableInfo {
                index: 0,
                value: VariableValue::ObjectRef {
                    id: 20,
                    class_name: "ArrayList".to_string(),
                    entry_count: Some(2),
                },
            }],
        );
        state.expansion.collection_chunks.insert(
            20,
            CollectionChunks {
                total_count: 2,
                eager_page: Some(CollectionPage {
                    entries: vec![EntryInfo {
                        index: 0,
                        key: None,
                        value: FieldValue::ObjectRef {
                            id: 30,
                            class_name: "Node".to_string(),
                            entry_count: None,
                            inline_value: None,
                        },
                    }],
                    total_count: 2,
                    offset: 0,
                    has_more: false,
                }),
                chunk_pages: HashMap::new(),
            },
        );
        state.set_expansion_done(
            30,
            vec![FieldInfo {
                name: "value".to_string(),
                value: FieldValue::Int(7),
            }],
        );

        let cursor = StackCursor::OnVar {
            frame_idx: 0,
            var_idx: 0,
        };
        let item = snapshot_from_cursor(&cursor, &state, "main").unwrap();

        match item.snapshot {
            PinnedSnapshot::Subtree { object_fields, .. } => {
                assert!(
                    object_fields.contains_key(&30),
                    "expanded collection entry object must be captured into snapshot"
                );
            }
            _ => panic!("expected subtree snapshot"),
        }
    }

    #[test]
    fn snapshot_on_collection_entry_primitive_produces_primitive_and_key() {
        use crate::views::stack_view::CollectionChunks;

        let mut state = make_state_with_frame(
            1,
            vec![VariableInfo {
                index: 0,
                value: VariableValue::ObjectRef {
                    id: 20,
                    class_name: "ArrayList".to_string(),
                    entry_count: Some(2),
                },
            }],
        );
        state.expansion.collection_chunks.insert(
            20,
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

        let cursor = StackCursor::OnCollectionEntry {
            frame_idx: 0,
            var_idx: 0,
            field_path: vec![],
            collection_id: 20,
            entry_index: 0,
        };
        let item = snapshot_from_cursor(&cursor, &state, "main").unwrap();

        assert!(
            matches!(&item.snapshot, PinnedSnapshot::Primitive { value_label } if value_label == "7")
        );
        assert!(matches!(
            item.key,
            PinKey::CollectionEntry {
                var_idx: 0,
                collection_id: 20,
                entry_index: 0,
                ..
            }
        ));
        assert!(item.item_label.contains("var[0][0]"));
    }

    #[test]
    fn snapshot_on_collection_entry_objectref_produces_subtree() {
        use crate::views::stack_view::CollectionChunks;

        let mut state = make_state_with_frame(
            1,
            vec![VariableInfo {
                index: 0,
                value: VariableValue::ObjectRef {
                    id: 20,
                    class_name: "ArrayList".to_string(),
                    entry_count: Some(2),
                },
            }],
        );
        state.expansion.collection_chunks.insert(
            20,
            CollectionChunks {
                total_count: 2,
                eager_page: Some(CollectionPage {
                    entries: vec![EntryInfo {
                        index: 0,
                        key: None,
                        value: FieldValue::ObjectRef {
                            id: 30,
                            class_name: "Node".to_string(),
                            entry_count: None,
                            inline_value: None,
                        },
                    }],
                    total_count: 2,
                    offset: 0,
                    has_more: false,
                }),
                chunk_pages: HashMap::new(),
            },
        );
        state.set_expansion_done(
            30,
            vec![FieldInfo {
                name: "value".to_string(),
                value: FieldValue::Int(9),
            }],
        );

        let cursor = StackCursor::OnCollectionEntry {
            frame_idx: 0,
            var_idx: 0,
            field_path: vec![],
            collection_id: 20,
            entry_index: 0,
        };
        let item = snapshot_from_cursor(&cursor, &state, "main").unwrap();

        assert!(matches!(
            item.snapshot,
            PinnedSnapshot::Subtree { root_id: 30, .. }
        ));
    }

    #[test]
    fn snapshot_on_collection_entry_obj_field_produces_field_pin() {
        use crate::views::stack_view::CollectionChunks;

        let mut state = make_state_with_frame(
            1,
            vec![VariableInfo {
                index: 0,
                value: VariableValue::ObjectRef {
                    id: 20,
                    class_name: "ArrayList".to_string(),
                    entry_count: Some(2),
                },
            }],
        );
        state.expansion.collection_chunks.insert(
            20,
            CollectionChunks {
                total_count: 2,
                eager_page: Some(CollectionPage {
                    entries: vec![EntryInfo {
                        index: 0,
                        key: None,
                        value: FieldValue::ObjectRef {
                            id: 30,
                            class_name: "Node".to_string(),
                            entry_count: None,
                            inline_value: None,
                        },
                    }],
                    total_count: 2,
                    offset: 0,
                    has_more: false,
                }),
                chunk_pages: HashMap::new(),
            },
        );
        state.set_expansion_done(
            30,
            vec![FieldInfo {
                name: "value".to_string(),
                value: FieldValue::Int(11),
            }],
        );

        let cursor = StackCursor::OnCollectionEntryObjField {
            frame_idx: 0,
            var_idx: 0,
            field_path: vec![],
            collection_id: 20,
            entry_index: 0,
            obj_field_path: vec![0],
        };
        let item = snapshot_from_cursor(&cursor, &state, "main").unwrap();

        assert!(
            matches!(&item.snapshot, PinnedSnapshot::Primitive { value_label } if value_label == "11")
        );
        assert!(matches!(
            item.key,
            PinKey::CollectionEntryField {
                collection_id: 20,
                entry_index: 0,
                ..
            }
        ));
        assert!(item.item_label.contains("value"));
    }

    #[test]
    fn snapshot_on_collection_entry_obj_field_empty_path_returns_none() {
        let state = StackState::new(vec![make_frame(1)]);
        let cursor = StackCursor::OnCollectionEntryObjField {
            frame_idx: 0,
            var_idx: 0,
            field_path: vec![],
            collection_id: 1,
            entry_index: 0,
            obj_field_path: vec![],
        };

        assert!(snapshot_from_cursor(&cursor, &state, "main").is_none());
    }

    #[test]
    fn snapshot_captures_static_fields_for_expanded_object() {
        let mut state = make_state_with_frame(
            1,
            vec![VariableInfo {
                index: 0,
                value: VariableValue::ObjectRef {
                    id: 42,
                    class_name: "Node".to_string(),
                    entry_count: None,
                },
            }],
        );
        state.set_expansion_done(
            42,
            vec![FieldInfo {
                name: "value".to_string(),
                value: FieldValue::Int(1),
            }],
        );
        state.set_static_fields(
            42,
            vec![FieldInfo {
                name: "SOME_STATIC".to_string(),
                value: FieldValue::Int(99),
            }],
        );

        let cursor = StackCursor::OnVar {
            frame_idx: 0,
            var_idx: 0,
        };
        let item = snapshot_from_cursor(&cursor, &state, "main").unwrap();

        match item.snapshot {
            PinnedSnapshot::Subtree {
                object_static_fields,
                ..
            } => {
                let Some(fields) = object_static_fields.get(&42) else {
                    panic!("expected static fields for object 42 in snapshot");
                };
                assert_eq!(fields.len(), 1);
                assert_eq!(fields[0].name, "SOME_STATIC");
            }
            _ => panic!("expected subtree snapshot"),
        }
    }
}
