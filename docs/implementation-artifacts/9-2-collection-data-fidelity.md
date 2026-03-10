# Story 9.2: Collection Data Fidelity (ArrayList, Object[])

Status: done

## Story

As a user,
I want ArrayList and other Java collections to display their actual element count and all
elements (matching VisualVM output),
So that I can trust the data shown in the tool.

## Acceptance Criteria

**AC1** — Given a `java.util.ArrayList` with N elements
When the user expands it (presses Enter on the variable row)
Then the tool displays N elements directly, by treating the ArrayList as a collection
rather than showing raw instance fields (`size`, `elementData`, `modCount`, etc.)

**AC2** — Given a `java.util.LinkedList`, `java.util.ArrayDeque`, or any other known
collection class (from `COLLECTION_CLASS_SUFFIXES` in engine_impl.rs)
When expanded
Then the tool resolves the backing structure and displays all contained objects via
the existing pagination system

**AC3** — Given an `Object[]` array referenced as a local variable
When the user expands the array node
Then all array elements are resolved and displayed via the collection pagination system
(batches per `CollectionChunks` rules — eager ≤100 entries, chunks above)

**AC4** — Given a `PRIM_ARRAY_DUMP` (primitive array, e.g. `int[]`, `byte[]`)
When the user expands the array node
Then all primitive elements are displayed via the existing `try_prim_array` path in
`pagination.rs`, without requiring object resolution

**AC5** — Given a local variable that is any of: ArrayList, Object[], int[], etc.
When it is rendered in the stack frame view (collapsed)
Then the label shows `ClassName (N entries)` using the existing
`format_object_ref_collapsed` format

**AC6** — Given an `Object[]` or `int[]` (or any array type) that appears as an **element
inside another collection** (e.g. `Object[][]`, `ArrayList<int[]>`)
When the user navigates to that element in the collection view and presses Enter
Then the nested array expands as a collection (non-eagerly) — the user sees its elements
on the next interaction, not the raw object fields or a Failed state

**AC7** — Given all existing tests
When `cargo test --all` is run
Then zero failures — no regressions

## Out of Scope

- Collections that are fields of expanded objects (already working — `entry_count` is
  populated in `FieldValue::ObjectRef` by `decode_object_fields`, TUI dispatches
  `StartCollection` via `selected_field_collection_info`)
- The "unsupported collection type" path that falls back to `expand_object` — no change
  needed there (it remains correct behavior for types not in `COLLECTION_CLASS_SUFFIXES`)
- Adding new collection extractors in `pagination.rs` — only existing extractors in scope
- `OnCollectionEntryObjField` nested expansion — if a user expands a collection entry
  object and one of its **fields** is an `Object[]`, that field is handled by
  `OnCollectionEntryObjField` (app.rs ~line 538) which also dispatches `StartEntryObj`.
  This case is NOT fixed here. The user would see a `Failed` state for that specific
  sub-field. Fixing it would require the same `entry_count` check in
  `OnCollectionEntryObjField` — deferred to a future story since it requires
  `selected_collection_entry_obj_field_collection_info()`, a non-trivial helper.

## Tasks / Subtasks

### 1. Add `entry_count` to `VariableValue::ObjectRef` (AC5)

- [x] In `crates/hprof-engine/src/engine.rs`, update `VariableValue::ObjectRef`:
  ```rust
  ObjectRef {
      id: u64,
      class_name: String,
      entry_count: Option<u64>,  // ADD: Some(n) if collection/array, None otherwise
  }
  ```
- [x] Update `MemorySize for VariableValue` (engine.rs ~line 189):
  `ObjectRef { class_name, .. }` body is unchanged (entry_count is Copy, no heap alloc)
- [x] Update ALL construction sites of `VariableValue::ObjectRef` to add `entry_count: None`
  (there are at least 3 sites: engine_impl.rs and test code):
  ```
  rg "VariableValue::ObjectRef" crates/
  ```

### 2. Populate `entry_count` in `get_local_variables()` (AC1–AC4)

