//! Centralized theme for the hprof-visualizer TUI.
//!
//! Defines [`Theme`] — a struct grouping all [`ratatui::style::Style`]
//! semantic roles — and the [`THEME`] constant singleton.
//!
//! All colors use the 16-color ANSI palette only (no 256-color or RGB).
//! Widgets MUST reference colors via `THEME` — never hardcode `Color::*`
//! literals elsewhere.
//!
//! ## Semantic color vocabulary
//!
//! | Role | Color | Usage |
//! |---|---|---|
//! | `thread_runnable` | Green | Runnable thread state dot |
//! | `thread_waiting` | Yellow | Waiting thread state dot |
//! | `thread_blocked` | Red | Blocked thread state dot |
//! | `thread_unknown` | DarkGray | Unknown thread state dot |
//! | `primitive_value` | Yellow | Numeric / bool / char field values |
//! | `string_value` | Green | String wrapper field values |
//! | `null_value` | DarkGray | Null values, secondary info |
//! | `object_id_hint` | DarkGray | Object ID metadata suffix (`@ 0x...`) |
//! | `cyclic_ref` | DarkGray | Cyclic/self-ref marker rows |
//! | `expand_indicator` | DarkGray | `+`/`-` expand toggle prefix |
//! | `loading_indicator` | Cyan | `~ Loading...` row text |
//! | `error_indicator` | Red | Failed expansion rows |
//! | `warning` | Yellow | Warning text in status bar |
//! | `selection_bg` | REVERSED | Selected row background |
//! | `selection_fg` | Default | (Reserved for future use) |
//! | `border_focused` | Bold White | Focused panel border |
//! | `border_unfocused` | DarkGray | Unfocused panel border |
//! | `status_bar_bg` | White on DarkGray | Status bar background |
//! | `search_active` | Cyan | Active search input text |

use ratatui::style::{Color, Modifier, Style};

/// Semantic style roles for the hprof-visualizer TUI.
///
/// All fields are [`Style`] values using the 16-color ANSI palette.
/// Instantiate via the [`THEME`] constant.
pub struct Theme {
    /// Runnable thread state dot color.
    pub thread_runnable: Style,
    /// Waiting thread state dot color.
    pub thread_waiting: Style,
    /// Blocked thread state dot color.
    pub thread_blocked: Style,
    /// Unknown thread state dot color.
    pub thread_unknown: Style,
    /// Numeric / bool / char field value row style.
    pub primitive_value: Style,
    /// String wrapper field value row style.
    pub string_value: Style,
    /// Null value and secondary info style.
    pub null_value: Style,
    /// Object ID metadata suffix style (`@ 0x...`).
    pub object_id_hint: Style,
    /// Cyclic/self-reference marker row style.
    pub cyclic_ref: Style,
    /// Expand/collapse toggle prefix style (`+` / `-`).
    pub expand_indicator: Style,
    /// Loading indicator row style (`~ Loading...`).
    pub loading_indicator: Style,
    /// Failed expansion row style.
    pub error_indicator: Style,
    /// Warning text style.
    pub warning: Style,
    /// Selected row background (reversed video).
    pub selection_bg: Style,
    /// Selected row foreground — reserved for future multi-panel use.
    pub selection_fg: Style,
    /// Focused panel border style.
    pub border_focused: Style,
    /// Unfocused panel border style.
    pub border_unfocused: Style,
    /// Status bar background style.
    pub status_bar_bg: Style,
    /// Active search input style.
    pub search_active: Style,
}

/// Singleton theme instance. Widgets reference colors via this constant.
///
/// ```rust
/// use hprof_tui::theme::THEME;
/// let style = THEME.thread_runnable;
/// ```
pub const THEME: Theme = Theme {
    thread_runnable: Style::new().fg(Color::Green),
    thread_waiting: Style::new().fg(Color::Yellow),
    thread_blocked: Style::new().fg(Color::Red),
    thread_unknown: Style::new().fg(Color::DarkGray),
    primitive_value: Style::new().fg(Color::Yellow),
    string_value: Style::new().fg(Color::Green),
    null_value: Style::new().fg(Color::DarkGray),
    object_id_hint: Style::new().fg(Color::DarkGray),
    cyclic_ref: Style::new().fg(Color::DarkGray),
    expand_indicator: Style::new().fg(Color::DarkGray),
    loading_indicator: Style::new().fg(Color::Cyan),
    error_indicator: Style::new().fg(Color::Red),
    warning: Style::new().fg(Color::Yellow),
    selection_bg: Style::new().add_modifier(Modifier::REVERSED),
    selection_fg: Style::new(),
    border_focused: Style::new().fg(Color::White).add_modifier(Modifier::BOLD),
    border_unfocused: Style::new().fg(Color::DarkGray),
    status_bar_bg: Style::new().fg(Color::White).bg(Color::DarkGray),
    search_active: Style::new().fg(Color::Cyan),
};

#[cfg(test)]
mod tests {
    use ratatui::style::Style;

    use super::*;

    #[test]
    fn all_theme_fields_are_of_type_style() {
        fn assert_style(_: Style) {}
        assert_style(THEME.thread_runnable);
        assert_style(THEME.thread_waiting);
        assert_style(THEME.thread_blocked);
        assert_style(THEME.thread_unknown);
        assert_style(THEME.primitive_value);
        assert_style(THEME.string_value);
        assert_style(THEME.null_value);
        assert_style(THEME.object_id_hint);
        assert_style(THEME.cyclic_ref);
        assert_style(THEME.expand_indicator);
        assert_style(THEME.loading_indicator);
        assert_style(THEME.error_indicator);
        assert_style(THEME.warning);
        assert_style(THEME.selection_bg);
        assert_style(THEME.selection_fg);
        assert_style(THEME.border_focused);
        assert_style(THEME.border_unfocused);
        assert_style(THEME.status_bar_bg);
        assert_style(THEME.search_active);
    }
}
