# Story 4.1: Collection Pagination Engine

Status: done

## Story

As a developer,
I want the engine to paginate collections exceeding 1000
entries into batches of 1000, exposable via the
`get_page(collection_id, offset, limit)` method,
So that the TUI never needs to load an entire large
collection into memory at once.

## Acceptance Criteria

### AC1: First page of a large collection

**Given** a collection object with 524,288 entries
**When** `get_page(collection_id, 0, 1000)` is called
**Then** the first 1000 entries are resolved and returned
with their values (FR20)

### AC2: Arbitrary page offset

**Given** a page request with `offset=5000, limit=1000`
**When** the engine processes it
**Then** entries 5000-5999 are returned, resolving object
references as needed via segment filters

### AC3: Small collection — no pagination needed

**Given** a collection with 500 entries (below threshold)
**When** expanded
**Then** all entries are returned in a single batch without
pagination

### AC4: Last page with fewer entries

**Given** the last page of a collection where remaining
entries < 1000
**When** requested
**Then** only the remaining entries are returned with
correct count

### AC5: Performance — NFR3

**Given** a collection with 500K+ entries
**When** `get_page` is called for any page
**Then** the page is returned within 5 seconds wall clock

### AC6: Unsupported collection types return None

**Given** a collection type in `COLLECTION_CLASS_SUFFIXES`
without a dedicated extractor (e.g. TreeMap, Vector)
**When** `get_page` is called
**Then** `None` is returned (user sees entry_count in
expand_object but cannot paginate)

### AC7: All existing tests pass

**Given** all existing tests (401 tests)
**When** I run `cargo test`
**Then** all tests pass — zero regressions

## Tasks / Subtasks

- [x] Task 1: Define `EntryInfo` struct and
  `CollectionPage` return type (AC: 1, 2, 3, 4)
  - [x] 1.1: Enrich `EntryInfo` in `engine.rs` with
    fields: `index: usize`, `key: Option<FieldValue>`,
    `value: FieldValue`
  - [x] 1.2: Add `CollectionPage` struct with
    `entries: Vec<EntryInfo>`, `total_count: u64`,
    `offset: usize`, `has_more: bool`
  - [x] 1.3: Update `get_page` return type from
    `Vec<EntryInfo>` to `Option<CollectionPage>`
  - [x] 1.4: Update `DummyEngine` in `engine.rs` tests
  - [x] 1.5: Update `StubEngine` in
    `crates/hprof-tui/src/app.rs` (line 666) — also
    implements `NavigationEngine`, will break on
    signature change

- [x] Task 2: Add `find_object_array` to `HprofFile`
  and create `pagination.rs` module (AC: 1, 2)
  - [x] 2.1: Add `find_object_array(id) -> Option<(u64,
    Vec<u64>)>` to `hprof_file.rs`, modeled after
    `find_prim_array` (line 147) but for
    `HeapSubTag::ObjectArrayDump` (0x22). Returns
    `(array_class_id, element_ids)`.
  - [x] 2.2: Create `crates/hprof-engine/src/pagination.rs`
  - [x] 2.3: Implement type dispatch function that reads
    the collection object, identifies its concrete Java
    type via `COLLECTION_CLASS_SUFFIXES` matching, and
    delegates to the correct extractor
  - [x] 2.4: Register module in `lib.rs`

