# Adversarial Review — "Failed to resolve" for Primitive Arrays
## Scope: diff en cours (stories 9.1–9.5, uncommitted changes)
## Focus: residual "Failed to resolve" bug for Object[], int[], etc.
## Date: 2026-03-11

---

## Findings

### CRITICAL

**1. `field_byte_size` vs `prim_elem_size` — silent divergence creates a "Failed to resolve" trap**

Two separate functions compute prim array element byte sizes, with different failure behavior for
unknown element types:

- `engine_impl::field_byte_size(elem_type, id_size)` returns `0` for unrecognized type codes.
- `pagination::prim_elem_size(elem_type)` returns `None` for unrecognized type codes.

Call sites in `id_to_field_value` (pagination/mod.rs:523) and `decode_object_fields`
(engine_impl/mod.rs:637) both use `field_byte_size`. `get_local_variables` (engine_impl/mod.rs:769)
also uses `field_byte_size`. But `try_prim_array` (pagination/mod.rs:121) uses `prim_elem_size`.

**Consequence:** For an unrecognized prim array elem_type:
1. `get_local_variables` / `id_to_field_value` assign `entry_count = Some(0)` (via `field_byte_size`
   returning 0 → `if esz > 0 { } else { 0 }`).
2. TUI dispatches `StartCollection(oid, 0, cursor)`.
3. `get_page(oid, 0, 0)` is called → `try_prim_array` → `prim_elem_size(unknown)` → `None` → returns
   `None` immediately.
4. Falls through to `read_instance_public(oid)` → `None` (it is a prim array, not an instance).
5. `get_page` returns `None` → fallback: `start_object_expansion(oid)`.
6. `expand_object(oid)` → `read_instance` → `None` → returns `None`.
7. `poll_expansions` `Ok(None)` arm → `set_expansion_failed(oid, "Failed to resolve object")`.

This is a latent but deterministic "Failed to resolve" failure for any hprof file that contains a
prim array with an element type outside the set {4–11}. The two size functions must be unified; the
`entry_count` assignment in `id_to_field_value` and `get_local_variables` should use `prim_elem_size`
(or its equivalent) so that when `try_prim_array` would return `None`, `entry_count` is left `None`
rather than `Some(0)`.

---

**2. `try_prim_array` uses `prim_elem_size` (returns `None` = skip), `id_to_field_value` uses
`field_byte_size` (returns 0 = treat as empty collection) — inconsistent dispatch**

This is the same bug from a different angle. The dispatch logic in the TUI is: if
`entry_count.is_some()` → try `StartCollection`. If `entry_count` is `None` → try `StartObj`.
For an unknown prim array type, `id_to_field_value` returns `entry_count = Some(0)`, which forces
the collection path. But `get_page` via `try_prim_array` immediately rejects the ID because
`prim_elem_size` returns `None`. The mismatch means: the same array ID is classified as "a collection
with 0 entries" for dispatch, but as "not a prim array" for pagination. There is no consistent way to
recover from this without unifying the two functions.

---

### HIGH

**3. `find_object_array` allocates a full `Vec<u64>` to read only `.len()` — O(n) memory per
collection page element**

In `id_to_field_value` (pagination/mod.rs:512):

```rust
if let Some((_cid, elems)) = hfile.find_object_array(id) {
    return FieldValue::ObjectRef {
        entry_count: Some(elems.len() as u64),
        ...
    };
}
```

`find_object_array` returns the **full** element `Vec<u64>`, discarded immediately after `.len()`.
This function is called for **every element** in every loaded collection page via `id_to_field_value`
→ `paginate_id_slice`. For an `Object[][]` (an outer Object[] whose elements are inner Object[]
arrays), loading the outer page with 1000 entries requires 1000 separate `find_object_array` calls,
each allocating and immediately dropping a Vec of the inner array's element IDs. On a real heap dump
this is quadratic memory pressure.

Story 9.2 acknowledged this as acceptable, but it was not acknowledged that `id_to_field_value` is
on the hot path for every page load, not just for variable expansion. A `find_object_array_count(id)
-> Option<usize>` helper that reads only the element count without materializing the Vec is the
minimum fix.

---

**4. `get_local_variables` bypasses offset-indexed lookup for prim arrays and object arrays**

`Engine::read_prim_array(hfile, arr_id)` (engine_impl/mod.rs) attempts an offset-indexed lookup via
`hfile.index.instance_offsets` before falling back to a linear scan. But `get_local_variables`
(engine_impl/mod.rs:764–771) calls `self.hfile.find_object_array(object_id)` and
`self.hfile.find_prim_array(object_id)` directly — bypassing the fast path. This inconsistency means
that variables backed by primitive or object arrays are always resolved via O(n) linear scan, even
when offset indexing is available. The same inconsistency exists in `decode_object_fields`
(engine_impl/mod.rs:632–644).

---

**5. `selected_collection_entry_obj_field_collection_info` does not guard `id == 0`**

In state.rs:

```rust
if let FieldValue::ObjectRef {
    id,
    entry_count: Some(ec),
    ..
} = field.value
{
    return Some((id, ec));
}
```

If `id == 0` with `entry_count: Some(ec)` (a degenerate state: `id_to_field_value` guards against
this with an early return for `id == 0`, but defensive coding requires the consumer to guard too),
`StartCollection(0, ec, cursor)` is dispatched. `get_page(0, ...)` will fail all three
`try_object_array` / `try_prim_array` / `read_instance_public` checks and return `None`, triggering
the fallback `start_object_expansion(0)` → `expand_object(0)` → `None` → "Failed to resolve object".
This is a null-propagation path that survives because the null guard lives only in the producer, not
the consumer.

---

### MEDIUM

