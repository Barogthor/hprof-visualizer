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
const ENTRY_COUNT: u16 = 21;

// Context bitmask constants — private to this module.
const THREAD: u8 = 0b001;
const STACK: u8 = 0b010;
const FAV: u8 = 0b100;
const ALL: u8 = 0b111;

/// Keymap entries: `(key label, action label, context_mask)`.
const ENTRIES: &[(&str, &str, u8)] = &[
    ("q / Ctrl+C", "Quit", ALL),
    ("Esc", "Search off -> clear filter -> back", ALL),
    ("Tab", "Cycle panel focus", ALL),
    ("\u{2191} / \u{2193}", "Move selection", ALL),
    ("PgUp / PgDn", "Scroll one page", ALL),
    ("Ctrl/Shift+\u{2191}", "Scroll view up", STACK),
    ("Ctrl/Shift+\u{2193}", "Scroll view down", STACK),
    ("Ctrl/Shift+PgUp/PgDn", "Scroll view one page", STACK),
    ("Ctrl+L", "Center selection", STACK),
    ("Home / End", "Jump to first / last", ALL),
    ("Enter", "Expand / confirm", THREAD | STACK),
    ("\u{2192}", "Expand node", STACK),
    ("\u{2190}", "Unexpand / go to parent", STACK),
    ("f", "Pin / unpin favorite", STACK | FAV),
    ("F", "Focus favorites panel", ALL),
    ("g", "Favorites: go to source", FAV),
    ("h", "Favorites: hide / show field", FAV),
    ("H", "Favorites: reveal / hide hidden", FAV),
    ("i", "Toggle object IDs (stack)", STACK),
    ("s or /", "Open search (thread list only)", THREAD),
    ("?", "Toggle help panel", ALL),
];

/// Panel focus context passed to [`HelpBar`] for context-aware dimming.
///
/// Variants map one-to-one with `app::Focus` and are converted at the render
/// call site to avoid a circular dependency (`app` imports `views::help_bar`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HelpContext {
    ThreadList,
    StackFrames,
    Favorites,
    /// Go-to-pin navigation in progress — same shortcuts as
    /// `StackFrames` but Esc shows "Cancel navigation".
    Navigating,
}

/// Stateless keyboard shortcut help widget.
///
/// Construct with a [`HelpContext`] to apply context-aware dimming: shortcuts
/// not applicable in the current panel focus are visually dimmed.
pub struct HelpBar {
    pub context: HelpContext,
}

/// Returns the context bitmask bit for a given [`HelpContext`].
///
/// `pub(crate)` so tests in this module can verify masks without rendering.
pub(crate) fn context_bit(ctx: &HelpContext) -> u8 {
    match ctx {
        HelpContext::ThreadList => THREAD,
        HelpContext::StackFrames | HelpContext::Navigating => STACK,
        HelpContext::Favorites => FAV,
    }
}

/// Returns the total height (in terminal rows) required to render [`HelpBar`].
///
/// Formula: `2 (borders) + 1 (padding) + div_ceil(ENTRY_COUNT, 2) + 1 (separator)`
///
/// With `ENTRY_COUNT = 21`: `2 + 1 + 11 + 1 = 15`.
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

        let rows = build_rows(self.context);
        let text = Text::from(rows);
        Paragraph::new(text).render(inner, buf);
    }
}

