//! Thread list view: filterable, searchable list of Java threads.
//!
//! `ThreadListState` owns the full thread list and filtered view.
//! `SearchableList` is the ratatui `StatefulWidget` that renders it.
//!
//! ## Selection stability
//! Selection tracks `thread_serial` (not list index), so filtered views
//! preserve the highlighted thread across filter changes (AC #4).

use hprof_engine::{ThreadInfo, ThreadState};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, StatefulWidget, Widget},
};

use crate::theme::THEME;
use crate::views::cursor::CursorState;

/// Owns all thread list state: full data, active filter, and selection.
pub struct ThreadListState {
    /// Full unfiltered thread list, sorted by thread_serial.
    threads: Vec<ThreadInfo>,
    /// Active case-insensitive substring filter. Empty = no filter.
    filter: String,
    /// Thread serials in display order after filtering.
    filtered_serials: Vec<u32>,
    /// Cursor and ratatui list state for filtered serials.
    nav: CursorState<u32>,
    /// Whether the search input box is focused.
    search_active: bool,
}

impl ThreadListState {
    /// Builds state from a sorted thread list. Selects first thread if any.
    pub fn new(threads: Vec<ThreadInfo>) -> Self {
        let filtered_serials: Vec<u32> = threads.iter().map(|t| t.thread_serial).collect();
        let mut nav = CursorState::new(filtered_serials.first().copied().unwrap_or(0));
        nav.sync(&filtered_serials);
        Self {
            threads,
            filter: String::new(),
            filtered_serials,
            nav,
            search_active: false,
        }
    }

    /// Rebuilds `filtered_serials` from threads whose name contains `query`
    /// case-insensitively. Keeps the selected serial if still visible.
    pub fn apply_filter(&mut self, query: &str) {
        self.filter = query.to_string();
        let q = query.to_lowercase();
        self.filtered_serials = self
            .threads
            .iter()
            .filter(|t| t.name.to_lowercase().contains(&q))
            .map(|t| t.thread_serial)
            .collect();

        if self.filtered_serials.is_empty() {
            self.nav.sync(&self.filtered_serials);
        } else {
            self.nav.sync_or_select_first(&self.filtered_serials);
        }
    }

    /// Moves selection down one item (clamps at end).
    pub fn move_down(&mut self) {
        self.nav.move_down(&self.filtered_serials);
    }

    /// Moves selection up one item (clamps at start).
    pub fn move_up(&mut self) {
        self.nav.move_up(&self.filtered_serials);
    }

    /// Moves selection to the first item.
    pub fn move_home(&mut self) {
        self.nav.move_home(&self.filtered_serials);
    }

    /// Moves selection to the last item.
    pub fn move_end(&mut self) {
        self.nav.move_end(&self.filtered_serials);
    }

    /// Moves selection down by one visible page (clamps at end).
    pub fn page_down(&mut self) {
        self.nav.move_page_down(&self.filtered_serials);
    }

    /// Moves selection up by one visible page (clamps at start).
    pub fn page_up(&mut self) {
        self.nav.move_page_up(&self.filtered_serials);
    }

    /// Sets visible list height used by page navigation.
    pub fn set_visible_height(&mut self, h: usize) {
        self.nav.set_visible_height(h);
    }

    /// Returns the serial of the currently highlighted thread, or `None`
    /// when the filtered list is empty.
    pub fn selected_serial(&self) -> Option<u32> {
        if self.filtered_serials.is_empty() {
            None
        } else {
            Some(*self.nav.cursor())
        }
    }

    /// Current filter string.
    pub fn filter(&self) -> &str {
        &self.filter
    }

    /// Selects a thread by serial when it is visible in the current filtered list.
    pub fn select_serial(&mut self, serial: u32) {
        if self.filtered_serials.contains(&serial) {
            self.nav.set_cursor_and_sync(serial, &self.filtered_serials);
        }
    }

    /// Number of threads in the filtered view.
    pub fn filtered_count(&self) -> usize {
        self.filtered_serials.len()
    }

    /// Returns `true` if the search input is active.
    pub fn is_search_active(&self) -> bool {
        self.search_active
    }

    /// Activates the search input box.
    pub fn activate_search(&mut self) {
        if self.search_active {
            return;
        }
        self.search_active = true;
    }

    /// Deactivates the search input box.
    pub fn deactivate_search(&mut self) {
        self.search_active = false;
    }

    /// Clears the active filter and restores the full list.
    pub fn clear_filter(&mut self) {
        self.apply_filter("");
    }

