//! No-op metrics implementation.

use super::DatabaseMetrics;
#[cfg(feature = "std")]
use std::time::Duration;

/// A no-op metrics implementation for when metrics are disabled.
///
/// The default when no metrics collector is configured.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopMetrics;

impl DatabaseMetrics for NoopMetrics {
    #[cfg(feature = "std")]
    #[inline]
    fn record_query(&self, _operation: &str, _table: &str, _duration: Duration, _success: bool) {}

    #[cfg(feature = "std")]
    #[inline]
    fn record_connection_acquire(&self, _duration: Duration, _success: bool) {}

    #[cfg(feature = "std")]
    #[inline]
    fn record_slow_query(&self, _operation: &str, _table: &str, _duration: Duration) {}

    #[cfg(feature = "std")]
    #[inline]
    fn record_transaction(&self, _committed: bool, _duration: Duration) {}
}