- [x] In `crates/hprof-engine/src/engine_impl.rs`, update `get_local_variables()`
  (lines 736–766). After resolving `class_name`, add entry_count computation:
  ```rust
  // Try instance → compute collection entry_count
  let (class_name, entry_count) =
      if let Some(raw) = self.hfile.find_instance(object_id) {
          let cn = self.hfile.index.class_names_by_id
              .get(&raw.class_object_id)
              .cloned()
              .unwrap_or_else(|| "Object".to_string());
          let ec = collection_entry_count(&cn, &raw, &self.hfile.index,
                                         self.hfile.header.id_size,
                                         self.hfile.records_bytes());
          (cn, ec)
      } else if let Some((_cid, elems)) = self.hfile.find_object_array(object_id) {
          ("Object[]".to_string(), Some(elems.len() as u64))
      } else if let Some((etype, bytes)) = self.hfile.find_prim_array(object_id) {
          let type_name = prim_array_type_name(etype).to_string();
          let esz = field_byte_size(etype, self.hfile.header.id_size);
          let cnt = if esz > 0 { bytes.len() / esz } else { 0 };
          (format!("{type_name}[]"), Some(cnt as u64))
      } else {
          ("Object".to_string(), None)
      };
  VariableValue::ObjectRef { id: object_id, class_name, entry_count }
  ```
  - [x] **`find_instance` API note**: In `get_local_variables()`, `self.hfile.find_instance()`
    is a method on `HprofFile` (direct file-level lookup). This is different from
    `Engine::read_instance_public(hfile, id)`, which is a public static helper on `Engine`
    used in `pagination.rs` (same underlying lookup, different call site). Use
    `self.hfile.find_instance(object_id)` here — `self` is `Engine`, not callable via the
    static form inside an instance method.
  - [x] Verify call sites of `engine.get_local_variables()` in `app.rs` — confirm it is
    called only once per frame expand (cached in `StackState.vars`) and NOT on every render
    tick. The fallback chain now calls up to 3 hfile lookups per variable, so a re-call on
    every tick would be a performance regression:
    ```
    rg -n "get_local_variables" crates/hprof-tui/src/app.rs
    ```
  - [x] Verify `prim_array_type_name(etype)` has a `_ => "unknown"` fallback arm and does
    NOT panic on an unrecognized type byte — check body at its definition site:
    ```
    rg -n "fn prim_array_type_name" crates/hprof-engine/src/engine_impl.rs
    ```
    If it uses `match etype { ... }` without a wildcard, add `_ => "unknown"` to prevent
    a future hprof file with a non-standard primitive type from crashing.

### 3. Update `OnVar` Enter handler in TUI (AC1–AC4)

- [x] In `crates/hprof-tui/src/app.rs`, update the `StackCursor::OnVar` arm of the
  Enter key handler (~line 476). Add collection detection before the phase match:
  ```rust
  StackCursor::OnVar { .. } => {
      let oid = s.selected_object_id()?;
      // Check if var is a collection/array → dispatch StartCollection
      if let Some(ec) = s.selected_var_entry_count() {
          if s.collection_chunks.contains_key(&oid) {
              return Some(Cmd::CollapseCollection(oid));
          }
          return Some(Cmd::StartCollection(oid, ec));
      }
      match s.expansion_state(oid) {
          ExpansionPhase::Collapsed => Cmd::StartObj(oid),
          ExpansionPhase::Failed => return None,
          ExpansionPhase::Expanded => Cmd::CollapseObj(oid),
          ExpansionPhase::Loading => return None,
      }
  }
  ```
  Note: `collection_chunks` is a public(crate) field on `StackState` — accessible here.

### 4. Add `selected_var_entry_count()` to `StackState` (AC1–AC4)

- [x] In `crates/hprof-tui/src/views/stack_view.rs`, add a method to `StackState`:
  ```rust
  /// Returns `Some(entry_count)` if the currently selected variable is a collection
  /// or array, `None` otherwise.
  pub fn selected_var_entry_count(&self) -> Option<u64> {
      let StackCursor::OnVar { frame_idx, var_idx } = self.cursor else {
          return None;
      };
      let frame_id = self.frames.get(frame_idx)?.id;
      let vars = self.vars.get(&frame_id)?;
      let var = vars.get(var_idx)?;
      if let VariableValue::ObjectRef { entry_count, .. } = &var.value {
          *entry_count
      } else {
          None
      }
  }
  ```
  Locate near `selected_field_collection_info()` (~line 496).

### 5. Update var rendering to show `(N entries)` (AC5)

- [x] In `stack_view.rs`, find ALL sites where `VariableValue::ObjectRef { class_name, .. }`
  is formatted for display. Update each to pass `entry_count` to
  `format_object_ref_collapsed`:
  ```
  rg -n "VariableValue::ObjectRef" crates/hprof-tui/src/views/stack_view.rs
  ```
  Replace bare `class_name` display with:
  ```rust
  format_object_ref_collapsed(class_name, *entry_count)
  ```
  Update **every** matching site — not just the first. Missing one produces inconsistent
  labels (e.g. stack view shows count, favorites panel does not).

### 6. Enrich `id_to_field_value` with array fallback (AC6)