    /// Returns the [`ThreadInfo`] for the currently selected thread, or `None`.
    pub fn selected_thread(&self) -> Option<&ThreadInfo> {
        self.selected_serial()
            .and_then(|s| self.thread_by_serial(s))
    }

    fn thread_by_serial(&self, serial: u32) -> Option<&ThreadInfo> {
        self.threads.iter().find(|t| t.thread_serial == serial)
    }
}

fn state_style(state: ThreadState) -> Style {
    match state {
        ThreadState::Runnable => THEME.thread_runnable,
        ThreadState::Waiting => THEME.thread_waiting,
        ThreadState::Blocked => THEME.thread_blocked,
        ThreadState::Unknown => THEME.thread_unknown,
    }
}

/// ratatui `StatefulWidget` that renders the thread list with search bar
/// and legend.
pub struct SearchableList {
    /// Whether this panel has keyboard focus.
    pub focused: bool,
}

impl StatefulWidget for SearchableList {
    type State = ThreadListState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let border_style = if self.focused {
            THEME.border_focused
        } else {
            THEME.border_unfocused
        };

        let title = format!("Threads ({})", state.filtered_count());
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Plain)
            .border_style(border_style)
            .title(title.as_str());

        let inner = block.inner(area);
        block.render(area, buf);

        // Layout: search (1), sep (1), list (fill), sep (1), legend (1)
        let [
            search_area,
            sep_above_list,
            list_area,
            sep_above_legend,
            legend_area,
        ] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(inner);

        // Search row
        let search_line = if state.is_search_active() {
            Line::from(vec![
                Span::styled("/ ", THEME.search_active),
                Span::styled(state.filter(), THEME.search_active),
                Span::styled("_", THEME.search_active),
            ])
        } else if !state.filter().is_empty() {
            Line::from(vec![
                Span::styled("/ ", THEME.search_active),
                Span::styled(state.filter(), THEME.search_active),
            ])
        } else {
            Line::from(Span::styled("Press / to search", THEME.null_value))
        };
        search_line.render(search_area, buf);

        Line::from(Span::styled(
            "─".repeat(sep_above_list.width as usize),
            THEME.border_unfocused,
        ))
        .render(sep_above_list, buf);

        Line::from(Span::styled(
            "─".repeat(sep_above_legend.width as usize),
            THEME.border_unfocused,
        ))
        .render(sep_above_legend, buf);

        // Thread list or empty message
        if state.filtered_serials.is_empty() {
            let msg = if state.filter().is_empty() {
                "No threads".to_string()
            } else {
                format!("No threads match \"{}\"", state.filter())
            };
            Line::from(Span::styled(msg, THEME.null_value)).render(list_area, buf);
        } else {
            let items: Vec<ListItem> = state
                .filtered_serials
                .iter()
                .filter_map(|&s| state.thread_by_serial(s))
                .map(|t| {
                    let dot = Span::styled("o ", state_style(t.state));
                    let name = Span::raw(t.name.clone());
                    ListItem::new(Line::from(vec![dot, name]))
                })
                .collect();

            let list = List::new(items).highlight_style(THEME.selection_bg);
            StatefulWidget::render(list, list_area, buf, state.nav.list_state_mut());
        }

        // Legend row
        let legend = Line::from(vec![
            Span::styled("o", THEME.thread_runnable),
            Span::raw(" Run  "),
            Span::styled("o", THEME.thread_waiting),
            Span::raw(" Wt  "),
            Span::styled("o", THEME.thread_blocked),
            Span::raw(" Blk  "),
            Span::styled("o", THEME.thread_unknown),
            Span::styled(" Unknown", THEME.null_value),
        ]);
        legend.render(legend_area, buf);
    }
}

#[cfg(test)]
mod tests {
    use hprof_engine::{ThreadInfo, ThreadState};

    use super::*;

    fn make_threads(names: &[&str]) -> Vec<ThreadInfo> {
        names
            .iter()
            .enumerate()
            .map(|(i, &name)| ThreadInfo {
                thread_serial: (i + 1) as u32,
                name: name.to_string(),
                state: ThreadState::Unknown,
            })
            .collect()
    }

    #[test]
    fn new_with_three_threads_selects_first() {
        let state = ThreadListState::new(make_threads(&["main", "worker-1", "worker-2"]));
        assert_eq!(state.selected_serial(), Some(1));
    }

    #[test]
    fn move_down_moves_to_second_thread() {
        let mut state = ThreadListState::new(make_threads(&["main", "worker-1", "worker-2"]));
        state.move_down();
        assert_eq!(state.selected_serial(), Some(2));
    }

