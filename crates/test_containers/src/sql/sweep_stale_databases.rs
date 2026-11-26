use fundamental_sql::test_support::drop_stale_test_databases;

use fundamental_db::ephemeral::stale_cutoff;

use crate::sql::sql_database::DATABASE_PREFIX;
use crate::test_container_error::TestContainerError;

/// Removes test databases old enough to be certain nobody is using them.
///
/// # Errors
/// Returns [`TestContainerError::Provision`] if the node could not be reached or the
/// listing failed. Individual databases that resist removal are reported and skipped.
pub(crate) async fn sweep_stale_databases(
    bootstrap_url: &str,
) -> Result<usize, TestContainerError> {
    drop_stale_test_databases(
        bootstrap_url,
        DATABASE_PREFIX,
        &stale_cutoff(DATABASE_PREFIX),
    )
    .await
    .map_err(|error| TestContainerError::Provision {
        engine: "sql",
        fixture: "the stale-database sweep".to_owned(),
        detail: error.to_string(),
    })
}