**6. `emit_collection_children` called from `flat_items` with hardcoded `field_path = &[]` — cursor
path mismatch for nested collections opened from field nodes**

In the new `flat_items` path (state.rs diff around line 649):

```rust
self.emit_collection_children(fi, vi, &[], *object_id, cc, &mut out);
```

The `field_path` is always `&[]`, regardless of whether the variable is a top-level `OnVar` or was
reached through a `OnObjectField` with a non-empty path. If a collection is opened from an object
field (not from a top-level variable), the cursor emitted by `emit_collection_children` will have
`field_path = vec![]` instead of the actual field path. This generates `StackCursor::OnVar`-like
entries instead of `OnObjectField`-like ones for what are actually field-level collection expansions,
causing `flat_items` / `build_items` cursor mismatches and potential navigation desync.

---

**7. Race condition in `collection_entry_object_field_collection_opens_without_failed_resolve` test**

```rust
let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
while !app.pending_expansions.is_empty() && std::time::Instant::now() < deadline {
    app.poll_expansions();
    std::thread::sleep(std::time::Duration::from_millis(1));
}
```

A 2-second busy-wait with 1ms sleeps is fragile on CI under load. If the worker thread is scheduled
out for more than 2 seconds (possible under heavy CI parallelism), the test exits the loop with a
non-empty `pending_expansions`, the subsequent `Down` navigation land on the wrong cursor type, and
the assertion fails with an obscure state dump. The test should use a mock/stub engine that resolves
synchronously to eliminate timing sensitivity entirely.

---

**8. `append_collection_entry_obj` — no depth limit for new nested `append_collection_items` calls**

The tree_render diff adds `append_collection_items(...)` calls inside `append_collection_entry_obj`,
which is itself called recursively. There is no depth limit applied to the new collection-items
emission path. For a deeply nested structure (e.g. `ArrayList<ArrayList<Object[]>>`), with all levels
expanded, the render loop can recurse without bound through alternating `append_collection_entry_obj`
→ `append_collection_items` → `append_collection_entry_obj` calls. The existing
`MAX_DEPTH`/`depth + 1` guard on `append_collection_entry_obj` does not protect the
`append_collection_items` insertion point inside it.

---

**9. `collapse_collection_to_parent` dual-path logic: `collection_restore_cursors` lookup has
priority over computed restore cursor, but computed path changed behavior**

Before the diff: `collapse_collection_to_parent` for `StackCursor::OnCollectionEntry` always
computed the restore cursor as `StackCursor::OnObjectField { frame_idx, var_idx, field_path }`.

After the diff: it first checks `collection_restore_cursors.get(collection_id)`. If present, it uses
that. If not, it computes: `if field_path.is_empty() { OnVar } else { OnObjectField }`.

The `if field_path.is_empty()` branch is new behavior — it was never emitted before. This means: if
a collection is opened through some path that does NOT go through the `StartCollection` cmd (e.g.,
if a collection entry was already in `collection_chunks` before the restore-cursor map was
populated), the fallback path now returns `OnVar` when `field_path.is_empty()`. Previously it
returned `OnObjectField` unconditionally. This changes existing behavior for any code path where
`collection_restore_cursors` does not have an entry, and could silently regress cursor restoration
for collections opened via `OnObjectField` with an empty field path.

---

**10. `make_var_collection_app` test helper uses hardcoded `id: 888` with `entry_count: Some(ec)` —
`StubEngine.get_page(888, ...)` behavior not verified in test body**

The new `make_var_collection_app` builder creates a var with `id: 888, class_name: "Object[]",
entry_count: Some(ec)`. The test `escape_from_collection_opened_on_var_restores_on_var_cursor` calls
`handle_input(Enter)` on this var and then `poll_all_pages`. But it does not assert that
`StubEngine.get_page(888, ...)` returns `Some(page)` — it only asserts post-escape cursor state. If
`StubEngine.get_page(888, ...)` returns `None` (because `StubEngine` has no entry for id 888 in its
page map), the fallback `start_object_expansion(888)` fires, `expand_object(888)` fails, and the
collection ends up in a `Failed` state before the escape. The test then asserts the cursor is
`OnVar`, which might accidentally pass because the collection was removed on failure — not because
the escape worked correctly. The test does not distinguish "escaped cleanly" from "collection never
opened".

---

**11. `id_to_field_value` prim-array count uses `field_byte_size(etype, hfile.header.id_size)` —
passes `id_size` for prim elem types that ignore it, but masks type-2 misclassification**

`field_byte_size(2, 8)` returns `8` (object reference size). If any element type field in a prim
array record has the value `2` (which should not appear per hprof spec but can in corrupt files),
`id_to_field_value` computes `entry_count = Some(bytes.len() / 8)` — treating the array as having
`bytes.len() / 8` elements. Then `try_prim_array` → `prim_elem_size(2)` → `None` → no page rendered
→ fallback → "Failed to resolve". The id_size parameter being threaded into `field_byte_size` for a
prim elem-type context is conceptually wrong and masks the misclassification.

---

**12. No regression test for the `OnVar → prim array → StartCollection` path in the post-9.3 diff**

Stories 9.3–9.5 add new rendering paths (collection-in-var flat_items, nested collection expansion,
restore-cursor on Escape). The 9.2 tests cover `selected_var_entry_count` and
`object_array_var_dispatches_start_collection_not_start_obj`. But no new test in the current diff
verifies the end-to-end path: `OnVar` on a prim-array var → `Enter` → `StartCollection` dispatched
→ page loaded → rendered as collection (not as "Failed to resolve"), after the new flat_items and
tree_render changes are in place. The existing 9.2 tests do not exercise the `entry_count.is_some()`
branch in the new flat_items code.
