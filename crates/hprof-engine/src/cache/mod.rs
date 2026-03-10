//! Memory budget tracking for parsed hprof data.

pub mod budget;
pub mod lru;
pub mod system_memory;
pub use budget::MemoryCounter;
pub use lru::ObjectCache;
