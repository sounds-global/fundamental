//! Constant-time redaction guard for [`Secret`].

use std::fmt::Write as _;
use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};

use fundamental_core::secrets::Secret;

/// Wrapped-payload sizes; redaction cost must not depend on these.
const SIZES: [usize; 3] = [16, 256, 4_096];

fn bench_debug(c: &mut Criterion) {
    let mut group = c.benchmark_group("secrets/debug_redaction");
    for &n in &SIZES {
        let secret = Secret::new("x".repeat(n));
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| {
                let mut out = String::new();
                let _ = write!(out, "{:?}", black_box(&secret));
                black_box(out)
            });
        });
    }
    group.finish();
}

fn bench_serialize(c: &mut Criterion) {
    let mut group = c.benchmark_group("secrets/serialize_redaction");
    for &n in &SIZES {
        let secret = Secret::new("x".repeat(n));
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| black_box(serde_json::to_string(black_box(&secret))));
        });
    }
    group.finish();
}

criterion_group!(benches, bench_debug, bench_serialize);
criterion_main!(benches);
