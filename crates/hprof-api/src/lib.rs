//! Public API traits for hprof-visualizer.
//!
//! Shared crate for cross-cutting traits used by
//! `hprof-parser`, `hprof-engine`, and `hprof-cli`.

pub mod progress;
pub use progress::{NullProgressObserver, ParseProgressObserver, ProgressNotifier};

#[cfg(feature = "test-utils")]
pub use progress::{ProgressEvent, TestObserver};
