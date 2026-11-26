//! Native-driver integration tests for the offline database stack.

#![cfg(not(target_arch = "wasm32"))]

use fundamental_core::i18n::{Locale, Localized};
use fundamental_core::model::offline::ConflictStrategy;
use fundamental_core::model::{BoundTo, Model, Offline, OfflineModel};
use fundamental_macros::Model;
use fundamental_offline::db::{
    MIN_SQLITE_VERSION, OfflineConfig, OfflineDatabase, OfflineDbError, Row, Value,
};
use fundamental_offline::runner::OfflineMigrator;
use fundamental_offline::store::{
    OfflineRecord, OfflineStore, SchemaAction, SyncOperation, SyncQueue,
};

/// The native driver's futures are immediately ready.
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

fn open_memory() -> OfflineDatabase {
    block_on(OfflineDatabase::open(OfflineConfig::memory())).expect("open in-memory db")
}

#[test]
fn test_version_guard_modern_sqlite() {
    let db = open_memory();
    assert!(
        db.sqlite_version_number() >= MIN_SQLITE_VERSION,
        "bundled SQLite {} must be >= 3.53.0",
        db.sqlite_version()
    );
}

#[test]
fn test_modern_alter_dialect() {
    let db = open_memory();
    db.execute_batch("CREATE TABLE users (id TEXT PRIMARY KEY, email TEXT, age INTEGER);")
        .unwrap();

    // SQLite 3.53 syntax.
    db.execute_batch("ALTER TABLE users ALTER COLUMN email SET NOT NULL;")
        .unwrap();
    db.execute_batch("ALTER TABLE users ALTER COLUMN email DROP NOT NULL;")
        .unwrap();
    db.execute_batch("ALTER TABLE users ADD CONSTRAINT chk_age CHECK (age >= 0);")
        .unwrap();

    let err = db.execute(
        "INSERT INTO users (id, age) VALUES (?1, ?2);",
        &[Value::from("u1"), Value::from(-3)],
    );
    assert!(err.is_err(), "check constraint must reject negative age");

    db.execute_batch("ALTER TABLE users DROP CONSTRAINT chk_age;")
        .unwrap();
    db.execute(
        "INSERT INTO users (id, age) VALUES (?1, ?2);",
        &[Value::from("u1"), Value::from(-3)],
    )
    .unwrap();
}

#[test]
fn test_value_round_trip() {
    let db = open_memory();
    db.execute_batch(
        "CREATE TABLE v (id INTEGER PRIMARY KEY, n TEXT, i INTEGER, r REAL, t TEXT, b BLOB);",
    )
    .unwrap();

    db.execute(
        "INSERT INTO v (id, n, i, r, t, b) VALUES (?1, ?2, ?3, ?4, ?5, ?6);",
        &[
            Value::from(1),
            Value::Null,
            Value::from(-42i64),
            Value::from(2.5),
            Value::from("hello"),
            Value::from(vec![0xDEu8, 0xAD]),
        ],
    )
    .unwrap();

    let row = db
        .query_one(
            "SELECT n, i, r, t, b FROM v WHERE id = ?1;",
            &[Value::from(1)],
        )
        .unwrap();

    assert_eq!(row.get(0), Some(&Value::Null));
    assert_eq!(row.get(1), Some(&Value::Integer(-42)));
    assert_eq!(row.get(2), Some(&Value::Real(2.5)));
    assert_eq!(row.get_named("t"), Some(&Value::Text("hello".to_string())));
    assert_eq!(row.get_named("b"), Some(&Value::Blob(vec![0xDE, 0xAD])));
}

#[test]
fn test_query_one_and_optional() {
    let db = open_memory();
    db.execute_batch("CREATE TABLE t (x INTEGER);").unwrap();

    assert!(matches!(
        db.query_one("SELECT x FROM t;", &[]),
        Err(OfflineDbError::NoRows)
    ));
    assert!(
        db.query_optional("SELECT x FROM t;", &[])
            .unwrap()
            .is_none()
    );
}

#[test]
fn test_transaction_commit_and_rollback_on_drop() {
    let db = open_memory();
    db.execute_batch("CREATE TABLE t (x INTEGER);").unwrap();

    let tx = db.transaction().unwrap();
    tx.execute("INSERT INTO t (x) VALUES (?1);", &[Value::from(1)])
        .unwrap();
    tx.commit().unwrap();

    {
        let tx = db.transaction().unwrap();
        tx.execute("INSERT INTO t (x) VALUES (?1);", &[Value::from(2)])
            .unwrap();
    }

    let rows = db.query("SELECT x FROM t;", &[]).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get(0), Some(&Value::Integer(1)));
}

