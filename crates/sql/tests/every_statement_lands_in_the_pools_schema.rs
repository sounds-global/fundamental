//! Every generated statement addresses the schema its pool names, not `public`; requires Docker.

#![cfg(not(target_arch = "wasm32"))]

use fundamental_core::i18n::{Locale, Localized};
use fundamental_db::RepositoryTranslation;
use fundamental_db::migration::MigrationGuard;
use fundamental_db::{KeyedRepository, SchemaName};
use fundamental_macros::Model;
use fundamental_sql::runner::SqlMigrator;
use fundamental_sql::sqlx::Row;
use fundamental_sql::{SequenceRef, SqlConfig, SqlPool, SqlSliceStore, WriteOutcome};
use fundamental_sync::{SliceStore, SliceWriteStore, SyncMeta, SyncRecord};
use fundamental_test_containers::sql::SqlSchema;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Model)]
#[model(sql, table = "catalogue_items")]
struct CatalogueItem {
    #[model(id)]
    id: String,
    label: Localized<String>,
}

#[derive(Clone, Debug, PartialEq, Model)]
#[model(sql, table = "plain_rows")]
struct PlainRow {
    #[model(id)]
    id: i64,
    note: String,
}

#[derive(Clone, Debug, PartialEq, Model)]
#[model(sql, table = "seq_rows")]
struct SeqRow {
    #[model(id)]
    idx: i64,
    commitment: String,
}

/// A table in the shared host `public`, named so no model's generated statement can address it.
const SEARCH_PATH_PROBE: &str = "fundamental_search_path_probe";

async fn pool_in(url: &str, schema: &SchemaName) -> SqlPool {
    SqlPool::connect(&SqlConfig::development(url).default_schema(schema.clone()))
        .await
        .expect("a pool in the named schema")
}

/// A freshly created, empty schema on a shared host database and a pool that names it; hold the [`SqlSchema`].
async fn scratch(_name: &str) -> (SqlSchema, SchemaName, SqlPool) {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let name = schema.schema().clone();
    let pool = pool_in(schema.url(), &name).await;
    (schema, name, pool)
}

#[tokio::test]
async fn the_derives_insert_writes_where_its_connection_points() {
    let (database, schema, pool) = scratch("hole_a").await;
    pool.execute_ddl(&format!(
        "CREATE TABLE {schema}.plain_rows (id BIGINT PRIMARY KEY, note TEXT NOT NULL)"
    ))
    .await
    .expect("the table, in the scratch schema only");

    PlainRow {
        id: 1,
        note: "through the derive".to_owned(),
    }
    .insert(&pool)
    .await
    .expect("the generated insert");

    assert_eq!(
        pool.repository::<PlainRow>()
            .find(&1)
            .await
            .expect("read back")
            .map(|row| row.note),
        Some("through the derive".to_owned()),
        "the write must land where the qualified read looks"
    );
    // No model's table is ever written to the shared host `public`, so this cannot meet another test's row.
    let public = pool_in(database.url(), &SchemaName::default()).await;
    assert!(
        public.repository::<PlainRow>().find(&1).await.is_err(),
        "a pool on `public` must not find a table it does not have"
    );
}

