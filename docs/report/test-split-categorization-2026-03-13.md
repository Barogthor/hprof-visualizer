# Test File Split — Categorization Report

**Date:** 2026-03-13
**Scope:** `engine_impl/tests.rs`, `first_pass/tests.rs`, `stack_view/tests.rs`

---

## Summary

Three test files have grown into mega-files. This report identifies natural
categorization patterns for splitting them into focused modules or separate files.

| File | Lines | Existing `mod` blocks | Split complexity |
|---|---|---|---|
| `crates/hprof-engine/src/engine_impl/tests.rs` | ~2 070 | 10 | Low — mods already exist |
| `crates/hprof-parser/src/indexer/first_pass/tests.rs` | ~1 560 | 1 | Medium — needs mods first |
| `crates/hprof-tui/src/views/stack_view/tests.rs` | ~3 080 | 0 | High — shared helpers |

---

## `engine_impl/tests.rs`

The file is already well-structured with ten internal `mod` blocks. Splitting is
mechanical: move each `mod` into a file under a `tests/` sub-directory.

| Target file | Content | Source |
|---|---|---|
| `file_loading_tests.rs` | `memory_used_*`, `indexing_ratio_*`, `is_fully_indexed_*`, `skeleton_bytes_*`, `from_file_*`, progress observer | Top-level tests (lines 1–270) |
| `stack_frame_tests.rs` | `get_stack_frames_*`, `get_local_variables_*`, `list_threads` (frame level) | `mod stack_frame_tests` |
| `string_tests.rs` | `decode_prim_array_*`, `resolve_string_*`, `truncate_inline_*`, `resolve_inline_value_*` | 4 mergeable mods |
| `collection_tests.rs` | `hashmap_*`, `arraylist_*`, collection detection, `linked_hashmap_*` | `mod collection_tests` |
| `object_expansion_tests.rs` | `expand_object_*`, `class_of_object_*`, `get_static_fields_*` | `mod expand_object_tests` + `mod static_fields_tests` |
| `thread_tests.rs` | `list_threads` builders, thread state mapping, `real_dump_thread_states`, `memory_budget_*` | `mod builder_tests` + `mod thread_state_mapping` + top-level |
| `lru_cache_tests.rs` | `expand_object_cached_*`, eviction order, `re_parse_after_eviction`, counter overflow | `mod lru_eviction_tests` |

**Recommendation:** create `crates/hprof-engine/src/engine_impl/tests/` and move each
block into its own file. The parent `tests.rs` becomes a thin re-export of sub-modules.

---

## `first_pass/tests.rs`

Only one `mod builder_tests` exists. The remaining ~930 top-level test functions fall
into five themes.

| Target file | Content |
|---|---|
| `progress_tests.rs` | `progress_observer_called_*`, `scan_phase_reports_monotonic_bytes`, `progress_observer_reports_partial_*` |
| `record_parsing_tests.rs` | `single_string_record`, `single_load_class`, `single_start_thread`, `single_stack_*`, `unknown_tag_skipped`, `three_string_records`, `id_size_4_*`, `zgc_high_bit_ids_*`, `load_class_populates_*`, `load_class_with_unknown_*` |
| `error_handling_tests.rs` | `eof_mid_header_*`, `payload_end_exceeds_*`, `corrupted_payload_*`, `two_records_first_corrupt_*`, `too_short_declared_length_*`, `extra_payload_bytes_*`, `start_thread_with_extra_bytes_*`, `string_declared_length_smaller_*` |
| `heap_parsing_tests.rs` | `heap_dump_0x0c_record_*`, `truncated_heap_dump_segment_*`, `gc_root_before_instance_dump_*`, `truncated_gc_root_*` |
| `thread_resolution_tests.rs` | `stack_trace_without_start_thread_synthesises_*`, `stack_trace_thread_serial_zero_*`, `start_thread_record_takes_priority_*`, `lookup_offset_*`, `scan_records_*` |
| `builder_tests.rs` | Existing `mod builder_tests` — heap segments, class dumps, subdivide, parallel/sequential paths |

**Recommendation:** introduce `mod` blocks in-place first (zero test changes, easy
review), then extract to files in a second pass.

---

## `stack_view/tests.rs`

The largest file with zero existing `mod` blocks. All tests are flat. The main
complication is that ~230 lines of helper functions (`make_frame`, `rc_*`, `path_*`,
`make_var_*`) are used across every category.

### Shared helpers (must be addressed first)

```
make_frame, make_var, make_var_object_ref
rc_frame, rc_var, rc_field, rc_static_field, rc_loading, rc_cyclic,
rc_section_header, rc_overflow, rc_coll_entry, rc_coll_entry_field,
rc_chunk_section, rc_static_obj_field, rc_coll_entry_static_field,
rc_coll_entry_static_obj_field
path_field, path_coll_entry
item_text, rendered_fg_at, nav_hash
```

Two options:
- **Option A:** extract to a `test_helpers.rs` (or `tests/helpers.rs`) and `pub use`
  from each sub-module.
- **Option B:** duplicate locally the small subset each module actually needs — simpler
  but more verbose.

### Proposed split

| Target file | Content | Lines (approx.) |
|---|---|---|
| `cursor_tests.rs` | Basic frame/var navigation, `toggle_expand`, `selected_frame_id`, `format_frame_label` | ~258–445 |
| `expansion_lifecycle_tests.rs` | `set_expansion_loading/done/failed`, `cancel_expansion`, cursor recovery on failure | ~369–505 |
| `flat_items_tests.rs` | `flat_items_loading_*`, `flat_items_expanded_*`, `flat_items_depth2_*`, length invariant | ~454–1660 |
| `cycle_tests.rs` | `flat_items_self_ref_*`, `flat_items_indirect_cycle_*`, `flat_items_acyclic_*`, `flat_items_diamond_*`, `collapse_object_recursive_*`, `collapse_cyclic_child_*` | ~779–1308 |
| `collection_tests.rs` | `flat_items_collection_entry_*`, `selected_collection_entry_*`, `right_on_collection_var`, `cursor_collection_id`, `left_from_collection_entry`, `left_on_collection_entry_*` | ~1014–2205 (dispersed) |
| `scroll_tests.rs` | `scroll_view_down/up/page_*`, `center_view_on_selection_*` | ~2371–2575 |
| `static_fields_rendering_tests.rs` | `render_static_section_*`, `render_collection_entry_*`, `render_static_overflow_row`, `static_object_field_rows_*`, `collection_entry_static_object_field_*` | ~2576–2960 |
| `navigation_path_tests.rs` | `nav_path_*`, `parent_cursor_on_*`, `left_on_*`, `right_on_*`, `expansion_at_path_*` | ~2062–3073 (dispersed) |
| `value_style_tests.rs` | `value_style_null/int/bool/char/object_ref_*` | ~1410–1460 |
| `chunk_ranges_tests.rs` | `chunk_ranges_total_*` | ~1308–1362 |

**Recommendation:** tackle helpers first (Option A), then split in thematic batches
starting with the most self-contained groups (`chunk_ranges`, `value_style`, `scroll`).

---

## General Approach

1. **engine_impl** — lowest risk, split directly (mods already in place).
2. **first_pass** — introduce `mod` blocks as a preparatory commit, then extract.
3. **stack_view** — most effort; helpers extraction is a prerequisite.

All splits should be pure moves with no logic changes to keep diffs reviewable.
