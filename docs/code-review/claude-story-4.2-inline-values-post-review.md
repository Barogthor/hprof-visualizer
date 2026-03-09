# Code Review: Inline Values & Auto-Resolve (commits 7880d9d..a627252)

## Summary

Three commits that extend the collection entry display and object expansion:

1. **7880d9d** — `expand_object` now resolves arrays (object and primitive) to
   `Object[]` / `int[]` etc. with element counts for ObjectRef fields;
   `build_collection_entry_obj_items` gains StringRef phase support and
   toggles.
2. **2f77f1d** — Collection entries now use a dedicated `format_entry_value`
   formatter; `format_entry_line` gains a value_phase toggle; `format_field_value`
   gains inline_value display; `FieldValue::ResolvedString` is handled everywhere.
3. **a627252** — Introduces `resolve_inline_value` / `truncate_inline` for
   String and boxed-primitive inline values; `id_to_field_value` (pagination)
   calls `resolve_inline_value` so collection entries also show inline values.

All 132 tests pass. Clippy emits 7 warnings, none of them bugs.

---

## Findings

### H1 — Byte-slice index can panic on non-char-boundary in `format_entry_value`

**File:** `crates/hprof-tui/src/views/stack_view.rs`, ~line 1264
**Severity:** H

```rust
FieldValue::ResolvedString(s) => {
    if s.len() > 80 {
        format!("\"{}...\"", &s[..77])   // ← byte slice, not char slice
    } else {
        format!("\"{s}\"")
    }
}
```

`s.len()` is a byte count; `&s[..77]` is a byte-index slice.
If the `ResolvedString` contains any multi-byte UTF-8 character whose
boundary does not fall at byte 77, this panics at runtime with
`byte index 77 is not a char boundary`.

The same value originates from `decode_prim_array_as_string`, which can
return any valid Rust `String` including strings containing multi-byte
characters (e.g. Latin-1 bytes above 0x7F decoded as `char` cast to
`String`, or full UTF-16 content via `from_utf16_lossy`).

Contrast with `truncate_string_display` (line 1169), which correctly uses
`char_indices().nth(MAX_STRING_DISPLAY)` to find a safe boundary.
`format_entry_value` uses a different, unsafe approach.

**Fix:** replace `s.len() > 80` / `&s[..77]` with the same char-count
logic already used in `truncate_string_display`.

---

### H2 — `truncate_inline` slices by bytes, not chars

**File:** `crates/hprof-engine/src/engine_impl.rs`, lines 220–226
**Severity:** H

```rust
fn truncate_inline(s: String) -> String {
    if s.len() <= 80 {
        s
    } else {
        format!("{}..", &s[..78])   // ← byte slice
    }
}
```

Same root cause as H1. `s.len()` is bytes; `&s[..78]` panics if byte 78
is not a char boundary. The inline value is directly derived from heap
string content, so multi-byte sequences are realistic.

**Fix:** use `s.chars().count()` for the length check and
`s.char_indices().nth(78).map(|(i, _)| i).unwrap_or(s.len())` for the
safe byte index.

---

### H3 — `expand_object` uses `find_instance` for the root but offset-lookup for children; inconsistency causes enrichment miss

**File:** `crates/hprof-engine/src/engine_impl.rs`, lines 679–740
**Severity:** H (latent; observable when the root object has an indexed offset)

```rust
fn expand_object(&self, object_id: u64) -> Option<Vec<FieldInfo>> {
    let raw = self.hfile.find_instance(object_id)?;   // linear scan
    // ...
    if let Some(child_raw) = self.hfile.find_instance(child_id) {  // also linear scan
```

For the root, `expand_object` calls `self.hfile.find_instance` (linear
scan that ignores the fast-path index), whereas the private helper
`read_instance` checks `instance_offsets` first.  This is inconsistent:
if the index is populated, the root scan is unnecessarily slow and the
child scan also bypasses the fast path.

While not a correctness bug per se, it means `expand_object` always does
linear scans even though `read_instance` with the offset map exists
specifically to avoid that. If the file is large this is a latent
performance regression for every object expansion.

