//! TUI application loop and top-level state machine.
//!
//! `App` owns the navigation engine and all UI state. `run_tui` sets up
//! the terminal and drives the 16ms event loop (60 fps target, NFR4).

use std::{
    collections::HashMap,
    io,
    sync::{
        Arc,
        mpsc::{self, Receiver},
    },
    time::{Duration, Instant},
};

use crossterm::event::{self, Event, KeyEventKind};
use hprof_engine::{CollectionPage, FieldInfo, FieldValue, NavigationEngine, VariableValue};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
};

use crate::{
    favorites::{PinnedItem, PinnedSnapshot, snapshot_from_cursor},
    input::{self, InputEvent},
    views::{
        favorites_panel::{FavoritesPanel, FavoritesPanelState},
        help_bar::{self, HelpBar, HelpContext},
        stack_view::{
            ChunkState, CollectionChunks, ExpansionPhase, FrameId, NavigationPath,
            NavigationPathBuilder, PathSegment, RenderCursor, StackState, StackView, ThreadId,
            compute_chunk_ranges,
        },
        status_bar::StatusBar,
        thread_list::{SearchableList, ThreadListState},
    },
    warnings::WarningLog,
};

#[cfg(test)]
use crate::views::stack_view::{CollectionId, EntryIdx, FieldIdx, StaticFieldIdx, VarIdx};

/// Delay before showing the loading spinner for expansions/page loads.
/// Operations completing before this threshold show no spinner.
const EXPANSION_LOADING_THRESHOLD: Duration = Duration::from_millis(200);

/// Minimum time the spinner stays visible once shown, preventing
/// sub-frame flicker for operations that complete shortly after
/// the threshold.
const MINIMUM_SPINNER_DURATION: Duration = Duration::from_millis(400);

/// Status bar spinner state — mutual exclusion by construction.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum SpinnerState {
    Idle,
    Resolving,
    NavigatingToPin,
}

struct PendingExpansion {
    rx: Receiver<Option<Vec<FieldInfo>>>,
    object_id: u64,
    #[allow(dead_code)]
    path: NavigationPath,
    pub(super) started: Instant,
    loading_shown: bool,
}

struct PendingPage {
    rx: Receiver<Option<CollectionPage>>,
    pub(super) started: Instant,
    loading_shown: bool,
    /// Path of the collection node (set for offset-0 eager loads
    /// so the node can be marked Loading after threshold).
    parent_path: Option<NavigationPath>,
}

/// Minimum terminal width to show the favorites panel.
const MIN_WIDTH_FAVORITES_PANEL: u16 = 120;

/// Maximum number of additional chunk pages loaded into a single pinned snapshot.
const SNAPSHOT_CHUNK_PAGE_LIMIT: usize = 10;

/// Which panel currently holds keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    ThreadList,
    StackFrames,
    Favorites,
}

/// Action returned by `App::handle_input` to drive the event loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppAction {
    Continue,
    Quit,
}

/// Outcome of a [`App::navigate_to_path`] walk.
#[derive(Debug)]
#[allow(dead_code)]
enum WalkOutcome {
    /// All segments resolved; cursor placed at the given path.
    Success(NavigationPath),
    /// Walk deferred on an async operation; cursor on last resolved step.
    PartialAt(NavigationPath),
}

/// Resource being awaited by an in-progress go-to-pin walk.
#[derive(Debug, PartialEq)]
enum AwaitedResource {
    /// Waiting for async object expansion of the given `object_id`.
    ObjectExpansion(u64),
    /// Waiting for async collection page load for the given `collection_id`.
    CollectionPage(u64),
    /// In-frame step cap reached — resume on the next event-loop tick.
    Continue,
}

/// State for an in-progress go-to-pin navigation.
struct PendingNavigation {
    /// Unresolved tail of the walk (including the step that triggered defer).
    remaining_path: Vec<PathSegment>,
    /// Full original path, kept for stale-context restart (AC9).
    original_path: NavigationPath,
    /// Thread serial that owns the walk (authoritative over selection state).
    thread_id: u32,
    /// What we are waiting for before the walk can continue.
    awaited: AwaitedResource,
    /// Object IDs already expanded in the materialised prefix.
    ///
    /// Checked on resume: if any is no longer `Expanded`, the context is
    /// stale and the walk restarts from `original_path`.
    prereq_expanded: Vec<NavigationPath>,
}

/// Top-level TUI application state.
pub struct App<E: NavigationEngine> {
    engine: Arc<E>,
    thread_list: ThreadListState,
    focus: Focus,
    filename: String,
    /// Total thread count, captured once at construction (threads never change).
    thread_count: usize,
    /// Number of parse warnings, captured once at construction.
    warning_count: usize,
    /// Preview state shown in the stack panel while focus is on thread list.
    preview_stack_state: StackState,
    /// Stack frame state — `Some` when a thread is entered, `None` otherwise.
    stack_state: Option<StackState>,
    /// In-flight object expansion receivers keyed by `NavigationPath`.
    pending_expansions: HashMap<NavigationPath, PendingExpansion>,
    /// In-flight collection page load receivers keyed by
    /// `(collection_id, chunk_offset)`.
    pending_pages: HashMap<(u64, usize), PendingPage>,
    /// In-flight collection page load receivers for pinned snapshots keyed by
    /// `(pinned_item_idx, collection_id, chunk_offset)`.
    pending_pinned_pages: HashMap<(usize, u64, usize), PendingPage>,
    /// Warnings accumulated during the session (e.g. unresolved string backing arrays).
    warnings: WarningLog,
    /// Timestamp of the last periodic memory log emission.
    last_memory_log: Instant,
    /// Pinned items in the favorites panel.
    pinned: Vec<PinnedItem>,
    /// ratatui list state for the favorites panel scroll position.
    favorites_list_state: FavoritesPanelState,
    /// Focus before entering `Focus::Favorites`, restored on `Esc` / `F`.
    prev_focus: Focus,
    /// Transient status bar message (e.g. "Terminal trop étroit"). Taken on render.
    ui_status: Option<String>,
    /// Terminal width as of the last render call.
    last_area_width: u16,
    /// Whether the keyboard shortcut help panel is visible.
    show_help: bool,
    /// Whether object IDs are displayed in stack frame rows.
    show_object_ids: bool,
    /// In-progress go-to-pin navigation state (async walk deferred).
    pending_navigation: Option<PendingNavigation>,
    /// Consolidated spinner state for the status bar.
    spinner_state: SpinnerState,
    /// Spinner frame counter, incremented via `wrapping_add(1)` each render.
    spinner_tick: u8,
    /// Earliest instant at which the spinner may be hidden after going
    /// non-idle. Arms on `Idle → non-Idle`; resets to `None` on return
    /// to `Idle`.
    loading_until: Option<Instant>,
}

impl<E: NavigationEngine> App<E> {
    /// Constructs the app from a ready engine. Loads thread list immediately.
    pub fn new(engine: E, filename: String) -> Self {
        let engine = Arc::new(engine);
        let threads = engine.list_threads();
        let thread_count = threads.len();
        let warning_count = engine.warnings().len();
        let mut thread_list = ThreadListState::new(threads);
        thread_list.set_visible_height(0);
        let preview_frames = thread_list
            .selected_serial()
            .map(|serial| engine.get_stack_frames(serial))
            .unwrap_or_default();
        let mut preview_stack_state = StackState::new(preview_frames);
        preview_stack_state.set_visible_height(0);
        Self {
            engine,
            thread_list,
            focus: Focus::ThreadList,
            filename,
            thread_count,
            warning_count,
            preview_stack_state,
            stack_state: None,
            pending_expansions: HashMap::new(),
            pending_pages: HashMap::new(),
            pending_pinned_pages: HashMap::new(),
            warnings: WarningLog::default(),
            last_memory_log: Instant::now(),
            pinned: Vec::new(),
            favorites_list_state: FavoritesPanelState::default(),
            prev_focus: Focus::ThreadList,
            ui_status: None,
            last_area_width: 0,
            show_help: false,
            show_object_ids: false,
            pending_navigation: None,
            spinner_state: SpinnerState::Idle,
            spinner_tick: 0,
            loading_until: None,
        }
    }

    fn open_stack_for_selected_thread(&mut self, serial: u32) {
        self.thread_list.select_serial(serial);
        let frames = self.engine.get_stack_frames(serial);
        let mut stack_state = StackState::new(frames);
        stack_state.set_visible_height(0);
        self.stack_state = Some(stack_state);
        self.focus = Focus::StackFrames;
    }

    #[allow(dead_code)]
    fn expand_object_sync(&mut self, object_id: u64, path: &NavigationPath) -> bool {
        let Some(fields) = self.engine.expand_object(object_id) else {
            return false;
        };
        let static_fields = self
            .engine
            .class_of_object(object_id)
            .map(|cid| self.engine.get_static_fields(cid))
            .unwrap_or_default();
        let Some(stack_state) = &mut self.stack_state else {
            return false;
        };
        stack_state.set_expansion_done_at_path(path, object_id, fields);
        stack_state.set_static_fields(object_id, static_fields);
        true
    }

    fn root_object_id_for_var(&self, frame_id: u64, var_idx: usize) -> Option<u64> {
        self.stack_state.as_ref().and_then(|stack_state| {
            stack_state
                .vars()
                .get(&frame_id)
                .and_then(|vars| vars.get(var_idx))
                .and_then(|var| {
                    if let VariableValue::ObjectRef { id, .. } = var.value {
                        Some(id)
                    } else {
                        None
                    }
                })
        })
    }

    fn child_object_id_at(&self, object_id: u64, field_idx: usize) -> Option<u64> {
        self.stack_state.as_ref().and_then(|stack_state| {
            stack_state
                .object_fields()
                .get(&object_id)
                .and_then(|fields| fields.get(field_idx))
                .and_then(|field| {
                    if let FieldValue::ObjectRef { id, .. } = field.value {
                        Some(id)
                    } else {
                        None
                    }
                })
        })
    }

