# Code Review Comparison — Story 8.1

**Claude (Opus 4.6, Dev Agent)** vs **Codex**
**Date:** 2026-03-08

## Issue-by-Issue Comparison

| # | Issue | Claude | Codex | Agreement |
|---|-------|--------|-------|-----------|
| Reformatting out of scope | M1+M2 (main.rs, segment.rs, first_pass.rs cosmetic) | Not seen | Claude only |
| Incomplete File List | M3 | M5 (different angle: no git delta) | Same root cause |
| `PreciseIndex::new()` dead code | M4 | Not mentioned | Claude only |
| `cargo fmt` fail | Not mentioned | C1 (CRITICAL) | Codex only — legitimate, bench edit broke fmt |
| Unbounded pre-alloc memory | Not mentioned | H1 (HIGH — OOM risk on large dumps) | Codex only — valid for >10 GB dumps |
| ZGC test standalone vs integration | L1 (LOW) | H2 (HIGH) | Same finding, different severity (Codex more aggressive) |
| Unknown sub-tag `break` | Not mentioned | M4 (MEDIUM) | Codex only — pre-existing but relevant |
| AC2 "zero reallocations" unverifiable | Not mentioned | L1 (LOW) | Codex only — good point |
| Bench does not measure sort in isolation | L2 | Not mentioned | Claude only |
| `sub_record_start - 1` undocumented | L3 | Not mentioned | Claude only |

## Severity Summary

| Metric | Claude | Codex |
|--------|--------|-------|
| Total issues | 7 | 6 |
| Critical | 0 | 1 |
| High | 0 | 2 |
| Medium | 4 | 2 |
| Low | 3 | 1 |

## Analysis

**Codex** was more aggressive and found real problems that Claude missed:
- **`cargo fmt` fail** — Claude trusted the Dev Agent Record instead of running the check. Legitimate CRITICAL.
- **Unbounded pre-allocation** — valid concern. On a 20 GB dump, `data.len() / 80` = 250M entries = ~4 GB Vec pre-allocated.
- **Unknown sub-tag `break`** — pre-existing behavior but relevant observation.

**Claude** had better coverage of cosmetic noise (out-of-scope reformatting) and dead code (`new()` vs `with_capacity()`).

Both reviews complement each other well. Combined, they surface 10 unique issues across the full severity spectrum.
