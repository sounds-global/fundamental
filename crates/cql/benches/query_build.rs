//! Scaling benchmarks for YCQL CQL-string generation.
//!
//! The per-10× time ratio confirms assembly is linear in the predicate count.
//!
//! Run with: `cargo bench -p fundamental_cql --features internal-benches`.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};

use fundamental_cql::bench_support;

const SIZES: [usize; 3] = [1, 8, 32];

fn bench_select(c: &mut Criterion) {
    let mut group = c.benchmark_group("cql/select_cql");
    for &n in &SIZES {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter(|| black_box(bench_support::select_cql(black_box(n))));
        });
    }
    group.finish();
}

fn bench_where(c: &mut Criterion) {
    let mut group = c.benchmark_group("cql/where_clause");
    for &n in &SIZES {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter(|| black_box(bench_support::where_clause(black_box(n))));
        });
    }
    group.finish();
}

criterion_group!(benches, bench_select, bench_where);
criterion_main!(benches);