#[test]
fn test_foreign_keys_enforced_by_default() {
    let db = open_memory();
    db.execute_batch(
        "CREATE TABLE parent (id TEXT PRIMARY KEY);\n\
         CREATE TABLE child (id TEXT PRIMARY KEY, parent_id TEXT NOT NULL REFERENCES parent (id));",
    )
    .unwrap();

    let err = db.execute(
        "INSERT INTO child (id, parent_id) VALUES (?1, ?2);",
        &[Value::from("c1"), Value::from("missing")],
    );
    assert!(err.is_err(), "FK violation must be rejected");
}

/// SQLite's twelve-step rebuild; the runner must hoist its `PRAGMA foreign_keys` pair.
const REBUILD_AGE_TO_INTEGER: &str = "PRAGMA foreign_keys=OFF;\n\
     CREATE TABLE users_new (id TEXT NOT NULL, age INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (id));\n\
     INSERT INTO users_new (id, age) SELECT id, COALESCE(CAST(age AS INTEGER), 0) FROM users;\n\
     DROP TABLE users;\n\
     ALTER TABLE users_new RENAME TO users;\n\
     PRAGMA foreign_keys=ON;";

#[test]
fn a_hand_written_rebuild_converts_data_and_leaves_foreign_keys_on() {
    let db = open_memory();

    db.execute_batch(
        "CREATE TABLE users (id TEXT NOT NULL, age TEXT, PRIMARY KEY (id));\n\
         INSERT INTO users (id, age) VALUES ('u1', '7');\n\
         INSERT INTO users (id, age) VALUES ('u2', NULL);",
    )
    .unwrap();

    let migrator = OfflineMigrator::new();
    let applied = migrator
        .apply_all(&db, &[("0001_alter_age", REBUILD_AGE_TO_INTEGER)])
        .unwrap();
    assert_eq!(applied, vec!["0001_alter_age".to_string()]);

    let rows = db
        .query("SELECT id, age FROM users ORDER BY id;", &[])
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].get(1), Some(&Value::Integer(7)));
    assert_eq!(rows[1].get(1), Some(&Value::Integer(0)));

    let info: Vec<Row> = db.query("PRAGMA table_info(users);", &[]).unwrap();
    let age = info
        .iter()
        .find(|r| r.get_named("name") == Some(&Value::Text("age".to_string())))
        .unwrap();
    assert_eq!(
        age.get_named("type"),
        Some(&Value::Text("INTEGER".to_string()))
    );
    assert_eq!(age.get_named("notnull"), Some(&Value::Integer(1)));

    let applied_again = migrator
        .apply_all(&db, &[("0001_alter_age", REBUILD_AGE_TO_INTEGER)])
        .unwrap();
    assert!(applied_again.is_empty());

    let fk = db.query_one("PRAGMA foreign_keys;", &[]).unwrap();
    assert_eq!(fk.get(0), Some(&Value::Integer(1)));
}

#[test]
fn test_runner_rolls_back_failed_migration() {
    let db = open_memory();
    let migrator = OfflineMigrator::new();

    let err = migrator.apply_all(
        &db,
        &[(
            "0001_bad",
            "CREATE TABLE good (x INTEGER);\nINSERT INTO nonexistent VALUES (1);",
        )],
    );
    assert!(matches!(err, Err(OfflineDbError::MigrationFailed { .. })));

    assert!(db.query("SELECT * FROM good;", &[]).is_err());
    assert!(migrator.applied(&db).unwrap().is_empty());
}

// `Note` / `NoteV2` / `NoteV2Preserve` are hand-written: the derive hashes `SCHEMA_VERSION`,
// so a deliberate version bump cannot be authored with it.

#[derive(Debug, Clone, PartialEq)]
struct Note {
    id: String,
    title: String,
    stars: i64,
}

impl BoundTo<Offline> for Note {}

impl Model for Note {
    const TYPE_NAME: &'static str = "Note";
}

impl OfflineModel for Note {
    const SQLITE_TABLE: &'static str = "notes";
    const SCHEMA_VERSION: u64 = 1;
    const PRESERVE_DATA: bool = false;
    const CONFLICT_RESOLUTION: ConflictStrategy = ConflictStrategy::LastWriteWins;

    fn create_table_sql() -> &'static str {
        "CREATE TABLE IF NOT EXISTS notes (\n    \
             id TEXT NOT NULL,\n    \
             title TEXT NOT NULL,\n    \
             stars INTEGER NOT NULL,\n    \
             PRIMARY KEY (id)\n)"
    }

    fn column_names() -> &'static [&'static str] {
        &["id", "title", "stars"]
    }

    fn column_definitions() -> &'static [(&'static str, &'static str, bool)] {
        &[
            ("id", "TEXT", false),
            ("title", "TEXT", false),
            ("stars", "INTEGER", false),
        ]
    }
}

