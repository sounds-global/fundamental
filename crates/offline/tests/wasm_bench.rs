//! Tier 3: true in-browser benchmark for the offline store.
//!
//! criterion cannot run in a browser, so this is a hand-rolled harness.
//!
//! Run in a headless browser (requires `wasm-pack` and headless Chrome/Firefox):
//!
//! ```text
//! wasm-pack test --headless --chrome crates/offline
//! ```
#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};
use web_time::Instant;

use fundamental_macros::Model;
use fundamental_offline::db::{OfflineConfig, OfflineDatabase, Value};
use fundamental_offline::store::OfflineStore;

wasm_bindgen_test_configure!(run_in_browser);

#[derive(Clone, Debug, PartialEq, Model)]
#[model(offline)]
struct Task {
    #[model(id)]
    id: String,
    title: String,
    done: bool,
    priority: Option<i64>,
}

fn task(i: usize) -> Task {
    Task {
        id: format!("t{i}"),
        title: "benchmark task title".to_string(),
        done: false,
        priority: Some(0),
    }
}

/// Logs one timed operation to the console.
fn report(label: &str, elapsed_ns: u128) {
    web_sys::console::log_1(&format!("[bench] {label}: {elapsed_ns} ns").into());
}

async fn measure_insert(label: &str, n: usize, config: OfflineConfig) {
    let db = OfflineDatabase::open(config)
        .await
        .expect("open offline database");
    let store = OfflineStore::new(&db);
    store.ensure_schema::<Task>().expect("ensure schema");

    let start = Instant::now();
    for i in 0..n {
        store.insert(&task(i)).expect("insert");
    }
    report(label, start.elapsed().as_nanos());

    let start = Instant::now();
    let got: Option<Task> = store.get(&Value::from("t0")).expect("get");
    report(&format!("{label}/get_by_pk"), start.elapsed().as_nanos());
    assert!(got.is_some(), "inserted row must be retrievable");
}

/// In-memory wasm SQLite, isolated from IndexedDB I/O.
#[wasm_bindgen_test]
async fn bench_memory_store() {
    for &n in &[10usize, 100, 1_000] {
        measure_insert(&format!("memory/insert/{n}"), n, OfflineConfig::memory()).await;
    }
}

/// IndexedDB-backed store; a distinct database per size keeps runs apart.
#[wasm_bindgen_test]
async fn bench_indexeddb_store() {
    for &n in &[10usize, 100] {
        let config = OfflineConfig::persistent(format!("bench_idb_{n}"));
        measure_insert(&format!("indexeddb/insert/{n}"), n, config).await;
    }
}
