//! An offline model round-tripped through an in-memory store; native only.
//!
//! ```text
//! cargo run --example offline_store -p fundamental_offline --all-features
//! ```

#[cfg(not(target_arch = "wasm32"))]
#[expect(clippy::too_many_lines, reason = "readability baseline")]
fn main() {
    use fundamental_core::model::OfflineModel;
    use fundamental_macros::Model;
    use fundamental_offline::db::{OfflineConfig, OfflineDatabase, Value};
    use fundamental_offline::store::{OfflineStore, SchemaAction};

    #[derive(Clone, Debug, PartialEq, Model)]
    #[model(offline)]
    struct Task {
        #[model(id)]
        id: String,
        title: String,
        done: bool,
        priority: Option<i64>,
    }

    // The native driver's futures resolve immediately.
    fn block_on<F: std::future::Future>(fut: F) -> F::Output {
        use std::task::{Context, Poll, Waker};

        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);
        let mut fut = std::pin::pin!(fut);
        loop {
            match fut.as_mut().poll(&mut cx) {
                Poll::Ready(v) => return v,
                Poll::Pending => std::thread::yield_now(),
            }
        }
    }

    let db = block_on(OfflineDatabase::open(OfflineConfig::memory()))
        .expect("open in-memory offline database");
    let store = OfflineStore::new(&db);

    println!("opened in-memory SQLite {}", db.sqlite_version());
    println!(
        "table for Task model: {}",
        <Task as OfflineModel>::SQLITE_TABLE
    );

    let action = store.ensure_schema::<Task>().expect("create task table");
    assert_eq!(action, SchemaAction::Created);
    println!("ensure_schema: {action:?}");

    let task = Task {
        id: "t1".to_string(),
        title: "write the example".to_string(),
        done: false,
        priority: Some(5),
    };
    store.insert(&task).expect("insert task");
    println!("inserted: {task:?}");

    let loaded: Task = store
        .get(&Value::from("t1"))
        .expect("query task")
        .expect("task t1 should exist");
    assert_eq!(loaded, task);
    println!("loaded:   {loaded:?}");

    let updated = Task {
        done: true,
        priority: None,
        ..task.clone()
    };
    store.upsert(&updated).expect("upsert task");
    let after: Task = store
        .get(&Value::from("t1"))
        .expect("query task")
        .expect("task t1 should still exist");
    println!("upserted: {after:?}");

    let all: Vec<Task> = store.all().expect("list tasks");
    println!("all tasks ({}):", all.len());
    for t in &all {
        println!("  - {t:?}");
    }

    let removed = store
        .delete::<Task>(&Value::from("t1"))
        .expect("delete task");
    let still_there = store.get::<Task>(&Value::from("t1")).expect("query task");
    println!("deleted t1: {removed}; lookup after delete: {still_there:?}");

    println!("done.");
}

#[cfg(target_arch = "wasm32")]
fn main() {
    // The offline store needs a browser VFS on wasm32; this `main` only keeps it compiling.
}