impl OfflineRecord for Note {
    fn primary_key_column() -> &'static str {
        "id"
    }

    fn to_values(&self) -> Vec<Value> {
        vec![
            Value::from(self.id.as_str()),
            Value::from(self.title.as_str()),
            Value::from(self.stars),
        ]
    }

    fn from_row(row: &Row) -> Result<Self, OfflineDbError> {
        let decode = |message: &str| OfflineDbError::Decode {
            model: "Note".to_string(),
            message: message.to_string(),
        };
        Ok(Self {
            id: row
                .get_named("id")
                .and_then(|v| v.as_text())
                .ok_or_else(|| decode("id"))?
                .to_string(),
            title: row
                .get_named("title")
                .and_then(|v| v.as_text())
                .ok_or_else(|| decode("title"))?
                .to_string(),
            stars: row
                .get_named("stars")
                .and_then(Value::as_integer)
                .ok_or_else(|| decode("stars"))?,
        })
    }
}

#[derive(Debug)]
struct NoteV2;

impl BoundTo<Offline> for NoteV2 {}

impl Model for NoteV2 {
    const TYPE_NAME: &'static str = "NoteV2";
}

impl OfflineModel for NoteV2 {
    const SQLITE_TABLE: &'static str = "notes";
    const SCHEMA_VERSION: u64 = 2;
    const PRESERVE_DATA: bool = false;
    const CONFLICT_RESOLUTION: ConflictStrategy = ConflictStrategy::LastWriteWins;

    fn create_table_sql() -> &'static str {
        "CREATE TABLE IF NOT EXISTS notes (\n    \
             id TEXT NOT NULL,\n    \
             title TEXT NOT NULL,\n    \
             stars INTEGER NOT NULL,\n    \
             pinned INTEGER NOT NULL DEFAULT 0,\n    \
             PRIMARY KEY (id)\n)"
    }

    fn column_names() -> &'static [&'static str] {
        &["id", "title", "stars", "pinned"]
    }

    fn column_definitions() -> &'static [(&'static str, &'static str, bool)] {
        &[
            ("id", "TEXT", false),
            ("title", "TEXT", false),
            ("stars", "INTEGER", false),
            ("pinned", "INTEGER", false),
        ]
    }
}

#[derive(Debug)]
struct NoteV2Preserve;

impl BoundTo<Offline> for NoteV2Preserve {}

impl Model for NoteV2Preserve {
    const TYPE_NAME: &'static str = "NoteV2Preserve";
}

impl OfflineModel for NoteV2Preserve {
    const SQLITE_TABLE: &'static str = "notes";
    const SCHEMA_VERSION: u64 = 3;
    const PRESERVE_DATA: bool = true;
    const CONFLICT_RESOLUTION: ConflictStrategy = ConflictStrategy::LastWriteWins;

    fn create_table_sql() -> &'static str {
        NoteV2::create_table_sql()
    }

    fn column_names() -> &'static [&'static str] {
        NoteV2::column_names()
    }

    fn column_definitions() -> &'static [(&'static str, &'static str, bool)] {
        NoteV2::column_definitions()
    }
}

#[test]
fn test_store_schema_lifecycle_and_crud() {
    let db = open_memory();
    let store = OfflineStore::new(&db);

    assert_eq!(
        store.ensure_schema::<Note>().unwrap(),
        SchemaAction::Created
    );
    assert_eq!(
        store.ensure_schema::<Note>().unwrap(),
        SchemaAction::Unchanged
    );

    let note = Note {
        id: "n1".to_string(),
        title: "first".to_string(),
        stars: 3,
    };
    store.insert(&note).unwrap();

    let loaded: Note = store.get(&Value::from("n1")).unwrap().unwrap();
    assert_eq!(loaded, note);

    let updated = Note {
        stars: 5,
        ..note.clone()
    };
    store.upsert(&updated).unwrap();
    let loaded: Note = store.get(&Value::from("n1")).unwrap().unwrap();
    assert_eq!(loaded.stars, 5);

    let all: Vec<Note> = store.all().unwrap();
    assert_eq!(all.len(), 1);

    assert!(store.delete::<Note>(&Value::from("n1")).unwrap());
    assert!(!store.delete::<Note>(&Value::from("n1")).unwrap());
    assert!(store.get::<Note>(&Value::from("n1")).unwrap().is_none());
}

#[test]
fn test_store_schema_version_change_recreates_cache_table() {
    let db = open_memory();
    let store = OfflineStore::new(&db);

    store.ensure_schema::<Note>().unwrap();
    store
        .insert(&Note {
            id: "n1".to_string(),
            title: "cached".to_string(),
            stars: 1,
        })
        .unwrap();

    assert_eq!(
        store.ensure_schema::<NoteV2>().unwrap(),
        SchemaAction::Recreated
    );
    let rows = db.query("SELECT * FROM notes;", &[]).unwrap();
    assert!(rows.is_empty(), "cache table is recreated empty");

    assert!(matches!(
        store.ensure_schema::<NoteV2Preserve>(),
        Err(OfflineDbError::SchemaPreserved { .. })
    ));
}

