//! Native-only async repository tests over the offline (SQLite) backend.

#![cfg(not(target_arch = "wasm32"))]

use fundamental_core::Error;
use fundamental_core::i18n::Localized;
use fundamental_db::{
    DatabaseError, KeyedRepository, KeyedRepositoryBatch, Repository, RepositoryAggregate,
    RepositoryBatch, RepositoryExt, RepositoryTranslation,
};
use fundamental_macros::Model;
use fundamental_offline::SharedOfflineDatabase;
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

async fn shared_db() -> fundamental_offline::SharedOfflineDatabase {
    let db = OfflineDatabase::open(OfflineConfig::memory())
        .await
        .expect("open in-memory db");
    OfflineStore::new(&db)
        .ensure_schema::<Task>()
        .expect("ensure schema");
    db.into_shared()
}

fn task(id: &str, priority: Option<i64>) -> Task {
    Task {
        id: id.to_string(),
        title: format!("task {id}"),
        done: false,
        priority,
    }
}

/// The offline repository implements `RepositoryTranslation` with `Value` ids.
fn _assert_repository_translation()
where
    fundamental_offline::OfflineRepository<Task>:
        RepositoryTranslation<Task, fundamental_core::model::Offline, TranslationId = Value>,
{
}

#[tokio::test]
async fn find_ids_by_translation_empty_for_non_translatable_model() {
    // `Task` has no companion table: the search returns no ids rather than erroring.
    let repo = shared_db().await.repository::<Task>();
    let hits = repo
        .find_ids_by_translation("title", "en", "anything")
        .await
        .unwrap();
    assert!(hits.is_empty());
}

#[tokio::test]
async fn insert_find_count_delete_round_trip() {
    let repo = shared_db().await.repository::<Task>();

    assert_eq!(repo.count().await.unwrap(), 0);
    assert!(repo.find(&Value::from("t1")).await.unwrap().is_none());
    assert!(!repo.exists(&Value::from("t1")).await.unwrap());

    let t = task("t1", Some(5));
    let inserted = repo.insert(&t).await.unwrap();
    assert_eq!(inserted, t);

    assert!(repo.exists(&Value::from("t1")).await.unwrap());
    assert_eq!(repo.count().await.unwrap(), 1);

    let loaded = repo.find(&Value::from("t1")).await.unwrap().unwrap();
    assert_eq!(loaded, t);

    let required = repo.find_required(&Value::from("t1")).await.unwrap();
    assert_eq!(required, t);

    let updated = Task {
        done: true,
        priority: None,
        ..t.clone()
    };
    let returned = repo.update(&updated).await.unwrap();
    assert_eq!(returned, updated);
    let loaded = repo.find(&Value::from("t1")).await.unwrap().unwrap();
    assert!(loaded.done);
    assert_eq!(loaded.priority, None);

    assert!(repo.delete(&Value::from("t1")).await.unwrap());
    assert!(!repo.delete(&Value::from("t1")).await.unwrap());
    assert_eq!(repo.count().await.unwrap(), 0);
}

#[tokio::test]
async fn find_required_missing_is_not_found() {
    let repo = shared_db().await.repository::<Task>();
    let err = repo
        .find_required(&Value::from("missing"))
        .await
        .unwrap_err();
    assert_eq!(err.error_code(), "DB_004");
}

#[tokio::test]
async fn batch_operations() {
    let repo = shared_db().await.repository::<Task>();

    let models = vec![task("a", Some(1)), task("b", Some(2)), task("c", None)];
    let inserted = repo.insert_many(&models).await.unwrap();
    assert_eq!(inserted.len(), 3);
    assert_eq!(repo.count().await.unwrap(), 3);

    let found = repo
        .find_many(&[Value::from("a"), Value::from("missing"), Value::from("c")])
        .await
        .unwrap();
    assert_eq!(found.len(), 2, "missing ids are skipped");

    let updates = vec![
        Task {
            done: true,
            ..task("a", Some(9))
        },
        Task {
            done: true,
            ..task("z", Some(0))
        }, // upsert inserts new
    ];
    let updated = repo.update_many(&updates).await.unwrap();
    assert_eq!(updated.len(), 2);
    assert_eq!(repo.count().await.unwrap(), 4);
    assert!(repo.find(&Value::from("a")).await.unwrap().unwrap().done);

    let deleted = repo
        .delete_many(&[Value::from("a"), Value::from("nope"), Value::from("b")])
        .await
        .unwrap();
    assert_eq!(deleted, 2);
    assert_eq!(repo.count().await.unwrap(), 2);
}