**Fix:** change the root lookup to use `Self::read_instance(&self.hfile, object_id)`.

---

### M1 — `resolve_inline_value` called twice for the same object in `expand_object`

**File:** `crates/hprof-engine/src/engine_impl.rs`, lines 698–716
**Severity:** M (unnecessary I/O per expanded field)

Inside `expand_object`, for every ObjectRef field that is a `String` or
boxed type, `resolve_inline_value` is called. `resolve_inline_value` for
`java.lang.String` calls `resolve_string_static`, which calls
`read_instance` + `decode_fields` + `find_prim_array`. The same child
instance has already been fetched on line 699 (`find_instance`) and its
fields decoded in `resolve_string_static`. This means two full field
decode passes per String child field.

The impact is bounded (only String/boxed children), but on objects with
many String fields this doubles the parse work per `expand_object` call.

**Fix:** pass the already-decoded fields into `resolve_inline_value`, or
cache the raw/fields for the child when both `class_name` and
`inline_value` need to be resolved.

---

### M2 — `StringRef` variant is produced by the parser but never stored by `expand_object` or `pagination`

**File:** `crates/hprof-engine/src/engine.rs` (type definition), `engine_impl.rs`, `pagination.rs`
**Severity:** M (dead variant, semantic confusion)

`FieldValue::StringRef { id }` exists in the `FieldValue` enum and is
documented as the variant used for `java.lang.String` fields. However,
`expand_object` (lines 690–738) now produces `ObjectRef { class_name:
"java.lang.String", inline_value: Some(...) }` instead. The `StringRef`
variant is only produced by `decode_fields` indirectly… except it is not
— `decode_fields` always returns `ObjectRef` for type-code 2, regardless
of whether the referenced object is a String. The `StringRef` variant is
only ever set in tests directly.

This creates two issues:
- The docstring of `FieldValue::StringRef` says it is used by
  `expand_object`, which is false.
- `App::handle_stack_frames_input` calls `selected_field_string_id()` to
  check for `StringRef` fields; since `expand_object` never produces
  `StringRef`, the manual string-load path (`LoadString` command) for
  regular object fields is dead code for the current engine.

**Recommendation:** either remove `StringRef` and unify with
`ObjectRef { inline_value }`, or actually produce `StringRef` in
`expand_object` when the class is `java.lang.String` and document the
distinction clearly.

---

### M3 — No tests for `resolve_inline_value` in isolation

**File:** `crates/hprof-engine/src/engine_impl.rs`
**Severity:** M

`resolve_inline_value` is `pub(crate)` and covers non-trivial logic
(String lookup, per-boxed-type dispatch). There are no unit tests that
directly call it with a controlled `HprofFile`. Existing tests exercise it
indirectly only via `expand_object` for the String case (and only with a
missing backing array that returns `None`). The following cases have
zero coverage:

- `java.lang.Integer` with a known `value` field → returns `"42"`.
- `java.lang.Boolean` with `value = true` → returns `"true"`.
- `java.lang.Character` with `value = 'A'` → returns `"'A'"`.
- `java.lang.Double` / `java.lang.Float` with edge values (NaN, ∞) —
  format output is unspecified.
- A class whose name matches `BOXED_TYPES` but has no `value` field → returns `None`.
- A class that is not String and not in `BOXED_TYPES` → returns `None` fast.

**Fix:** add a `mod resolve_inline_value_tests` in `engine_impl.rs` using
`HprofTestBuilder` to cover the above cases.

---

### M4 — No tests for `truncate_inline` / `truncate_string_display` with multi-byte input

**File:** `crates/hprof-engine/src/engine_impl.rs` and `crates/hprof-tui/src/views/stack_view.rs`
**Severity:** M (directly related to H1/H2)

There are no tests passing strings with multi-byte UTF-8 characters to the
truncation helpers. Until H1 and H2 are fixed, adding such tests would
demonstrate the panic. Even after a fix they are needed as regression
guards.

---