- [x] In `crates/hprof-engine/src/pagination.rs`, update `id_to_field_value()` (~line 484)
  to apply the same instance → object_array → prim_array fallback chain as
  `get_local_variables()`. Replace the current body:
  ```rust
  fn id_to_field_value(id: u64, hfile: &HprofFile) -> FieldValue {
      if id == 0 {
          return FieldValue::Null;
      }
      // Try instance first (covers all regular objects and collections)
      if let Some(raw) = Engine::read_instance_public(hfile, id) {
          let class_name = hfile.index.class_names_by_id
              .get(&raw.class_object_id)
              .cloned()
              .unwrap_or_else(|| "Object".to_string());
          let entry_count = collection_entry_count(
              &class_name, &raw, &hfile.index,
              hfile.header.id_size, hfile.records_bytes(),
          );
          let inline_value = resolve_inline_value(&class_name, hfile, id);
          return FieldValue::ObjectRef { id, class_name, entry_count, inline_value };
      }
      // Try Object[] array
      if let Some((_cid, elems)) = hfile.find_object_array(id) {
          return FieldValue::ObjectRef {
              id,
              class_name: "Object[]".to_string(),
              entry_count: Some(elems.len() as u64),
              inline_value: None,
          };
      }
      // Try primitive array
      if let Some((etype, bytes)) = hfile.find_prim_array(id) {
          let type_name = prim_array_type_name(etype);
          let esz = field_byte_size(etype, hfile.header.id_size);
          let cnt = if esz > 0 { bytes.len() / esz } else { 0 };
          return FieldValue::ObjectRef {
              id,
              class_name: format!("{type_name}[]"),
              entry_count: Some(cnt as u64),
              inline_value: None,
          };
      }
      // Unknown ID
      FieldValue::ObjectRef { id, class_name: "Object".to_string(),
                              entry_count: None, inline_value: None }
  }
  ```
  **Visibility changes required** — these private functions in `engine_impl.rs` must become
  `pub(crate)` so `pagination.rs` can call them:
  - `collection_entry_count` → `pub(crate)`
  - `prim_array_type_name` → `pub(crate)`
  - `field_byte_size` → `pub(crate)`
  `resolve_inline_value` is already `pub(crate)` (used in this file via
  `crate::engine_impl::resolve_inline_value`).

  **Performance note:** `find_object_array(id)` allocates and returns the **full** element
  `Vec<u64>` just to get `.len()`. This is called for every entry in every loaded page.
  Acceptable for now given typical collection sizes, but note the allocation. If profiling
  later shows this as a hotspot, a `find_object_array_count(id) -> Option<usize>` helper
  that only reads the element count without allocating the vector would be the fix.

### 7. Update `OnCollectionEntry` handler to dispatch `StartCollection` (AC6)

- [x] In `crates/hprof-tui/src/app.rs`, update the `StackCursor::OnCollectionEntry` arm
  (~line 529). Add collection detection before the phase match — same pattern as `OnVar`:
  ```rust
  StackCursor::OnCollectionEntry { .. } => {
      let oid = s.selected_collection_entry_ref_id()?;
      // If entry is itself a collection/array, dispatch StartCollection
      if let Some(ec) = s.selected_collection_entry_count() {
          if s.collection_chunks.contains_key(&oid) {
              return Some(Cmd::CollapseCollection(oid));
          }
          return Some(Cmd::StartCollection(oid, ec));
      }
      match s.expansion_state(oid) {
          ExpansionPhase::Collapsed => Cmd::StartEntryObj(oid),
          ExpansionPhase::Failed => return None,
          ExpansionPhase::Expanded => Cmd::CollapseEntryObj(oid),
          ExpansionPhase::Loading => return None,
      }
  }
  ```

### 8. Add `selected_collection_entry_count()` to `StackState` (AC6)

- [x] In `crates/hprof-tui/src/views/stack_view.rs`, add a method alongside
  `selected_var_entry_count()` (added in task 4):
  ```rust
  /// Returns `Some(entry_count)` if the currently selected collection entry is itself
  /// a collection or array, `None` otherwise.
  pub fn selected_collection_entry_count(&self) -> Option<u64> {
      let StackCursor::OnCollectionEntry { collection_id, entry_index } = self.cursor
      else {
          return None;
      };
      // `entry_index` is the **absolute** index within the full collection
      // (matches `EntryInfo.index`, not the offset within the current page/chunk).
      let chunks = self.collection_chunks.get(&collection_id)?;
      // Navigate chunk state the same way selected_collection_entry_ref_id() does —
      // check eager_page first, then chunk_pages for the matching offset range.
      let entry = chunks.eager_page.as_ref()
          .and_then(|p| p.entries.iter().find(|e| e.index == entry_index))
          .or_else(|| {
              chunks.chunk_pages.values().find_map(|cs| {
                  if let ChunkState::Loaded(page) = cs {
                      page.entries.iter().find(|e| e.index == entry_index)
                  } else {
                      None
                  }
              })
          })?;
      if let FieldValue::ObjectRef { entry_count, .. } = &entry.value {
          *entry_count
      } else {
          None
      }
  }
  ```
  Follow the exact same navigation pattern used by `selected_collection_entry_ref_id()`:
  ```
  rg -n "fn selected_collection_entry_ref_id" crates/hprof-tui/src/views/stack_view.rs
  ```
  Adapt the field access above to match the actual struct field names at that site.

