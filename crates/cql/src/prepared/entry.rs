//! Cache entry type for the prepared statement cache.

use std::{sync::Arc, time::Instant};

use scylla::statement::prepared::PreparedStatement;

/// Entry in the prepared statement cache with LRU tracking.
#[derive(Debug)]
pub(crate) struct CacheEntry {
    pub(crate) statement: Arc<PreparedStatement>,
    pub(crate) last_access: Instant,
}