    #[allow(dead_code)]
    fn ensure_collection_entry_loaded(&mut self, collection_id: u64, entry_index: usize) -> bool {
        let mut needs_init = false;
        let mut total_count = 0u64;

        if let Some(stack_state) = &self.stack_state {
            if let Some(cc) = stack_state.expansion.collection_chunks.get(&collection_id) {
                total_count = cc.total_count;
                if cc.find_entry(entry_index).is_some() {
                    return true;
                }
            } else {
                needs_init = true;
            }
        } else {
            return false;
        }

        if needs_init {
            let Some(first_page) = self.engine.get_page(collection_id, 0, 100) else {
                return false;
            };
            total_count = first_page.total_count;
            let chunks = CollectionChunks {
                total_count,
                eager_page: Some(first_page),
                chunk_pages: compute_chunk_ranges(total_count)
                    .into_iter()
                    .map(|(o, _)| (o, ChunkState::Collapsed))
                    .collect(),
            };
            if let Some(stack_state) = &mut self.stack_state {
                stack_state
                    .expansion
                    .collection_chunks
                    .insert(collection_id, chunks);
            }
        }

        if let Some(stack_state) = &self.stack_state
            && let Some(cc) = stack_state.expansion.collection_chunks.get(&collection_id)
        {
            if cc.find_entry(entry_index).is_some() {
                return true;
            }
            total_count = cc.total_count;
        }

        let (offset, limit) = if entry_index < 100 {
            (0usize, (total_count as usize).min(100))
        } else if let Some((offset, limit)) = compute_chunk_ranges(total_count)
            .into_iter()
            .find(|(offset, limit)| entry_index >= *offset && entry_index < *offset + *limit)
        {
            (offset, limit)
        } else {
            return false;
        };

        let Some(page) = self.engine.get_page(collection_id, offset, limit) else {
            return false;
        };
        if let Some(stack_state) = &mut self.stack_state
            && let Some(cc) = stack_state
                .expansion
                .collection_chunks
                .get_mut(&collection_id)
        {
            if offset == 0 {
                cc.eager_page = Some(page);
            } else {
                cc.chunk_pages.insert(offset, ChunkState::Loaded(page));
            }
            return cc.find_entry(entry_index).is_some();
        }
        false
    }

    fn collection_entry_object_id(&self, collection_id: u64, entry_index: usize) -> Option<u64> {
        self.stack_state.as_ref().and_then(|stack_state| {
            let cc = stack_state
                .expansion
                .collection_chunks
                .get(&collection_id)?;
            let entry = cc.find_entry(entry_index)?;
            if let FieldValue::ObjectRef { id, .. } = entry.value {
                Some(id)
            } else {
                None
            }
        })
    }

    /// Navigates to `path` within the thread identified by `thread_id`.
    ///
    /// Each segment is resolved using cached state only — no blocking engine
    /// calls. When a cache miss occurs the async work is spawned and the walk
    /// is suspended via [`PendingNavigation`]. Resumed by [`poll_expansions`]
    /// or [`poll_pages`] when the awaited resource is ready.
    fn navigate_to_path(&mut self, thread_id: ThreadId, path: &NavigationPath) -> WalkOutcome
    where
        E: Send + Sync + 'static,
    {
        let segs = path.segments().to_vec();
        self.navigate_walk(thread_id.0, path.clone(), vec![], &segs, 0)
    }

    /// Internal incremental walker.
    ///
    /// - `thread_id`: raw thread serial (authoritative even after thread switch)
    /// - `original_path`: full path, kept for stale-context restart
    /// - `materialised`: segments already resolved (prefix)
    /// - `remaining`: segments still to process (may overlap with end of orig)
    /// - `in_frame_steps`: cached steps taken so far (cap = 10)
    fn navigate_walk(
        &mut self,
        thread_id: u32,
        original_path: NavigationPath,
        materialised: Vec<PathSegment>,
        remaining: &[PathSegment],
        in_frame_steps: usize,
    ) -> WalkOutcome
    where
        E: Send + Sync + 'static,
    {
        let target_serial = self
            .engine
            .list_threads()
            .into_iter()
            .find(|t| t.thread_serial == thread_id)
            .map(|t| t.thread_serial);
        let Some(serial) = target_serial else {
            return WalkOutcome::PartialAt(NavigationPathBuilder::frame_only(FrameId(0)));
        };
        let current_serial = self.thread_list.selected_serial();
        if current_serial != Some(serial) || self.stack_state.is_none() {
            self.open_stack_for_selected_thread(serial);
        }

        if remaining.is_empty() {
            self.position_cursor_and_scroll(&materialised);
            self.pending_navigation = None;
            self.spinner_state = SpinnerState::Idle;
            self.spinner_tick = 0;
            return WalkOutcome::Success(NavigationPath::from_segments(materialised));
        }

        // Reconstruct current_object_id from already-resolved prefix.
        let mut current_object_id = self.object_id_at_path_end(&materialised);

        let mut materialised = materialised;
        let mut step_count = in_frame_steps;
        let mut prereq_expanded: Vec<NavigationPath> = Vec::new();

        for (i, seg) in remaining.iter().enumerate() {
            if step_count >= 10 {
                let new_remaining = remaining[i..].to_vec();
                let partial = NavigationPath::from_segments(materialised.clone());
                self.pending_navigation = Some(PendingNavigation {
                    remaining_path: new_remaining,
                    original_path,
                    thread_id,
                    awaited: AwaitedResource::Continue,
                    prereq_expanded,
                });
                self.spinner_state = SpinnerState::NavigatingToPin;
                return WalkOutcome::PartialAt(partial);
            }

            match seg {
                PathSegment::Frame(fid) => {
                    let frame_exists = self
                        .stack_state
                        .as_ref()
                        .is_some_and(|s| s.frames().iter().any(|f| f.frame_id == fid.0));
                    if !frame_exists {
                        break;
                    }
                    if self
                        .stack_state
                        .as_ref()
                        .is_some_and(|s| !s.is_expanded(fid.0))
                    {
                        let vars = self.engine.get_local_variables(fid.0);
                        if let Some(s) = &mut self.stack_state {
                            s.toggle_expand(fid.0, vars);
                        }
                    }
                    materialised.push(seg.clone());
                    self.position_cursor_and_scroll(&materialised);
                    current_object_id = None;
                    step_count += 1;
                }
                PathSegment::Var(vi) => {
                    let frame_id = match materialised.first() {
                        Some(PathSegment::Frame(fid)) => fid.0,
                        _ => break,
                    };
                    let oid = self.root_object_id_for_var(frame_id, vi.0);
                    materialised.push(seg.clone());
                    self.position_cursor_and_scroll(&materialised);
                    current_object_id = oid;
                    step_count += 1;
                }
                PathSegment::Field(fi) => {
                    let Some(oid) = current_object_id else {
                        break;
                    };
                    let mat_path = NavigationPath::from_segments(materialised.clone());
                    let expanded = self.stack_state.as_ref().is_some_and(|s| {
                        s.expansion_state_for_path(&mat_path) == ExpansionPhase::Expanded
                    });
                    if !expanded {
                        let exp_path = NavigationPath::from_segments(materialised.clone());
                        self.start_object_expansion(oid, exp_path);
                        let new_remaining = remaining[i..].to_vec();
                        let partial = NavigationPath::from_segments(materialised.clone());
                        self.pending_navigation = Some(PendingNavigation {
                            remaining_path: new_remaining,
                            original_path,
                            thread_id,
                            awaited: AwaitedResource::ObjectExpansion(oid),
                            prereq_expanded,
                        });
                        self.spinner_state = SpinnerState::NavigatingToPin;
                        self.position_cursor_and_scroll(&materialised);
                        return WalkOutcome::PartialAt(partial);
                    }
                    prereq_expanded.push(mat_path);
                    materialised.push(seg.clone());
                    current_object_id = self.child_object_id_at(oid, fi.0);
                    self.position_cursor_and_scroll(&materialised);
                    step_count += 1;
                }
                PathSegment::StaticField(si) => {
                    let Some(oid) = current_object_id else {
                        break;
                    };
                    let mat_path = NavigationPath::from_segments(materialised.clone());
                    let expanded = self.stack_state.as_ref().is_some_and(|s| {
                        s.expansion_state_for_path(&mat_path) == ExpansionPhase::Expanded
                    });
                    if !expanded {
                        let exp_path = NavigationPath::from_segments(materialised.clone());
                        self.start_object_expansion(oid, exp_path);
                        let new_remaining = remaining[i..].to_vec();
                        let partial = NavigationPath::from_segments(materialised.clone());
                        self.pending_navigation = Some(PendingNavigation {
                            remaining_path: new_remaining,
                            original_path,
                            thread_id,
                            awaited: AwaitedResource::ObjectExpansion(oid),
                            prereq_expanded,
                        });
                        self.spinner_state = SpinnerState::NavigatingToPin;
                        self.position_cursor_and_scroll(&materialised);
                        return WalkOutcome::PartialAt(partial);
                    }
                    if !self
                        .stack_state
                        .as_ref()
                        .is_some_and(|s| s.object_static_fields().contains_key(&oid))
                    {
                        break;
                    }
                    prereq_expanded.push(mat_path);
                    materialised.push(seg.clone());
                    current_object_id = self.stack_state.as_ref().and_then(|s| {
                        let fields = s.object_static_fields().get(&oid)?;
                        let field = fields.get(si.0)?;
                        if let hprof_engine::FieldValue::ObjectRef { id, .. } = field.value {
                            Some(id)
                        } else {
                            None
                        }
                    });
                    self.position_cursor_and_scroll(&materialised);
                    step_count += 1;
                }
                PathSegment::CollectionEntry(cid, ei) => {
                    let collection_path = NavigationPath::from_segments(materialised.clone());
                    if let Some(s) = &mut self.stack_state {
                        s.expansion
                            .expansion_phases
                            .insert(collection_path, ExpansionPhase::Expanded);
                    }
                    let loaded = self.stack_state.as_ref().is_some_and(|s| {
                        s.expansion
                            .collection_chunks
                            .get(&cid.0)
                            .is_some_and(|cc| cc.find_entry(ei.0).is_some())
                    });
                    if !loaded {
                        self.ensure_collection_initialized_async(cid.0, ei.0);
                        let new_remaining = remaining[i..].to_vec();
                        let partial = NavigationPath::from_segments(materialised.clone());
                        self.pending_navigation = Some(PendingNavigation {
                            remaining_path: new_remaining,
                            original_path,
                            thread_id,
                            awaited: AwaitedResource::CollectionPage(cid.0),
                            prereq_expanded,
                        });
                        self.spinner_state = SpinnerState::NavigatingToPin;
                        self.position_cursor_and_scroll(&materialised);
                        return WalkOutcome::PartialAt(partial);
                    }
                    materialised.push(seg.clone());
                    current_object_id = self.collection_entry_object_id(cid.0, ei.0);
                    self.position_cursor_and_scroll(&materialised);
                    step_count += 1;
                }
            }
        }

        let final_path = NavigationPath::from_segments(materialised.clone());
        self.position_cursor_and_scroll(&materialised);
        self.pending_navigation = None;
        self.spinner_state = SpinnerState::Idle;
        self.spinner_tick = 0;
        WalkOutcome::Success(final_path)
    }

