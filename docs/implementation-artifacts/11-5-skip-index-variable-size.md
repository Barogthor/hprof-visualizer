# Story 11.5: Skip-Index for Variable-Size Sequences

Status: review

## Story

As a user,
I want the system to build a skip-index at first traversal
for variable-size collection sequences (e.g., consecutive
INSTANCE_DUMP node chains in LinkedList/HashMap),
So that subsequent page navigation is fast without
re-scanning from the beginning.

## Acceptance Criteria

1. **Given** a LinkedList with N nodes traversed for the
   first time
   **When** the traversal completes (or reaches the
   requested page)
   **Then** a skip-index is built and cached, storing the
   object ID of every `skip_interval`-th node
   (default: 100 — kept small because each node is a
   dispersed INSTANCE_DUMP requiring a full parse)

2. **Given** a subsequent page access at offset O on the
   same LinkedList
   **When** the system serves the page
   **Then** chain-walking starts from the nearest
   checkpoint at or before O (via `nearest_before(O)`)
   and reads at most `skip_interval` nodes — not from
   the head

3. **Given** a HashMap/LinkedHashMap with N entries
   traversed for the first time
   **When** the traversal completes (or reaches the
   requested page)
   **Then** a skip-index is built, storing (slot_index,
   chain_position) at every `skip_interval`-th entry

4. **Given** a subsequent page access (page K) on the
   same HashMap
   **When** the system serves the page
   **Then** the walk resumes from the skip-index checkpoint,
   not from slot 0

5. **Given** the skip-index for a collection
   **When** the collection is no longer accessed
   **Then** the skip-index remains in memory but is
   negligible (~1.6-2.4 KB per 10K-element collection).
   LRU eviction integration is deferred — a TODO
   comment marks the eviction path for future wiring

6. **Given** existing tests for `extract_linked_list`,
   `extract_hash_map`, `extract_hash_set`, and pagination
   **When** `cargo test` is run
   **Then** all pass unchanged — the skip-index is a
   transparent optimization

7. **Given** a collection where only page 0 is ever
   accessed (the common case)
   **When** skip-index is evaluated
   **Then** no skip-index is built (lazy — only created
   when `offset > 0` is requested for the first time)

## Tasks / Subtasks

- [x] Task 1: Add `SkipIndex` data structure (AC: #1, #3)
  - [x] 1.1 Create file
        `crates/hprof-engine/src/pagination/skip_index.rs`.
        Define `SkipIndex` struct:
        ```rust
        /// Skip-index for O(skip_interval) page access
        /// on variable-size collection chains.
        ///
        /// Stores checkpoints at regular intervals during
        /// chain traversal. On subsequent page requests,
        /// the walk resumes from the nearest checkpoint
        /// instead of scanning from the beginning.
        pub(crate) struct SkipIndex {
            /// Interval between checkpoints (default 100)
            interval: usize,
            /// Checkpoints: entry index → resume state
            checkpoints: Vec<SkipCheckpoint>,
            /// True if the full collection has been traversed
            /// (prevents unnecessary re-traversal)
            complete: bool,
        }
        ```
  - [x] 1.2 Define `SkipCheckpoint` enum to handle both
        LinkedList and HashMap resume states:
        ```rust
        /// Resume state at a skip-index checkpoint.
        pub(crate) enum SkipCheckpoint {
            /// LinkedList: the node object ID at this
            /// checkpoint position
            LinkedListNode { node_id: u64 },
            /// HashMap/LinkedHashMap/ConcurrentHashMap:
            /// the table slot index and current node ID
            /// within that slot's chain.
            /// LinkedHashMap uses this variant too (it
            /// iterates via table slots, not its doubly-
            /// linked order chain).
            HashMapSlot {
                slot_index: usize,
                node_id: u64,
            },
        }
        ```
  - [x] 1.3 Add methods to `SkipIndex`:
        ```rust
        impl SkipIndex {
            pub(crate) fn new(interval: usize) -> Self
            pub(crate) fn record(
                &mut self,
                entry_index: usize,
                checkpoint: SkipCheckpoint,
            )
            pub(crate) fn nearest_before(
                &self,
                entry_index: usize,
            ) -> Option<(usize, &SkipCheckpoint)>
            pub(crate) fn mark_complete(&mut self)
            pub(crate) fn is_complete(&self) -> bool
        }
        ```
        `record()` is called **BEFORE** pushing the
        entry into the items Vec. Convention:
        `record(items.len(), checkpoint)` — so checkpoint
        at index N means "the node about to become
        entry N". On resume, `nearest_before(offset)`
        returns `(N, checkpoint)` meaning "start
        collecting from entry N" (the checkpoint node
        IS entry N, include it). This avoids off-by-one:
        checkpoint index = first entry to collect.
        `record()` appends only if `entry_index` is a
        multiple of `interval` AND equals
        `checkpoints.len() * interval` (the next expected
        checkpoint index in strict sequence). "Next
        expected" is always `checkpoints.len() * interval`
        — so calling `record(30, ...)` when only 0, 10
        are recorded (missing 20) silently no-ops; gaps
        are never filled. Calling `record(X)` twice with
        the same `X` is safe (idempotent — second call
        is not the next expected index).
        `nearest_before(offset)` returns the highest
        checkpoint with index ≤ `offset` via binary
        search. The caller then walks forward from that
        checkpoint until reaching `offset`.
  - [x] 1.4 Add `skip_index` module to
        `crates/hprof-engine/src/pagination/mod.rs`
        imports:
        ```rust
        mod skip_index;
        use skip_index::{SkipCheckpoint, SkipIndex};
        ```