#[tokio::test]
async fn count_where_is_unsupported() {
    let repo = shared_db().await.repository::<Task>();
    let q = repo.query();
    let err = repo.count_where(&q).await.unwrap_err();
    assert_eq!(err.error_code(), "DB_002", "reported as a query error");
}

#[tokio::test]
async fn ext_find_or_create_and_update_if_exists() {
    let repo = shared_db().await.repository::<Task>();

    let created = repo
        .find_or_create(&Value::from("x1"), || task("x1", Some(7)))
        .await
        .unwrap();
    assert_eq!(created.priority, Some(7));
    assert_eq!(repo.count().await.unwrap(), 1);

    let again = repo
        .find_or_create(&Value::from("x1"), || task("x1", Some(999)))
        .await
        .unwrap();
    assert_eq!(
        again.priority,
        Some(7),
        "existing row returned, closure ignored"
    );
    assert_eq!(repo.count().await.unwrap(), 1);

    let updated = repo
        .update_if_exists(&Task {
            done: true,
            ..task("x1", Some(7))
        })
        .await
        .unwrap();
    assert!(updated.is_some());
    assert!(repo.find(&Value::from("x1")).await.unwrap().unwrap().done);

    let missing = repo.update_if_exists(&task("ghost", None)).await.unwrap();
    assert!(missing.is_none());
    assert_eq!(repo.count().await.unwrap(), 1);
}

#[derive(Clone, Debug, PartialEq, Model)]
#[model(offline, soft_delete)]
struct Note {
    #[model(id)]
    id: String,
    body: String,
}

async fn shared_notes_db() -> fundamental_offline::SharedOfflineDatabase {
    let db = OfflineDatabase::open(OfflineConfig::memory())
        .await
        .expect("open in-memory db");
    OfflineStore::new(&db)
        .ensure_schema::<Note>()
        .expect("ensure schema");
    db.into_shared()
}

#[tokio::test]
async fn soft_delete_round_trip() {
    use fundamental_db::RepositorySoftDelete;

    let repo = shared_notes_db().await.repository::<Note>();
    let note = Note {
        id: "n1".into(),
        body: "hello".into(),
    };
    repo.insert(&note).await.unwrap();

    let id = Value::from("n1");

    assert!(repo.soft_delete(&id).await.unwrap());
    assert!(repo.find(&id).await.unwrap().is_none());
    assert_eq!(
        repo.find_with_deleted(&id).await.unwrap(),
        Some(note.clone())
    );
    assert!(!repo.soft_delete(&id).await.unwrap());

    assert!(repo.restore(&id).await.unwrap());
    assert_eq!(repo.find(&id).await.unwrap(), Some(note.clone()));

    assert!(repo.purge(&id).await.unwrap());
    assert!(repo.find_with_deleted(&id).await.unwrap().is_none());
    assert!(!repo.purge(&id).await.unwrap());
}

#[derive(Clone, Debug, PartialEq, Model)]
#[model(offline)]
struct Article {
    #[model(id)]
    id: String,
    title: Localized<String>,
}

async fn shared_articles_db() -> fundamental_offline::SharedOfflineDatabase {
    let db = OfflineDatabase::open(OfflineConfig::memory())
        .await
        .expect("open in-memory db");
    OfflineStore::new(&db)
        .ensure_schema::<Article>()
        .expect("ensure schema");
    db.into_shared()
}

#[tokio::test]
async fn find_ids_by_translation_hits_for_localized_model() {
    let repo = shared_articles_db().await.repository::<Article>();

    let mut title = Localized::new("Wireless Headphones");
    title.set("es", "Auriculares inalámbricos");
    repo.insert(&Article {
        id: "a1".into(),
        title,
    })
    .await
    .unwrap();

    let hits = repo
        .find_ids_by_translation("title", "es", "%inalámbricos%")
        .await
        .unwrap();
    assert_eq!(hits, [Value::from("a1")]);

    let misses = repo
        .find_ids_by_translation("title", "es", "%nope%")
        .await
        .unwrap();
    assert!(misses.is_empty());
}

