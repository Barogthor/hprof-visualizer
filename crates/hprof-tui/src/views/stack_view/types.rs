//! Core types for the stack frame panel.
//!
//! Defines [`NavigationPath`], [`NavigationPathBuilder`], [`RenderCursor`],
//! [`PathSegment`], [`ExpansionPhase`], [`ChunkState`], [`CollectionChunks`],
//! and associated newtypes.

use std::collections::HashMap;

use hprof_engine::CollectionPage;

/// Separator used in Failed node labels: `"! ClassName — error message"`.
pub(crate) const FAILED_LABEL_SEP: &str = " — ";

/// Maximum number of static fields rendered per object before overflow marker.
pub(crate) const STATIC_FIELDS_RENDER_LIMIT: usize = 20;

// === Newtype IDs ===

/// HPROF thread serial number.
#[derive(Hash, PartialEq, Eq, Clone, Copy, Debug)]
pub struct ThreadId(pub u32);

/// HPROF STACK_FRAME serial.
#[derive(Hash, PartialEq, Eq, Clone, Copy, Debug)]
pub struct FrameId(pub u64);

/// Collection (array/list) object ID.
#[derive(Hash, PartialEq, Eq, Clone, Copy, Debug)]
pub struct CollectionId(pub u64);

/// Variable index within a frame.
#[derive(Hash, PartialEq, Eq, Clone, Copy, Debug)]
pub struct VarIdx(pub usize);

/// Instance field index within an object.
#[derive(Hash, PartialEq, Eq, Clone, Copy, Debug)]
pub struct FieldIdx(pub usize);

/// Static field index within an object's class.
#[derive(Hash, PartialEq, Eq, Clone, Copy, Debug)]
pub struct StaticFieldIdx(pub usize);

/// Entry index within a collection.
#[derive(Hash, PartialEq, Eq, Clone, Copy, Debug)]
pub struct EntryIdx(pub usize);

/// Chunk offset within a paginated collection.
#[derive(Hash, PartialEq, Eq, Clone, Copy, Debug)]
pub struct ChunkOffset(pub usize);

// === PathSegment ===

/// One hop in a [`NavigationPath`].
///
/// Encodes the semantic position within the stack tree:
/// - `Frame` is always at index 0.
/// - `Var` is always at index 1 when present.
/// - All other variants appear at index 2+.
#[derive(Hash, PartialEq, Eq, Clone, Debug)]
pub enum PathSegment {
    /// A stack frame (must be at position 0).
    Frame(FrameId),
    /// A local variable (must be at position 1).
    Var(VarIdx),
    /// An instance field at the given index.
    Field(FieldIdx),
    /// A static field at the given index.
    StaticField(StaticFieldIdx),
    /// An entry in a collection.
    CollectionEntry(CollectionId, EntryIdx),
}

// === NavigationPath ===

/// Composable semantic identity for a position in the stack tree.
///
/// Scoped to one thread's stack — thread identity lives in [`PinKey`],
/// not in the path. Always starts with `Frame` at index 0.
///
/// Build via [`NavigationPathBuilder`].
#[derive(Hash, PartialEq, Eq, Clone, Debug)]
pub struct NavigationPath(Vec<PathSegment>);

impl NavigationPath {
    /// Returns the segments as a slice.
    pub(crate) fn segments(&self) -> &[PathSegment] {
        &self.0
    }

    /// Constructs a `NavigationPath` from segments without validation.
    ///
    /// Use `NavigationPathBuilder::build()` for validated construction.
    /// This constructor is for internal use when segments are already trusted
    /// (e.g. slicing an existing valid path).
    pub(crate) fn from_segments(segs: Vec<PathSegment>) -> Self {
        NavigationPath(segs)
    }

    /// Constructs a raw `NavigationPath` from segments without validation.
    ///
    /// Only for use in tests that verify invariant enforcement in `build()`.
    #[cfg(test)]
    pub(crate) fn from_raw(segs: Vec<PathSegment>) -> Self {
        NavigationPath(segs)
    }

