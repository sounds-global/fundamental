//! Compile-and-behaviour checks for the `#[derive(Model)]`-generated YSQL code, without a live database.

#![allow(dead_code)]
#![expect(
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]

use crate::ToRow;
use fundamental_core::secrets::{InMemorySecretString, SecretBytes, SecretJson, SecretString};
use fundamental_db::SchemaName;
use fundamental_macros::{Identifier, Model};

/// A strongly-typed id; inside this crate the `Identifier` derive emits its sqlx bindings directly.
#[derive(Clone, PartialEq, Eq, Hash, Identifier)]
struct WidgetId(uuid::Uuid);

/// A model keyed on a `#[derive(Identifier)]` newtype.
#[derive(Clone, Model)]
#[model(sql)]
struct TypedWidget {
    #[model(id)]
    id: WidgetId,
    name: String,
}

/// A UUID-keyed model, whose primary key is bound as `uuid`; `table` is inferred as `widgets`.
#[derive(Clone, Model)]
#[model(sql)]
struct Widget {
    #[model(id)]
    id: uuid::Uuid,
    name: String,
    count: i64,
}

/// An integer-keyed model — verifies a non-UUID native key type also compiles.
#[derive(Clone, Model)]
#[model(sql)]
struct Counter {
    #[model(id)]
    id: i64,
    label: String,
}

/// A soft-deletable model, whose `deleted_at` column is in the DDL but not in `columns()`.
#[derive(Clone, Model)]
#[model(sql, soft_delete)]
struct SoftWidget {
    #[model(id)]
    id: uuid::Uuid,
    name: String,
}

/// An `InMemorySecret` field is accepted and skipped like `#[model(skip)]`, so it is never persisted.
#[derive(Clone, Model)]
#[model(sql)]
struct SessionRow {
    #[model(id)]
    id: uuid::Uuid,
    label: String,
    decrypted: InMemorySecretString,
}

/// A model with persisted `Secret` columns across all three at-rest shapes.
#[derive(Clone, Model)]
#[model(sql, table = "secret_rows")]
struct SecretRow {
    #[model(id)]
    id: uuid::Uuid,
    token: SecretString, // → key "default"
    #[model(encrypted = "pii", column = "ssn")]
    ssn: SecretString, // → key "pii", column "ssn"
    #[model(encrypted = "pii")]
    recovery_key: SecretBytes, // binary, → key "pii"
    #[model(encrypted = "pii")]
    profile: SecretJson<Vec<String>>, // structured, → key "pii"
}

/// Type-checks the generated query methods, passing ids by native type; never executed.
async fn _typecheck_generated_methods(pool: &crate::SqlPool, wid: uuid::Uuid, cid: i64) {
    let _ = Widget::find(pool, &wid).await;
    let _ = Widget::delete(pool, &wid).await;
    let _builder = Widget::update(pool, &wid).set("name", "x");
    let _ = Widget::query(pool);

    let _ = Counter::find(pool, &cid).await;
    let _ = Counter::delete(pool, &cid).await;
    let _builder = Counter::update(pool, &cid).set("label", "y");
}

#[test]
fn to_row_columns_are_generated() {
    assert_eq!(Widget::columns(), &["id", "name", "count"]);
    assert_eq!(Widget::columns_without_pk(), &["name", "count"]);
}

#[test]
fn in_memory_secret_field_is_skipped_from_persistence() {
    assert_eq!(SessionRow::columns(), &["id", "label"]);
    assert_eq!(SessionRow::columns_without_pk(), &["label"]);
}

#[test]
fn secret_columns_are_persisted_and_reported() {
    use fundamental_core::Model;

    assert_eq!(
        <SecretRow as Model>::secret_columns(),
        &[
            ("token", "default"),
            ("ssn", "pii"),
            ("recovery_key", "pii"),
            ("profile", "pii"),
        ],
    );

    assert_eq!(
        SecretRow::columns(),
        &["id", "token", "ssn", "recovery_key", "profile"],
    );
    assert_eq!(
        SecretRow::columns_without_pk(),
        &["token", "ssn", "recovery_key", "profile"],
    );
}

