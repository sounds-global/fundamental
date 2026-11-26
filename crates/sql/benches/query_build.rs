//! Scaling benchmarks for YSQL SQL-string generation; run with `cargo bench -p fundamental_sql --features internal-benches`.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};

use fundamental_sql::bench_support;

const SIZES: [usize; 3] = [1, 10, 100];

fn bench_select(c: &mut Criterion) {
    let mut group = c.benchmark_group("sql/select_sql");
    for &n in &SIZES {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter(|| black_box(bench_support::select_sql(black_box(n))));
        });
    }
    group.finish();
}

fn bench_update(c: &mut Criterion) {
    let mut group = c.benchmark_group("sql/update_sql");
    for &n in &SIZES {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter(|| black_box(bench_support::update_sql(black_box(n))));
        });
    }
    group.finish();
}

fn bench_where(c: &mut Criterion) {
    let mut group = c.benchmark_group("sql/where_clause");
    for &n in &SIZES {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter(|| black_box(bench_support::where_clause(black_box(n))));
        });
    }
    group.finish();
}

criterion_group!(benches, bench_select, bench_update, bench_where);
criterion_main!(benches);