#[test]
fn test_sync_queue() {
    let db = open_memory();
    let queue = SyncQueue::new(&db).unwrap();

    assert!(queue.is_empty().unwrap());

    queue
        .enqueue(
            "notes",
            "n1",
            SyncOperation::Insert,
            Some("{\"id\":\"n1\"}"),
        )
        .unwrap();
    queue
        .enqueue(
            "notes",
            "n1",
            SyncOperation::Update,
            Some("{\"id\":\"n1\"}"),
        )
        .unwrap();
    queue
        .enqueue("notes", "n2", SyncOperation::Delete, None)
        .unwrap();

    let pending = queue.pending().unwrap();
    assert_eq!(pending.len(), 3);
    assert_eq!(pending[0].operation, SyncOperation::Insert);
    assert_eq!(pending[1].operation, SyncOperation::Update);
    assert_eq!(pending[2].operation, SyncOperation::Delete);
    assert_eq!(pending[2].payload, None);
    assert!(pending[0].seq < pending[1].seq);

    let removed = queue
        .mark_synced(&[pending[0].seq, pending[1].seq])
        .unwrap();
    assert_eq!(removed, 2);
    assert_eq!(queue.len().unwrap(), 1);
}

#[test]
fn test_export_import_round_trip() {
    let dir = std::env::temp_dir();
    let src_path = dir.join(format!("fundamental_test_src_{}.db", std::process::id()));
    let dst_path = dir.join(format!("fundamental_test_dst_{}.db", std::process::id()));
    let _ = std::fs::remove_file(&src_path);
    let _ = std::fs::remove_file(&dst_path);

    let bytes = {
        let db = block_on(OfflineDatabase::open(OfflineConfig::persistent(
            src_path.to_string_lossy().into_owned(),
        )))
        .unwrap();
        db.execute_batch("CREATE TABLE t (x INTEGER); INSERT INTO t VALUES (41), (1);")
            .unwrap();
        db.export_bytes().unwrap()
    };

    let dst_config = OfflineConfig::persistent(dst_path.to_string_lossy().into_owned());
    block_on(OfflineDatabase::import_bytes(&dst_config, &bytes)).unwrap();

    let db = block_on(OfflineDatabase::open(dst_config)).unwrap();
    let row = db.query_one("SELECT SUM(x) FROM t;", &[]).unwrap();
    assert_eq!(row.get(0), Some(&Value::Integer(42)));

    let _ = std::fs::remove_file(&src_path);
    let _ = std::fs::remove_file(&dst_path);
}

#[derive(Clone, Debug, PartialEq, Model)]
#[model(offline)]
struct Task {
    #[model(id)]
    id: String,
    title: String,
    done: bool,
    priority: Option<i64>,
}

#[test]
fn test_derived_model_generated_schema() {
    assert_eq!(<Task as OfflineModel>::SQLITE_TABLE, "task");
    assert_eq!(Task::column_names(), &["id", "title", "done", "priority"]);

    let db = open_memory();
    let store = OfflineStore::new(&db);
    assert_eq!(
        store.ensure_schema::<Task>().unwrap(),
        SchemaAction::Created
    );
    assert_eq!(
        store.ensure_schema::<Task>().unwrap(),
        SchemaAction::Unchanged
    );

    let info = db.query("PRAGMA table_info(task);", &[]).unwrap();
    let by_name = |name: &str| {
        info.iter()
            .find(|r| r.get_named("name") == Some(&Value::Text(name.to_string())))
            .unwrap_or_else(|| panic!("column {name} missing from generated schema"))
    };
    assert_eq!(
        by_name("id").get_named("type"),
        Some(&Value::Text("TEXT".to_string()))
    );
    assert_eq!(by_name("id").get_named("notnull"), Some(&Value::Integer(1)));
    assert_eq!(by_name("id").get_named("pk"), Some(&Value::Integer(1)));
    assert_eq!(
        by_name("done").get_named("type"),
        Some(&Value::Text("INTEGER".to_string()))
    );
    assert_eq!(
        by_name("done").get_named("notnull"),
        Some(&Value::Integer(1))
    );
    assert_eq!(
        by_name("priority").get_named("type"),
        Some(&Value::Text("INTEGER".to_string()))
    );
    assert_eq!(
        by_name("priority").get_named("notnull"),
        Some(&Value::Integer(0))
    );
}

