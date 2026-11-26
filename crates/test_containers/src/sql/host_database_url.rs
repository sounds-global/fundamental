use super::sql_node::SqlNode;
use super::sql_schema::host_url;
use crate::test_container_error::TestContainerError;

/// The URL of the host database this process holds a lease on, creating nothing.
///
/// For a test whose subject is inside a database, minting its own namespaces there. The lease
/// is the process's, so the caller isolates itself by a name prefix of its own.
///
/// # Errors
///
/// [`TestContainerError::Provision`] if the host database cannot be made or reached; the
/// node's own start-up failures arrive from [`SqlNode::shared`].
pub async fn host_database_url() -> Result<&'static str, TestContainerError> {
    host_url(SqlNode::shared().await?).await
}
