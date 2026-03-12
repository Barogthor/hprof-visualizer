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
    favorites::{PinKey, PinnedItem, snapshot_from_cursor},
    input::{self, InputEvent},
    views::{
        favorites_panel::{FavoritesPanel, FavoritesPanelState},
        help_bar::{self, HelpBar},
        stack_view::{
            ChunkState, CollectionChunks, ExpansionPhase, StackCursor, StackState, StackView,
            compute_chunk_ranges,
        },
        status_bar::StatusBar,
        thread_list::{SearchableList, ThreadListState},
    },
    warnings::WarningLog,
};

/// Delay before showing the loading spinner for expansions/page loads.
/// Operations completing before this threshold show no spinner.
const EXPANSION_LOADING_THRESHOLD: Duration = Duration::from_secs(1);

struct PendingExpansion {
    rx: Receiver<Option<Vec<FieldInfo>>>,
    pub(super) started: Instant,
    loading_shown: bool,
}

struct PendingPage {
    rx: Receiver<Option<CollectionPage>>,
    pub(super) started: Instant,
    loading_shown: bool,
}

/// Minimum terminal width to show the favorites panel.
const MIN_WIDTH_FAVORITES_PANEL: u16 = 120;

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
    /// In-flight object expansion receivers keyed by `object_id`.
    pending_expansions: HashMap<u64, PendingExpansion>,
    /// In-flight collection page load receivers keyed by
    /// `(collection_id, chunk_offset)`.
    pending_pages: HashMap<(u64, usize), PendingPage>,
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
            warnings: WarningLog::default(),
            last_memory_log: Instant::now(),
            pinned: Vec::new(),
            favorites_list_state: FavoritesPanelState::default(),
            prev_focus: Focus::ThreadList,
            ui_status: None,
            last_area_width: 0,
            show_help: false,
            show_object_ids: false,
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

    fn expand_object_sync(&mut self, object_id: u64) -> bool {
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
        stack_state.set_expansion_done(object_id, fields);
        stack_state.set_static_fields(object_id, static_fields);
        true
    }

    fn navigate_stack_cursor_to_pin_key(&mut self, pin_key: &PinKey) {
        let frame_id = match pin_key {
            PinKey::Frame { frame_id, .. }
            | PinKey::Var { frame_id, .. }
            | PinKey::Field { frame_id, .. } => *frame_id,
        };

        let Some(frame_idx) = self.stack_state.as_ref().and_then(|stack_state| {
            stack_state
                .frames()
                .iter()
                .position(|frame| frame.frame_id == frame_id)
        }) else {
            return;
        };

        let needs_expand = self
            .stack_state
            .as_ref()
            .is_some_and(|stack_state| !stack_state.is_expanded(frame_id));
        if needs_expand {
            let vars = self.engine.get_local_variables(frame_id);
            if let Some(stack_state) = &mut self.stack_state {
                stack_state.toggle_expand(frame_id, vars);
            }
        }

        match pin_key {
            PinKey::Frame { .. } => {
                if let Some(stack_state) = &mut self.stack_state {
                    stack_state.set_cursor(StackCursor::OnFrame(frame_idx));
                }
            }
            PinKey::Var { var_idx, .. } => {
                let cursor = StackCursor::OnVar {
                    frame_idx,
                    var_idx: *var_idx,
                };
                if let Some(stack_state) = &mut self.stack_state {
                    if stack_state.flat_items().contains(&cursor) {
                        stack_state.set_cursor(cursor);
                    } else {
                        stack_state.set_cursor(StackCursor::OnFrame(frame_idx));
                    }
                }
            }
            PinKey::Field {
                var_idx,
                field_path,
                ..
            } => {
                let Some(mut current_object_id) =
                    self.stack_state.as_ref().and_then(|stack_state| {
                        stack_state
                            .vars()
                            .get(&frame_id)
                            .and_then(|vars| vars.get(*var_idx))
                            .and_then(|var| {
                                if let VariableValue::ObjectRef { id, .. } = var.value {
                                    Some(id)
                                } else {
                                    None
                                }
                            })
                    })
                else {
                    return;
                };

                for (depth, field_idx) in field_path.iter().enumerate() {
                    let is_expanded = self.stack_state.as_ref().is_some_and(|stack_state| {
                        stack_state.expansion_state(current_object_id) == ExpansionPhase::Expanded
                    });
                    if !is_expanded && !self.expand_object_sync(current_object_id) {
                        break;
                    }

                    if depth + 1 < field_path.len() {
                        let next_object_id = self.stack_state.as_ref().and_then(|stack_state| {
                            stack_state
                                .object_fields()
                                .get(&current_object_id)
                                .and_then(|fields| fields.get(*field_idx))
                                .and_then(|field| {
                                    if let FieldValue::ObjectRef { id, .. } = field.value {
                                        Some(id)
                                    } else {
                                        None
                                    }
                                })
                        });
                        let Some(next_object_id) = next_object_id else {
                            break;
                        };
                        current_object_id = next_object_id;
                    }
                }

                let field_cursor = StackCursor::OnObjectField {
                    frame_idx,
                    var_idx: *var_idx,
                    field_path: field_path.clone(),
                };
                let var_cursor = StackCursor::OnVar {
                    frame_idx,
                    var_idx: *var_idx,
                };

                if let Some(stack_state) = &mut self.stack_state {
                    if stack_state.flat_items().contains(&field_cursor) {
                        stack_state.set_cursor(field_cursor);
                    } else if stack_state.flat_items().contains(&var_cursor) {
                        stack_state.set_cursor(var_cursor);
                    } else {
                        stack_state.set_cursor(StackCursor::OnFrame(frame_idx));
                    }
                }
            }
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

    fn handle_favorites_input(&mut self, event: InputEvent) -> AppAction {
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

                let thread_name = match &pin_key {
                    PinKey::Frame { thread_name, .. }
                    | PinKey::Var { thread_name, .. }
                    | PinKey::Field { thread_name, .. } => thread_name.clone(),
                };

                let matches: Vec<_> = self
                    .engine
                    .list_threads()
                    .into_iter()
                    .filter(|t| t.name == thread_name)
                    .collect();
                let Some(target) = matches.first() else {
                    self.ui_status = Some(format!("Thread '{thread_name}' no longer found"));
                    return AppAction::Continue;
                };
                if matches.len() > 1 {
                    self.ui_status = Some(format!(
                        "Multiple threads named '{thread_name}' — navigated to first match"
                    ));
                }

                self.open_stack_for_selected_thread(target.thread_serial);
                self.navigate_stack_cursor_to_pin_key(&pin_key);
            }
            InputEvent::FocusFavorites | InputEvent::Escape => {
                self.focus = self.prev_focus;
            }
            InputEvent::Tab => {
                self.cycle_focus();
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
                // If inside a collection, collapse it and
                // return cursor to the parent field.
                let coll_info = self
                    .stack_state
                    .as_ref()
                    .and_then(|s| s.cursor_collection_id());
                if let Some((cid, restore_cursor)) = coll_info {
                    self.pending_pages.retain(|&(id, _), _| id != cid);
                    if let Some(s) = &mut self.stack_state {
                        s.expansion.collection_chunks.remove(&cid);
                        s.expansion.collection_restore_cursors.remove(&cid);
                        s.set_cursor(restore_cursor);
                    }
                    return AppAction::Continue;
                }
                // If cursor is on a loading node, cancel.
                let loading_id = self
                    .stack_state
                    .as_ref()
                    .and_then(|s| s.selected_loading_object_id());
                if let Some(oid) = loading_id {
                    self.pending_expansions.remove(&oid);
                    if let Some(s) = &mut self.stack_state {
                        s.cancel_expansion(oid);
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
                    StartObj(u64),
                    StartCollection(u64, u64, StackCursor),
                    StartEntryObj(u64),
                    LoadChunk(u64, usize, usize),
                }
                let cmd = self.stack_state.as_ref().and_then(|s| {
                    Some(match s.cursor().clone() {
                        StackCursor::OnFrame(_) => {
                            let fid = s.selected_frame_id()?;
                            if s.is_expanded(fid) {
                                return None;
                            }
                            RightCmd::ExpandFrame(fid)
                        }
                        StackCursor::OnVar { .. } => {
                            let oid = s.selected_object_id()?;
                            if let Some(ec) = s.selected_var_entry_count() {
                                if s.expansion.collection_chunks.contains_key(&oid) {
                                    return None;
                                }
                                return Some(RightCmd::StartCollection(
                                    oid,
                                    ec,
                                    s.cursor().clone(),
                                ));
                            }
                            match s.expansion_state(oid) {
                                ExpansionPhase::Collapsed => RightCmd::StartObj(oid),
                                _ => return None,
                            }
                        }
                        StackCursor::OnObjectField { .. } => {
                            if let Some((cid, ec)) = s.selected_field_collection_info() {
                                if s.expansion.collection_chunks.contains_key(&cid) {
                                    return None;
                                }
                                return Some(RightCmd::StartCollection(
                                    cid,
                                    ec,
                                    s.cursor().clone(),
                                ));
                            }
                            let nested_id = s.selected_field_ref_id()?;
                            match s.expansion_state(nested_id) {
                                ExpansionPhase::Collapsed => RightCmd::StartObj(nested_id),
                                _ => return None,
                            }
                        }
                        StackCursor::OnChunkSection { .. } => {
                            if let Some((cid, co, cl)) = s.selected_chunk_info() {
                                match s.chunk_state(cid, co) {
                                    Some(ChunkState::Collapsed) => RightCmd::LoadChunk(cid, co, cl),
                                    _ => return None,
                                }
                            } else {
                                return None;
                            }
                        }
                        StackCursor::OnCollectionEntry { .. } => {
                            let oid = s.selected_collection_entry_ref_id()?;
                            if let Some(ec) = s.selected_collection_entry_count() {
                                if s.expansion.collection_chunks.contains_key(&oid) {
                                    return None;
                                }
                                return Some(RightCmd::StartCollection(
                                    oid,
                                    ec,
                                    s.cursor().clone(),
                                ));
                            }
                            match s.expansion_state(oid) {
                                ExpansionPhase::Collapsed => RightCmd::StartEntryObj(oid),
                                _ => return None,
                            }
                        }
                        StackCursor::OnCollectionEntryObjField { .. } => {
                            if let Some((oid, ec)) =
                                s.selected_collection_entry_obj_field_collection_info()
                            {
                                if s.expansion.collection_chunks.contains_key(&oid) {
                                    return None;
                                }
                                return Some(RightCmd::StartCollection(
                                    oid,
                                    ec,
                                    s.cursor().clone(),
                                ));
                            }
                            let oid = s.selected_collection_entry_obj_field_ref_id()?;
                            match s.expansion_state(oid) {
                                ExpansionPhase::Collapsed => RightCmd::StartEntryObj(oid),
                                _ => return None,
                            }
                        }
                        StackCursor::OnStaticSectionHeader { .. } => return None,
                        StackCursor::OnStaticField { .. } => {
                            if let Some((cid, ec)) = s.selected_static_field_collection_info() {
                                if s.expansion.collection_chunks.contains_key(&cid) {
                                    return None;
                                }
                                return Some(RightCmd::StartCollection(
                                    cid,
                                    ec,
                                    s.cursor().clone(),
                                ));
                            }
                            let nested_id = s.selected_static_field_ref_id()?;
                            match s.expansion_state(nested_id) {
                                ExpansionPhase::Collapsed => RightCmd::StartObj(nested_id),
                                _ => return None,
                            }
                        }
                        StackCursor::OnStaticOverflowRow { .. }
                        | StackCursor::OnCollectionEntryStaticSectionHeader { .. }
                        | StackCursor::OnCollectionEntryStaticOverflowRow { .. } => return None,
                        StackCursor::OnStaticObjectField { .. } => {
                            if let Some((cid, ec)) = s.selected_static_obj_field_collection_info() {
                                if s.expansion.collection_chunks.contains_key(&cid) {
                                    return None;
                                }
                                return Some(RightCmd::StartCollection(
                                    cid,
                                    ec,
                                    s.cursor().clone(),
                                ));
                            }
                            let nested_id = s.selected_static_obj_field_ref_id()?;
                            match s.expansion_state(nested_id) {
                                ExpansionPhase::Collapsed => RightCmd::StartObj(nested_id),
                                _ => return None,
                            }
                        }
                        StackCursor::OnCollectionEntryStaticField { .. } => {
                            if let Some((cid, ec)) =
                                s.selected_collection_entry_static_field_collection_info()
                            {
                                if s.expansion.collection_chunks.contains_key(&cid) {
                                    return None;
                                }
                                return Some(RightCmd::StartCollection(
                                    cid,
                                    ec,
                                    s.cursor().clone(),
                                ));
                            }
                            let nested_id = s.selected_collection_entry_static_field_ref_id()?;
                            match s.expansion_state(nested_id) {
                                ExpansionPhase::Collapsed => RightCmd::StartEntryObj(nested_id),
                                _ => return None,
                            }
                        }
                        StackCursor::OnCollectionEntryStaticObjectField { .. } => {
                            if let Some((cid, ec)) =
                                s.selected_collection_entry_static_obj_field_collection_info()
                            {
                                if s.expansion.collection_chunks.contains_key(&cid) {
                                    return None;
                                }
                                return Some(RightCmd::StartCollection(
                                    cid,
                                    ec,
                                    s.cursor().clone(),
                                ));
                            }
                            let nested_id =
                                s.selected_collection_entry_static_obj_field_ref_id()?;
                            match s.expansion_state(nested_id) {
                                ExpansionPhase::Collapsed => RightCmd::StartEntryObj(nested_id),
                                _ => return None,
                            }
                        }
                        StackCursor::OnCyclicNode { .. }
                        | StackCursor::OnObjectLoadingNode { .. }
                        | StackCursor::NoFrames => return None,
                    })
                });
                match cmd {
                    Some(RightCmd::ExpandFrame(fid)) => {
                        let vars = self.engine.get_local_variables(fid);
                        if let Some(s) = &mut self.stack_state {
                            s.toggle_expand(fid, vars);
                        }
                    }
                    Some(RightCmd::StartObj(oid)) => {
                        self.start_object_expansion(oid);
                    }
                    Some(RightCmd::StartCollection(cid, ec, restore_cursor)) => {
                        let limit = (ec as usize).min(100);
                        let chunks = CollectionChunks {
                            total_count: ec,
                            eager_page: None,
                            chunk_pages: compute_chunk_ranges(ec)
                                .into_iter()
                                .map(|(o, _)| (o, ChunkState::Collapsed))
                                .collect(),
                        };
                        if let Some(s) = &mut self.stack_state {
                            s.expansion.collection_chunks.insert(cid, chunks);
                            s.expansion
                                .collection_restore_cursors
                                .insert(cid, restore_cursor);
                        }
                        self.start_collection_page_load(cid, 0, limit);
                    }
                    Some(RightCmd::StartEntryObj(oid)) => {
                        self.start_object_expansion(oid);
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
                    CollapseObj(u64),
                    CollapseNestedObj(u64),
                    CollapseCollection(u64),
                    CollapseEntryObj(u64),
                    NavigateToParent(StackCursor),
                }
                let cmd = self.stack_state.as_ref().and_then(|s| {
                    Some(match s.cursor().clone() {
                        StackCursor::OnFrame(_) => {
                            let fid = s.selected_frame_id()?;
                            if s.is_expanded(fid) {
                                LeftCmd::CollapseFrame(fid)
                            } else {
                                return None;
                            }
                        }
                        StackCursor::OnVar { .. } => {
                            let Some(oid) = s.selected_object_id() else {
                                return Some(LeftCmd::NavigateToParent(s.parent_cursor()?));
                            };
                            if s.expansion.collection_chunks.contains_key(&oid) {
                                return Some(LeftCmd::CollapseCollection(oid));
                            }
                            match s.expansion_state(oid) {
                                ExpansionPhase::Expanded => LeftCmd::CollapseObj(oid),
                                _ => LeftCmd::NavigateToParent(s.parent_cursor()?),
                            }
                        }
                        StackCursor::OnObjectField { .. } => {
                            if let Some((cid, _)) = s.selected_field_collection_info()
                                && s.expansion.collection_chunks.contains_key(&cid)
                            {
                                return Some(LeftCmd::CollapseCollection(cid));
                            }
                            if let Some(nested_id) = s.selected_field_ref_id()
                                && s.expansion_state(nested_id) == ExpansionPhase::Expanded
                            {
                                return Some(LeftCmd::CollapseNestedObj(nested_id));
                            }
                            LeftCmd::NavigateToParent(s.parent_cursor()?)
                        }
                        StackCursor::OnCollectionEntry { .. } => {
                            if let Some(oid) = s.selected_collection_entry_ref_id()
                                && s.expansion_state(oid) == ExpansionPhase::Expanded
                            {
                                return Some(LeftCmd::CollapseEntryObj(oid));
                            }
                            let parent = s.parent_cursor()?;
                            LeftCmd::NavigateToParent(parent)
                        }
                        StackCursor::OnCollectionEntryObjField { .. } => {
                            if let Some((cid, _)) =
                                s.selected_collection_entry_obj_field_collection_info()
                                && s.expansion.collection_chunks.contains_key(&cid)
                            {
                                return Some(LeftCmd::CollapseCollection(cid));
                            }
                            if let Some(oid) = s.selected_collection_entry_obj_field_ref_id()
                                && s.expansion_state(oid) == ExpansionPhase::Expanded
                            {
                                return Some(LeftCmd::CollapseEntryObj(oid));
                            }
                            LeftCmd::NavigateToParent(s.parent_cursor()?)
                        }
                        StackCursor::OnStaticSectionHeader { .. }
                        | StackCursor::OnStaticOverflowRow { .. }
                        | StackCursor::OnCollectionEntryStaticSectionHeader { .. }
                        | StackCursor::OnCollectionEntryStaticOverflowRow { .. } => {
                            LeftCmd::NavigateToParent(s.parent_cursor()?)
                        }
                        StackCursor::OnStaticField { .. } => {
                            if let Some((cid, _)) = s.selected_static_field_collection_info()
                                && s.expansion.collection_chunks.contains_key(&cid)
                            {
                                return Some(LeftCmd::CollapseCollection(cid));
                            }
                            if let Some(nested_id) = s.selected_static_field_ref_id()
                                && s.expansion_state(nested_id) == ExpansionPhase::Expanded
                            {
                                return Some(LeftCmd::CollapseNestedObj(nested_id));
                            }
                            LeftCmd::NavigateToParent(s.parent_cursor()?)
                        }
                        StackCursor::OnStaticObjectField { .. } => {
                            if let Some((cid, _)) = s.selected_static_obj_field_collection_info()
                                && s.expansion.collection_chunks.contains_key(&cid)
                            {
                                return Some(LeftCmd::CollapseCollection(cid));
                            }
                            if let Some(nested_id) = s.selected_static_obj_field_ref_id()
                                && s.expansion_state(nested_id) == ExpansionPhase::Expanded
                            {
                                return Some(LeftCmd::CollapseNestedObj(nested_id));
                            }
                            LeftCmd::NavigateToParent(s.parent_cursor()?)
                        }
                        StackCursor::OnCollectionEntryStaticField { .. } => {
                            if let Some((cid, _)) =
                                s.selected_collection_entry_static_field_collection_info()
                                && s.expansion.collection_chunks.contains_key(&cid)
                            {
                                return Some(LeftCmd::CollapseCollection(cid));
                            }
                            if let Some(oid) = s.selected_collection_entry_static_field_ref_id()
                                && s.expansion_state(oid) == ExpansionPhase::Expanded
                            {
                                return Some(LeftCmd::CollapseEntryObj(oid));
                            }
                            LeftCmd::NavigateToParent(s.parent_cursor()?)
                        }
                        StackCursor::OnCollectionEntryStaticObjectField { .. } => {
                            if let Some((cid, _)) =
                                s.selected_collection_entry_static_obj_field_collection_info()
                                && s.expansion.collection_chunks.contains_key(&cid)
                            {
                                return Some(LeftCmd::CollapseCollection(cid));
                            }
                            if let Some(oid) = s.selected_collection_entry_static_obj_field_ref_id()
                                && s.expansion_state(oid) == ExpansionPhase::Expanded
                            {
                                return Some(LeftCmd::CollapseEntryObj(oid));
                            }
                            LeftCmd::NavigateToParent(s.parent_cursor()?)
                        }
                        StackCursor::OnChunkSection { .. } => {
                            LeftCmd::NavigateToParent(s.parent_cursor()?)
                        }
                        StackCursor::OnObjectLoadingNode { .. }
                        | StackCursor::OnCyclicNode { .. } => {
                            LeftCmd::NavigateToParent(s.parent_cursor()?)
                        }
                        StackCursor::NoFrames => return None,
                    })
                });
                match cmd {
                    Some(LeftCmd::CollapseFrame(fid)) => {
                        if let Some(s) = &mut self.stack_state {
                            s.toggle_expand(fid, vec![]);
                        }
                    }
                    Some(LeftCmd::CollapseObj(oid)) => {
                        self.pending_expansions.remove(&oid);
                        if let Some(s) = &mut self.stack_state {
                            s.collapse_object_recursive(oid);
                        }
                    }
                    Some(LeftCmd::CollapseNestedObj(oid)) => {
                        self.pending_expansions.remove(&oid);
                        if let Some(s) = &mut self.stack_state {
                            s.collapse_object(oid);
                        }
                    }
                    Some(LeftCmd::CollapseCollection(cid)) => {
                        if let Some(s) = &mut self.stack_state {
                            s.expansion.collection_chunks.remove(&cid);
                            s.expansion.collection_restore_cursors.remove(&cid);
                        }
                        self.pending_pages.retain(|&(id, _), _| id != cid);
                    }
                    Some(LeftCmd::CollapseEntryObj(oid)) => {
                        self.pending_expansions.remove(&oid);
                        if let Some(s) = &mut self.stack_state {
                            s.collapse_object_recursive(oid);
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
                // Collect the intended command from an immutable borrow, then act on it.
                enum Cmd {
                    CollapseFrame(u64),
                    ExpandFrame(u64),
                    StartObj(u64),
                    CollapseObj(u64),
                    StartNestedObj(u64),
                    CollapseNestedObj(u64),
                    StartCollection(u64, u64, StackCursor),
                    CollapseCollection(u64),
                    LoadChunk(u64, usize, usize),
                    ToggleChunk(u64, usize),
                    StartEntryObj(u64),
                    CollapseEntryObj(u64),
                }
                let cmd = self.stack_state.as_ref().and_then(|s| {
                    Some(match s.cursor().clone() {
                        StackCursor::OnFrame(_) => {
                            let fid = s.selected_frame_id()?;
                            if s.is_expanded(fid) {
                                Cmd::CollapseFrame(fid)
                            } else {
                                Cmd::ExpandFrame(fid)
                            }
                        }
                        StackCursor::OnVar { .. } => {
                            let oid = s.selected_object_id()?;
                            dbg_log!(
                                "OnVar Enter: oid=0x{:X} phase={:?}",
                                oid,
                                s.expansion_state(oid)
                            );
                            if let Some(ec) = s.selected_var_entry_count() {
                                if s.expansion.collection_chunks.contains_key(&oid) {
                                    return Some(Cmd::CollapseCollection(oid));
                                }
                                return Some(Cmd::StartCollection(oid, ec, s.cursor().clone()));
                            }
                            match s.expansion_state(oid) {
                                ExpansionPhase::Collapsed => Cmd::StartObj(oid),
                                ExpansionPhase::Failed => return None,
                                ExpansionPhase::Expanded => Cmd::CollapseObj(oid),
                                ExpansionPhase::Loading => {
                                    return None;
                                }
                            }
                        }
                        StackCursor::OnObjectField { .. } => {
                            // Check for collection field.
                            let coll_info = s.selected_field_collection_info();
                            dbg_log!("OnObjectField Enter: coll_info={:?}", coll_info);
                            if let Some((cid, ec)) = coll_info {
                                if s.expansion.collection_chunks.contains_key(&cid) {
                                    return Some(Cmd::CollapseCollection(cid));
                                }
                                return Some(Cmd::StartCollection(cid, ec, s.cursor().clone()));
                            }
                            let nested_id = s.selected_field_ref_id()?;
                            dbg_log!(
                                "OnObjectField Enter: nested_id=0x{:X} phase={:?}",
                                nested_id,
                                s.expansion_state(nested_id)
                            );
                            match s.expansion_state(nested_id) {
                                ExpansionPhase::Collapsed => Cmd::StartNestedObj(nested_id),
                                ExpansionPhase::Failed => return None,
                                ExpansionPhase::Expanded => Cmd::CollapseNestedObj(nested_id),
                                ExpansionPhase::Loading => {
                                    return None;
                                }
                            }
                        }
                        StackCursor::OnChunkSection { .. } => {
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
                        StackCursor::OnCollectionEntry { .. } => {
                            let oid = s.selected_collection_entry_ref_id()?;
                            if let Some(ec) = s.selected_collection_entry_count() {
                                if s.expansion.collection_chunks.contains_key(&oid) {
                                    return Some(Cmd::CollapseCollection(oid));
                                }
                                return Some(Cmd::StartCollection(oid, ec, s.cursor().clone()));
                            }
                            match s.expansion_state(oid) {
                                ExpansionPhase::Collapsed => Cmd::StartEntryObj(oid),
                                ExpansionPhase::Failed => return None,
                                ExpansionPhase::Expanded => Cmd::CollapseEntryObj(oid),
                                ExpansionPhase::Loading => return None,
                            }
                        }
                        StackCursor::OnCollectionEntryObjField { .. } => {
                            if let Some((oid, ec)) =
                                s.selected_collection_entry_obj_field_collection_info()
                            {
                                if s.expansion.collection_chunks.contains_key(&oid) {
                                    return Some(Cmd::CollapseCollection(oid));
                                }
                                return Some(Cmd::StartCollection(oid, ec, s.cursor().clone()));
                            }
                            let oid = s.selected_collection_entry_obj_field_ref_id()?;
                            match s.expansion_state(oid) {
                                ExpansionPhase::Collapsed => Cmd::StartEntryObj(oid),
                                ExpansionPhase::Failed => return None,
                                ExpansionPhase::Expanded => Cmd::CollapseEntryObj(oid),
                                ExpansionPhase::Loading => return None,
                            }
                        }
                        StackCursor::OnStaticSectionHeader { .. }
                        | StackCursor::OnStaticOverflowRow { .. }
                        | StackCursor::OnCollectionEntryStaticSectionHeader { .. }
                        | StackCursor::OnCollectionEntryStaticOverflowRow { .. } => return None,
                        StackCursor::OnStaticField { .. } => {
                            if let Some((cid, ec)) = s.selected_static_field_collection_info() {
                                if s.expansion.collection_chunks.contains_key(&cid) {
                                    return Some(Cmd::CollapseCollection(cid));
                                }
                                return Some(Cmd::StartCollection(cid, ec, s.cursor().clone()));
                            }
                            let nested_id = s.selected_static_field_ref_id()?;
                            match s.expansion_state(nested_id) {
                                ExpansionPhase::Collapsed => Cmd::StartNestedObj(nested_id),
                                ExpansionPhase::Failed => return None,
                                ExpansionPhase::Expanded => Cmd::CollapseNestedObj(nested_id),
                                ExpansionPhase::Loading => return None,
                            }
                        }
                        StackCursor::OnStaticObjectField { .. } => {
                            if let Some((cid, ec)) = s.selected_static_obj_field_collection_info() {
                                if s.expansion.collection_chunks.contains_key(&cid) {
                                    return Some(Cmd::CollapseCollection(cid));
                                }
                                return Some(Cmd::StartCollection(cid, ec, s.cursor().clone()));
                            }
                            let nested_id = s.selected_static_obj_field_ref_id()?;
                            match s.expansion_state(nested_id) {
                                ExpansionPhase::Collapsed => Cmd::StartNestedObj(nested_id),
                                ExpansionPhase::Failed => return None,
                                ExpansionPhase::Expanded => Cmd::CollapseNestedObj(nested_id),
                                ExpansionPhase::Loading => return None,
                            }
                        }
                        StackCursor::OnCollectionEntryStaticField { .. } => {
                            if let Some((oid, ec)) =
                                s.selected_collection_entry_static_field_collection_info()
                            {
                                if s.expansion.collection_chunks.contains_key(&oid) {
                                    return Some(Cmd::CollapseCollection(oid));
                                }
                                return Some(Cmd::StartCollection(oid, ec, s.cursor().clone()));
                            }
                            let oid = s.selected_collection_entry_static_field_ref_id()?;
                            match s.expansion_state(oid) {
                                ExpansionPhase::Collapsed => Cmd::StartEntryObj(oid),
                                ExpansionPhase::Failed => return None,
                                ExpansionPhase::Expanded => Cmd::CollapseEntryObj(oid),
                                ExpansionPhase::Loading => return None,
                            }
                        }
                        StackCursor::OnCollectionEntryStaticObjectField { .. } => {
                            if let Some((oid, ec)) =
                                s.selected_collection_entry_static_obj_field_collection_info()
                            {
                                if s.expansion.collection_chunks.contains_key(&oid) {
                                    return Some(Cmd::CollapseCollection(oid));
                                }
                                return Some(Cmd::StartCollection(oid, ec, s.cursor().clone()));
                            }
                            let oid = s.selected_collection_entry_static_obj_field_ref_id()?;
                            match s.expansion_state(oid) {
                                ExpansionPhase::Collapsed => Cmd::StartEntryObj(oid),
                                ExpansionPhase::Failed => return None,
                                ExpansionPhase::Expanded => Cmd::CollapseEntryObj(oid),
                                ExpansionPhase::Loading => return None,
                            }
                        }
                        StackCursor::OnCyclicNode { .. }
                        | StackCursor::OnObjectLoadingNode { .. }
                        | StackCursor::NoFrames => return None,
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
                    Some(Cmd::StartObj(oid)) => self.start_object_expansion(oid),
                    Some(Cmd::CollapseObj(oid)) => {
                        self.pending_expansions.remove(&oid);
                        if let Some(s) = &mut self.stack_state {
                            s.collapse_object_recursive(oid);
                        }
                    }
                    Some(Cmd::StartNestedObj(oid)) => self.start_object_expansion(oid),
                    Some(Cmd::CollapseNestedObj(oid)) => {
                        self.pending_expansions.remove(&oid);
                        if let Some(s) = &mut self.stack_state {
                            s.collapse_object(oid);
                        }
                    }
                    Some(Cmd::StartCollection(cid, ec, restore_cursor)) => {
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
                        if let Some(s) = &mut self.stack_state {
                            s.expansion.collection_chunks.insert(cid, chunks);
                            s.expansion
                                .collection_restore_cursors
                                .insert(cid, restore_cursor);
                        }
                        self.start_collection_page_load(cid, 0, limit);
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
                    Some(Cmd::StartEntryObj(oid)) => self.start_object_expansion(oid),
                    Some(Cmd::CollapseEntryObj(oid)) => {
                        self.pending_expansions.remove(&oid);
                        if let Some(s) = &mut self.stack_state {
                            s.collapse_object_recursive(oid);
                        }
                    }
                    Some(Cmd::CollapseCollection(cid)) => {
                        if let Some(s) = &mut self.stack_state {
                            s.expansion.collection_chunks.remove(&cid);
                            s.expansion.collection_restore_cursors.remove(&cid);
                        }
                        // Remove pending page loads for
                        // this collection.
                        self.pending_pages.retain(|&(id, _), _| id != cid);
                    }
                    None => {}
                }
            }
            InputEvent::ToggleFavorite => {
                if let Some(state) = &self.stack_state {
                    let thread_name = self.active_thread_name();
                    if let Some(item) = snapshot_from_cursor(state.cursor(), state, &thread_name) {
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
    /// If `object_id` is already pending, this is a no-op. The loading spinner
    /// is deferred until [`EXPANSION_LOADING_THRESHOLD`] has elapsed without
    /// the operation completing.
    fn start_object_expansion(&mut self, object_id: u64)
    where
        E: Send + Sync + 'static,
    {
        if self.pending_expansions.contains_key(&object_id) {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let engine = Arc::clone(&self.engine);
        std::thread::spawn(move || {
            let result = engine.expand_object(object_id);
            let _ = tx.send(result);
        });
        self.pending_expansions.insert(
            object_id,
            PendingExpansion {
                rx,
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
            },
        );
    }

    /// Polls in-flight collection page receivers.
    ///
    /// Returns object IDs that need fallback expansion
    /// (unsupported collection types where `get_page`
    /// returned `None`).
    pub fn poll_pages(&mut self) -> Vec<u64> {
        let mut done = Vec::new();
        let mut fallback = Vec::new();
        for (&(cid, offset), pp) in self.pending_pages.iter_mut() {
            match pp.rx.try_recv() {
                Ok(Some(page)) => {
                    dbg_log!(
                        "poll_pages: 0x{:X}+{} → {} entries",
                        cid,
                        offset,
                        page.entries.len()
                    );
                    if let Some(s) = &mut self.stack_state
                        && let Some(cc) = s.expansion.collection_chunks.get_mut(&cid)
                    {
                        if offset == 0 {
                            cc.eager_page = Some(page);
                            s.collapse_object(cid);
                        } else {
                            cc.chunk_pages.insert(offset, ChunkState::Loaded(page));
                        }
                    }
                    done.push((cid, offset));
                }
                Ok(None) => {
                    dbg_log!("poll_pages: 0x{:X}+{} → None (fallback)", cid, offset);
                    if let Some(s) = &mut self.stack_state {
                        s.expansion.collection_chunks.remove(&cid);
                        s.expansion.collection_restore_cursors.remove(&cid);
                        if offset == 0 {
                            s.collapse_object(cid);
                        }
                    }
                    fallback.push(cid);
                    done.push((cid, offset));
                }
                Err(mpsc::TryRecvError::Empty) => {
                    if !pp.loading_shown && pp.started.elapsed() >= EXPANSION_LOADING_THRESHOLD {
                        if offset == 0 {
                            if let Some(s) = &mut self.stack_state {
                                s.set_expansion_loading(cid);
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
                        s.expansion.collection_restore_cursors.remove(&cid);
                        if offset == 0 {
                            s.collapse_object(cid);
                        }
                    }
                    done.push((cid, offset));
                }
            }
        }
        for key in done {
            self.pending_pages.remove(&key);
        }
        fallback
    }

    /// Polls all in-flight expansion receivers and updates `StackState`.
    ///
    /// Completed or failed expansions are removed from `pending_expansions`.
    /// The loading spinner is shown only after [`EXPANSION_LOADING_THRESHOLD`]
    /// has elapsed.
    pub fn poll_expansions(&mut self) {
        let mut done = Vec::new();
        for (&object_id, pe) in self.pending_expansions.iter_mut() {
            match pe.rx.try_recv() {
                Ok(Some(fields)) => {
                    let class_id = self.engine.class_of_object(object_id);
                    let static_fields = class_id
                        .map(|cid| self.engine.get_static_fields(cid))
                        .unwrap_or_default();
                    #[cfg(feature = "dev-profiling")]
                    match class_id {
                        Some(cid) => dbg_log!(
                            "poll_expansions(0x{:X}): class=0x{:X} instance_fields={} static_fields={}",
                            object_id,
                            cid,
                            fields.len(),
                            static_fields.len()
                        ),
                        None => dbg_log!(
                            "poll_expansions(0x{:X}): class=<none> instance_fields={} static_fields=0",
                            object_id,
                            fields.len()
                        ),
                    }
                    if let Some(s) = &mut self.stack_state {
                        s.set_expansion_done(object_id, fields);
                        s.set_static_fields(object_id, static_fields);
                    }
                    done.push(object_id);
                }
                Ok(None) => {
                    dbg_log!("expand_object(0x{:X}) → None", object_id);
                    if let Some(s) = &mut self.stack_state {
                        s.set_expansion_failed(object_id, "Failed to resolve object".to_string());
                    }
                    self.warnings
                        .add(format!("Object 0x{object_id:X} could not be resolved"));
                    done.push(object_id);
                }
                Err(mpsc::TryRecvError::Empty) => {
                    if !pe.loading_shown && pe.started.elapsed() >= EXPANSION_LOADING_THRESHOLD {
                        if let Some(s) = &mut self.stack_state {
                            s.set_expansion_loading(object_id);
                        }
                        pe.loading_shown = true;
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    if let Some(s) = &mut self.stack_state {
                        s.set_expansion_failed(object_id, "Worker thread disconnected".to_string());
                    }
                    self.warnings
                        .add(format!("Worker disconnected for object 0x{object_id:X}"));
                    done.push(object_id);
                }
            }
        }
        for id in done {
            self.pending_expansions.remove(&id);
        }
    }

    /// Renders the current state into a ratatui `Frame`.
    pub fn render(&mut self, frame: &mut ratatui::Frame)
    where
        E: Send + Sync + 'static,
    {
        self.poll_expansions();
        let page_fallbacks = self.poll_pages();
        for cid in page_fallbacks {
            self.start_object_expansion(cid);
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
            frame.render_stateful_widget(
                StackView {
                    focused: stack_focused,
                    show_object_ids: self.show_object_ids,
                },
                stack_area,
                &mut self.preview_stack_state,
            );
        }

        // Favorites panel — only when visible.
        if let Some(fav_area) = fav_area {
            let fav_focused = self.focus == Focus::Favorites;
            frame.render_stateful_widget(
                FavoritesPanel {
                    focused: fav_focused,
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
            },
            status_area,
        );

        if let Some(area) = help_area {
            frame.render_widget(HelpBar, area);
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
        terminal.draw(|f| app.render(f))?;

        if event::poll(Duration::from_millis(16))?
            && let Event::Key(key) = event::read()?
        {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            if let Some(ev) = input::from_key(key)
                && app.handle_input(ev) == AppAction::Quit
            {
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
mod tests;
