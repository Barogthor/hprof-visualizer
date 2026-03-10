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
    time::Duration,
};

use crossterm::event::{self, Event, KeyEventKind};
use hprof_engine::{CollectionPage, FieldInfo, NavigationEngine};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
};

use crate::{
    input::{self, InputEvent},
    views::{
        stack_view::{
            ChunkState, CollectionChunks, ExpansionPhase, StackCursor, StackState, StackView,
            compute_chunk_ranges,
        },
        status_bar::StatusBar,
        thread_list::{SearchableList, ThreadListState},
    },
};

/// Which panel currently holds keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    ThreadList,
    StackFrames,
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
    pending_expansions: HashMap<u64, Receiver<Option<Vec<FieldInfo>>>>,
    /// In-flight collection page load receivers keyed by
    /// `(collection_id, chunk_offset)`.
    pending_pages: HashMap<(u64, usize), Receiver<Option<CollectionPage>>>,
    /// Warnings accumulated during the session (e.g. unresolved string backing arrays).
    app_warnings: Vec<String>,
    /// Visible height of the thread list panel (set during render).
    thread_list_height: u16,
}

impl<E: NavigationEngine> App<E> {
    /// Constructs the app from a ready engine. Loads thread list immediately.
    pub fn new(engine: E, filename: String) -> Self {
        let engine = Arc::new(engine);
        let threads = engine.list_threads();
        let thread_count = threads.len();
        let warning_count = engine.warnings().len();
        let thread_list = ThreadListState::new(threads);
        let preview_frames = thread_list
            .selected_serial()
            .map(|serial| engine.get_stack_frames(serial))
            .unwrap_or_default();
        Self {
            engine,
            thread_list,
            focus: Focus::ThreadList,
            filename,
            thread_count,
            warning_count,
            preview_stack_state: StackState::new(preview_frames),
            stack_state: None,
            pending_expansions: HashMap::new(),
            pending_pages: HashMap::new(),
            app_warnings: Vec::new(),
            thread_list_height: 0,
        }
    }

    fn refresh_preview_stack(&mut self) {
        let frames = self
            .thread_list
            .selected_serial()
            .map(|serial| self.engine.get_stack_frames(serial))
            .unwrap_or_default();
        self.preview_stack_state = StackState::new(frames);
    }

    /// Processes one input event and returns the next `AppAction`.
    pub fn handle_input(&mut self, event: InputEvent) -> AppAction
    where
        E: Send + Sync + 'static,
    {
        match self.focus {
            Focus::ThreadList => self.handle_thread_list_input(event),
            Focus::StackFrames => self.handle_stack_frames_input(event),
        }
    }

    fn handle_thread_list_input(&mut self, event: InputEvent) -> AppAction {
        let mut refresh_preview = false;
        if self.thread_list.is_search_active() {
            match event {
                InputEvent::Escape => {
                    self.thread_list.deactivate_search();
                    self.thread_list.apply_filter("");
                    refresh_preview = true;
                }
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
                    let h = self.thread_list_height as usize;
                    self.thread_list.page_down(h);
                    refresh_preview = true;
                }
                InputEvent::PageUp => {
                    let h = self.thread_list_height as usize;
                    self.thread_list.page_up(h);
                    refresh_preview = true;
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
                    let h = self.thread_list_height as usize;
                    self.thread_list.page_down(h);
                    refresh_preview = true;
                }
                InputEvent::PageUp => {
                    let h = self.thread_list_height as usize;
                    self.thread_list.page_up(h);
                    refresh_preview = true;
                }
                InputEvent::SearchActivate => {
                    self.thread_list.activate_search();
                }
                InputEvent::Enter => {
                    if let Some(serial) = self.thread_list.selected_serial() {
                        let frames = self.engine.get_stack_frames(serial);
                        self.stack_state = Some(StackState::new(frames));
                        self.focus = Focus::StackFrames;
                    }
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
                        s.collection_chunks.remove(&cid);
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
            InputEvent::Enter => {
                // Collect the intended command from an immutable borrow, then act on it.
                enum Cmd {
                    CollapseFrame(u64),
                    ExpandFrame(u64),
                    StartObj(u64),
                    CollapseObj(u64),
                    StartNestedObj(u64),
                    CollapseNestedObj(u64),
                    StartCollection(u64, u64),
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
                            match s.expansion_state(oid) {
                                ExpansionPhase::Collapsed | ExpansionPhase::Failed => {
                                    Cmd::StartObj(oid)
                                }
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
                                if s.collection_chunks.contains_key(&cid) {
                                    return Some(Cmd::CollapseCollection(cid));
                                }
                                return Some(Cmd::StartCollection(cid, ec));
                            }
                            let nested_id = s.selected_field_ref_id()?;
                            dbg_log!(
                                "OnObjectField Enter: nested_id=0x{:X} phase={:?}",
                                nested_id,
                                s.expansion_state(nested_id)
                            );
                            match s.expansion_state(nested_id) {
                                ExpansionPhase::Collapsed | ExpansionPhase::Failed => {
                                    Cmd::StartNestedObj(nested_id)
                                }
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
                            match s.expansion_state(oid) {
                                ExpansionPhase::Collapsed | ExpansionPhase::Failed => {
                                    Cmd::StartEntryObj(oid)
                                }
                                ExpansionPhase::Expanded => Cmd::CollapseEntryObj(oid),
                                ExpansionPhase::Loading => return None,
                            }
                        }
                        StackCursor::OnCollectionEntryObjField { .. } => {
                            let oid = s.selected_collection_entry_obj_field_ref_id()?;
                            match s.expansion_state(oid) {
                                ExpansionPhase::Collapsed | ExpansionPhase::Failed => {
                                    Cmd::StartEntryObj(oid)
                                }
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
                        if let Some(s) = &mut self.stack_state {
                            s.collection_chunks.insert(cid, chunks);
                        }
                        self.start_collection_page_load(cid, 0, limit);
                    }
                    Some(Cmd::LoadChunk(cid, offset, limit)) => {
                        if let Some(s) = &mut self.stack_state
                            && let Some(cc) = s.collection_chunks.get_mut(&cid)
                        {
                            cc.chunk_pages.insert(offset, ChunkState::Loading);
                        }
                        self.start_collection_page_load(cid, offset, limit);
                    }
                    Some(Cmd::ToggleChunk(cid, offset)) => {
                        if let Some(s) = &mut self.stack_state
                            && let Some(cc) = s.collection_chunks.get_mut(&cid)
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
                            s.collection_chunks.remove(&cid);
                        }
                        // Remove pending page loads for
                        // this collection.
                        self.pending_pages.retain(|&(id, _), _| id != cid);
                    }
                    None => {}
                }
            }
            InputEvent::Quit => return AppAction::Quit,
            _ => {}
        }
        AppAction::Continue
    }

    /// Spawns a worker thread to expand `object_id` and registers a receiver.
    ///
    /// If `object_id` is already pending, this is a no-op. The `StackState`
    /// is immediately set to `Loading` so the UI shows a spinner.
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
        self.pending_expansions.insert(object_id, rx);
        if let Some(s) = &mut self.stack_state {
            s.set_expansion_loading(object_id);
        }
    }

