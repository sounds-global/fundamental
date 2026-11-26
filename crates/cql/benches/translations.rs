//! Scaling benchmark for matching a page's translations back to its records.
//!
//! The per-10x time ratio should stay near 10: linear in rows plus translations.
//!
//! Run with: `cargo bench -p fundamental_cql --features internal-benches --bench translations`.

use std::hint::black_box;

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};

use fundamental_cql::bench_support;

const SIZES: [usize; 3] = [10, 100, 1_000];

fn bench_match(c: &mut Criterion) {
    let mut group = c.benchmark_group("cql/page_translations");
    for &n in &SIZES {
        let page = bench_support::translated_page(n);
        assert_eq!(bench_support::match_translations(page.clone()), 2 * n);
        group.bench_with_input(BenchmarkId::from_parameter(n), &page, |b, page| {
            b.iter_batched(
                || page.clone(),
                |page| black_box(bench_support::match_translations(page)),
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

criterion_group!(benches, bench_match);
criterion_main!(benches);
