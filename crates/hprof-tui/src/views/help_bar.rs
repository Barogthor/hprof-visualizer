//! Keyboard shortcut help panel, rendered as a non-focusable bottom section.
//!
//! [`HelpBar`] is a stateless widget displaying a two-column keymap table.
//! Toggle visibility via `?` from any panel. Call [`required_height`] to
//! determine the layout slot to reserve.

use crossterm::event::KeyCode;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Widget},
};

use crate::{keymap::Keymap, theme::THEME};

/// Number of keymap entries documented in the help panel.
const ENTRY_COUNT: u16 = 23;

// Context bitmask constants — private to this module.
const THREAD: u8 = 0b001;
const STACK: u8 = 0b010;
const FAV: u8 = 0b100;
const ALL: u8 = 0b111;

/// Format a [`KeyCode`] for display in the help panel.
///
/// Single printable chars are returned as their character; other codes
/// fall back to their debug representation (not expected in configurable
/// bindings).
fn key_label(code: KeyCode) -> String {
    match code {
        KeyCode::Char(c) => c.to_string(),
        other => format!("{other:?}"),
    }
}

/// Build the dynamic keymap entries for the help panel.
///
/// Returns `(key_label, action_label, context_mask)` tuples. Key labels
/// are derived from `keymap` for configurable bindings; all other labels
/// are hardcoded (layout-independent).
pub fn help_entries(keymap: &Keymap) -> Vec<(String, &'static str, u8)> {
    let q = key_label(keymap.quit);
    let c = key_label(keymap.batch_expand);
    let b = key_label(keymap.prev_pin);
    let n = key_label(keymap.next_pin);
    let f = key_label(keymap.toggle_favorite);
    let shift_f = key_label(keymap.focus_favorites);
    let g = key_label(keymap.navigate_to_source);
    let h = key_label(keymap.hide_field);
    let shift_h = key_label(keymap.reveal_hidden);
    let i = key_label(keymap.toggle_object_ids);
    let s = key_label(keymap.search_activate);

    vec![
        (format!("{q} / Ctrl+C"), "Quit", ALL),
        ("Esc".to_string(), "Search off -> clear filter -> back", ALL),
        ("Tab".to_string(), "Cycle panel focus", ALL),
        ("\u{2191} / \u{2193}".to_string(), "Move selection", ALL),
        ("PgUp / PgDn".to_string(), "Scroll one page", ALL),
        ("Ctrl/Shift+\u{2191}".to_string(), "Scroll view up", STACK),
        ("Ctrl/Shift+\u{2193}".to_string(), "Scroll view down", STACK),
        (
            "Ctrl/Shift+PgUp/PgDn".to_string(),
            "Scroll view one page",
            STACK,
        ),
        ("Ctrl+L".to_string(), "Center selection", STACK),
        ("Home / End".to_string(), "Jump to first / last", ALL),
        ("Enter".to_string(), "Expand / confirm", THREAD | STACK),
        ("\u{2192}".to_string(), "Expand node", STACK),
        ("\u{2190}".to_string(), "Unexpand / go to parent", STACK),
        (c, "Re-expand collapsed", STACK | FAV),
        (format!("{b} / {n}"), "Favorites: prev/next pin", FAV),
        (f, "Pin / unpin favorite", STACK | FAV),
        (shift_f, "Focus favorites panel", ALL),
        (g, "Favorites: go to source", FAV),
        (h, "Favorites: hide / show field", FAV),
        (shift_h, "Favorites: reveal / hide hidden", FAV),
        (i, "Toggle object IDs (stack)", STACK),
        (
            format!("{s} or /"),
            "Open search (thread list only)",
            THREAD,
        ),
        ("?".to_string(), "Toggle help panel", ALL),
    ]
}

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
/// Construct with a [`HelpContext`] and a [`Keymap`] to apply context-aware
/// dimming and display the active key bindings for the current layout.
pub struct HelpBar {
    pub context: HelpContext,
    pub keymap: Keymap,
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
/// With `ENTRY_COUNT = 23`: `2 + 1 + 12 + 1 = 16`.
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

        let rows = build_rows(self.context, &self.keymap);
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
pub(crate) fn build_rows(ctx: HelpContext, keymap: &Keymap) -> Vec<Line<'static>> {
    let key_col_width: usize = 18;
    let entry_col_width: usize = 36;
    let ctx_bit = context_bit(&ctx);

    let entries = help_entries(keymap);
    let mut lines: Vec<Line<'static>> = Vec::new();

    // Blank padding row.
    lines.push(Line::from(""));

