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
use hprof_engine::{FieldInfo, NavigationEngine};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
};

use crate::{
    input::{self, InputEvent},
    views::{
        stack_view::{ExpansionPhase, StackCursor, StackState, StackView},
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
    /// In-flight string load receivers keyed by `string object_id`.
    pending_strings: HashMap<u64, Receiver<Option<String>>>,
    /// Warnings accumulated during the session (e.g. unresolved string backing arrays).
    app_warnings: Vec<String>,
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
            pending_strings: HashMap::new(),
            app_warnings: Vec::new(),
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
                // If cursor is on a loading node, cancel that expansion instead of going back.
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
            InputEvent::Enter => {
                // Collect the intended command from an immutable borrow, then act on it.
                enum Cmd {
                    CollapseFrame(u64),
                    ExpandFrame(u64),
                    StartObj(u64),
                    CollapseObj(u64),
                    StartNestedObj(u64),
                    CollapseNestedObj(u64),
                    LoadString(u64),
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
                            match s.expansion_state(oid) {
                                ExpansionPhase::Collapsed | ExpansionPhase::Failed => {
                                    Cmd::StartObj(oid)
                                }
                                ExpansionPhase::Expanded => Cmd::CollapseObj(oid),
                                ExpansionPhase::Loading => return None, // no-op
                            }
                        }
                        StackCursor::OnObjectField { .. } => {
                            if let Some(sid) = s.selected_field_string_id() {
                                return Some(Cmd::LoadString(sid));
                            }
                            let nested_id = s.selected_field_ref_id()?;
                            match s.expansion_state(nested_id) {
                                ExpansionPhase::Collapsed | ExpansionPhase::Failed => {
                                    Cmd::StartNestedObj(nested_id)
                                }
                                ExpansionPhase::Expanded => Cmd::CollapseNestedObj(nested_id),
                                ExpansionPhase::Loading => return None, // no-op
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
                            for sid in s.string_ids_in_subtree(oid) {
                                self.pending_strings.remove(&sid);
                            }
                            s.collapse_object_recursive(oid);
                        }
                    }
                    Some(Cmd::StartNestedObj(oid)) => self.start_object_expansion(oid),
                    Some(Cmd::CollapseNestedObj(oid)) => {
                        self.pending_expansions.remove(&oid);
                        if let Some(s) = &mut self.stack_state {
                            for sid in s.string_ids_in_subtree(oid) {
                                self.pending_strings.remove(&sid);
                            }
                            s.collapse_object_recursive(oid);
                        }
                    }
                    Some(Cmd::LoadString(sid)) => self.start_string_loading(sid),
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

    /// Spawns a worker thread to load the string value for `string_id`.
    ///
    /// If `string_id` is already pending, this is a no-op (AC4).
    fn start_string_loading(&mut self, string_id: u64)
    where
        E: Send + Sync + 'static,
    {
        if self.pending_strings.contains_key(&string_id) {
            return;
        }
        let Some(s) = &mut self.stack_state else {
            return;
        };
        s.start_string_loading(string_id);
        let (tx, rx) = mpsc::channel();
        let engine = Arc::clone(&self.engine);
        std::thread::spawn(move || {
            let result = engine.resolve_string(string_id);
            let _ = tx.send(result);
        });
        self.pending_strings.insert(string_id, rx);
    }

    /// Polls all in-flight string load receivers and updates `StackState`.
    ///
    /// `None` results emit a warning (unresolved backing array).
    pub fn poll_strings(&mut self) {
        let mut done = Vec::new();
        for (&string_id, rx) in &self.pending_strings {
            match rx.try_recv() {
                Ok(Some(val)) => {
                    if let Some(s) = &mut self.stack_state {
                        s.set_string_loaded(string_id, val);
                    }
                    done.push(string_id);
                }
                Ok(None) => {
                    if let Some(s) = &mut self.stack_state {
                        s.set_string_failed(string_id, "unresolved".to_string());
                    }
                    self.app_warnings
                        .push(format!("String 0x{:X}: backing array not found", string_id));
                    done.push(string_id);
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    if let Some(s) = &mut self.stack_state {
                        s.set_string_failed(string_id, "Worker thread disconnected".to_string());
                    }
                    done.push(string_id);
                }
            }
        }
        for id in done {
            self.pending_strings.remove(&id);
        }
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
    pub fn render(&mut self, frame: &mut ratatui::Frame) {
        self.poll_expansions();
        self.poll_strings();

        let area = frame.area();

        // Carve out status bar at the bottom.
        let [main_area, status_area] =
            Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);

        // Split main area horizontally: 30% thread list, 70% stack view.
        let [list_area, stack_area] =
            Layout::horizontal([Constraint::Percentage(30), Constraint::Percentage(70)])
                .areas(main_area);

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
        EntryInfo, FieldInfo, FieldValue, FrameInfo, LineNumber, NavigationEngine, ThreadInfo,
        ThreadState, VariableInfo, VariableValue,
    };
    use std::collections::HashMap;

    use super::*;

    struct StubEngine {
        threads: Vec<ThreadInfo>,
        frames: Vec<FrameInfo>,
        frames_by_thread: HashMap<u32, Vec<FrameInfo>>,
        vars_by_frame: HashMap<u64, Vec<VariableInfo>>,
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
        fn expand_object(&self, _: u64) -> Option<Vec<FieldInfo>> {
            Some(vec![
                FieldInfo {
                    name: "x".to_string(),
                    value: FieldValue::Int(42),
                },
                FieldInfo {
                    name: "child".to_string(),
                    value: FieldValue::ObjectRef {
                        id: 999,
                        class_name: "java.util.ArrayList".to_string(),
                        entry_count: Some(3),
                    },
                },
            ])
        }
        fn get_page(&self, _: u64, _: usize, _: usize) -> Vec<EntryInfo> {
            vec![]
        }
        fn resolve_string(&self, _: u64) -> Option<String> {
            Some("value".to_string())
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

    // --- Task 6: async string loading tests ---

    fn make_string_ref_field(name: &str, string_id: u64) -> FieldInfo {
        FieldInfo {
            name: name.to_string(),
            value: FieldValue::StringRef { id: string_id },
        }
    }

    #[test]
    fn enter_on_string_ref_field_sets_loading_state() {
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
        // Inject a StringRef field into StackState directly
        if let Some(s) = &mut app.stack_state {
            s.set_expansion_done(42, vec![make_string_ref_field("name", 200)]);
        }
        // Navigate to the StringRef field (field_path=[0])
        app.handle_input(InputEvent::Down); // → OnObjectField{0,0,[0]} (StringRef 200)
        assert_eq!(
            app.stack_state.as_ref().unwrap().cursor(),
            &crate::views::stack_view::StackCursor::OnObjectField {
                frame_idx: 0,
                var_idx: 0,
                field_path: vec![0],
            }
        );
        app.handle_input(InputEvent::Enter); // start_string_loading(200)
        use crate::views::stack_view::StringPhase;
        assert_eq!(
            app.stack_state.as_ref().unwrap().string_phase(200),
            StringPhase::Loading
        );
    }

    #[test]
    fn poll_strings_with_completed_channel_sets_loaded() {
        use crate::views::stack_view::StringPhase;
        let frames = vec![make_frame(10)];
        let vars = vec![make_obj_var(0, 42)];
        let engine = StubEngine::with_threads_and_frames(&["main"], frames).with_vars(10, vars);
        let mut app = App::new(engine, "test.hprof".to_string());
        app.handle_input(InputEvent::Enter);
        app.handle_input(InputEvent::Enter);
        app.handle_input(InputEvent::Down);
        // Directly inject string loading state
        if let Some(s) = &mut app.stack_state {
            s.start_string_loading(200);
        }
        let (tx, rx) = std::sync::mpsc::channel::<Option<String>>();
        app.pending_strings.insert(200, rx);
        tx.send(Some("hello".to_string())).unwrap();
        app.poll_strings();
        assert_eq!(
            app.stack_state.as_ref().unwrap().string_phase(200),
            StringPhase::Loaded
        );
        assert!(app.pending_strings.is_empty());
    }

    #[test]
    fn enter_on_loading_string_ref_is_noop() {
        use crate::views::stack_view::StringPhase;
        let frames = vec![make_frame(10)];
        let vars = vec![make_obj_var(0, 42)];
        let engine = StubEngine::with_threads_and_frames(&["main"], frames).with_vars(10, vars);
        let mut app = App::new(engine, "test.hprof".to_string());
        app.handle_input(InputEvent::Enter);
        app.handle_input(InputEvent::Enter);
        app.handle_input(InputEvent::Down);
        if let Some(s) = &mut app.stack_state {
            s.set_expansion_done(42, vec![make_string_ref_field("name", 200)]);
            s.start_string_loading(200);
        }
        // Cursor at OnVar, navigate to field
        app.handle_input(InputEvent::Down); // → OnVar{0,0}
        app.handle_input(InputEvent::Down); // → OnObjectField{0,0,[0]}
        // Already loading — enter must be no-op
        app.handle_input(InputEvent::Enter);
        // pending_strings must NOT grow (still Loading, not a new spawn)
        assert_eq!(
            app.stack_state.as_ref().unwrap().string_phase(200),
            StringPhase::Loading
        );
    }

    #[test]
    fn poll_strings_with_none_result_sets_failed_and_emits_warning() {
        use crate::views::stack_view::StringPhase;
        let frames = vec![make_frame(10)];
        let vars = vec![make_obj_var(0, 42)];
        let engine = StubEngine::with_threads_and_frames(&["main"], frames).with_vars(10, vars);
        let mut app = App::new(engine, "test.hprof".to_string());
        app.handle_input(InputEvent::Enter);
        if let Some(s) = &mut app.stack_state {
            s.start_string_loading(200);
        }
        let (tx, rx) = std::sync::mpsc::channel::<Option<String>>();
        app.pending_strings.insert(200, rx);
        tx.send(None).unwrap();
        app.poll_strings();
        assert_eq!(
            app.stack_state.as_ref().unwrap().string_phase(200),
            StringPhase::Failed
        );
        assert!(
            app.app_warnings.iter().any(|w| w.contains("0xC8")),
            "must emit warning mentioning the hex id"
        );
    }

    #[test]
    fn moving_cursor_while_string_ref_loading_does_not_start_new_load() {
        use crate::views::stack_view::StringPhase;
        let frames = vec![make_frame(10)];
        let vars = vec![make_obj_var(0, 42)];
        let engine = StubEngine::with_threads_and_frames(&["main"], frames).with_vars(10, vars);
        let mut app = App::new(engine, "test.hprof".to_string());
        app.handle_input(InputEvent::Enter); // → StackFrames
        app.handle_input(InputEvent::Enter); // expand frame 10
        app.handle_input(InputEvent::Down); // → OnVar{0,0}
        // Inject StringRef in Loading state directly
        if let Some(s) = &mut app.stack_state {
            s.set_expansion_done(42, vec![make_string_ref_field("name", 200)]);
            s.start_string_loading(200);
        }
        // Navigate to the field, then move cursor — must not spawn a new load
        app.handle_input(InputEvent::Down); // → OnObjectField{0,0,[0]}
        app.handle_input(InputEvent::Up);
        app.handle_input(InputEvent::Down);
        assert!(
            app.pending_strings.is_empty(),
            "cursor movement must not spawn string loads"
        );
        assert_eq!(
            app.stack_state.as_ref().unwrap().string_phase(200),
            StringPhase::Loading
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
}
