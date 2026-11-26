//! A closed value is stored as its `Variant` value and a day as a `DATE`, so a row reads back as
//! it was written and a reader of the table sees `in_use` and `2026-10-01`, never a debug name.

use std::collections::HashMap;

use chrono::NaiveDate;
use fundamental_core::outbox::RefusalKind;
use fundamental_db::{DatabaseError, KeyedRepository as _, Repository as _};
use fundamental_macros::{Model, Variant};
use fundamental_sql::{SqlConnections, SqlError, SqlPool};
use fundamental_test_containers::sql::SqlSchema;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Variant)]
#[variant(key = "stage")]
enum Stage {
    Concept,
    InUse,
}

#[derive(Clone, Debug, PartialEq, Model)]
#[model(sql)]
struct Marked {
    #[model(id)]
    id: uuid::Uuid,
    #[model(variant)]
    stage: Stage,
    #[model(variant)]
    previous: Option<Stage>,
    reached: Option<NaiveDate>,
}

async fn a_table() -> (SqlSchema, SqlPool, SqlConnections) {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let pool = schema.pool().clone();
    pool.execute_ddl(Marked::create_table_sql())
        .await
        .expect("create the table");
    let connections = SqlConnections::new(Some(pool.clone()), HashMap::new());
    (schema, pool, connections)
}

#[test]
fn the_table_holds_a_closed_value_as_text_and_a_day_as_a_date() {
    let ddl = Marked::create_table_sql();
    assert!(ddl.contains("stage TEXT NOT NULL"), "DDL was: {ddl}");
    assert!(ddl.contains("previous TEXT"), "DDL was: {ddl}");
    assert!(!ddl.contains("previous TEXT NOT NULL"), "DDL was: {ddl}");
    assert!(ddl.contains("reached DATE"), "DDL was: {ddl}");
}

#[tokio::test]
async fn a_row_reads_back_as_it_was_written() {
    let (_schema, pool, connections) = a_table().await;
    let repository = connections.repository::<Marked>().expect("repository");
    let row = Marked {
        id: uuid::Uuid::new_v4(),
        stage: Stage::InUse,
        previous: Some(Stage::Concept),
        reached: NaiveDate::from_ymd_opt(2026, 10, 1),
    };
    repository.insert(&row).await.expect("insert");
    let empty = Marked {
        id: uuid::Uuid::new_v4(),
        stage: Stage::Concept,
        previous: None,
        reached: None,
    };
    repository.insert(&empty).await.expect("insert");

    assert_eq!(
        repository.find(&row.id).await.expect("read"),
        Some(row.clone())
    );
    assert_eq!(repository.find(&empty.id).await.expect("read"), Some(empty));
    let stored = pool
        .fetch_one(&format!(
            "SELECT stage, reached::text AS reached FROM markeds WHERE id = '{}'",
            row.id
        ))
        .await
        .expect("the stored values");
    let read = |column: &str| -> String { fundamental_sql::sqlx::Row::get(&stored, column) };
    assert_eq!(
        (read("stage"), read("reached")),
        ("in_use".to_owned(), "2026-10-01".to_owned())
    );
}

#[tokio::test]
async fn a_value_no_variant_has_is_refused_as_a_value_of_its_column() {
    let (_schema, pool, connections) = a_table().await;
    let repository = connections.repository::<Marked>().expect("repository");
    let id = uuid::Uuid::new_v4();
    repository
        .insert(&Marked {
            id,
            stage: Stage::Concept,
            previous: None,
            reached: None,
        })
        .await
        .expect("insert");
    pool.execute(&format!(
        "UPDATE markeds SET previous = 'mauve' WHERE id = '{id}'"
    ))
    .await
    .expect("tamper");

    let error = repository
        .find(&id)
        .await
        .expect_err("an unknown value refuses");
    let DatabaseError::Other(inner) = &error else {
        panic!("not a typed refusal: {error:?}");
    };
    assert!(matches!(
        inner.downcast_ref::<SqlError>(),
        Some(SqlError::RowRefused {
            column: "previous",
            kind: RefusalKind::ValueRefused,
            ..
        })
    ));
}
