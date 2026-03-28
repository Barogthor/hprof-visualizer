//! Keyboard layout presets and action-to-key mapping.
//!
//! [`KeymapPreset`] selects a named layout (AZERTY or QWERTY).
//! [`Keymap`] holds one [`KeyCode`] per configurable action and is
//! instantiated from a preset via [`KeymapPreset::build`].
//!
//! Both presets start with identical bindings — the infrastructure
//! supports future divergence once ergonomic gaps are identified.

use std::str::FromStr;

use crossterm::event::KeyCode;

/// Named keyboard layout preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeymapPreset {
    #[default]
    Azerty,
    Qwerty,
}

impl FromStr for KeymapPreset {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "azerty" => Ok(Self::Azerty),
            "qwerty" => Ok(Self::Qwerty),
            _ => Err(format!("unknown keymap '{}': expected azerty or qwerty", s)),
        }
    }
}

impl KeymapPreset {
    /// Build a [`Keymap`] for this preset.
    pub fn build(self) -> Keymap {
        match self {
            Self::Azerty => Keymap::azerty(),
            Self::Qwerty => Keymap::qwerty(),
        }
    }
}

/// Configurable action-to-key mapping for a single layout preset.
///
/// Each field holds the [`KeyCode`] bound to the corresponding action.
/// Layout-independent keys (arrows, Enter, Esc, Ctrl+C, …) are
/// hardcoded in `input.rs` and absent from this struct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keymap {
    pub quit: KeyCode,
    pub toggle_favorite: KeyCode,
    pub focus_favorites: KeyCode,
    pub navigate_to_source: KeyCode,
    pub hide_field: KeyCode,
    pub reveal_hidden: KeyCode,
    pub prev_pin: KeyCode,
    pub next_pin: KeyCode,
    pub batch_expand: KeyCode,
    pub toggle_object_ids: KeyCode,
    pub search_activate: KeyCode,
}

impl Default for Keymap {
    fn default() -> Self {
        KeymapPreset::default().build()
    }
}

impl Keymap {
    fn azerty() -> Self {
        Self {
            quit: KeyCode::Char('q'),
            toggle_favorite: KeyCode::Char('f'),
            focus_favorites: KeyCode::Char('F'),
            navigate_to_source: KeyCode::Char('g'),
            hide_field: KeyCode::Char('h'),
            reveal_hidden: KeyCode::Char('H'),
            prev_pin: KeyCode::Char('b'),
            next_pin: KeyCode::Char('n'),
            batch_expand: KeyCode::Char('c'),
            toggle_object_ids: KeyCode::Char('i'),
            search_activate: KeyCode::Char('s'),
        }
    }

    fn qwerty() -> Self {
        // Both presets start identical — diverge here when ergonomic
        // gaps between layouts are identified.
        Self::azerty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_preset_is_azerty() {
        assert_eq!(KeymapPreset::default(), KeymapPreset::Azerty);
    }

    #[test]
    fn azerty_from_str_roundtrip() {
        assert_eq!("azerty".parse::<KeymapPreset>(), Ok(KeymapPreset::Azerty));
    }

    #[test]
    fn qwerty_from_str_roundtrip() {
        assert_eq!("qwerty".parse::<KeymapPreset>(), Ok(KeymapPreset::Qwerty));
    }

    #[test]
    fn unknown_preset_from_str_error() {
        let err = "dvorak".parse::<KeymapPreset>().unwrap_err();
        assert!(
            err.contains("unknown keymap"),
            "error should mention 'unknown keymap': {err}"
        );
        assert!(err.contains("dvorak"), "error should echo the bad value");
    }

    #[test]
    fn error_message_lists_valid_options() {
        let err = "bogus".parse::<KeymapPreset>().unwrap_err();
        assert!(err.contains("azerty"), "error should list 'azerty'");
        assert!(err.contains("qwerty"), "error should list 'qwerty'");
    }

    #[test]
    fn default_keymap_uses_azerty_preset() {
        assert_eq!(Keymap::default(), KeymapPreset::Azerty.build());
    }

    #[test]
    fn no_two_actions_share_same_keycode_azerty() {
        let km = KeymapPreset::Azerty.build();
        let keys = all_keys(&km);
        let n = keys.len();
        for i in 0..n {
            for j in (i + 1)..n {
                assert_ne!(
                    keys[i], keys[j],
                    "duplicate KeyCode at positions {i} and {j}: {:?}",
                    keys[i]
                );
            }
        }
    }

    #[test]
    fn no_two_actions_share_same_keycode_qwerty() {
        let km = KeymapPreset::Qwerty.build();
        let keys = all_keys(&km);
        let n = keys.len();
        for i in 0..n {
            for j in (i + 1)..n {
                assert_ne!(
                    keys[i], keys[j],
                    "duplicate KeyCode at positions {i} and {j}: {:?}",
                    keys[i]
                );
            }
        }
    }

    /// Collect all key codes in a stable order for uniqueness checks.
    fn all_keys(km: &Keymap) -> Vec<KeyCode> {
        vec![
            km.quit,
            km.toggle_favorite,
            km.focus_favorites,
            km.navigate_to_source,
            km.hide_field,
            km.reveal_hidden,
            km.prev_pin,
            km.next_pin,
            km.batch_expand,
            km.toggle_object_ids,
            km.search_activate,
        ]
    }
}
