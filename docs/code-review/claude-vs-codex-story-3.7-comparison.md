# Review Comparison — Story 3.7: Claude vs Codex

**Date:** 2026-03-08
**Story:** `3-7-real-thread-names-and-local-variable-resolution`

## Overview

| Aspect | Codex | Claude |
|---|---|---|
| **Outcome** | Changes Requested | No critical blocker |
| **Issues** | 1 HIGH, 2 MEDIUM, 2 LOW | 0 CRITICAL, 3 MEDIUM, 3 LOW |

## Common Finding

| Finding | Codex | Claude |
|---|---|---|
| `ROOT_THREAD_OBJ` silent on truncation | M3 (MEDIUM) | M3 (MEDIUM) |

## Codex-Only Findings

| # | Sev. | Finding | Valid? |
|---|---|---|---|
| C-1 | HIGH | Task 5 marked `[x]` but 5.4/5.5 unchecked | Yes — Claude noted AC7 PENDING in matrix but did not escalate as finding |
| C-2 | MEDIUM | Metadata inconsistent (changelog says "all 5 tasks complete" but e2e pending) | Yes — logical complement of C-1 |
| C-4 | LOW | No validation that object is actually a Thread before reading "name" field | Yes — defensive finding Claude missed |
| C-5 | LOW | Unicode arrows vs UX spec ASCII-only guidance | Yes — pre-existing issue (story 3.6) correctly detected |

## Claude-Only Findings

| # | Sev. | Finding | Valid? |
|---|---|---|---|
| M1 | MEDIUM | Module doc `precise.rs` stale (says "five HashMaps", struct has 9) | Yes — concrete doc debt |
| M2 | MEDIUM | `decode_fields` parses all Thread fields just to find "name" (perf) | Yes — architecture/perf finding |
| L1 | LOW | Test `new_creates_empty_index` misses `java_frame_roots` | Yes — test coverage gap |
| L2 | LOW | Fragile coupling ObjectRef vs StringRef in `resolve_thread_name_from_heap` | Yes — regression anticipation |
| L3 | LOW | Fully-qualified vs short class name in local var display | Debatable — AC wording is ambiguous |

## Verdict

- **Codex stronger on process:** correctly identified Task 5 falsely marked
  complete (HIGH) and metadata inconsistency. Most important finding of the
  review.
- **Claude stronger on architecture:** stale docs, `decode_fields` perf,
  fragile coupling, test coverage gap.
- **Overlap:** single shared finding (ROOT_THREAD_OBJ silent truncation).
- Complementary profiles: process-oriented + architecture-oriented.
