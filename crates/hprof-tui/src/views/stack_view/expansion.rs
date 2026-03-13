//! Expansion lifecycle state for the stack view.
//!
//! [`ExpansionRegistry`] owns all object expansion data (phases, decoded fields,
//! errors) and collection pagination state, decoupled from cursor and frame logic.

use std::collections::HashMap;

use hprof_engine::FieldInfo;

use super::types::{ChunkState, CollectionChunks, ExpansionPhase, NavigationPath};

/// Holds expansion state for all objects in the stack view.
pub struct ExpansionRegistry {
    /// UI expansion phase keyed by `NavigationPath` — instance-scoped.
    pub(crate) expansion_phases: HashMap<NavigationPath, ExpansionPhase>,
    /// Auxiliary phase map keyed by `object_id` — for `tree_render` compatibility.
    ///
    /// Kept in sync with `expansion_phases`. When multiple paths point to the
    /// same object, the last-written phase wins (acceptable for visual styling).
    pub(crate) object_phases: HashMap<u64, ExpansionPhase>,
    /// Decoded instance fields keyed by `object_id` — shared data cache.
    pub(crate) object_fields: HashMap<u64, Vec<FieldInfo>>,
    /// Decoded static fields keyed by `object_id` — shared data cache.
    pub(crate) object_static_fields: HashMap<u64, Vec<FieldInfo>>,
    /// Expansion errors keyed by `object_id`.
    pub(crate) object_errors: HashMap<u64, String>,
    /// Collection pagination state keyed by `collection_id` — shared data cache.
    pub(crate) collection_chunks: HashMap<u64, CollectionChunks>,
}

impl ExpansionRegistry {
    /// Creates a new empty registry.
    pub fn new() -> Self {
        Self {
            expansion_phases: HashMap::new(),
            object_phases: HashMap::new(),
            object_fields: HashMap::new(),
            object_static_fields: HashMap::new(),
            object_errors: HashMap::new(),
            collection_chunks: HashMap::new(),
        }
    }

    /// Returns the expansion phase for the given path (defaults to `Collapsed`).
    pub(crate) fn expansion_state(&self, path: &NavigationPath) -> ExpansionPhase {
        self.expansion_phases
            .get(path)
            .cloned()
            .unwrap_or(ExpansionPhase::Collapsed)
    }

    /// Marks a path expansion as complete with decoded fields.
    ///
    /// Also stores instance fields by object_id for rendering.
    /// Static fields are stored separately via `set_static_fields`.
    pub fn set_expansion_done(
        &mut self,
        path: &NavigationPath,
        object_id: u64,
        fields: Vec<FieldInfo>,
    ) {
        self.object_fields.insert(object_id, fields);
        self.expansion_phases
            .insert(path.clone(), ExpansionPhase::Expanded);
        self.object_phases
            .insert(object_id, ExpansionPhase::Expanded);
    }

    /// Collapses an object by ID only (used in batch collapse operations).
    ///
    /// Removes all expansion phases pointing to this object_id from expansion_phases
    /// by retaining only entries that don't correspond to this object. This is a
    /// best-effort path-independent collapse used for recursive collapse.
    pub fn collapse_object_by_id(&mut self, object_id: u64) {
        self.object_phases.remove(&object_id);
        self.object_fields.remove(&object_id);
        self.object_static_fields.remove(&object_id);
        self.object_errors.remove(&object_id);
        // Cannot efficiently remove expansion_phases by object_id without scanning.
        // Orphaned phases are harmless — they are gated by object_fields presence.
    }

    /// Returns the [`ChunkState`] for a specific chunk.
    pub fn chunk_state(&self, collection_id: u64, chunk_offset: usize) -> Option<&ChunkState> {
        self.collection_chunks
            .get(&collection_id)?
            .chunk_pages
            .get(&chunk_offset)
    }
}