    /// Returns the parent path, or `None` if this is a Frame-only path (depth 1).
    ///
    /// - Depth 1 (Frame-only): returns `None`.
    /// - Depth 2 (Frame + Var): returns Frame-only path.
    /// - Depth 3+: returns path with last segment removed.
    pub fn parent(&self) -> Option<Self> {
        if self.0.len() <= 1 {
            return None;
        }
        let mut segs = self.0.clone();
        segs.pop();
        Some(NavigationPath(segs))
    }
}

// === NavigationPathBuilder ===

/// Builder for [`NavigationPath`] with move semantics to avoid clones.
pub struct NavigationPathBuilder {
    segments: Vec<PathSegment>,
}

impl NavigationPathBuilder {
    /// Builds a depth-1 path containing only a frame segment.
    pub fn frame_only(frame_id: FrameId) -> NavigationPath {
        NavigationPath(vec![PathSegment::Frame(frame_id)])
    }

    /// Starts a depth-2 path with a frame and variable segment.
    pub fn new(frame_id: FrameId, var_idx: VarIdx) -> Self {
        Self {
            segments: vec![PathSegment::Frame(frame_id), PathSegment::Var(var_idx)],
        }
    }

    /// Takes ownership of an existing path to extend it further.
    pub fn extend(path: NavigationPath) -> Self {
        Self { segments: path.0 }
    }

    /// Appends an instance field segment.
    pub fn field(mut self, idx: FieldIdx) -> Self {
        self.segments.push(PathSegment::Field(idx));
        self
    }

    /// Appends a static field segment.
    pub fn static_field(mut self, idx: StaticFieldIdx) -> Self {
        self.segments.push(PathSegment::StaticField(idx));
        self
    }

    /// Appends a collection entry segment.
    pub fn collection_entry(mut self, cid: CollectionId, entry: EntryIdx) -> Self {
        self.segments.push(PathSegment::CollectionEntry(cid, entry));
        self
    }

    /// Builds the [`NavigationPath`], asserting structural invariants.
    ///
    /// Invariants:
    /// - `Frame` must be at index 0.
    /// - If length >= 2, `Var` must be at index 1.
    /// - `Frame` and `Var` must not appear at positions 2+.
    pub fn build(self) -> NavigationPath {
        let segs = &self.segments;
        assert!(
            !segs.is_empty() && matches!(segs[0], PathSegment::Frame(_)),
            "NavigationPath: segment[0] must be Frame, got {:?}",
            segs.first()
        );
        if segs.len() >= 2 {
            assert!(
                matches!(segs[1], PathSegment::Var(_)),
                "NavigationPath: segment[1] must be Var, got {:?}",
                segs.get(1)
            );
        }
        for (i, seg) in segs.iter().enumerate().skip(2) {
            assert!(
                !matches!(seg, PathSegment::Frame(_) | PathSegment::Var(_)),
                "NavigationPath: Frame/Var must not appear at position {i}"
            );
        }
        NavigationPath(self.segments)
    }
}

// === RenderCursor ===

/// Thin ratatui rendering wrapper over a [`NavigationPath`].
///
/// 8 variants replacing the legacy `StackCursor` (17 variants).
/// The [`NavigationPath`] inside each variant is the semantic identity
/// of the row — use it for expansion keying, pin matching, and navigation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderCursor {
    /// No frames loaded.
    NoFrames,
    /// Cursor on an interactive row at this path.
    At(NavigationPath),
    /// Loading spinner for an object expansion at this path.
    LoadingNode(NavigationPath),
    /// Expansion error node at this path.
    FailedNode(NavigationPath),
    /// Cyclic reference marker at this path (non-expandable).
    CyclicNode(NavigationPath),
    /// Chunk section header inside a paginated collection.
    ChunkSection(NavigationPath, ChunkOffset),
    /// Non-interactive `[static]` section header.
    SectionHeader(NavigationPath),
    /// Non-interactive `[+N more static fields]` overflow row.
    OverflowRow(NavigationPath),
}

// === ExpansionPhase ===

/// Phase of an object expansion driven by `App`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpansionPhase {
    Collapsed,
    Loading,
    Expanded,
    Failed,
}

// === ChunkState ===

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

// === CollectionChunks ===

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
