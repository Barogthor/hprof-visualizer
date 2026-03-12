//! [`StackView`] — stateful ratatui widget for the stack frame panel.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    widgets::{Block, BorderType, Borders, List, StatefulWidget, Widget},
};

use crate::theme::THEME;

use super::state::StackState;

/// Stateful widget for the stack frame panel.
pub struct StackView {
    /// Whether this panel has keyboard focus.
    pub focused: bool,
    /// Whether object IDs are displayed inline for object references.
    pub show_object_ids: bool,
}

impl StatefulWidget for StackView {
    type State = StackState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let border_style = if self.focused {
            THEME.border_focused
        } else {
            THEME.border_unfocused
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Plain)
            .border_style(border_style)
            .title("Stack Frames");
        let inner = block.inner(area);
        block.render(area, buf);

        let items = state.build_items_with_object_ids(self.show_object_ids);
        let list = List::new(items).highlight_style(THEME.selection_bg);
        StatefulWidget::render(list, inner, buf, state.list_state_mut());
    }
}
