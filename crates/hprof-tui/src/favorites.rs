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
}

/// Frozen content captured at pin time.
pub enum PinnedSnapshot {
    /// Whole frame: all variables and their expanded object/collection trees.
    Frame {
        variables: Vec<VariableInfo>,
        object_fields: HashMap<u64, Vec<FieldInfo>>,
        collection_chunks: HashMap<u64, CollectionChunks>,
        truncated: bool,
    },
    /// A single expanded `ObjectRef` variable or field plus its subtree.
    Subtree {
        root_id: u64,
        object_fields: HashMap<u64, Vec<FieldInfo>>,
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
    /// Structural key used for toggle detection and de-duplication.
    pub key: PinKey,
}

/// Walks the reachable object graph from `root_id`, cloning fields and
/// collection chunks into fresh maps.
///
/// `reachable` is shared across all vars of a frame so the global
/// `SNAPSHOT_OBJECT_LIMIT` applies across the entire frame snapshot.
///
/// Returns `(object_fields, collection_chunks, truncated)`.
pub(crate) fn subtree_snapshot(
    root_id: u64,
    state: &StackState,
    reachable: &mut HashSet<u64>,
) -> (
    HashMap<u64, Vec<FieldInfo>>,
    HashMap<u64, CollectionChunks>,
    bool,
) {
    let mut desc_order: Vec<u64> = Vec::new();
    let truncated = collect_descendants_limited(
        root_id,
        state.object_fields(),
        reachable,
        &mut desc_order,
        SNAPSHOT_OBJECT_LIMIT,
    ) || reachable.len() >= SNAPSHOT_OBJECT_LIMIT;

    let mut snap_fields: HashMap<u64, Vec<FieldInfo>> = HashMap::new();
    let mut snap_chunks: HashMap<u64, CollectionChunks> = HashMap::new();

    for &id in &desc_order {
        if let Some(fields) = state.object_fields().get(&id) {
            snap_fields.insert(id, fields.clone());
        }
        if let Some(cc) = state.collection_chunks_map().get(&id) {
            snap_chunks.insert(id, freeze_collection_chunks(cc));
        }
    }
    (snap_fields, snap_chunks, truncated)
}

fn collect_descendants_limited(
    root_id: u64,
    fields: &HashMap<u64, Vec<FieldInfo>>,
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
                if collect_descendants_limited(id, fields, visited, out, limit) {
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
/// cursor position cannot be pinned (e.g. `OnCollectionEntry`, loading nodes).
pub fn snapshot_from_cursor(
    cursor: &StackCursor,
    state: &StackState,
    thread_name: &str,
) -> Option<PinnedItem> {
    match cursor {
        StackCursor::OnFrame(frame_idx) => {
            let frame = state.frames().get(*frame_idx)?;
            let frame_id = frame.frame_id;
            let frame_label = format_frame_label(frame);

            let vars = state.vars().get(&frame_id).cloned().unwrap_or_default();

            let mut reachable = HashSet::new();
            let mut all_fields: HashMap<u64, Vec<FieldInfo>> = HashMap::new();
            let mut all_chunks: HashMap<u64, CollectionChunks> = HashMap::new();
            let mut any_truncated = false;

            for var in &vars {
                if reachable.len() >= SNAPSHOT_OBJECT_LIMIT {
                    any_truncated = true;
                    break;
                }
                if let VariableValue::ObjectRef { id, .. } = var.value {
                    let (fields, chunks, trunc) = subtree_snapshot(id, state, &mut reachable);
                    all_fields.extend(fields);
                    all_chunks.extend(chunks);
                    if trunc {
                        any_truncated = true;
                    }
                }
            }

            Some(PinnedItem {
                thread_name: thread_name.to_string(),
                frame_label: frame_label.clone(),
                item_label: frame_label,
                snapshot: PinnedSnapshot::Frame {
                    variables: vars,
                    object_fields: all_fields,
                    collection_chunks: all_chunks,
                    truncated: any_truncated,
                },
                key: PinKey::Frame {
                    frame_id,
                    thread_name: thread_name.to_string(),
                },
            })
        }

        StackCursor::OnVar { frame_idx, var_idx } => {
            let frame = state.frames().get(*frame_idx)?;
            let frame_id = frame.frame_id;
            let frame_label = format_frame_label(frame);
            let var = state.vars().get(&frame_id)?.get(*var_idx)?;
            let item_label = format!("var[{var_idx}]");

            let snapshot = match &var.value {
                VariableValue::ObjectRef { id, class_name, .. } => {
                    if state.object_fields().contains_key(id) {
                        let mut reachable = HashSet::new();
                        let (fields, chunks, truncated) =
                            subtree_snapshot(*id, state, &mut reachable);
                        PinnedSnapshot::Subtree {
                            root_id: *id,
                            object_fields: fields,
                            collection_chunks: chunks,
                            truncated,
                        }
                    } else {
                        PinnedSnapshot::UnexpandedRef {
                            class_name: class_name.clone(),
                            object_id: *id,
                        }
                    }
                }
                VariableValue::Null => PinnedSnapshot::Primitive {
                    value_label: "null".to_string(),
                },
            };

            Some(PinnedItem {
                thread_name: thread_name.to_string(),
                frame_label,
                item_label,
                snapshot,
                key: PinKey::Var {
                    frame_id,
                    thread_name: thread_name.to_string(),
                    var_idx: *var_idx,
                },
            })
        }

        StackCursor::OnObjectField {
            frame_idx,
            var_idx,
            field_path,
        } => {
            let frame = state.frames().get(*frame_idx)?;
            let frame_id = frame.frame_id;
            let frame_label = format_frame_label(frame);
            let var = state.vars().get(&frame_id)?.get(*var_idx)?;

            let root_id = match var.value {
                VariableValue::ObjectRef { id, .. } => id,
                _ => return None,
            };

            // Walk field_path to find the leaf field.
            let mut current_id = root_id;
            for &field_idx in &field_path[..field_path.len().saturating_sub(1)] {
                let fields = state.object_fields().get(&current_id)?;
                match fields.get(field_idx)?.value {
                    FieldValue::ObjectRef { id, .. } => current_id = id,
                    _ => return None,
                }
            }
            let leaf_field_idx = *field_path.last()?;
            let leaf_fields = state.object_fields().get(&current_id)?;
            let leaf_field = leaf_fields.get(leaf_field_idx)?;

            // Build item_label from field_path by resolving names.
            let item_label = build_field_path_label(state, root_id, *var_idx, field_path);

            let snapshot = match &leaf_field.value {
                FieldValue::ObjectRef { id, class_name, .. } => {
                    if state.object_fields().contains_key(id) {
                        let mut reachable = HashSet::new();
                        let (fields, chunks, truncated) =
                            subtree_snapshot(*id, state, &mut reachable);
                        PinnedSnapshot::Subtree {
                            root_id: *id,
                            object_fields: fields,
                            collection_chunks: chunks,
                            truncated,
                        }
                    } else {
                        PinnedSnapshot::UnexpandedRef {
                            class_name: class_name.clone(),
                            object_id: *id,
                        }
                    }
                }
                FieldValue::Null => PinnedSnapshot::Primitive {
                    value_label: "null".to_string(),
                },
                other => PinnedSnapshot::Primitive {
                    value_label: format_primitive_field_value(other),
                },
            };

            Some(PinnedItem {
                thread_name: thread_name.to_string(),
                frame_label,
                item_label,
                snapshot,
                key: PinKey::Field {
                    frame_id,
                    thread_name: thread_name.to_string(),
                    var_idx: *var_idx,
                    field_path: field_path.clone(),
                },
            })
        }

        // Not supported in 7.1 — silently ignored.
        StackCursor::OnCollectionEntry { .. } | StackCursor::OnCollectionEntryObjField { .. } => {
            None
        }

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
}