### M5 — `format_entry_value` for `StringRef` always shows `"\"...\""` regardless of load phase

**File:** `crates/hprof-tui/src/views/stack_view.rs`, line 1261
**Severity:** M

```rust
FieldValue::StringRef { .. } => "\"...\"".to_string(),
```

`format_field_value` (used in regular object field rows) correctly accepts
a `string_phase` parameter and shows `"~"` / resolved value / `<unresolved>`.
`format_entry_value` (used in collection entry rows including keys) ignores
the phase entirely and always shows `"..."`.

Since collection entry keys can be Strings (e.g. `HashMap<String, V>`),
the key column will never display the resolved string, making key-based
disambiguation impossible. This is a UX regression compared to field rows.

**Fix:** pass `(StringPhase, Option<&str>)` into `format_entry_value` and
apply the same logic as `format_field_value`.

---

### M6 — `id_to_field_value` in pagination calls `resolve_inline_value` for every element, including non-String/boxed objects

**File:** `crates/hprof-engine/src/pagination.rs`, lines 499–527
**Severity:** M (performance)

```rust
fn id_to_field_value(id: u64, hfile: &HprofFile) -> FieldValue {
    // ...
    let inline_value = crate::engine_impl::resolve_inline_value(
        &class_name, hfile, id,
    );
```

`resolve_inline_value` starts by checking `class_name == "java.lang.String"`
and `BOXED_TYPES.contains(class_name)`. For all other classes it returns
`None` immediately. However, for those other classes it still pays the
`BOXED_TYPES.contains` linear scan on every element of every
Object[] array page. For large arrays (up to 1000 entries per chunk)
this is 1000 × O(8) string comparisons on the hot path.

The guard is inexpensive but was not present before this commit, so it is
new overhead that was not benchmarked. Since `resolve_inline_value` is
called from within the pagination worker thread, the cost is absorbed, but
it should be documented or the guard should be made explicit.

**Recommendation:** add a short-circuit at the call site:
```rust
let inline_value = if matches!(class_name.as_str(),
    "java.lang.String" | "java.lang.Boolean" | "java.lang.Byte"
    | "java.lang.Short" | "java.lang.Integer" | "java.lang.Long"
    | "java.lang.Float" | "java.lang.Double" | "java.lang.Character")
{
    resolve_inline_value(&class_name, hfile, id)
} else {
    None
};
```

---

### M7 — `emit_collection_entry_obj_children` does not emit a cursor for cyclic fields; rendering skips them silently

**File:** `crates/hprof-tui/src/views/stack_view.rs`, lines 1114–1119
**Severity:** M (navigation inconsistency)

In `emit_collection_entry_obj_children`, cyclic ObjectRef fields are
silently dropped:
```rust
if let FieldValue::ObjectRef { id, .. } = field.value
    && visited.contains(&id)
{
    // Cyclic — emit as non-navigable leaf (no cursor)
    continue;
}
```

But `build_collection_entry_obj_items` renders such fields as a regular row
with the value formatted via `format_field_value`. This divergence means
the rendered row has no corresponding `StackCursor`, so the item appears in
the UI but can never receive keyboard focus. The user sees a line they
cannot navigate to.

In the top-level object tree (`emit_object_children`), cycles correctly
emit an `OnCyclicNode` cursor. The collection-entry path should do the
same.

**Fix:** emit `OnCollectionEntryObjField` with the cycle's `child_path` and
render it with a cyclic marker in `build_collection_entry_obj_items`,
mirroring the top-level approach.

---

### M8 — `CollectionEntryObjField` string load path ignores `Loading` phase for entry key

**File:** `crates/hprof-tui/src/app.rs`, lines 375–388
**Severity:** M

When cursor is `OnCollectionEntryObjField`, `App` checks
`selected_collection_entry_obj_field_string_id()` which returns `Some`
only when phase is `Unloaded` or `Failed`. This correctly prevents
re-loading. However, if the user presses Enter while phase is `Loading`,
the code falls through to `selected_collection_entry_obj_field_ref_id()`,
which returns `None` for a `StringRef` field (it matches only
`ObjectRef`). The result is `None` and no command is issued — silent.
This is acceptable behaviour but is not documented or tested.