- [x] Task 3: Implement extractors for core collection
  types (AC: 1, 2, 3, 4)
  - [x] 3.1: `ObjectArrayDump` extractor — direct element
    access by offset into the array's element list
  - [x] 3.2: `ArrayList` extractor — read `elementData`
    field (Object[]), then delegate to ObjectArrayDump
    extractor with `size` field as bounds
  - [x] 3.3: `HashMap` extractor — read `table` field
    (Node[]), walk non-null entries extracting key/value
    fields, skip null slots
  - [x] 3.4: `HashSet` extractor — delegate to HashMap
    extractor on backing `map` field, return keys only
  - [x] 3.5: `LinkedList` extractor — walk `first` →
    `next` chain, extract `item` field per node
  - [x] 3.6: `ConcurrentHashMap` extractor — read `table`
    field (Node[]), walk non-null entries (same structure
    as HashMap.Node but different class hierarchy)
  - [x] 3.7: `PrimArrayDump` extractor — direct element
    access by offset for primitive arrays (int[], long[],
    etc.)
  - [x] 3.8: Fallback for remaining collection types in
    `COLLECTION_CLASS_SUFFIXES` without dedicated
    extractor (Hashtable, Vector, ArrayDeque,
    LinkedHashMap, LinkedHashSet, TreeMap, TreeSet,
    CopyOnWriteArrayList, PriorityQueue) — delegate to
    `expand_object` fields display or return `None`
    with a logged warning. Users see entry_count but
    cannot paginate these types in MVP.
  - [x] 3.9: Fallback for fully unknown types — return
    `None` from `get_page`

- [x] Task 4: Implement `get_page` in `engine_impl.rs`
  (AC: 1, 2, 3, 4, 5, 6)
  - [x] 4.1: Replace stub `get_page` with real impl that
    calls pagination module
  - [x] 4.2: Use `collection_entry_count()` (existing)
    to get total count
  - [x] 4.3: Pass `offset` and `limit` to extractor,
    validate bounds (clamp offset to total, clamp limit
    to remaining)
  - [x] 4.4: Return `None` if object not found or not a
    collection type

- [x] Task 5: Write unit tests (AC: 1, 2, 3, 4, 6, 7)
  - [x] 5.1: Test ObjectArrayDump pagination —
    first page, middle page, last partial page
  - [x] 5.2: Test small array returns all entries
  - [x] 5.3: Test offset beyond bounds returns empty
  - [x] 5.4: Test HashMap with null slots are skipped
  - [x] 5.5: Test ArrayList uses `size` not array capacity
  - [x] 5.6: Test unsupported collection type (e.g.
    TreeMap) returns None
  - [x] 5.7: Test fully unknown type returns None
  - [x] 5.8: Test `has_more` flag correctness

- [x] Task 6: Run full test suite — verify zero
  regressions (AC: 7)

## Dev Notes

### Architecture Compliance

- **Crate boundary:** All pagination logic lives in
  `hprof-engine`. It calls `hprof-parser` via `HprofFile`
  methods (`find_instance`, `read_instance_at_offset`,
  `find_prim_array`, `find_object_array`). Do NOT add
  pagination to `hprof-parser`.
- **Trait boundary:** TUI only sees `NavigationEngine`
  trait. The `get_page` method signature change must be
  reflected in the trait definition.
- **No `unwrap()` in production code** — use `?` or
  explicit `match`.

### Key Code Locations

- **NavigationEngine trait + view models:**
  `crates/hprof-engine/src/engine.rs`
  - `EntryInfo` (line 149): currently empty placeholder
  - `get_page` (line 182): current stub returns `vec![]`
  - `FieldValue` enum (line 107): reuse for entry values
- **Engine implementation:**
  `crates/hprof-engine/src/engine_impl.rs`
  - `COLLECTION_CLASS_SUFFIXES` (line 27): array of 14
    known Java collection short names — reuse for type
    dispatch in pagination
  - `collection_entry_count()` (line 50): free function
    that detects collection type by suffix match and
    extracts count from `size`/`elementCount`/`count`
    fields — REUSE for type identification and total
    count. Note: it walks superclass chain to skip
    prefix bytes, then reads immediate class fields
    only.
  - `expand_object` (line 574): model for object
    resolution flow — but NOT for extractors. Extractors
    need to search fields by name, not iterate all.
- **Field resolution:**
  `crates/hprof-engine/src/resolver.rs`
  - `decode_fields()` (line 25): parses instance data
    bytes per class hierarchy, returns fields in
    **leaf-first order** (subclass fields before
    superclass). When extractors need a specific field
    (e.g. `table`, `elementData`), search the returned
    `Vec<FieldInfo>` by name — do NOT assume position.
