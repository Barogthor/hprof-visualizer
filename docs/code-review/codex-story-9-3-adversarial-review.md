# Adversarial Code Review - Story 9.3

## Scope

- Story file: `docs/implementation-artifacts/9-3-arrow-expand-unexpand-parent-navigation.md`
- Story status: `review`
- Reviewed source files from story File List:
  - `crates/hprof-tui/src/input.rs`
  - `crates/hprof-tui/src/views/stack_view/state.rs`
  - `crates/hprof-tui/src/views/stack_view/tests.rs`
  - `crates/hprof-tui/src/app/mod.rs`
  - `crates/hprof-tui/src/views/help_bar.rs`
- Validation commands executed:
  - `cargo test --all` (pass)
  - `cargo clippy --all-targets -- -D warnings` (pass)

## Git vs Story Discrepancies

- Files changed in git but not listed in story File List:
  - `crates/hprof-engine/src/pagination/tests.rs`
  - `tools/hprof-redact-custom/src/main/java/io/hprofvisualizer/redact/PathOnlyTransformer.java`
  - `docs/code-review/claude-story-9-4-adversarial-review.md`
  - `docs/implementation-artifacts/9-4-camera-scroll.md`
  - `docs/implementation-artifacts/9-5-stack-frame-variable-names-and-static-fields.md`
- Files listed in story File List but with no current git diff:
  - `crates/hprof-tui/src/input.rs`
  - `crates/hprof-tui/src/views/stack_view/state.rs`
  - `crates/hprof-tui/src/views/stack_view/tests.rs`
  - `crates/hprof-tui/src/app/mod.rs`
  - `crates/hprof-tui/src/views/help_bar.rs`
  - `docs/implementation-artifacts/sprint-status.yaml`

## Findings

### HIGH-1 - ArrowLeft no-op on collection entries without ObjectRef

- **AC impacted:** AC3 (navigate to parent when no expanded children)
- **Evidence:**
  - `crates/hprof-tui/src/app/mod.rs:614` uses `s.selected_collection_entry_ref_id()?`
  - `crates/hprof-tui/src/views/stack_view/state.rs:275` returns `None` for non-`ObjectRef` entries
- **Why this is wrong:** for primitive/null collection entries, `?` returns `None` and the handler exits with no command instead of navigating to parent.
- **Expected behavior:** `ArrowLeft` should dispatch `NavigateToParent(s.parent_cursor()?)` when entry is not expandable.

### HIGH-2 - ArrowLeft no-op on non-ObjectRef collection entry object fields

- **AC impacted:** AC3
- **Evidence:**
  - `crates/hprof-tui/src/app/mod.rs:622` uses `s.selected_collection_entry_obj_field_ref_id()?`
  - `crates/hprof-tui/src/views/stack_view/state.rs:293` returns `None` for non-`ObjectRef` fields (and cyclic terminals)
- **Why this is wrong:** when the field is primitive or terminal, `ArrowLeft` becomes no-op instead of moving to parent cursor.
- **Expected behavior:** if field is not expanded/expandable, always navigate to parent.

### HIGH-3 - ArrowRight is not equivalent to Enter for collection entries

- **AC impacted:** AC1 (Right expands collapsed node equivalent to Enter)
- **Evidence:**
  - `crates/hprof-tui/src/app/mod.rs:515` (Right handler on `OnCollectionEntry`) only checks `selected_collection_entry_ref_id()` and `expansion_state`
  - `crates/hprof-tui/src/app/mod.rs:765` (Enter handler) includes `selected_collection_entry_count()` branch for collection pagination
- **Why this is wrong:** Right fails to follow Enter behavior for collection entries that should open paged collections.
- **Expected behavior:** Right path must mirror Enter path for collection entry collection-detection and `StartCollection` dispatch.

### HIGH-4 - ArrowRight is not equivalent to Enter for collection entry object fields

- **AC impacted:** AC1
- **Evidence:**
  - `crates/hprof-tui/src/app/mod.rs:523` (Right handler on `OnCollectionEntryObjField`) only checks object-ref expansion
  - `crates/hprof-tui/src/app/mod.rs:779` (Enter handler) includes `selected_collection_entry_obj_field_collection_info()` for collection expansion
- **Why this is wrong:** Right cannot open nested collection fields where Enter can.
- **Expected behavior:** Right handler must include the same collection-info branch as Enter.

### MEDIUM-1 - Review context mismatch between story File List and working tree

- **Evidence:** mismatch list in section "Git vs Story Discrepancies" above.
- **Why this matters:** traceability is degraded; reviewer cannot correlate claimed implementation session with current local changes.
- **Expected behavior:** story File List should match actual change set used for review context, or explicitly note review is against committed state.

### LOW-1 - Stale story annotation leaks into user-facing help text

- **Evidence:** `crates/hprof-tui/src/views/help_bar.rs:30` still has `TODO(7.1)` and labels like `"(Story 7.1)"` in rendered help entries (`:31`, `:32`).
- **Why this matters:** internal planning labels appear in runtime UI.
- **Expected behavior:** remove sprint/story tags from user-facing keymap labels.

## AC Validation

- **AC1:** PARTIAL (Right handler diverges from Enter for collection-entry paths)
- **AC2:** PARTIAL (collapse behavior present for expanded object refs, but navigation fallback issues remain around collection-entry leaves)
- **AC3:** PARTIAL (parent navigation missing for non-ObjectRef collection-entry paths)
- **AC4:** IMPLEMENTED (top-level `OnFrame`/`NoFrames` no-op)
- **AC5:** IMPLEMENTED (help keymap includes Right/Left)
- **AC6:** IMPLEMENTED (`cargo test --all` passes)

## Outcome

- **Decision:** Changes Requested
- **Reason:** 4 HIGH issues affecting core keyboard semantics (Right/Left equivalence and parent navigation).

## Post-Fix Resolution (2026-03-11)

- HIGH-1 fixed in `crates/hprof-tui/src/app/mod.rs` (`InputEvent::Left` on `OnCollectionEntry` now falls back to parent navigation when entry is non-`ObjectRef`).
- HIGH-2 fixed in `crates/hprof-tui/src/app/mod.rs` (`InputEvent::Left` on `OnCollectionEntryObjField` now falls back to parent navigation when field is non-`ObjectRef`).
- HIGH-3 fixed in `crates/hprof-tui/src/app/mod.rs` (`InputEvent::Right` on `OnCollectionEntry` now mirrors Enter and dispatches `StartCollection` when `entry_count` is present).
- HIGH-4 fixed in `crates/hprof-tui/src/app/mod.rs` (`InputEvent::Right` on `OnCollectionEntryObjField` now mirrors Enter and dispatches `StartCollection` for collection fields).
- MEDIUM-1 addressed by updating story metadata and traceability in `docs/implementation-artifacts/9-3-arrow-expand-unexpand-parent-navigation.md`.
- Regression coverage added in `crates/hprof-tui/src/app/tests.rs`:
  - `right_on_nested_collection_entry_starts_collection_paging`
  - `right_on_collection_entry_object_field_collection_starts_collection_paging`
  - `left_on_primitive_collection_entry_navigates_to_parent_var`
  - `left_on_primitive_collection_entry_object_field_navigates_to_parent_entry`
