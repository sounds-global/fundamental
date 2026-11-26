use fundamental_sql::sqlx::postgres::PgConnection;
use fundamental_sql::sqlx::query;
use fundamental_sql::{SqlConfig, SqlPool};

use crate::TestContainerError;
use crate::sql::SqlNode;

/// The advisory lock every test's migrations take turns under, machine-wide.
const TURN: i64 = 0x5d1_7a1e;

/// One test's turn to run DDL on the shared node; the next waits until it drops.
///
/// Concurrent DDL conflicts on one catalog version; taking turns removes the conflicts.
pub struct DdlTurn {
    // Detached from its pool: dropping it closes the session, and the lock with it.
    _connection: PgConnection,
}

impl DdlTurn {
    /// Waits for the node's turn and takes it.
    ///
    /// # Errors
    ///
    /// [`TestContainerError::Provision`] when the node cannot be reached or the lock cannot
    /// be taken.
    pub async fn take() -> Result<Self, TestContainerError> {
        let node = SqlNode::shared().await?;
        let pool = SqlPool::connect(
            &SqlConfig::development(node.bootstrap_url())
                .max_connections(1)
                .application_name("ddl-turn"),
        )
        .await
        .map_err(|error| refused(&error))?;
        let mut connection = pool
            .inner()
            .acquire()
            .await
            .map_err(|error| refused(&error))?;
        query("SELECT pg_advisory_lock($1)")
            .bind(TURN)
            .execute(&mut *connection)
            .await
            .map_err(|error| refused(&error))?;
        Ok(Self {
            _connection: connection.detach(),
        })
    }
}

fn refused(error: &dyn std::fmt::Display) -> TestContainerError {
    TestContainerError::Provision {
        engine: "sql",
        fixture: "ddl turn".to_owned(),
        detail: error.to_string(),
    }
}
