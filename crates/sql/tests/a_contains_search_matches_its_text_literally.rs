//! `Filter::icontains` against a live table matches a person's search text as text; requires Docker.

use fundamental_core::model::SqlDb;
use fundamental_db::{
    DatabaseError, Filter, IsolationLevel, QueryBuilder, QueryExecutor as _,
    RepositoryTransaction as _,
};
use fundamental_macros::Model;
use fundamental_test_containers::sql::SqlSchema;

#[derive(Clone, Debug, PartialEq, Model)]
#[model(sql, table = "searched_notes")]
struct SearchedNote {
    #[model(id)]
    organisation_id: String,
    #[model(id)]
    note_id: String,
    body: String,
}

async fn found(schema: &SqlSchema, text: &str) -> Vec<String> {
    let query = QueryBuilder::<SearchedNote, SqlDb>::new()
        .pin_key("org-a")
        .filter(Filter::icontains("body", text));
    let mut bodies: Vec<String> = schema
        .pool()
        .query_executor::<SearchedNote>()
        .execute(&query)
        .await
        .expect("the search runs")
        .into_iter()
        .map(|row| row.body)
        .collect();
    bodies.sort();
    bodies
}

#[tokio::test]
async fn a_search_finds_its_text_and_nothing_its_symbols_would_match() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    schema
        .pool()
        .execute_ddl(
            "CREATE TABLE IF NOT EXISTS searched_notes (organisation_id TEXT NOT NULL, \
             note_id TEXT NOT NULL, body TEXT NOT NULL, \
             PRIMARY KEY ((organisation_id) HASH, note_id ASC))",
        )
        .await
        .expect("the table exists");
    let bodies = ["50% korting", "500 euro", "a_b", "axb", "c:\\temp"];
    schema
        .pool()
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            for (index, body) in bodies.iter().enumerate() {
                tx.insert(&SearchedNote {
                    organisation_id: "org-a".to_owned(),
                    note_id: format!("n{index}"),
                    body: (*body).to_owned(),
                })
                .await?;
            }
            Ok::<_, DatabaseError>(())
        })
        .await
        .expect("the rows are written");

    assert_eq!(found(&schema, "%").await, ["50% korting"]);
    assert_eq!(found(&schema, "a_b").await, ["a_b"]);
    assert_eq!(found(&schema, "\\").await, ["c:\\temp"]);
    assert_eq!(found(&schema, "KORTING").await, ["50% korting"]);
    assert_eq!(found(&schema, "50").await, ["50% korting", "500 euro"]);
}
