# Code Review — Story 2.6: Indexing Progress Bar

**Date:** 2026-03-07
**Reviewer:** Amelia (Dev Agent — claude-sonnet-4-6)
**Story:** `docs/implementation-artifacts/2-6-indexing-progress-bar.md`
**Story Key:** `2-6-indexing-progress-bar`
**Status at review:** review

---

## Git vs Story File List — Discrepancies

| File | Git | Story File List |
|---|---|---|
| `Cargo.lock` | Modified | **MISSING** |
| All other listed files | Modified/New | Present |

**1 discrepancy found.**

---

## Summary

| Severity | Count |
|---|---|
| HIGH | 1 |
| MEDIUM | 3 |
| LOW | 3 |

---

## HIGH Issues

### H1 — AC1 Violated: Percentage Missing from Progress Bar Template

**File:** `crates/hprof-tui/src/progress.rs:31-36`

**AC1 requires:**
> a progress bar is displayed showing: bytes processed / total bytes, **percentage**, speed in
> GB/s, and ETA

**Actual template:**
```rust
"[{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, ETA {eta})"
```

`{percent}` or `{percent:.1}%` is entirely absent. The template shows `bytes/total_bytes` (raw
count, auto-formatted to MiB/GiB by indicatif) but no human-readable percentage such as `42.3%`.
In `indicatif` 0.17 the correct placeholder is `{percent}` (integer) or `{percent:.1}` (one
decimal). AC1 explicitly enumerates "percentage" as a required display element.

**Fix:** Add `{percent:.1}%` to the template, e.g.:
```rust
"[{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} {percent:.1}% \
 ({bytes_per_sec}, ETA {eta})"
```

---

## MEDIUM Issues

### M1 — AC4 Partially Violated: Summary Shows Count, Not Percentage

**File:** `crates/hprof-tui/src/progress.rs:58-61`

**AC4 requires:**
> the summary includes a warning about incomplete indexing with **percentage** of records
> successfully processed

**Actual output for a truncated file:**
```
Indexed 98/100 records in 2.1s (0.89 GB/s)
warning: record 0x1C payload end 74512384 exceeds file size 74507001
```

Two issues:

1. The summary line shows a raw ratio (`98/100`) not a percentage (`98.0%`). The AC explicitly
   says "percentage".
2. The printed warning comes verbatim from the parser's technical string ("record 0x1C payload
   end …"). The AC says a "warning about **incomplete indexing**" — a user-facing phrasing. The
   current warning is a low-level parser detail, not a user-friendly "Indexing was incomplete"
   message.

**Fix suggestion:**
```rust
// In finish():
let percent = if summary.records_attempted > 0 {
    summary.records_indexed as f64 / summary.records_attempted as f64 * 100.0
} else {
    100.0
};
println!(
    "Indexed {}/{} records ({:.1}%) in {:.1?} ({:.2} GB/s)",
    summary.records_indexed, summary.records_attempted, percent, elapsed, speed
);
if !summary.warnings.is_empty() {
    eprintln!(
        "warning: indexing incomplete — {:.1}% of records processed",
        percent
    );
    for w in &summary.warnings {
        eprintln!("  {w}");
    }
}
```

---

### M2 — Weak Test: Truncated-Data Progress Callback Assertion Is Trivially True

**File:** `crates/hprof-parser/src/indexer/first_pass.rs` — test
`progress_callback_reports_partial_position_for_truncated_data`

The test asserts:
```rust
assert!(
    (reported as usize) < data.len() + 1000,
    "reported position must be less than declared end"
);
```

`data.len()` is 9 (the truncated record header with no payload). `data.len() + 1000 = 1009`.
`reported` is the cursor position after the header is consumed but before the overflowing payload
window is advanced — which is 9. So `9 < 1009` is always true regardless of implementation
correctness.

The test name says "partial position for truncated data" but it never verifies that
`reported < data.len()` (which would confirm the cursor stopped before the declared payload end).
It also never checks that `reported > 0` (confirming the callback was even called with a
meaningful value).