async fn shared_kv_db() -> SharedOfflineDatabase {
    let db = OfflineDatabase::open(OfflineConfig::memory())
        .await
        .expect("open in-memory db");
    db.execute_batch("CREATE TABLE kv (k TEXT PRIMARY KEY, v TEXT NOT NULL);")
        .expect("create kv table");
    db.into_shared()
}

async fn kv_count(db: &SharedOfflineDatabase) -> usize {
    db.transaction(|tx| {
        let rows = tx
            .query("SELECT k FROM kv;", &[])
            .map_err(DatabaseError::from)?;
        Ok::<usize, DatabaseError>(rows.len())
    })
    .await
    .expect("count query")
}

#[tokio::test]
async fn transaction_commits_on_ok() {
    let db = shared_kv_db().await;

    db.transaction(|tx| {
        tx.execute(
            "INSERT INTO kv (k, v) VALUES (?1, ?2);",
            &[Value::from("a"), Value::from("1")],
        )
        .map_err(DatabaseError::from)?;
        tx.execute(
            "INSERT INTO kv (k, v) VALUES (?1, ?2);",
            &[Value::from("b"), Value::from("2")],
        )
        .map_err(DatabaseError::from)?;
        Ok::<(), DatabaseError>(())
    })
    .await
    .expect("transaction should commit");

    assert_eq!(kv_count(&db).await, 2, "both rows persist after commit");
}

#[tokio::test]
async fn transaction_reads_its_own_writes() {
    let db = shared_kv_db().await;

    let seen = db
        .transaction(|tx| {
            tx.execute(
                "INSERT INTO kv (k, v) VALUES (?1, ?2);",
                &[Value::from("a"), Value::from("1")],
            )
            .map_err(DatabaseError::from)?;
            let rows = tx
                .query("SELECT v FROM kv WHERE k = ?1;", &[Value::from("a")])
                .map_err(DatabaseError::from)?;
            Ok::<usize, DatabaseError>(rows.len())
        })
        .await
        .expect("transaction should commit");

    assert_eq!(
        seen, 1,
        "the write is visible to a later read in the same tx"
    );
}

#[tokio::test]
async fn transaction_rolls_back_on_err() {
    let db = shared_kv_db().await;

    let result: Result<(), DatabaseError> = db
        .transaction(|tx| {
            tx.execute(
                "INSERT INTO kv (k, v) VALUES (?1, ?2);",
                &[Value::from("a"), Value::from("1")],
            )
            .map_err(DatabaseError::from)?;
            Err(DatabaseError::query("boom"))
        })
        .await;

    let err = result.expect_err("closure error must propagate");
    assert!(matches!(err, DatabaseError::Query { .. }));
    assert_eq!(err.error_code(), "DB_002");
    assert_eq!(
        kv_count(&db).await,
        0,
        "the write is rolled back after the error"
    );
}

#[tokio::test]
async fn transaction_error_type_absorbs_database_error() {
    #[derive(Debug)]
    enum AppError {
        Db(DatabaseError),
        Domain,
    }
    impl From<DatabaseError> for AppError {
        fn from(e: DatabaseError) -> Self {
            AppError::Db(e)
        }
    }

    let db = shared_kv_db().await;

    let domain: Result<(), AppError> = db
        .transaction(|tx| {
            tx.execute(
                "INSERT INTO kv (k, v) VALUES (?1, ?2);",
                &[Value::from("a"), Value::from("1")],
            )
            .map_err(DatabaseError::from)?;
            Err(AppError::Domain)
        })
        .await;
    assert!(matches!(domain, Err(AppError::Domain)));
    assert_eq!(kv_count(&db).await, 0, "rolled back on a domain error too");

    let converted: Result<(), AppError> = db
        .transaction(|_tx| Err(DatabaseError::query("explicit").into()))
        .await;
    match converted {
        Err(AppError::Db(err)) => assert_eq!(err.error_code(), "DB_002"),
        other => panic!("expected AppError::Db, got {other:?}"),
    }
}