- **HprofFile methods:**
  `crates/hprof-parser/src/hprof_file.rs`
  - `find_instance(id)`: locate object via BinaryFuse8
  - `read_instance_at_offset(offset)`: fast O(1) lookup
  - `find_prim_array(id)` (line 147): retrieve primitive
    arrays
  - `read_prim_array_at_offset(offset)` (line 226):
    fast O(1) variant when offset is known
  - **`find_object_array(id)` does NOT exist yet** —
    must be added (see Task 2.1)
- **Heap sub-tags:**
  `crates/hprof-parser/src/indexer/tags.rs`
  - `ObjectArrayDump` (0x22): array of object references
  - `PrimArrayDump` (0x23): array of primitives
  - `InstanceDump` (0x21): object instances

### Collection Internal Structures (Java)

Understanding how Java collections store data internally
is critical. The extractor must navigate these structures:

- **`ArrayList`**: `Object[] elementData` + `int size`
  (capacity != size — use `size` not array length)
- **`HashMap`**: `Node[] table` (sparse — null slots),
  each Node has `key`, `value`, `next` (linked chain)
- **`HashSet`**: wraps `HashMap` via `map` field, entries
  are keys with dummy `PRESENT` value
- **`LinkedList`**: `Node first`, `Node last`, `int size`,
  each Node has `item`, `next`, `prev`
- **`ConcurrentHashMap`**: `Node[] table` (similar to
  HashMap but different class hierarchy), Nodes have
  `key`, `val`, `next`
- **`TreeMap`**: `Entry root`, each Entry has `key`,
  `value`, `left`, `right`, `parent` — requires in-order
  traversal
- **Object arrays (`Object[]`)**: direct element access
  via ObjectArrayDump, elements are sequential object IDs

### Pagination Strategy

For **indexed** collections (ArrayList, Object arrays):
- Direct offset computation — skip to `offset` position,
  read `limit` elements. O(1) seek.

For **linked/hashed** collections (HashMap, LinkedList):
- Must walk from start to reach `offset` position.
  O(offset) seek per page request. This is acceptable
  for MVP — a cursor-based caching optimization can be
  added later if needed.

For **tree** collections (TreeMap):
- In-order traversal, skip first `offset` nodes.
  O(offset) seek. Same deferral strategy as linked.

### Performance Considerations

- **NFR3 target:** < 5s for 500K+ entries per page.
  Since each page is only 1000 entries max, the
  bottleneck is walking to `offset` for hash/linked
  types. For 500K HashMap, worst case is walking 500K
  Node references — each requires `find_instance` (BF8
  filter + scan). This may exceed 5s.
- **Mitigation:** For HashMap/ConcurrentHashMap, walk
  the `table` array (ObjectArrayDump) directly — null
  slots are ID 0, skip them. This avoids resolving Node
  instances for null entries.
- **Future optimization:** Cache resolved page cursors
  for sequential page-forward navigation (Story 5.x
  or future).

### Anti-Patterns — DO NOT

- Do NOT load all collection entries into memory before
  paginating — the whole point is lazy page-by-page
  loading
- Do NOT add pagination logic to `hprof-parser` — keep
  it in `hprof-engine/pagination.rs`. The ONLY addition
  to `hprof-parser` is `find_object_array` (a generic
  data accessor, not pagination-specific).
- Do NOT handle TUI rendering concerns — that is Story
  4.2
- Do NOT implement cursor caching or LRU — that is
  Epic 5
- Do NOT modify `expand_object` behavior — it already
  works correctly with `entry_count`
- Do NOT break the `collection_entry_count()` function
  — reuse it

### Previous Story Intelligence

**From Epic 8 (stories 8.0-8.3):**
- `all_offsets` was replaced with sorted `Vec<(u64,u64)>`
  + binary_search — pagination can use `instance_offsets`
  the same way
- `find_instance` uses BinaryFuse8 filters — expect
  ~0.4% false positives, handle `None` results
- Parallel heap parsing established `HeapSegmentResult`
  pattern — similar per-worker patterns may apply if
  pagination needs parallelism (unlikely for 1000-entry
  pages)