    /// Resumes a deferred go-to-pin walk after its awaited resource is ready.
    ///
    /// Checks stale context (AC9): if any prerequisite object is no longer
    /// expanded, restarts the full walk from `original_path`.
    fn resume_pending_navigation(&mut self, pending: PendingNavigation)
    where
        E: Send + Sync + 'static,
    {
        // Stale context check: prerequisites must still be Expanded (AC9).
        let stale = pending.prereq_expanded.iter().any(|p| {
            self.stack_state
                .as_ref()
                .is_some_and(|s| s.expansion_state_for_path(p) != ExpansionPhase::Expanded)
        });
        if stale {
            self.ui_status = Some("Pin context changed, retrying...".to_string());
            let original = pending.original_path.clone();
            let segs = original.segments().to_vec();
            self.navigate_walk(pending.thread_id, original, vec![], &segs, 0);
            return;
        }

        // Invariant: `remaining_path` is always a suffix of
        // `original_path.segments()`. We reconstruct `materialised`
        // as the prefix that has already been resolved.
        let all_segs = pending.original_path.segments().to_vec();
        let remaining_len = pending.remaining_path.len();
        let done_count = all_segs.len().saturating_sub(remaining_len);
        let materialised = all_segs[..done_count].to_vec();
        let remaining = pending.remaining_path.clone();
        self.navigate_walk(
            pending.thread_id,
            pending.original_path,
            materialised,
            &remaining,
            0,
        );
    }

    /// Positions the cursor at the last segment in `materialised` and scrolls.
    ///
    /// Falls back to the first `At(_)` cursor when the exact path is not yet
    /// rendered (e.g. expansion not yet reflected in `flat_items`).
    fn position_cursor_and_scroll(&mut self, materialised: &[PathSegment]) {
        if materialised.is_empty() {
            return;
        }
        let target_path = NavigationPath::from_segments(materialised.to_vec());
        let target = RenderCursor::At(target_path);
        if let Some(s) = &mut self.stack_state {
            let flat = s.flat_items();
            if flat.contains(&target) {
                s.set_cursor(target);
            } else if let Some(fb) = flat.into_iter().find(|c| matches!(c, RenderCursor::At(_))) {
                s.set_cursor(fb);
            }
            s.scroll_to_cursor();
        }
    }

    /// Reconstructs the `current_object_id` by walking the materialised prefix.
    ///
    /// Used on walk resume to re-derive object context without re-expanding.
    fn object_id_at_path_end(&self, materialised: &[PathSegment]) -> Option<u64> {
        let mut current: Option<u64> = None;
        let frame_id = match materialised.first() {
            Some(PathSegment::Frame(fid)) => fid.0,
            _ => return None,
        };
        for seg in materialised {
            match seg {
                PathSegment::Frame(_) => {
                    current = None;
                }
                PathSegment::Var(vi) => {
                    current = self.root_object_id_for_var(frame_id, vi.0);
                }
                PathSegment::Field(fi) => {
                    current = current.and_then(|oid| self.child_object_id_at(oid, fi.0));
                }
                PathSegment::StaticField(si) => {
                    current = current.and_then(|oid| {
                        self.stack_state.as_ref().and_then(|s| {
                            let fields = s.object_static_fields().get(&oid)?;
                            let field = fields.get(si.0)?;
                            if let hprof_engine::FieldValue::ObjectRef { id, .. } = field.value {
                                Some(id)
                            } else {
                                None
                            }
                        })
                    });
                }
                PathSegment::CollectionEntry(cid, ei) => {
                    current = self.collection_entry_object_id(cid.0, ei.0);
                }
            }
        }
        current
    }

    /// Initialises async loading for a collection entry when cache misses.
    ///
    /// Creates a deferred `CollectionChunks` entry (total_count=0 placeholder)
    /// and spawns the appropriate page load. `total_count` is updated when the
    /// page arrives in `poll_pages`.
    fn ensure_collection_initialized_async(&mut self, collection_id: u64, entry_index: usize)
    where
        E: Send + Sync + 'static,
    {
        let has_chunks = self
            .stack_state
            .as_ref()
            .is_some_and(|s| s.expansion.collection_chunks.contains_key(&collection_id));

        if !has_chunks {
            let chunks = CollectionChunks {
                total_count: 0,
                eager_page: None,
                chunk_pages: std::collections::HashMap::new(),
            };
            if let Some(s) = &mut self.stack_state {
                s.expansion.collection_chunks.insert(collection_id, chunks);
            }
            self.start_collection_page_load(collection_id, 0, 100);
            self.engine.spawn_walker(collection_id);
            return;
        }

        // Collection exists but the specific entry isn't loaded yet.
        let chunk_info = self.stack_state.as_ref().and_then(|s| {
            let cc = s.expansion.collection_chunks.get(&collection_id)?;
            let total = cc.total_count;
            if total == 0 {
                return None;
            }
            if entry_index < 100 && cc.eager_page.is_none() {
                return Some((0usize, (total as usize).min(100)));
            }
            compute_chunk_ranges(total)
                .into_iter()
                .find(|(offset, limit)| entry_index >= *offset && entry_index < *offset + *limit)
        });
        if let Some((offset, limit)) = chunk_info {
            self.start_collection_page_load(collection_id, offset, limit);
        }
    }

    fn cycle_focus(&mut self) {
        match self.focus {
            Focus::ThreadList => {
                if self.stack_state.is_some() {
                    self.focus = Focus::StackFrames;
                }
            }
            Focus::StackFrames => {
                if self.favorites_visible() {
                    self.focus = Focus::Favorites;
                } else {
                    self.focus = Focus::ThreadList;
                    self.refresh_preview_stack();
                }
            }
            Focus::Favorites => {
                self.focus = Focus::ThreadList;
                self.refresh_preview_stack();
            }
        }
    }

    fn refresh_preview_stack(&mut self) {
        let frames = self
            .thread_list
            .selected_serial()
            .map(|serial| self.engine.get_stack_frames(serial))
            .unwrap_or_default();
        let mut preview_stack_state = StackState::new(frames);
        preview_stack_state.set_visible_height(0);
        self.preview_stack_state = preview_stack_state;
    }

    /// Processes one input event and returns the next `AppAction`.
    pub fn handle_input(&mut self, event: InputEvent) -> AppAction
    where
        E: Send + Sync + 'static,
    {
        if event == InputEvent::ToggleHelp {
            self.show_help = !self.show_help;
            return AppAction::Continue;
        }
        match self.focus {
            Focus::ThreadList => self.handle_thread_list_input(event),
            Focus::StackFrames => self.handle_stack_frames_input(event),
            Focus::Favorites => self.handle_favorites_input(event),
        }
    }

    /// Returns the name of the currently active thread, or an empty string.
    fn active_thread_name(&self) -> String {
        self.thread_list
            .selected_thread()
            .map(|t| t.name.clone())
            .unwrap_or_default()
    }

    /// Adds or removes a `PinnedItem` by key (toggle semantics).
    fn toggle_pin(&mut self, item: PinnedItem) {
        if let Some(pos) = self.pinned.iter().position(|p| p.key == item.key) {
            self.pinned.remove(pos);
            self.pending_pinned_pages.clear();
        } else {
            self.pinned.push(item);
        }
        self.sync_favorites_selection();
    }

    fn sync_favorites_selection(&mut self) {
        self.favorites_list_state.set_items_len(self.pinned.len());
        let sel = if self.pinned.is_empty() {
            None
        } else {
            Some(
                self.favorites_list_state
                    .selected_index()
                    .min(self.pinned.len().saturating_sub(1)),
            )
        };
        self.favorites_list_state.set_selected_index(sel);
    }

    fn handle_favorites_input(&mut self, event: InputEvent) -> AppAction
    where
        E: Send + Sync + 'static,
    {
        match event {
            InputEvent::Up => {
                if !self.pinned.is_empty() {
                    self.favorites_list_state.move_up();
                }
            }
            InputEvent::Down => {
                if !self.pinned.is_empty() {
                    self.favorites_list_state.move_down();
                }
            }
            InputEvent::Right | InputEvent::Enter => {
                if !self.pinned.is_empty()
                    && self
                        .favorites_list_state
                        .current_toggleable_object()
                        .is_some()
                    && let Some(path) = self.favorites_list_state.current_toggleable_path()
                {
                    let idx = self
                        .favorites_list_state
                        .selected_index()
                        .min(self.pinned.len().saturating_sub(1));
                    let path = path.clone();
                    if let Some(item) = self.pinned.get_mut(idx)
                        && item.local_collapsed.contains(&path)
                    {
                        item.local_collapsed.remove(&path);
                    }
                }
            }
            InputEvent::Left => {
                if !self.pinned.is_empty()
                    && self
                        .favorites_list_state
                        .current_toggleable_object()
                        .is_some()
                    && let Some(path) = self.favorites_list_state.current_toggleable_path()
                {
                    let idx = self
                        .favorites_list_state
                        .selected_index()
                        .min(self.pinned.len().saturating_sub(1));
                    let path = path.clone();
                    if let Some(item) = self.pinned.get_mut(idx)
                        && !item.local_collapsed.contains(&path)
                    {
                        item.local_collapsed.insert(path);
                    }
                    self.favorites_list_state.clamp_sub_row();
                }
            }
            InputEvent::ToggleFavorite => {
                if !self.pinned.is_empty() {
                    let idx = self
                        .favorites_list_state
                        .selected_index()
                        .min(self.pinned.len().saturating_sub(1));
                    let Some(key) = self.pinned.get(idx).map(|item| item.key.clone()) else {
                        return AppAction::Continue;
                    };
                    self.pinned.retain(|item| item.key != key);
                    self.pending_pinned_pages.clear();
                    self.sync_favorites_selection();
                    if self.pinned.is_empty() {
                        self.focus = if self.stack_state.is_some() {
                            Focus::StackFrames
                        } else {
                            Focus::ThreadList
                        };
                    }
                }
            }
            InputEvent::NavigateToSource => {
                if self.pinned.is_empty() {
                    return AppAction::Continue;
                }
                let idx = self
                    .favorites_list_state
                    .selected_index()
                    .min(self.pinned.len().saturating_sub(1));
                let Some(item) = self.pinned.get(idx) else {
                    return AppAction::Continue;
                };
                let pin_key = item.key.clone();
                let thread_id = pin_key.thread_id;
                let nav_path = pin_key.nav_path.clone();

                let thread_name = pin_key.thread_name.clone();
                let thread_exists = self
                    .engine
                    .list_threads()
                    .into_iter()
                    .any(|t| t.thread_serial == thread_id.0);
                if !thread_exists {
                    self.ui_status = Some(format!("Thread '{thread_name}' no longer found"));
                    return AppAction::Continue;
                }

                // Cancel any in-progress navigation before starting a new one (RT-A1).
                self.pending_navigation = None;
                self.spinner_state = SpinnerState::Idle;
                self.spinner_tick = 0;
                self.navigate_to_path(thread_id, &nav_path);
                self.focus = Focus::StackFrames;
            }
            // h — hide / show field (AC1, AC2)
            // 'h'/'H' are caught here as SearchChar because input::from_key maps
            // unbound printable keys to SearchChar. Focus-based dispatch ensures
            // these arms fire only when the favorites panel is focused, leaving
            // thread-list incremental search fully intact.
            InputEvent::SearchChar('h') => {
                if let Some((key, is_hidden)) = self.favorites_list_state.field_key_at_cursor() {
                    let idx = self.favorites_list_state.selected_index();
                    if let Some(item) = self.pinned.get_mut(idx) {
                        if is_hidden {
                            item.hidden_fields.remove(&key);
                        } else {
                            item.hidden_fields.insert(key);
                        }
                        // row_counts are one frame stale — clamp so sub_row
                        // doesn't point past the (now shorter) item.
                        self.favorites_list_state.clamp_sub_row();
                    }
                }
            }
            // H — toggle reveal mode for hidden rows in current snapshot
            InputEvent::SearchChar('H') => {
                let idx = self.favorites_list_state.selected_index();
                if let Some(item) = self.pinned.get_mut(idx) {
                    item.show_hidden = !item.show_hidden;
                    // Transitioning from reveal→hidden may shorten the item.
                    if !item.show_hidden {
                        self.favorites_list_state.clamp_sub_row();
                    }
                }
            }
            InputEvent::FocusFavorites | InputEvent::Escape => {
                self.focus = self.prev_focus;
            }
            InputEvent::Tab => {
                self.cycle_focus();
            }
            InputEvent::ToggleObjectIds => {
                self.show_object_ids = !self.show_object_ids;
            }
            InputEvent::Quit => return AppAction::Quit,
            _ => {}
        }
        AppAction::Continue
    }