### 9. Update tests (AC1–AC7)

- [x] In `engine_impl.rs` test module `expand_object_tests` / `get_local_variables` tests
  (~line 1269), add `entry_count: None` to ALL existing `VariableValue::ObjectRef`
  constructions (compiler will guide you after step 1):
  ```
  rg "VariableValue::ObjectRef" crates/hprof-engine/src/engine_impl.rs
  ```
- [x] Add new tests in `engine_impl.rs` test module for `get_local_variables`:

  ```rust
  #[test]
  fn get_local_variables_object_array_root_has_entry_count() {
      // root points to OBJ_ARRAY_DUMP → entry_count = Some(len)
      let mut builder = HprofTestBuilder::new(1, 8);
      builder.add_object_array(0xBBB, 0xCC, &[0xC01, 0xC02, 0xC03]);
      builder.add_stack_frame(0xF1, 0, 0, 0, 0, 0);
      builder.add_stack_trace(1, &[0xF1]);
      builder.add_thread_roots(0xF1, &[0xBBB]);
      let engine = Engine::from_hprof_bytes(builder.build(), default_config());
      let vars = engine.get_local_variables(0xF1);
      assert_eq!(vars.len(), 1);
      match &vars[0].value {
          VariableValue::ObjectRef { class_name, entry_count, .. } => {
              assert_eq!(class_name, "Object[]");
              assert_eq!(*entry_count, Some(3));
          }
          _ => panic!("expected ObjectRef"),
      }
  }

  #[test]
  fn get_local_variables_prim_array_root_has_entry_count() {
      // root points to PRIM_ARRAY_DUMP (int, 4 bytes each, 5 elements)
      let int_bytes: Vec<u8> = (0u32..5).flat_map(|n| n.to_be_bytes()).collect(); // 5 ints × 4 bytes = 20 bytes
      let mut builder = HprofTestBuilder::new(1, 8);
      builder.add_prim_array(0xCCC, 0, 5, 10, &int_bytes); // num_elements=5, type 10=int
      builder.add_stack_frame(0xF1, 0, 0, 0, 0, 0);
      builder.add_stack_trace(1, &[0xF1]);
      builder.add_thread_roots(0xF1, &[0xCCC]);
      let engine = Engine::from_hprof_bytes(builder.build(), default_config());
      let vars = engine.get_local_variables(0xF1);
      assert_eq!(vars.len(), 1);
      match &vars[0].value {
          VariableValue::ObjectRef { class_name, entry_count, .. } => {
              assert_eq!(class_name, "int[]");
              assert_eq!(*entry_count, Some(5));
          }
          _ => panic!("expected ObjectRef"),
      }
  }

  #[test]
  fn get_local_variables_plain_object_has_no_entry_count() {
      // Regular INSTANCE_DUMP (not a collection) → entry_count must be None.
      // Use the minimal builder pattern: one instance of a non-collection class,
      // one stack frame, one stack trace, one thread root pointing to the instance.
      // After engine construction:
      //   let vars = engine.get_local_variables(frame_id);
      //   assert_eq!(vars.len(), 1);
      //   match &vars[0].value {
      //       VariableValue::ObjectRef { entry_count, .. } => {
      //           assert_eq!(*entry_count, None);
      //       }
      //       _ => panic!("expected ObjectRef"),
      //   }
      // Use any existing non-collection instance test in engine_impl.rs as reference —
      // e.g. look for "add_instance" + "add_stack_frame" builder patterns.
      todo!("implement using add_instance builder — see existing non-collection tests")
  }
  ```

- [x] Add `selected_var_entry_count` unit test in `stack_view.rs` test module:
  ```rust
  // Construct StackState with OnVar → ObjectRef { entry_count: Some(42) }
  // assert selected_var_entry_count() == Some(42)
  // Also: with entry_count: None → assert returns None
  // Also: with cursor NOT OnVar (e.g. OnFrame) → assert returns None
  ```