**Fix:** Replace the weak assertion:
```rust
// cursor sits at 9 (past the 9-byte header, before declared payload of 1000 bytes)
// data.len() == 9 — cursor never advanced into payload
assert_eq!(reported, data.len() as u64,
    "cursor must be at header-consumed position (9), not beyond data");
```
Or more precisely, verify the scenario actually exercises a meaningful truncation (data > 9
bytes so that the cursor is mid-payload when it stops):
```rust
// Build: valid header + partial payload (50 bytes of a declared 1000-byte payload)
let mut data: Vec<u8> = Vec::new();
data.write_u8(0x01).unwrap();
data.write_u32::<BigEndian>(0).unwrap();
data.write_u32::<BigEndian>(1000).unwrap(); // declared length
data.extend_from_slice(&[0u8; 50]);         // only 50 bytes of payload
// cursor stops at 9 (payload window check fails before advancing)
let reported = ...;
assert!(reported < 1009, "reported position is before declared end");
assert!(reported >= 9,   "at least header bytes consumed");
```

---

### M3 — `Cargo.lock` Not Listed in Story File List

**File:** `docs/implementation-artifacts/2-6-indexing-progress-bar.md` → File List

`Cargo.lock` is present in `git diff --name-only` (modified by adding `indicatif` and its
transitive dependencies) but is absent from the story's File List section.

**Fix:** Add `Cargo.lock` to the File List.

---

## LOW Issues

### L1 — `indicatif` Version Not Verified Against Latest Stable

**File:** `Cargo.toml:14`, story task: "verify latest stable on crates.io"

The story explicitly requires verifying the latest stable version of `indicatif`. At the time
of implementation `indicatif = "0.17"` was added, but indicatif `0.18.4` is the current stable
release (visible in the Cargo.lock resolution output during CI: "Adding indicatif v0.17.11
(available: v0.18.4)"). The task checkbox is marked `[x]` but the verification step was not
actually performed.

**Recommendation:** Update to `indicatif = "0.18"` or explicitly document the decision to pin
to 0.17 (e.g., API compatibility reason).

---

### L2 — `open_hprof_header` Is Now Dead Code Without Deprecation Notice

**File:** `crates/hprof-engine/src/lib.rs:59-63`

After this story, `hprof-cli` switched from `open_hprof_header` to
`open_hprof_file_with_progress`. `open_hprof_header` now has no callers inside the workspace.
It is `pub` (so Rust does not emit an unused warning), but it is also:

- Not marked `#[deprecated]`
- Not mentioned in the `//!` module docstring update
- No test covers the non-happy path via this function (its test coverage is zero)

**Recommendation:** Add `#[deprecated(note = "Use open_hprof_file or open_hprof_file_with_progress")]`
or remove it entirely in a clean-up story.

---

### L3 — Speed Computation Produces `Inf`/`NaN` on Edge Cases

**File:** `crates/hprof-tui/src/progress.rs:57`

```rust
let speed = self.total_bytes as f64 / elapsed.as_secs_f64() / 1e9;
```

Two edge cases:
- `elapsed.as_secs_f64() == 0.0`: produces `Inf` (in practice impossible on physical hardware
  but observable in tests that construct and immediately call `finish`).
- `total_bytes == 0` and `elapsed > 0`: produces `0.0 GB/s` (harmless but odd).

**Recommendation:** Guard the division:
```rust
let speed = if elapsed.as_secs_f64() > 0.0 {
    self.total_bytes as f64 / elapsed.as_secs_f64() / 1e9
} else {
    0.0
};
```

---

## AC Validation Summary

| AC | Status | Note |
|---|---|---|
| AC1 — progress bar with bytes, %, speed, ETA | PARTIAL | `%` missing from template (H1) |
| AC2 — at least once per second | PASS | 4 MiB interval @ 4 MiB/s = 1s; indicatif throttles internally |
| AC3 — summary with time, speed, record count | PASS | All present in `finish()` |
| AC4 — truncated file: warning + % of records | PARTIAL | Count ratio shown, not `%`; warning is technical, not user-friendly (M1) |

---

## Task Completion Audit

All tasks marked `[x]` are actually implemented. No false claims found beyond the AC gaps
documented above.

---

## Senior Developer Review (AI)

**Outcome:** Changes Requested
**Date:** 2026-03-07

### Action Items

- [x] [H1][High] Add `{percent:.1}%` to `ProgressStyle` template in `progress.rs:31`
- [x] [M1][Medium] Compute and display percentage in `finish()` summary; add user-friendly incomplete-indexing warning
- [x] [M2][Medium] Fix trivially-true assertion in `progress_callback_reports_partial_position_for_truncated_data`
- [x] [M3][Medium] Add `Cargo.lock` to story File List
- [ ] [L1][Low] Update `indicatif` to `0.18` or document the pin decision
- [ ] [L2][Low] Mark `open_hprof_header` as `#[deprecated]` or remove it
- [ ] [L3][Low] Guard speed computation against `elapsed = 0`
