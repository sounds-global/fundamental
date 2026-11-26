//! Scaling benchmarks for [`ValidationErrors`] accumulation and lookup.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};

use fundamental_core::validate::ValidationErrors;

const SIZES: [usize; 3] = [100, 1_000, 10_000];

fn populated(n: usize) -> ValidationErrors {
    let mut errs = ValidationErrors::new();
    for i in 0..n {
        errs.add_for_field(format!("field_{i}"), "CODE_001", "must be valid");
    }
    errs
}

#[expect(clippy::excessive_nesting, reason = "readability baseline")]
fn bench_add(c: &mut Criterion) {
    let mut group = c.benchmark_group("validation/add");
    for &n in &SIZES {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter(|| {
                let mut errs = ValidationErrors::new();
                for _ in 0..n {
                    errs.add_for_field(
                        black_box("field"),
                        black_box("CODE_001"),
                        black_box("must be valid"),
                    );
                }
                black_box(errs.len())
            });
        });
    }
    group.finish();
}

fn bench_errors_for_field(c: &mut Criterion) {
    let mut group = c.benchmark_group("validation/errors_for_field");
    for &n in &SIZES {
        let errs = populated(n);
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| black_box(errs.errors_for_field(black_box("field_0")).len()));
        });
    }
    group.finish();
}

fn bench_errors_for_prefix(c: &mut Criterion) {
    let mut group = c.benchmark_group("validation/errors_for_prefix");
    for &n in &SIZES {
        let errs = populated(n);
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| black_box(errs.errors_for_prefix(black_box("field")).len()));
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_add,
    bench_errors_for_field,
    bench_errors_for_prefix
);
criterion_main!(benches);