- [x] Add `selected_collection_entry_count` unit test in `stack_view.rs` test module:
  ```rust
  // Construct StackState with OnCollectionEntry pointing to an entry whose
  // FieldValue::ObjectRef has entry_count: Some(3)
  // assert selected_collection_entry_count() == Some(3)
  // Also: entry_count: None → returns None
  // Also: cursor not OnCollectionEntry → returns None
  // Also: entry not yet loaded (chunk still Collapsed) → returns None (graceful)
  ```
- [x] Add test for AC2 — non-ArrayList known collection (e.g. LinkedList) shows entry count
  as a local variable. In `engine_impl.rs` test module:
  ```rust
  #[test]
  fn get_local_variables_linked_list_root_has_entry_count() {
      // Build a LinkedList instance with size=3.
      // collection_entry_count() reads the "size" field of the instance —
      // verify it returns Some(3) for a LinkedList class name.
      // Builder: add_class(LLC_ID, "java.util.LinkedList")
      //          add_instance(LIST_ID, LLC_ID, fields: size=3, ...)
      //          add_stack_frame + add_stack_trace + add_thread_roots(LIST_ID)
      // Then:
      //   let vars = engine.get_local_variables(frame_id);
      //   match &vars[0].value {
      //       VariableValue::ObjectRef { class_name, entry_count, .. } => {
      //           assert_eq!(class_name, "java.util.LinkedList");
      //           assert_eq!(*entry_count, Some(3));
      //       }
      //       _ => panic!("expected ObjectRef"),
      //   }
      // Use the COLLECTION_CLASS_SUFFIXES list to confirm "LinkedList" is covered.
      todo!("implement using add_instance with LinkedList class name and size field")
  }
  ```
- [x] Add end-to-end integration test in `stack_view.rs` test module covering the full
  dispatch path from variable → entry_count → correct command:
  ```rust
  // 5-Whys insight: FieldValue and VariableValue diverged silently because no test
  // ever exercised the full path: array variable → entry_count set → StartCollection.
  // This test pins the invariant so the asymmetry cannot regress.
  #[test]
  fn object_array_var_dispatches_start_collection_not_start_obj() {
      // Build a StackState with one frame, one var with entry_count: Some(3)
      let var = VariableInfo {
          index: 0,
          value: VariableValue::ObjectRef {
              id: 0xA00,
              class_name: "Object[]".to_string(),
              entry_count: Some(3),
          },
      };
      let mut state = StackState::new_for_test(frame, vec![var]);
      state.set_cursor(StackCursor::OnVar { frame_idx: 0, var_idx: 0 });
      assert_eq!(state.selected_var_entry_count(), Some(3));
      // And confirm selected_object_id returns the array id
      assert_eq!(state.selected_object_id(), Some(0xA00));
  }
  ```
  Note: adapt to actual `StackState` test construction pattern used in the file
  (see `make_var_object_ref` helper at ~line 1394). Before implementing, verify
  whether `new_for_test` and `set_cursor` exist:
  ```
  rg -n "fn new_for_test\|fn set_cursor" crates/hprof-tui/src/views/stack_view.rs
  ```
  Use whichever construction helpers the file actually provides.

- [x] Add edge case test for empty collection (`entry_count: Some(0)`):
  ```rust
  #[test]
  fn get_local_variables_empty_object_array_has_entry_count_zero() {
      // empty Object[] → entry_count = Some(0), not None
      let mut builder = HprofTestBuilder::new(1, 8);
      builder.add_object_array(0xEEE, 0xCC, &[]);  // 0 elements
      builder.add_stack_frame(0xF1, 0, 0, 0, 0, 0);
      builder.add_stack_trace(1, &[0xF1]);
      builder.add_thread_roots(0xF1, &[0xEEE]);
      let engine = Engine::from_hprof_bytes(builder.build(), default_config());
      let vars = engine.get_local_variables(0xF1);
      match &vars[0].value {
          VariableValue::ObjectRef { entry_count, .. } => {
              assert_eq!(*entry_count, Some(0));  // not None — dispatch StartCollection
          }
          _ => panic!("expected ObjectRef"),
      }
  }
  ```
  Rationale: `Some(0)` must dispatch `StartCollection(oid, 0)` (not `StartObj`), so that
  `get_page(oid, 0, 0)` returns an empty page cleanly rather than attempting instance
  expansion on an array ID. Verify `get_page` returns `Some(CollectionPage { entries: [],
  total_count: 0 })` and does not panic when limit=0.
