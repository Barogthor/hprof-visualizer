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
    widgets::{Block, BorderType, Borders, List, ListItem, ListState, StatefulWidget, Widget},
};

use crate::theme::THEME;

/// Owns all thread list state: full data, active filter, and selection.
pub struct ThreadListState {
    /// Full unfiltered thread list, sorted by thread_serial.
    threads: Vec<ThreadInfo>,
    /// Active case-insensitive substring filter. Empty = no filter.
    filter: String,
    /// Thread serials in display order after filtering.
    filtered_serials: Vec<u32>,
    /// Serial of the currently highlighted thread.
    selected_serial: Option<u32>,
    /// ratatui list state (owns the visual scroll offset).
    list_state: ListState,
    /// Whether the search input box is focused.
    search_active: bool,
}

impl ThreadListState {
    /// Builds state from a sorted thread list. Selects first thread if any.
    pub fn new(threads: Vec<ThreadInfo>) -> Self {
        let filtered_serials: Vec<u32> = threads.iter().map(|t| t.thread_serial).collect();
        let selected_serial = filtered_serials.first().copied();
        let mut list_state = ListState::default();
        if selected_serial.is_some() {
            list_state.select(Some(0));
        }
        Self {
            threads,
            filter: String::new(),
            filtered_serials,
            selected_serial,
            list_state,
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

        // Keep selection if still visible; otherwise pick first.
        let keep = self
            .selected_serial
            .filter(|s| self.filtered_serials.contains(s));
        if let Some(s) = keep {
            self.selected_serial = Some(s);
            let idx = self.filtered_serials.iter().position(|&x| x == s);
            self.list_state.select(idx);
        } else {
            self.selected_serial = self.filtered_serials.first().copied();
            self.list_state.select(if self.filtered_serials.is_empty() {
                None
            } else {
                Some(0)
            });
        }
    }

    /// Moves selection down one item (clamps at end).
    pub fn move_down(&mut self) {
        if self.filtered_serials.is_empty() {
            return;
        }
        let current = self.list_state.selected().unwrap_or(0);
        let next = (current + 1).min(self.filtered_serials.len() - 1);
        self.list_state.select(Some(next));
        self.selected_serial = self.filtered_serials.get(next).copied();
    }

    /// Moves selection up one item (clamps at start).
    pub fn move_up(&mut self) {
        if self.filtered_serials.is_empty() {
            return;
        }
        let current = self.list_state.selected().unwrap_or(0);
        let prev = current.saturating_sub(1);
        self.list_state.select(Some(prev));
        self.selected_serial = self.filtered_serials.get(prev).copied();
    }

    /// Moves selection to the first item.
    pub fn move_home(&mut self) {
        if self.filtered_serials.is_empty() {
            return;
        }
        self.list_state.select(Some(0));
        self.selected_serial = self.filtered_serials.first().copied();
    }

    /// Moves selection to the last item.
    pub fn move_end(&mut self) {
        if self.filtered_serials.is_empty() {
            return;
        }
        let last = self.filtered_serials.len() - 1;
        self.list_state.select(Some(last));
        self.selected_serial = self.filtered_serials.last().copied();
    }

    /// Moves selection down by `n` items (clamps at end).
    pub fn page_down(&mut self, n: usize) {
        if self.filtered_serials.is_empty() {
            return;
        }
        let current = self.list_state.selected().unwrap_or(0);
        let next = (current + n).min(self.filtered_serials.len() - 1);
        self.list_state.select(Some(next));
        self.selected_serial = self.filtered_serials.get(next).copied();
    }

    /// Moves selection up by `n` items (clamps at start).
    pub fn page_up(&mut self, n: usize) {
        if self.filtered_serials.is_empty() {
            return;
        }
        let current = self.list_state.selected().unwrap_or(0);
        let prev = current.saturating_sub(n);
        self.list_state.select(Some(prev));
        self.selected_serial = self.filtered_serials.get(prev).copied();
    }

    /// Returns the serial of the currently highlighted thread, or `None`
    /// when the filtered list is empty.
    pub fn selected_serial(&self) -> Option<u32> {
        self.selected_serial
    }

    /// Current filter string.
    pub fn filter(&self) -> &str {
        &self.filter
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
        self.search_active = true;
    }

    /// Deactivates the search input box.
    pub fn deactivate_search(&mut self) {
        self.search_active = false;
    }

    /// Returns the [`ThreadInfo`] for the currently selected thread, or `None`.
    pub fn selected_thread(&self) -> Option<&ThreadInfo> {
        self.selected_serial.and_then(|s| self.thread_by_serial(s))
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

        // Layout: search row (1), list (fill), legend row (1)
        let [search_area, list_area, legend_area] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(0),
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
        } else {
            Line::from(Span::styled("Press / to search", THEME.null_value))
        };
        search_line.render(search_area, buf);

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
            StatefulWidget::render(list, list_area, buf, &mut state.list_state);
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
        state.page_down(3);
        assert_eq!(state.selected_serial(), Some(4));
    }

    #[test]
    fn page_down_clamps_at_last_item() {
        let mut state = ThreadListState::new(make_threads(&["t1", "t2", "t3"]));
        state.page_down(10);
        assert_eq!(state.selected_serial(), Some(3));
    }

    #[test]
    fn page_up_jumps_by_n_items() {
        let mut state = ThreadListState::new(make_threads(&["t1", "t2", "t3", "t4", "t5", "t6"]));
        state.move_end();
        state.page_up(3);
        assert_eq!(state.selected_serial(), Some(3));
    }

    #[test]
    fn page_up_clamps_at_first_item() {
        let mut state = ThreadListState::new(make_threads(&["t1", "t2", "t3"]));
        state.move_end();
        state.page_up(10);
        assert_eq!(state.selected_serial(), Some(1));
    }
}