    let mut i = 0;
    while i < entries.len() {
        let (key_a, action_a, mask_a) = &entries[i];
        // Override Esc label when navigating.
        let action_a: &str = if ctx == HelpContext::Navigating && key_a == "Esc" {
            "Cancel navigation"
        } else {
            action_a
        };
        let applicable_a = ctx_bit & mask_a != 0;
        let left_key = format!("  {:width$}", key_a, width = key_col_width);
        let left_action = format!("{:<width$}", action_a, width = entry_col_width);

        let spans: Vec<Span<'static>> = if i + 1 < entries.len() {
            let (key_b, action_b, mask_b) = &entries[i + 1];
            let action_b: &str = if ctx == HelpContext::Navigating && key_b == "Esc" {
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
    use crate::keymap::Keymap;

    fn km() -> Keymap {
        Keymap::default()
    }

    #[test]
    fn required_height_returns_sixteen_for_twenty_three_entries() {
        // div_ceil(23, 2) = 12; 2 + 1 + 12 + 1 = 16
        assert_eq!(required_height(), 16);
    }

    #[test]
    fn entry_count_matches_help_entries_for_azerty() {
        assert_eq!(ENTRY_COUNT as usize, help_entries(&Keymap::default()).len());
    }

    #[test]
    fn entry_count_matches_help_entries_for_qwerty() {
        use crate::keymap::KeymapPreset;
        assert_eq!(
            ENTRY_COUNT as usize,
            help_entries(&KeymapPreset::Qwerty.build()).len()
        );
    }

    #[test]
    fn build_rows_produces_correct_line_count() {
        // 1 padding + ceil(23/2) + 1 separator = 1 + 12 + 1 = 14
        assert_eq!(build_rows(HelpContext::ThreadList, &km()).len(), 14);
        assert_eq!(build_rows(HelpContext::StackFrames, &km()).len(), 14);
        assert_eq!(build_rows(HelpContext::Favorites, &km()).len(), 14);
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
        let entries = help_entries(&km());
        let idx = entries
            .iter()
            .position(|(k, _, _)| k.contains("s or"))
            .unwrap();
        assert_ne!(entries[idx].2 & context_bit(&HelpContext::ThreadList), 0);
        assert_eq!(entries[idx].2 & context_bit(&HelpContext::StackFrames), 0);
        assert_eq!(entries[idx].2 & context_bit(&HelpContext::Favorites), 0);
    }

    #[test]
    fn help_bar_camera_scroll_applicable_only_in_stack_frames() {
        let entries = help_entries(&km());
        let idx = entries
            .iter()
            .position(|(k, _, _)| k.contains("Ctrl/Shift+\u{2191}"))
            .unwrap();
        assert_ne!(entries[idx].2 & context_bit(&HelpContext::StackFrames), 0);
        assert_eq!(entries[idx].2 & context_bit(&HelpContext::ThreadList), 0);
        assert_eq!(entries[idx].2 & context_bit(&HelpContext::Favorites), 0);
    }

    #[test]
    fn help_bar_f_key_applicable_in_stack_and_favorites_not_thread() {
        let entries = help_entries(&km());
        let idx = entries
            .iter()
            .position(|(k, _, _)| k.as_str() == "f")
            .unwrap();
        assert_ne!(entries[idx].2 & context_bit(&HelpContext::StackFrames), 0);
        assert_ne!(entries[idx].2 & context_bit(&HelpContext::Favorites), 0);
        assert_eq!(entries[idx].2 & context_bit(&HelpContext::ThreadList), 0);
    }

    #[test]
    fn help_bar_global_entries_applicable_in_all_contexts() {
        let entries = help_entries(&km());
        for key_label in ["q / Ctrl+C", "Esc", "?"] {
            let idx = entries
                .iter()
                .position(|(k, _, _)| k.as_str() == key_label)
                .unwrap();
            assert_eq!(entries[idx].2, ALL, "mask for '{key_label}' should be ALL");
        }
    }

    #[test]
    fn help_bar_all_entries_have_valid_mask() {
        for (key, _action, mask) in help_entries(&km()) {
            assert!(mask != 0 && mask <= ALL, "invalid mask for entry '{key}'");
        }
    }

    #[test]
    fn help_bar_h_key_applicable_only_in_favorites() {
        let entries = help_entries(&km());
        let idx = entries
            .iter()
            .position(|(k, _, _)| k.as_str() == "h")
            .unwrap();
        assert_ne!(entries[idx].2 & context_bit(&HelpContext::Favorites), 0);
        assert_eq!(entries[idx].2 & context_bit(&HelpContext::ThreadList), 0);
        assert_eq!(entries[idx].2 & context_bit(&HelpContext::StackFrames), 0);
    }

    #[test]
    fn help_bar_c_key_applicable_in_stack_and_favorites() {
        let entries = help_entries(&km());
        let idx = entries
            .iter()
            .position(|(k, _, _)| k.as_str() == "c")
            .unwrap();
        assert_ne!(entries[idx].2 & context_bit(&HelpContext::StackFrames), 0);
        assert_ne!(entries[idx].2 & context_bit(&HelpContext::Favorites), 0);
        assert_eq!(entries[idx].2 & context_bit(&HelpContext::ThreadList), 0);
    }

    #[test]
    fn help_bar_bn_keys_applicable_only_in_favorites() {
        let entries = help_entries(&km());
        let idx = entries
            .iter()
            .position(|(k, _, _)| k.contains("b / n"))
            .unwrap();
        assert_ne!(entries[idx].2 & context_bit(&HelpContext::Favorites), 0);
        assert_eq!(entries[idx].2 & context_bit(&HelpContext::ThreadList), 0);
        assert_eq!(entries[idx].2 & context_bit(&HelpContext::StackFrames), 0);
    }

    #[test]
    fn navigating_context_overrides_esc_label() {
        let rows = build_rows(HelpContext::Navigating, &km());
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
            build_rows(HelpContext::Navigating, &km()).len(),
            build_rows(HelpContext::StackFrames, &km()).len(),
        );
    }

    #[test]
    #[allow(non_snake_case)]
    fn help_bar_H_key_applicable_only_in_favorites() {
        let entries = help_entries(&km());
        let idx = entries
            .iter()
            .position(|(k, _, _)| k.as_str() == "H")
            .unwrap();
        assert_ne!(entries[idx].2 & context_bit(&HelpContext::Favorites), 0);
        assert_eq!(entries[idx].2 & context_bit(&HelpContext::ThreadList), 0);
        assert_eq!(entries[idx].2 & context_bit(&HelpContext::StackFrames), 0);
    }
}
