//! In a colocated database, a model's placement and `written_by` statements run and read back as
//! declared. Requires Docker; one test, as `CREATE DATABASE` is global-impact DDL.

use fundamental_core::model::sql::ShardKind;
use fundamental_db::migration::generate::{Snapshot, next_statements};
use fundamental_db::migration::{KeyPart, Lints, TableColumn, TableSchema};
use fundamental_sql::migration::TableCatalog;
use fundamental_sql::{SqlConfig, SqlPool};
use fundamental_test_containers::sql::SqlNode;

fn table(name: &str, split: ShardKind) -> TableSchema {
    let mut table = TableSchema::new(name);
    for (column, data_type) in [("id", "UUID"), ("note", "TEXT")] {
        table.add_column(TableColumn {
            name: column.to_owned(),
            data_type: data_type.to_owned(),
            nullable: column == "note",
            default: None,
            primary_key: column == "id",
            unique: false,
        });
    }
    table.set_key(KeyPart::parts(&[("id", Some(split))]));
    table
}

#[tokio::test]
async fn a_table_out_of_colocation_is_split_and_written_as_its_model_says() {
    let node = SqlNode::shared().await.expect("the shared node");
    let name = format!("placed_{}", uuid::Uuid::new_v4().simple());
    let admin = SqlPool::connect(&SqlConfig::development(node.bootstrap_url().as_str()))
        .await
        .expect("the bootstrap database");
    // A retried `CREATE DATABASE` can find the database its first attempt made.
    if let Err(error) = admin
        .execute_ddl(&format!("CREATE DATABASE {name} WITH COLOCATION = true"))
        .await
    {
        assert!(
            error.to_string().contains("already exists"),
            "a colocated database: {error:?}"
        );
    }
    let pool = SqlPool::connect(&SqlConfig::development(node.url(&name).as_str()))
        .await
        .expect("the colocated database");

    let mut apart = table("placed_events", ShardKind::Hash);
    apart.set_colocated(false);
    apart.set_tablets(4);
    apart.add_writer("placed_importer");
    // A colocated table's key cannot be hashed, so this one is split in order.
    let together = table("placed_kinds", ShardKind::Asc);
    let statements = next_statements(
        &Snapshot::default(),
        &Snapshot::of(&[apart, together], None),
        &Lints::default().colocated(true),
    )
    .expect("written");
    for statement in &statements {
        pool.execute_ddl(statement)
            .await
            .unwrap_or_else(|error| panic!("{statement}: {error}"));
    }

    let events = TableCatalog::read(&pool, "placed_events")
        .await
        .expect("read")
        .expect("the table");
    let kinds = TableCatalog::read(&pool, "placed_kinds")
        .await
        .expect("read")
        .expect("the table");
    drop(pool);
    admin
        .execute_ddl(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"))
        .await
        .expect("dropped");

    assert!(!events.colocated, "{statements:#?}");
    assert_eq!(events.tablets, Some(4));
    assert!(
        events.writers.contains("placed_importer"),
        "{:?}",
        events.writers
    );
    assert!(
        kinds.colocated,
        "a table that says nothing shares the tablet"
    );
}
