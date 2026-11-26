//! The tuple table: made by the migrator, served by a row-access role, checked at boot.
#![cfg(feature = "sql")]
#![allow(missing_docs)]
#![expect(
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]

mod support;

use std::path::Path;
use std::sync::Arc;

use fundamental_authz::authorizer::{
    Authorizer, LockedQuestions, NativeEngine, TransactionalAuthorizer,
};
use fundamental_authz::backend::SqlTupleStore;
use fundamental_authz::schema::{
    NamespaceIr, NamespaceName, RelatedType, RelationIr, RelationName, SchemaIr,
};
use fundamental_authz::tuple::{CanonicalId, CaveatContext, ObjectRef, RelationTuple, SubjectRef};
use fundamental_authz::zookie::Consistency;
use fundamental_authz::{AuthzError, SQL_TUPLE_MIGRATIONS};
use fundamental_core::Error as _;
use fundamental_db::{IsolationLevel, SchemaName};
use fundamental_sql::migration::TableCatalog;
use fundamental_sql::{SqlConfig, SqlError, SqlPool};
use fundamental_test_containers::sql::{SqlAuthNode, SqlRole, SqlSchema};

const TABLE: &str = "authz_relation_tuple";

fn ns(name: &str) -> NamespaceName {
    NamespaceName::new(name).expect("a valid namespace")
}
fn rel(name: &str) -> RelationName {
    RelationName::new(name).expect("a valid relation")
}
fn document(id: &str) -> ObjectRef {
    ObjectRef::new(ns("document"), CanonicalId::new(id).expect("a valid id"))
}
fn user(id: &str) -> SubjectRef {
    SubjectRef::leaf(ns("user"), CanonicalId::new(id).expect("a valid id"))
}
fn owner_of(doc: &str, who: &str) -> RelationTuple {
    RelationTuple::new(document(doc), rel("owner"), user(who))
}

fn schema() -> Arc<SchemaIr> {
    Arc::new(
        SchemaIr::builder()
            .namespace_ir(NamespaceIr::new(ns("user"), vec![]))
            .namespace_ir(NamespaceIr::new(
                ns("document"),
                vec![
                    RelationIr::direct(rel("owner"), vec![RelatedType::leaf(ns("user"))])
                        .with_lock(),
                ],
            ))
            .build()
            .expect("a valid schema"),
    )
}

async fn catalog(pool: &SqlPool) -> Option<TableCatalog> {
    TableCatalog::read(pool, TABLE)
        .await
        .expect("the catalog reads")
}

/// Runs one idempotent DDL statement, retried when the catalog moved under it.
async fn run(pool: &SqlPool, statement: &str) {
    pool.execute_ddl(statement)
        .await
        .unwrap_or_else(|error| panic!("{statement}: {error}"));
}

/// Creates an index, dropping a half-built one from a failed attempt first.
async fn create_index(pool: &SqlPool, name: &str, create: &str) {
    for attempt in 1..=8 {
        run(pool, &format!("DROP INDEX IF EXISTS {name}")).await;
        match pool.execute(create).await {
            Ok(_) => return,
            Err(SqlError::SerializationConflict { .. }) if attempt < 8 => {}
            Err(error) => panic!("{create}: {error}"),
        }
    }
}

fn mismatch(outcome: Result<(), AuthzError>) -> String {
    match outcome {
        Err(AuthzError::TupleTableMismatch { problems }) => {
            let error = AuthzError::TupleTableMismatch {
                problems: problems.clone(),
            };
            assert_eq!(error.error_code(), "AUTHZ_029");
            assert_eq!(error.http_status_code(), 500);
            problems
        }
        other => panic!("expected AUTHZ_029, got {other:?}"),
    }
}

