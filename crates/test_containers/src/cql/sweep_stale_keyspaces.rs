use fundamental_cql::CqlConfig;
use fundamental_cql::test_support::drop_stale_test_keyspaces;

use fundamental_db::ephemeral::stale_cutoff;

use crate::cql::cql_keyspace::KEYSPACE_PREFIX;
use crate::test_container_error::TestContainerError;

/// Removes test keyspaces old enough to be certain nobody is using them.
///
/// # Errors
/// Returns [`TestContainerError::Provision`] if the cluster could not be reached or the
/// listing failed. Individual keyspaces that resist removal are reported and skipped.
pub(crate) async fn sweep_stale_keyspaces(config: &CqlConfig) -> Result<usize, TestContainerError> {
    drop_stale_test_keyspaces(config, KEYSPACE_PREFIX, &stale_cutoff(KEYSPACE_PREFIX))
        .await
        .map_err(|error| TestContainerError::Provision {
            engine: "cql",
            fixture: "the stale-keyspace sweep".to_owned(),
            detail: error.to_string(),
        })
}
