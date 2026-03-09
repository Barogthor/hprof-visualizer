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
    /// Quit the application.
    Quit,
}

/// Translates a [`KeyEvent`] into an [`InputEvent`], returning `None`
/// for events that have no TUI binding.
pub fn from_key(key: KeyEvent) -> Option<InputEvent> {
    match (key.code, key.modifiers) {
        (KeyCode::Char('q'), KeyModifiers::NONE) => Some(InputEvent::Quit),
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => Some(InputEvent::Quit),
        (KeyCode::Up, _) => Some(InputEvent::Up),
        (KeyCode::Down, _) => Some(InputEvent::Down),
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
