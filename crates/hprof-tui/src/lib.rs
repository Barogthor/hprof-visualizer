//! ratatui-based TUI frontend, thin client consuming the NavigationEngine API.
//!
//! Entry point: [`run_tui`] — sets up the terminal and runs the event loop.
//! Modules: [`app`], [`input`], [`theme`], [`views`].

/// Debug log macro: routes to `tracing::debug!` when
/// `dev-profiling` feature is active, otherwise no-op.
#[cfg(feature = "dev-profiling")]
macro_rules! dbg_log {
    ($($arg:tt)*) => { tracing::debug!($($arg)*) };
}

#[cfg(not(feature = "dev-profiling"))]
macro_rules! dbg_log {
    ($($arg:tt)*) => {()};
}

pub mod app;
pub mod input;
pub mod theme;
pub mod views;

pub use app::run_tui;