- [x] Add test in `pagination.rs` for `id_to_field_value` with a nested array (AC6):
  ```rust
  #[test]
  fn id_to_field_value_for_object_array_id_sets_entry_count() {
      // Build an HprofFile with an Object[] of 3 elements via HprofTestBuilder.
      // Then call id_to_field_value(array_id, &hfile) directly (it's a private fn —
      // call it through get_page: build an outer Object[] containing the inner Object[],
      // call get_page on the outer array, and inspect the first entry's entry_count).
      // Alternative: make id_to_field_value pub(crate) for testing, then call directly.
      //
      // Expected result for the inner Object[] entry:
      //   FieldValue::ObjectRef { class_name: "Object[]", entry_count: Some(3), .. }
      //
      // Reference builder pattern: prim_array_int_pagination() test (~line 808).
      todo!("implement using HprofTestBuilder — add_object_array with 3 elem IDs")
  }

  #[test]
  fn id_to_field_value_for_prim_array_id_sets_entry_count() {
      // Build an HprofFile with an outer Object[] containing one int[] of 5 elements.
      // Call get_page(outer_array_id, 0, 100).
      // Inspect entries[0]:
      //   FieldValue::ObjectRef { class_name: "int[]", entry_count: Some(5), .. }
      //
      // Builder: add_prim_array(inner_id, 0, 5, 10, &int_bytes)
      //          add_object_array(outer_id, 0, &[inner_id])
      // Then: engine.get_page(outer_id, 0, 100)
      todo!("implement using HprofTestBuilder — nested prim array inside object array")
  }
  ```
  These two tests pin that `paginate_id_slice` entries for nested arrays carry `entry_count`,
  enabling `StartCollection` dispatch in `OnCollectionEntry`.

- [x] Add test in `pagination.rs` test module verifying that `get_page` works when called
  with an **ArrayList instance ID directly** (not the backing array ID):
  ```rust
  // Critical path: OnVar dispatches StartCollection(list_id, ec) where list_id is the
  // ArrayList instance — confirm get_page routes correctly to extract_array_list.
  #[test]
  fn get_page_with_arraylist_instance_id_returns_elements() {
      // Verify that get_page routes correctly when given the ArrayList instance ID
      // (not the backing elementData ID). This is the path taken by OnVar after this fix.
      //
      // Build: follow the exact pattern of an existing ArrayList pagination test in
      // pagination.rs (search for `extract_array_list` test near line 808).
      // Minimal setup:
      //   - add_class(ARRAYLIST_CLASS_ID, "java.util.ArrayList")
      //   - add_object_array(ELEM_ARRAY_ID, 0, &[0xE1, 0xE2])  // elementData backing
      //   - add_instance(ARRAYLIST_ID, ARRAYLIST_CLASS_ID, fields including:
      //       size=2, elementData=ELEM_ARRAY_ID)
      // Then:
      //   let page = engine.get_page(ARRAYLIST_ID, 0, 100).unwrap();
      //   assert_eq!(page.total_count, 2);
      //   assert_eq!(page.entries.len(), 2);
      //
      // If no such test exists yet, look at how extract_array_list is called in the
      // existing test suite and replicate the builder pattern here.
      todo!("implement using existing ArrayList pagination test builder pattern")
  }
  ```
  See existing test `prim_array_int_pagination` (~line 808) and ArrayList tests for
  builder patterns.
- [x] Run `cargo test --all` — zero failures
- [x] Run `cargo clippy --all-targets -- -D warnings` — zero warnings

## Dev Notes

### Root Cause

`VariableValue::ObjectRef` has no `entry_count` field. `get_local_variables()` only calls
`find_instance()`, which returns `None` for `OBJ_ARRAY_DUMP` / `PRIM_ARRAY_DUMP` records,
leading to a fallback class_name `"Object"` with no collection metadata.

In the TUI, the `OnVar` Enter handler always dispatches `Cmd::StartObj(oid)`, which calls
`engine.expand_object(oid)`. For array IDs, `expand_object` → `decode_object_fields` →
`read_instance` returns `None` → expansion fails with `"! Object[] — Failed to resolve"`.

For ArrayList instances, expansion succeeds but returns raw fields (`size`, `elementData`,
`modCount`), not the list elements. The user must drill into `elementData` manually.

### Minimal Change, Maximum Reuse

Adding `entry_count: Option<u64>` to `VariableValue::ObjectRef` and populating it in
`get_local_variables()` propagates correctly to the TUI. The TUI's existing
`StartCollection` / `get_page()` / `pagination.rs` pipeline already handles all four
collection types correctly for **field** expansions — this story extends that to
**variable** expansions. The nested-array fix in `id_to_field_value()` additionally
propagates to all collection entry depths. Five files changed, no new pipeline code.

### Existing Code Reuse (Do NOT reinvent)

- `collection_entry_count(class_name, raw, index, id_size, data)` — already in
  `engine_impl.rs` (~line 51): detects ArrayList/HashMap/etc. via `size` / `elementCount`
  / `count` field scan. Call this for instances.
- `prim_array_type_name(etype)` — already in `engine_impl.rs`: converts type byte to
  `"int"`, `"char"`, etc.