#[tokio::test]
async fn transaction_clones_share_committed_state() {
    let db = shared_kv_db().await;
    let clone = db.clone();

    db.transaction(|tx| {
        tx.execute(
            "INSERT INTO kv (k, v) VALUES (?1, ?2);",
            &[Value::from("a"), Value::from("1")],
        )
        .map_err(DatabaseError::from)?;
        Ok::<(), DatabaseError>(())
    })
    .await
    .expect("first transaction commits");

    // Both handles share one connection.
    assert_eq!(kv_count(&clone).await, 1);
}

#[tokio::test]
async fn typed_tx_commits_models() {
    let db = shared_db().await;

    db.transaction(|tx| {
        tx.insert(&task("t1", Some(1)))?;
        tx.insert(&task("t2", Some(2)))?;
        Ok::<(), DatabaseError>(())
    })
    .await
    .expect("typed tx should commit");

    let repo = db.repository::<Task>();
    assert_eq!(repo.count().await.unwrap(), 2, "both models persist");
    assert_eq!(
        repo.find(&Value::from("t1")).await.unwrap(),
        Some(task("t1", Some(1))),
        "committed model reads back with its fields"
    );
}

#[tokio::test]
async fn typed_tx_reads_its_own_writes() {
    let db = shared_db().await;

    let seen = db
        .transaction(|tx| {
            tx.insert(&task("t1", None))?;
            let found: Option<Task> = tx.find(&Value::from("t1"))?;
            Ok::<bool, DatabaseError>(found.is_some())
        })
        .await
        .expect("typed tx should commit");

    assert!(
        seen,
        "the insert is visible to a later typed find in the same tx"
    );
}

#[tokio::test]
async fn typed_tx_rolls_back_on_err() {
    let db = shared_db().await;

    let result: Result<(), DatabaseError> = db
        .transaction(|tx| {
            tx.insert(&task("t1", None))?;
            Err(DatabaseError::query("boom"))
        })
        .await;

    let err = result.expect_err("closure error must propagate");
    assert!(matches!(err, DatabaseError::Query { .. }));
    assert_eq!(err.error_code(), "DB_002");
    assert_eq!(
        db.repository::<Task>().count().await.unwrap(),
        0,
        "the typed write is rolled back after the error"
    );
}

#[tokio::test]
async fn typed_tx_batch_operations() {
    let db = shared_db().await;

    let (found_len, total) = db
        .transaction(|tx| {
            tx.insert_many(&[task("a", None), task("b", None), task("c", None)])?;
            let some: Vec<Task> = tx.find_many(&[Value::from("a"), Value::from("b")])?;
            Ok::<(usize, u64), DatabaseError>((some.len(), tx.count::<Task>()?))
        })
        .await
        .expect("typed batch tx should commit");

    assert_eq!(
        found_len, 2,
        "find_many returns the requested existing rows"
    );
    assert_eq!(total, 3, "count reflects all inserted rows within the tx");
    assert_eq!(db.repository::<Task>().count().await.unwrap(), 3);
}

#[tokio::test]
async fn typed_tx_ext_find_or_create_and_update_if_exists() {
    let db = shared_db().await;

    let (created, refetched, updated_missing) = db
        .transaction(|tx| {
            let created = tx.find_or_create(&Value::from("x"), || task("x", Some(5)))?;
            let refetched = tx.find_or_create(&Value::from("x"), || task("x", Some(999)))?;
            let updated_missing = tx.update_if_exists(&task("ghost", Some(1)))?;
            Ok::<(Task, Task, Option<Task>), DatabaseError>((created, refetched, updated_missing))
        })
        .await
        .expect("typed ext tx should commit");

    assert_eq!(created.priority, Some(5));
    assert_eq!(
        refetched.priority,
        Some(5),
        "existing row wins over the closure"
    );
    assert!(
        updated_missing.is_none(),
        "no row to update for a missing id"
    );
    assert_eq!(db.repository::<Task>().count().await.unwrap(), 1);
}
