//! A pool reads and writes in the schema it was configured with, not in `public`.

use fundamental_db::{KeyedRepository, Repository, SchemaName};
use fundamental_macros::Model;
use fundamental_sql::{SqlConfig, SqlPool};
use fundamental_test_containers::sql::SqlSchema;

#[derive(Clone, Debug, PartialEq, Model)]
#[model(sql, table = "widgets")]
struct Widget {
    #[model(id)]
    id: i64,
    name: String,
}

async fn pool_in(url: &str, schema: &SchemaName) -> SqlPool {
    SqlPool::connect(&SqlConfig::development(url).default_schema(schema.clone()))
        .await
        .expect("a pool in the named schema")
}

#[tokio::test]
async fn a_pool_reads_and_writes_in_its_own_schema() {
    // Two mints land in the same host database, which is resolved once per process.
    let tenant_schema = SqlSchema::mint().await.expect("the tenant's schema");
    let other_schema = SqlSchema::mint().await.expect("the other tenant's schema");
    let tenant = tenant_schema.schema().clone();
    let other = other_schema.schema().clone();
    assert_eq!(
        tenant_schema.url(),
        other_schema.url(),
        "both schemas must share a database, or this proves nothing about schema isolation"
    );
    let database_url = tenant_schema.url().to_owned();

    for (schema, owner) in [(&tenant, &tenant_schema), (&other, &other_schema)] {
        owner
            .pool()
            .execute_ddl(&format!(
                "CREATE TABLE {schema}.widgets (id BIGINT PRIMARY KEY, name TEXT NOT NULL)"
            ))
            .await
            .expect("the table");
    }

    let a = pool_in(&database_url, &tenant).await;
    let b = pool_in(&database_url, &other).await;

    a.repository::<Widget>()
        .insert(&Widget {
            id: 1,
            name: "in tenant_a".to_owned(),
        })
        .await
        .expect("insert through the tenant_a pool");

    assert_eq!(
        a.repository::<Widget>()
            .find(&1)
            .await
            .expect("read back")
            .map(|w| w.name),
        Some("in tenant_a".to_owned()),
        "the write landed in the schema its pool names"
    );
    assert_eq!(
        b.repository::<Widget>().find(&1).await.expect("read back"),
        None,
        "and is invisible to a pool pointed at another schema"
    );
}

/// Generated statements and hand-written DDL resolve the schema two ways, which must agree.
#[tokio::test]
async fn unqualified_ddl_lands_where_qualified_reads_look() {
    let minted = SqlSchema::mint().await.expect("a test schema");
    let schema = minted.schema().clone();
    let pool = pool_in(minted.url(), &schema).await;
    pool.execute_ddl("CREATE TABLE widgets (id BIGINT PRIMARY KEY, name TEXT NOT NULL)")
        .await
        .expect("unqualified DDL");

    pool.repository::<Widget>()
        .insert(&Widget {
            id: 7,
            name: "found".to_owned(),
        })
        .await
        .expect("the qualified write finds the table the bare DDL made");
}

/// A clone carrying `SchemaName::default()` would write to `public` and read nothing back.
#[tokio::test]
async fn a_clone_reads_and_writes_where_the_original_did() {
    let minted = SqlSchema::mint().await.expect("a test schema");
    let schema = minted.schema().clone();
    minted
        .pool()
        .execute_ddl(&format!(
            "CREATE TABLE {schema}.widgets (id BIGINT PRIMARY KEY, name TEXT NOT NULL)"
        ))
        .await
        .expect("the table");

    let pool = pool_in(minted.url(), &schema).await;
    let cloned = pool.repository::<Widget>().clone();

    cloned
        .insert(&Widget {
            id: 42,
            name: "written through a clone".to_owned(),
        })
        .await
        .expect("the clone writes");

    assert_eq!(
        pool.repository::<Widget>()
            .find(&42)
            .await
            .expect("read back")
            .map(|w| w.name),
        Some("written through a clone".to_owned()),
        "a clone must write where its original reads"
    );
}