- `field_byte_size(etype, id_size)` — already in `engine_impl.rs`: byte size per element.
- `format_object_ref_collapsed(class_name, entry_count)` — already in `stack_view.rs`
  (~line 215): formats `"ArrayList (42 entries)"`.
- `selected_field_collection_info()` — already in `stack_view.rs` (~line 496): pattern
  to follow for the new `selected_var_entry_count()`.
- `try_object_array()`, `try_prim_array()`, `extract_array_list()` — all in
  `pagination.rs`, all already correct.

### Key Invariants to Preserve

1. `expand_object()` is NOT called for collection variables after this fix — the TUI
   dispatches `StartCollection` instead. Do NOT change `expand_object()` itself.
2. `decode_object_fields()` enrichment of `FieldValue::ObjectRef` is unchanged — it
   already sets `entry_count` for fields pointing to arrays/collections.
3. The `OnObjectField` collection path (`selected_field_collection_info`) is unchanged.
4. LRU memory budget tracking: `VariableValue::ObjectRef.entry_count` is `Option<u64>`
   — it is `Copy`, adds no heap allocation, `MemorySize` impl needs no change to the
   calculation (just ensure the struct field is added to `size_of::<VariableValue>()`
   which happens automatically).

### Nested Arrays — Same Fix, Two Entry Points

The fix for `OnVar` and `OnCollectionEntry` is strictly the same pattern applied at two
entry points:

| Entry point | Where `entry_count` comes from | Handler fix |
|---|---|---|
| `OnVar` | `VariableValue::ObjectRef.entry_count` set by `get_local_variables()` | Task 3 |
| `OnCollectionEntry` | `FieldValue::ObjectRef.entry_count` set by `id_to_field_value()` | Task 7 |

`id_to_field_value` is the single function that builds `FieldValue` for every element
in every `Object[]` / ArrayList / collection page. Fixing it once covers all nesting
depths — `int[][]`, `Object[][][]`, `ArrayList<Object[]>`, etc. Non-eager: each depth
level expands only when the user presses Enter.

### `StartCollection` uses Instance ID, not Backing Array ID

When dispatching `StartCollection(list_id, ec)` from `OnVar` for an ArrayList, `list_id`
is the **instance ID** of the ArrayList object — NOT the ID of its `elementData` backing
array. This is correct and intentional:

- `get_page(list_id, ...)` in `pagination.rs` routes: `try_object_array(list_id)` → None
  (it is an instance) → `try_prim_array(list_id)` → None → `read_instance(list_id)` →
  dispatch by class name → `extract_array_list(hfile, raw, ...)` → reads `elementData`
  internally → paginates elements. ✅ Correct.
- `collection_chunks` is therefore keyed by `list_id`. Collapse: `collection_chunks
  .contains_key(&list_id)` → true → `CollapseCollection(list_id)` → correct.

This is **different** from the `OnObjectField` path, where `collection_id` is the backing
`elementData` array ID (because `selected_field_collection_info()` returns the child ref's
ID). Do NOT conflate the two paths.

### Collapse Path

The `CollapseCollection` command is already handled for `OnObjectField`. After this fix,
the `OnVar` handler dispatches `CollapseCollection(oid)` when the collection is already
expanded. Verify that `StackState` handles `CollapseCollection` for an oid that was
expanded from `OnVar` (search for `handle_collapse_collection` or similar in `app.rs`).

### Watch Out For: `find_instance` vs `find_object_array`

In `get_local_variables()`, the fallback chain must be tried in this order:
1. `find_instance(object_id)` — handles ArrayList, HashMap, and all JVM instances
2. `find_object_array(object_id)` — handles Object[]
3. `find_prim_array(object_id)` — handles int[], byte[], char[], etc.
4. Fallback to `("Object".to_string(), None)` — unknown ID (already correct behavior)

An object ID can only be in one of these categories in a valid hprof file.

**Known simplification:** For typed arrays (`String[]`, `Integer[]`), `find_object_array`
returns a `class_id` that could be used to resolve the element type name. This story uses
`"Object[]"` unconditionally for all object arrays — same as the existing `decode_object_fields`
enrichment path. VisualVM shows `String[]` with the correct type. Improving this requires
resolving element class names from `class_id` and is out of scope here — document as a
known limitation if user feedback requests it.

### Test Builder API

From `crates/hprof-parser/src/test_utils.rs` (feature `test-utils`):
- `add_object_array(id, class_id, elem_ids: &[u64])` — adds `OBJ_ARRAY_DUMP` record
- `add_prim_array(id, stack_serial, num_elements, elem_type, bytes: &[u8])` — adds
  `PRIM_ARRAY_DUMP` record