/// Only a role holding just row access passes `verify_runtime_role`; the owner is refused.
#[tokio::test]
#[expect(clippy::too_many_lines, reason = "readability baseline")]
async fn the_tuple_table_migrates_and_serves() {
    let owner = SqlRole::mint().await.expect("the migrator's role");
    let runtime = SqlRole::mint().await.expect("the runtime role");
    let schema_name = SchemaName::new(owner.name()).expect("the owner's schema");
    let connect = |role: &SqlRole| {
        SqlConfig::development(role.url().expose().as_str()).default_schema(schema_name.clone())
    };
    let migrator = SqlPool::connect(&connect(&owner))
        .await
        .expect("the migrator connects");
    assert_eq!(
        support::migrate_tuple_table(&migrator).await,
        ["fundamental_authz/0001_authz_relation_tuple"]
    );
    run(
        &migrator,
        &format!(
            "GRANT USAGE ON SCHEMA {} TO {}",
            owner.name(),
            runtime.name()
        ),
    )
    .await;
    run(
        &migrator,
        &format!(
            "GRANT SELECT, INSERT, UPDATE, DELETE ON {TABLE} TO {}",
            runtime.name()
        ),
    )
    .await;
    let served = SqlPool::connect(&connect(&runtime))
        .await
        .expect("the runtime role connects");

    let store = SqlTupleStore::new(served.clone());
    store.verify_schema().await.expect("the migrated table");
    store
        .verify_runtime_role()
        .await
        .expect("the runtime role holds only row access");
    let refused = SqlTupleStore::new(migrator.clone())
        .verify_runtime_role()
        .await;
    match refused {
        Err(AuthzError::TupleRoleTooBroad { ref excess }) => {
            assert_eq!(
                excess,
                "owns the table, CREATE on its schema, TRIGGER, TRUNCATE, REFERENCES"
            );
            let error = refused.expect_err("refused");
            assert_eq!(error.error_code(), "AUTHZ_030");
        }
        other => panic!("the migrator's role owns the table: {other:?}"),
    }

    let authz = NativeEngine::new(store, schema());
    Authorizer::write(&authz, &[owner_of("plan", "ada")])
        .await
        .expect("a write");
    let check = |doc: &'static str| {
        Authorizer::check(
            &authz,
            document(doc),
            rel("owner"),
            user("ada"),
            CaveatContext::new(),
            Consistency::FullConsistency,
        )
    };
    assert!(check("plan").await.expect("a check").allowed);
    let listed = Authorizer::list_objects(
        &authz,
        user("ada"),
        ns("document"),
        rel("owner"),
        CaveatContext::new(),
        Consistency::FullConsistency,
        10,
    )
    .await
    .expect("a listing");
    assert_eq!(listed.objects, [document("plan")]);

    served
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            SqlTupleStore::write_in(tx, &[owner_of("draft", "ada")]).await
        })
        .await
        .expect("a write in the caller's transaction");
    let decision = served
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            authz
                .check_in_locked(
                    tx,
                    LockedQuestions::new(document("draft"), rel("owner"), user("ada")),
                )
                .await
        })
        .await
        .expect("a locked check reads FOR UPDATE");
    assert!(decision.all());
    let removed = served
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            SqlTupleStore::delete_in(tx, &[owner_of("draft", "ada")]).await
        })
        .await
        .expect("a delete in the caller's transaction");
    assert_eq!(removed, 1);
    Authorizer::delete(&authz, &[owner_of("plan", "ada")])
        .await
        .expect("a delete");
    assert!(!check("plan").await.expect("a check").allowed);

    let created = served
        .execute(&format!("CREATE INDEX probe ON {TABLE} (caveat_name HASH)"))
        .await;
    assert!(created.is_err(), "the runtime role cannot change the table");

    // A role holding the owner's membership without inheriting it can `SET ROLE` to it.
    let borrower = SqlRole::mint()
        .await
        .expect("a role that can become the owner");
    let node = SqlAuthNode::shared().await.expect("the auth node");
    let admin = SqlPool::connect(&SqlConfig::development(node.superuser_url().as_str()))
        .await
        .expect("the superuser connects");
    run(&admin, &format!("ALTER ROLE {} NOINHERIT", borrower.name())).await;
    run(
        &admin,
        &format!("GRANT {} TO {}", owner.name(), borrower.name()),
    )
    .await;
    run(
        &migrator,
        &format!(
            "GRANT USAGE ON SCHEMA {} TO {}",
            owner.name(),
            borrower.name()
        ),
    )
    .await;
    run(
        &migrator,
        &format!(
            "GRANT SELECT, INSERT, UPDATE, DELETE ON {TABLE} TO {}",
            borrower.name()
        ),
    )
    .await;
    let borrowed = SqlPool::connect(&connect(&borrower))
        .await
        .expect("the borrower connects");
    match SqlTupleStore::new(borrowed.clone())
        .verify_runtime_role()
        .await
    {
        Err(AuthzError::TupleRoleTooBroad { excess }) => {
            assert!(excess.starts_with("owns the table"), "{excess}");
        }
        other => panic!("a member of the owner's role can become it: {other:?}"),
    }
    run(
        &admin,
        &format!("REVOKE {} FROM {}", owner.name(), borrower.name()),
    )
    .await;

    drop((authz, served, migrator, borrowed, admin));
    owner.release().await.expect("the owner is released");
    borrower.release().await.expect("the borrower is released");
    runtime
        .release()
        .await
        .expect("the runtime role is released");
}

