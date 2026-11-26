//! A model's `CREATE TABLE`, applied and read back by [`OfflineIntrospector`], diffs to nothing.

#![cfg(not(target_arch = "wasm32"))]

use fundamental_core::model::{Offline, OfflineModel};
use fundamental_db::migration::{
    DatabaseSchema, MigrationDatabase, ModelSchema, SchemaDiffer, SchemaIntrospector,
};
use fundamental_macros::Model;
use fundamental_offline::db::{OfflineConfig, OfflineDatabase};
use fundamental_offline::migration::OfflineIntrospector;

#[derive(Clone, Debug, Model)]
#[model(offline)]
struct Gadget {
    #[model(id)]
    id: String,
    count: i64,
    weight: f64,
    label: Option<String>,
}

#[tokio::test]
async fn introspected_schema_matches_the_model() {
    let db = OfflineDatabase::open(OfflineConfig::memory())
        .await
        .expect("in-memory database opens");

    db.execute_batch(<Gadget as OfflineModel>::create_table_sql())
        .expect("create the model's table");

    let actual = OfflineIntrospector::introspect(&db)
        .await
        .expect("introspect the live schema");

    let mut expected = DatabaseSchema::for_database(MigrationDatabase::OfflineDb);
    expected.add_table(<Gadget as ModelSchema<Offline>>::table_schema());

    let changes = SchemaDiffer::new().diff(&expected, &actual);
    assert!(
        changes.is_empty(),
        "model and freshly-created table must match exactly, found: {changes:?}"
    );
}

#[tokio::test]
async fn dropping_a_column_in_the_model_is_detected_as_drift() {
    let db = OfflineDatabase::open(OfflineConfig::memory())
        .await
        .expect("in-memory database opens");
    db.execute_batch(
        "CREATE TABLE gadget (\
            id TEXT NOT NULL,\
            count INTEGER NOT NULL,\
            weight REAL NOT NULL,\
            label TEXT,\
            stale TEXT,\
            PRIMARY KEY (id))",
    )
    .expect("create a drifted table");

    let actual = OfflineIntrospector::introspect(&db)
        .await
        .expect("introspect");
    let mut expected = DatabaseSchema::for_database(MigrationDatabase::OfflineDb);
    expected.add_table(<Gadget as ModelSchema<Offline>>::table_schema());

    let changes = SchemaDiffer::new().diff(&expected, &actual);
    assert!(
        !changes.is_empty(),
        "an extra live column must be reported as drift against the model"
    );
}

/// A draft keyed by an integer, with a title in several languages.
#[derive(Clone, Debug, Model)]
#[model(offline)]
struct Draft {
    #[model(id)]
    id: i64,
    title: fundamental_core::i18n::Localized<String>,
}

#[test]
fn a_localized_models_companion_is_keyed_by_its_own_key_column() {
    let companion = <Draft as ModelSchema<Offline>>::translation_schema()
        .expect("a model with localized text has a companion");

    assert_eq!(companion.name, "draft_i18n");
    assert_eq!(companion.primary_key, ["id", "field", "locale"]);
    assert_eq!(companion.columns["id"].data_type, "INTEGER");
    assert!(<Gadget as ModelSchema<Offline>>::translation_schema().is_none());
}

#[tokio::test]
async fn the_companion_the_store_creates_is_the_one_the_model_expects() {
    let db = OfflineDatabase::open(OfflineConfig::memory())
        .await
        .expect("in-memory database opens");
    db.execute_batch(<Draft as OfflineModel>::create_table_sql())
        .expect("create the model's table");
    db.execute_batch(
        <Draft as OfflineModel>::translation_table_sql().expect("a localized model's companion"),
    )
    .expect("create the companion");

    let actual = OfflineIntrospector::introspect(&db)
        .await
        .expect("introspect the live schema");

    let mut expected = DatabaseSchema::for_database(MigrationDatabase::OfflineDb);
    expected.add_table(<Draft as ModelSchema<Offline>>::table_schema());
    expected.add_table(
        <Draft as ModelSchema<Offline>>::translation_schema().expect("a localized model"),
    );
    let changes = SchemaDiffer::new().diff(&expected, &actual);
    assert!(
        changes.is_empty(),
        "the companion is read back as the model defines it, found: {changes:?}"
    );
}