**From Epic 3 (story 3.5 — collection size indicators):**
- `collection_entry_count()` (line 50) already identifies
  14 Java collection types via `COLLECTION_CLASS_SUFFIXES`
  (line 27) — suffix matching with
  `eq_ignore_ascii_case`
- Size field detection is inline (searches for `size`,
  `elementCount`, `count` among immediate class fields)
- Class name resolution uses `rsplit('.').next()` inline

**From cyclic reference fix (post-Epic 8):**
- `expand_object` now detects cycles via `expanding` set
- Pagination extractors that follow `next` pointers
  (HashMap chains, LinkedList) MUST also guard against
  cycles to prevent infinite loops

### Git Intelligence

Recent commits show:
- `73d7d79` — Non-recursive collapse for nested objects
- `8e3ff3e` — Cyclic reference detection in expansion
- `180bf7a` — Epic 8 squash merge (FxHashMap, lazy
  strings, parallel heap)

Code patterns: feature commits are squash-merged per
epic. Individual story work is on feature branches.

### Project Structure Notes

- New file: `crates/hprof-engine/src/pagination.rs`
- Modified files:
  - `crates/hprof-engine/src/engine.rs` (EntryInfo,
    CollectionPage, get_page signature)
  - `crates/hprof-engine/src/engine_impl.rs` (get_page
    implementation)
  - `crates/hprof-engine/src/lib.rs` (register module)
- Modified in `hprof-parser` (data accessor only):
  - `crates/hprof-parser/src/hprof_file.rs` (add
    `find_object_array`)
- **Test builder:** `add_object_array` already exists in
  `crates/hprof-parser/src/test_utils.rs` (line 189) —
  use it to build test fixtures for pagination tests

### References

- [Source: docs/planning-artifacts/epics.md#Epic 4]
- [Source: docs/planning-artifacts/architecture.md#Data Architecture]
- [Source: docs/planning-artifacts/architecture.md#Frontend Architecture]
- [Source: docs/planning-artifacts/ux-design-specification.md#Journey 3]
- [Source: docs/implementation-artifacts/epic-8-retro-2026-03-09.md#Action Items]
- [Source: crates/hprof-engine/src/engine.rs#NavigationEngine trait]
- [Source: crates/hprof-engine/src/engine_impl.rs#collection_entry_count]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6

### Debug Log References

None — clean implementation, no debug issues.

### Completion Notes List

- Enriched `EntryInfo` with `index`, `key`, `value` fields
  and added `CollectionPage` return type
- Changed `get_page` trait signature from `Vec<EntryInfo>`
  to `Option<CollectionPage>`
- Added `find_object_array` to `HprofFile` (segment filter
  scan, same pattern as `find_prim_array`)
- Created `pagination.rs` module with type dispatch and
  extractors for: ObjectArrayDump, PrimArrayDump,
  ArrayList, HashMap, LinkedHashMap, ConcurrentHashMap,
  HashSet, LinkedHashSet, LinkedList
- Vector and CopyOnWriteArrayList route through ArrayList
  extractor (same internal structure)
- Unsupported types (TreeMap, TreeSet, Hashtable,
  ArrayDeque, PriorityQueue) return `None`
- Cycle guards on HashMap chain walking and LinkedList
  traversal
- 14 new unit tests covering all ACs (original)
- 6 additional tests added by code review: LinkedList chain walk, LinkedList
  offset, ConcurrentHashMap "val" field, HashSet keys-only, LinkedHashMap
  delegation, Vector elementCount field
- 421 total tests passing, zero regressions
- Zero clippy warnings, code formatted

### File List

- `crates/hprof-engine/src/engine.rs` (modified)
- `crates/hprof-engine/src/engine_impl.rs` (modified)
- `crates/hprof-engine/src/lib.rs` (modified)
- `crates/hprof-engine/src/pagination.rs` (new)
- `crates/hprof-parser/src/hprof_file.rs` (modified)
- `crates/hprof-tui/src/app.rs` (modified)
- `docs/implementation-artifacts/sprint-status.yaml` (modified)
- `docs/implementation-artifacts/4-1-collection-pagination-engine.md` (modified)