/// A database without the table answers `AUTHZ_028`; a drifted one answers `AUTHZ_029` naming
/// every difference, and reading it changes nothing.
#[tokio::test]
#[expect(clippy::too_many_lines, reason = "readability baseline")]
async fn verify_schema_checks_without_ddl() {
    let empty = SqlSchema::mint().await.expect("a test schema");
    let missing = SqlTupleStore::new(empty.pool().clone())
        .verify_schema()
        .await;
    let error = missing.expect_err("no table");
    assert!(matches!(error, AuthzError::TupleTableMissing), "{error:?}");
    assert_eq!(error.error_code(), "AUTHZ_028");
    assert!(
        catalog(empty.pool()).await.is_none(),
        "the check created nothing"
    );

    // An app's own copy of the table, with the set registered over it afterwards.
    let copied = SqlSchema::mint().await.expect("a test schema");
    let pool = copied.pool();
    run(
        pool,
        "CREATE TABLE IF NOT EXISTS authz_relation_tuple (tuple_key TEXT NOT NULL, object_ns TEXT NOT NULL, \
         object_id TEXT NOT NULL, relation TEXT NOT NULL, subject_ns TEXT NOT NULL, \
         subject_id TEXT NOT NULL, subject_relation TEXT NOT NULL, caveat_name TEXT, \
         PRIMARY KEY (tuple_key))",
    )
    .await;
    create_index(
        pool,
        "authz_relation_tuple_forward_idx",
        "CREATE INDEX authz_relation_tuple_forward_idx \
         ON authz_relation_tuple (object_ns, object_id, relation)",
    )
    .await;
    support::migrate_tuple_table(pool).await;
    create_index(
        pool,
        "one_edge",
        "CREATE UNIQUE INDEX one_edge ON authz_relation_tuple ((object_ns, object_id) HASH)",
    )
    .await;
    run(
        pool,
        "CREATE OR REPLACE FUNCTION keep() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NEW; END $$",
    )
    .await;
    run(
        pool,
        "CREATE OR REPLACE TRIGGER rewrite_grants BEFORE INSERT ON authz_relation_tuple \
         FOR EACH ROW EXECUTE FUNCTION keep()",
    )
    .await;
    run(
        pool,
        "ALTER TABLE authz_relation_tuple ENABLE ROW LEVEL SECURITY",
    )
    .await;

    let before = catalog(pool).await;
    let problems = mismatch(SqlTupleStore::new(pool.clone()).verify_schema().await);
    assert_eq!(catalog(pool).await, before, "the check changed nothing");
    for wanted in [
        "column caveat_context is missing",
        "index authz_relation_tuple_forward_idx is CREATE INDEX",
        "index one_edge is not in the set",
        "trigger rewrite_grants is not in the set",
        "row-level security is enabled",
    ] {
        assert!(problems.contains(wanted), "no `{wanted}` in: {problems}");
    }
    // An undeclared single-column key is hashed by the engine, as the set's own is.
    assert!(
        !problems.contains("primary key"),
        "the copy's key reads back as the set's: {problems}"
    );
    assert!(
        !problems.contains("authz_relation_tuple_reverse_idx is missing"),
        "the set's reverse index was created over the copy: {problems}"
    );

    // The set's own table, then a partial forward index in place of its own.
    let partial = SqlSchema::mint().await.expect("a test schema");
    support::migrate_tuple_table(partial.pool()).await;
    create_index(
        partial.pool(),
        "authz_relation_tuple_forward_idx",
        "CREATE INDEX authz_relation_tuple_forward_idx ON authz_relation_tuple \
         ((object_ns, object_id, relation) HASH) WHERE object_ns = 'document'",
    )
    .await;
    assert_eq!(
        mismatch(
            SqlTupleStore::new(partial.pool().clone())
                .verify_schema()
                .await
        ),
        "index authz_relation_tuple_forward_idx is partial; the set's is not"
    );

    // The reverse index gone, and a foreign key elsewhere refusing a tuple's deletion.
    let pinned = SqlSchema::mint().await.expect("a test schema");
    support::migrate_tuple_table(pinned.pool()).await;
    run(
        pinned.pool(),
        "DROP INDEX IF EXISTS authz_relation_tuple_reverse_idx",
    )
    .await;
    run(
        pinned.pool(),
        "CREATE TABLE IF NOT EXISTS pins (k TEXT NOT NULL REFERENCES authz_relation_tuple (tuple_key), \
         PRIMARY KEY (k HASH))",
    )
    .await;
    let problems = mismatch(
        SqlTupleStore::new(pinned.pool().clone())
            .verify_schema()
            .await,
    );
    assert!(
        problems.starts_with("index authz_relation_tuple_reverse_idx is missing; constraint "),
        "{problems}"
    );
    assert!(
        problems.contains(" on pins references the table: FOREIGN KEY (k) REFERENCES"),
        "{problems}"
    );
}

