//! A CQL migration that fails carries the driver's verdict, not a rendering of it.
//!
//! Only the permanent class is reachable on a single node; the retryable projection is pinned in
//! `CqlMigrationCause`'s own tests.

#![cfg(not(target_arch = "wasm32"))]

use fundamental_core::Error as _;
use fundamental_cql::CqlError;
use fundamental_cql::runner::CqlMigrator;
use fundamental_db::DatabaseError;
use fundamental_db::migration::{Lints, MigrationGuard};
use fundamental_test_containers::cql::{CqlKeyspace, CqlNode};

/// A real guard, so the probe migration stays the shape a fleet would accept.
fn guard() -> MigrationGuard {
    MigrationGuard::new(1_000, Lints::default(), &[])
}

#[tokio::test]
async fn a_failed_statement_carries_the_drivers_error_rather_than_its_text() {
    let node = CqlNode::shared().await.expect("a YCQL node");
    let keyspace = CqlKeyspace::mint(node).await.expect("a test keyspace");
    let pool = keyspace.pool().clone();

    let error = CqlMigrator::new(guard())
        .apply_all(
            &pool,
            &[(
                "0001_missing",
                "INSERT INTO a_table_that_does_not_exist (id) VALUES ('x');",
            )],
        )
        .await
        .expect_err("a migration naming a missing table must fail");

    let CqlError::MigrationFailed {
        name,
        step,
        total,
        cause,
        ..
    } = &error
    else {
        panic!("expected MigrationFailed, got {error:?}");
    };
    assert_eq!(
        name, "0001_missing",
        "the failure must still name the migration"
    );
    assert_eq!((*step, *total), (1, 1), "and where in the set it stopped");
    assert_eq!(
        error.error_code(),
        "CQL_014",
        "the code stays the migration's own"
    );
    assert!(
        matches!(cause.error(), CqlError::Query(_)),
        "the driver's own error must be carried, not rendered: {cause}"
    );

    // The three ways a caller can hold this, which must not disagree.
    assert!(
        !error.is_retryable(),
        "a migration written against a table that is not there is not fixed by running it again"
    );
    assert!(!fundamental_core::Error::is_retryable(&error));
    assert!(!DatabaseError::from(error).is_retryable());
}
