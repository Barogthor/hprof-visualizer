# Adversarial Review — Tech-Spec: Refactor StackState / Extract ExpansionRegistry

**Reviewed:** 2026-03-11
**Spec file:** `docs/implementation-artifacts/tech-spec-refactor-stack-state-expansion-registry.md`
**Reviewer:** Claude (adversarial pre-mortem)

---

## Findings

### F1
**Severity:** Critical
**Validity:** Real
**Description:** `object_fields` is currently `pub(crate)` on `StackState` (line 29 of
`state.rs`), not `pub(super)`. The spec lists it as one of the four fields moving into
`ExpansionRegistry` with visibility `pub(super)`. However `favorites.rs` accesses it via
the `object_fields()` getter method — that getter already wraps the field, so
`favorites.rs` itself does not break. The real problem is subtler: the spec never
acknowledges the asymmetric visibility — `object_fields` is `pub(crate)` today, while
`object_errors` is `pub(super)`. After the move, the spec sets ALL four to `pub(super)`.
Silently downgrading `object_fields` visibility inside `ExpansionRegistry` is correct only
if all external call-sites go through the delegating getter on `StackState`. The spec does
NOT list or verify `favorites.rs` as a call-site to audit. If any caller in a future story
tries to access `state.object_fields` directly (not via the getter), it will silently fail
to compile with a confusing error pointing at `ExpansionRegistry`. The spec must explicitly
enumerate every pub(crate) access path and confirm each one is covered by the wrapper getter.

---

### F2
**Severity:** Critical
**Validity:** Real
**Description:** `collect_descendants` (in `format.rs`) is called inside
`collapse_object_recursive` as:
```rust
collect_descendants(object_id, &self.object_fields, &mut visited, &mut to_remove);
```
After the refactor, `self.object_fields` no longer exists on `StackState`; the call site
must become `&self.expansion.object_fields`. The spec lists `collapse_object_recursive` as
staying on `StackState` (correct) and lists `resolve_object_at_path`,
`emit_object_children`, etc. in the "methods to adapt" list — but `collapse_object_recursive`
itself is NOT on that list. This means T2b instructions are incomplete: a developer
following the spec to the letter will miss this substitution and hit a compilation error
with no guidance on why.

---

### F3
**Severity:** High
**Validity:** Real
**Description:** The spec states `expansion_state` on `ExpansionRegistry` is `pub(super)`
and that `StackState::expansion_state` delegates to it. But `emit_object_children` and
`emit_collection_entry_obj_children` (which stay on `StackState`) call
`self.expansion_state(...)` — which is fine through the delegation chain. However, the spec
also says in Technical Decisions: "Rust ne requiert pas la même visibilité" without
spelling out the actual visibility chain. The real issue: `ExpansionRegistry::expansion_state`
is `pub(super)`, meaning only code in `stack_view/` can call it. Code in `expansion.rs`
calling a `pub(super)` fn on the same struct is fine (same parent module). But if someone
later moves a caller outside `stack_view/`, the error message will be confusing. More
importantly for implementation: the spec does not explicitly state the signature of
`ExpansionRegistry::expansion_state`. Given that `StackState::expansion_state` is `pub`
(no restriction), and the delegated method is `pub(super)`, the implementer must remember
to write TWO `expansion_state` functions with different visibility — one on each struct.
The spec only mentions this in passing; it's a non-obvious inversion that will cause
confusion.

---

### F4
**Severity:** High
**Validity:** Real
**Description:** The `set_expansion_failed` split logic described in the spec (Technical
Decisions + T2b) is subtly wrong in one case. The current implementation checks
`self.flat_index().is_none()` AFTER inserting `Failed` into `object_phases`, which makes
the failed node disappear from the flat list (correct behavior). The spec correctly
preserves this order. However, the spec also says `set_expansion_failed` on
`ExpansionRegistry` should contain only the mutation logic "sans logique curseur". That
means `ExpansionRegistry::set_expansion_failed` inserts into `object_errors` and sets
`object_phases` to `Failed`. The `StackState` wrapper then calls:
1. `self.expansion.set_expansion_failed(object_id, error)`
2. `if self.flat_index().is_none() ...`

The current code also calls `self.sync_list_state()` at the end. The spec does NOT mention
that `sync_list_state()` must be kept in the `StackState` wrapper of `set_expansion_failed`.
If the implementer forgets it, the `list_state` desync is a silent bug (cursor visually
jumps but the ratatui highlight stays at the old position).

---

### F5
**Severity:** High
**Validity:** Real
**Description:** The spec claims there are 5 direct field accesses to migrate in `tests.rs`:
`state.object_phases` (×1) and `state.collection_chunks` (×4). Actual grep confirms those
counts. However, `state.object_phases` appears at line 555 inside a test function named
`toggle_expand_collapse_frame_clears_nested_object_phases`. That test directly asserts
`state.object_phases.is_empty()`. After the refactor, `object_phases` is inside
`ExpansionRegistry` which is `pub(super)` on its fields. The test is in `tests.rs` which
is a sub-module of `stack_view` — so `pub(super)` is accessible. This is fine
mechanically. The spec says so. But the spec's "5 accesses" count is wrong: T6 says
migrate `state.object_phases.is_empty()` (×1) and `state.collection_chunks.insert(...)` (×4)
= 5. The spec front-matter however says "5 accès directs aux champs à migrer" which
aligns. What is actually missing: the spec does NOT mention that `state.expansion_state(..)`
is already being called in many tests through the public API — those do NOT need migration.
But a naïve developer scanning for "things to migrate" may waste time auditing every single
field access in the 1,600-line test file. The spec should state explicitly: "only direct
field accesses, NOT method calls, need migration".