/// Builds the display lines for the help panel inner area.
///
/// Returns one [`Line`] per pair of entries (two entries per row), followed
/// by a blank separator line. Entries inapplicable in `ctx` are visually
/// dimmed (action span styled with `THEME.null_value`); row count is always
/// stable regardless of context (no omission).
pub(crate) fn build_rows(ctx: HelpContext) -> Vec<Line<'static>> {
    let key_col_width: usize = 18;
    let entry_col_width: usize = 36;
    let ctx_bit = context_bit(&ctx);

    let mut lines: Vec<Line<'static>> = Vec::new();

    // Blank padding row.
    lines.push(Line::from(""));

    let mut i = 0;
    while i < ENTRIES.len() {
        let (key_a, action_a, mask_a) = ENTRIES[i];
        // Override Esc label when navigating (AC6 / Task 3.5).
        let action_a = if ctx == HelpContext::Navigating && key_a == "Esc" {
            "Cancel navigation"
        } else {
            action_a
        };
        let applicable_a = ctx_bit & mask_a != 0;
        let left_key = format!("  {:width$}", key_a, width = key_col_width);
        let left_action = format!("{:<width$}", action_a, width = entry_col_width);

        let spans: Vec<Span<'static>> = if i + 1 < ENTRIES.len() {
            let (key_b, action_b, mask_b) = ENTRIES[i + 1];
            let action_b = if ctx == HelpContext::Navigating && key_b == "Esc" {
                "Cancel navigation"
            } else {
                action_b
            };
            let applicable_b = ctx_bit & mask_b != 0;
            let right_key = format!("{:width$}", key_b, width = key_col_width);
            let right_action = action_b.to_string();

            let left_action_span = if applicable_a {
                Span::raw(left_action)
            } else {
                Span::styled(left_action, THEME.null_value)
            };
            let right_action_span = if applicable_b {
                Span::raw(right_action)
            } else {
                Span::styled(right_action, THEME.null_value)
            };
            vec![
                Span::styled(left_key, THEME.null_value),
                left_action_span,
                Span::styled(right_key, THEME.null_value),
                right_action_span,
            ]
        } else {
            let left_action_span = if applicable_a {
                Span::raw(left_action)
            } else {
                Span::styled(left_action, THEME.null_value)
            };
            vec![Span::styled(left_key, THEME.null_value), left_action_span]
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
    fn required_height_returns_fifteen_for_twenty_one_entries() {
        // div_ceil(21, 2) = 11; 2 + 1 + 11 + 1 = 15
        assert_eq!(required_height(), 15);
    }

    #[test]
    fn entry_count_constant_matches_entries_slice() {
        assert_eq!(ENTRY_COUNT as usize, ENTRIES.len());
    }

    #[test]
    fn build_rows_produces_correct_line_count() {
        // 1 padding + ceil(21/2) + 1 separator = 1 + 11 + 1 = 13
        assert_eq!(build_rows(HelpContext::ThreadList).len(), 13);
        assert_eq!(build_rows(HelpContext::StackFrames).len(), 13);
        assert_eq!(build_rows(HelpContext::Favorites).len(), 13);
    }

    // --- Task 2 tests ---

    #[test]
    fn help_bar_context_bit_returns_correct_value() {
        assert_eq!(context_bit(&HelpContext::ThreadList), 0b001);
        assert_eq!(context_bit(&HelpContext::StackFrames), 0b010);
        assert_eq!(context_bit(&HelpContext::Favorites), 0b100);
    }

    #[test]
    fn help_bar_search_entry_applicable_only_in_thread_list() {
        let idx = ENTRIES
            .iter()
            .position(|(k, _, _)| k.contains("s or"))
            .unwrap();
        assert_ne!(ENTRIES[idx].2 & context_bit(&HelpContext::ThreadList), 0);
        assert_eq!(ENTRIES[idx].2 & context_bit(&HelpContext::StackFrames), 0);
        assert_eq!(ENTRIES[idx].2 & context_bit(&HelpContext::Favorites), 0);
    }

    #[test]
    fn help_bar_camera_scroll_applicable_only_in_stack_frames() {
        let idx = ENTRIES
            .iter()
            .position(|(k, _, _)| k.contains("Ctrl/Shift+\u{2191}"))
            .unwrap();
        assert_ne!(ENTRIES[idx].2 & context_bit(&HelpContext::StackFrames), 0);
        assert_eq!(ENTRIES[idx].2 & context_bit(&HelpContext::ThreadList), 0);
        assert_eq!(ENTRIES[idx].2 & context_bit(&HelpContext::Favorites), 0);
    }

    #[test]
    fn help_bar_f_key_applicable_in_stack_and_favorites_not_thread() {
        let idx = ENTRIES.iter().position(|(k, _, _)| *k == "f").unwrap();
        assert_ne!(ENTRIES[idx].2 & context_bit(&HelpContext::StackFrames), 0);
        assert_ne!(ENTRIES[idx].2 & context_bit(&HelpContext::Favorites), 0);
        assert_eq!(ENTRIES[idx].2 & context_bit(&HelpContext::ThreadList), 0);
    }

    #[test]
    fn help_bar_global_entries_applicable_in_all_contexts() {
        for key_label in ["q / Ctrl+C", "Esc", "?"] {
            let idx = ENTRIES
                .iter()
                .position(|(k, _, _)| *k == key_label)
                .unwrap();
            assert_eq!(ENTRIES[idx].2, ALL, "mask for '{key_label}' should be ALL");
        }
    }

    #[test]
    fn help_bar_all_entries_have_valid_mask() {
        for (key, _action, mask) in ENTRIES {
            assert!(*mask != 0 && *mask <= ALL, "invalid mask for entry '{key}'");
        }
    }

    #[test]
    fn help_bar_h_key_applicable_only_in_favorites() {
        let idx = ENTRIES.iter().position(|(k, _, _)| *k == "h").unwrap();
        assert_ne!(ENTRIES[idx].2 & context_bit(&HelpContext::Favorites), 0);
        assert_eq!(ENTRIES[idx].2 & context_bit(&HelpContext::ThreadList), 0);
        assert_eq!(ENTRIES[idx].2 & context_bit(&HelpContext::StackFrames), 0);
    }

    #[test]
    fn navigating_context_overrides_esc_label() {
        let rows = build_rows(HelpContext::Navigating);
        let text: String = rows
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect();
        assert!(
            text.contains("Cancel navigation"),
            "Navigating context must show 'Cancel navigation' for Esc; \
             got: {text}"
        );
        assert!(
            !text.contains("clear filter"),
            "Navigating context must NOT show normal Esc label"
        );
    }

    #[test]
    fn navigating_context_uses_stack_bit() {
        assert_eq!(
            context_bit(&HelpContext::Navigating),
            context_bit(&HelpContext::StackFrames),
            "Navigating must have same bitmask as StackFrames"
        );
    }

    #[test]
    fn build_rows_navigating_same_line_count() {
        assert_eq!(
            build_rows(HelpContext::Navigating).len(),
            build_rows(HelpContext::StackFrames).len(),
        );
    }

    #[test]
    #[allow(non_snake_case)]
    fn help_bar_H_key_applicable_only_in_favorites() {
        let idx = ENTRIES.iter().position(|(k, _, _)| *k == "H").unwrap();
        assert_ne!(ENTRIES[idx].2 & context_bit(&HelpContext::Favorites), 0);
        assert_eq!(ENTRIES[idx].2 & context_bit(&HelpContext::ThreadList), 0);
        assert_eq!(ENTRIES[idx].2 & context_bit(&HelpContext::StackFrames), 0);
    }
}
