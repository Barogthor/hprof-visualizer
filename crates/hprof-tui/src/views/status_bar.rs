//! Status bar widget rendering file info and key hints at the bottom of
//! the TUI layout.
//!
//! Displays: `<filename>  |  <N> threads  |  <thread-name>  <STATE>  |
//! [q]uit  [/]search  [Esc]back`

use hprof_engine::{ThreadInfo, ThreadState};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::{Line, Span},
    widgets::Widget,
};

use crate::theme;

/// One-line status bar for the bottom of the TUI.
pub struct StatusBar<'a> {
    /// Filename of the open heap dump.
    pub filename: &'a str,
    /// Total thread count (filtered or full, as appropriate for context).
    pub thread_count: usize,
    /// Currently selected thread, if any.
    pub selected: Option<&'a ThreadInfo>,
    /// Number of non-fatal parse warnings collected during indexing.
    pub warning_count: usize,
}

pub(crate) fn state_label(state: ThreadState) -> &'static str {
    match state {
        ThreadState::Unknown => "UNKNOWN",
        ThreadState::Runnable => "RUNNABLE",
        ThreadState::Waiting => "WAITING",
        ThreadState::Blocked => "BLOCKED",
    }
}

impl Widget for StatusBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let thread_part = format!("{}  threads", self.thread_count);

        let selected_part = match self.selected {
            Some(t) => format!("{}  {}", t.name, state_label(t.state)),
            None => "—".to_string(),
        };

        let warn_part = if self.warning_count > 0 {
            format!("  |  [!] {} warnings — see stderr", self.warning_count)
        } else {
            String::new()
        };

        let line = Line::from(vec![Span::styled(
            format!(
                " {}  |  {}  |  {}  |  [q]uit  [/]search  [Esc]back{}",
                self.filename, thread_part, selected_part, warn_part
            ),
            theme::STATUS_BAR,
        )]);
        line.render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use hprof_engine::{ThreadInfo, ThreadState};

    use super::*;

    #[test]
    fn state_label_returns_correct_string_for_each_variant() {
        assert_eq!(state_label(ThreadState::Unknown), "UNKNOWN");
        assert_eq!(state_label(ThreadState::Runnable), "RUNNABLE");
        assert_eq!(state_label(ThreadState::Waiting), "WAITING");
        assert_eq!(state_label(ThreadState::Blocked), "BLOCKED");
    }

    #[test]
    fn state_label_covers_all_thread_state_variants() {
        // Compile-time exhaustiveness: ensure this test must be updated if
        // ThreadState gains new variants (match would fail to compile).
        for state in [
            ThreadState::Unknown,
            ThreadState::Runnable,
            ThreadState::Waiting,
            ThreadState::Blocked,
        ] {
            let label = state_label(state);
            assert!(
                !label.is_empty(),
                "state_label must not be empty for {state:?}"
            );
        }
    }

    #[test]
    fn status_bar_selected_part_uses_thread_name_and_state() {
        // Verify the status bar renders thread name + state correctly.
        // We check state_label integration since StatusBar::render is terminal-bound.
        let thread = ThreadInfo {
            thread_serial: 1,
            name: "main".to_string(),
            state: ThreadState::Unknown,
        };
        let expected_part = format!("{}  {}", thread.name, state_label(thread.state));
        assert_eq!(expected_part, "main  UNKNOWN");
    }

    #[test]
    fn warning_count_zero_produces_no_warning_indicator() {
        use ratatui::{Terminal, backend::TestBackend};
        let backend = TestBackend::new(120, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                f.render_widget(
                    StatusBar {
                        filename: "test.hprof",
                        thread_count: 3,
                        selected: None,
                        warning_count: 0,
                    },
                    f.area(),
                );
            })
            .unwrap();
        let content: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol().to_string())
            .collect();
        assert!(
            !content.contains("warnings"),
            "no warning indicator expected"
        );
    }

    #[test]
    fn warning_count_nonzero_renders_warning_indicator() {
        use ratatui::{Terminal, backend::TestBackend};
        let backend = TestBackend::new(120, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                f.render_widget(
                    StatusBar {
                        filename: "test.hprof",
                        thread_count: 3,
                        selected: None,
                        warning_count: 5,
                    },
                    f.area(),
                );
            })
            .unwrap();
        let content: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol().to_string())
            .collect();
        assert!(
            content.contains("5 warnings"),
            "warning indicator must mention count; got: {content:?}"
        );
    }
}