#[test]
fn test_derived_model_crud_round_trip() {
    let db = open_memory();
    let store = OfflineStore::new(&db);
    store.ensure_schema::<Task>().unwrap();

    let task = Task {
        id: "t1".to_string(),
        title: "write tests".to_string(),
        done: false,
        priority: Some(5),
    };
    store.insert(&task).unwrap();

    let loaded: Task = store.get(&Value::from("t1")).unwrap().unwrap();
    assert_eq!(loaded, task);

    let updated = Task {
        done: true,
        priority: None,
        ..task.clone()
    };
    store.upsert(&updated).unwrap();
    let loaded: Task = store.get(&Value::from("t1")).unwrap().unwrap();
    assert!(loaded.done);
    assert_eq!(loaded.priority, None);

    let raw = db
        .query_one(
            "SELECT done, priority FROM task WHERE id = ?1;",
            &[Value::from("t1")],
        )
        .unwrap();
    assert_eq!(raw.get(0), Some(&Value::Integer(1)));
    assert_eq!(raw.get(1), Some(&Value::Null));

    assert_eq!(store.all::<Task>().unwrap().len(), 1);
    assert!(store.delete::<Task>(&Value::from("t1")).unwrap());
    assert!(store.get::<Task>(&Value::from("t1")).unwrap().is_none());
}

#[test]
fn test_transaction_explicit_rollback_and_visibility() {
    let db = open_memory();
    db.execute_batch("CREATE TABLE t (x INTEGER);").unwrap();

    let tx = db.transaction().unwrap();
    tx.execute("INSERT INTO t (x) VALUES (?1);", &[Value::from(10)])
        .unwrap();
    tx.execute("INSERT INTO t (x) VALUES (?1);", &[Value::from(20)])
        .unwrap();
    let inside = tx.query("SELECT x FROM t ORDER BY x;", &[]).unwrap();
    assert_eq!(inside.len(), 2);
    tx.rollback().unwrap();

    assert!(db.query("SELECT x FROM t;", &[]).unwrap().is_empty());
}

fn apply_migration(db: &OfflineDatabase, name: &str, sql: &str) {
    OfflineMigrator::new()
        .apply_all(db, &[(name, sql)])
        .unwrap_or_else(|e| panic!("apply {name}: {e}"));
}

fn columns(db: &OfflineDatabase, table: &str) -> Vec<String> {
    db.query(&format!("PRAGMA table_info({table});"), &[])
        .unwrap()
        .iter()
        .filter_map(|r| {
            r.get_named("name")
                .and_then(|v| v.as_text())
                .map(str::to_string)
        })
        .collect()
}

#[test]
fn adding_and_dropping_a_column_through_the_runner() {
    let db = open_memory();
    db.execute_batch("CREATE TABLE users (id TEXT PRIMARY KEY, tmp TEXT);")
        .unwrap();

    apply_migration(
        &db,
        "0001_add_email",
        "ALTER TABLE users ADD COLUMN email TEXT;",
    );
    assert!(columns(&db, "users").contains(&"email".to_string()));

    apply_migration(&db, "0002_drop_tmp", "ALTER TABLE users DROP COLUMN tmp;");
    assert!(!columns(&db, "users").contains(&"tmp".to_string()));
}

#[test]
fn indexing_and_dropping_a_table_through_the_runner() {
    let db = open_memory();
    db.execute_batch("CREATE TABLE users (id TEXT PRIMARY KEY, email TEXT);")
        .unwrap();

    apply_migration(
        &db,
        "0001_create_index",
        "CREATE INDEX idx_users_email ON users (email);",
    );
    let has_index = |db: &OfflineDatabase| {
        db.query("PRAGMA index_list(users);", &[])
            .unwrap()
            .iter()
            .any(|r| r.get_named("name") == Some(&Value::Text("idx_users_email".to_string())))
    };
    assert!(has_index(&db), "index should exist after CREATE INDEX");

    apply_migration(&db, "0002_drop_index", "DROP INDEX idx_users_email;");
    assert!(!has_index(&db), "index should be gone after DROP INDEX");

    db.execute_batch("CREATE TABLE junk (x INTEGER);").unwrap();
    apply_migration(&db, "0003_drop_table", "DROP TABLE junk;");
    assert!(db.query("SELECT * FROM junk;", &[]).is_err());
}

#[test]
fn test_runner_multiple_and_incremental() {
    let db = open_memory();
    let migrator = OfflineMigrator::new();

    let applied = migrator
        .apply_all(
            &db,
            &[
                ("0001_a", "CREATE TABLE a (x INTEGER);"),
                ("0002_b", "CREATE TABLE b (x INTEGER);"),
            ],
        )
        .unwrap();
    assert_eq!(applied, vec!["0001_a".to_string(), "0002_b".to_string()]);
    assert_eq!(
        migrator.applied(&db).unwrap(),
        vec!["0001_a".to_string(), "0002_b".to_string()]
    );

    let applied = migrator
        .apply_all(
            &db,
            &[
                ("0001_a", "CREATE TABLE a (x INTEGER);"),
                ("0003_c", "CREATE TABLE c (x INTEGER);"),
            ],
        )
        .unwrap();
    assert_eq!(applied, vec!["0003_c".to_string()]);
    assert_eq!(
        migrator.applied(&db).unwrap(),
        vec![
            "0001_a".to_string(),
            "0002_b".to_string(),
            "0003_c".to_string()
        ]
    );
}

