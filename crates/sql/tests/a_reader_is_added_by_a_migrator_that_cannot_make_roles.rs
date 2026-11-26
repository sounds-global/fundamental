//! A migrator without `CREATEROLE` adds a model's reader once an administrator has made the role; requires Docker.

use fundamental_core::model::SqlDb;
use fundamental_db::SchemaName;
use fundamental_db::migration::{ModelSchema, render};
use fundamental_macros::Model;
use fundamental_sql::{SqlConfig, SqlPool};
use fundamental_test_containers::sql::{SqlAuthNode, SqlRole};
use uuid::Uuid;

const READER: &str = "fundamental_test_premade_reader";

#[derive(Clone, Debug, PartialEq, Model)]
#[model(
    sql,
    table = "notes",
    read_by(role = "fundamental_test_premade_reader", when = "state = 'published'")
)]
struct Note {
    #[model(id, shard = "hash")]
    organisation_id: Uuid,
    #[model(id, shard = "asc")]
    id: Uuid,
    state: String,
}

#[tokio::test]
async fn the_generated_reader_runs_as_a_login_without_createrole() {
    let node = SqlAuthNode::shared().await.expect("the auth node");
    let admin = SqlPool::connect(&SqlConfig::development(node.superuser_url().as_str()))
        .await
        .expect("the administrator connects");
    admin
        .execute_ddl(&format!(
            "DO $$ BEGIN IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = '{READER}') THEN \
             CREATE ROLE {READER} NOLOGIN; END IF; EXCEPTION WHEN duplicate_object THEN NULL; END $$"
        ))
        .await
        .expect("the administrator makes the reader's role");
    let migrator = SqlRole::mint().await.expect("a login that owns its schema");
    let own = SchemaName::new(migrator.name()).expect("a role's name is a schema's");
    let pool = SqlPool::connect(
        &SqlConfig::development(migrator.url().expose().as_str()).default_schema(own),
    )
    .await
    .expect("the migrator connects");

    let made_anyway = pool
        .execute_ddl(&format!(
            "DO $$ BEGIN CREATE ROLE {READER} NOLOGIN; \
             EXCEPTION WHEN duplicate_object THEN NULL; END $$"
        ))
        .await
        .expect_err("a login without CREATEROLE is refused a role, even one that exists");
    assert!(
        format!("{made_anyway:?}").contains("42501"),
        "refused for want of the privilege, insufficient_privilege, which no duplicate handler \
         catches: {made_anyway:?}"
    );

    for statement in render::create_all(&<Note as ModelSchema<SqlDb>>::table_schema()) {
        pool.execute_ddl(&statement)
            .await
            .unwrap_or_else(|error| panic!("the migrator runs {statement}: {error:?}"));
    }
    let policies = admin
        .fetch_all(&format!(
            "SELECT 1 FROM pg_policies WHERE schemaname = '{}' AND tablename = 'notes' \
             AND policyname = 'notes_read_by_{READER}'",
            migrator.name()
        ))
        .await
        .expect("the catalog answers");
    assert_eq!(policies.len(), 1, "the reader's policy is on the table");

    migrator.release().await.expect("the migrator is released");
}
