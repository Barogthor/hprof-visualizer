//! Memory budget tracking for parsed hprof data.

pub mod budget;
pub mod system_memory;
pub use budget::MemoryCounter;
