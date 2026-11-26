//! [`HealthCheck`]: whether a store can be reached right now.

use std::future::Future;

use crate::DatabaseResult;

/// A store that can report its own reachability.
///
/// Its own one-method trait so a readiness probe can include YCQL pools, which cannot
/// implement [`Transactional`](crate::Transactional).
pub trait HealthCheck: Send + Sync {
    /// Verifies connectivity, usually with a trivial round trip.
    ///
    /// # Errors
    /// Returns the store's own error when it cannot be reached.
    fn health_check(&self) -> impl Future<Output = DatabaseResult<()>> + Send;
}
