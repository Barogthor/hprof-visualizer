//! Keyboard event abstraction layer.
//!
//! Translates raw [`crossterm::event::KeyEvent`] into [`InputEvent`]
//! variants consumed by [`crate::app::App`]. Centralizing key bindings
//! here makes remapping straightforward.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// High-level TUI input events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    /// Move selection up one item.
    Up,
    /// Move selection down one item.
    Down,
    /// Expand the item at the current cursor position (stack view only).
    Right,
    /// Unexpand the current node, or navigate to its logical parent.
    Left,
    /// Jump to first item.
    Home,
    /// Jump to last item.
    End,
    /// Confirm selection / enter sub-panel.
    Enter,
    /// Cancel current action or go back.
    Escape,
    /// Activate search mode (thread list only).
    SearchActivate,
    /// A printable character typed during search.
    SearchChar(char),
    /// Delete last character in search input.
    SearchBackspace,
    /// Scroll up by one screen height.
    PageUp,
    /// Scroll down by one screen height.
    PageDown,
    /// Scroll the visible window up one line without moving the cursor (stack view only).
    CameraScrollUp,
    /// Scroll the visible window down one line without moving the cursor (stack view only).
    CameraScrollDown,
    /// Scroll the visible window up by one page without moving the cursor (stack view only).
    CameraPageUp,
    /// Scroll the visible window down by one page without moving the cursor (stack view only).
    CameraPageDown,
    /// Center the current selection in the visible window (stack view only).
    CameraCenterSelection,
    /// Pin/unpin the item at the current cursor position in the stack panel.
    ToggleFavorite,
    /// Move focus to/from the favorites panel.
    FocusFavorites,
    /// Cycle keyboard focus to the next panel.
    Tab,
    /// Toggle the keyboard shortcut help panel.
    ToggleHelp,
    /// Quit the application.
    Quit,
}