---

### F6
**Severity:** High
**Validity:** Real
**Description:** The spec says `format_entry_line` becomes `pub(super)` in `format.rs`
and "Non re-exportée dans `mod.rs`". The current `mod.rs` re-exports several functions from
`format.rs` via `pub(crate) use format::{...}`. If `format_entry_line` is added to
`format.rs` as `pub(super)` but NOT added to the re-export list in `mod.rs`, it is only
callable from within `stack_view/`. The spec claims the only caller is `state.rs`, which
delegates calls to it. But `state.rs` currently calls `format_entry_line` as a method on
`Self` (an associated function, not a free function). After moving it to `format.rs`, the
call site inside `state.rs` must change from `Self::format_entry_line(...)` to
`format::format_entry_line(...)` (or `super::format::format_entry_line(...)` depending on
imports). The spec says "Coller le corps de `format_entry_line` depuis `state.rs`" but
does NOT tell the implementer to update the call-site syntax in `state.rs`. This is an
easily missed, compilation-breaking omission.

---

### F7
**Severity:** Medium
**Validity:** Real
**Description:** The `build_items` method in `state.rs` calls `render_variable_tree` with
four separate map references:
```rust
render_variable_tree(
    TreeRoot::Frame { vars },
    &self.object_fields,
    &self.collection_chunks,
    &self.object_phases,
    &self.object_errors,
)
```
After the refactor all four maps live inside `self.expansion`. The spec lists `build_items`
in the "methods to adapt" list in T2b — correct. But it does NOT explicitly spell out
that the call to `render_variable_tree` needs to change to:
```
&self.expansion.object_fields,
&self.expansion.collection_chunks,
&self.expansion.object_phases,
&self.expansion.object_errors,
```
A developer reading the spec quickly could miss that `build_items` contains an
`render_variable_tree` call that itself directly uses the four maps. The spec says
"`build_items` passera `&self.expansion.object_fields`, etc." in the Codebase Patterns
section, which is the only place this is mentioned. It should also appear in T2b's notes
to be actionable where the developer is working.

---

### F8
**Severity:** Medium
**Validity:** Real
**Description:** The spec's line number references for `app/mod.rs` (T5 notes: lines 409,
484, 503, 538, 602, 611, 625, 734, 748, 763, 772) are point-in-time snapshots. The branch
is `feature/epic-9-navigation-data-fidelity` and has modified `app/mod.rs`. Any intervening
commits before this spec is executed will shift line numbers and make the notes misleading.
The spec should not include line numbers as authoritative guidance for a text-substitution
task. The verification grep (`grep -n 's.collection_chunks'`) is sufficient; the line
numbers add false precision and will cause wasted debugging time when they don't match.

---

### F9
**Severity:** Medium
**Validity:** Real
**Description:** The spec does not define the module-level docstring (`//!`) for
`expansion.rs`. The CLAUDE.md coding standards mandate every module has a docstring. The
spec says "Créer le fichier avec le module docstring" but provides no content. A developer
following the spec will have to invent it, potentially writing something redundant
("module for expansion") or over-long. For a spec at `ready-for-dev` status, the docstring
content should be explicitly provided the same way struct fields are provided.

---

### F10
**Severity:** Medium
**Validity:** Real
**Description:** AC9 is the only new behavioral test. Its description is:
> Given cursor `OnObjectLoadingNode { frame_idx: 0, var_idx: 0, field_path: [] }` for
> object X, When `set_expansion_failed(X, err)` is called, Then cursor is
> `OnVar { frame_idx: 0, var_idx: 0 }`.

But the current `set_expansion_failed` has a guard: `if self.flat_index().is_none()`. The
flat list only contains `OnObjectLoadingNode` when the frame is expanded and the object is
in `Loading` phase. The test setup therefore requires that:
1. A frame exists with a var pointing to object X,
2. The frame is expanded,
3. The object X is in `Loading` phase (so `OnObjectLoadingNode` appears in the flat list),
4. The cursor is set to `OnObjectLoadingNode`.

The spec does NOT describe how to set up this precondition in the test. The existing test
helpers (`make_frame`, `make_var`) do not set up expansion state. A developer must infer
this setup from scratch. For a TDD spec, the test scaffold should be described completely
— especially since the comment in the spec says "write this test FIRST before adapting
`set_expansion_failed`" (Red phase). Writing a test that cannot compile or that silently
passes due to incomplete setup is a TDD anti-pattern.

---

