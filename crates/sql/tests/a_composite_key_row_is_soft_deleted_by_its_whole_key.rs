//! `SqlTransaction::soft_delete_by_key` and `restore_by_key`, and the by-key reads that honour them; requires Docker.

use fundamental_core::model::SqlDb;
use fundamental_db::{
    DatabaseError, IsolationLevel, QueryBuilder, QueryExecutor as _, RepositoryTransaction as _,
};
use fundamental_macros::Model;
use fundamental_test_containers::sql::SqlSchema;

#[derive(Clone, Debug, PartialEq, Model)]
#[model(sql, table = "tenant_notes", soft_delete)]
struct TenantNote {
    #[model(id)]
    organisation_id: String,
    #[model(id)]
    note_id: String,
    body: String,
}

#[derive(Clone, Debug, PartialEq, Model)]
#[model(sql, table = "tenant_tags")]
struct TenantTag {
    #[model(id)]
    organisation_id: String,
    #[model(id)]
    tag_id: String,
}

fn note(organisation: &str, id: &str) -> TenantNote {
    TenantNote {
        organisation_id: organisation.to_owned(),
        note_id: id.to_owned(),
        body: format!("{organisation}/{id}"),
    }
}

fn key(organisation: &str, id: &str) -> (String, String) {
    (organisation.to_owned(), id.to_owned())
}

async fn with_notes() -> SqlSchema {
    let schema = SqlSchema::mint().await.expect("a test schema");
    schema
        .pool()
        .execute_ddl(
            "CREATE TABLE IF NOT EXISTS tenant_notes (organisation_id TEXT NOT NULL, \
             note_id TEXT NOT NULL, body TEXT NOT NULL, deleted_at TIMESTAMPTZ, \
             PRIMARY KEY ((organisation_id) HASH, note_id ASC))",
        )
        .await
        .expect("the table exists");
    schema
        .pool()
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            for row in [
                note("org-a", "n1"),
                note("org-a", "n2"),
                note("org-b", "n1"),
            ] {
                tx.insert(&row).await?;
            }
            Ok::<_, DatabaseError>(())
        })
        .await
        .expect("the rows are written");
    schema
}

async fn soft_delete(schema: &SqlSchema, organisation: &str, id: &str) -> bool {
    let key = key(organisation, id);
    schema
        .pool()
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            tx.soft_delete_by_key::<TenantNote>(&key).await
        })
        .await
        .expect("the soft delete runs")
}

async fn restore(schema: &SqlSchema, organisation: &str, id: &str) -> bool {
    let key = key(organisation, id);
    schema
        .pool()
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            tx.restore_by_key::<TenantNote>(&key).await
        })
        .await
        .expect("the restore runs")
}

async fn visible(schema: &SqlSchema, organisation: &str, id: &str) -> (bool, bool) {
    let key = key(organisation, id);
    schema
        .pool()
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            let read = tx.find_by_key::<TenantNote>(&key).await?.is_some();
            let locked = tx
                .find_for_update_by_key::<TenantNote>(&key)
                .await?
                .is_some();
            Ok::<_, DatabaseError>((read, locked))
        })
        .await
        .expect("the reads run")
}

#[tokio::test]
async fn a_soft_delete_reaches_exactly_the_row_its_whole_key_names() {
    let schema = with_notes().await;

    assert!(soft_delete(&schema, "org-a", "n1").await);

    assert_eq!(visible(&schema, "org-a", "n1").await, (false, false));
    assert_eq!(
        visible(&schema, "org-a", "n2").await,
        (true, true),
        "a sibling in the same organisation was reached"
    );
    assert_eq!(
        visible(&schema, "org-b", "n1").await,
        (true, true),
        "the same id in another organisation was reached"
    );
    let left = schema
        .pool()
        .query_executor::<TenantNote>()
        .execute(&QueryBuilder::<TenantNote, SqlDb>::new().pin_key("org-a"))
        .await
        .expect("the organisation's rows are read");
    assert_eq!(left, vec![note("org-a", "n2")]);
}

#[tokio::test]
async fn a_second_soft_delete_reports_it_did_nothing() {
    let schema = with_notes().await;
    assert!(soft_delete(&schema, "org-a", "n1").await);
    assert!(!soft_delete(&schema, "org-a", "n1").await);
    assert!(!soft_delete(&schema, "org-a", "absent").await);
}

#[tokio::test]
async fn a_restore_brings_the_row_back_once() {
    let schema = with_notes().await;
    assert!(soft_delete(&schema, "org-a", "n1").await);

    assert!(restore(&schema, "org-a", "n1").await);
    assert_eq!(visible(&schema, "org-a", "n1").await, (true, true));
    assert!(
        !restore(&schema, "org-a", "n1").await,
        "a second restore claimed it restored a live row"
    );
}

#[tokio::test]
async fn a_restore_of_a_live_row_reports_it_did_nothing() {
    let schema = with_notes().await;
    assert!(!restore(&schema, "org-a", "n2").await);
    assert!(!restore(&schema, "org-b", "absent").await);
}

#[tokio::test]
async fn a_model_without_soft_delete_is_refused_before_any_statement() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let key = key("org-a", "t1");
    let soft = schema
        .pool()
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            tx.soft_delete_by_key::<TenantTag>(&key).await
        })
        .await;
    assert!(
        soft.is_err(),
        "a hard-delete model accepted a soft delete: {soft:?}"
    );
    let restored = schema
        .pool()
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            tx.restore_by_key::<TenantTag>(&key).await
        })
        .await;
    assert!(restored.is_err(), "{restored:?}");
}