/// Translates a [`KeyEvent`] into an [`InputEvent`], returning `None`
/// for events that have no TUI binding.
pub fn from_key(key: KeyEvent) -> Option<InputEvent> {
    match (key.code, key.modifiers) {
        (KeyCode::Char('q'), KeyModifiers::NONE) => Some(InputEvent::Quit),
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => Some(InputEvent::Quit),
        (KeyCode::Up, mods)
            if mods.contains(KeyModifiers::CONTROL) || mods.contains(KeyModifiers::SHIFT) =>
        {
            Some(InputEvent::CameraScrollUp)
        }
        (KeyCode::Down, mods)
            if mods.contains(KeyModifiers::CONTROL) || mods.contains(KeyModifiers::SHIFT) =>
        {
            Some(InputEvent::CameraScrollDown)
        }
        (KeyCode::PageUp, mods)
            if mods.contains(KeyModifiers::CONTROL) || mods.contains(KeyModifiers::SHIFT) =>
        {
            Some(InputEvent::CameraPageUp)
        }
        (KeyCode::PageDown, mods)
            if mods.contains(KeyModifiers::CONTROL) || mods.contains(KeyModifiers::SHIFT) =>
        {
            Some(InputEvent::CameraPageDown)
        }
        (KeyCode::Char('l'), KeyModifiers::CONTROL) => Some(InputEvent::CameraCenterSelection),
        (KeyCode::Up, _) => Some(InputEvent::Up),
        (KeyCode::Down, _) => Some(InputEvent::Down),
        (KeyCode::Right, _) => Some(InputEvent::Right),
        (KeyCode::Left, _) => Some(InputEvent::Left),
        (KeyCode::Home, _) => Some(InputEvent::Home),
        (KeyCode::End, _) => Some(InputEvent::End),
        (KeyCode::PageUp, _) => Some(InputEvent::PageUp),
        (KeyCode::PageDown, _) => Some(InputEvent::PageDown),
        (KeyCode::Enter, _) => Some(InputEvent::Enter),
        (KeyCode::Esc, _) => Some(InputEvent::Escape),
        (KeyCode::Char('/'), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
            Some(InputEvent::SearchActivate)
        }
        (KeyCode::Backspace, _) => Some(InputEvent::SearchBackspace),
        (KeyCode::Char('f'), KeyModifiers::NONE) => Some(InputEvent::ToggleFavorite),
        (KeyCode::Char('F'), KeyModifiers::SHIFT) => Some(InputEvent::FocusFavorites),
        (KeyCode::Tab, _) => Some(InputEvent::Tab),
        (KeyCode::Char('?'), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
            Some(InputEvent::ToggleHelp)
        }
        (KeyCode::Char(c), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
            Some(InputEvent::SearchChar(c))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    use super::*;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn from_key_maps_quit_on_q() {
        assert_eq!(
            from_key(key(KeyCode::Char('q'), KeyModifiers::NONE)),
            Some(InputEvent::Quit)
        );
    }

    #[test]
    fn from_key_maps_quit_on_ctrl_c() {
        assert_eq!(
            from_key(key(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(InputEvent::Quit)
        );
    }

    #[test]
    fn from_key_maps_arrow_keys() {
        assert_eq!(
            from_key(key(KeyCode::Up, KeyModifiers::NONE)),
            Some(InputEvent::Up)
        );
        assert_eq!(
            from_key(key(KeyCode::Down, KeyModifiers::NONE)),
            Some(InputEvent::Down)
        );
        assert_eq!(
            from_key(key(KeyCode::Right, KeyModifiers::NONE)),
            Some(InputEvent::Right)
        );
        assert_eq!(
            from_key(key(KeyCode::Left, KeyModifiers::NONE)),
            Some(InputEvent::Left)
        );
        assert_eq!(
            from_key(key(KeyCode::Home, KeyModifiers::NONE)),
            Some(InputEvent::Home)
        );
        assert_eq!(
            from_key(key(KeyCode::End, KeyModifiers::NONE)),
            Some(InputEvent::End)
        );
    }

    #[test]
    fn from_key_maps_enter_and_escape() {
        assert_eq!(
            from_key(key(KeyCode::Enter, KeyModifiers::NONE)),
            Some(InputEvent::Enter)
        );
        assert_eq!(
            from_key(key(KeyCode::Esc, KeyModifiers::NONE)),
            Some(InputEvent::Escape)
        );
    }

    #[test]
    fn from_key_maps_search_activate_on_slash() {
        assert_eq!(
            from_key(key(KeyCode::Char('/'), KeyModifiers::NONE)),
            Some(InputEvent::SearchActivate)
        );
    }

    #[test]
    fn from_key_maps_search_activate_on_shift_slash() {
        assert_eq!(
            from_key(key(KeyCode::Char('/'), KeyModifiers::SHIFT)),
            Some(InputEvent::SearchActivate)
        );
    }

    #[test]
    fn from_key_maps_backspace_to_search_backspace() {
        assert_eq!(
            from_key(key(KeyCode::Backspace, KeyModifiers::NONE)),
            Some(InputEvent::SearchBackspace)
        );
    }

    #[test]
    fn from_key_maps_printable_chars_to_search_char() {
        assert_eq!(
            from_key(key(KeyCode::Char('a'), KeyModifiers::NONE)),
            Some(InputEvent::SearchChar('a'))
        );
        assert_eq!(
            from_key(key(KeyCode::Char('A'), KeyModifiers::SHIFT)),
            Some(InputEvent::SearchChar('A'))
        );
    }

    #[test]
    fn from_key_maps_f_to_toggle_favorite() {
        assert_eq!(
            from_key(key(KeyCode::Char('f'), KeyModifiers::NONE)),
            Some(InputEvent::ToggleFavorite)
        );
    }

    #[test]
    fn from_key_maps_shift_f_to_focus_favorites() {
        assert_eq!(
            from_key(key(KeyCode::Char('F'), KeyModifiers::SHIFT)),
            Some(InputEvent::FocusFavorites)
        );
    }

    #[test]
    fn from_key_maps_tab_to_tab() {
        assert_eq!(
            from_key(key(KeyCode::Tab, KeyModifiers::NONE)),
            Some(InputEvent::Tab)
        );
    }

    #[test]
    fn from_key_maps_question_mark_to_toggle_help() {
        assert_eq!(
            from_key(key(KeyCode::Char('?'), KeyModifiers::NONE)),
            Some(InputEvent::ToggleHelp)
        );
        assert_eq!(
            from_key(key(KeyCode::Char('?'), KeyModifiers::SHIFT)),
            Some(InputEvent::ToggleHelp)
        );
    }

    #[test]
    fn from_key_maps_s_to_search_char() {
        assert_eq!(
            from_key(key(KeyCode::Char('s'), KeyModifiers::NONE)),
            Some(InputEvent::SearchChar('s'))
        );
    }

    #[test]
    fn from_key_maps_ctrl_up_to_camera_scroll_up() {
        assert_eq!(
            from_key(key(KeyCode::Up, KeyModifiers::CONTROL)),
            Some(InputEvent::CameraScrollUp)
        );
    }

    #[test]
    fn from_key_maps_ctrl_down_to_camera_scroll_down() {
        assert_eq!(
            from_key(key(KeyCode::Down, KeyModifiers::CONTROL)),
            Some(InputEvent::CameraScrollDown)
        );
    }

    #[test]
    fn from_key_maps_shift_up_to_camera_scroll_up() {
        assert_eq!(
            from_key(key(KeyCode::Up, KeyModifiers::SHIFT)),
            Some(InputEvent::CameraScrollUp)
        );
    }

    #[test]
    fn from_key_maps_shift_down_to_camera_scroll_down() {
        assert_eq!(
            from_key(key(KeyCode::Down, KeyModifiers::SHIFT)),
            Some(InputEvent::CameraScrollDown)
        );
    }

    #[test]
    fn from_key_maps_ctrl_l_to_camera_center_selection() {
        assert_eq!(
            from_key(key(KeyCode::Char('l'), KeyModifiers::CONTROL)),
            Some(InputEvent::CameraCenterSelection)
        );
    }

    #[test]
    fn from_key_maps_ctrl_page_up_to_camera_page_up() {
        assert_eq!(
            from_key(key(KeyCode::PageUp, KeyModifiers::CONTROL)),
            Some(InputEvent::CameraPageUp)
        );
    }

    #[test]
    fn from_key_maps_ctrl_page_down_to_camera_page_down() {
        assert_eq!(
            from_key(key(KeyCode::PageDown, KeyModifiers::CONTROL)),
            Some(InputEvent::CameraPageDown)
        );
    }

    #[test]
    fn from_key_maps_shift_page_up_to_camera_page_up() {
        assert_eq!(
            from_key(key(KeyCode::PageUp, KeyModifiers::SHIFT)),
            Some(InputEvent::CameraPageUp)
        );
    }

    #[test]
    fn from_key_maps_shift_page_down_to_camera_page_down() {
        assert_eq!(
            from_key(key(KeyCode::PageDown, KeyModifiers::SHIFT)),
            Some(InputEvent::CameraPageDown)
        );
    }

    #[test]
    fn from_key_plain_up_still_maps_to_up() {
        assert_eq!(
            from_key(key(KeyCode::Up, KeyModifiers::NONE)),
            Some(InputEvent::Up)
        );
    }

    #[test]
    fn ctrl_up_does_not_map_to_up() {
        assert_ne!(
            from_key(key(KeyCode::Up, KeyModifiers::CONTROL)),
            Some(InputEvent::Up)
        );
    }

    #[test]
    fn from_key_returns_none_for_unbound_keys() {
        assert_eq!(from_key(key(KeyCode::F(1), KeyModifiers::NONE)), None);
    }

    #[test]
    fn from_key_maps_page_up_and_page_down() {
        assert_eq!(
            from_key(key(KeyCode::PageUp, KeyModifiers::NONE)),
            Some(InputEvent::PageUp)
        );
        assert_eq!(
            from_key(key(KeyCode::PageDown, KeyModifiers::NONE)),
            Some(InputEvent::PageDown)
        );
    }
}