/// The three refusals keep their codes, answer 500 and are not retried.
#[test]
fn the_schema_refusals_are_stable() {
    for (error, code) in [
        (AuthzError::TupleTableMissing, "AUTHZ_028"),
        (
            AuthzError::TupleTableMismatch {
                problems: "index x is missing".to_owned(),
            },
            "AUTHZ_029",
        ),
        (
            AuthzError::TupleRoleTooBroad {
                excess: "owns the table".to_owned(),
            },
            "AUTHZ_030",
        ),
    ] {
        assert_eq!(error.error_code(), code);
        assert_eq!(error.http_status_code(), 500, "{code}");
        assert!(!error.is_retryable(), "{code}");
    }
}

/// Applying the set twice changes nothing and the table passes the check.
#[tokio::test]
async fn the_set_is_idempotent_and_guarded() {
    let database = SqlSchema::mint().await.expect("a test schema");
    let pool = database.pool();
    assert_eq!(
        support::migrate_tuple_table(pool).await,
        ["fundamental_authz/0001_authz_relation_tuple"]
    );
    let first = catalog(pool).await.expect("the table exists");
    assert!(
        support::migrate_tuple_table(pool).await.is_empty(),
        "a second apply applies nothing"
    );
    assert_eq!(catalog(pool).await, Some(first), "the catalog is unchanged");

    let dir = tempfile::tempdir().expect("a temporary directory");
    let runner = support::runner(&dir);
    let applied = runner.applied_sql(pool).await.expect("the ledger");
    assert_eq!(
        applied
            .iter()
            .filter(|name| name.starts_with("fundamental_authz/"))
            .count(),
        1,
        "{applied:?}"
    );
    let check = runner.check_sql(pool).await.expect("nothing unapplied");
    let drift: Vec<_> = check
        .drift
        .iter()
        .filter(|drift| format!("{drift:?}").contains(TABLE))
        .collect();
    assert!(drift.is_empty(), "{drift:?}");
    assert_eq!(SQL_TUPLE_MIGRATIONS.owner(), "fundamental_authz");
    SqlTupleStore::new(pool.clone())
        .verify_schema()
        .await
        .expect("the migrated table");
}

/// The SQL backend's own code carries no DDL — the table is the migrator's, never the store's.
#[test]
fn the_store_never_issues_ddl() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/backend/sql");
    let mut offenders = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("the SQL backend's sources") {
        let path = entry.expect("an entry").path();
        let text = std::fs::read_to_string(&path).expect("a source file");
        let code = text.split("#[cfg(test)]").next().unwrap_or_default();
        for needle in [
            "execute_ddl",
            "CREATE TABLE",
            "CREATE INDEX",
            "ALTER TABLE",
            "DROP TABLE",
            "DROP INDEX",
            "create_table_sql",
        ] {
            if code.contains(needle) {
                offenders.push(format!("{}: {needle}", path.display()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "runtime DDL in the SQL store: {offenders:?}"
    );
}
