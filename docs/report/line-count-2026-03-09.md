# Line Count Report — 2026-03-09

Format: `filename — total (tests)`

```
crates/
├── hprof-api/                                         174 (43)
│   └── src/
│       ├── lib.rs                                      11 (0)
│       └── progress.rs                                163 (43)
├── hprof-cli/                                         294 (60)
│   └── src/
│       ├── main.rs                                    145 (34)
│       └── progress.rs                                149 (26)
├── hprof-engine/                                    2 572 (1 518)
│   └── src/
│       ├── lib.rs                                     247 (148)
│       ├── engine.rs                                  390 (200)
│       ├── engine_impl.rs                           1 627 (985)
│       └── resolver.rs                                308 (185)
├── hprof-parser/                                    6 363 (3 367)
│   ├── benches/
│   │   └── first_pass.rs                               66 (0)
│   └── src/
│       ├── lib.rs                                      59 (0)
│       ├── error.rs                                    99 (56)
│       ├── header.rs                                  204 (117)
│       ├── hprof_file.rs                              816 (328)
│       ├── id.rs                                       84 (45)
│       ├── java_types.rs                              132 (58)
│       ├── mmap.rs                                     96 (53)
│       ├── record.rs                                  237 (167)
│       ├── strings.rs                                 215 (123)
│       ├── tags.rs                                    297 (99)
│       ├── test_utils.rs                              646 (262)
│       ├── types.rs                                   590 (305)
│       └── indexer/
│           ├── mod.rs                                  87 (31)
│           ├── precise.rs                             227 (106)
│           ├── segment.rs                             243 (108)
│           └── first_pass/
│               ├── mod.rs                             163 (100)
│               ├── heap_extraction.rs                 272 (0)
│               ├── hprof_primitives.rs                163 (0)
│               ├── record_scan.rs                     297 (0)
│               ├── tests.rs                         1 409 (1 409)
│               └── thread_resolution.rs               253 (0)
└── hprof-tui/                                       3 566 (1 672)
    └── src/
        ├── lib.rs                                      12 (0)
        ├── app.rs                                   1 169 (606)
        ├── input.rs                                   161 (104)
        ├── theme.rs                                    63 (23)
        └── views/
            ├── mod.rs                                   5 (0)
            ├── stack_view.rs                         1 635 (740)
            ├── status_bar.rs                          171 (107)
            └── thread_list.rs                         350 (92)
```

## Summary

| Crate | Total lines | Test lines | % tests |
|---|---:|---:|---:|
| hprof-api | 174 | 43 | 24.7% |
| hprof-cli | 294 | 60 | 20.4% |
| hprof-engine | 2 572 | 1 518 | 59.0% |
| hprof-parser | 6 363 | 3 367 | 52.9% |
| hprof-tui | 3 566 | 1 672 | 46.9% |
| **Grand total** | **12 969** | **6 660** | **51.3%** |

## Observations

- **Largest files:** `stack_view.rs` (1 635), `engine_impl.rs` (1 627), `first_pass/tests.rs` (1 409), `app.rs` (1 169). These four files alone account for 45% of all code.
- **Best test ratios:** `hprof-engine` leads at 59.0% test lines, followed by `hprof-parser` at 52.9%. Overall the project is 51.3% test code — strong TDD discipline.
- **Dedicated test file:** `first_pass/tests.rs` is 100% test code (1 409 lines), centralising tests for the split first_pass sub-modules which themselves contain 0 inline tests.
- **Lowest test coverage:** `hprof-cli` (20.4%) and `hprof-api` (24.7%) — expected for thin CLI/API boundary crates.
- **New crate since last report:** `hprof-api` (174 lines) was extracted, and `hprof-cli` grew from 1 to 2 files with the progress observer refactor.
- **first_pass split:** The monolithic `first_pass.rs` (2 849 lines) was split into 6 focused sub-modules totalling 2 557 lines of source + tests.
- **hprof-parser dominates:** Nearly half the codebase (49.1%) lives in the parser crate, reflecting the complexity of hprof binary format handling.