- Look at existing tests like `prim_array_int_pagination()` in `pagination.rs` (~line 808)
  for correct builder usage patterns.

### Project Structure

| File | Change | Tasks |
|------|--------|-------|
| `crates/hprof-engine/src/engine.rs` | Add `entry_count: Option<u64>` to `VariableValue::ObjectRef` | 1 |
| `crates/hprof-engine/src/engine_impl.rs` | Update `get_local_variables()`, make `collection_entry_count` / `prim_array_type_name` / `field_byte_size` `pub(crate)`, update all `VariableValue::ObjectRef` construction sites, add tests | 2, 9 |
| `crates/hprof-engine/src/pagination.rs` | Update `id_to_field_value()` with array fallback chain, add tests | 6, 9 |
| `crates/hprof-tui/src/app.rs` | Update `OnVar` and `OnCollectionEntry` Enter handlers | 3, 7 |
| `crates/hprof-tui/src/views/stack_view.rs` | Add `selected_var_entry_count()`, `selected_collection_entry_count()`, update var label formatting at ALL render sites | 4, 5, 8, 9 |

### References

Line numbers are approximate (`~`) — verify with `rg -n` before editing.

- [Source: docs/planning-artifacts/epics.md#Story 9.2] — original ACs and description
- [Source: crates/hprof-engine/src/engine.rs ~line 83] — `VariableValue` enum
- [Source: crates/hprof-engine/src/engine_impl.rs ~line 51] — `collection_entry_count()`
- [Source: crates/hprof-engine/src/engine_impl.rs ~line 736] — `get_local_variables()`
- [Source: crates/hprof-engine/src/engine_impl.rs ~line 768] — `expand_object()`
- [Source: crates/hprof-engine/src/pagination.rs ~line 14] — `get_page()` with array-first dispatch
- [Source: crates/hprof-engine/src/pagination.rs ~line 114] — `try_prim_array()`
- [Source: crates/hprof-engine/src/pagination.rs ~line 150] — `extract_array_list()`
- [Source: crates/hprof-engine/src/pagination.rs ~line 484] — `id_to_field_value()`
- [Source: crates/hprof-tui/src/app.rs ~line 476] — `OnVar` Enter handler
- [Source: crates/hprof-tui/src/app.rs ~line 529] — `OnCollectionEntry` handler
- [Source: crates/hprof-tui/src/views/stack_view.rs ~line 215] — `format_object_ref_collapsed()`
- [Source: crates/hprof-tui/src/views/stack_view.rs ~line 496] — `selected_field_collection_info()`

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

### Completion Notes List

- Added `entry_count: Option<u64>` to `VariableValue::ObjectRef` — zero heap cost (Copy)
- `get_local_variables()` now applies instance→Object[]→prim_array fallback chain to populate entry_count
- `collection_entry_count`, `prim_array_type_name`, `field_byte_size` promoted to `pub(crate)` for reuse in pagination.rs
- `id_to_field_value()` updated with same fallback chain → nested arrays now carry entry_count
- `OnVar` and `OnCollectionEntry` Enter handlers now dispatch `StartCollection` when entry_count is set
- Rendering in `tree_render.rs` updated to show `ClassName (N entries)` via `format_object_ref_collapsed`
- 20 new tests across `engine_impl.rs`, `pagination.rs`, and `stack_view.rs`

### File List

crates/hprof-engine/src/engine.rs
crates/hprof-engine/src/engine_impl.rs
crates/hprof-engine/src/pagination.rs
crates/hprof-tui/src/app.rs
crates/hprof-tui/src/favorites.rs
crates/hprof-tui/src/views/stack_view.rs
crates/hprof-tui/src/views/tree_render.rs
docs/implementation-artifacts/9-2-collection-data-fidelity.md
docs/implementation-artifacts/sprint-status.yaml

## Senior Developer Review (AI)

### Reviewer

Codex (2026-03-10)

### Findings Summary

- Initial review flagged documentation-traceability gaps (task checkbox mismatch, missing review notes).
- Code implementation and tests for AC1-AC7 are present and passing.
- Story/file-list traceability validated against commit `67ebd20`.

### Actions Applied

- Marked the remaining pagination test subtask complete (test exists in
  `crates/hprof-engine/src/pagination.rs`).
- Added this `Senior Developer Review (AI)` section.
- Added `Change Log` entry below and set story status to `done`.

### Verification

- `cargo test --all`: pass
- `cargo clippy --all-targets -- -D warnings`: pass

## Change Log

- 2026-03-10 (Codex): Applied code-review follow-up updates to story metadata:
  status -> done, completed remaining test checklist item, added senior review
  section, and synced sprint status.