    #[test]
    fn move_up_at_top_does_nothing() {
        let mut state = ThreadListState::new(make_threads(&["main", "worker-1", "worker-2"]));
        state.move_up();
        assert_eq!(state.selected_serial(), Some(1));
    }

    #[test]
    fn move_up_from_last_moves_to_second_to_last() {
        let mut state = ThreadListState::new(make_threads(&["main", "worker-1", "worker-2"]));
        state.move_end();
        state.move_up();
        assert_eq!(state.selected_serial(), Some(2));
    }

    #[test]
    fn apply_filter_worker_keeps_both_workers() {
        let mut state = ThreadListState::new(make_threads(&["main", "worker-1", "worker-2"]));
        // Select worker-1 first
        state.move_down();
        assert_eq!(state.selected_serial(), Some(2));
        state.apply_filter("worker");
        assert_eq!(state.filtered_count(), 2);
        // worker-1 (serial 2) was selected and is still visible — keep it
        assert_eq!(state.selected_serial(), Some(2));
    }

    #[test]
    fn apply_filter_xyz_yields_empty_list() {
        let mut state = ThreadListState::new(make_threads(&["main", "worker-1"]));
        state.apply_filter("xyz");
        assert_eq!(state.filtered_count(), 0);
    }

    #[test]
    fn apply_filter_empty_restores_full_list() {
        let mut state = ThreadListState::new(make_threads(&["main", "worker-1", "worker-2"]));
        state.apply_filter("worker");
        assert_eq!(state.filtered_count(), 2);
        state.apply_filter("");
        assert_eq!(state.filtered_count(), 3);
    }

    #[test]
    fn selection_tracks_serial_not_index() {
        let mut state = ThreadListState::new(make_threads(&["main", "worker-1", "worker-2"]));
        // Select worker-1 (serial 2, index 1)
        state.move_down();
        assert_eq!(state.selected_serial(), Some(2));
        state.apply_filter("worker");
        // worker-1 is now at index 0 in filtered list, but serial is still 2
        assert_eq!(state.selected_serial(), Some(2));
    }

    #[test]
    fn selected_serial_returns_none_when_filtered_list_is_empty() {
        let mut state = ThreadListState::new(make_threads(&["main", "worker-1"]));
        state.apply_filter("xyz");
        assert_eq!(state.selected_serial(), None);
    }

    #[test]
    fn page_down_jumps_by_n_items() {
        let mut state = ThreadListState::new(make_threads(&["t1", "t2", "t3", "t4", "t5", "t6"]));
        state.set_visible_height(3);
        state.page_down();
        assert_eq!(state.selected_serial(), Some(4));
    }

    #[test]
    fn page_down_clamps_at_last_item() {
        let mut state = ThreadListState::new(make_threads(&["t1", "t2", "t3"]));
        state.set_visible_height(10);
        state.page_down();
        assert_eq!(state.selected_serial(), Some(3));
    }

    #[test]
    fn page_up_jumps_by_n_items() {
        let mut state = ThreadListState::new(make_threads(&["t1", "t2", "t3", "t4", "t5", "t6"]));
        state.move_end();
        state.set_visible_height(3);
        state.page_up();
        assert_eq!(state.selected_serial(), Some(3));
    }

    #[test]
    fn page_up_clamps_at_first_item() {
        let mut state = ThreadListState::new(make_threads(&["t1", "t2", "t3"]));
        state.move_end();
        state.set_visible_height(10);
        state.page_up();
        assert_eq!(state.selected_serial(), Some(1));
    }

    #[test]
    fn thread_list_clear_filter_on_empty_result_syncs_cursor() {
        let mut state = ThreadListState::new(make_threads(&["main", "worker-1", "worker-2"]));
        state.apply_filter("xyz");
        assert_eq!(state.selected_serial(), None);

        state.clear_filter();

        assert_eq!(state.filter(), "");
        assert_eq!(state.filtered_count(), 3);
        assert_eq!(state.selected_serial(), Some(1));
    }

    #[test]
    fn thread_list_reopen_search_preserves_existing_filter_in_input() {
        let mut state = ThreadListState::new(make_threads(&["main", "worker-1"]));
        state.apply_filter("work");
        state.activate_search();
        state.deactivate_search();

        state.activate_search();

        assert!(state.is_search_active());
        assert_eq!(state.filter(), "work");
    }

    #[test]
    fn select_serial_sets_cursor_when_visible() {
        let mut state = ThreadListState::new(make_threads(&["main", "worker-1", "worker-2"]));
        state.select_serial(3);
        assert_eq!(state.selected_serial(), Some(3));
    }

