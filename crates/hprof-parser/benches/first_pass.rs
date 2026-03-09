//! Criterion benchmarks for the first-pass indexer.
//!
//! Requires `HPROF_BENCH_FILE` env var pointing to a real
//! `.hprof` file. All benchmarks are skipped when unset.
//!
//! Per-phase granularity (string parsing, heap extraction,
//! segment filter build) is measured via tracing-chrome
//! spans in Perfetto UI — not separate criterion groups.
//! See Story 8.0 Dev Notes for rationale.

use criterion::{Criterion, criterion_group, criterion_main};
use hprof_parser::indexer::first_pass::run_first_pass;
use hprof_parser::parse_header;

/// Returns `(data, records_start, id_size)` or `None` if
/// env var is unset.
fn load_bench_file() -> Option<(Vec<u8>, usize, u32)> {
    let path = match std::env::var("HPROF_BENCH_FILE") {
        Ok(p) => {
            let p = std::path::PathBuf::from(p);
            if p.is_relative() {
                let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .ancestors()
                    .nth(2)
                    .expect("workspace root");
                workspace.join(p)
            } else {
                p
            }
        }
        Err(_) => {
            eprintln!(
                "HPROF_BENCH_FILE not set, skipping \
                 benchmarks"
            );
            return None;
        }
    };
    let data = std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "failed to read HPROF_BENCH_FILE \
                 (resolved: {}): {e}",
            path.display()
        )
    });
    let header = parse_header(&data).expect("invalid hprof header");
    Some((data, header.records_start, header.id_size))
}

fn bench_first_pass_total(c: &mut Criterion) {
    let Some((data, start, id_size)) = load_bench_file() else {
        return;
    };
    c.bench_function("first_pass_total", |b| {
        b.iter(|| run_first_pass(&data[start..], id_size, |_| {}));
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(10)
        .measurement_time(std::time::Duration::from_secs(10));
    targets = bench_first_pass_total
}
criterion_main!(benches);
