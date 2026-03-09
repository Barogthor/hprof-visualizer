# Line Count Report — 2026-03-10

> Format: `filename — total (tests)`

## ASCII Tree

```
crates/                                                        17 623 (8 423)
├── hprof-api/                                                    174 (44)
│   └── src/
│       ├── lib.rs                                                 11 (0)
│       └── progress.rs                                           163 (44)
│
├── hprof-cli/                                                    316 (57)
│   └── src/
│       ├── main.rs                                               169 (32)
│       └── progress.rs                                           147 (25)
│
├── hprof-engine/                                               4 261 (2 480)
│   └── src/
│       ├── lib.rs                                                259 (147)
│       ├── engine.rs                                             404 (192)
│       ├── engine_impl.rs                                      2 081 (1 313)
│       ├── pagination.rs                                       1 208 (643)
│       └── resolver.rs                                           309 (185)
│
├── hprof-parser/                                               6 848 (2 969)
│   └── src/
│       ├── lib.rs                                                 60 (0)
│       ├── error.rs                                               99 (56)
│       ├── id.rs                                                  84 (47)
│       ├── mmap.rs                                                96 (53)
│       ├── java_types.rs                                         132 (60)
│       ├── tags.rs                                               297 (101)
│       ├── record.rs                                             237 (174)
│       ├── strings.rs                                            215 (124)
│       ├── header.rs                                             204 (117)
│       ├── types.rs                                              590 (345)
│       ├── test_utils.rs                                         646 (267)
│       ├── hprof_file.rs                                         916 (329)
│       └── indexer/
│           ├── mod.rs                                             97 (33)
│           ├── precise.rs                                        227 (107)
│           ├── segment.rs                                        243 (110)
│           └── first_pass/
│               ├── mod.rs                                        205 (18)
│               ├── heap_extraction.rs                            291 (0)
│               ├── hprof_primitives.rs                           166 (0)
│               ├── record_scan.rs                                209 (0)
│               ├── thread_resolution.rs                          254 (0)
│               └── tests.rs                                    1 580 (1 028)
│
└── hprof-tui/                                                  6 024 (2 873)
    └── src/
        ├── lib.rs                                                 23 (0)
        ├── theme.rs                                               62 (23)
        ├── input.rs                                              178 (116)
        ├── app.rs                                              1 976 (1 140)
        └── views/
            ├── mod.rs                                              4 (0)
            ├── status_bar.rs                                     170 (107)
            ├── thread_list.rs                                    405 (126)
            └── stack_view.rs                                   3 206 (1 361)
```

## Summary Table

| Crate          | Total lines | Test lines | % Tests |
|----------------|------------:|-----------:|--------:|
| hprof-api      |         174 |         44 |   25.3% |
| hprof-cli      |         316 |         57 |   18.0% |
| hprof-engine   |       4 261 |      2 480 |   58.2% |
| hprof-parser   |       6 848 |      2 969 |   43.4% |
| hprof-tui      |       6 024 |      2 873 |   47.7% |
| **Grand Total**| **17 623** |  **8 423** | **47.8%** |

## Observations

### Largest files
- `hprof-tui/src/views/stack_view.rs` — **3 206 lines** (1 361 test): the heaviest file in the project by far, warrants a split review
- `hprof-engine/src/engine_impl.rs` — **2 081 lines** (1 313 test): core engine implementation, over half is tests
- `hprof-tui/src/app.rs` — **1 976 lines** (1 140 test): TUI application state, also large
- `hprof-parser/src/indexer/first_pass/tests.rs` — **1 580 lines** (1 028 test): dedicated test file for the first-pass indexer

### Best test ratios
- `hprof-engine` leads all crates at **58.2%** test lines — strong TDD discipline
- `hprof-tui/src/input.rs` — **65.2%** test lines within its file (best single-file ratio)
- `hprof-engine/src/engine_impl.rs` — **63.1%** test lines (1 313 / 2 081)

### Notable patterns
- **`hprof-api` and `hprof-cli`** are thin layers with few tests (18–25%), consistent with their role as wiring/entry-point code
- **Four `first_pass/` modules** (`heap_extraction`, `hprof_primitives`, `record_scan`, `thread_resolution`) carry **0 test lines** — their tests are consolidated in the sibling `tests.rs` file
- **`test_utils.rs`** (646 lines) is a significant shared fixture file, not a test file per se — its 267 "test lines" are feature-gated helpers (`#[cfg(test)]`) rather than test cases
- Total project is **~47.8% test code**, consistent with the TDD mandate in CLAUDE.md
