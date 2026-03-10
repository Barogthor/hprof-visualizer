//! LRU object cache for expanded subtrees.
//!
//! Wraps `lru::LruCache` in a `Mutex` for interior mutability
//! (all `NavigationEngine` methods take `&self`). Memory budget
//! is managed externally by `Engine` via `MemoryCounter`.

use crate::engine::FieldInfo;
use hprof_api::MemorySize;
use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::Mutex;

/// Eviction fires when `usage_ratio()` reaches this threshold.
pub(crate) const EVICTION_TRIGGER: f64 = 0.80;

/// Eviction stops when `usage_ratio()` drops below this target.
pub(crate) const EVICTION_TARGET: f64 = 0.60;

/// Thread-safe LRU cache mapping `object_id` to decoded fields.
///
/// Each entry stores `(Vec<FieldInfo>, precomputed_memory_size)`.
/// `get()` promotes the entry to MRU. `evict_lru()` pops the
/// least recently used entry and returns its precomputed size.
pub struct ObjectCache(
    Mutex<LruCache<u64, (Vec<FieldInfo>, usize)>>,
);

impl ObjectCache {
    /// Creates an unbounded LRU cache (no count-based cap).
    pub fn new() -> Self {
        Self(Mutex::new(LruCache::unbounded()))
    }

    /// Returns cached fields for `id`, promoting to MRU.
    ///
    /// Returns `None` on cache miss.
    pub fn get(&self, id: u64) -> Option<Vec<FieldInfo>> {
        self.0
            .lock()
            .unwrap()
            .get(&id)
            .map(|(fields, _)| fields.clone())
    }

    /// Inserts fields into the cache, returning the
    /// precomputed memory size in bytes.
    pub fn insert(
        &self,
        id: u64,
        fields: Vec<FieldInfo>,
    ) -> usize {
        let mem = compute_fields_size(&fields);
        self.0.lock().unwrap().put(id, (fields, mem));
        mem
    }

    /// Evicts the least recently used entry, returning
    /// its precomputed size in bytes, or `None` if empty.
    pub fn evict_lru(&self) -> Option<usize> {
        self.0
            .lock()
            .unwrap()
            .pop_lru()
            .map(|(_, (_, size))| size)
    }

    /// Returns `true` if the cache contains no entries.
    pub fn is_empty(&self) -> bool {
        self.0.lock().unwrap().is_empty()
    }

    /// Returns the number of cached entries.
    pub fn len(&self) -> usize {
        self.0.lock().unwrap().len()
    }
}

/// Approximates the heap memory of a `Vec<FieldInfo>`.
///
/// Counts `len` slots (not `capacity`) — intentional
/// approximation consistent with `CollectionPage::memory_size`.
fn compute_fields_size(fields: &[FieldInfo]) -> usize {
    std::mem::size_of::<Vec<FieldInfo>>()
        + fields
            .iter()
            .map(|f| f.memory_size())
            .sum::<usize>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{FieldInfo, FieldValue};

    fn make_field(name: &str) -> FieldInfo {
        FieldInfo {
            name: name.to_string(),
            value: FieldValue::Int(42),
        }
    }

    #[test]
    fn cache_miss_returns_none() {
        let cache = ObjectCache::new();
        assert!(cache.get(42).is_none());
    }

    #[test]
    fn cache_hit_returns_same_fields() {
        let cache = ObjectCache::new();
        let fields = vec![make_field("x"), make_field("y")];
        cache.insert(1, fields.clone());
        let got = cache.get(1).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].name, "x");
        assert_eq!(got[1].name, "y");
    }

    #[test]
    fn cache_hit_promotes_to_mru() {
        let cache = ObjectCache::new();
        cache.insert(1, vec![make_field("a")]);
        cache.insert(2, vec![make_field("b")]);
        // Promote A to MRU
        let _ = cache.get(1);
        // Evict LRU → should be B (id=2)
        let first_evict = cache.evict_lru();
        assert!(first_evict.is_some());
        // A should still be in cache
        assert!(cache.get(1).is_some());
        // B should be gone
        assert!(cache.get(2).is_none());
    }

    #[test]
    fn evict_lru_on_empty_returns_none() {
        let cache = ObjectCache::new();
        assert!(cache.evict_lru().is_none());
    }

    #[test]
    fn insert_returns_nonzero_size_for_nonempty_fields() {
        let cache = ObjectCache::new();
        let size = cache.insert(
            1,
            vec![make_field("big_field_name")],
        );
        assert!(size > 0);
    }

    #[test]
    fn evict_lru_returns_precomputed_size() {
        let cache = ObjectCache::new();
        let size = cache.insert(1, vec![make_field("x")]);
        let evicted = cache.evict_lru().unwrap();
        assert_eq!(evicted, size);
    }

    #[test]
    fn is_empty_true_on_new_cache() {
        assert!(ObjectCache::new().is_empty());
    }

    #[test]
    fn len_reflects_inserted_entries() {
        let cache = ObjectCache::new();
        cache.insert(1, vec![make_field("a")]);
        cache.insert(2, vec![make_field("b")]);
        assert_eq!(cache.len(), 2);
        cache.evict_lru();
        assert_eq!(cache.len(), 1);
    }
}
