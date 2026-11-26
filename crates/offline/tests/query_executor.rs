//! The offline `QueryExecutor`: `QueryBuilder`/`Filter` against SQLite.
#![cfg(not(target_arch = "wasm32"))]
#![allow(missing_docs)]
#![expect(
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]

use fundamental_core::model::{LeadingKeyAddressed, NoLeadingKey, Offline, Satisfied};
use fundamental_db::{Filter, QueryBuilder, QueryExecutor, Repository};
use fundamental_macros::Model;
use fundamental_offline::SharedOfflineDatabase;
use fundamental_offline::db::{OfflineConfig, OfflineDatabase};
use fundamental_offline::store::OfflineStore;

#[derive(Clone, Debug, PartialEq, Model)]
#[model(offline, offline_table = "widgets")]
struct Widget {
    #[model(id)]
    id: String,
    kind: String,
    size: i64,
    active: bool,
}

fn widget(id: &str, kind: &str, size: i64, active: bool) -> Widget {
    Widget {
        id: id.to_string(),
        kind: kind.to_string(),
        size,
        active,
    }
}

async fn seeded() -> SharedOfflineDatabase {
    let db = OfflineDatabase::open(OfflineConfig::memory())
        .await
        .expect("open");
    OfflineStore::new(&db)
        .ensure_schema::<Widget>()
        .expect("schema");
    let shared = db.into_shared();
    let repo = shared.repository::<Widget>();
    for w in [
        widget("a", "bolt", 3, true),
        widget("b", "bolt", 8, true),
        widget("c", "nut", 5, false),
        widget("d", "nut", 12, true),
    ] {
        repo.insert(&w).await.expect("insert");
    }
    shared
}

fn query() -> QueryBuilder<Widget, Offline> {
    QueryBuilder::new()
}

#[tokio::test]
async fn execute_filters_and_binds_values() {
    let db = seeded().await;
    let exec = db.query_executor::<Widget>();

    let q = query()
        .filter(Filter::eq("kind", "bolt"))
        .filter(Filter::gt("size", 4_i64));
    let mut rows = exec.execute(&q).await.expect("execute");
    rows.sort_by_key(|w| w.id.clone());
    assert_eq!(rows, vec![widget("b", "bolt", 8, true)]);
}

#[tokio::test]
async fn boolean_and_in_filters() {
    let db = seeded().await;
    let exec = db.query_executor::<Widget>();

    let active = exec
        .execute(&query().filter(Filter::eq("active", true)))
        .await
        .expect("execute");
    assert_eq!(active.len(), 3);

    let by_ids = exec
        .execute(&query().filter(Filter::in_list("id", vec!["a".into(), "d".into()])))
        .await
        .expect("execute");
    assert_eq!(by_ids.len(), 2);
}

#[tokio::test]
async fn order_by_and_pagination() {
    let db = seeded().await;
    let exec = db.query_executor::<Widget>();

    let top = exec
        .execute(
            &query()
                .order_by(fundamental_db::OrderBy::desc("size"))
                .limit(2),
        )
        .await
        .expect("execute");
    assert_eq!(top.iter().map(|w| w.size).collect::<Vec<_>>(), vec![12, 8]);
}

#[tokio::test]
async fn count_delete_and_update_where() {
    let db = seeded().await;
    let exec = db.query_executor::<Widget>();

    assert_eq!(
        exec.count(&query().filter(Filter::eq("kind", "nut")))
            .await
            .expect("count"),
        2
    );

    let updated = exec
        .update_where(
            &query().filter(Filter::eq("kind", "bolt")),
            &[("size".into(), 99_i64.into())],
        )
        .await
        .expect("update");
    assert_eq!(updated, 2);
    assert_eq!(
        exec.count(&query().filter(Filter::eq("size", 99_i64)))
            .await
            .expect("count"),
        2
    );

    let deleted = exec
        .delete_where(&query().filter(Filter::lt("size", 10_i64)))
        .await
        .expect("delete");
    assert_eq!(
        deleted, 1,
        "only widget c (size 5) is < 10 after the update"
    );
    assert_eq!(exec.count(&query()).await.expect("count all"), 3);
}

/// An offline read needs no key pinned, and that has to be stated rather than inherited.
///
/// The leading-key constraint is written per database: an offline record does not
/// implement `SingleKeyModel`, so a model-only constraint would break every offline read.
#[test]
fn an_offline_read_is_addressable_without_pinning_a_key() {
    fn assert_addressable<M, D, State>()
    where
        LeadingKeyAddressed<M, D, State>: Satisfied,
    {
    }

    assert_addressable::<Widget, Offline, NoLeadingKey>();
}