#[tokio::test]
async fn a_translatable_model_writes_and_reads_in_its_pools_schema() {
    let (_database, schema, pool) = scratch("hole_b").await;
    let companion = <CatalogueItem as fundamental_db::migration::ModelSchema<
        fundamental_core::model::SqlDb,
    >>::translation_schema()
    .expect("a localized record");
    for ddl in std::iter::once(CatalogueItem::create_table_sql().to_owned())
        .chain(fundamental_db::migration::render::create_all(&companion))
    {
        pool.execute_ddl(&ddl).await.expect("the tables");
    }

    let item = CatalogueItem {
        id: "i1".to_owned(),
        label: Localized::new("Headphones").with("nl", "Koptelefoon"),
    };
    item.insert(&pool).await.expect("insert + companion rows");

    let loaded = CatalogueItem::find(&pool, &"i1".to_owned())
        .await
        .expect("find")
        .expect("the row exists");
    let nl: Locale = "nl".parse().expect("a locale");
    assert_eq!(loaded.label.resolve(&nl), "Koptelefoon");

    assert_eq!(
        CatalogueItem::find_ids_by_translation(&pool, "label", "nl", "%telefoon%")
            .await
            .expect("search the companion table"),
        ["i1".to_owned()]
    );
    assert_eq!(
        pool.repository::<CatalogueItem>()
            .find_ids_by_translation("label", "nl", "%telefoon%")
            .await
            .expect("the repository's own companion search"),
        ["i1".to_owned()],
        "a repository must search the companion table in its own schema"
    );

    CatalogueItem {
        id: "i1".to_owned(),
        label: Localized::new("Headphones"),
    }
    .upsert(&pool)
    .await
    .expect("upsert rewrites the companion rows");
    let count: i64 = sqlx::Row::get(
        &pool
            .fetch_one(&format!(
                "SELECT count(*) FROM {schema}.catalogue_items_i18n"
            ))
            .await
            .expect("count the companion rows"),
        0,
    );
    assert_eq!(count, 0, "the dropped locale's row must be gone");
}

#[tokio::test]
async fn a_sequence_counter_and_its_model_share_one_schema() {
    let (_database, schema, pool) = scratch("hole_c").await;
    pool.execute_ddl(&format!(
        "CREATE TABLE {schema}.seq_rows (idx BIGINT PRIMARY KEY, commitment TEXT NOT NULL UNIQUE)"
    ))
    .await
    .expect("the model table");
    pool.execute_ddl(&format!(
        "CREATE TABLE {schema}.seq_counter (id BIGINT PRIMARY KEY, next_val BIGINT NOT NULL)"
    ))
    .await
    .expect("the counter table");
    pool.execute(&format!(
        "INSERT INTO {schema}.seq_counter (id, next_val) VALUES (0, 0)"
    ))
    .await
    .expect("the counter row");

    let sequence = SequenceRef {
        table: "seq_counter",
        key_column: "id",
        key: 0,
        next_column: "next_val",
    };
    let (row, outcome) = pool
        .repository::<SeqRow>()
        .insert_or_get_sequenced(
            &SeqRow {
                idx: 999,
                commitment: "aa".to_owned(),
            },
            "commitment",
            &sequence,
        )
        .await
        .expect("the sequenced insert reaches the counter in this schema");

    assert_eq!(outcome, WriteOutcome::Inserted);
    assert_eq!(row.idx, 0, "the index comes from this schema's counter");
}

#[tokio::test]
async fn a_slice_store_reads_and_writes_the_schema_its_pool_names() {
    let (_database, schema, pool) = scratch("hole_d").await;
    pool.execute_ddl(&format!(
        "CREATE TABLE {schema}.notes (\
             id UUID PRIMARY KEY, \
             body TEXT, \
             updated_at TIMESTAMPTZ NOT NULL DEFAULT now(), \
             version BIGINT NOT NULL DEFAULT 1, \
             _tombstone BOOLEAN NOT NULL DEFAULT false)"
    ))
    .await
    .expect("a syncable table, in the scratch schema only");

    let store = SqlSliceStore::new(&pool);

    assert!(
        SliceStore::table_exists(&store, "notes")
            .await
            .expect("introspect"),
        "the store must see the table in its own schema"
    );

    let id = uuid::Uuid::new_v4();
    let mut payload = BTreeMap::new();
    payload.insert("body".to_owned(), "first".to_owned());
    let record = SyncRecord {
        id: fundamental_db::RecordKey::single(id.to_string()),
        meta: SyncMeta {
            updated_at: 0,
            version: 0,
            tombstone: false,
        },
        payload,
    };
    SliceWriteStore::upsert(&store, "notes", &["body"], &["id"], &record)
        .await
        .expect("the push writes into this schema");

    // The `ON CONFLICT … DO UPDATE SET` branch, where the bare relation name is correct.
    let mut second = record.clone();
    second
        .payload
        .insert("body".to_owned(), "second".to_owned());
    SliceWriteStore::upsert(&store, "notes", &["body"], &["id"], &second)
        .await
        .expect("the conflicting push updates rather than failing");

    let rows = store
        .changed_since("notes", &["body"], &["id"], &[], 0)
        .await
        .expect("the pull reads from this schema");
    assert_eq!(rows.len(), 1, "one row, updated in place");
    assert_eq!(
        rows[0].payload.get("body").map(String::as_str),
        Some("second")
    );
    assert_eq!(
        rows[0].meta.version, 2,
        "the self-reference bumped the version"
    );
}

