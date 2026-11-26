//! Scaling benchmarks for SQL identifier validation; the per-10x ratios should stay linear.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};

use fundamental_db::ident::{is_valid_identifier, validate_identifier};

const LENGTHS: [usize; 3] = [1, 16, 63];
const SEGMENTS: [usize; 3] = [1, 4, 16];

fn bench_is_valid_by_length(c: &mut Criterion) {
    let mut group = c.benchmark_group("ident/is_valid_identifier");
    for &len in &LENGTHS {
        let s = "a".repeat(len);
        group.bench_with_input(BenchmarkId::from_parameter(len), &s, |b, s| {
            b.iter(|| black_box(is_valid_identifier(black_box(s))));
        });
    }
    group.finish();
}

fn bench_validate_by_segments(c: &mut Criterion) {
    let mut group = c.benchmark_group("ident/validate_identifier");
    for &segs in &SEGMENTS {
        let s = vec!["segment"; segs].join(".");
        group.bench_with_input(BenchmarkId::from_parameter(segs), &s, |b, s| {
            b.iter(|| black_box(validate_identifier(black_box(s), false)));
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_is_valid_by_length,
    bench_validate_by_segments
);
criterion_main!(benches);
