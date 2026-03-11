//! Core types for the stack frame panel.
//!
//! Defines [`StackCursor`], [`ExpansionPhase`], [`ChunkState`],
//! [`CollectionChunks`], and the [`FAILED_LABEL_SEP`] constant.

use std::collections::HashMap;

use hprof_engine::CollectionPage;

/// Separator used in Failed node labels: `"! ClassName — error message"`.
pub(crate) const FAILED_LABEL_SEP: &str = " — ";

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
