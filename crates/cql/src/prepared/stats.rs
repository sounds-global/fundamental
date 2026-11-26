//! Snapshot statistics for the prepared statement cache.

/// Statistics about the prepared statement cache.
#[derive(Debug, Clone, Copy)]
pub struct CacheStats {
    /// Current number of cached statements.
    pub size: usize,

    /// Maximum cache size.
    pub max_size: usize,

    /// Number of cache hits.
    pub hits: u64,

    /// Number of cache misses.
    pub misses: u64,

    /// Number of evictions.
    pub evictions: u64,

    /// Cache hit rate (0.0 to 1.0).
    pub hit_rate: f64,
}

impl CacheStats {
    /// Returns the cache utilization as a percentage.
    pub fn utilization(&self) -> f64 {
        if self.max_size == 0 {
            0.0
        } else {
            (self.size as f64 / self.max_size as f64) * 100.0
        }
    }

    /// Returns the total number of cache requests.
    pub fn total_requests(&self) -> u64 {
        self.hits + self.misses
    }

    /// Returns the hit rate as a percentage.
    pub fn hit_rate_percent(&self) -> f64 {
        self.hit_rate * 100.0
    }
}
