//! Prepared statement cache for YCQL.

mod cache;
mod entry;
mod stats;

pub use cache::PreparedCache;
pub(crate) use entry::CacheEntry;
pub use stats::CacheStats;

#[cfg(test)]
mod tests {
    use super::*;

    fn make_stats(
        size: usize,
        max_size: usize,
        hits: u64,
        misses: u64,
        evictions: u64,
    ) -> CacheStats {
        let total = hits + misses;
        CacheStats {
            size,
            max_size,
            hits,
            misses,
            evictions,
            hit_rate: if total > 0 {
                hits as f64 / total as f64
            } else {
                0.0
            },
        }
    }

    #[test]
    fn test_cache_stats_utilization() {
        let stats = make_stats(50, 100, 0, 0, 0);
        assert!((stats.utilization() - 50.0).abs() < f64::EPSILON);

        let empty_stats = make_stats(0, 0, 0, 0, 0);
        assert!((empty_stats.utilization() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_cache_stats_hit_rate() {
        let stats = make_stats(10, 100, 80, 20, 0);
        assert!((stats.hit_rate - 0.8).abs() < f64::EPSILON);
        assert!((stats.hit_rate_percent() - 80.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_cache_stats_total_requests() {
        let stats = make_stats(10, 100, 80, 20, 5);
        assert_eq!(stats.total_requests(), 100);
        assert_eq!(stats.evictions, 5);
    }

    #[test]
    fn test_cache_stats_no_requests() {
        let stats = make_stats(0, 100, 0, 0, 0);
        assert!((stats.hit_rate - 0.0).abs() < f64::EPSILON);
        assert_eq!(stats.total_requests(), 0);
    }
}