    #[test]
    fn select_serial_noop_when_hidden_by_filter() {
        let mut state = ThreadListState::new(make_threads(&["main", "worker-1", "worker-2"]));
        state.apply_filter("main");
        state.select_serial(3);
        assert_eq!(state.selected_serial(), Some(1));
    }

    // --- Separator rendering tests (AC #1–#5) ---

    const RENDER_W: u16 = 80;
    const RENDER_H: u16 = 20;

    fn render_thread_list(
        threads: Vec<hprof_engine::ThreadInfo>,
        filter: &str,
        focused: bool,
    ) -> (ratatui::buffer::Buffer, ratatui::layout::Rect) {
        use ratatui::{Terminal, backend::TestBackend};
        let backend = TestBackend::new(RENDER_W, RENDER_H);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = ThreadListState::new(threads);
        if !filter.is_empty() {
            state.apply_filter(filter);
        }
        let area = ratatui::layout::Rect::new(0, 0, RENDER_W, RENDER_H);
        terminal
            .draw(|f| {
                f.render_stateful_widget(SearchableList { focused }, area, &mut state);
            })
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        (buf, area)
    }

    fn cell_at(buf: &ratatui::buffer::Buffer, x: u16, y: u16) -> &ratatui::buffer::Cell {
        buf.cell((x, y)).expect("cell coordinates out of buffer bounds")
    }

    #[test]
    fn separators_char_and_style_at_expected_rows() {
        // AC #1, #2: both separator rows contain ─ with border_unfocused fg color
        use ratatui::style::Color;
        let (buf, area) = render_thread_list(make_threads(&["main", "worker"]), "", false);
        let sep_y1 = area.y + 2;
        let sep_y2 = area.y + area.height - 3;
        let c1 = cell_at(&buf, area.x + 1, sep_y1);
        assert_eq!(c1.symbol(), "─", "sep above list: wrong char");
        assert_eq!(c1.fg, Color::DarkGray, "sep above list: wrong fg");
        let c2 = cell_at(&buf, area.x + 1, sep_y2);
        assert_eq!(c2.symbol(), "─", "sep above legend: wrong char");
        assert_eq!(c2.fg, Color::DarkGray, "sep above legend: wrong fg");
    }

    #[test]
    fn separators_fill_full_inner_width() {
        // AC #3: separators stretch to fill available inner width
        let (buf, area) = render_thread_list(make_threads(&["main"]), "", false);
        let sep_y1 = area.y + 2;
        let sep_y2 = area.y + area.height - 3;
        for x in (area.x + 1)..(area.x + area.width - 1) {
            assert_eq!(
                cell_at(&buf, x, sep_y1).symbol(),
                "─",
                "sep above list missing at x={x}"
            );
            assert_eq!(
                cell_at(&buf, x, sep_y2).symbol(),
                "─",
                "sep above legend missing at x={x}"
            );
        }
    }

    #[test]
    fn separators_visible_when_list_empty_due_to_filter() {
        // AC #5: both separators still render when filter matches nothing
        let (buf, area) = render_thread_list(make_threads(&["main"]), "xyz", false);
        let sep_y1 = area.y + 2;
        let sep_y2 = area.y + area.height - 3;
        assert_eq!(cell_at(&buf, area.x + 1, sep_y1).symbol(), "─");
        assert_eq!(cell_at(&buf, area.x + 1, sep_y2).symbol(), "─");
    }

    #[test]
    fn separators_use_unfocused_style_regardless_of_focus() {
        // AC #4: separators always use border_unfocused fg and no selection modifier
        use ratatui::style::{Color, Modifier};
        let (buf, area) = render_thread_list(make_threads(&["main"]), "", true);
        let sep_y1 = area.y + 2;
        let sep_y2 = area.y + area.height - 3;
        let c1 = cell_at(&buf, area.x + 1, sep_y1);
        assert_eq!(c1.symbol(), "─", "sep above list: wrong char when focused");
        assert_eq!(
            c1.fg,
            Color::DarkGray,
            "sep above list must use border_unfocused when focused"
        );
        assert!(
            !c1.modifier.contains(Modifier::REVERSED),
            "sep above list must not have selection highlight when focused"
        );
        let c2 = cell_at(&buf, area.x + 1, sep_y2);
        assert_eq!(
            c2.symbol(),
            "─",
            "sep above legend: wrong char when focused"
        );
        assert_eq!(
            c2.fg,
            Color::DarkGray,
            "sep above legend must use border_unfocused when focused"
        );
    }
}