    /// Spawns a worker to load a collection page.
    ///
    /// If the `(collection_id, offset)` key is already
    /// pending, this is a no-op.
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
        self.pending_pages.insert(key, rx);
    }

    /// Polls in-flight collection page receivers.
    ///
    /// Returns object IDs that need fallback expansion
    /// (unsupported collection types where `get_page`
    /// returned `None`).
    pub fn poll_pages(&mut self) -> Vec<u64> {
        let mut done = Vec::new();
        let mut fallback = Vec::new();
        for (&(cid, offset), rx) in &self.pending_pages {
            match rx.try_recv() {
                Ok(Some(page)) => {
                    dbg_log!(
                        "poll_pages: 0x{:X}+{} → {} entries",
                        cid,
                        offset,
                        page.entries.len()
                    );
                    if let Some(s) = &mut self.stack_state
                        && let Some(cc) = s.collection_chunks.get_mut(&cid)
                    {
                        if offset == 0 {
                            cc.eager_page = Some(page);
                        } else {
                            cc.chunk_pages.insert(offset, ChunkState::Loaded(page));
                        }
                    }
                    done.push((cid, offset));
                }
                Ok(None) => {
                    dbg_log!("poll_pages: 0x{:X}+{} → None (fallback)", cid, offset);
                    if let Some(s) = &mut self.stack_state {
                        s.collection_chunks.remove(&cid);
                    }
                    fallback.push(cid);
                    done.push((cid, offset));
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    if let Some(s) = &mut self.stack_state {
                        s.collection_chunks.remove(&cid);
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
    pub fn poll_expansions(&mut self) {
        let mut done = Vec::new();
        for (&object_id, rx) in &self.pending_expansions {
            match rx.try_recv() {
                Ok(Some(fields)) => {
                    if let Some(s) = &mut self.stack_state {
                        s.set_expansion_done(object_id, fields);
                    }
                    done.push(object_id);
                }
                Ok(None) => {
                    dbg_log!("expand_object(0x{:X}) → None", object_id);
                    if let Some(s) = &mut self.stack_state {
                        s.set_expansion_failed(object_id, "Failed to resolve object".to_string());
                    }
                    done.push(object_id);
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    if let Some(s) = &mut self.stack_state {
                        s.set_expansion_failed(object_id, "Worker thread disconnected".to_string());
                    }
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

        let area = frame.area();

        // Carve out status bar at the bottom.
        let [main_area, status_area] =
            Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);

        // Split main area horizontally: 30% thread list, 70% stack view.
        let [list_area, stack_area] =
            Layout::horizontal([Constraint::Percentage(30), Constraint::Percentage(70)])
                .areas(main_area);

        // Store visible heights for PageUp/PageDown.
        self.thread_list_height = list_area.height.saturating_sub(2);
        if let Some(ref mut ss) = self.stack_state {
            ss.set_visible_height(stack_area.height.saturating_sub(2));
        }
        self.preview_stack_state
            .set_visible_height(stack_area.height.saturating_sub(2));

        // Thread list
        let list_focused = self.focus == Focus::ThreadList;
        frame.render_stateful_widget(
            SearchableList {
                focused: list_focused,
            },
            list_area,
            &mut self.thread_list,
        );

        // Stack view — use StackState if available, else create empty state
        let stack_focused = self.focus == Focus::StackFrames;
        if stack_focused {
            if let Some(ref mut ss) = self.stack_state {
                frame.render_stateful_widget(
                    StackView {
                        focused: stack_focused,
                    },
                    stack_area,
                    ss,
                );
            } else {
                frame.render_stateful_widget(
                    StackView {
                        focused: stack_focused,
                    },
                    stack_area,
                    &mut self.preview_stack_state,
                );
            }
        } else {
            frame.render_stateful_widget(
                StackView {
                    focused: stack_focused,
                },
                stack_area,
                &mut self.preview_stack_state,
            );
        }

        // Status bar — resolve selected thread once, use StatusBar widget.
        let selected_serial = self.thread_list.selected_serial();
        let selected_thread = selected_serial.and_then(|s| self.engine.select_thread(s));
        frame.render_widget(
            StatusBar {
                filename: &self.filename,
                thread_count: self.thread_count,
                selected: selected_thread.as_ref(),
                warning_count: self.warning_count + self.app_warnings.len(),
            },
            status_area,
        );
    }
}

/// RAII guard ensuring terminal cleanup on drop, even if a panic occurs.
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(
            io::stdout(),
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
mod tests {
    use hprof_engine::{
        CollectionPage, EntryInfo, FieldInfo, FieldValue, FrameInfo, LineNumber, NavigationEngine,
        ThreadInfo, ThreadState, VariableInfo, VariableValue,
    };
    use std::collections::HashMap;

    use super::*;

    struct StubEngine {
        threads: Vec<ThreadInfo>,
        frames: Vec<FrameInfo>,
        frames_by_thread: HashMap<u32, Vec<FrameInfo>>,
        vars_by_frame: HashMap<u64, Vec<VariableInfo>>,
        expand_results: HashMap<u64, Option<Vec<FieldInfo>>>,
    }

    impl StubEngine {
        fn with_threads(names: &[&str]) -> Self {
            Self {
                threads: names
                    .iter()
                    .enumerate()
                    .map(|(i, &n)| ThreadInfo {
                        thread_serial: (i + 1) as u32,
                        name: n.to_string(),
                        state: ThreadState::Unknown,
                    })
                    .collect(),
                frames: vec![],
                frames_by_thread: HashMap::new(),
                vars_by_frame: HashMap::new(),
                expand_results: HashMap::new(),
            }
        }

        fn with_threads_and_frames(names: &[&str], frames: Vec<FrameInfo>) -> Self {
            let mut s = Self::with_threads(names);
            s.frames = frames;
            s
        }

        fn with_thread_specific_frames(
            names: &[&str],
            by_thread: &[(u32, Vec<FrameInfo>)],
        ) -> Self {
            let mut s = Self::with_threads(names);
            s.frames_by_thread = by_thread
                .iter()
                .map(|(serial, frames)| (*serial, frames.clone()))
                .collect();
            s
        }

        fn with_vars(mut self, frame_id: u64, vars: Vec<VariableInfo>) -> Self {
            self.vars_by_frame.insert(frame_id, vars);
            self
        }

        fn with_expand(mut self, oid: u64, fields: Option<Vec<FieldInfo>>) -> Self {
            self.expand_results.insert(oid, fields);
            self
        }
    }

    impl NavigationEngine for StubEngine {
        fn warnings(&self) -> &[String] {
            &[]
        }
        fn list_threads(&self) -> Vec<ThreadInfo> {
            self.threads.clone()
        }
        fn select_thread(&self, serial: u32) -> Option<ThreadInfo> {
            self.threads
                .iter()
                .find(|t| t.thread_serial == serial)
                .cloned()
        }
        fn get_stack_frames(&self, thread_serial: u32) -> Vec<FrameInfo> {
            self.frames_by_thread
                .get(&thread_serial)
                .cloned()
                .unwrap_or_else(|| self.frames.clone())
        }
        fn get_local_variables(&self, frame_id: u64) -> Vec<VariableInfo> {
            self.vars_by_frame
                .get(&frame_id)
                .cloned()
                .unwrap_or_default()
        }
        fn expand_object(&self, oid: u64) -> Option<Vec<FieldInfo>> {
            if let Some(result) = self.expand_results.get(&oid) {
                return result.clone();
            }
            Some(vec![
                FieldInfo {
                    name: "x".to_string(),
                    value: FieldValue::Int(42),
                },
                FieldInfo {
                    name: "child".to_string(),
                    value: FieldValue::ObjectRef {
                        id: 999,
                        class_name: "Child".to_string(),
                        entry_count: None,
                        inline_value: None,
                    },
                },
            ])
        }
        fn get_page(
            &self,
            collection_id: u64,
            offset: usize,
            limit: usize,
        ) -> Option<CollectionPage> {
            match collection_id {
                888 => {
                    // Int entries
                    let total: u64 = 250;
                    let end = (offset + limit).min(total as usize);
                    let entries = (offset..end)
                        .map(|i| EntryInfo {
                            index: i,
                            key: None,
                            value: FieldValue::Int(i as i32),
                        })
                        .collect();
                    Some(CollectionPage {
                        entries,
                        total_count: total,
                        offset,
                        has_more: end < total as usize,
                    })
                }
                889 => {
                    // ObjectRef entries — used for AC10 expansion tests.
                    // Entry 0 has value ObjectRef(id=700), entry 1 etc.
                    let total: u64 = 3;
                    let end = (offset + limit).min(total as usize);
                    let entries = (offset..end)
                        .map(|i| EntryInfo {
                            index: i,
                            key: None,
                            value: FieldValue::ObjectRef {
                                id: 700 + i as u64,
                                class_name: "SomeItem".to_string(),
                                entry_count: None,
                                inline_value: None,
                            },
                        })
                        .collect();
                    Some(CollectionPage {
                        entries,
                        total_count: total,
                        offset,
                        has_more: end < total as usize,
                    })
                }
                _ => None,
            }
        }
        fn resolve_string(&self, _: u64) -> Option<String> {
            Some("value".to_string())
        }
        fn memory_used(&self) -> usize {
            0
        }
        fn memory_budget(&self) -> u64 {
            u64::MAX
        }
    }

    fn make_frame(frame_id: u64) -> FrameInfo {
        FrameInfo {
            frame_id,
            method_name: "run".to_string(),
            class_name: "Thread".to_string(),
            source_file: "Thread.java".to_string(),
            line: LineNumber::Line(1),
            has_variables: false,
        }
    }

    fn make_obj_var(index: usize, object_id: u64) -> VariableInfo {
        VariableInfo {
            index,
            value: VariableValue::ObjectRef {
                id: object_id,
                class_name: "Object".to_string(),
            },
        }
    }

    #[test]
    fn app_new_builds_without_panic_with_zero_threads() {
        let engine = StubEngine::with_threads(&[]);
        let app = App::new(engine, "test.hprof".to_string());
        assert_eq!(app.focus, Focus::ThreadList);
        assert_eq!(app.thread_list.selected_serial(), None);
        assert_eq!(app.thread_count, 0);
    }

    #[test]
    fn app_new_builds_without_panic_with_three_threads() {
        let engine = StubEngine::with_threads(&["main", "worker-1", "worker-2"]);
        let app = App::new(engine, "test.hprof".to_string());
        assert_eq!(app.thread_list.selected_serial(), Some(1));
        assert_eq!(app.thread_count, 3);
    }

    #[test]
    fn handle_input_down_in_thread_list_updates_selection() {
        let engine = StubEngine::with_threads(&["main", "worker-1", "worker-2"]);
        let mut app = App::new(engine, "test.hprof".to_string());
        app.handle_input(InputEvent::Down);
        assert_eq!(app.thread_list.selected_serial(), Some(2));
    }

    #[test]
    fn handle_input_search_activate_sets_search_active() {
        let engine = StubEngine::with_threads(&["main"]);
        let mut app = App::new(engine, "test.hprof".to_string());
        app.handle_input(InputEvent::SearchActivate);
        assert!(app.thread_list.is_search_active());
    }

    #[test]
    fn handle_input_search_char_appends_to_filter_query() {
        let engine = StubEngine::with_threads(&["main", "worker"]);
        let mut app = App::new(engine, "test.hprof".to_string());
        app.handle_input(InputEvent::SearchActivate);
        app.handle_input(InputEvent::SearchChar('w'));
        app.handle_input(InputEvent::SearchChar('o'));
        assert_eq!(app.thread_list.filter(), "wo");
        assert_eq!(app.thread_list.filtered_count(), 1);
    }

    #[test]
    fn handle_input_escape_in_search_mode_clears_filter_and_deactivates() {
        let engine = StubEngine::with_threads(&["main", "worker"]);
        let mut app = App::new(engine, "test.hprof".to_string());
        app.handle_input(InputEvent::SearchActivate);
        app.handle_input(InputEvent::SearchChar('w'));
        app.handle_input(InputEvent::Escape);
        assert!(!app.thread_list.is_search_active());
        assert_eq!(app.thread_list.filtered_count(), 2);
    }

    #[test]
    fn handle_input_enter_in_thread_list_loads_frames_and_transitions_to_stack_frames() {
        let frames = vec![make_frame(10), make_frame(20)];
        let engine = StubEngine::with_threads_and_frames(&["main"], frames);
        let mut app = App::new(engine, "test.hprof".to_string());
        app.handle_input(InputEvent::Enter);
        assert_eq!(app.focus, Focus::StackFrames);
        let ss = app.stack_state.as_ref().expect("stack_state must be Some");
        assert_eq!(ss.selected_frame_id(), Some(10));
    }

    #[test]
    fn handle_input_up_down_in_stack_frames_moves_cursor() {
        let frames = vec![make_frame(10), make_frame(20), make_frame(30)];
        let engine = StubEngine::with_threads_and_frames(&["main"], frames);
        let mut app = App::new(engine, "test.hprof".to_string());
        app.handle_input(InputEvent::Enter); // → StackFrames
        app.handle_input(InputEvent::Down);
        assert_eq!(
            app.stack_state.as_ref().unwrap().selected_frame_id(),
            Some(20)
        );
        app.handle_input(InputEvent::Up);
        assert_eq!(
            app.stack_state.as_ref().unwrap().selected_frame_id(),
            Some(10)
        );
    }

    #[test]
    fn handle_input_enter_in_stack_frames_expands_then_collapses() {
        let frames = vec![make_frame(10)];
        let engine = StubEngine::with_threads_and_frames(&["main"], frames);
        let mut app = App::new(engine, "test.hprof".to_string());
        app.handle_input(InputEvent::Enter); // → StackFrames
        // Enter on collapsed frame → expands
        app.handle_input(InputEvent::Enter);
        assert!(app.stack_state.as_ref().unwrap().is_expanded(10));
        // Enter on expanded frame → collapses
        app.handle_input(InputEvent::Enter);
        assert!(!app.stack_state.as_ref().unwrap().is_expanded(10));
    }

    #[test]
    fn handle_input_escape_in_stack_frames_clears_state_and_returns_to_thread_list() {
        let frames = vec![make_frame(10)];
        let engine = StubEngine::with_threads_and_frames(&["main"], frames);
        let mut app = App::new(engine, "test.hprof".to_string());
        app.handle_input(InputEvent::Enter);
        assert_eq!(app.focus, Focus::StackFrames);
        app.handle_input(InputEvent::Escape);
        assert_eq!(app.focus, Focus::ThreadList);
        assert!(app.stack_state.is_none());
    }

    #[test]
    fn handle_input_quit_returns_app_action_quit() {
        let engine = StubEngine::with_threads(&["main"]);
        let mut app = App::new(engine, "test.hprof".to_string());
        assert_eq!(app.handle_input(InputEvent::Quit), AppAction::Quit);
    }

    #[test]
    fn app_new_captures_thread_count_without_repeated_list_calls() {
        let engine = StubEngine::with_threads(&["a", "b", "c"]);
        let app = App::new(engine, "x.hprof".to_string());
        assert_eq!(app.thread_count, 3);
    }

    #[test]
    fn handle_input_enter_with_no_selected_thread_does_not_transition() {
        let engine = StubEngine::with_threads(&[]);
        let mut app = App::new(engine, "test.hprof".to_string());
        app.handle_input(InputEvent::Enter);
        assert_eq!(app.focus, Focus::ThreadList);
        assert!(app.stack_state.is_none());
    }

    #[test]
    fn stack_state_is_none_on_construction() {
        let engine = StubEngine::with_threads(&["main"]);
        let app = App::new(engine, "test.hprof".to_string());
        assert!(app.stack_state.is_none());
    }

    #[test]
    fn app_new_initializes_stack_preview_for_selected_thread() {
        let engine = StubEngine::with_thread_specific_frames(
            &["main", "worker"],
            &[(1, vec![make_frame(10)]), (2, vec![make_frame(20)])],
        );
        let app = App::new(engine, "test.hprof".to_string());
        assert_eq!(app.focus, Focus::ThreadList);
        assert!(app.stack_state.is_none());
        assert_eq!(app.preview_stack_state.selected_frame_id(), Some(10));
    }

    #[test]
    fn moving_thread_selection_updates_stack_preview_without_enter() {
        let engine = StubEngine::with_thread_specific_frames(
            &["main", "worker"],
            &[(1, vec![make_frame(10)]), (2, vec![make_frame(20)])],
        );
        let mut app = App::new(engine, "test.hprof".to_string());
        app.handle_input(InputEvent::Down);
        assert_eq!(app.focus, Focus::ThreadList);
        assert!(app.stack_state.is_none());
        assert_eq!(app.preview_stack_state.selected_frame_id(), Some(20));
    }

    #[test]
    fn variable_value_variants_accessible_via_hprof_engine() {
        let v = VariableValue::Null;
        assert_eq!(v, VariableValue::Null);
    }

    // --- Task 9: async expansion machinery tests ---

    #[test]
    fn start_object_expansion_sets_loading_state() {
        let frames = vec![make_frame(10)];
        let vars = vec![make_obj_var(0, 42)];
        let engine = StubEngine::with_threads_and_frames(&["main"], frames).with_vars(10, vars);
        let mut app = App::new(engine, "test.hprof".to_string());
        // Enter StackFrames, expand frame 10, then move down to the ObjectRef var.
        app.handle_input(InputEvent::Enter); // → StackFrames, OnFrame(0)
        app.handle_input(InputEvent::Enter); // expand frame 10, cursor stays OnFrame(0)
        app.handle_input(InputEvent::Down); // → OnVar{0,0} (ObjectRef 42)
        app.handle_input(InputEvent::Enter); // start_object_expansion(42)
        assert_eq!(
            app.stack_state.as_ref().unwrap().expansion_state(42),
            ExpansionPhase::Loading
        );
    }

    #[test]
    fn poll_expansions_completes_and_moves_to_expanded() {
        let frames = vec![make_frame(10)];
        let vars = vec![make_obj_var(0, 42)];
        let engine = StubEngine::with_threads_and_frames(&["main"], frames).with_vars(10, vars);
        let mut app = App::new(engine, "test.hprof".to_string());
        app.handle_input(InputEvent::Enter); // → StackFrames
        app.handle_input(InputEvent::Enter); // expand frame 10
        app.handle_input(InputEvent::Down); // → OnVar{0,0}
        app.handle_input(InputEvent::Enter); // start expansion
        // Poll until the worker thread finishes (StubEngine is synchronous so it's fast).
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !app.pending_expansions.is_empty() && std::time::Instant::now() < deadline {
            app.poll_expansions();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(
            app.stack_state.as_ref().unwrap().expansion_state(42),
            ExpansionPhase::Expanded
        );
    }

    // --- Task 6.4: nested expansion tests ---

    #[test]
    fn enter_on_nested_object_field_starts_expansion() {
        // StubEngine.expand_object returns a field "child" ObjectRef(999)
        let frames = vec![make_frame(10)];
        let vars = vec![make_obj_var(0, 42)];
        let engine = StubEngine::with_threads_and_frames(&["main"], frames).with_vars(10, vars);
        let mut app = App::new(engine, "test.hprof".to_string());
        app.handle_input(InputEvent::Enter); // → StackFrames
        app.handle_input(InputEvent::Enter); // expand frame 10
        app.handle_input(InputEvent::Down); // → OnVar{0,0}
        app.handle_input(InputEvent::Enter); // start expansion of object 42
        // Poll until complete
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !app.pending_expansions.is_empty() && std::time::Instant::now() < deadline {
            app.poll_expansions();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        // Object 42 expanded, has "child" field (ObjectRef 999) at index 1
        assert_eq!(
            app.stack_state.as_ref().unwrap().expansion_state(42),
            ExpansionPhase::Expanded
        );
        // Navigate down to the "child" field (index 1 in flat list: field_path=[1])
        app.handle_input(InputEvent::Down); // → OnObjectField{0,0,[0]} (field "x")
        app.handle_input(InputEvent::Down); // → OnObjectField{0,0,[1]} (field "child" = ObjectRef 999)
        app.handle_input(InputEvent::Enter); // start nested expansion of 999
        assert_eq!(
            app.stack_state.as_ref().unwrap().expansion_state(999),
            ExpansionPhase::Loading
        );
    }

    #[test]
    fn collapse_object_recursive_called_on_enter_for_expanded_root_obj() {
        let frames = vec![make_frame(10)];
        let vars = vec![make_obj_var(0, 42)];
        let engine = StubEngine::with_threads_and_frames(&["main"], frames).with_vars(10, vars);
        let mut app = App::new(engine, "test.hprof".to_string());
        app.handle_input(InputEvent::Enter); // → StackFrames
        app.handle_input(InputEvent::Enter); // expand frame 10
        app.handle_input(InputEvent::Down); // → OnVar{0,0}
        app.handle_input(InputEvent::Enter); // start expansion
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !app.pending_expansions.is_empty() && std::time::Instant::now() < deadline {
            app.poll_expansions();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        // Now collapse via Enter on OnVar (expanded state)
        app.handle_input(InputEvent::Enter); // CollapseObj(42)
        assert_eq!(
            app.stack_state.as_ref().unwrap().expansion_state(42),
            ExpansionPhase::Collapsed
        );
    }

    #[test]
    fn enter_twice_on_nested_object_field_collapses_it() {
        // StubEngine.expand_object always returns [x:Int, child:ObjectRef(999)]
        let frames = vec![make_frame(10)];
        let vars = vec![make_obj_var(0, 42)];
        let engine = StubEngine::with_threads_and_frames(&["main"], frames).with_vars(10, vars);
        let mut app = App::new(engine, "test.hprof".to_string());
        app.handle_input(InputEvent::Enter); // → StackFrames
        app.handle_input(InputEvent::Enter); // expand frame 10
        app.handle_input(InputEvent::Down); // → OnVar{0,0}
        app.handle_input(InputEvent::Enter); // start expansion of object 42
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !app.pending_expansions.is_empty() && std::time::Instant::now() < deadline {
            app.poll_expansions();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        // Navigate to "child" field at path [1] (ObjectRef 999)
        app.handle_input(InputEvent::Down); // → OnObjectField{0,0,[0]} ("x")
        app.handle_input(InputEvent::Down); // → OnObjectField{0,0,[1]} ("child" = 999)
        app.handle_input(InputEvent::Enter); // start nested expansion of 999
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !app.pending_expansions.is_empty() && std::time::Instant::now() < deadline {
            app.poll_expansions();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(
            app.stack_state.as_ref().unwrap().expansion_state(999),
            ExpansionPhase::Expanded
        );
        // Enter again on the same field → CollapseNestedObj(999)
        app.handle_input(InputEvent::Enter);
        assert_eq!(
            app.stack_state.as_ref().unwrap().expansion_state(999),
            ExpansionPhase::Collapsed
        );
    }

    #[test]
    fn escape_on_loading_node_cancels_expansion_without_leaving_stack_frames() {
        let frames = vec![make_frame(10)];
        let vars = vec![make_obj_var(0, 42)];
        let engine = StubEngine::with_threads_and_frames(&["main"], frames).with_vars(10, vars);
        let mut app = App::new(engine, "test.hprof".to_string());
        app.handle_input(InputEvent::Enter); // → StackFrames
        app.handle_input(InputEvent::Enter); // expand frame 10
        app.handle_input(InputEvent::Down); // → OnVar{0,0}
        app.handle_input(InputEvent::Enter); // start expansion → Loading
        app.handle_input(InputEvent::Down); // → OnObjectLoadingNode{0,0}
        app.handle_input(InputEvent::Escape); // cancel expansion (not go-back)
        assert_eq!(
            app.stack_state.as_ref().unwrap().expansion_state(42),
            ExpansionPhase::Collapsed
        );
        // Focus must remain in StackFrames.
        assert_eq!(app.focus, Focus::StackFrames);
    }

    // --- Collection pagination tests ---

    /// Builds an App with a frame that has one var
    /// (object 42) whose expand returns a field "items"
    /// = ObjectRef(888, ArrayList, entry_count=ec).
    /// StubEngine.get_page(888, ..) returns test entries.
    fn make_collection_app(ec: u64) -> App<StubEngine> {
        let frames = vec![{
            let mut f = make_frame(10);
            f.has_variables = true;
            f
        }];
        let vars = vec![make_obj_var(0, 42)];
        let expand_fields = Some(vec![FieldInfo {
            name: "items".to_string(),
            value: FieldValue::ObjectRef {
                id: 888,
                class_name: "java.util.ArrayList".to_string(),
                entry_count: Some(ec),
                inline_value: None,
            },
        }]);
        let engine = StubEngine::with_threads_and_frames(&["main"], frames)
            .with_vars(10, vars)
            .with_expand(42, expand_fields);
        App::new(engine, "test.hprof".to_string())
    }

    /// Navigate from thread list into the collection
    /// field. Returns app positioned on the "items"
    /// ObjectField.
    fn nav_to_collection_field(app: &mut App<StubEngine>) {
        app.handle_input(InputEvent::Enter); // StackFrames
        app.handle_input(InputEvent::Enter); // expand frame
        app.handle_input(InputEvent::Down); // → OnVar
        app.handle_input(InputEvent::Enter); // expand obj 42
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !app.pending_expansions.is_empty() && std::time::Instant::now() < deadline {
            app.poll_expansions();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        app.handle_input(InputEvent::Down); // → items field
    }

    fn poll_all_pages(app: &mut App<StubEngine>) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !app.pending_pages.is_empty() && std::time::Instant::now() < deadline {
            let _fallbacks = app.poll_pages();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    #[test]
    fn collection_enter_triggers_get_page_not_expand() {
        let mut app = make_collection_app(250);
        nav_to_collection_field(&mut app);
        // Enter on collection field → StartCollection
        app.handle_input(InputEvent::Enter);
        // Should have pending_pages, not pending_expansions
        // for collection 888.
        assert!(
            !app.pending_pages.is_empty(),
            "collection load should be pending"
        );
        assert!(
            !app.pending_expansions.contains_key(&888),
            "should NOT use expand_object for collection"
        );
    }

    #[test]
    fn collection_small_no_chunk_sections() {
        let mut app = make_collection_app(50);
        nav_to_collection_field(&mut app);
        app.handle_input(InputEvent::Enter);
        poll_all_pages(&mut app);
        let ss = app.stack_state.as_ref().unwrap();
        let cc = ss.collection_chunks.get(&888).unwrap();
        assert!(cc.eager_page.is_some());
        assert_eq!(cc.eager_page.as_ref().unwrap().entries.len(), 50);
        assert!(
            cc.chunk_pages.is_empty(),
            "<= 100 entries → no chunk sections"
        );
    }

    #[test]
    fn collection_250_chunk_layout() {
        let mut app = make_collection_app(250);
        nav_to_collection_field(&mut app);
        app.handle_input(InputEvent::Enter);
        poll_all_pages(&mut app);
        let ss = app.stack_state.as_ref().unwrap();
        let cc = ss.collection_chunks.get(&888).unwrap();
        // Eager page: 100 entries.
        assert_eq!(cc.eager_page.as_ref().unwrap().entries.len(), 100);
        // Chunk sections: [100..199], [200..249].
        let ranges = compute_chunk_ranges(250);
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0], (100, 100));
        assert_eq!(ranges[1], (200, 50));
        // All chunk sections start as Collapsed.
        for (offset, _) in &ranges {
            assert!(matches!(
                cc.chunk_pages.get(offset),
                Some(ChunkState::Collapsed)
            ));
        }
    }

    #[test]
    fn collection_3000_chunk_layout() {
        let ranges = compute_chunk_ranges(3000);
        // 9 sections of 100 + 2 sections of 1000
        assert_eq!(ranges.len(), 11);
        assert_eq!(ranges[0], (100, 100));
        assert_eq!(ranges[8], (900, 100));
        assert_eq!(ranges[9], (1000, 1000));
        assert_eq!(ranges[10], (2000, 1000));
    }

    #[test]
    fn collection_2348_last_chunk_truncated() {
        let ranges = compute_chunk_ranges(2348);
        let last = ranges.last().unwrap();
        assert_eq!(*last, (2000, 348));
    }

    #[test]
    fn chunk_section_enter_loads_correct_range() {
        let mut app = make_collection_app(250);
        nav_to_collection_field(&mut app);
        app.handle_input(InputEvent::Enter);
        poll_all_pages(&mut app);
        // Navigate down past eager entries to first chunk
        // section: 100 entries + 1 to pass them.
        for _ in 0..101 {
            app.handle_input(InputEvent::Down);
        }
        let ss = app.stack_state.as_ref().unwrap();
        // Should be on first chunk section [100..199].
        assert!(matches!(
            ss.cursor(),
            StackCursor::OnChunkSection {
                chunk_offset: 100,
                ..
            }
        ));
        // Enter on chunk → LoadChunk(888, 100, 100).
        app.handle_input(InputEvent::Enter);
        assert!(
            app.pending_pages.contains_key(&(888, 100)),
            "chunk load should be pending"
        );
        poll_all_pages(&mut app);
        let ss = app.stack_state.as_ref().unwrap();
        let cc = ss.collection_chunks.get(&888).unwrap();
        assert!(matches!(
            cc.chunk_pages.get(&100),
            Some(ChunkState::Loaded(_))
        ));
    }

    #[test]
    fn chunk_loading_indicator() {
        let mut app = make_collection_app(250);
        nav_to_collection_field(&mut app);
        app.handle_input(InputEvent::Enter);
        poll_all_pages(&mut app);
        // Navigate to first chunk.
        for _ in 0..101 {
            app.handle_input(InputEvent::Down);
        }
        app.handle_input(InputEvent::Enter);
        // Before polling, chunk should be Loading.
        let ss = app.stack_state.as_ref().unwrap();
        let cc = ss.collection_chunks.get(&888).unwrap();
        assert!(matches!(
            cc.chunk_pages.get(&100),
            Some(ChunkState::Loading)
        ));
    }

    #[test]
    fn escape_collapses_collection() {
        let mut app = make_collection_app(250);
        nav_to_collection_field(&mut app);
        app.handle_input(InputEvent::Enter);
        poll_all_pages(&mut app);
        // Move into collection entries.
        app.handle_input(InputEvent::Down);
        let ss = app.stack_state.as_ref().unwrap();
        assert!(matches!(ss.cursor(), StackCursor::OnCollectionEntry { .. }));
        // Escape → collapse collection.
        app.handle_input(InputEvent::Escape);
        let ss = app.stack_state.as_ref().unwrap();
        assert!(
            ss.collection_chunks.get(&888).is_none(),
            "collection should be removed"
        );
        // Cursor returns to the collection field.
        assert!(matches!(ss.cursor(), StackCursor::OnObjectField { .. }));
        // Focus stays in StackFrames.
        assert_eq!(app.focus, Focus::StackFrames);
    }

    #[test]
    fn escape_from_chunk_section_collapses_collection() {
        let mut app = make_collection_app(250);
        nav_to_collection_field(&mut app);
        app.handle_input(InputEvent::Enter);
        poll_all_pages(&mut app);
        // Navigate to first chunk section (past 100 eager entries + 1 entry node).
        for _ in 0..101 {
            app.handle_input(InputEvent::Down);
        }
        let ss = app.stack_state.as_ref().unwrap();
        assert!(
            matches!(ss.cursor(), StackCursor::OnChunkSection { .. }),
            "should be on chunk section, got {:?}",
            ss.cursor()
        );
        // Escape from chunk section should collapse the collection.
        app.handle_input(InputEvent::Escape);
        let ss = app.stack_state.as_ref().unwrap();
        assert!(
            ss.collection_chunks.get(&888).is_none(),
            "collection should be removed after escape from chunk section"
        );
        assert!(matches!(ss.cursor(), StackCursor::OnObjectField { .. }));
    }

    #[test]
    fn unsupported_type_falls_back_to_expand_object() {
        // Use collection ID 777 which StubEngine.get_page
        // returns None for.
        let frames = vec![{
            let mut f = make_frame(10);
            f.has_variables = true;
            f
        }];
        let vars = vec![make_obj_var(0, 42)];
        let expand_fields = Some(vec![FieldInfo {
            name: "tree".to_string(),
            value: FieldValue::ObjectRef {
                id: 777,
                class_name: "java.util.TreeMap".to_string(),
                entry_count: Some(50),
                inline_value: None,
            },
        }]);
        let engine = StubEngine::with_threads_and_frames(&["main"], frames)
            .with_vars(10, vars)
            .with_expand(42, expand_fields);
        let mut app = App::new(engine, "test.hprof".to_string());
        nav_to_collection_field(&mut app);
        app.handle_input(InputEvent::Enter);
        // poll_pages will get None and fall back to
        // expand_object.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while (!app.pending_pages.is_empty() || !app.pending_expansions.is_empty())
            && std::time::Instant::now() < deadline
        {
            let fallbacks = app.poll_pages();
            for cid in fallbacks {
                app.start_object_expansion(cid);
            }
            app.poll_expansions();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let ss = app.stack_state.as_ref().unwrap();
        // Collection chunks should be gone.
        assert!(ss.collection_chunks.get(&777).is_none());
        // Should have fallen back to expand_object →
        // expansion state should be Expanded.
        assert_eq!(ss.expansion_state(777), ExpansionPhase::Expanded);
    }

    #[test]
    fn re_enter_on_loaded_chunk_toggles_collapse() {
        let mut app = make_collection_app(250);
        nav_to_collection_field(&mut app);
        app.handle_input(InputEvent::Enter);
        poll_all_pages(&mut app);
        // Navigate to first chunk, load it.
        for _ in 0..101 {
            app.handle_input(InputEvent::Down);
        }
        app.handle_input(InputEvent::Enter);
        poll_all_pages(&mut app);
        // Chunk is now Loaded.
        let ss = app.stack_state.as_ref().unwrap();
        assert!(matches!(
            ss.collection_chunks
                .get(&888)
                .unwrap()
                .chunk_pages
                .get(&100),
            Some(ChunkState::Loaded(_))
        ));
        // Enter again → ToggleChunk → Collapsed.
        app.handle_input(InputEvent::Enter);
        let ss = app.stack_state.as_ref().unwrap();
        assert!(matches!(
            ss.collection_chunks
                .get(&888)
                .unwrap()
                .chunk_pages
                .get(&100),
            Some(ChunkState::Collapsed)
        ));
    }

    #[test]
    fn entry_rendering_map_vs_list_format() {
        use crate::views::stack_view::StackState;
        // Verify format_entry_line for map entry.
        let map_entry = EntryInfo {
            index: 5,
            key: Some(FieldValue::Int(42)),
            value: FieldValue::Int(100),
        };
        let line = StackState::format_entry_line(&map_entry, "  ", None);
        assert!(line.contains("[5] 42 => 100"), "map entry format: {}", line);
        // List entry.
        let list_entry = EntryInfo {
            index: 3,
            key: None,
            value: FieldValue::Int(77),
        };
        let line = StackState::format_entry_line(&list_entry, "  ", None);
        assert!(line.contains("[3] 77"), "list entry format: {}", line);
        assert!(
            !line.contains("=>"),
            "list entry should not have =>: {}",
            line
        );
        // ObjectRef value shows "+" toggle when collapsed.
        let obj_entry = EntryInfo {
            index: 0,
            key: None,
            value: FieldValue::ObjectRef {
                id: 999,
                class_name: "java.lang.String".to_string(),
                entry_count: None,
                inline_value: None,
            },
        };
        let line_collapsed = StackState::format_entry_line(
            &obj_entry,
            "  ",
            Some(&crate::views::stack_view::ExpansionPhase::Collapsed),
        );
        assert!(
            line_collapsed.contains("+ [0]") && line_collapsed.contains("String"),
            "ObjectRef collapsed should show '+ [0] ...': {}",
            line_collapsed
        );
        let line_expanded = StackState::format_entry_line(
            &obj_entry,
            "  ",
            Some(&crate::views::stack_view::ExpansionPhase::Expanded),
        );
        assert!(
            line_expanded.contains("- [0]") && line_expanded.contains("String"),
            "ObjectRef expanded should show '- [0] ...': {}",
            line_expanded
        );
    }

    /// Builds an App with collection 889 (entries are ObjectRef values).
    fn make_obj_entry_collection_app() -> App<StubEngine> {
        let frames = vec![{
            let mut f = make_frame(10);
            f.has_variables = true;
            f
        }];
        let vars = vec![make_obj_var(0, 42)];
        let expand_fields = Some(vec![FieldInfo {
            name: "items".to_string(),
            value: FieldValue::ObjectRef {
                id: 889,
                class_name: "java.util.ArrayList".to_string(),
                entry_count: Some(3),
                inline_value: None,
            },
        }]);
        let engine = StubEngine::with_threads_and_frames(&["main"], frames)
            .with_vars(10, vars)
            .with_expand(42, expand_fields);
        App::new(engine, "test.hprof".to_string())
    }

    #[test]
    fn collection_entry_objectref_shows_plus_prefix() {
        let mut app = make_obj_entry_collection_app();
        // Navigate to collection field "items" (collection 889).
        let frames = vec![{
            let mut f = make_frame(10);
            f.has_variables = true;
            f
        }];
        let _ = frames; // already built in app
        nav_to_collection_field(&mut app);
        app.handle_input(InputEvent::Enter); // start collection load
        poll_all_pages(&mut app);
        // Navigate down to first entry (index 0).
        app.handle_input(InputEvent::Down);
        let ss = app.stack_state.as_ref().unwrap();
        assert!(
            matches!(
                ss.cursor(),
                StackCursor::OnCollectionEntry { entry_index: 0, .. }
            ),
            "should be on entry 0, got {:?}",
            ss.cursor()
        );
        // The entry rendering is verified through format_entry_line above;
        // here we verify Enter triggers start_object_expansion.
        app.handle_input(InputEvent::Enter);
        // pending_expansions should contain ObjectRef id 700 (entry 0's value).
        assert!(
            app.pending_expansions.contains_key(&700),
            "entering on ObjectRef entry should start expansion of id 700"
        );
    }

    #[test]
    fn collection_entry_objectref_expanded_fields_appear_in_tree() {
        let mut app = make_obj_entry_collection_app();
        nav_to_collection_field(&mut app);
        app.handle_input(InputEvent::Enter);
        poll_all_pages(&mut app);
        // Navigate to entry 0 and expand it.
        app.handle_input(InputEvent::Down);
        app.handle_input(InputEvent::Enter); // start expand of id=700
        // Poll until expansion done.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !app.pending_expansions.is_empty() && std::time::Instant::now() < deadline {
            app.poll_expansions();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        // Navigate down — should enter OnCollectionEntryObjField.
        app.handle_input(InputEvent::Down);
        let ss = app.stack_state.as_ref().unwrap();
        assert!(
            matches!(
                ss.cursor(),
                StackCursor::OnCollectionEntryObjField { entry_index: 0, .. }
            ),
            "after expanding entry 0, down should reach OnCollectionEntryObjField, \
             got {:?}",
            ss.cursor()
        );
    }

    #[test]
    fn page_up_down_scrolls_tree_by_visible_height() {
        // This is a general tree scroll test, not
        // collection-specific.
        use crate::views::stack_view::StackState;
        let frames: Vec<_> = (1..=30).map(|i| make_frame(i)).collect();
        let mut state = StackState::new(frames);
        state.set_visible_height(10);
        // Move to frame 5.
        for _ in 0..5 {
            state.move_down();
        }
        assert_eq!(*state.cursor(), StackCursor::OnFrame(5));
        state.move_page_down();
        assert_eq!(*state.cursor(), StackCursor::OnFrame(15));
        state.move_page_up();
        assert_eq!(*state.cursor(), StackCursor::OnFrame(5));
    }
}
