//! Keyboard event abstraction layer.
//!
//! Translates raw [`crossterm::event::KeyEvent`] into [`InputEvent`]
//! variants consumed by [`crate::app::App`]. Centralizing key bindings
//! here makes remapping straightforward.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::keymap::Keymap;

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
    /// Navigate from favorites to source thread/frame.
    NavigateToSource,
    /// Toggle object id suffix display in stack frames.
    ToggleObjectIds,
    /// Cycle keyboard focus to the next panel.
    Tab,
    /// Toggle the keyboard shortcut help panel.
    ToggleHelp,
    /// Quit the application.
    Quit,
    /// Hide or show the field at the cursor in the favorites panel.
    HideField,
    /// Reveal all hidden fields in the current pinned snapshot.
    RevealHidden,
    /// Jump to the previous pinned item header in the favorites panel.
    PrevPin,
    /// Jump to the next pinned item header in the favorites panel.
    NextPin,
    /// Batch-expand the current pinned item or stack frame node.
    BatchExpand,
}

/// Translates a [`KeyEvent`] into an [`InputEvent`] using the active
/// [`Keymap`] for configurable bindings. Returns `None` for events
/// that have no TUI binding.
///
/// Layout-independent keys (arrows, Enter, Esc, Ctrl modifiers, …) are
/// hardcoded. Configurable single-character keys are looked up via `keymap`.
pub fn from_key(key: KeyEvent, keymap: &Keymap) -> Option<InputEvent> {
    // --- Layout-independent hardcoded keys ---
    match (key.code, key.modifiers) {
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => return Some(InputEvent::Quit),
        (KeyCode::Up, mods)
            if mods.contains(KeyModifiers::CONTROL) || mods.contains(KeyModifiers::SHIFT) =>
        {
            return Some(InputEvent::CameraScrollUp);
        }
        (KeyCode::Down, mods)
            if mods.contains(KeyModifiers::CONTROL) || mods.contains(KeyModifiers::SHIFT) =>
        {
            return Some(InputEvent::CameraScrollDown);
        }
        (KeyCode::PageUp, mods)
            if mods.contains(KeyModifiers::CONTROL) || mods.contains(KeyModifiers::SHIFT) =>
        {
            return Some(InputEvent::CameraPageUp);
        }
        (KeyCode::PageDown, mods)
            if mods.contains(KeyModifiers::CONTROL) || mods.contains(KeyModifiers::SHIFT) =>
        {
            return Some(InputEvent::CameraPageDown);
        }
        (KeyCode::Char('l'), KeyModifiers::CONTROL) => {
            return Some(InputEvent::CameraCenterSelection);
        }
        (KeyCode::Up, _) => return Some(InputEvent::Up),
        (KeyCode::Down, _) => return Some(InputEvent::Down),
        (KeyCode::Right, _) => return Some(InputEvent::Right),
        (KeyCode::Left, _) => return Some(InputEvent::Left),
        (KeyCode::Home, _) => return Some(InputEvent::Home),
        (KeyCode::End, _) => return Some(InputEvent::End),
        (KeyCode::PageUp, _) => return Some(InputEvent::PageUp),
        (KeyCode::PageDown, _) => return Some(InputEvent::PageDown),
        (KeyCode::Enter, _) => return Some(InputEvent::Enter),
        (KeyCode::Esc, _) => return Some(InputEvent::Escape),
        (KeyCode::Char('/'), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
            return Some(InputEvent::SearchActivate);
        }
        (KeyCode::Backspace, _) => return Some(InputEvent::SearchBackspace),
        (KeyCode::Tab, _) => return Some(InputEvent::Tab),
        (KeyCode::Char('?'), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
            return Some(InputEvent::ToggleHelp);
        }
        _ => {}
    }

    // --- Configurable single-char keys via keymap ---
    if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT {
        let code = key.code;
        if code == keymap.quit {
            return Some(InputEvent::Quit);
        }
        if code == keymap.toggle_favorite {
            return Some(InputEvent::ToggleFavorite);
        }
        if code == keymap.focus_favorites {
            return Some(InputEvent::FocusFavorites);
        }
        if code == keymap.navigate_to_source {
            return Some(InputEvent::NavigateToSource);
        }
        if code == keymap.toggle_object_ids {
            return Some(InputEvent::ToggleObjectIds);
        }
        if code == keymap.hide_field {
            return Some(InputEvent::HideField);
        }
        if code == keymap.reveal_hidden {
            return Some(InputEvent::RevealHidden);
        }
        if code == keymap.prev_pin {
            return Some(InputEvent::PrevPin);
        }
        if code == keymap.next_pin {
            return Some(InputEvent::NextPin);
        }
        if code == keymap.batch_expand {
            return Some(InputEvent::BatchExpand);
        }
        if code == keymap.search_activate {
            return Some(InputEvent::SearchActivate);
        }
    }

    // --- SearchChar catch-all for unbound printable keys ---
    if let (KeyCode::Char(c), KeyModifiers::NONE | KeyModifiers::SHIFT) = (key.code, key.modifiers)
    {
        return Some(InputEvent::SearchChar(c));
    }

    None
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    use super::*;
    use crate::keymap::Keymap;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn km() -> Keymap {
        Keymap::default()
    }

    #[test]
    fn from_key_maps_quit_on_q() {
        assert_eq!(
            from_key(key(KeyCode::Char('q'), KeyModifiers::NONE), &km()),
            Some(InputEvent::Quit)
        );
    }

    #[test]
    fn from_key_maps_quit_on_ctrl_c() {
        assert_eq!(
            from_key(key(KeyCode::Char('c'), KeyModifiers::CONTROL), &km()),
            Some(InputEvent::Quit)
        );
    }

    #[test]
    fn from_key_maps_arrow_keys() {
        assert_eq!(
            from_key(key(KeyCode::Up, KeyModifiers::NONE), &km()),
            Some(InputEvent::Up)
        );
        assert_eq!(
            from_key(key(KeyCode::Down, KeyModifiers::NONE), &km()),
            Some(InputEvent::Down)
        );
        assert_eq!(
            from_key(key(KeyCode::Right, KeyModifiers::NONE), &km()),
            Some(InputEvent::Right)
        );
        assert_eq!(
            from_key(key(KeyCode::Left, KeyModifiers::NONE), &km()),
            Some(InputEvent::Left)
        );
        assert_eq!(
            from_key(key(KeyCode::Home, KeyModifiers::NONE), &km()),
            Some(InputEvent::Home)
        );
        assert_eq!(
            from_key(key(KeyCode::End, KeyModifiers::NONE), &km()),
            Some(InputEvent::End)
        );
    }

    #[test]
    fn from_key_maps_enter_and_escape() {
        assert_eq!(
            from_key(key(KeyCode::Enter, KeyModifiers::NONE), &km()),
            Some(InputEvent::Enter)
        );
        assert_eq!(
            from_key(key(KeyCode::Esc, KeyModifiers::NONE), &km()),
            Some(InputEvent::Escape)
        );
    }

    #[test]
    fn from_key_maps_search_activate_on_slash() {
        assert_eq!(
            from_key(key(KeyCode::Char('/'), KeyModifiers::NONE), &km()),
            Some(InputEvent::SearchActivate)
        );
    }

    #[test]
    fn from_key_maps_search_activate_on_shift_slash() {
        assert_eq!(
            from_key(key(KeyCode::Char('/'), KeyModifiers::SHIFT), &km()),
            Some(InputEvent::SearchActivate)
        );
    }

    #[test]
    fn from_key_maps_backspace_to_search_backspace() {
        assert_eq!(
            from_key(key(KeyCode::Backspace, KeyModifiers::NONE), &km()),
            Some(InputEvent::SearchBackspace)
        );
    }

    #[test]
    fn from_key_maps_printable_chars_to_search_char() {
        assert_eq!(
            from_key(key(KeyCode::Char('a'), KeyModifiers::NONE), &km()),
            Some(InputEvent::SearchChar('a'))
        );
        assert_eq!(
            from_key(key(KeyCode::Char('A'), KeyModifiers::SHIFT), &km()),
            Some(InputEvent::SearchChar('A'))
        );
    }

    #[test]
    fn from_key_maps_f_to_toggle_favorite() {
        assert_eq!(
            from_key(key(KeyCode::Char('f'), KeyModifiers::NONE), &km()),
            Some(InputEvent::ToggleFavorite)
        );
    }

    #[test]
    fn from_key_maps_shift_f_to_focus_favorites() {
        assert_eq!(
            from_key(key(KeyCode::Char('F'), KeyModifiers::SHIFT), &km()),
            Some(InputEvent::FocusFavorites)
        );
    }

    #[test]
    fn from_key_maps_g_to_navigate_to_source() {
        assert_eq!(
            from_key(key(KeyCode::Char('g'), KeyModifiers::NONE), &km()),
            Some(InputEvent::NavigateToSource)
        );
    }

    #[test]
    fn from_key_maps_i_to_toggle_object_ids() {
        assert_eq!(
            from_key(key(KeyCode::Char('i'), KeyModifiers::NONE), &km()),
            Some(InputEvent::ToggleObjectIds)
        );
    }

    #[test]
    fn from_key_maps_tab_to_tab() {
        assert_eq!(
            from_key(key(KeyCode::Tab, KeyModifiers::NONE), &km()),
            Some(InputEvent::Tab)
        );
    }

    #[test]
    fn from_key_maps_question_mark_to_toggle_help() {
        assert_eq!(
            from_key(key(KeyCode::Char('?'), KeyModifiers::NONE), &km()),
            Some(InputEvent::ToggleHelp)
        );
        assert_eq!(
            from_key(key(KeyCode::Char('?'), KeyModifiers::SHIFT), &km()),
            Some(InputEvent::ToggleHelp)
        );
    }

    #[test]
    fn from_key_maps_s_to_search_activate() {
        assert_eq!(
            from_key(key(KeyCode::Char('s'), KeyModifiers::NONE), &km()),
            Some(InputEvent::SearchActivate)
        );
    }

    #[test]
    fn from_key_maps_h_to_hide_field() {
        assert_eq!(
            from_key(key(KeyCode::Char('h'), KeyModifiers::NONE), &km()),
            Some(InputEvent::HideField)
        );
    }

    #[test]
    fn from_key_maps_shift_h_to_reveal_hidden() {
        assert_eq!(
            from_key(key(KeyCode::Char('H'), KeyModifiers::SHIFT), &km()),
            Some(InputEvent::RevealHidden)
        );
    }

    #[test]
    fn from_key_maps_b_to_prev_pin() {
        assert_eq!(
            from_key(key(KeyCode::Char('b'), KeyModifiers::NONE), &km()),
            Some(InputEvent::PrevPin)
        );
    }

    #[test]
    fn from_key_maps_n_to_next_pin() {
        assert_eq!(
            from_key(key(KeyCode::Char('n'), KeyModifiers::NONE), &km()),
            Some(InputEvent::NextPin)
        );
    }

    #[test]
    fn from_key_maps_c_to_batch_expand() {
        assert_eq!(
            from_key(key(KeyCode::Char('c'), KeyModifiers::NONE), &km()),
            Some(InputEvent::BatchExpand)
        );
    }

    #[test]
    fn from_key_maps_ctrl_up_to_camera_scroll_up() {
        assert_eq!(
            from_key(key(KeyCode::Up, KeyModifiers::CONTROL), &km()),
            Some(InputEvent::CameraScrollUp)
        );
    }

    #[test]
    fn from_key_maps_ctrl_down_to_camera_scroll_down() {
        assert_eq!(
            from_key(key(KeyCode::Down, KeyModifiers::CONTROL), &km()),
            Some(InputEvent::CameraScrollDown)
        );
    }

    #[test]
    fn from_key_maps_shift_up_to_camera_scroll_up() {
        assert_eq!(
            from_key(key(KeyCode::Up, KeyModifiers::SHIFT), &km()),
            Some(InputEvent::CameraScrollUp)
        );
    }

    #[test]
    fn from_key_maps_shift_down_to_camera_scroll_down() {
        assert_eq!(
            from_key(key(KeyCode::Down, KeyModifiers::SHIFT), &km()),
            Some(InputEvent::CameraScrollDown)
        );
    }

    #[test]
    fn from_key_maps_ctrl_l_to_camera_center_selection() {
        assert_eq!(
            from_key(key(KeyCode::Char('l'), KeyModifiers::CONTROL), &km()),
            Some(InputEvent::CameraCenterSelection)
        );
    }

    #[test]
    fn from_key_maps_ctrl_page_up_to_camera_page_up() {
        assert_eq!(
            from_key(key(KeyCode::PageUp, KeyModifiers::CONTROL), &km()),
            Some(InputEvent::CameraPageUp)
        );
    }

    #[test]
    fn from_key_maps_ctrl_page_down_to_camera_page_down() {
        assert_eq!(
            from_key(key(KeyCode::PageDown, KeyModifiers::CONTROL), &km()),
            Some(InputEvent::CameraPageDown)
        );
    }

    #[test]
    fn from_key_maps_shift_page_up_to_camera_page_up() {
        assert_eq!(
            from_key(key(KeyCode::PageUp, KeyModifiers::SHIFT), &km()),
            Some(InputEvent::CameraPageUp)
        );
    }

    #[test]
    fn from_key_maps_shift_page_down_to_camera_page_down() {
        assert_eq!(
            from_key(key(KeyCode::PageDown, KeyModifiers::SHIFT), &km()),
            Some(InputEvent::CameraPageDown)
        );
    }

    #[test]
    fn from_key_plain_up_still_maps_to_up() {
        assert_eq!(
            from_key(key(KeyCode::Up, KeyModifiers::NONE), &km()),
            Some(InputEvent::Up)
        );
    }

    #[test]
    fn ctrl_up_does_not_map_to_up() {
        assert_ne!(
            from_key(key(KeyCode::Up, KeyModifiers::CONTROL), &km()),
            Some(InputEvent::Up)
        );
    }

    #[test]
    fn from_key_returns_none_for_unbound_keys() {
        assert_eq!(
            from_key(key(KeyCode::F(1), KeyModifiers::NONE), &km()),
            None
        );
    }

    #[test]
    fn from_key_maps_page_up_and_page_down() {
        assert_eq!(
            from_key(key(KeyCode::PageUp, KeyModifiers::NONE), &km()),
            Some(InputEvent::PageUp)
        );
        assert_eq!(
            from_key(key(KeyCode::PageDown, KeyModifiers::NONE), &km()),
            Some(InputEvent::PageDown)
        );
    }
}
