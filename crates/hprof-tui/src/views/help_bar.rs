//! Keyboard shortcut help panel, rendered as a non-focusable bottom section.
//!
//! [`HelpBar`] is a stateless widget displaying a two-column keymap table.
//! Toggle visibility via `?` from any panel. Call [`required_height`] to
//! determine the layout slot to reserve.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Widget},
};

use crate::theme::THEME;

/// Number of keymap entries documented in the help panel.
const ENTRY_COUNT: u16 = 13;

/// Keymap entries: `(key label, action label)`.
const ENTRIES: &[(&str, &str)] = &[
    ("q / Ctrl+C", "Quit"),
    ("Esc", "Go back / cancel search"),
    ("Tab", "Cycle panel focus"),
    ("\u{2191} / \u{2193}", "Move selection"),
    ("PgUp / PgDn", "Scroll one page"),
    ("Home / End", "Jump to first / last"),
    ("Enter", "Expand / confirm"),
    ("\u{2192}", "Expand node"),
    ("\u{2190}", "Unexpand / go to parent"),
    // TODO(7.1): remove "(Story 7.1)" annotations
    ("f", "Pin / unpin favorite (Story 7.1)"),
    ("F", "Focus favorites panel (Story 7.1)"),
    ("s or /", "Open search (thread list only)"),
    ("?", "Toggle help panel"),
];

/// Stateless keyboard shortcut help widget.
pub struct HelpBar;

/// Returns the total height (in terminal rows) required to render [`HelpBar`].
///
/// Formula: `2 (borders) + 1 (padding) + div_ceil(ENTRY_COUNT, 2) + 1 (separator)`
///
/// With `ENTRY_COUNT = 13`: `2 + 1 + 7 + 1 = 11`.
pub fn required_height() -> u16 {
    2 + 1 + ENTRY_COUNT.div_ceil(2) + 1
}

impl Widget for HelpBar {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .title(" Keyboard Shortcuts ")
            .borders(Borders::ALL)
            .border_style(THEME.border_focused);

        let inner = block.inner(area);
        block.render(area, buf);

        let rows = build_rows();
        let text = Text::from(rows);
        Paragraph::new(text).render(inner, buf);
    }
}

/// Builds the display lines for the help panel inner area.
///
/// Returns one [`Line`] per pair of entries (two entries per row), followed
/// by a blank separator line.
fn build_rows() -> Vec<Line<'static>> {
    let key_col_width: usize = 18;
    let entry_col_width: usize = 36;

    let mut lines: Vec<Line<'static>> = Vec::new();

    // Blank padding row.
    lines.push(Line::from(""));

    let mut i = 0;
    while i < ENTRIES.len() {
        let (key_a, action_a) = ENTRIES[i];
        let left_key = format!("  {:width$}", key_a, width = key_col_width);
        let left_action = format!("{:<width$}", action_a, width = entry_col_width);

        let spans: Vec<Span<'static>> = if i + 1 < ENTRIES.len() {
            let (key_b, action_b) = ENTRIES[i + 1];
            let right_key = format!("{:width$}", key_b, width = key_col_width);
            let right_action = action_b.to_string();
            vec![
                Span::styled(left_key, THEME.null_value),
                Span::raw(left_action),
                Span::styled(right_key, THEME.null_value),
                Span::raw(right_action),
            ]
        } else {
            vec![
                Span::styled(left_key, THEME.null_value),
                Span::raw(left_action),
            ]
        };

        lines.push(Line::from(spans));
        i += 2;
    }

    // Blank separator row.
    lines.push(Line::from(""));

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_height_returns_eleven_for_thirteen_entries() {
        assert_eq!(required_height(), 11);
    }

    #[test]
    fn entry_count_constant_matches_entries_slice() {
        assert_eq!(ENTRY_COUNT as usize, ENTRIES.len());
    }

    #[test]
    fn build_rows_produces_correct_line_count() {
        // 1 padding + ceil(13/2) + 1 separator = 1 + 7 + 1 = 9
        let rows = build_rows();
        assert_eq!(rows.len(), 9);
    }
}