    /// Returns whether the favorites panel is visible given the current state.
    fn favorites_visible(&self) -> bool {
        !self.pinned.is_empty() && self.last_area_width >= MIN_WIDTH_FAVORITES_PANEL
    }

    fn handle_thread_list_input(&mut self, event: InputEvent) -> AppAction {
        let mut refresh_preview = false;
        if event == InputEvent::Escape {
            // ORDER MATTERS: deactivate before clear
            if self.thread_list.is_search_active() {
                self.thread_list.deactivate_search();
            } else if !self.thread_list.filter().is_empty() {
                self.thread_list.clear_filter();
                refresh_preview = true;
            }
            if refresh_preview {
                self.refresh_preview_stack();
            }
            return AppAction::Continue;
        }

        if self.thread_list.is_search_active() {
            match event {
                InputEvent::SearchChar(c) => {
                    let mut q = self.thread_list.filter().to_string();
                    q.push(c);
                    self.thread_list.apply_filter(&q);
                    refresh_preview = true;
                }
                InputEvent::SearchBackspace => {
                    let mut q = self.thread_list.filter().to_string();
                    q.pop();
                    self.thread_list.apply_filter(&q);
                    refresh_preview = true;
                }
                InputEvent::Up => {
                    self.thread_list.move_up();
                    refresh_preview = true;
                }
                InputEvent::Down => {
                    self.thread_list.move_down();
                    refresh_preview = true;
                }
                InputEvent::Home => {
                    self.thread_list.move_home();
                    refresh_preview = true;
                }
                InputEvent::End => {
                    self.thread_list.move_end();
                    refresh_preview = true;
                }
                InputEvent::PageDown => {
                    self.thread_list.page_down();
                    refresh_preview = true;
                }
                InputEvent::PageUp => {
                    self.thread_list.page_up();
                    refresh_preview = true;
                }
                InputEvent::ToggleFavorite => {
                    let mut q = self.thread_list.filter().to_string();
                    q.push('f');
                    self.thread_list.apply_filter(&q);
                    refresh_preview = true;
                }
                InputEvent::FocusFavorites => {
                    let mut q = self.thread_list.filter().to_string();
                    q.push('F');
                    self.thread_list.apply_filter(&q);
                    refresh_preview = true;
                }
                InputEvent::Enter => {
                    self.thread_list.deactivate_search();
                    if let Some(serial) = self.thread_list.selected_serial() {
                        self.open_stack_for_selected_thread(serial);
                    }
                }
                InputEvent::Tab => {
                    self.cycle_focus();
                }
                InputEvent::Quit => return AppAction::Quit,
                _ => {}
            }
        } else {
            match event {
                InputEvent::Up => {
                    self.thread_list.move_up();
                    refresh_preview = true;
                }
                InputEvent::Down => {
                    self.thread_list.move_down();
                    refresh_preview = true;
                }
                InputEvent::Home => {
                    self.thread_list.move_home();
                    refresh_preview = true;
                }
                InputEvent::End => {
                    self.thread_list.move_end();
                    refresh_preview = true;
                }
                InputEvent::PageDown => {
                    self.thread_list.page_down();
                    refresh_preview = true;
                }
                InputEvent::PageUp => {
                    self.thread_list.page_up();
                    refresh_preview = true;
                }
                InputEvent::SearchActivate => {
                    self.thread_list.activate_search();
                }
                InputEvent::Enter => {
                    if let Some(serial) = self.thread_list.selected_serial() {
                        self.open_stack_for_selected_thread(serial);
                    }
                }
                InputEvent::FocusFavorites => {
                    if self.favorites_visible() {
                        self.prev_focus = self.focus;
                        self.focus = Focus::Favorites;
                    } else if !self.pinned.is_empty() {
                        self.ui_status = Some("Terminal trop étroit (< 120 cols)".to_string());
                    }
                }
                InputEvent::Tab => {
                    self.cycle_focus();
                }
                InputEvent::SearchChar('s') => {
                    self.thread_list.activate_search();
                }
                InputEvent::Quit => return AppAction::Quit,
                _ => {}
            }
        }
        if refresh_preview {
            self.refresh_preview_stack();
        }
        AppAction::Continue
    }

