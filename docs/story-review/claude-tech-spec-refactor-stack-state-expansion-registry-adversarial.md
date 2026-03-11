# Adversarial Review: Tech-Spec Refactor StackState — Extract ExpansionRegistry

**Reviewed:** `docs/implementation-artifacts/tech-spec-refactor-stack-state-expansion-registry.md`
**Date:** 2026-03-11
**Reviewer:** Claude (adversarial pass — new findings only)

---

## Findings

### F15 — `favorites.rs` test directly accesses `state.collection_chunks` — silently omitted

**Severity:** High
**Validity:** Real

The spec lists 5 direct accesses to migrate in `tests.rs` and 11 in `app/mod.rs`. It does not mention
`favorites.rs`, which also has a direct field access: line 686 of
`crates/hprof-tui/src/favorites.rs`:

```rust
state.collection_chunks.insert(20, CollectionChunks { ... });
```

This is inside the `#[cfg(test)]` block at the bottom of `favorites.rs`, in the test
`snapshot_freezes_loading_chunks_to_collapsed`. After the refactor, `collection_chunks` becomes
`pub(super)` on `ExpansionRegistry` (only accessible from within `stack_view/`). Since
`favorites.rs` is outside that module, this access will fail to compile.

The spec does not include `favorites.rs` in `files_to_modify`, nor does it mention this access
anywhere. The implementer will hit a compilation error with no guidance on resolution. The fix is
either to expose a test-only `insert_collection_chunk` helper on `StackState`, use
`state.expansion.collection_chunks.insert(...)` (which requires `pub(crate)` on that field — a
deliberate downgrade from the design's `pub(super)`), or to expose the field at `pub(crate)` on
`ExpansionRegistry`. The spec must decide and document the approach.

---

### F16 — AC9 test already exists; spec's TDD framing is wrong

**Severity:** Medium
**Validity:** Real

The spec in T2b mandates writing the AC9 test first (TDD — "it doit échouer jusqu'à ce que
`set_expansion_failed` soit adapté"). AC9 describes a scenario where `OnObjectLoadingNode` with
`field_path: []` recovers to `OnVar` after `set_expansion_failed`.

This behavior is **already covered** by the existing test
`set_expansion_failed_recovers_cursor_from_loading_node_top_level` at line 194 of `tests.rs`. That
test is essentially identical to what AC9 describes: frame expanded with one `ObjectRef` var, object
in `Loading` phase, cursor navigated to `OnObjectLoadingNode`, then `set_expansion_failed` called,
asserts cursor lands on `OnVar { frame_idx: 0, var_idx: 0 }`.

The spec's TDD claim ("le test doit échouer jusqu'à…") is false — the test already passes before
any refactoring work begins. Writing a duplicate test in T2b adds noise, and if the implementer
relies on the test failing as a progress signal, they will get confused. The spec should either
strike the AC9 test entirely (covered) or redirect to a distinct scenario not currently covered
(e.g., cursor recovery after `set_expansion_failed` where `flat_index()` is `Some`, confirming the
mutation-first ordering matters).

---

### F17 — `expansion.rs` `use` block missing `HashSet`

**Severity:** Medium
**Validity:** Real

The spec's T1 prescribes a minimal `use` block for `expansion.rs`:

```rust
use std::collections::HashMap;
use hprof_engine::{CollectionPage, FieldInfo};
use super::types::{ChunkState, CollectionChunks, ExpansionPhase};
```

`CollectionPage` is not used by any of the methods listed for migration to `ExpansionRegistry`.
None of `set_expansion_loading`, `set_expansion_done`, `set_expansion_failed` (mutation half),
`cancel_expansion`, `collapse_object`, `chunk_state`, or `expansion_state` touch `CollectionPage`
directly; that type appears in `CollectionChunks` internally. Including it without a use site will
trigger a `dead_code`/`unused_import` clippy warning under `-D warnings`, breaking AC7.

Conversely, `HashSet` is not in the import list even though `cancel_expansion` and `collapse_object`
do not need it. However if any future migration step onto `ExpansionRegistry` requires it the
implementer has no guidance. The more immediate issue is the spurious `CollectionPage` import.

---

### F18 — `format_entry_line` visibility on `format.rs` conflicts with `favorites.rs` usage path

**Severity:** Low
**Validity:** Undecided

The spec downgrades `format_entry_line` to `pub(super)` in `format.rs`, meaning it is accessible
only from within `stack_view/`. The current call site in `state.rs` (`Self::format_entry_line(...)`)
will be updated to `super::format::format_entry_line(...)` — fine since `state.rs` is inside
`stack_view/`. However, `format_entry_line` is currently `pub(crate)` on `StackState`, which means
any caller outside `stack_view/` that was reaching it as `StackState::format_entry_line` would
break. A grep confirms no external callers exist today, making the downgrade safe. Still, the spec
does not show the result of that verification — it asserts "aucun usage externe aujourd'hui" in the
Notes section without citing the grep. An implementer who adds a call elsewhere before completing
the refactor would create a silent dependency. Low risk but should be verified with an explicit grep
command in T3 rather than relied on from prose.

---

### F19 — `object_phases.is_empty()` in `tests.rs` (line 555) accesses the field directly on `StackState`, not through a method — this will break

**Severity:** High
**Validity:** Real

The spec's T6 instruction says:

> `state.object_phases.is_empty()` → `state.expansion.object_phases.is_empty()`

The spec counts this as 1 access. This is correct as stated. However, the spec simultaneously
claims `ExpansionRegistry`'s fields are `pub(super)` — visible only from within `stack_view/`. The
test file `tests.rs` is a submodule of `stack_view` (declared as `mod tests` in `mod.rs` under
`#[cfg(test)]`). In Rust, a `mod tests` declared inside `stack_view/mod.rs` is a child module of
`stack_view`, so `pub(super)` fields of `ExpansionRegistry` are accessible from `tests.rs` via
`state.expansion.object_phases`. This is correct.

The issue is that `state.expansion` itself is `pub(crate)` on `StackState` (not `pub(super)`), and
`expansion.object_phases` is `pub(super)` on `ExpansionRegistry`. For `tests.rs` to access
`state.expansion.object_phases`, two conditions must hold:

1. `state.expansion` must be visible from `tests.rs` — satisfied (`pub(crate)`).
2. `expansion.object_phases` must be visible from `tests.rs` — `pub(super)` on `ExpansionRegistry`
   means "visible to the parent module of `expansion`", which is `stack_view`. Since `tests` is a
   submodule of `stack_view`, `pub(super)` fields are NOT visible to `tests` from that angle —
   `pub(super)` on a field of a struct defined in `expansion.rs` means visible to `expansion`'s
   parent (`stack_view`), not to `stack_view`'s children.

In Rust, `pub(super)` visibility on a struct field restricts access to the module containing the
struct definition's parent — i.e., `expansion`'s parent is `stack_view`. A child module of
`stack_view` (namely `tests`) does NOT automatically get `pub(super)` access; that access flows
upward, not downward. The test at line 555 (`state.expansion.object_phases.is_empty()`) will likely
fail to compile unless the fields are elevated to `pub(crate)` or the `tests` module uses
`use super::expansion::ExpansionRegistry` and relies on `pub(super)` correctly flowing. This is a
subtle Rust visibility nuance the spec glosses over with the phrase "tests.rs y accède aussi car il
est dans le même module stack_view" — that claim is imprecise.

**Concrete recommendation:** The spec must clarify whether `ExpansionRegistry`'s fields need
`pub(crate)` (accessible anywhere in the crate) to satisfy both `tests.rs` and `favorites.rs` test
access, or whether a dedicated accessor method is needed.

---

### F20 — T5 count discrepancy: spec says 11 occurrences of `s.collection_chunks` but grep shows 11 — however the `s.` prefix assumption may be wrong

**Severity:** Low
**Validity:** Undecided

The spec states "11 accès directs à `s.collection_chunks`" in `app/mod.rs`. The actual grep
(`grep -n 'collection_chunks' app/mod.rs`) confirms exactly 11 occurrences all prefixed by `s.`.
This is accurate. However, the spec uses `s.collection_chunks` as the search pattern in the
verification step:

> `grep -n 'collection_chunks' app/mod.rs`

This grep pattern would also match variable names, comments, or struct field names containing
`collection_chunks` if any were added. The verification is not tight enough: it should be
`s\.collection_chunks` (with escaped dot) to confirm zero remaining direct accesses through
the `StackState` binding. As written, AC8 (`grep -n 's.collection_chunks'`) uses an unescaped dot,
which in shell grep matches any character between `s` and `collection_chunks`. Minor but a
misleading verification command.

---

### F21 — `collapse_object` on `ExpansionRegistry` does not clear `collection_chunks` — undocumented data leak

**Severity:** Medium
**Validity:** Real

Looking at the current `collapse_object` implementation:

```rust
pub fn collapse_object(&mut self, object_id: u64) {
    self.object_phases.remove(&object_id);
    self.object_fields.remove(&object_id);
    self.object_errors.remove(&object_id);
}
```

It does NOT remove from `collection_chunks`. This is existing behavior and the spec explicitly says
"Tout changement de comportement ou de logique" is out of scope. However, after migration, this
method lives on `ExpansionRegistry` which owns all four maps. The spec notes in the method list that
`collapse_object` is one of the methods to migrate but does not call attention to the fact that
`collection_chunks` is an owned field of `ExpansionRegistry` yet is NOT cleared by `collapse_object`.

A developer reading only the spec and implementing `collapse_object` on `ExpansionRegistry` from the
description "collapses an expanded object" would likely add
`self.collection_chunks.remove(&object_id)` — a behavior change that could mask existing bugs or
break the `cursor_collection_id` + collection removal flow in `app/mod.rs` (which manages
`collection_chunks` cleanup explicitly). The spec should explicitly document that `collapse_object`
on `ExpansionRegistry` intentionally does NOT touch `collection_chunks`, because `app/mod.rs`
manages collection lifecycle independently.

---

### F22 — No mention of updating `mod.rs` re-export list when `format_entry_line` moves

**Severity:** Low
**Validity:** Real

`format_entry_line` is currently `pub(crate)` on `StackState`. It is not re-exported from `mod.rs`.
After moving to `format.rs` as `pub(super)`, the function is no longer accessible from `state.rs`
via its current path. The spec correctly handles the call-site update in T2b. However, T4 says
"Ajouter `mod expansion;`" as the only change to `mod.rs`. If the implementer checks whether
`format_entry_line` should be added to `mod.rs`'s `pub(crate) use format::{...}` list, they will
find it is not there currently and correctly won't add it — but the spec never explicitly says
"do NOT add `format_entry_line` to the `mod.rs` re-export list". Given that `format.rs` already has
a lengthy re-export list in `mod.rs`, an implementer might auto-include it. One sentence in T4
would close this gap.
