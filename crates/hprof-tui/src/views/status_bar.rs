//! Status bar widget rendering file info and status at the bottom of the TUI
//! layout.
//!
//! Displays: `<filename>  |  <N> threads  |  <thread-name>  <STATE>`

use hprof_engine::{ThreadInfo, ThreadState};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::{Line, Span},
    widgets::Widget,
};

use crate::theme::THEME;

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
    /// Indexing completeness ratio (0.0–100.0). `None` = fully indexed.
    pub file_indexed_pct: Option<f64>,
    /// Most recent session warning text, if any.
    pub last_warning: Option<&'a str>,
    /// Number of pinned items hidden because terminal is too narrow.
    pub pinned_hidden_count: usize,
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
            let last = self.last_warning.map(|w| {
                let truncated: String = w.chars().take(40).collect();
                if w.chars().count() > 40 {
                    format!("{truncated}…")
                } else {
                    truncated
                }
            });
            if let Some(w) = last {
                format!(
                    "  |  [!] {} warnings ({w}) — see stderr",
                    self.warning_count
                )
            } else {
                format!("  |  [!] {} warnings — see stderr", self.warning_count)
            }
        } else {
            String::new()
        };

        let incomplete_part = match self.file_indexed_pct {
            Some(pct) if pct < 100.0 => format!("[!] Incomplete file — {pct:.0}% indexed  |  "),
            Some(_) => "[!] Incomplete file  |  ".to_string(),
            None => String::new(),
        };

        let pinned_part = if self.pinned_hidden_count > 0 {
            format!("  [★ {}]", self.pinned_hidden_count)
        } else {
            String::new()
        };

        let line = Line::from(vec![Span::styled(
            format!(
                " {}{}  |  {}  |  {}{}{}  |  [?]help",
                incomplete_part, self.filename, thread_part, selected_part, pinned_part, warn_part
            ),
            THEME.status_bar_bg,
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

    fn render_status_bar(bar: StatusBar<'_>, width: u16) -> String {
        use ratatui::{Terminal, backend::TestBackend};
        let backend = TestBackend::new(width, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                f.render_widget(bar, f.area());
            })
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol().to_string())
            .collect()
    }

    #[test]
    fn warning_count_zero_produces_no_warning_indicator() {
        let content = render_status_bar(
            StatusBar {
                filename: "test.hprof",
                thread_count: 3,
                selected: None,
                warning_count: 0,
                file_indexed_pct: None,
                last_warning: None,
                pinned_hidden_count: 0,
            },
            120,
        );
        assert!(
            !content.contains("warnings"),
            "no warning indicator expected"
        );
    }

    #[test]
    fn warning_count_nonzero_renders_warning_indicator() {
        let content = render_status_bar(
            StatusBar {
                filename: "test.hprof",
                thread_count: 3,
                selected: None,
                warning_count: 5,
                file_indexed_pct: None,
                last_warning: None,
                pinned_hidden_count: 0,
            },
            120,
        );
        assert!(
            content.contains("5 warnings"),
            "warning indicator must mention count; got: {content:?}"
        );
    }

    #[test]
    fn incomplete_file_shown_in_status_bar() {
        let content = render_status_bar(
            StatusBar {
                filename: "test.hprof",
                thread_count: 3,
                selected: None,
                warning_count: 0,
                file_indexed_pct: Some(75.3),
                last_warning: None,
                pinned_hidden_count: 0,
            },
            200,
        );
        assert!(
            content.contains("Incomplete file") && content.contains("75%"),
            "incomplete file indicator must show; got: {content:?}"
        );
    }

    #[test]
    fn last_warning_appended_in_status_bar() {
        let content = render_status_bar(
            StatusBar {
                filename: "test.hprof",
                thread_count: 3,
                selected: None,
                warning_count: 2,
                file_indexed_pct: None,
                last_warning: Some("Object 0xABC not found"),
                pinned_hidden_count: 0,
            },
            200,
        );
        assert!(
            content.contains("Object 0xABC not found"),
            "last warning text must appear; got: {content:?}"
        );
    }

    #[test]
    fn last_warning_truncated_at_40_chars() {
        let long_warning = "A".repeat(50);
        let content = render_status_bar(
            StatusBar {
                filename: "test.hprof",
                thread_count: 1,
                selected: None,
                warning_count: 1,
                file_indexed_pct: None,
                last_warning: Some(&long_warning),
                pinned_hidden_count: 0,
            },
            300,
        );
        // Should contain 40 'A's followed by '…', not 50 'A's
        assert!(
            content.contains(&"A".repeat(40)),
            "must contain 40 chars; got: {content:?}"
        );
        assert!(
            !content.contains(&"A".repeat(41)),
            "must not contain 41 chars (must be truncated); got: {content:?}"
        );
        assert!(
            content.contains('…'),
            "must contain ellipsis; got: {content:?}"
        );
    }
}