#[test]
fn test_runner_hoists_fk_pragma_and_restores() {
    let db = open_memory();
    let migrator = OfflineMigrator::new();

    migrator
        .apply_all(
            &db,
            &[(
                "0001_fk_pragma",
                "PRAGMA foreign_keys=OFF;\nCREATE TABLE t (id TEXT PRIMARY KEY);",
            )],
        )
        .unwrap();

    let fk = db.query_one("PRAGMA foreign_keys;", &[]).unwrap();
    assert_eq!(
        fk.get(0),
        Some(&Value::Integer(1)),
        "FK enforcement restored"
    );
}

#[test]
fn test_runner_foreign_key_check_fails_and_rolls_back() {
    let db = open_memory();
    let migrator = OfflineMigrator::new();

    // The dangling insert passes mid-migration but the runner's `foreign_key_check`
    // rolls the whole migration back.
    let body = "PRAGMA foreign_keys=OFF;\n\
        CREATE TABLE p (id TEXT PRIMARY KEY);\n\
        CREATE TABLE c (id TEXT PRIMARY KEY, pid TEXT NOT NULL REFERENCES p(id));\n\
        INSERT INTO c (id, pid) VALUES ('c1', 'ghost');";
    let err = migrator.apply_all(&db, &[("0001_dangling_fk", body)]);
    assert!(matches!(err, Err(OfflineDbError::MigrationFailed { .. })));
    assert!(migrator.applied(&db).unwrap().is_empty());
    assert!(db.query("SELECT * FROM c;", &[]).is_err(), "rolled back");
}

#[test]
fn test_without_foreign_keys_allows_violation() {
    let db = block_on(OfflineDatabase::open(
        OfflineConfig::memory().without_foreign_keys(),
    ))
    .unwrap();
    db.execute_batch(
        "CREATE TABLE parent (id TEXT PRIMARY KEY);\n\
         CREATE TABLE child (id TEXT PRIMARY KEY, parent_id TEXT NOT NULL REFERENCES parent (id));",
    )
    .unwrap();

    db.execute(
        "INSERT INTO child (id, parent_id) VALUES (?1, ?2);",
        &[Value::from("c1"), Value::from("missing")],
    )
    .unwrap();
}

