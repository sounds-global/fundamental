//! The schema read back holds a covering index's carried columns, and names no index the planner
//! uses as unusable. Requires Docker.

use fundamental_db::migration::SchemaIntrospector as _;
use fundamental_sql::migration::SqlIntrospector;
use fundamental_test_containers::sql::SqlSchema;

#[tokio::test]
async fn a_covering_index_reads_back_with_its_carried_columns() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    for statement in [
        "CREATE TABLE IF NOT EXISTS notes (id UUID NOT NULL, kind TEXT NOT NULL, title TEXT, \
         body TEXT, PRIMARY KEY (id HASH))",
        "CREATE INDEX IF NOT EXISTS notes_by_kind ON notes (kind HASH) INCLUDE (title, body)",
    ] {
        schema
            .pool()
            .execute_ddl(statement)
            .await
            .unwrap_or_else(|error| panic!("{statement}: {error}"));
    }

    let read = SqlIntrospector::introspect(schema.pool())
        .await
        .expect("the schema reads");
    let index = &read.tables["notes"].indexes["notes_by_kind"];
    assert_eq!(index.include, ["title", "body"]);
    assert_eq!(index.columns, ["kind"]);

    let unusable = SqlIntrospector::unusable_indexes(schema.pool())
        .await
        .expect("the catalog reads");
    assert_eq!(unusable, Vec::<(String, String)>::new());
}