    fn handle_stack_frames_input(&mut self, event: InputEvent) -> AppAction
    where
        E: Send + Sync + 'static,
    {
        match event {
            InputEvent::Escape => {
                // Priority: cancel any in-progress go-to-pin navigation (AC6).
                if let Some(nav) = self.pending_navigation.take() {
                    // Remove the awaited async op so its result
                    // is discarded (the node won't expand).
                    match nav.awaited {
                        AwaitedResource::ObjectExpansion(oid) => {
                            self.pending_expansions.retain(|_, pe| pe.object_id != oid);
                        }
                        AwaitedResource::CollectionPage(cid) => {
                            self.pending_pages.retain(|&(id, _), _| id != cid);
                        }
                        AwaitedResource::Continue => {}
                    }
                    let has_loading = self.has_loading_shown_pending();
                    if has_loading {
                        self.spinner_state = SpinnerState::Resolving;
                    } else {
                        self.spinner_state = SpinnerState::Idle;
                        self.spinner_tick = 0;
                        self.loading_until = None;
                    }
                    return AppAction::Continue;
                }
                // If inside a collection, collapse it and
                // return cursor to the parent field.
                let coll_info = self
                    .stack_state
                    .as_ref()
                    .and_then(|s| s.cursor_collection_id());
                if let Some((cid, restore_cursor)) = coll_info {
                    self.pending_pages.retain(|&(id, _), _| id != cid);
                    if let Some(s) = &mut self.stack_state {
                        let cpath = match s.cursor() {
                            RenderCursor::At(p) => Some(p.clone()),
                            _ => None,
                        };
                        s.expansion.collection_chunks.remove(&cid);
                        if let Some(p) = &cpath {
                            s.expansion.collapse_at_path(p);
                        }
                        s.set_cursor(restore_cursor);
                    }
                    return AppAction::Continue;
                }
                // If cursor is on a loading node, cancel.
                let loading_info = self.stack_state.as_ref().and_then(|s| {
                    let oid = s.selected_loading_object_id()?;
                    let path = match s.cursor() {
                        RenderCursor::LoadingNode(p) | RenderCursor::FailedNode(p) => p.clone(),
                        _ => return None,
                    };
                    Some((oid, path))
                });
                if let Some((oid, path)) = loading_info {
                    self.pending_expansions.retain(|_, pe| pe.object_id != oid);
                    if let Some(s) = &mut self.stack_state {
                        s.cancel_expansion(&path);
                    }
                } else {
                    self.stack_state = None;
                    self.focus = Focus::ThreadList;
                    self.refresh_preview_stack();
                }
            }
            InputEvent::Up => {
                if let Some(s) = &mut self.stack_state {
                    s.move_up();
                }
            }
            InputEvent::Down => {
                if let Some(s) = &mut self.stack_state {
                    s.move_down();
                }
            }
            InputEvent::PageDown => {
                if let Some(s) = &mut self.stack_state {
                    s.move_page_down();
                }
            }
            InputEvent::PageUp => {
                if let Some(s) = &mut self.stack_state {
                    s.move_page_up();
                }
            }
            InputEvent::CameraScrollUp => {
                if let Some(s) = &mut self.stack_state {
                    s.scroll_view_up();
                }
            }
            InputEvent::CameraScrollDown => {
                if let Some(s) = &mut self.stack_state {
                    s.scroll_view_down();
                }
            }
            InputEvent::CameraPageUp => {
                if let Some(s) = &mut self.stack_state {
                    s.scroll_view_page_up();
                }
            }
            InputEvent::CameraPageDown => {
                if let Some(s) = &mut self.stack_state {
                    s.scroll_view_page_down();
                }
            }
            InputEvent::CameraCenterSelection => {
                if let Some(s) = &mut self.stack_state {
                    s.center_view_on_selection();
                }
            }
            InputEvent::Right => {
                enum RightCmd {
                    ExpandFrame(u64),
                    StartObj(u64, NavigationPath),
                    StartCollection(u64, u64),
                    StartEntryObj(u64, NavigationPath),
                    LoadChunk(u64, usize, usize),
                }
                let cmd = self.stack_state.as_ref().and_then(|s| {
                    Some(match s.cursor().clone() {
                        RenderCursor::At(ref path) => {
                            let segs = path.segments();
                            if segs.len() == 1 {
                                let fid = s.selected_frame_id()?;
                                if s.is_expanded(fid) {
                                    return None;
                                }
                                RightCmd::ExpandFrame(fid)
                            } else if segs.len() == 2 {
                                let oid = s.selected_object_id()?;
                                if let Some(ec) = s.selected_var_entry_count() {
                                    if ec == 0 || s.expansion.collection_chunks.contains_key(&oid) {
                                        return None;
                                    }
                                    return Some(RightCmd::StartCollection(oid, ec));
                                }
                                match s.expansion_state_for_path(path) {
                                    ExpansionPhase::Collapsed => {
                                        RightCmd::StartObj(oid, path.clone())
                                    }
                                    _ => return None,
                                }
                            } else {
                                let last = segs.last()?;
                                match last {
                                    PathSegment::Field(_) => {
                                        if let Some((cid, ec)) = s.selected_field_collection_info()
                                        {
                                            if ec == 0
                                                || s.expansion.collection_chunks.contains_key(&cid)
                                            {
                                                return None;
                                            }
                                            return Some(RightCmd::StartCollection(cid, ec));
                                        }
                                        let nested_id = s.selected_field_ref_id()?;
                                        match s.expansion_state_for_path(path) {
                                            ExpansionPhase::Collapsed => {
                                                RightCmd::StartObj(nested_id, path.clone())
                                            }
                                            _ => return None,
                                        }
                                    }
                                    PathSegment::CollectionEntry(_, _) => {
                                        let oid = s.selected_collection_entry_ref_id()?;
                                        if let Some(ec) = s.selected_collection_entry_count() {
                                            if ec == 0
                                                || s.expansion.collection_chunks.contains_key(&oid)
                                            {
                                                return None;
                                            }
                                            return Some(RightCmd::StartCollection(oid, ec));
                                        }
                                        match s.expansion_state_for_path(path) {
                                            ExpansionPhase::Collapsed => {
                                                RightCmd::StartEntryObj(oid, path.clone())
                                            }
                                            _ => return None,
                                        }
                                    }
                                    PathSegment::StaticField(_) => {
                                        if let Some((cid, ec)) =
                                            s.selected_static_field_collection_info()
                                        {
                                            if ec == 0
                                                || s.expansion.collection_chunks.contains_key(&cid)
                                            {
                                                return None;
                                            }
                                            return Some(RightCmd::StartCollection(cid, ec));
                                        }
                                        let nested_id = s.selected_static_field_ref_id()?;
                                        match s.expansion_state_for_path(path) {
                                            ExpansionPhase::Collapsed => {
                                                RightCmd::StartObj(nested_id, path.clone())
                                            }
                                            _ => return None,
                                        }
                                    }
                                    _ => return None,
                                }
                            }
                        }
                        RenderCursor::ChunkSection(_, _) => {
                            if let Some((cid, co, cl)) = s.selected_chunk_info() {
                                match s.chunk_state(cid, co) {
                                    Some(ChunkState::Collapsed) => RightCmd::LoadChunk(cid, co, cl),
                                    _ => return None,
                                }
                            } else {
                                return None;
                            }
                        }
                        _ => return None,
                    })
                });
                match cmd {
                    Some(RightCmd::ExpandFrame(fid)) => {
                        let vars = self.engine.get_local_variables(fid);
                        if let Some(s) = &mut self.stack_state {
                            s.toggle_expand(fid, vars);
                        }
                    }
                    Some(RightCmd::StartObj(oid, path)) => {
                        self.start_object_expansion(oid, path);
                    }
                    Some(RightCmd::StartCollection(cid, ec)) => {
                        let limit = (ec as usize).min(100);
                        let chunks = CollectionChunks {
                            total_count: ec,
                            eager_page: None,
                            chunk_pages: compute_chunk_ranges(ec)
                                .into_iter()
                                .map(|(o, _)| (o, ChunkState::Collapsed))
                                .collect(),
                        };
                        let cursor_path =
                            self.stack_state.as_ref().and_then(|s| match s.cursor() {
                                RenderCursor::At(p) => Some(p.clone()),
                                _ => None,
                            });
                        if let Some(s) = &mut self.stack_state {
                            s.expansion.collection_chunks.insert(cid, chunks);
                            if let Some(ref path) = cursor_path {
                                s.expansion
                                    .expansion_phases
                                    .insert(path.clone(), ExpansionPhase::Expanded);
                            }
                        }
                        self.start_collection_page_load(cid, 0, limit);
                        self.engine.spawn_walker(cid);
                        if let Some(path) = cursor_path
                            && let Some(pp) = self.pending_pages.get_mut(&(cid, 0))
                        {
                            pp.parent_path = Some(path);
                        }
                    }
                    Some(RightCmd::StartEntryObj(oid, path)) => {
                        self.start_object_expansion(oid, path);
                    }
                    Some(RightCmd::LoadChunk(cid, offset, limit)) => {
                        self.start_collection_page_load(cid, offset, limit);
                    }
                    None => {}
                }
            }
            InputEvent::Left => {
                enum LeftCmd {
                    CollapseFrame(u64),
                    CollapseObj(u64, NavigationPath),
                    CollapseNestedObj(u64, NavigationPath),
                    CollapseCollection(u64, NavigationPath),
                    CollapseEntryObj(u64, NavigationPath),
                    NavigateToParent(RenderCursor),
                }
                let cmd = self.stack_state.as_ref().and_then(|s| {
                    Some(match s.cursor().clone() {
                        RenderCursor::At(ref path) => {
                            let segs = path.segments();
                            if segs.len() == 1 {
                                let fid = s.selected_frame_id()?;
                                if s.is_expanded(fid) {
                                    LeftCmd::CollapseFrame(fid)
                                } else {
                                    return None;
                                }
                            } else if segs.len() == 2 {
                                let Some(oid) = s.selected_object_id() else {
                                    return Some(LeftCmd::NavigateToParent(s.parent_cursor()?));
                                };
                                if s.expansion.collection_chunks.contains_key(&oid) {
                                    return Some(LeftCmd::CollapseCollection(oid, path.clone()));
                                }
                                match s.expansion_state_for_path(path) {
                                    ExpansionPhase::Expanded => {
                                        LeftCmd::CollapseObj(oid, path.clone())
                                    }
                                    _ => LeftCmd::NavigateToParent(s.parent_cursor()?),
                                }
                            } else {
                                let last = segs.last()?;
                                match last {
                                    PathSegment::Field(_) => {
                                        if let Some((cid, _)) = s.selected_field_collection_info()
                                            && s.expansion.collection_chunks.contains_key(&cid)
                                        {
                                            return Some(LeftCmd::CollapseCollection(
                                                cid,
                                                path.clone(),
                                            ));
                                        }
                                        if s.selected_field_ref_id().is_some()
                                            && s.expansion_state_for_path(path)
                                                == ExpansionPhase::Expanded
                                        {
                                            let nid = s.selected_field_ref_id().unwrap();
                                            return Some(LeftCmd::CollapseNestedObj(
                                                nid,
                                                path.clone(),
                                            ));
                                        }
                                        LeftCmd::NavigateToParent(s.parent_cursor()?)
                                    }
                                    PathSegment::CollectionEntry(_, _) => {
                                        if s.selected_collection_entry_ref_id().is_some()
                                            && s.expansion_state_for_path(path)
                                                == ExpansionPhase::Expanded
                                        {
                                            let oid = s.selected_collection_entry_ref_id().unwrap();
                                            return Some(LeftCmd::CollapseEntryObj(
                                                oid,
                                                path.clone(),
                                            ));
                                        }
                                        LeftCmd::NavigateToParent(s.parent_cursor()?)
                                    }
                                    PathSegment::StaticField(_) => {
                                        if let Some((cid, _)) =
                                            s.selected_static_field_collection_info()
                                            && s.expansion.collection_chunks.contains_key(&cid)
                                        {
                                            return Some(LeftCmd::CollapseCollection(
                                                cid,
                                                path.clone(),
                                            ));
                                        }
                                        if s.selected_static_field_ref_id().is_some()
                                            && s.expansion_state_for_path(path)
                                                == ExpansionPhase::Expanded
                                        {
                                            let nid = s.selected_static_field_ref_id().unwrap();
                                            return Some(LeftCmd::CollapseNestedObj(
                                                nid,
                                                path.clone(),
                                            ));
                                        }
                                        LeftCmd::NavigateToParent(s.parent_cursor()?)
                                    }
                                    _ => LeftCmd::NavigateToParent(s.parent_cursor()?),
                                }
                            }
                        }
                        RenderCursor::ChunkSection(_, _)
                        | RenderCursor::LoadingNode(_)
                        | RenderCursor::CyclicNode(_)
                        | RenderCursor::SectionHeader(_)
                        | RenderCursor::OverflowRow(_)
                        | RenderCursor::FailedNode(_) => {
                            LeftCmd::NavigateToParent(s.parent_cursor()?)
                        }
                        RenderCursor::NoFrames => return None,
                    })
                });
                match cmd {
                    Some(LeftCmd::CollapseFrame(fid)) => {
                        if let Some(s) = &mut self.stack_state {
                            s.toggle_expand(fid, vec![]);
                        }
                    }
                    Some(LeftCmd::CollapseObj(oid, path)) => {
                        self.pending_expansions.retain(|_, pe| pe.object_id != oid);
                        if let Some(s) = &mut self.stack_state {
                            s.collapse_object_recursive(&path);
                        }
                    }
                    Some(LeftCmd::CollapseNestedObj(oid, path)) => {
                        self.pending_expansions.retain(|_, pe| pe.object_id != oid);
                        if let Some(s) = &mut self.stack_state {
                            s.collapse_object(&path);
                        }
                    }
                    Some(LeftCmd::CollapseCollection(cid, path)) => {
                        if let Some(s) = &mut self.stack_state {
                            s.expansion.collection_chunks.remove(&cid);
                            s.expansion.collapse_at_path(&path);
                        }
                        self.pending_pages.retain(|&(id, _), _| id != cid);
                    }
                    Some(LeftCmd::CollapseEntryObj(oid, path)) => {
                        self.pending_expansions.retain(|_, pe| pe.object_id != oid);
                        if let Some(s) = &mut self.stack_state {
                            s.collapse_object_recursive(&path);
                        }
                    }
                    Some(LeftCmd::NavigateToParent(parent)) => {
                        if let Some(s) = &mut self.stack_state {
                            s.set_cursor(parent);
                        }
                    }
                    None => {}
                }
            }
            InputEvent::Enter => {
                enum Cmd {
                    CollapseFrame(u64),
                    ExpandFrame(u64),
                    StartObj(u64, NavigationPath),
                    CollapseObj(u64, NavigationPath),
                    StartNestedObj(u64, NavigationPath),
                    CollapseNestedObj(u64, NavigationPath),
                    StartCollection(u64, u64),
                    CollapseCollection(u64, NavigationPath),
                    LoadChunk(u64, usize, usize),
                    ToggleChunk(u64, usize),
                    StartEntryObj(u64, NavigationPath),
                    CollapseEntryObj(u64, NavigationPath),
                }
                let cmd = self.stack_state.as_ref().and_then(|s| {
                    Some(match s.cursor().clone() {
                        RenderCursor::At(ref path) => {
                            let segs = path.segments();
                            if segs.len() == 1 {
                                let fid = s.selected_frame_id()?;
                                if s.is_expanded(fid) {
                                    Cmd::CollapseFrame(fid)
                                } else {
                                    Cmd::ExpandFrame(fid)
                                }
                            } else if segs.len() == 2 {
                                let oid = s.selected_object_id()?;
                                dbg_log!(
                                    "Var Enter: oid=0x{:X} phase={:?}",
                                    oid,
                                    s.expansion_state_for_path(path)
                                );
                                if let Some(ec) = s.selected_var_entry_count() {
                                    if ec == 0 {
                                        return None;
                                    }
                                    if s.expansion.collection_chunks.contains_key(&oid) {
                                        return Some(Cmd::CollapseCollection(oid, path.clone()));
                                    }
                                    return Some(Cmd::StartCollection(oid, ec));
                                }
                                match s.expansion_state_for_path(path) {
                                    ExpansionPhase::Collapsed => Cmd::StartObj(oid, path.clone()),
                                    ExpansionPhase::Failed => return None,
                                    ExpansionPhase::Expanded => Cmd::CollapseObj(oid, path.clone()),
                                    ExpansionPhase::Loading => return None,
                                }
                            } else {
                                let last = segs.last()?;
                                match last {
                                    PathSegment::Field(_) => {
                                        let coll_info = s.selected_field_collection_info();
                                        dbg_log!("Field Enter: coll_info={:?}", coll_info);
                                        if let Some((cid, ec)) = coll_info {
                                            if ec == 0 {
                                                return None;
                                            }
                                            if s.expansion.collection_chunks.contains_key(&cid) {
                                                return Some(Cmd::CollapseCollection(
                                                    cid,
                                                    path.clone(),
                                                ));
                                            }
                                            return Some(Cmd::StartCollection(cid, ec));
                                        }
                                        let nested_id = s.selected_field_ref_id()?;
                                        match s.expansion_state_for_path(path) {
                                            ExpansionPhase::Collapsed => {
                                                Cmd::StartNestedObj(nested_id, path.clone())
                                            }
                                            ExpansionPhase::Failed => {
                                                return None;
                                            }
                                            ExpansionPhase::Expanded => {
                                                Cmd::CollapseNestedObj(nested_id, path.clone())
                                            }
                                            ExpansionPhase::Loading => {
                                                return None;
                                            }
                                        }
                                    }
                                    PathSegment::CollectionEntry(_, _) => {
                                        let oid = s.selected_collection_entry_ref_id()?;
                                        if let Some(ec) = s.selected_collection_entry_count() {
                                            if ec == 0 {
                                                return None;
                                            }
                                            if s.expansion.collection_chunks.contains_key(&oid) {
                                                return Some(Cmd::CollapseCollection(
                                                    oid,
                                                    path.clone(),
                                                ));
                                            }
                                            return Some(Cmd::StartCollection(oid, ec));
                                        }
                                        match s.expansion_state_for_path(path) {
                                            ExpansionPhase::Collapsed => {
                                                Cmd::StartEntryObj(oid, path.clone())
                                            }
                                            ExpansionPhase::Failed => {
                                                return None;
                                            }
                                            ExpansionPhase::Expanded => {
                                                Cmd::CollapseEntryObj(oid, path.clone())
                                            }
                                            ExpansionPhase::Loading => {
                                                return None;
                                            }
                                        }
                                    }
                                    PathSegment::StaticField(_) => {
                                        if let Some((cid, ec)) =
                                            s.selected_static_field_collection_info()
                                        {
                                            if ec == 0 {
                                                return None;
                                            }
                                            if s.expansion.collection_chunks.contains_key(&cid) {
                                                return Some(Cmd::CollapseCollection(
                                                    cid,
                                                    path.clone(),
                                                ));
                                            }
                                            return Some(Cmd::StartCollection(cid, ec));
                                        }
                                        let nested_id = s.selected_static_field_ref_id()?;
                                        match s.expansion_state_for_path(path) {
                                            ExpansionPhase::Collapsed => {
                                                Cmd::StartNestedObj(nested_id, path.clone())
                                            }
                                            ExpansionPhase::Failed => {
                                                return None;
                                            }
                                            ExpansionPhase::Expanded => {
                                                Cmd::CollapseNestedObj(nested_id, path.clone())
                                            }
                                            ExpansionPhase::Loading => {
                                                return None;
                                            }
                                        }
                                    }
                                    _ => return None,
                                }
                            }
                        }
                        RenderCursor::ChunkSection(_, _) => {
                            if let Some((cid, co, cl)) = s.selected_chunk_info() {
                                let cs = s.chunk_state(cid, co);
                                match cs {
                                    Some(ChunkState::Collapsed) => Cmd::LoadChunk(cid, co, cl),
                                    Some(ChunkState::Loaded(_)) => Cmd::ToggleChunk(cid, co),
                                    _ => return None,
                                }
                            } else {
                                return None;
                            }
                        }
                        RenderCursor::SectionHeader(_)
                        | RenderCursor::OverflowRow(_)
                        | RenderCursor::CyclicNode(_)
                        | RenderCursor::LoadingNode(_)
                        | RenderCursor::FailedNode(_)
                        | RenderCursor::NoFrames => return None,
                    })
                });
                match cmd {
                    Some(Cmd::CollapseFrame(fid)) => {
                        if let Some(s) = &mut self.stack_state {
                            s.toggle_expand(fid, vec![]);
                        }
                    }
                    Some(Cmd::ExpandFrame(fid)) => {
                        let vars = self.engine.get_local_variables(fid);
                        if let Some(s) = &mut self.stack_state {
                            s.toggle_expand(fid, vars);
                        }
                    }
                    Some(Cmd::StartObj(oid, path)) => {
                        self.start_object_expansion(oid, path);
                    }
                    Some(Cmd::CollapseObj(oid, path)) => {
                        self.pending_expansions.retain(|_, pe| pe.object_id != oid);
                        if let Some(s) = &mut self.stack_state {
                            s.collapse_object_recursive(&path);
                        }
                    }
                    Some(Cmd::StartNestedObj(oid, path)) => {
                        self.start_object_expansion(oid, path);
                    }
                    Some(Cmd::CollapseNestedObj(oid, path)) => {
                        self.pending_expansions.retain(|_, pe| pe.object_id != oid);
                        if let Some(s) = &mut self.stack_state {
                            s.collapse_object(&path);
                        }
                    }
                    Some(Cmd::StartCollection(cid, ec)) => {
                        dbg_log!("StartCollection cid=0x{:X} ec={}", cid, ec);
                        let limit = (ec as usize).min(100);
                        let chunks = CollectionChunks {
                            total_count: ec,
                            eager_page: None,
                            chunk_pages: compute_chunk_ranges(ec)
                                .into_iter()
                                .map(|(o, _)| (o, ChunkState::Collapsed))
                                .collect(),
                        };
                        let cursor_path =
                            self.stack_state.as_ref().and_then(|s| match s.cursor() {
                                RenderCursor::At(p) => Some(p.clone()),
                                _ => None,
                            });
                        if let Some(s) = &mut self.stack_state {
                            s.expansion.collection_chunks.insert(cid, chunks);
                            if let Some(ref path) = cursor_path {
                                s.expansion
                                    .expansion_phases
                                    .insert(path.clone(), ExpansionPhase::Expanded);
                            }
                        }
                        self.start_collection_page_load(cid, 0, limit);
                        self.engine.spawn_walker(cid);
                        if let Some(path) = cursor_path
                            && let Some(pp) = self.pending_pages.get_mut(&(cid, 0))
                        {
                            pp.parent_path = Some(path);
                        }
                    }
                    Some(Cmd::LoadChunk(cid, offset, limit)) => {
                        self.start_collection_page_load(cid, offset, limit);
                    }
                    Some(Cmd::ToggleChunk(cid, offset)) => {
                        if let Some(s) = &mut self.stack_state
                            && let Some(cc) = s.expansion.collection_chunks.get_mut(&cid)
                        {
                            cc.chunk_pages.insert(offset, ChunkState::Collapsed);
                        }
                    }
                    Some(Cmd::StartEntryObj(oid, path)) => {
                        self.start_object_expansion(oid, path);
                    }
                    Some(Cmd::CollapseEntryObj(oid, path)) => {
                        self.pending_expansions.retain(|_, pe| pe.object_id != oid);
                        if let Some(s) = &mut self.stack_state {
                            s.collapse_object_recursive(&path);
                        }
                    }
                    Some(Cmd::CollapseCollection(cid, path)) => {
                        if let Some(s) = &mut self.stack_state {
                            s.expansion.collection_chunks.remove(&cid);
                            s.expansion.collapse_at_path(&path);
                        }
                        self.pending_pages.retain(|&(id, _), _| id != cid);
                    }
                    None => {}
                }
            }
            InputEvent::ToggleFavorite => {
                if let Some(state) = &self.stack_state {
                    let thread_name = self.active_thread_name();
                    let thread_id = ThreadId(self.thread_list.selected_serial().unwrap_or(0));
                    if let Some(item) =
                        snapshot_from_cursor(state.cursor(), state, &thread_name, thread_id)
                    {
                        self.toggle_pin(item);
                    }
                }
            }
            InputEvent::FocusFavorites => {
                if self.favorites_visible() {
                    self.prev_focus = self.focus;
                    self.focus = Focus::Favorites;
                } else if !self.pinned.is_empty() {
                    self.ui_status = Some("Terminal trop étroit (< 120 cols)".to_string());
                }
            }
            InputEvent::Tab => {
                self.cycle_focus();
            }
            InputEvent::ToggleObjectIds => {
                self.show_object_ids = !self.show_object_ids;
            }
            InputEvent::Quit => return AppAction::Quit,
            _ => {}
        }
        AppAction::Continue
    }

