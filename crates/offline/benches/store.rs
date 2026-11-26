//! Tier 1 (native) scaling benchmark for the offline store.
//!
//! The browser driver is measured by `tests/wasm_bench.rs`.

use std::future::Future;
use std::hint::black_box;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};

use fundamental_core::i18n::Localized;
use fundamental_macros::Model;
use fundamental_offline::db::{OfflineConfig, OfflineDatabase, Value};
use fundamental_offline::store::OfflineStore;

#[derive(Clone, Debug, PartialEq, Model)]
#[model(offline)]
struct Task {
    #[model(id)]
    id: String,
    title: String,
    done: bool,
    priority: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Model)]
#[model(offline)]
struct Article {
    #[model(id)]
    id: String,
    title: Localized<String>,
}

/// The native driver's futures resolve immediately.
fn block_on<F: Future>(fut: F) -> F::Output {
    let waker = Waker::noop();
    let mut cx = Context::from_waker(waker);
    let mut fut = pin!(fut);
    loop {
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(v) => return v,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}

fn task(i: usize) -> Task {
    Task {
        id: format!("t{i}"),
        title: "benchmark task title".to_string(),
        done: false,
        priority: Some(0),
    }
}

const SIZES: [usize; 3] = [10, 100, 1_000];

#[expect(clippy::excessive_nesting, reason = "readability baseline")]
fn bench_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("offline/insert");
    for &n in &SIZES {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter(|| {
                let db = block_on(OfflineDatabase::open(OfflineConfig::memory()))
                    .expect("open in-memory db");
                let store = OfflineStore::new(&db);
                store.ensure_schema::<Task>().expect("ensure schema");
                for i in 0..n {
                    store.insert(&task(i)).expect("insert");
                }
                black_box(store.count::<Task>().expect("count"))
            });
        });
    }
    group.finish();
}

fn bench_get(c: &mut Criterion) {
    let mut group = c.benchmark_group("offline/get_by_pk");
    for &n in &SIZES {
        let db =
            block_on(OfflineDatabase::open(OfflineConfig::memory())).expect("open in-memory db");
        let store = OfflineStore::new(&db);
        store.ensure_schema::<Task>().expect("ensure schema");
        for i in 0..n {
            store.insert(&task(i)).expect("insert");
        }
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| {
                let got: Option<Task> = store.get(black_box(&Value::from("t0"))).expect("get");
                black_box(got)
            });
        });
    }
    group.finish();
}

fn article(i: usize) -> Article {
    Article {
        id: format!("a{i}"),
        title: Localized::new(format!("Article {i}"))
            .with("nl", format!("Artikel {i}"))
            .with("de", format!("Artikel {i} (de)")),
    }
}

/// `all` over records with two translations each: measures the hydration.
fn bench_all_translated(c: &mut Criterion) {
    let mut group = c.benchmark_group("offline/all_translated");
    for &n in &[10, 100, 1_000, 5_000] {
        let db =
            block_on(OfflineDatabase::open(OfflineConfig::memory())).expect("open in-memory db");
        let store = OfflineStore::new(&db);
        store.ensure_schema::<Article>().expect("ensure schema");
        for i in 0..n {
            store.insert(&article(i)).expect("insert");
        }
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| black_box(store.all::<Article>().expect("all")));
        });
    }
    group.finish();
}

criterion_group!(benches, bench_insert, bench_get, bench_all_translated);
criterion_main!(benches);