#[test]
#[allow(clippy::assertions_on_constants)]
#[expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]
fn soft_delete_const_and_schema() {
    use fundamental_core::model::SqlModel;

    assert!(!<Widget as SqlModel>::SOFT_DELETE);

    assert!(<SoftWidget as SqlModel>::SOFT_DELETE);
    assert_eq!(<SoftWidget as SqlModel>::DELETED_AT_COLUMN, "deleted_at");

    assert_eq!(SoftWidget::columns(), &["id", "name"]);
    assert_eq!(SoftWidget::columns_without_pk(), &["name"]);

    let ddl = SoftWidget::create_table_sql();
    assert!(ddl.contains("deleted_at TIMESTAMPTZ"), "DDL was: {ddl}");
    assert!(
        !ddl.contains("deleted_at TIMESTAMPTZ NOT NULL"),
        "DDL was: {ddl}"
    );
}

#[test]
fn soft_delete_query_filtering() {
    use crate::build_select_sql;
    use fundamental_core::model::SqlDb;
    use fundamental_db::QueryBuilder;

    let query = QueryBuilder::<SoftWidget, SqlDb>::new();
    let (sql, _values) =
        build_select_sql::<SoftWidget, _>(&SchemaName::default(), &query, None).unwrap();
    assert!(
        sql.contains("WHERE deleted_at IS NULL"),
        "expected soft-delete predicate, got: {sql}"
    );

    let query = QueryBuilder::<Widget, SqlDb>::new();
    let (sql, _values) =
        build_select_sql::<Widget, _>(&SchemaName::default(), &query, None).unwrap();
    assert!(
        !sql.contains("deleted_at"),
        "unexpected predicate, got: {sql}"
    );
}

#[test]
fn bind_column_for_batch_binds_each_non_pk_column() {
    let w = Widget {
        id: uuid::Uuid::nil(),
        name: "hello".to_string(),
        count: 7,
    };
    let q = sqlx::query("SELECT 1");
    let q = w.bind_column_for_batch(q, 0); // binds `name`
    let q = w.bind_column_for_batch(q, 1); // binds `count`
    let q = w.bind_column_for_batch(q, 99); // out-of-range → unchanged
    let _ = q;
}

/// A composite-keyed model; the zone is ordered, since hashing a near-constant column puts a cell on one tablet.
#[derive(Clone, Model)]
#[model(sql, table = "sharded_widgets")]
struct ShardedWidget {
    #[model(id, shard = asc)]
    zone: String,
    #[model(id, shard = hash)]
    widget_id: String,
    name: String,
}

/// A model that says nothing about how its key spreads.
#[derive(Clone, Model)]
#[model(sql, table = "undeclared_widgets")]
struct UndeclaredWidget {
    #[model(id)]
    id: uuid::Uuid,
}

#[test]
fn a_declared_key_carries_its_sharding_into_the_ddl() {
    let ddl = ShardedWidget::create_table_sql();

    assert!(
        ddl.contains("PRIMARY KEY (zone ASC, widget_id HASH)"),
        "each key column keeps the sharding its field declared: {ddl}"
    );
}

#[test]
fn an_undeclared_key_is_rendered_as_written_not_as_the_engine_will_store_it() {
    // The engine stores this as `(id HASH)`, but the DDL must not claim what the model does not say.
    let ddl = UndeclaredWidget::create_table_sql();

    assert!(ddl.contains("PRIMARY KEY (id)"), "{ddl}");
    assert!(!ddl.contains("HASH"), "{ddl}");
}

/// A model whose columns are a typed id, a closed value and a day.
#[derive(Clone, Model)]
#[model(sql, table = "typed_columns")]
struct TypedColumns {
    #[model(id, shard = hash)]
    id: WidgetId,
    #[model(variant)]
    side: Side,
    on: Option<chrono::NaiveDate>,
    owner: Option<WidgetId>,
}

