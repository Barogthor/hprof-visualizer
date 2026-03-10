//! Session warning accumulator for non-fatal navigation errors.
//!
//! [`WarningLog`] collects warnings generated during TUI interaction
//! (e.g. unresolvable object references) and exposes them to the
//! status bar for display. Bounded at [`MAX_SESSION_WARNINGS`] to
//! prevent unbounded growth on pathological inputs.

/// Maximum number of session warnings retained. Older warnings are kept;
/// new ones are silently dropped once the cap is reached.
const MAX_SESSION_WARNINGS: usize = 500;

/// Accumulates non-fatal session warnings for display in the status bar.
///
/// Capped at [`MAX_SESSION_WARNINGS`] entries.
#[derive(Debug, Default)]
pub(crate) struct WarningLog {
    messages: Vec<String>,
}

impl WarningLog {
    /// Adds a warning. No-op once [`MAX_SESSION_WARNINGS`] is reached.
    pub(crate) fn add(&mut self, msg: String) {
        if self.messages.len() < MAX_SESSION_WARNINGS {
            self.messages.push(msg);
        }
    }

    /// Returns the number of warnings collected.
    pub(crate) fn count(&self) -> usize {
        self.messages.len()
    }

    /// Returns the most recent warning text, or `None` if empty.
    pub(crate) fn last(&self) -> Option<&str> {
        self.messages.last().map(|s| s.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_count() {
        let mut log = WarningLog::default();
        assert_eq!(log.count(), 0);
        log.add("first".to_string());
        log.add("second".to_string());
        assert_eq!(log.count(), 2);
    }

    #[test]
    fn last_returns_most_recent() {
        let mut log = WarningLog::default();
        log.add("a".to_string());
        log.add("b".to_string());
        assert_eq!(log.last(), Some("b"));
    }

    #[test]
    fn last_on_empty_returns_none() {
        let log = WarningLog::default();
        assert_eq!(log.last(), None);
    }

    #[test]
    fn add_drops_message_when_cap_reached() {
        let mut log = WarningLog::default();
        for i in 0..MAX_SESSION_WARNINGS {
            log.add(format!("msg-{i}"));
        }
        assert_eq!(log.count(), MAX_SESSION_WARNINGS);
        log.add("overflow".to_string());
        assert_eq!(log.count(), MAX_SESSION_WARNINGS);
        // The last retained message is the one at index MAX-1, not "overflow".
        assert_eq!(
            log.last(),
            Some(format!("msg-{}", MAX_SESSION_WARNINGS - 1).as_str())
        );
    }
}