- [x] Task 2: Add skip-index cache to pagination layer
      (AC: #5, #7)
  - [x] 2.1 Add a `skip_indexes` field to hold cached
        skip-indexes per collection ID. The cache lives
        in the `Engine` struct
        (`crates/hprof-engine/src/engine_impl/mod.rs`).
        The field must use `Mutex` for interior mutability
        because `Engine` is shared via `Arc<E>` in the TUI
        (`app/mod.rs:140`) and accessed from spawned threads
        (`app/mod.rs:1887-1889`). `RefCell` is NOT an option
        — it is `!Sync`. Add:
        ```rust
        use std::collections::HashMap as StdHashMap;
        use std::sync::Mutex;
        use crate::pagination::skip_index::SkipIndex;

        // In Engine struct fields:
        skip_indexes: Mutex<StdHashMap<u64, SkipIndex>>,
        ```
        Initialize as `Mutex::new(StdHashMap::new())` in
        the constructor.
  - [x] 2.2 `get_page` signature stays clean — it
        receives `Option<&mut SkipIndex>` (a single
        skip-index, already looked up), NOT the entire
        cache HashMap. The pagination module does not
        know about the global cache.
        ```rust
        pub(crate) fn get_page(
            hfile: &HprofFile,
            collection_id: u64,
            offset: usize,
            limit: usize,
            skip_index: Option<&mut SkipIndex>,
        ) -> Option<CollectionPage>
        ```
        `get_page` passes the `Option<&mut SkipIndex>`
        through `match_extractor` (line 68) to
        `extract_linked_list` and `extract_hash_map`.
        **`match_extractor` must also accept the
        parameter** and forward it to each extractor.
  - [x] 2.3 The **Engine** is responsible for the
        lookup + lazy creation before calling `get_page`.
        **The `NavigationEngine::get_page` trait signature
        stays `&self`** — no trait change required. The
        TUI stores `engine: Arc<E>` (app/mod.rs:140) and
        spawns threads with `Arc::clone` (app/mod.rs:1887),
        so `&mut self` on the trait is impossible. Interior
        mutability via `Mutex` handles the mutable
        skip-index access.
        In the `Engine` impl
        (`engine_impl/mod.rs`, `get_page` method):
        ```rust
        fn get_page(&self, id: u64, off: usize,
            lim: usize) -> Option<CollectionPage> {
            let mut guard = self.skip_indexes
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let si = if off > 0 {
                Some(guard
                    .entry(id)
                    .or_insert_with(|| {
                        SkipIndex::new(100)
                    }))
            } else {
                guard.get_mut(&id)
            };
            pagination::get_page(
                &self.hfile, id, off, lim, si,
            )
        }
        ```
        The lock is held for the duration of the
        `get_page` call only. No contention concern:
        collection pagination is sequential per-collection
        and the lock scope is short.
        `unwrap_or_else(|e| e.into_inner())` recovers
        from a poisoned mutex (a panic in another thread
        during `get_page` is not expected, but if it
        happens, the skip-index data is still valid —
        it only grows monotonically).
        This keeps lazy creation (AC #7) in the Engine
        and the pagination module pure.
  - [x] 2.4 LRU eviction integration is **deferred** —
        out of scope for this story. Rationale: skip-index
        memory is ~1.6-2.4 KB per 10K-element collection,
        negligible vs. resolved objects (MBs). The
        skip-index naturally becomes stale if the
        collection is never re-accessed. A future story
        can add `skip_indexes.remove(id)` in the eviction
        path if memory tracking shows it matters.
        Add a `// TODO(11.5): evict skip_indexes on LRU`
        comment in `crates/hprof-engine/src/engine_impl/mod.rs`
        inside the `expand_object` method, immediately after
        the `self.memory_counter.subtract(freed.min(current))`
        line in the eviction while-loop (there is no
        standalone `evict` method — eviction logic is inline
        in `expand_object`). This co-locates the TODO with
        the existing eviction logic.

- [x] Task 3: Integrate skip-index into
      `extract_linked_list` (AC: #1, #2)
  - [x] 3.1 Modify `extract_linked_list`
        (`pagination/mod.rs` lines 335-420) to accept an
        optional `&mut SkipIndex` parameter.
  - [x] 3.2 At the start of traversal, if skip-index
        exists and offset > 0:
        - Call `skip_index.nearest_before(offset)` to
          find the highest checkpoint with index ≤ offset
        - If checkpoint found (e.g., index=200,
          node_id=0xABC): set `node_id = 0xABC` and
          `items_skipped = 200`, then walk forward
          `offset - 200` nodes to reach the target
        - If no checkpoint found: start from `first_id`
          as today (full walk from head)
        When a checkpoint IS found, match exclusively on
        `SkipCheckpoint::LinkedListNode { node_id }`.
        Any other variant means a logic error (wrong
        skip-index passed to this extractor) — use
        `unreachable!("unexpected checkpoint variant in
        extract_linked_list")` to surface it immediately.
        **Critical (Issue #14):** The existing code
        collects items into a Vec then calls
        `.skip(offset)`. When resuming from a
        checkpoint, `items[0]` is entry
        `checkpoint_index`, NOT entry 0. The pagination
        offset must be adjusted:
        `adjusted_offset = offset - checkpoint_index`.
        Similarly, `target_count` becomes
        `adjusted_offset + limit` (not `offset + limit`)
        — this also affects the early-exit guard
        (`items.len() < target_count`).
        The final `.skip(clamped_offset)` call must use
        `adjusted_offset`, not the raw `offset`.
        Without this adjustment, `.skip(offset)` would
        skip past the end of the Vec → empty page.
  - [x] 3.3 During traversal, call
        `skip_index.record(current_index, checkpoint)`
        at each node visit. The `record()` method
        internally checks if this is a checkpoint
        boundary (multiple of interval) — no if-check
        needed at the call site.
        **Critical:** `current_index` must match the
        pagination's entry count exactly — i.e., every
        item pushed into the `items` Vec increments the
        counter (including `FieldValue::Null` for nodes
        with missing item fields). Only cycle-detected
        skips (where no item is pushed) and `node_id==0`
        stops are excluded. This ensures checkpoint
        positions align with paginator indices.
  - [x] 3.4 On resume from checkpoint, `visited` is
        **empty** — nodes upstream of the checkpoint
        were never seen during this walk. This means
        ALL cycle detection is lost on resume, not just
        upstream cycles. Add a **max iterations guard**:
        `max_iter = total_count - checkpoint_index`
        (where `checkpoint_index` is the index returned
        by `nearest_before`, or 0 if no checkpoint was
        found). `total_count` comes from the LinkedList
        `size` field. If the loop exceeds `max_iter`
        without reaching `node_id == 0`, break and
        return a partial page. Using
        `total_count - checkpoint_index` rather than the
        full `total_count` prevents a cycle from running
        O(total_count) iterations on every resumed page
        request near the end of the list.
  - [x] 3.5 After traversal, if all nodes were visited
        (reached `node_id == 0`), call
        `skip_index.mark_complete()` to prevent
        unnecessary re-traversal on future page requests.
  - [x] 3.6 `total_count` always comes from the Java
        `size` field of the LinkedList instance (existing
        behavior, lines 362-363). The skip-index does
        NOT track total count — `size` is the
        authoritative source. If `size` and actual node
        count differ, the dump is corrupted — not our
        concern.

- [x] Task 4: Integrate skip-index into
      `extract_hash_map` (AC: #3, #4)
  - [x] 4.1 Modify `extract_hash_map`
        (`pagination/mod.rs` lines 195-287) to accept
        an optional `&mut SkipIndex` parameter.
  - [x] 4.2 At the start of traversal, if skip-index
        exists and offset > 0:
        - Call `skip_index.nearest_before(offset)` to
          find the highest checkpoint with index ≤ offset
        - If a checkpoint is found, match exclusively on
          `SkipCheckpoint::HashMapSlot { slot_index,
          node_id }`. Any other variant is a logic error
          — use `unreachable!("unexpected checkpoint
          variant in extract_hash_map")`.
        - If `HashMapSlot { slot_index, node_id }` found:
          (1) skip table iteration to `slot_index`
          (bounds check: if `slot_index >= table.len()`
          → fallback to slot 0),
          (2) within that slot's chain, walk `next`
          until `current_node == node_id` is found,
          (3) resume collecting **starting from** that
          node (the checkpoint index = first entry to
          collect, per `record()` convention in Task
          1.3). Include the checkpoint node as entry.
          If `node_id` is not found in the slot chain
          → fallback to full walk from slot 0.
        **Critical (Issue #14):** Same offset adjustment
        as LinkedList (Task 3.2): `adjusted_offset =
        offset - checkpoint_index`. The existing code
        collects into `all_entries` then delegates to
        `paginate_kv_entries(all_entries, total, offset,
        limit)` which slices at `offset..offset+limit`.
        When resuming from a checkpoint, `all_entries[0]`
        is entry `checkpoint_index`, NOT entry 0.
        **Concrete fix:** pass `adjusted_offset` to
        `paginate_kv_entries` instead of `offset`:
        ```rust
        paginate_kv_entries(
            &all_entries, total, adjusted_offset, limit,
        )
        ```
        Also set `target_count = adjusted_offset + limit`
        (not `offset + limit`) for the early-exit guard,
        otherwise the loop walks more entries than needed
        from the checkpoint start.
  - [x] 4.3 During traversal, record checkpoints:
        ```rust
        skip_index.record(
            entry_count,
            SkipCheckpoint::HashMapSlot {
                slot_index: current_slot,
                node_id: current_node_id,
            },
        );
        ```
        **Critical:** `entry_count` must count only
        **valid resolved entries** (non-null key/value
        pairs), NOT empty table slots or null chain
        nodes. Empty slots are skipped (existing
        `if slot_id == 0 { continue; }` logic), so they
        must not increment the counter.
  - [x] 4.4 `extract_hash_set` delegates to
        `extract_hash_map` — no separate changes needed.
        Pass the skip-index through.
        **Note (HashSet indirection):**
        `extract_hash_set` resolves the backing `map`
        field and calls `extract_hash_map` on the
        backing HashMap instance (not the HashSet
        itself). The skip-index is keyed by the
        `collection_id` passed to `get_page` (= HashSet
        ID). This is correct — the Engine creates the
        skip-index keyed by HashSet ID, and the
        checkpoints describe the backing HashMap's
        internal structure. As long as the same backing
        map is always used (guaranteed — heap dump is
        immutable), the checkpoints remain valid.

- [x] Task 5: Tests (AC: #1, #2, #3, #4, #6, #7)
  - [x] 5.1 Unit tests for `SkipIndex` struct:
        - `record` + `nearest_before` with interval=3:
          record at 0, 3, 6 → nearest_before(5) returns
          (3, checkpoint_3)
        - `nearest_before(1)` returns (0, checkpoint_0)
          (offset between checkpoints → nearest before)
        - `nearest_before` on empty index returns None
        - `mark_complete()` + `is_complete()`
        - Duplicate `record` at same index is idempotent
        - Gap skipping: record at 0, then call
          `record(30, ...)` — no-ops (20 was never
          recorded, 30 is not the next expected index).
          `nearest_before(25)` returns (0, checkpoint_0)
        Note: `nearest_before(0)` is NOT tested here —
        the integration code guards `offset > 0` before
        calling `nearest_before`, so offset=0 is a dead
        path.
  - [x] 5.2 LinkedList skip-index integration test:
        Build a LinkedList with 30 nodes using
        `HprofTestBuilder`. Use skip_interval=10.
        - First call: `get_page(id, 0, 10)` → returns
          page 0 (no skip-index created — lazy)
        - Second call: `get_page(id, 20, 10)` → skip-
          index created, walk from head records
          checkpoints at 0, 10, 20. Returns correct
          entries for offset=20
        - **Assert** skip-index contains checkpoints at
          indices 0, 10, 20 (3 checkpoints for 30 nodes
          with interval=10)
        - Verify entries match sequential full traversal
  - [x] 5.3 HashMap skip-index integration test:
        Build a HashMap with 30 entries using
        `HprofTestBuilder`. Use skip_interval=10.
        - First call: `get_page(id, 0, 10)` → page 0
        - Second call: `get_page(id, 20, 10)` → resumes
          from checkpoint, correct entries
  - [x] 5.4 Lazy creation test (AC #7):
        Add a `#[cfg(test)] fn skip_index_count(&self)
        -> usize` accessor on `Engine` that locks the
        mutex and returns `skip_indexes.lock().len()`.
        - Call `get_page(id, 0, 10)` on a LinkedList
        - Assert `skip_index_count() == 0` — no
          skip-index allocated for page 0 access
        - Call `get_page(id, 10, 10)` → skip-index now
          created, assert `skip_index_count() == 1`
  - [x] 5.5 Existing test regression: update ~30 existing
        `get_page(...)` call sites in `pagination/tests.rs`
        to pass `None` as the new `skip_index` parameter.
        Verify all existing pagination tests still pass
        (`cargo test`)
  - [x] 5.6 Edge case: empty LinkedList (0 nodes) with
        skip-index → no crash, returns empty page
  - [x] 5.7 Edge case: offset beyond known_count but
        skip-index not complete → falls back to sequential
        walk from last known checkpoint
  - [x] 5.8 LinkedList with cycle: build a chain where
        node C.next → node A (cycle). Two sub-tests:
        (a) Full traversal from head: `visited` HashSet
        breaks the cycle — verify finite completion.
        (b) Resumed walk INTO cycle: first access at
        `offset=0, limit=10` on a 15-node chain that
        contains a cycle at node 12 (node 12.next →
        node 5). The first traversal stops at the cycle
        (visited detects node 5 revisit after node 12),
        recording checkpoints at 0, 10. Second access
        at `offset=10, limit=10` resumes from checkpoint
        10. The resumed walk has empty `visited`, so it
        walks node 10 → 11 → 12 → 5 → 6 → … cycling
        indefinitely without `max_iter`. Verify the
        `max_iter` guard (Task 3.4,
        `max_iter = total_count - checkpoint_index =
        15 - 10 = 5`) breaks the loop after 5
        iterations and returns a partial page.
  - [x] 5.9 HashMap with 50% empty slots: build a
        HashMap where half the table[] slots are null.
        Page through it. Verify checkpoint entry indices
        match the actual valid entry count (not slot
        count). Resuming from checkpoint must land on the
        correct entry
  - [x] 5.10 Partial skip-index extension test (most
        common real-world case):
        Build a LinkedList with 50 nodes, skip_interval=10.
        - First call: `get_page(id, 0, 10)` → no skip-
          index created (lazy, AC #7)
        - Second call: `get_page(id, 20, 10)` → walk
          from head, record checkpoints at 0, 10, 20.
          `is_complete()` is false (nodes 30-49 not seen)
        - Assert skip-index has exactly 3 checkpoints
          (at 0, 10, 20) and is NOT complete
        - Third call: `get_page(id, 40, 10)` → resumes
          from checkpoint 20, walks forward 20 nodes,
          records checkpoints at 30, 40. Returns entries
          40-49
        - Assert skip-index now has 5 checkpoints
          (0, 10, 20, 30, 40), still NOT complete
        - Fourth call: `get_page(id, 40, 10)` again →
          resumes from checkpoint 40, walks 10 nodes,
          reaches node_id==0, `mark_complete()` called
        - Assert `is_complete()` is now true
        - Verify entries in all pages match sequential
          full traversal

- [x] Task 6: Regression & clippy (AC: #6)
  - [x] 6.1 Run `cargo test` — all existing tests pass
  - [x] 6.2 Run `cargo clippy --all-targets -- -D warnings`
  - [x] 6.3 Run `cargo fmt -- --check`
  - [x] 6.4 Manual test on
        `assets/heapdump-visualvm.hprof` (41 MB): expand
        a LinkedList or HashMap, page through it, verify
        correct rendering on page 0 and subsequent pages
  - [x] 6.5 Skip-index activation smoke test: in the
        LinkedList integration test (5.2), after the
        second `get_page` call, assert that
        `skip_index.nearest_before(20)` returns
        `Some((20, _))` — i.e., the skip-index was
        actually populated and `nearest_before` returns
        a non-None result for the offset that was just
        served. This guards against a silent regression
        where the skip-index is allocated but never
        populated (checkpoints never recorded), which
        correctness tests alone would not catch.

## Dev Notes

### Core Change Summary

Add a **skip-index** mechanism for variable-size collection
chains (LinkedList node chains, HashMap/LinkedHashMap slot
+ node chains). The skip-index stores **object IDs at
regular intervals** during traversal, enabling subsequent
page requests to resume from the nearest checkpoint instead
of re-walking from the head.

**Current path (slow on page K):**
```
extract_linked_list(offset=K*100, limit=100)
  → walk from first_id: node.next → node.next → …
  → skip K*100 nodes
  → collect 100 entries
  → return CollectionPage
```

**After 11.5 (fast on subsequent page K):**
```
extract_linked_list(offset=K*100, limit=100, skip_index)
  → skip_index.nearest_before(K*100)
    → checkpoint at entry (K-1)*100, node_id=0xABC
  → walk from 0xABC: only ~100 nodes
  → collect 100 entries
  → record new checkpoints during walk
  → return CollectionPage
```

**Performance impact on 10K-element LinkedList
(page 90 = entries 9000-9099, page_size=100):**

| Phase | Before | After 11.5 |
|-------|--------|------------|
| Chain walk to offset | ~9000 node resolves | ~100 node resolves |
| Page data collection | 100 resolves | 100 resolves (same) |
| Skip-index memory | 0 | ~100 checkpoints × 16-24 bytes = 1.6-2.4 KB |

### Where the Skip-Index Actually Helps

The skip-index does **not** reduce the cost of rendering
a page (dominated by `id_to_field_value` at ~99% of
wall-clock). It eliminates the **walk cost to reach the
page**.

**Walk cost depends on `instance_offsets` availability:**

| Scenario | Walk cost/node | 9000 nodes (no skip) | 100 nodes (with skip) |
|----------|---------------|---------------------|----------------------|
| Node in `instance_offsets` | ~350ns | ~3ms | ~0.035ms |
| Fallback (segment scan) | 10-200ms | **90s-1800s** | **1-20s** |

`instance_offsets` only pre-caches thread-related
objects. Collection nodes (LinkedList.Node, HashMap.Node)
are typically **not** in the cache on large dumps → the
fallback path dominates. This is where the skip-index
delivers a **50-90× reduction** in walk cost.

**Page resolution cost** (`id_to_field_value` per entry)
is unchanged at 50-500ms per page. Stories 11.2
(batch-scan) and 11.3 (parallel resolution) address
that separately.

**Primary gain is reduced node resolution count.** Each
node resolve involves `read_instance` which is either O(1)
via `instance_offsets` cache or O(segment_size) via
BinaryFuse8 fallback. Skipping 8000 unnecessary resolves
on page 9 is a major win.

### Design Decisions

**1. Skip-index granularity: object ID, not byte offset**

Unlike Story 11.4 (ObjectArrayMeta) which uses byte offsets
for O(1) arithmetic access, variable-size INSTANCE_DUMPs
cannot be accessed by byte offset arithmetic. The skip-index
stores the **object ID** of the node at each checkpoint. The
engine then resolves this ID via the normal path
(`read_instance` → `instance_offsets` or `find_instance`).

This is simpler than storing raw byte offsets because:
- Node IDs are what the chain exposes (`next` field)
- Decouples skip-index from parser internals
  (byte offsets are an implementation detail of
  `instance_offsets`)
- No need to track mmap byte positions during traversal
- If `instance_offsets` doesn't contain the node,
  `find_instance` resolves it via segment filter — the
  skip-index doesn't need to know which path is used

**2. Lazy skip-index creation (AC #7)**

Most collections are only browsed on page 0. Building a
skip-index eagerly would waste memory. The skip-index is
only created when `offset > 0` is requested for the first
time. During page 0 traversal, no skip-index overhead is
incurred.

**Important subtlety:** The first access to page K > 0
always performs a full walk from head (no skip-index
exists yet). During that walk, checkpoints are recorded.
The skip-index helps starting from the **second non-
sequential access**. This is by design — "at first
traversal" in the story title means the index is built
during the first walk, not that the first walk is fast.

**3. Skip-index lives in Engine, not HprofFile**

The skip-index is a navigation-layer optimization, not a
parser concern. It caches traversal state (object IDs at
checkpoints) which depends on the engine's resolution
infrastructure. Placing it in `Engine` alongside
`object_cache` follows the existing pattern: Engine owns
all mutable session state. The Engine looks up (or lazily
creates) the correct `SkipIndex` for a given collection
ID before calling `pagination::get_page`, which receives
only `Option<&mut SkipIndex>` — a single pre-resolved
index, not the full cache HashMap. This keeps pagination
pure and free of cache-management concerns.

The `skip_indexes` field uses `Mutex<HashMap<u64,
SkipIndex>>` for interior mutability. This is required
because `Engine` is shared via `Arc<E>` in the TUI and
accessed from spawned threads. `RefCell` is `!Sync` and
cannot be used with `Arc`. The lock scope is limited to
each `get_page` call — no contention risk.

**4. Single SkipIndex struct with SkipCheckpoint enum**

One `SkipIndex` container (Vec + interval + count) shared
across collection types. Only the checkpoint payload
differs (enum variant). Each extractor (`extract_linked_
list`, `extract_hash_map`) knows which variant it uses —
the `match` on the enum happens at the point of use, not
inside `SkipIndex`. Adding a new collection type later =
adding an enum variant, no container refactoring.

**5. HashMap checkpoint: (slot_index, node_id)**

HashMap traversal iterates slots in order, then follows
node chains within each slot. The checkpoint stores both
the slot index (to skip past earlier slots) and the current
node's object ID (to resume within the chain). This is
sufficient because HashMap iteration order is deterministic
for a given table[] layout.

**6. Default interval: 100 (small due to dispersed nodes)**

Unlike Object[] (contiguous, O(1) per element), each node
in a LinkedList/HashMap chain is a dispersed INSTANCE_DUMP
requiring a full parse + field extraction. An interval of
100 ensures at most 100 instance parses from any
checkpoint — an acceptable cost. Memory overhead is still
negligible (~1.6 KB LinkedList, ~2.4 KB HashMap per
10K-element collection — `HashMapSlot` variant is 24
bytes with alignment vs 16 for `LinkedListNode`).

### Callers Affected — Change Matrix

> **Note:** Line numbers below were accurate at story authoring time. Stories 11.1–11.4
> modified these files. Verify actual line numbers in the current codebase before editing;
> use function/struct names as the authoritative anchors.

| Caller | File (search by name) | Change |
|--------|-----------------------|--------|
| `extract_linked_list` | `pagination/mod.rs` | Add `Option<&mut SkipIndex>` param, checkpoint recording, resume from checkpoint, offset adjustment |
| `extract_hash_map` | `pagination/mod.rs` | Add `Option<&mut SkipIndex>` param, checkpoint recording, resume from checkpoint, offset adjustment |
| `extract_hash_set` | `pagination/mod.rs` | Pass skip-index through to `extract_hash_map` |
| `match_extractor` | `pagination/mod.rs` | Accept and forward `Option<&mut SkipIndex>` to each extractor |
| `get_page` | `pagination/mod.rs` | Accept `Option<&mut SkipIndex>`, pass to `match_extractor`. No cache awareness |
| `Engine` struct | `engine_impl/mod.rs` | Add `skip_indexes: Mutex<HashMap<u64, SkipIndex>>` field |
| `NavigationEngine` trait | `engine.rs` | **No change** — trait stays `&self`, interior mutability via `Mutex` in `Engine` |
| `NavigationEngine::get_page` impl | `engine_impl/mod.rs` | Lock mutex, lookup/create skip-index (lazy), pass `Option<&mut SkipIndex>` to `pagination::get_page` |

### What NOT To Do

- Do NOT build skip-indexes eagerly on page 0 — lazy
  creation only (AC #7). The common case is page 0 only.
- Do NOT cache resolved `FieldValue`s in the skip-index —
  only store node IDs. Value resolution is done by the
  existing pipeline (`id_to_field_value`).
- Do NOT add thread pooling or async for skip-index
  building — it happens during the synchronous traversal
  that already occurs.
- Do NOT modify `extract_array_list` — ArrayList backs
  onto Object[] which is covered by Story 11.4 (O(1)
  offset arithmetic). No skip-index needed.
- Do NOT modify `try_object_array` or `try_prim_array` —
  these are already O(1) or will be via 11.4.
- Do NOT persist skip-indexes to disk — they are cheap to
  rebuild from cached `instance_offsets`.
- Do NOT over-engineer eviction integration — if wiring
  skip-index removal into LRU eviction is complex, defer
  to a follow-up. The skip-index memory is ~1.6-2.4 KB per
  10K-element collection (negligible vs. resolved objects).

### Existing Code to Reuse

- `extract_linked_list` (pagination/mod.rs:335-420) —
  modify in-place to accept skip-index
- `extract_hash_map` (pagination/mod.rs:195-287) —
  modify in-place to accept skip-index
- `read_instance` (engine_impl/mod.rs:448-455) — used to
  resolve node IDs to instances (unchanged)
- `instance_offsets` (precise.rs:74) — O(1) lookups
  **only for thread-related objects** (pre-cached during
  first-pass). Collection node IDs are typically NOT in
  this cache — they fall back to `find_instance` via
  BinaryFuse8 segment filter. Do NOT assume O(1) for
  collection nodes on large dumps.
- `HprofTestBuilder` (test_utils.rs) — use for building
  test fixtures. **Note:** building LinkedList/HashMap
  structures requires CLASS_DUMP records with correct
  field layouts, string records for field names (`first`,
  `next`, `item`, `table`, `size`, `key`, `value`), and
  multiple INSTANCE_DUMP records for nodes. Check
  existing pagination tests in `pagination/tests.rs` for
  reusable builder patterns before creating new ones.
  Test setup is non-trivial — budget accordingly.

### Pre-mortem Risks & Mitigations

**Risk 1 — Entry count mismatch (cycles/nulls):**
LinkedList chains can have null nodes or cycles
(detected by `visited` HashSet). The checkpoint index
must count only valid resolved entries. If it counts
raw node visits, checkpoint positions drift from
paginator positions → duplicates or missing entries.
Mitigated by Tasks 3.3, 5.8.

**Risk 2 — HashMap empty slot counting:**
HashMap tables have empty slots (`slot_id == 0`).
`entry_count` must only increment on valid entries,
not on empty slots. If mismatched, resuming from a
checkpoint lands on the wrong entry.
Mitigated by Tasks 4.3, 5.9.

**Risk 3 — Fallback segment scan on resume:**
Skip-index stores node IDs. If `instance_offsets` does
not contain the node (it only pre-caches thread-related
objects), resume triggers `find_instance` → BinaryFuse8
segment scan. This is the expected fallback — not a bug.
Performance depends on Story 11.2 (batch-scan) for
mitigation on very large dumps.

**Risk 4 — Skip-index memory accumulation:**
Each explored collection (beyond page 0) creates a
`SkipIndex` that is never evicted (LRU deferred).
Worst case: 5000 collections × ~2.4 KB = ~12 MB.
Acceptable for now. If profiling shows this matters,
add a cap (e.g., max 1000 skip-indexes, evict oldest).

**Risk 5 — Cycle upstream of checkpoint (LinkedList):**
If a cycle exists between nodes upstream of the
checkpoint, the resumed walk won't detect it (those
nodes are not in `visited`). The `max_iter` guard
(Task 3.4) caps iterations at `total_count` to prevent
infinite loops. Mitigated by Task 3.4, Test 5.8b.

**Risk 6 — HashMap slot resume skips to wrong node:**
When resuming at `HashMapSlot { slot_index, node_id }`,
the code must walk the slot's chain until finding
`node_id`, then resume AFTER it. If it starts
collecting from the slot head instead, entries from
the beginning of the chain are duplicated. Mitigated
by explicit 3-step resume in Task 4.2.

**Risk 7 — HashMap slot_index out of bounds:**
If a bug causes `slot_index >= table.len()`, the
resume would panic. Bounds check + fallback to slot 0
in Task 4.2 prevents this.

### Future Consideration: Batch Pre-Resolution for Chain Nodes

Story 11.2 added `batch_find_instances` for Object[] pagination
(`paginate_object_array`), but `extract_linked_list` and
`extract_hash_map` still resolve nodes one-by-one via
`Engine::read_instance_public` → `find_instance` fallback.
The skip-index reduces the number of nodes to resolve (e.g.,
from 9000 to ~100), but each remaining node still incurs the
full segment-scan cost. A follow-up story could batch-resolve
the ~100 forward-walk node IDs after resuming from a
checkpoint, compounding the skip-index gain with 11.2's
batch-scan.

### Future Consideration: Custom Collection Recognition

Currently `match_extractor` (pagination/mod.rs:68) only
recognizes JDK standard collection class names (ArrayList,
HashMap, LinkedList, etc.). Custom collections implementing
the same patterns (e.g., Guava `ImmutableList`,
`com.mycompany.MyLinkedList`) are not detected and fall
back to raw field expansion without pagination.

A future improvement could detect collections by
**structure/inheritance** (presence of `first`/`next`/`item`
fields, or subclass of `java.util.AbstractList`) rather
than exact class name matching. Out of scope for this
story — noted in sprint-status backlog ideas.

### No New Dependencies

All required infrastructure exists:
- `std::collections::HashMap` for skip-index cache
- Existing `HprofTestBuilder` for test fixtures
- Existing pagination infrastructure for page assembly

### Project Structure Notes

- New file: `crates/hprof-engine/src/pagination/skip_index.rs`
  (new module for SkipIndex struct)
- Changes in `crates/hprof-engine/src/pagination/mod.rs`
  (add module, modify `get_page`, `extract_linked_list`,
  `extract_hash_map`, `extract_hash_set`)
- Changes in `crates/hprof-engine/src/engine_impl/mod.rs`
  (add `skip_indexes: Mutex<HashMap>` field, lock + pass
  to `get_page`)
- **No changes** to `crates/hprof-engine/src/engine.rs`
  (trait stays `&self`)
- No changes to hprof-parser crate
- No new dependencies

### Previous Story Intelligence

From Story 11.4 (skip-index for Object[]):
- `ObjectArrayMeta` pattern: separate metadata from data
  reading — same principle applies here (separate
  checkpoint from resolution)
- `paginate_object_array`: template for paginating from
  a starting position — reusable concept
- No new dependencies were needed — same applies here
- Test approach: `HprofTestBuilder` with specific record
  types, verify page content matches expectations

From Story 11.3 (parallel eager resolution):
- rayon available in workspace (not needed for this story)
- `OffsetCache` wraps `instance_offsets` — complementary
  to skip-index (skip-index uses object IDs, resolution
  uses `instance_offsets` for O(1) lookup)

### References

> Line numbers are indicative only — verify against current code (stories 11.1–11.4 may
> have shifted offsets). Use function names as the authoritative search targets.

- [Source: docs/planning-artifacts/epics.md#Story 11.5]
- [Source: crates/hprof-engine/src/pagination/mod.rs]
  — `extract_linked_list`
- [Source: crates/hprof-engine/src/pagination/mod.rs]
  — `extract_hash_map`
- [Source: crates/hprof-engine/src/pagination/mod.rs]
  — `extract_hash_set`
- [Source: crates/hprof-engine/src/pagination/mod.rs]
  — `get_page`
- [Source: crates/hprof-engine/src/engine_impl/mod.rs]
  — `read_instance`
- [Source: crates/hprof-engine/src/engine_impl/mod.rs]
  — `NavigationEngine::get_page` impl
- [Source: crates/hprof-parser/src/indexer/precise.rs]
  — `instance_offsets`
- [Source: crates/hprof-engine/src/pagination/mod.rs]
  — `paginate_object_array` (Story 11.4 — template for
  paginating from a starting offset; search by name)

## Dev Agent Record

### Agent Model Used
Claude Opus 4.6

### Debug Log References
N/A

### Completion Notes List
- Task 1: Created `SkipIndex` struct with `SkipCheckpoint` enum (LinkedListNode, HashMapSlot variants). Methods: `new`, `record`, `nearest_before`, `mark_complete`, `is_complete`. 7 unit tests.
- Task 2: Added `skip_indexes: Mutex<StdHashMap<u64, SkipIndex>>` to `Engine`. `get_page` impl locks mutex, lazily creates skip-index only when `offset > 0`. TODO(11.5) eviction comment placed. `skip_index_count` test accessor added.
- Task 3: `extract_linked_list` accepts `Option<&mut SkipIndex>`. On resume: `nearest_before(offset)` → checkpoint, adjusted_offset = offset - checkpoint_index. Records checkpoints during walk. `max_iter` guard = `total_count - checkpoint_index`. `mark_complete()` when node_id reaches 0.
- Task 4: `extract_hash_map` accepts `Option<&mut SkipIndex>`. Resume walks slot chain to find checkpoint node_id, then collects from there. Fallback `extract_hash_map_full` if checkpoint node not found. `extract_hash_set` forwards skip-index through.
- Task 5: 16 tests total (7 unit + 9 integration). LinkedList checkpoint recording, HashMap checkpoint recording, empty list, beyond-known checkpoint, 50% empty slots, partial extension, cycle detection with max_iter, activation smoke test. 27 existing get_page calls updated to pass None.
- Task 6: 975 tests pass, clippy clean (`-D warnings`), fmt clean.

### File List
- `crates/hprof-engine/src/pagination/skip_index.rs` (new)
- `crates/hprof-engine/src/pagination/mod.rs` (modified)
- `crates/hprof-engine/src/pagination/tests.rs` (modified)
- `crates/hprof-engine/src/engine_impl/mod.rs` (modified)

### Change Log
- 2026-03-17: Implemented Story 11.5 — skip-index for variable-size collection pagination (LinkedList, HashMap, HashSet). All ACs satisfied, 16 new tests added.
