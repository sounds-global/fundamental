//! Native-only: every `OfflineStore` mutation emits a `TableChange` on the
//! database's change bus.

#![cfg(not(target_arch = "wasm32"))]

use fundamental_core::model::OfflineModel;
use fundamental_macros::Model;
use fundamental_offline::change::{ChangeOp, TableChange};
use fundamental_offline::db::{OfflineConfig, OfflineDatabase, Value};
use fundamental_offline::store::OfflineStore;

#[derive(Clone, Debug, PartialEq, Model)]
#[model(offline)]
struct Task {
    #[model(id)]
    id: String,
    title: String,
}

fn task(id: &str, title: &str) -> Task {
    Task {
        id: id.to_string(),
        title: title.to_string(),
    }
}

fn change(id: &str, op: ChangeOp) -> TableChange {
    TableChange::new(Task::SQLITE_TABLE, Value::from(id.to_string()), op)
}

#[tokio::test]
async fn every_mutation_emits_a_table_change() {
    let db = OfflineDatabase::open(OfflineConfig::memory())
        .await
        .expect("open in-memory db");
    let store = OfflineStore::new(&db);
    store.ensure_schema::<Task>().expect("ensure schema");

    let mut changes = db.watch(Task::SQLITE_TABLE);

    store.insert(&task("t1", "one")).expect("insert");
    assert_eq!(
        changes.try_recv().expect("open"),
        change("t1", ChangeOp::Insert)
    );

    store.upsert(&task("t1", "renamed")).expect("upsert");
    assert_eq!(
        changes.try_recv().expect("open"),
        change("t1", ChangeOp::Update)
    );

    let removed = store
        .delete::<Task>(&Value::from("t1".to_string()))
        .expect("delete");
    assert!(removed);
    assert_eq!(
        changes.try_recv().expect("open"),
        change("t1", ChangeOp::Delete)
    );

    store
        .delete::<Task>(&Value::from("absent".to_string()))
        .expect("delete absent");
    assert!(
        changes.try_recv().is_err(),
        "a no-op delete must not emit a change"
    );
}

#[tokio::test]
async fn a_change_only_reaches_watchers_of_that_table() {
    let db = OfflineDatabase::open(OfflineConfig::memory())
        .await
        .expect("open in-memory db");
    let store = OfflineStore::new(&db);
    store.ensure_schema::<Task>().expect("ensure schema");

    let mut other = db.watch("something_else");
    store.insert(&task("t1", "one")).expect("insert");
    assert!(
        other.try_recv().is_err(),
        "unrelated table must not be notified"
    );
}