---

### L1 — Clippy warnings left unaddressed (7 total)

**Severity:** L

- `pagination.rs:70-78`: `match` with single arm should be `if let` (clippy::single_match).
- `pagination.rs:71`: `page` unused inside the `match` arm.
- `stack_view.rs:68-72`, `74-78`: collapsible nested `if let` (clippy::collapsible_if).
- `stack_view.rs:1069`: function `emit_collection_entry_obj_children` has 10 arguments (>7, clippy::too_many_arguments).
- `stack_view.rs:1408-1410`: `fi`, `vi`, `field_path` are "only used in recursion" (clippy::only_used_in_recursion).
- `stack_view.rs:1544`: collapsible `if` with `!cycle`.

None are bugs, but the `too_many_arguments` warning (L1e) is a code-smell
signal for a function that would benefit from a context struct.

---

### L2 — `format_entry_value` and `format_field_value` duplicate `ObjectRef` formatting logic

**File:** `crates/hprof-tui/src/views/stack_view.rs`, lines 1207–1235 and 1269–1292
**Severity:** L

Both functions independently compute:
- `display_name = if class_name.is_empty() { "Object" } else { class_name }`
- `short = display_name.rsplit('.').next().unwrap_or(display_name)`
- `base = match entry_count { Some(n) => ..., None => ... }`
- `match inline_value { Some(v) => format!("{base} = {v}"), None => base }`

This is verbatim duplication. A private helper extracting the common logic
would reduce future maintenance surface.

---

### L3 — `FieldValue::ResolvedString` is never produced by the engine

**File:** `crates/hprof-engine/src/engine.rs`, line 132
**Severity:** L

`ResolvedString(String)` is defined in the engine's public API and handled
in the TUI formatters, but neither `expand_object` nor `get_page` ever
produces it. It appears to have been introduced as a future extension point
or was produced in an earlier iteration and replaced by `inline_value` on
`ObjectRef`. If it is no longer needed, it should be removed to reduce the
variant count. If it is still planned, a `// TODO` comment explaining its
intended use is required.

---

### L4 — `expand_object_string_field_keeps_object_ref_with_inline` test name is misleading

**File:** `crates/hprof-engine/src/engine_impl.rs`, line 1538
**Severity:** L

The test is named `…keeps_object_ref_with_inline` but the comment says
`// No backing array → inline_value is None`. The test actually verifies
that a String field with a missing backing array produces
`ObjectRef { inline_value: None }`, not `ObjectRef { inline_value: Some(...) }`.
The `_with_inline` suffix implies an inline value is present, which is the
opposite of what is tested. Rename to
`expand_object_string_field_without_backing_array_has_no_inline`.

---

### L5 — `BOXED_TYPES.contains` uses linear search on a static slice

**File:** `crates/hprof-engine/src/engine_impl.rs`, line 194
**Severity:** L (trivial, 8 elements)

```rust
if !BOXED_TYPES.contains(&class_name) {
```

With only 8 entries the cost is negligible, but a `match` statement is
cleaner, type-checked at compile time, and removes the string heap
comparison overhead for the common case (most objects are not boxed types).

---

## Verdict

**Ship with blocking fixes:** H1 and H2 are real panic risks on any heap
dump containing multi-byte characters in String fields or boxed values with
non-ASCII content. Both are easy one-line fixes.

H3 is a consistency bug in `expand_object` that causes unnecessary linear
scans; it should be fixed in the same pass.

M3 and M4 are the most important test gaps: the new `resolve_inline_value`
function has essentially no isolation tests, and the truncation helpers are
unguarded against the inputs that will cause the H1/H2 panics.

M5 (StringRef in entries never resolved), M7 (cyclic entry-obj fields
missing cursor), and L3 (dead `ResolvedString` variant) are the most
significant design loose-ends and should be resolved before the feature is
considered complete.

The rest (M6, M8, L1–L5) are polish items that can follow in a cleanup
commit.