    /// Spawns a worker thread to expand `object_id` and registers a receiver.
    ///
    /// If the same path is already pending, this is a no-op. Keyed by path
    /// so the same object can be expanded at different paths concurrently.
    fn start_object_expansion(&mut self, object_id: u64, path: NavigationPath)
    where
        E: Send + Sync + 'static,
    {
        if self.pending_expansions.contains_key(&path) {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let engine = Arc::clone(&self.engine);
        std::thread::spawn(move || {
            let result = engine.expand_object(object_id);
            let _ = tx.send(result);
        });
        self.pending_expansions.insert(
            path.clone(),
            PendingExpansion {
                rx,
                object_id,
                path,
                started: Instant::now(),
                loading_shown: false,
            },
        );
    }

    /// Spawns a worker to load a collection page.
    ///
    /// If the `(collection_id, offset)` key is already
    /// pending, this is a no-op. The loading indicator is
    /// deferred until [`EXPANSION_LOADING_THRESHOLD`] elapses.
    fn start_collection_page_load(&mut self, collection_id: u64, offset: usize, limit: usize)
    where
        E: Send + Sync + 'static,
    {
        let key = (collection_id, offset);
        if self.pending_pages.contains_key(&key) {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let engine = Arc::clone(&self.engine);
        std::thread::spawn(move || {
            let result = engine.get_page(collection_id, offset, limit);
            let _ = tx.send(result);
        });
        self.pending_pages.insert(
            key,
            PendingPage {
                rx,
                started: Instant::now(),
                loading_shown: false,
                parent_path: None,
            },
        );
    }

    /// Polls in-flight collection page receivers.
    ///
    /// Handles fallback object expansions internally (no return value).
    /// Resumes any pending navigation whose awaited `CollectionPage` just loaded.
    pub fn poll_pages(&mut self)
    where
        E: Send + Sync + 'static,
    {
        let mut done = Vec::new();
        let mut fallback = Vec::new();
        let mut nav_resume_cid: Option<u64> = None;
        for (&(cid, offset), pp) in self.pending_pages.iter_mut() {
            match pp.rx.try_recv() {
                Ok(Some(page)) => {
                    dbg_log!(
                        "poll_pages: 0x{:X}+{} → {} entries",
                        cid,
                        offset,
                        page.entries.len()
                    );
                    // Restore parent node to Expanded if it was
                    // set to Loading during the threshold wait.
                    if offset == 0
                        && let Some(ref path) = pp.parent_path
                        && let Some(s) = &mut self.stack_state
                    {
                        s.expansion
                            .expansion_phases
                            .insert(path.clone(), ExpansionPhase::Expanded);
                    }
                    if let Some(s) = &mut self.stack_state
                        && let Some(cc) = s.expansion.collection_chunks.get_mut(&cid)
                    {
                        if offset == 0 {
                            if cc.total_count == 0 {
                                cc.total_count = page.total_count;
                                cc.chunk_pages = compute_chunk_ranges(page.total_count)
                                    .into_iter()
                                    .map(|(o, _)| (o, ChunkState::Collapsed))
                                    .collect();
                            }
                            cc.eager_page = Some(page);
                            s.collapse_object_by_id(cid);
                        } else {
                            cc.chunk_pages.insert(offset, ChunkState::Loaded(page));
                        }
                    }
                    // Check if pending nav is waiting for this collection.
                    let is_nav_cid = self
                        .pending_navigation
                        .as_ref()
                        .is_some_and(|p| p.awaited == AwaitedResource::CollectionPage(cid));
                    if is_nav_cid {
                        nav_resume_cid = Some(cid);
                    }
                    done.push((cid, offset));
                }
                Ok(None) => {
                    dbg_log!("poll_pages: 0x{:X}+{} → None (fallback)", cid, offset);
                    if let Some(s) = &mut self.stack_state {
                        s.expansion.collection_chunks.remove(&cid);
                        if offset == 0 {
                            s.collapse_object_by_id(cid);
                        }
                    }
                    // Async failure: clear pending nav with failure message (AC8).
                    if self
                        .pending_navigation
                        .as_ref()
                        .is_some_and(|p| p.awaited == AwaitedResource::CollectionPage(cid))
                    {
                        self.pending_navigation = None;
                        self.spinner_state = SpinnerState::Idle;
                        self.spinner_tick = 0;
                        self.ui_status =
                            Some("Failed to navigate — object not resolvable".to_string());
                    }
                    fallback.push(cid);
                    done.push((cid, offset));
                }
                Err(mpsc::TryRecvError::Empty) => {
                    if !pp.loading_shown && pp.started.elapsed() >= EXPANSION_LOADING_THRESHOLD {
                        if offset == 0 {
                            // Mark the collection node as Loading so it
                            // renders with the loading indicator color.
                            if let Some(ref path) = pp.parent_path
                                && let Some(s) = &mut self.stack_state
                            {
                                s.set_expansion_loading(path);
                            }
                        } else if let Some(s) = &mut self.stack_state
                            && let Some(cc) = s.expansion.collection_chunks.get_mut(&cid)
                        {
                            cc.chunk_pages.insert(offset, ChunkState::Loading);
                        }
                        pp.loading_shown = true;
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    if let Some(s) = &mut self.stack_state {
                        s.expansion.collection_chunks.remove(&cid);
                        if offset == 0 {
                            s.collapse_object_by_id(cid);
                        }
                    }
                    done.push((cid, offset));
                }
            }
        }
        for key in done {
            self.pending_pages.remove(&key);
        }

        // Resume pending navigation if the awaited collection page just loaded (1.6).
        if let Some(cid) = nav_resume_cid {
            let still_pending = self
                .pending_navigation
                .as_ref()
                .is_some_and(|p| p.awaited == AwaitedResource::CollectionPage(cid));
            if still_pending {
                let pending = self.pending_navigation.take().unwrap();
                self.resume_pending_navigation(pending);
            }
        }

        for cid in fallback {
            // Fallback: collection page failed, try regular expand.
            // Use current cursor path if available.
            let path = self.stack_state.as_ref().and_then(|s| match s.cursor() {
                RenderCursor::At(p) => Some(p.clone()),
                _ => None,
            });
            if let Some(p) = path {
                self.start_object_expansion(cid, p);
            }
        }

        let mut pinned_done = Vec::new();
        for (&(item_idx, collection_id, chunk_offset), pp) in self.pending_pinned_pages.iter_mut() {
            match pp.rx.try_recv() {
                Ok(Some(page)) => {
                    if let Some(item) = self.pinned.get_mut(item_idx) {
                        match &mut item.snapshot {
                            PinnedSnapshot::Frame {
                                collection_chunks, ..
                            }
                            | PinnedSnapshot::Subtree {
                                collection_chunks, ..
                            } => {
                                if let Some(cc) = collection_chunks.get_mut(&collection_id)
                                    && cc.chunk_pages.len() < SNAPSHOT_CHUNK_PAGE_LIMIT
                                {
                                    cc.chunk_pages
                                        .insert(chunk_offset, ChunkState::Loaded(page));
                                }
                            }
                            _ => {}
                        }
                    }
                    pinned_done.push((item_idx, collection_id, chunk_offset));
                }
                Ok(None) => {
                    pinned_done.push((item_idx, collection_id, chunk_offset));
                }
                Err(mpsc::TryRecvError::Empty) => {
                    if !pp.loading_shown && pp.started.elapsed() >= EXPANSION_LOADING_THRESHOLD {
                        if let Some(item) = self.pinned.get_mut(item_idx) {
                            match &mut item.snapshot {
                                PinnedSnapshot::Frame {
                                    collection_chunks, ..
                                }
                                | PinnedSnapshot::Subtree {
                                    collection_chunks, ..
                                } => {
                                    if let Some(cc) = collection_chunks.get_mut(&collection_id) {
                                        cc.chunk_pages.insert(chunk_offset, ChunkState::Loading);
                                    }
                                }
                                _ => {}
                            }
                        }
                        pp.loading_shown = true;
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.warnings.add(format!(
                        "pinned page load failed for collection 0x{:X}",
                        collection_id
                    ));
                    pinned_done.push((item_idx, collection_id, chunk_offset));
                }
            }
        }
        for key in pinned_done {
            self.pending_pinned_pages.remove(&key);
        }
    }

    /// Polls all in-flight expansion receivers and updates `StackState`.
    ///
    /// Completed or failed expansions are removed from `pending_expansions`.
    /// The loading spinner is shown only after [`EXPANSION_LOADING_THRESHOLD`]
    /// has elapsed. Resumes any pending navigation whose awaited object just
    /// expanded (AC1, task 1.7).
    pub fn poll_expansions(&mut self)
    where
        E: Send + Sync + 'static,
    {
        let mut done: Vec<NavigationPath> = Vec::new();
        let mut nav_resume_oid: Option<u64> = None;
        for (path, pe) in self.pending_expansions.iter_mut() {
            let object_id = pe.object_id;
            match pe.rx.try_recv() {
                Ok(Some(fields)) => {
                    let class_id = self.engine.class_of_object(object_id);
                    let static_fields = class_id
                        .map(|cid| self.engine.get_static_fields(cid))
                        .unwrap_or_default();
                    #[cfg(feature = "dev-profiling")]
                    match class_id {
                        Some(cid) => dbg_log!(
                            "poll_expansions(0x{:X}): class=0x{:X} fields={} statics={}",
                            object_id,
                            cid,
                            fields.len(),
                            static_fields.len()
                        ),
                        None => dbg_log!(
                            "poll_expansions(0x{:X}): class=<none> fields={}",
                            object_id,
                            fields.len()
                        ),
                    }
                    if let Some(s) = &mut self.stack_state {
                        s.set_expansion_done_at_path(path, object_id, fields);
                        s.set_static_fields(object_id, static_fields);
                    }
                    if self
                        .pending_navigation
                        .as_ref()
                        .is_some_and(|p| p.awaited == AwaitedResource::ObjectExpansion(object_id))
                    {
                        nav_resume_oid = Some(object_id);
                    }
                    done.push(path.clone());
                }
                Ok(None) => {
                    dbg_log!("expand_object(0x{:X}) → None", object_id);
                    if let Some(s) = &mut self.stack_state {
                        s.set_expansion_failed(path, object_id, "Failed to resolve object".into());
                    }
                    if self
                        .pending_navigation
                        .as_ref()
                        .is_some_and(|p| p.awaited == AwaitedResource::ObjectExpansion(object_id))
                    {
                        self.pending_navigation = None;
                        self.spinner_state = SpinnerState::Idle;
                        self.spinner_tick = 0;
                        self.ui_status =
                            Some("Failed to navigate — object not resolvable".to_string());
                    }
                    self.warnings
                        .add(format!("Object 0x{object_id:X} could not be resolved"));
                    done.push(path.clone());
                }
                Err(mpsc::TryRecvError::Empty) => {
                    if !pe.loading_shown && pe.started.elapsed() >= EXPANSION_LOADING_THRESHOLD {
                        if let Some(s) = &mut self.stack_state {
                            s.set_expansion_loading(path);
                        }
                        pe.loading_shown = true;
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    if let Some(s) = &mut self.stack_state {
                        s.set_expansion_failed(
                            path,
                            object_id,
                            "Worker thread disconnected".into(),
                        );
                    }
                    self.warnings
                        .add(format!("Worker disconnected for object 0x{object_id:X}"));
                    done.push(path.clone());
                }
            }
        }
        for path in &done {
            self.pending_expansions.remove(path);
        }

        // Resume navigation after expansion completed (1.7).
        if let Some(oid) = nav_resume_oid {
            let still_pending = self
                .pending_navigation
                .as_ref()
                .is_some_and(|p| p.awaited == AwaitedResource::ObjectExpansion(oid));
            if still_pending {
                let pending = self.pending_navigation.take().unwrap();
                self.resume_pending_navigation(pending);
            }
        }
    }

    /// Returns `true` if any pending operation has crossed the
    /// loading threshold (`loading_shown == true`).
    ///
    /// Scans all three pending HashMaps. With at most ~100 concurrent
    /// operations the O(n) scan is < 0.01ms. If Epic 11.3 introduces
    /// concurrency beyond ~1000 entries, replace with an `AtomicUsize`
    /// counter incremented when `loading_shown` is first set and
    /// decremented when the pending entry is removed.
    fn has_loading_shown_pending(&self) -> bool {
        self.pending_expansions.values().any(|pe| pe.loading_shown)
            || self.pending_pages.values().any(|pp| pp.loading_shown)
            || self
                .pending_pinned_pages
                .values()
                .any(|pp| pp.loading_shown)
    }

    /// Returns walker progress for the first active
    /// walker among currently viewed collections,
    /// or `None` if no walker is active.
    fn current_walker_info(&self) -> Option<(usize, u64)> {
        let cc_map = self
            .stack_state
            .as_ref()?
            .expansion
            .collection_chunks
            .iter();
        for (&cid, chunks) in cc_map {
            if let Some(progress) = self.engine.walker_progress(cid) {
                return Some((progress, chunks.total_count));
            }
        }
        None
    }

    /// Recomputes `spinner_state` from all pending operation maps and
    /// the `loading_until` minimum-display timer. Must be called once
    /// per tick, after `poll_expansions()` + `poll_pages()`.
    fn update_spinner_state(&mut self) {
        let prev = self.spinner_state;

        // NavigatingToPin takes priority over Resolving.
        let nav_active = self.pending_navigation.is_some();
        let has_loading = self.has_loading_shown_pending();

        let computed = if nav_active {
            SpinnerState::NavigatingToPin
        } else if has_loading {
            SpinnerState::Resolving
        } else {
            SpinnerState::Idle
        };

        // Minimum display: keep spinner visible until loading_until.
        let timer_active = self.loading_until.is_some_and(|t| Instant::now() < t);

        if computed != SpinnerState::Idle {
            // Arm the timer on Idle → non-Idle transition only.
            if prev == SpinnerState::Idle {
                self.loading_until = Some(Instant::now() + MINIMUM_SPINNER_DURATION);
            }
            self.spinner_state = computed;
        } else if timer_active {
            // Operations done but minimum display not expired —
            // keep the current spinner_state visible (it may be
            // non-Idle from a normal timer arm, or already Idle
            // if Escape explicitly cleared it — in either case
            // the render is correct as-is).
        } else {
            // Both conditions false: clear spinner.
            self.spinner_state = SpinnerState::Idle;
            self.loading_until = None;
        }
    }

    /// Renders the current state into a ratatui `Frame`.
    pub fn render(&mut self, frame: &mut ratatui::Frame) {
        // Advance spinner when any spinner is active.
        if self.spinner_state != SpinnerState::Idle {
            self.spinner_tick = self.spinner_tick.wrapping_add(1);
        }
        if self.last_memory_log.elapsed() >= Duration::from_secs(20) {
            #[cfg(feature = "dev-profiling")]
            {
                let skeleton_bytes = self.engine.skeleton_bytes();
                let cache_bytes = self.engine.memory_used().saturating_sub(skeleton_bytes);
                mem_log!(
                    "{}",
                    format_memory_log(cache_bytes, self.engine.memory_budget(), skeleton_bytes,)
                );
            }
            self.last_memory_log = Instant::now();
        }

        let area = frame.area();
        self.last_area_width = area.width;

        // Carve out status bar at the bottom.
        let [content_area, status_area] =
            Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);

        // Carve out help bar above status bar when visible.
        let (main_area, help_area) = if self.show_help {
            let [m, h] = Layout::vertical([
                Constraint::Min(0),
                Constraint::Length(help_bar::required_height()),
            ])
            .areas(content_area);
            (m, Some(h))
        } else {
            (content_area, None)
        };

        // Determine if the favorites panel should be shown.
        let show_favorites = !self.pinned.is_empty() && area.width >= MIN_WIDTH_FAVORITES_PANEL;
        if self.focus == Focus::Favorites && !show_favorites {
            self.focus = self.prev_focus;
        }

        // Split main area: 30% thread list | rest for stack (+ optional fav).
        let [list_area, right_area] =
            Layout::horizontal([Constraint::Percentage(30), Constraint::Min(0)]).areas(main_area);

        let (stack_area, fav_area) = if show_favorites {
            let areas = Layout::horizontal([Constraint::Min(0), Constraint::Min(40)])
                .areas::<2>(right_area);
            (areas[0], Some(areas[1]))
        } else {
            (right_area, None)
        };

        // Store visible heights for PageUp/PageDown.
        self.thread_list
            .set_visible_height(list_area.height.saturating_sub(2) as usize);
        if let Some(ref mut ss) = self.stack_state {
            ss.set_visible_height(stack_area.height.saturating_sub(2) as usize);
        }
        self.preview_stack_state
            .set_visible_height(stack_area.height.saturating_sub(2) as usize);

        // Thread list
        let list_focused = self.focus == Focus::ThreadList;
        frame.render_stateful_widget(
            SearchableList {
                focused: list_focused,
            },
            list_area,
            &mut self.thread_list,
        );

        // Stack view — use StackState if available, else preview state.
        let stack_focused = self.focus == Focus::StackFrames;
        if stack_focused {
            if let Some(ref mut ss) = self.stack_state {
                frame.render_stateful_widget(
                    StackView {
                        focused: stack_focused,
                        show_object_ids: self.show_object_ids,
                    },
                    stack_area,
                    ss,
                );
            } else {
                frame.render_stateful_widget(
                    StackView {
                        focused: stack_focused,
                        show_object_ids: self.show_object_ids,
                    },
                    stack_area,
                    &mut self.preview_stack_state,
                );
            }
        } else {
            let state = self
                .stack_state
                .as_mut()
                .unwrap_or(&mut self.preview_stack_state);
            frame.render_stateful_widget(
                StackView {
                    focused: false,
                    show_object_ids: self.show_object_ids,
                },
                stack_area,
                state,
            );
        }

        // Favorites panel — only when visible.
        if let Some(fav_area) = fav_area {
            let fav_focused = self.focus == Focus::Favorites;
            frame.render_stateful_widget(
                FavoritesPanel {
                    focused: fav_focused,
                    show_object_ids: self.show_object_ids,
                    pinned: &self.pinned,
                },
                fav_area,
                &mut self.favorites_list_state,
            );
        }

        // Status bar — resolve selected thread once, use StatusBar widget.
        let selected_serial = self.thread_list.selected_serial();
        let selected_thread = selected_serial.and_then(|s| self.engine.select_thread(s));
        let last_warning: Option<String> = self
            .warnings
            .last()
            .map(str::to_string)
            .or_else(|| self.engine.warnings().last().cloned())
            .or_else(|| self.ui_status.take());
        // Use is_fully_indexed() (integer comparison) rather than
        // indexing_ratio() == 100.0 to avoid floating-point imprecision.
        let file_indexed_pct = if self.engine.is_fully_indexed() {
            None
        } else {
            Some(self.engine.indexing_ratio())
        };
        let pinned_hidden_count = if !self.pinned.is_empty() && !show_favorites {
            self.pinned.len()
        } else {
            0
        };
        frame.render_widget(
            StatusBar {
                filename: &self.filename,
                thread_count: self.thread_count,
                selected: selected_thread.as_ref(),
                warning_count: self.warning_count + self.warnings.count(),
                last_warning: last_warning.as_deref(),
                file_indexed_pct,
                pinned_hidden_count,
                spinner_state: self.spinner_state,
                spinner_tick: self.spinner_tick,
                walker_info: self.current_walker_info(),
            },
            status_area,
        );

        if let Some(area) = help_area {
            let ctx = if self.spinner_state != SpinnerState::Idle {
                HelpContext::Navigating
            } else {
                match self.focus {
                    Focus::ThreadList => HelpContext::ThreadList,
                    Focus::StackFrames => HelpContext::StackFrames,
                    Focus::Favorites => HelpContext::Favorites,
                }
            };
            frame.render_widget(HelpBar { context: ctx }, area);
        }
    }
}

/// Formats a periodic memory usage line for stderr emission.
///
/// Returns a string of the form:
/// `[memory] cache N MB / M MB budget | skeleton K MB (non-evictable)`
#[cfg(any(test, feature = "dev-profiling"))]
pub(crate) fn format_memory_log(
    cache_bytes: usize,
    budget_bytes: u64,
    skeleton_bytes: usize,
) -> String {
    let cache_mb = cache_bytes / (1024 * 1024);
    let budget_mb = budget_bytes / 1_048_576;
    let skeleton_mb = skeleton_bytes / (1024 * 1024);
    format!(
        "[memory] cache {cache_mb} MB / {budget_mb} MB budget | skeleton {skeleton_mb} MB \
         (non-evictable)"
    )
}

/// RAII guard ensuring terminal cleanup on drop, even if a panic occurs.
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
        let mut stdout = io::stdout();
        // Non-fatal: terminals that don't support Kitty protocol ignore this.
        let _ = crossterm::execute!(stdout, crossterm::event::PopKeyboardEnhancementFlags);
        let _ = crossterm::execute!(
            stdout,
            crossterm::terminal::LeaveAlternateScreen,
            crossterm::cursor::Show,
        );
    }
}

/// Sets up the terminal and runs the TUI event loop.
///
/// Terminal state is always restored on return or panic via [`TerminalGuard`].
///
/// ## Errors
/// Propagates any `io::Error` from terminal setup or the event loop.
pub fn run_tui<E: NavigationEngine + Send + Sync + 'static>(
    engine: E,
    filename: String,
) -> io::Result<()> {
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    if let Err(err) = crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen) {
        let _ = crossterm::terminal::disable_raw_mode();
        return Err(err);
    }
    // Enable Kitty keyboard protocol for modifier+arrow support (Ctrl+Up/Down).
    // Non-fatal: terminals that don't support it silently ignore the sequence.
    let _ = crossterm::execute!(
        stdout,
        crossterm::event::PushKeyboardEnhancementFlags(
            crossterm::event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES,
        ),
    );
    // Guard created before `Terminal::new`: if terminal init fails,
    // raw mode and alternate screen are still restored.
    let _guard = TerminalGuard;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    run_loop(&mut terminal, engine, filename)
}

fn run_loop<E: NavigationEngine + Send + Sync + 'static>(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    engine: E,
    filename: String,
) -> io::Result<()> {
    let mut app = App::new(engine, filename);

    loop {
        // 1. Input handling — Escape clears pending nav before polls can resume.
        if event::poll(Duration::from_millis(16))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
            && let Some(ev) = input::from_key(key)
            && app.handle_input(ev) == AppAction::Quit
        {
            return Ok(());
        }

        // 2. Polls — must run after input so Escape can cancel before resume.
        app.engine.drain_walkers();
        app.poll_expansions();
        app.poll_pages();

        // 3. Resume cap-yielded navigation (AwaitedResource::Continue).
        if app
            .pending_navigation
            .as_ref()
            .is_some_and(|p| p.awaited == AwaitedResource::Continue)
            && let Some(pending) = app.pending_navigation.take()
        {
            app.resume_pending_navigation(pending);
        }

        // 4. Recompute consolidated spinner state once per tick.
        app.update_spinner_state();

        // 5. Render.
        terminal.draw(|f| app.render(f))?;
    }
}

#[cfg(test)]
mod tests;
