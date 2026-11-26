//! A role a model names as its reader sees only the rows the model says and writes nothing; requires Docker.

use fundamental_core::i18n::Localized;
use fundamental_core::model::SqlDb;
use fundamental_db::migration::{ModelSchema, render};
use fundamental_db::{DatabaseError, IsolationLevel, RepositoryTransaction as _};
use fundamental_macros::Model;
use fundamental_test_containers::sql::SqlSchema;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Model)]
#[model(
    sql,
    table = "articles",
    read_by(role = "fundamental_test_reader", when = "state = 'published'")
)]
struct Article {
    #[model(id, shard = "hash")]
    organisation_id: Uuid,
    #[model(id, shard = "asc")]
    id: Uuid,
    headline: Localized<String>,
    state: String,
}

async fn articles() -> SqlSchema {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let own = <Article as ModelSchema<SqlDb>>::table_schema();
    let companion =
        <Article as ModelSchema<SqlDb>>::translation_schema().expect("a localized record");
    for statement in render::create_all(&own)
        .into_iter()
        .chain(render::create_all(&companion))
        .chain([format!(
            "GRANT USAGE ON SCHEMA {} TO fundamental_test_reader",
            schema.schema()
        )])
    {
        schema
            .pool()
            .execute_ddl(&statement)
            .await
            .expect("the tables and their reader exist");
    }
    schema
}

async fn read_as_the_reader(schema: &SqlSchema, sql: &str) -> Vec<Uuid> {
    schema
        .pool()
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            tx.execute("SET LOCAL ROLE fundamental_test_reader")
                .await
                .map_err(DatabaseError::from)?;
            let rows = tx.fetch_all(sql).await.map_err(DatabaseError::from)?;
            Ok::<_, DatabaseError>(
                rows.iter()
                    .map(|row| fundamental_sql::sqlx::Row::get::<Uuid, _>(row, "id"))
                    .collect(),
            )
        })
        .await
        .expect("the reader reads")
}

#[tokio::test]
async fn a_reader_sees_only_published_rows_and_their_translations_and_writes_nothing() {
    let schema = articles().await;
    let organisation = Uuid::new_v4();
    let article = |headline: &str, state: &str| Article {
        organisation_id: organisation,
        id: Uuid::new_v4(),
        headline: Localized::new(headline).with("nl", headline),
        state: state.to_owned(),
    };
    let (open, draft) = (article("Open", "published"), article("Soon", "draft"));
    schema
        .pool()
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            tx.insert(&open).await?;
            tx.insert(&draft).await?;
            Ok::<_, DatabaseError>(())
        })
        .await
        .expect("the owner writes both");

    let seen = read_as_the_reader(&schema, "SELECT id FROM articles").await;
    let translated = read_as_the_reader(&schema, "SELECT id FROM articles_i18n").await;
    assert_eq!(seen, [open.id]);
    assert_eq!(translated, [open.id]);

    let refused = schema
        .pool()
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            tx.execute("SET LOCAL ROLE fundamental_test_reader")
                .await
                .map_err(DatabaseError::from)?;
            tx.execute("UPDATE articles SET state = 'published'")
                .await
                .map_err(DatabaseError::from)
        })
        .await
        .expect_err("the reader writes nothing");
    assert!(format!("{refused:?}").contains("42501"), "{refused:?}");
}

async fn drift(schema: &SqlSchema) -> Vec<fundamental_db::migration::SchemaChange> {
    use fundamental_db::migration::{DatabaseSchema, SchemaIntrospector as _};
    let actual = fundamental_sql::migration::SqlIntrospector::introspect(schema.pool())
        .await
        .expect("the catalog reads");
    let mut expected = DatabaseSchema::for_database(actual.database_type.expect("a SQL schema"));
    expected.add_table(<Article as ModelSchema<SqlDb>>::table_schema());
    expected.add_table(
        <Article as ModelSchema<SqlDb>>::translation_schema().expect("a localized record"),
    );
    let mut read = DatabaseSchema::for_database(actual.database_type.expect("a SQL schema"));
    for table in ["articles", "articles_i18n"] {
        read.add_table(actual.tables[table].clone());
    }
    fundamental_db::migration::SchemaDiffer::new().diff(&expected, &read)
}

#[tokio::test]
async fn the_catalog_reads_a_reader_back_as_its_model_names_it() {
    let schema = articles().await;
    assert!(
        drift(&schema).await.is_empty(),
        "{:?}",
        drift(&schema).await
    );

    schema
        .pool()
        .execute_ddl("REVOKE SELECT ON articles FROM fundamental_test_reader")
        .await
        .expect("the grant goes");
    let changes = drift(&schema).await;
    assert!(
        matches!(changes.as_slice(),
            [fundamental_db::migration::SchemaChange::ChangeReader(change)]
                if change.table_name == "articles" && change.old.is_none()),
        "a reader no longer granted the table reads none of it: {changes:?}"
    );
}
