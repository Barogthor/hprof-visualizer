//! ratatui-based TUI frontend, thin client consuming the NavigationEngine API.
//!
//! Entry point: [`run_tui`] — sets up the terminal and runs the event loop.
//! Modules: [`app`], [`input`], [`theme`], [`views`].

pub mod app;
pub mod input;
pub mod theme;
pub mod views;

pub use app::run_tui;
