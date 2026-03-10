//! Public API traits for hprof-visualizer.
//!
//! Shared crate for cross-cutting traits used by
//! `hprof-parser`, `hprof-engine`, and `hprof-cli`.

pub mod memory_size;
pub mod progress;
pub use memory_size::{MemorySize, fxhashmap_memory_size};
pub use progress::{NullProgressObserver, ParseProgressObserver, ProgressNotifier};

#[cfg(feature = "test-utils")]
pub use progress::{ProgressEvent, TestObserver};