#[derive(Clone, Copy, PartialEq, Eq, fundamental_macros::Variant)]
enum Side {
    Left,
}

#[test]
fn a_column_s_sql_type_is_its_rust_type_s() {
    use fundamental_core::model::SqlDb;
    use fundamental_db::migration::ModelSchema;

    let table = <TypedColumns as ModelSchema<SqlDb>>::table_schema();
    let typed = |column: &str| {
        table
            .columns
            .get(column)
            .map(|column| (column.data_type.clone(), column.nullable))
    };
    assert_eq!(
        typed("id"),
        Some(("UUID".to_owned(), false)),
        "an id is its inner type"
    );
    assert_eq!(
        typed("side"),
        Some(("TEXT".to_owned(), false)),
        "a closed value is text"
    );
    assert_eq!(
        typed("on"),
        Some(("DATE".to_owned(), true)),
        "a day is a date"
    );
    assert_eq!(
        typed("owner"),
        Some(("UUID".to_owned(), true)),
        "an optional id too"
    );
}

/// An edition: two hashed columns, then the newest version first.
#[derive(Clone, Model)]
#[model(sql, table = "split_editions")]
struct SplitEdition {
    #[model(id, shard = hash)]
    organisation_id: uuid::Uuid,
    #[model(id, shard = hash)]
    document: String,
    #[model(id, shard = desc)]
    version: i32,
    text: String,
}

#[test]
fn a_key_keeps_how_each_part_is_split() {
    use fundamental_core::model::SqlDb;
    use fundamental_core::model::sql::ShardKind;
    use fundamental_db::migration::{KeyPart, ModelSchema};

    let parts = |columns: &[&str], split| KeyPart {
        columns: columns.iter().map(|column| (*column).to_owned()).collect(),
        split,
        nulls: None,
    };
    assert_eq!(
        <SplitEdition as ModelSchema<SqlDb>>::table_schema().key,
        vec![
            parts(&["organisation_id", "document"], ShardKind::Hash),
            parts(&["version"], ShardKind::Desc),
        ],
        "hashed columns together, as one part"
    );
    assert_eq!(
        <Counter as ModelSchema<SqlDb>>::table_schema().key,
        vec![parts(&["id"], ShardKind::Hash)],
        "an id that says nothing is split as the engine splits it: hashed first"
    );
}

/// A portfolio's projects: one address per organisation among the projects not deleted, and a
/// list by name that reads the stage without the row.
#[derive(Clone, Model)]
#[model(
    sql,
    table = "indexed_projects",
    soft_delete,
    index(
        name = "indexed_projects_by_slug",
        columns = ["organisation_id hash", "slug asc"],
        unique,
        partial = "deleted_at IS NULL"
    ),
    index(
        name = "indexed_projects_by_name",
        columns = ["organisation_id", "name desc"],
        include = ["stage"]
    )
)]
struct IndexedProject {
    #[model(id, shard = hash)]
    organisation_id: uuid::Uuid,
    #[model(id, shard = asc)]
    id: uuid::Uuid,
    slug: String,
    name: String,
    stage: String,
}