#[test]
fn test_persistent_reopen_preserves_data() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("fundamental_persist_{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let location = path.to_string_lossy().into_owned();

    {
        let db = block_on(OfflineDatabase::open(OfflineConfig::persistent(
            location.clone(),
        )))
        .unwrap();
        db.execute_batch("CREATE TABLE k (id INTEGER PRIMARY KEY, v TEXT);")
            .unwrap();
        db.execute(
            "INSERT INTO k (id, v) VALUES (?1, ?2);",
            &[Value::from(1), Value::from("kept")],
        )
        .unwrap();
    }

    let db = block_on(OfflineDatabase::open(OfflineConfig::persistent(location))).unwrap();
    let row = db
        .query_one("SELECT v FROM k WHERE id = ?1;", &[Value::from(1)])
        .unwrap();
    assert_eq!(row.get(0), Some(&Value::Text("kept".to_string())));

    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_unique_violation_is_error() {
    let db = open_memory();
    db.execute_batch("CREATE TABLE u (id INTEGER PRIMARY KEY, email TEXT UNIQUE);")
        .unwrap();
    db.execute("INSERT INTO u (id, email) VALUES (1, 'a@x.com');", &[])
        .unwrap();
    let err = db.execute("INSERT INTO u (id, email) VALUES (2, 'a@x.com');", &[]);
    assert!(err.is_err(), "duplicate UNIQUE value must error");
}

#[test]
fn test_record_decode_error() {
    let db = open_memory();
    let store = OfflineStore::new(&db);
    store.ensure_schema::<Task>().unwrap();
    db.execute(
        "INSERT INTO task (id, title, done, priority) VALUES (?1, ?2, ?3, ?4);",
        &[
            Value::from("t1"),
            Value::from("x"),
            Value::from(false),
            Value::Null,
        ],
    )
    .unwrap();

    let row = db
        .query_one("SELECT id FROM task WHERE id = ?1;", &[Value::from("t1")])
        .unwrap();
    assert!(matches!(
        Task::from_row(&row),
        Err(OfflineDbError::Decode { .. })
    ));
}

#[test]
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
fn test_row_and_value_accessors() {
    let db = open_memory();
    db.execute_batch("CREATE TABLE r (a INTEGER, b TEXT);")
        .unwrap();
    db.execute(
        "INSERT INTO r (a, b) VALUES (?1, ?2);",
        &[Value::from(7), Value::from("hi")],
    )
    .unwrap();
    let row = db.query_one("SELECT a, b FROM r;", &[]).unwrap();

    assert_eq!(row.len(), 2);
    assert!(!row.is_empty());
    assert_eq!(row.columns(), &["a".to_string(), "b".to_string()]);
    assert_eq!(row.get(0), Some(&Value::Integer(7)));
    assert_eq!(row.get(99), None);
    assert_eq!(row.get_named("missing"), None);
    assert_eq!(
        row.into_values(),
        vec![Value::Integer(7), Value::Text("hi".to_string())]
    );

    assert_eq!(Value::Integer(3).as_integer(), Some(3));
    assert_eq!(Value::Integer(3).as_text(), None);
    assert_eq!(Value::Real(1.5).as_real(), Some(1.5));
    assert_eq!(Value::Text("z".into()).as_text(), Some("z"));
    assert_eq!(Value::Blob(vec![1, 2]).as_blob(), Some(&[1u8, 2][..]));
    assert_eq!(Value::Integer(1).as_bool(), Some(true));
    assert_eq!(Value::Integer(0).as_bool(), Some(false));
    assert_eq!(Value::Null.as_integer(), None);
}

#[derive(Clone, Debug, PartialEq, Model)]
#[model(offline)]
struct Product {
    #[model(id)]
    id: String,
    name: Localized<String>,
    price: i64,
}

#[test]
#[expect(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    reason = "readability baseline"
)]
fn test_localized_field_round_trip() {
    let db = open_memory();
    let store = OfflineStore::new(&db);
    assert_eq!(
        store.ensure_schema::<Product>().unwrap(),
        SchemaAction::Created
    );

    assert_eq!(
        <Product as OfflineModel>::translation_table(),
        Some("product_i18n")
    );
    assert_eq!(Product::translatable_columns(), &["name"]);

    let mut name = Localized::new("Wireless Headphones");
    name.set("es", "Auriculares inalámbricos")
        .set("nl", "Draadloze koptelefoon");
    let product = Product {
        id: "p1".to_string(),
        name,
        price: 9999,
    };
    store.insert(&product).unwrap();

    let raw = db
        .query_one(
            "SELECT name FROM product WHERE id = ?1;",
            &[Value::from("p1")],
        )
        .unwrap();
    assert_eq!(
        raw.get(0),
        Some(&Value::Text("Wireless Headphones".to_string()))
    );
    let trows = db
        .query("SELECT field, locale, value FROM product_i18n;", &[])
        .unwrap();
    assert_eq!(trows.len(), 2, "two overrides stored");

    let loaded: Product = store.get(&Value::from("p1")).unwrap().unwrap();
    assert_eq!(loaded, product);
    let es: Locale = "es".parse().unwrap();
    let de: Locale = "de".parse().unwrap();
    assert_eq!(loaded.name.resolve(&es), "Auriculares inalámbricos");
    assert_eq!(loaded.name.resolve(&de), "Wireless Headphones");

    let hits = store
        .find_ids_by_translation::<Product>("name", "es", "%inalámbricos%")
        .unwrap();
    assert_eq!(hits, [Value::from("p1")]);
    assert!(
        store
            .find_ids_by_translation::<Product>("name", "es", "%nope%")
            .unwrap()
            .is_empty()
    );

    let trimmed = Product {
        id: "p1".to_string(),
        name: Localized::new("Wireless Headphones").with("es", "Auriculares inalámbricos"),
        price: 8999,
    };
    store.upsert(&trimmed).unwrap();
    assert_eq!(
        db.query("SELECT locale FROM product_i18n;", &[])
            .unwrap()
            .len(),
        1,
        "the nl override was removed on upsert"
    );

    let all = store.all::<Product>().unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].name.resolve(&es), "Auriculares inalámbricos");

    let mut p: Product = store.get(&Value::from("p1")).unwrap().unwrap();
    assert!(
        p.name
            .to_check(&fundamental_core::i18n::Languages::new(
                "en".parse().unwrap(),
                ["es".parse().unwrap()]
            ))
            .is_empty(),
        "fresh translation is not stale"
    );
    p.name.set_source("Wireless Earbuds");
    store.upsert(&p).unwrap();
    let mut p: Product = store.get(&Value::from("p1")).unwrap().unwrap();
    assert_eq!(
        p.name.to_check(&fundamental_core::i18n::Languages::new(
            "en".parse().unwrap(),
            ["es".parse().unwrap()]
        )),
        ["es".parse::<fundamental_core::i18n::Locale>().unwrap()],
        "es is stale after the source changed"
    );
    let languages =
        fundamental_core::i18n::Languages::new("en".parse().unwrap(), ["es".parse().unwrap()]);
    let checked = fundamental_core::i18n::Checked::new("ann", 1_763_900_000_123);
    assert!(p.name.still_fits(&es, &languages, checked.clone()));
    store.upsert(&p).unwrap();
    let p: Product = store.get(&Value::from("p1")).unwrap().unwrap();
    assert!(
        p.name.to_check(&languages).is_empty(),
        "the Spanish still fits"
    );
    assert_eq!(p.name.checked(&es), Some(&checked), "who said so, and when");

    assert!(store.delete::<Product>(&Value::from("p1")).unwrap());
    assert!(
        db.query("SELECT * FROM product_i18n;", &[])
            .unwrap()
            .is_empty()
    );
}

