//! Generic cursor/navigation state shared by list-based views.

use ratatui::widgets::ListState;

/// Cursor + list scroll state for a flat list of stable IDs.
pub struct CursorState<Id: PartialEq + Clone> {
    cursor: Id,
    list_state: ListState,
    visible_height: usize,
}

impl<Id: PartialEq + Clone> CursorState<Id> {
    /// Creates a new cursor state with default page size = 1.
    pub fn new(initial: Id) -> Self {
        Self {
            cursor: initial,
            list_state: ListState::default(),
            visible_height: 1,
        }
    }

    /// Returns the current cursor ID.
    pub fn cursor(&self) -> &Id {
        &self.cursor
    }

    /// Returns mutable access to ratatui list state for rendering.
    pub fn list_state_mut(&mut self) -> &mut ListState {
        &mut self.list_state
    }

    /// Sets the visible list height used by page navigation.
    pub fn set_visible_height(&mut self, h: usize) {
        self.visible_height = h;
    }

    fn cursor_index(&self, items: &[Id]) -> Option<usize> {
        items.iter().position(|id| id == &self.cursor)
    }

    /// Syncs list selection with current cursor if cursor is visible in `items`.
    pub fn sync(&mut self, items: &[Id]) {
        self.list_state.select(self.cursor_index(items));
    }

    /// Syncs selection and re-anchors cursor to first item when orphaned.
    pub fn sync_or_select_first(&mut self, items: &[Id]) {
        if items.is_empty() {
            self.list_state.select(None);
            return;
        }
        if let Some(selected) = self.cursor_index(items) {
            self.list_state.select(Some(selected));
        } else {
            self.cursor = items[0].clone();
            self.list_state.select(Some(0));
        }
    }

    /// Replaces cursor and immediately syncs list selection against `items`.
    pub fn set_cursor_and_sync(&mut self, cursor: Id, items: &[Id]) {
        self.cursor = cursor;
        self.sync(items);
    }

    /// Moves cursor one item up (clamped).
    pub fn move_up(&mut self, items: &[Id]) {
        if items.is_empty() {
            return;
        }
        let Some(current) = self.cursor_index(items) else {
            self.move_home(items);
            return;
        };
        let target = current.saturating_sub(1);
        self.cursor = items[target].clone();
        self.list_state.select(Some(target));
    }

    /// Moves cursor one item down (clamped).
    pub fn move_down(&mut self, items: &[Id]) {
        if items.is_empty() {
            return;
        }
        let Some(current) = self.cursor_index(items) else {
            self.move_home(items);
            return;
        };
        let target = (current + 1).min(items.len().saturating_sub(1));
        self.cursor = items[target].clone();
        self.list_state.select(Some(target));
    }

    /// Moves cursor to first item.
    pub fn move_home(&mut self, items: &[Id]) {
        if items.is_empty() {
            return;
        }
        self.cursor = items[0].clone();
        self.list_state.select(Some(0));
    }

    /// Moves cursor to last item.
    pub fn move_end(&mut self, items: &[Id]) {
        if items.is_empty() {
            return;
        }
        let last = items.len().saturating_sub(1);
        self.cursor = items[last].clone();
        self.list_state.select(Some(last));
    }

    /// Moves cursor one page up (clamped).
    pub fn move_page_up(&mut self, items: &[Id]) {
        if items.is_empty() {
            return;
        }
        let Some(current) = self.cursor_index(items) else {
            self.move_home(items);
            return;
        };
        let target = current.saturating_sub(self.visible_height);
        self.cursor = items[target].clone();
        self.list_state.select(Some(target));
    }

    /// Moves cursor one page down (clamped).
    pub fn move_page_down(&mut self, items: &[Id]) {
        if items.is_empty() {
            return;
        }
        let Some(current) = self.cursor_index(items) else {
            self.move_home(items);
            return;
        };
        let target = (current + self.visible_height).min(items.len().saturating_sub(1));
        self.cursor = items[target].clone();
        self.list_state.select(Some(target));
    }
}

#[cfg(test)]
mod tests {
    use super::CursorState;

