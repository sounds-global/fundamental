//! `pin_index` executes through a secondary index a composite-key model declares; requires Docker.

use fundamental_core::model::SqlDb;
use fundamental_db::{
    DatabaseError, IsolationLevel, QueryBuilder, QueryExecutor as _, RepositoryTransaction as _,
};
use fundamental_macros::Model;
use fundamental_test_containers::sql::SqlSchema;

#[derive(Clone, Debug, PartialEq, Model)]
#[model(
    sql,
    index(name = "memberships_by_account", columns = ["staff_account_id", "organisation_id"])
)]
struct Membership {
    #[model(id)]
    organisation_id: String,
    #[model(id)]
    staff_account_id: String,
    role: String,
}

#[tokio::test]
async fn one_accounts_rows_across_organisations_come_back_through_its_index() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    schema
        .pool()
        .execute_ddl(
            "CREATE TABLE IF NOT EXISTS memberships (organisation_id TEXT NOT NULL, \
             staff_account_id TEXT NOT NULL, role TEXT NOT NULL, \
             PRIMARY KEY ((organisation_id) HASH, staff_account_id ASC))",
        )
        .await
        .expect("the table exists");
    schema
        .pool()
        .execute_ddl(
            "CREATE INDEX IF NOT EXISTS memberships_by_account \
             ON memberships ((staff_account_id) HASH, organisation_id ASC)",
        )
        .await
        .expect("the index exists");

    let mut rows = Vec::new();
    for organisation in ["org-a", "org-b", "org-c"] {
        for account in ["acct-1", "acct-2", "acct-3"] {
            rows.push(Membership {
                organisation_id: organisation.to_owned(),
                staff_account_id: account.to_owned(),
                role: "staff".to_owned(),
            });
        }
    }
    let written = rows.clone();
    schema
        .pool()
        .transaction(IsolationLevel::Serializable, async |tx| {
            for row in &written {
                tx.insert(row).await?;
            }
            Ok::<_, DatabaseError>(())
        })
        .await
        .expect("the rows are written");

    let query = QueryBuilder::<Membership, SqlDb>::new()
        .pin_index(Membership::MEMBERSHIPS_BY_ACCOUNT, "acct-2");
    let mut found = schema
        .pool()
        .query_executor::<Membership>()
        .execute(&query)
        .await
        .expect("the read through the index executes");
    found.sort_by(|a, b| a.organisation_id.cmp(&b.organisation_id));

    let organisations: Vec<&str> = found
        .iter()
        .map(|row| row.organisation_id.as_str())
        .collect();
    assert_eq!(organisations, ["org-a", "org-b", "org-c"]);
    assert!(
        found.iter().all(|row| row.staff_account_id == "acct-2"),
        "a row belonging to another account came back: {found:?}"
    );
}
