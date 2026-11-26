//! The [`ConnectionPool`] trait abstracting over backend pool implementations.

use super::PoolStatus;
use crate::{DatabaseResult, Transactional};
use fundamental_core::model::Database;

/// A database connection pool.
///
/// Repository access lives on the concrete pools (`SqlPool::repository<M>()`), because the
/// bounds cannot be expressed on this trait.
pub trait ConnectionPool<D>: Transactional<D> + Send + Sync + Clone
where
    D: Database,
{
    /// The configuration type for this pool.
    type Config;

    /// Creates a new connection pool.
    fn connect(config: &Self::Config) -> impl Future<Output = DatabaseResult<Self>> + Send;

    /// Returns the current status of the pool.
    fn status(&self) -> PoolStatus;

    /// Closes all connections in the pool.
    fn close(&self) -> impl Future<Output = ()> + Send;

    /// Checks if the pool is healthy.
    ///
    /// Runs a trivial query.
    fn health_check(&self) -> impl Future<Output = DatabaseResult<()>> + Send;

    /// Resizes the pool to have at least `min` idle connections.
    fn resize(&self, min: u32, max: u32) -> impl Future<Output = DatabaseResult<()>> + Send;
}