    #[test]
    fn move_down_moves_cursor_and_list_state() {
        let mut state = CursorState::new(0u32);
        let items = [0u32, 1, 2];
        state.move_down(&items);
        assert_eq!(state.cursor(), &1);
        assert_eq!(state.list_state.selected(), Some(1));
    }

    #[test]
    fn move_down_at_end_clamps() {
        let mut state = CursorState::new(2u32);
        let items = [0u32, 1, 2];
        state.move_down(&items);
        assert_eq!(state.cursor(), &2);
        assert_eq!(state.list_state.selected(), Some(2));
    }

    #[test]
    fn move_up_at_start_clamps() {
        let mut state = CursorState::new(0u32);
        let items = [0u32, 1, 2];
        state.move_up(&items);
        assert_eq!(state.cursor(), &0);
        assert_eq!(state.list_state.selected(), Some(0));
    }

    #[test]
    fn move_home_and_end_select_bounds() {
        let mut state = CursorState::new(1u32);
        let items = [10u32, 20, 30];
        state.move_home(&items);
        assert_eq!(state.cursor(), &10);
        assert_eq!(state.list_state.selected(), Some(0));

        state.move_end(&items);
        assert_eq!(state.cursor(), &30);
        assert_eq!(state.list_state.selected(), Some(2));
    }

    #[test]
    fn page_down_moves_by_visible_height() {
        let mut state = CursorState::new(0u32);
        let items: Vec<u32> = (0..10).collect();
        state.set_visible_height(3);
        state.move_page_down(&items);
        assert_eq!(state.cursor(), &3);
        assert_eq!(state.list_state.selected(), Some(3));
    }

    #[test]
    fn page_down_clamps_to_last_when_height_exceeds_len() {
        let mut state = CursorState::new(0u32);
        let items = [0u32, 1, 2];
        state.set_visible_height(10);
        state.move_page_down(&items);
        assert_eq!(state.cursor(), &2);
        assert_eq!(state.list_state.selected(), Some(2));
    }

    #[test]
    fn page_down_uses_default_height_one() {
        let mut state = CursorState::new(0u32);
        let items = [0u32, 1, 2];
        state.move_page_down(&items);
        assert_eq!(state.cursor(), &1);
        assert_eq!(state.list_state.selected(), Some(1));
    }

    #[test]
    fn move_methods_are_noop_on_empty_items() {
        let mut state = CursorState::new(0u32);
        let empty: [u32; 0] = [];

        state.move_up(&empty);
        state.move_down(&empty);
        state.move_home(&empty);
        state.move_end(&empty);
        state.move_page_up(&empty);
        state.move_page_down(&empty);

        assert_eq!(state.cursor(), &0);
        assert_eq!(state.list_state.selected(), None);
    }

    #[test]
    fn page_down_single_item_does_not_underflow() {
        let mut state = CursorState::new(0u32);
        let items = [0u32];
        state.set_visible_height(5);
        state.move_page_down(&items);
        assert_eq!(state.cursor(), &0);
        assert_eq!(state.list_state.selected(), Some(0));
    }

    #[test]
    fn sync_or_select_first_reanchors_orphan_cursor() {
        let mut state = CursorState::new(42u32);
        let items = [10u32, 20, 30];
        state.sync_or_select_first(&items);
        assert_eq!(state.cursor(), &10);
        assert_eq!(state.list_state.selected(), Some(0));
    }

    #[test]
    fn move_down_on_orphan_cursor_reanchors_first() {
        let mut state = CursorState::new(99u32);
        let items = [10u32, 20, 30];
        state.move_down(&items);
        assert_eq!(state.cursor(), &10);
        assert_eq!(state.list_state.selected(), Some(0));
    }

    #[test]
    fn page_down_with_visible_height_zero_is_noop() {
        let mut state = CursorState::new(0u32);
        let items = [0u32, 1, 2];
        state.set_visible_height(0);
        state.move_page_down(&items);
        assert_eq!(state.cursor(), &0);
        assert_eq!(state.list_state.selected(), Some(0));
    }
}
