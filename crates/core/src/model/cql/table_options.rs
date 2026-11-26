//! Per-table options (default TTL, compaction, comment) for YCQL tables.

#[cfg(not(feature = "std"))]
use alloc::string::String;

use super::CompactionStrategy;

/// Table options for YCQL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableOptions {
    /// Default TTL in seconds.
    pub default_ttl: Option<u32>,
    /// Compaction strategy.
    pub compaction: Option<CompactionStrategy>,
    /// Comment.
    pub comment: Option<String>,
}
