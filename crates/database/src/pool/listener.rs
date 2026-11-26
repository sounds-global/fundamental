//! The [`PoolListener`] trait for observing pool events.

use super::PoolEvent;

/// Listener for pool events.
pub trait PoolListener: Send + Sync {
    /// Called when a pool event occurs.
    fn on_event(&self, event: PoolEvent);
}