#[test]
fn test_sync_queue_persists_across_reopen() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("fundamental_syncq_{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let location = path.to_string_lossy().into_owned();

    {
        let db = block_on(OfflineDatabase::open(OfflineConfig::persistent(
            location.clone(),
        )))
        .unwrap();
        let queue = SyncQueue::new(&db).unwrap();
        queue
            .enqueue("task", "t1", SyncOperation::Insert, Some("{\"id\":\"t1\"}"))
            .unwrap();
        assert_eq!(queue.len().unwrap(), 1);
    }

    let db = block_on(OfflineDatabase::open(OfflineConfig::persistent(location))).unwrap();
    let queue = SyncQueue::new(&db).unwrap();
    let pending = queue.pending().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].operation, SyncOperation::Insert);
    assert_eq!(pending[0].payload.as_deref(), Some("{\"id\":\"t1\"}"));

    let _ = std::fs::remove_file(&path);
}

#[derive(Clone, Debug, PartialEq, Model)]
#[model(offline, soft_delete)]
struct SoftNote {
    #[model(id)]
    id: String,
    body: String,
}

#[test]
fn test_soft_delete_store_round_trip() {
    let db = open_memory();
    let store = OfflineStore::new(&db);
    store.ensure_schema::<SoftNote>().unwrap();

    let note = SoftNote {
        id: "n1".into(),
        body: "hello".into(),
    };
    store.insert(&note).unwrap();

    let id = Value::from("n1");

    assert_eq!(store.get::<SoftNote>(&id).unwrap(), Some(note.clone()));
    assert_eq!(store.all::<SoftNote>().unwrap().len(), 1);

    assert!(store.soft_delete::<SoftNote>(&id).unwrap());
    assert!(store.get::<SoftNote>(&id).unwrap().is_none());
    assert!(store.all::<SoftNote>().unwrap().is_empty());
    assert_eq!(
        store.find_with_deleted::<SoftNote>(&id).unwrap(),
        Some(note.clone())
    );

    assert!(!store.soft_delete::<SoftNote>(&id).unwrap());

    assert!(store.restore::<SoftNote>(&id).unwrap());
    assert_eq!(store.get::<SoftNote>(&id).unwrap(), Some(note.clone()));
    assert_eq!(store.all::<SoftNote>().unwrap().len(), 1);

    assert!(store.purge::<SoftNote>(&id).unwrap());
    assert!(store.get::<SoftNote>(&id).unwrap().is_none());
    assert!(store.find_with_deleted::<SoftNote>(&id).unwrap().is_none());
    assert!(!store.purge::<SoftNote>(&id).unwrap());
}

/// Offline stores a non-primitive key as TEXT via `Display` and reads it back via `FromStr`.
#[test]
fn test_typed_identifier_id_round_trip() {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, fundamental_macros::Identifier)]
    #[identifier(no_sql, no_cql)]
    struct UserId(u64);

    #[derive(Clone, Debug, PartialEq, Model)]
    #[model(offline)]
    struct User {
        #[model(id)]
        id: UserId,
        name: String,
    }

    let db = open_memory();
    let store = OfflineStore::new(&db);
    store.ensure_schema::<User>().unwrap();

    let user = User {
        id: UserId(7),
        name: "Ada".to_string(),
    };
    store.insert(&user).unwrap();

    let loaded: User = store.get(&Value::from("7")).unwrap().unwrap();
    assert_eq!(loaded, user);
    assert_eq!(loaded.id, UserId(7));

    assert!(store.delete::<User>(&Value::from("7")).unwrap());
    assert!(store.get::<User>(&Value::from("7")).unwrap().is_none());
}

#[test]
fn test_skip_field_defaults_on_load() {
    use fundamental_core::model::OfflineModel;

    #[derive(Clone, Debug, PartialEq, Model)]
    #[model(offline)]
    struct Cached {
        #[model(id)]
        id: String,
        value: String,
        #[model(skip)]
        scratch: i64, // never persisted
    }

    let cols = <Cached as OfflineModel>::column_names();
    assert!(cols.contains(&"value"));
    assert!(!cols.contains(&"scratch"));

    let db = open_memory();
    let store = OfflineStore::new(&db);
    store.ensure_schema::<Cached>().unwrap();
    store
        .insert(&Cached {
            id: "c1".to_string(),
            value: "keep".to_string(),
            scratch: 999, // not stored
        })
        .unwrap();

    let loaded: Cached = store.get(&Value::from("c1")).unwrap().unwrap();
    assert_eq!(loaded.value, "keep");
    assert_eq!(loaded.scratch, 0); // reconstructed from Default, not persisted
}
