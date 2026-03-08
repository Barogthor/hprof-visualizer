//! Centralized style constants for the hprof-visualizer TUI.
//!
//! All colors use the 16 ANSI base colors only (no 256-color or RGB).
//! Widgets MUST import styles from here — never hardcode colors or
//! modifiers elsewhere.
//!
//! ## Semantic color vocabulary
//! - Thread state: green (RUNNABLE), yellow (WAITING), red (BLOCKED),
//!   dark-gray (UNKNOWN)
//! - Panel: focused border (bold white), unfocused border (dark-gray)
//! - Search: active input area (cyan fg)
//! - Selection: reversed video

use ratatui::style::{Color, Modifier, Style};

// --- Thread state dots ---
pub const STATE_RUNNABLE: Style = Style::new().fg(Color::Green);
pub const STATE_WAITING: Style = Style::new().fg(Color::Yellow);
pub const STATE_BLOCKED: Style = Style::new().fg(Color::Red);
pub const STATE_UNKNOWN: Style = Style::new().fg(Color::DarkGray);

// --- Selection ---
pub const SELECTED: Style = Style::new().add_modifier(Modifier::REVERSED);

// --- Panel borders ---
pub const BORDER_FOCUSED: Style = Style::new().fg(Color::White).add_modifier(Modifier::BOLD);
pub const BORDER_UNFOCUSED: Style = Style::new().fg(Color::DarkGray);

// --- Search input ---
pub const SEARCH_ACTIVE: Style = Style::new().fg(Color::Cyan);
pub const SEARCH_HINT: Style = Style::new().fg(Color::DarkGray);

// --- Status bar ---
pub const STATUS_BAR: Style = Style::new().fg(Color::White).bg(Color::DarkGray);
pub const STATUS_WARNING: Style = Style::new().fg(Color::Yellow);

// --- Legend ---
pub const LEGEND: Style = Style::new().fg(Color::DarkGray);

#[cfg(test)]
mod tests {
    use ratatui::style::Style;

    use super::*;

    #[test]
    fn all_style_constants_are_of_type_style() {
        fn assert_style(_: Style) {}
        assert_style(STATE_RUNNABLE);
        assert_style(STATE_WAITING);
        assert_style(STATE_BLOCKED);
        assert_style(STATE_UNKNOWN);
        assert_style(SELECTED);
        assert_style(BORDER_FOCUSED);
        assert_style(BORDER_UNFOCUSED);
        assert_style(SEARCH_ACTIVE);
        assert_style(SEARCH_HINT);
        assert_style(STATUS_BAR);
        assert_style(STATUS_WARNING);
        assert_style(LEGEND);
    }
}
