# Party Mode Report: Story 3.7 Discovery Session

**Date:** 2026-03-07
**Participants:** John (PM), Winston (Architect), Amelia (Dev), Mary (Analyst), Bob (SM), Quinn (QA)

## Context

Florian reported two issues with real heap dumps (jvisualvm / jmap):
1. Thread names show defaults (`Thread-{serial}`) instead of real names
2. No local variables found at the stack trace level

## Diagnostic: Heap Dump Sub-Record Scan

Ran a Python scanner on both dumps in `assets/`:

### `heapdump-visualvm.hprof`
- `ROOT_THREAD_OBJ`: **32** (thread_serial -> thread object in heap)
- `ROOT_JAVA_FRAME`: **242** (local variable references per stack frame)
- 214K instances, 7K classes

### `heapdump-rustrover.hprof`
- `ROOT_THREAD_OBJ`: **88**
- `ROOT_JAVA_FRAME`: **1,123**
- 9.2M instances, 103K classes

**Conclusion:** Both sub-record types are present in both dumps. The data exists — we just don't fully exploit it yet.

## Root Cause Analysis

### Local Variables Bug (critical)

The correlation loop in `first_pass.rs:408-430` maps `ROOT_JAVA_FRAME` entries to `java_frame_roots` via `threads`. But for jvisualvm dumps (no `START_THREAD` records), thread synthesis from `STACK_TRACE` happens **after** the correlation loop (line 432+). Result: `threads` is empty during correlation, all 242 roots are silently dropped, `java_frame_roots` stays empty.

**Fix:** Move thread synthesis block (lines 432-455) **before** the frame root correlation block (lines 408-430).

### Thread Names (missing feature)

`ROOT_THREAD_OBJ` (sub-tag 0x08) is not parsed — it's skipped by `skip_sub_record`. This sub-record maps `thread_serial -> thread_object_id` in the heap. To get real names:
1. Parse `ROOT_THREAD_OBJ` -> map `thread_serial -> object_id`
2. `find_instance(object_id)` -> java.lang.Thread instance
3. Read `name` field -> resolve via `resolve_string()`

### Local Variable Display (missing enrichment)

Current display: `local_0 ObjectRef(0xABC)`. VisualVM shows: `local variable - java.lang.ref.NativeReferenceQueue#1 [GC root - Java frame]`.

The class name can be resolved via `find_instance(object_id)` + `class_names_by_id` — plumbing already exists.

## VisualVM Reference (screenshot-01.png)

What VisualVM actually shows for local variables:
- **No real variable names** — displays generic "local variable"
- Shows resolved class type + instance number (`NativeReferenceQueue#1`)
- `[GC root - Java frame]` tag
- On expansion: `<fields>` section with field names from `CLASS_DUMP` (lock, head, queueLength...)
- `<references>` section (retention graph — out of scope)
- Size in bytes, retained % (out of scope)

## Story 3.7 Scope: Real Thread Names & Local Variable Resolution

| # | Task | Effort | Notes |
|---|------|--------|-------|
| 1 | Bug fix: reorder thread synthesis before frame root correlation | Small | Move code block, fixes 242 missing locals |
| 2 | Parse `ROOT_THREAD_OBJ` (sub-tag 0x08) in first_pass | Small | Same pattern as other sub-records |
| 3 | Resolve real thread names via java.lang.Thread instance | Moderate | find_instance + resolve_string exist |
| 4 | Display resolved class type on local variables | Small | find_instance + class_names_by_id exist |
| 5 | E2E validation on both real dumps | Small | Assets already available |

## Key Decisions

- Stays within Epic 3 (not a new epic) — the AC of stories 3.3/3.4 aren't truly met without this
- Single story (3.7) — scope is cohesive and plumbing mostly exists
- `<references>` section (retention graph) and size/retained% are out of scope for Epic 3