#[test]
fn an_index_says_its_split_uniqueness_condition_and_covered_columns() {
    use fundamental_core::model::SqlDb;
    use fundamental_core::model::sql::ShardKind;
    use fundamental_db::migration::{KeyPart, ModelSchema};

    let part = |column: &str, split| KeyPart {
        columns: vec![column.to_owned()],
        split,
        nulls: None,
    };
    let table = <IndexedProject as ModelSchema<SqlDb>>::table_schema();
    let by_slug = table
        .indexes
        .get("indexed_projects_by_slug")
        .expect("declared");
    assert_eq!(
        by_slug.key,
        vec![
            part("organisation_id", ShardKind::Hash),
            part("slug", ShardKind::Asc)
        ]
    );
    assert!(by_slug.unique);
    assert_eq!(by_slug.condition.as_deref(), Some("deleted_at IS NULL"));
    assert_eq!(by_slug.columns, ["organisation_id", "slug"]);

    let by_name = table
        .indexes
        .get("indexed_projects_by_name")
        .expect("declared");
    assert_eq!(
        by_name.key,
        vec![
            part("organisation_id", ShardKind::Hash),
            part("name", ShardKind::Desc)
        ],
        "a column that says nothing is split as the engine splits it"
    );
    assert!(!by_name.unique);
    assert_eq!(by_name.include, ["stage"]);
    assert_eq!(
        IndexedProject::INDEXED_PROJECTS_BY_NAME.columns(),
        &["organisation_id", "name"],
        "a query pins the index by its columns alone"
    );
}

/// A team's organisation: one row, its place fixed by a check, its name empty until given.
#[derive(Clone, Model)]
#[model(
    sql,
    table = "checked_organisations",
    check(
        name = "checked_organisations_one",
        expr = "id = '00000000-0000-0000-0000-000000000001'"
    )
)]
struct CheckedOrganisation {
    #[model(id, shard = hash)]
    id: uuid::Uuid,
    #[model(default = "''")]
    name: String,
}

#[test]
fn a_model_declares_its_defaults_and_checks() {
    use fundamental_core::model::SqlDb;
    use fundamental_db::migration::ModelSchema;
    use fundamental_db::migration::render::create_table;

    let table = <CheckedOrganisation as ModelSchema<SqlDb>>::table_schema();
    assert_eq!(
        table
            .columns
            .get("name")
            .and_then(|column| column.default.as_deref()),
        Some("''")
    );
    let ddl = create_table(&table);
    assert!(ddl.contains("    name TEXT NOT NULL DEFAULT ''"), "{ddl}");
    assert!(
        ddl.contains(
            "    CONSTRAINT checked_organisations_one CHECK (id = '00000000-0000-0000-0000-000000000001')"
        ),
        "{ddl}"
    );
}

/// An article whose headline became its title, and whose body, its index and its check went.
#[derive(Clone, Model)]
#[model(
    sql,
    table = "moved_articles",
    retired(
        columns = ["body"],
        indexes = ["moved_articles_by_body"],
        checks = ["moved_articles_body"]
    )
)]
struct MovedArticle {
    #[model(id, shard = hash)]
    id: uuid::Uuid,
    #[model(was = "headline")]
    title: String,
}

#[test]
fn a_model_says_what_it_renamed_and_retired() {
    use fundamental_core::model::SqlDb;
    use fundamental_db::migration::ModelSchema;

    let table = <MovedArticle as ModelSchema<SqlDb>>::table_schema();
    assert_eq!(
        table.renamed.get("title").map(String::as_str),
        Some("headline")
    );
    assert_eq!(table.retired.columns, ["body"]);
    assert_eq!(table.retired.indexes, ["moved_articles_by_body"]);
    assert_eq!(table.retired.checks, ["moved_articles_body"]);
}

/// A customer whose code is unique per issuer, as a hand-written `CONSTRAINT … UNIQUE` made it.
#[derive(Clone, Model)]
#[model(
    sql,
    table = "constrained_customers",
    index(
        name = "constrained_customers_code_unique",
        columns = ["issuer_id hash", "code asc"],
        unique,
        constraint
    )
)]
struct ConstrainedCustomer {
    #[model(id, shard = hash)]
    id: uuid::Uuid,
    issuer_id: uuid::Uuid,
    code: String,
}

#[test]
fn a_model_says_which_unique_index_a_constraint_made() {
    use fundamental_core::model::SqlDb;
    use fundamental_db::migration::ModelSchema;

    let table = <ConstrainedCustomer as ModelSchema<SqlDb>>::table_schema();
    let index = &table.indexes["constrained_customers_code_unique"];
    assert!(index.unique && index.constraint, "{index:?}");
}
