//! Expansion lifecycle state for the stack view.
//!
//! [`ExpansionRegistry`] owns all object expansion data (phases,
//! decoded fields, errors) and collection pagination state,
//! decoupled from cursor and frame logic.

use std::collections::HashMap;

use hprof_engine::FieldInfo;

use super::types::{ChunkState, CollectionChunks, ExpansionPhase, NavigationPath};

/// Holds expansion state for all objects in the stack view.
pub struct ExpansionRegistry {
    /// UI expansion phase keyed by `NavigationPath`.
    pub(crate) expansion_phases: HashMap<NavigationPath, ExpansionPhase>,
    /// Decoded instance fields keyed by `object_id` — shared
    /// data cache.
    pub(crate) object_fields: HashMap<u64, Vec<FieldInfo>>,
    /// Decoded static fields keyed by `object_id` — shared
    /// data cache.
    pub(crate) object_static_fields: HashMap<u64, Vec<FieldInfo>>,
    /// Expansion errors keyed by `object_id`.
    pub(crate) object_errors: HashMap<u64, String>,
    /// Collection pagination state keyed by `collection_id`
    /// — shared data cache.
    pub(crate) collection_chunks: HashMap<u64, CollectionChunks>,
}

impl ExpansionRegistry {
    /// Creates a new empty registry.
    pub fn new() -> Self {
        Self {
            expansion_phases: HashMap::new(),
            object_fields: HashMap::new(),
            object_static_fields: HashMap::new(),
            object_errors: HashMap::new(),
            collection_chunks: HashMap::new(),
        }
    }

    /// Returns the expansion phase for the given path
    /// (defaults to `Collapsed`).
    pub(crate) fn expansion_state(&self, path: &NavigationPath) -> ExpansionPhase {
        self.expansion_phases
            .get(path)
            .cloned()
            .unwrap_or(ExpansionPhase::Collapsed)
    }

    /// Marks a path expansion as complete with decoded fields.
    ///
    /// Also stores instance fields by `object_id` for rendering.
    /// Static fields are stored separately via
    /// `set_static_fields`.
    pub fn set_expansion_done(
        &mut self,
        path: &NavigationPath,
        object_id: u64,
        fields: Vec<FieldInfo>,
    ) {
        self.object_fields.insert(object_id, fields);
        self.expansion_phases
            .insert(path.clone(), ExpansionPhase::Expanded);
    }

    /// Collapses at a path and all descendant paths.
    ///
    /// Removes the target path and every path whose segments
    /// start with the target's segments from `expansion_phases`.
    /// Data cache cleanup is deferred to LRU eviction.
    pub fn collapse_at_path(&mut self, path: &NavigationPath) {
        let target_segs = path.segments();
        let target_len = target_segs.len();
        self.expansion_phases.retain(|p, _| {
            let segs = p.segments();
            if segs.len() < target_len {
                return true;
            }
            segs[..target_len] != *target_segs
        });
    }

    /// Clears data caches for a given `object_id`.
    ///
    /// Removes instance fields, static fields, and errors.
    /// Does not touch `expansion_phases` — orphaned phases
    /// are harmless since they are gated by `object_fields`
    /// presence in the renderer.
    pub fn collapse_all_for_object(&mut self, object_id: u64) {
        self.object_fields.remove(&object_id);
        self.object_static_fields.remove(&object_id);
        self.object_errors.remove(&object_id);
    }

    /// Derives an `object_id → ExpansionPhase` map from data
    /// caches and `expansion_phases`.
    ///
    /// Temporary compatibility bridge for `tree_render` which
    /// still looks up phases by `object_id`.
    ///
    /// - `object_errors` keys → `Failed`
    /// - `object_fields` keys → `Expanded`
    /// - Remaining entries are inferred as `Collapsed` by the
    ///   renderer's default.
    pub(crate) fn derive_object_phases(&self) -> HashMap<u64, ExpansionPhase> {
        let mut map = HashMap::new();
        for &oid in self.object_fields.keys() {
            map.insert(oid, ExpansionPhase::Expanded);
        }
        for &oid in self.object_errors.keys() {
            map.insert(oid, ExpansionPhase::Failed);
        }
        map
    }

    /// Returns the [`ChunkState`] for a specific chunk.
    pub fn chunk_state(&self, collection_id: u64, chunk_offset: usize) -> Option<&ChunkState> {
        self.collection_chunks
            .get(&collection_id)?
            .chunk_pages
            .get(&chunk_offset)
    }
}
