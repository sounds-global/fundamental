//! The [`ChangePublisher`] seam: where captured changes fan out.

use std::future::Future;

use crate::change::{ChangeResult, SyncChange};

/// The seam a change-capture source publishes to; a change with no live subscriber is still
/// published, since the pull feed is the source of truth.
pub trait ChangePublisher: Send + Sync {
    /// Publishes a captured `change` to every live subscriber.
    ///
    /// # Errors
    /// Returns [`ChangeError::Publish`](crate::change::ChangeError::Publish) if the transport
    /// rejected the change.
    fn publish(&self, change: SyncChange) -> impl Future<Output = ChangeResult<()>> + Send;
}