### F11
**Severity:** Medium
**Validity:** Real
**Description:** The spec lists `cancel_expansion` as a method to migrate to
`ExpansionRegistry`. The current `cancel_expansion` removes entries from all three maps
(`object_phases`, `object_fields`, `object_errors`). After migration, this is entirely
self-contained on `ExpansionRegistry` — correct. However, the spec lists
`cancel_expansion` in T1 (methods to migrate) but does NOT list it in T2b (methods to
delegate on `StackState`). T2b's "Méthodes à déléguer" list reads:
`expansion_state`, `set_expansion_loading`, `set_expansion_done`, `cancel_expansion`,
`collapse_object`, `chunk_state` — wait, actually `cancel_expansion` IS in the T2b list.
Re-reading carefully: it is present. However `chunk_state` is in T2b's delegation list
but NOT in T1's "Méthodes à migrer" list. T1 says: `new`, `expansion_state`,
`set_expansion_loading`, `set_expansion_done`, `set_expansion_failed` (partial),
`cancel_expansion`, `collapse_object`, `chunk_state`. Actually `chunk_state` IS in T1.
This is noise — but the inconsistency between how T1 lists `set_expansion_failed` ("sans
logique curseur") and T2b's treatment of it creates ambiguity. A developer could
reasonably ask: does T1 mean a full `set_expansion_failed` on `ExpansionRegistry`, or just
the mutation part? The spec says "sans logique curseur" in T1's note but the parenthetical
is easy to miss when scanning. This should be a bold warning or a separate bullet.

---

### F12
**Severity:** Low
**Validity:** Real
**Description:** The spec is written in French throughout (titles, method names in prose,
annotations), while the codebase itself is in English and CLAUDE.md does not specify a
language. This is not a blocking issue but creates friction: any future developer (or AI
agent) reading the spec alongside English source code must context-switch constantly.
Grep commands in the spec use English identifiers (correct), but prose explanations are
French. For a `ready-for-dev` spec this is a minor readability risk, not a correctness
issue.

---

### F13
**Severity:** Low
**Validity:** Real
**Description:** T4 says "Ajouter `mod expansion;`" to `mod.rs` with the note "Pas de
re-export de `ExpansionRegistry` — aucun usage hors du module". This is correct today.
However, `app/mod.rs` currently accesses `s.expansion.collection_chunks` directly via
`pub(crate)` on the `expansion` field of `StackState`. For that access to compile,
`ExpansionRegistry` itself (the type) must be visible to `app/mod.rs` — otherwise the
type cannot be named in a `use` statement or pattern. Since `app/mod.rs` uses the fields
directly without naming the type in a `use`, Rust will resolve it structurally. This
actually works without re-exporting the type. But if any future code ever writes
`let reg: ExpansionRegistry = ...` inside `app/`, it will fail silently. The spec should
at minimum note that `ExpansionRegistry` is intentionally not `pub(crate)` and why this is
acceptable.

---

### F14
**Severity:** Low
**Validity:** Undecided
**Description:** The spec says `format_entry_line` becomes `pub(super)` in `format.rs`.
Currently in `state.rs` it is `pub(crate)`. Downgrading visibility from `pub(crate)` to
`pub(super)` is correct if and only if no consumer outside `stack_view/` calls it. A quick
grep confirms no external usages exist today. However, `format_entry_line` formats
collection entries, and `favorites.rs` already imports several symbols from `stack_view`.
If a future feature (favorites expansion rendering, say) needs `format_entry_line`, the
developer will find a `pub(super)` function that cannot be re-exported without changing
visibility. The spec should explicitly justify the `pub(super)` choice over `pub(crate)`.

---

## Summary

| ID  | Severity | Validity   | One-line summary |
|-----|----------|------------|------------------|
| F1  | Critical | Real       | `object_fields` pub(crate)→pub(super) downgrade not fully audited across callers |
| F2  | Critical | Real       | `collapse_object_recursive` call to `&self.object_fields` not listed in T2b adapt list |
| F3  | High     | Real       | Dual `expansion_state` functions with different visibility not spelled out for implementer |
| F4  | High     | Real       | `sync_list_state()` call in `set_expansion_failed` wrapper not mentioned |
| F5  | High     | Real       | "5 accesses to migrate" count correct but spec omits clarification: method calls excluded |
| F6  | High     | Real       | `Self::format_entry_line(...)` call-site in `state.rs` must change syntax; not mentioned |
| F7  | Medium   | Real       | `render_variable_tree` 4-argument update inside `build_items` not in T2b actionable notes |
| F8  | Medium   | Real       | Line numbers in T5 are stale-prone; mislead more than they guide |
| F9  | Medium   | Real       | Module docstring content for `expansion.rs` unspecified despite coding standards requirement |
| F10 | Medium   | Real       | AC9 test preconditions (frame expanded, object in Loading phase) not described |
| F11 | Medium   | Real       | `set_expansion_failed` partial-migration intent buried; easy to implement incorrectly |
| F12 | Low      | Real       | Spec is in French while codebase is in English; readability friction |
| F13 | Low      | Real       | `ExpansionRegistry` not `pub(crate)` — implication for future direct type access undocumented |
| F14 | Low      | Undecided  | `pub(super)` on `format_entry_line` not justified against potential future consumers |
