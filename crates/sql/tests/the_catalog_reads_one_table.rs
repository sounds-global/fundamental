//! `TableCatalog` reads one table from `pg_catalog`, finding only the table the search path resolves; requires Docker.

use fundamental_db::migration::ddl::{KeyElement, Sharding};
use fundamental_sql::migration::TableCatalog;
use fundamental_test_containers::sql::SqlSchema;

/// Through `execute_ddl`, so every statement below is written to be re-issued.
async fn run(schema: &SqlSchema, statement: &str) {
    schema
        .pool()
        .execute_ddl(statement)
        .await
        .unwrap_or_else(|error| panic!("{statement}: {error}"));
}

fn element(columns: &[&str], sharding: Sharding) -> KeyElement {
    KeyElement::new(
        columns.iter().map(|c| (*c).to_owned()).collect(),
        Some(sharding),
    )
}

#[tokio::test]
#[expect(clippy::too_many_lines, reason = "readability baseline")]
async fn the_catalog_reads_one_table() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    for statement in [
        "CREATE TABLE IF NOT EXISTS gadgets (id TEXT NOT NULL, kind TEXT NOT NULL, label TEXT COLLATE \"C\", \
         made BIGINT DEFAULT 1, PRIMARY KEY (id HASH), CONSTRAINT gadgets_made CHECK (made > 0))",
        "CREATE INDEX IF NOT EXISTS gadgets_by_kind ON gadgets ((kind, label) HASH, made ASC)",
        "CREATE UNIQUE INDEX IF NOT EXISTS gadgets_one_label ON gadgets (label HASH) WHERE kind = 'x'",
        "CREATE OR REPLACE FUNCTION keep() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NEW; END $$",
        "CREATE OR REPLACE TRIGGER gadgets_keep BEFORE INSERT ON gadgets FOR EACH ROW EXECUTE FUNCTION keep()",
        "ALTER TABLE gadgets ENABLE ROW LEVEL SECURITY",
        "DO $$ BEGIN CREATE POLICY gadgets_mine ON gadgets USING (kind = current_user); \
         EXCEPTION WHEN duplicate_object THEN NULL; END $$",
        "CREATE TABLE IF NOT EXISTS holds (gadget TEXT NOT NULL REFERENCES gadgets (id), \
         PRIMARY KEY (gadget HASH))",
    ] {
        run(&schema, statement).await;
    }

    let catalog = TableCatalog::read(schema.pool(), "gadgets")
        .await
        .expect("the catalog reads")
        .expect("the table is on the search path");

    let names: Vec<&str> = catalog.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["id", "kind", "label", "made"]);
    let label = catalog.column("label").expect("label");
    assert_eq!(
        (
            label.data_type.as_str(),
            label.nullable,
            label.collation.as_deref()
        ),
        ("TEXT", true, Some("C"))
    );
    let made = catalog.column("made").expect("made");
    assert_eq!(made.data_type, "INT8");
    assert_eq!(made.default.as_deref(), Some("1"));
    assert_eq!(made.collation, None, "a number does not collate");
    assert_eq!(
        catalog.column("id").and_then(|c| c.collation.as_deref()),
        Some("default")
    );

    let primary = catalog.primary_key().expect("a primary key");
    assert!(primary.unique && !primary.partial);
    assert_eq!(
        primary.key.as_ref().map(|key| key.elements().to_vec()),
        Some(vec![element(&["id"], Sharding::Hash)])
    );
    let by_kind = catalog.index("gadgets_by_kind").expect("the ranged index");
    assert_eq!(
        by_kind.key.as_ref().map(|key| key.elements().to_vec()),
        Some(vec![
            element(&["kind", "label"], Sharding::Hash),
            element(&["made"], Sharding::Asc),
        ])
    );
    assert!(!by_kind.unique && !by_kind.partial && !by_kind.primary);
    assert_eq!(by_kind.method, "lsm");
    let one_label = catalog
        .index("gadgets_one_label")
        .expect("the partial index");
    assert!(one_label.unique && one_label.partial);

    let kinds: Vec<(char, &str)> = catalog
        .constraints
        .iter()
        .map(|c| (c.kind, c.name.as_str()))
        .collect();
    assert!(kinds.contains(&('c', "gadgets_made")), "{kinds:?}");
    assert!(kinds.contains(&('p', "gadgets_pkey")), "{kinds:?}");
    let referencing = catalog
        .constraints
        .iter()
        .find(|c| c.kind == 'f')
        .expect("the foreign key that references the table");
    assert_eq!(referencing.table, "holds");
    assert!(
        catalog.indexes.iter().all(|index| index.usable),
        "every index finished building"
    );
    assert!(!catalog.partitioned && catalog.inheritance.is_empty());
    assert!(
        !catalog.colocated,
        "the table is split across its own tablets"
    );
    assert_eq!(catalog.triggers, ["gadgets_keep"]);
    assert!(catalog.rules.is_empty());
    assert!(catalog.row_security && !catalog.row_security_forced);
    assert_eq!(catalog.policies, ["gadgets_mine"]);
    assert!(
        catalog.access.owns,
        "the schema's own role owns what it made"
    );

    assert_eq!(
        TableCatalog::read(schema.pool(), "no_such_table")
            .await
            .expect("the catalog reads"),
        None
    );
    let other = SqlSchema::mint().await.expect("a second test schema");
    assert_eq!(
        TableCatalog::read(other.pool(), "gadgets")
            .await
            .expect("the catalog reads"),
        None,
        "a table in another schema is not on this search path"
    );
}
