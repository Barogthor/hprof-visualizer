//! Expansion lifecycle state for the stack view.
//!
//! [`ExpansionRegistry`] owns all object expansion data (phases, decoded fields,
//! errors) and collection pagination state, decoupled from cursor and frame logic.

use std::collections::HashMap;

use hprof_engine::FieldInfo;

use super::types::{ChunkState, CollectionChunks, ExpansionPhase, StackCursor};

/// Holds expansion state for all objects in the stack view.
pub struct ExpansionRegistry {
    pub(crate) object_phases: HashMap<u64, ExpansionPhase>,
    pub(crate) object_fields: HashMap<u64, Vec<FieldInfo>>,
    pub(crate) object_errors: HashMap<u64, String>,
    pub(crate) collection_chunks: HashMap<u64, CollectionChunks>,
    pub(crate) collection_restore_cursors: HashMap<u64, StackCursor>,
}

impl ExpansionRegistry {
    /// Creates a new empty registry.
    pub fn new() -> Self {
        Self {
            object_phases: HashMap::new(),
            object_fields: HashMap::new(),
            object_errors: HashMap::new(),
            collection_chunks: HashMap::new(),
            collection_restore_cursors: HashMap::new(),
        }
    }

    /// Returns the expansion phase for `object_id` (defaults to `Collapsed`).
    pub(crate) fn expansion_state(&self, object_id: u64) -> ExpansionPhase {
        self.object_phases
            .get(&object_id)
            .cloned()
            .unwrap_or(ExpansionPhase::Collapsed)
    }

    /// Marks an object as loading.
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

    /// Marks an object expansion as failed — mutation of phases/errors only.
    ///
    /// Cursor recovery and `sync_list_state` are handled by
    /// `StackState::set_expansion_failed`.
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

    /// Returns the [`ChunkState`] for a specific chunk.
    pub fn chunk_state(&self, collection_id: u64, chunk_offset: usize) -> Option<&ChunkState> {
        self.collection_chunks
            .get(&collection_id)?
            .chunk_pages
            .get(&chunk_offset)
    }
}