#[tokio::test]
async fn a_migration_ledger_is_per_schema_by_design() {
    // Every schema this process mints lands in the same host database.
    let one_schema = SqlSchema::mint().await.expect("the first test schema");
    let two_schema = SqlSchema::mint().await.expect("the second test schema");
    let first = one_schema.schema().clone();
    let second = two_schema.schema().clone();
    assert_eq!(
        one_schema.url(),
        two_schema.url(),
        "both schemas must share a database, or this proves nothing about one ledger per schema"
    );
    let database_url = one_schema.url().to_owned();

    let migrations = [(
        "0001_widgets",
        "CREATE TABLE widgets (id BIGINT PRIMARY KEY);",
    )];

    let one = pool_in(&database_url, &first).await;
    let applied = SqlMigrator::new(MigrationGuard::unenforced())
        .apply_all(&one, &migrations)
        .await
        .expect("apply in the first schema");
    assert_eq!(applied.len(), 1);

    let two = pool_in(&database_url, &second).await;
    let applied = SqlMigrator::new(MigrationGuard::unenforced())
        .apply_all(&two, &migrations)
        .await
        .expect("apply in the second schema");
    assert_eq!(
        applied.len(),
        1,
        "a second schema must apply the migration rather than read the first's ledger"
    );

    two.execute(&format!("INSERT INTO {second}.widgets (id) VALUES (1)"))
        .await
        .expect("the second schema really has the table");
}

/// The pool's `search_path` is the scratch schema alone, with no `public` fallback; never add one.
#[tokio::test]
async fn a_scratch_schema_cannot_see_the_shared_tables_behind_it() {
    let scratch = SqlSchema::mint().await.expect("a test schema");
    let host = pool_in(scratch.url(), &SchemaName::default()).await;

    // Idempotent, so runs reuse it; not a model's table, so sibling assertions cannot meet it.
    host.execute_ddl(&format!(
        "CREATE TABLE IF NOT EXISTS public.{SEARCH_PATH_PROBE} \
         (id BIGINT PRIMARY KEY, note TEXT NOT NULL)"
    ))
    .await
    .expect("the shared table");
    host.execute(&format!(
        "INSERT INTO public.{SEARCH_PATH_PROBE} (id, note) VALUES (1, 'the shared row') \
         ON CONFLICT (id) DO NOTHING"
    ))
    .await
    .expect("a row nobody's fixture wrote");

    let pool = pool_in(scratch.url(), scratch.schema()).await;

    let error = pool
        .fetch_one(&format!("SELECT count(*) FROM {SEARCH_PATH_PROBE}"))
        .await
        .expect_err("an empty scratch schema must not fall through to `public`");
    assert!(
        format!("{error:?}").contains("does not exist"),
        "the failure must be a missing relation, not a count of somebody else's rows: {error:?}"
    );

    // The table is really there, so the assertion above cannot pass vacuously.
    let count: i64 = host
        .fetch_one(&format!("SELECT count(*) FROM public.{SEARCH_PATH_PROBE}"))
        .await
        .expect("the shared table is readable when the statement names its schema")
        .try_get(0)
        .expect("a count is a number");
    assert!(
        count >= 1,
        "the stand-in table must hold the row the scratch schema is not allowed to see"
    );
}
