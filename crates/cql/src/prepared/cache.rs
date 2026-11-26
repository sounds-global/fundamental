use crate::CqlSession;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use rustc_hash::FxHashMap;
use scylla::statement::prepared::PreparedStatement;
use tokio::sync::RwLock;
use tracing::{debug, trace};

use super::{CacheEntry, CacheStats};
use crate::{CqlError, CqlResult};

/// A cache for prepared statements with LRU eviction.
///
/// Keyed by CQL text; when full, the least recently used entry is evicted.
#[derive(Debug)]
pub struct PreparedCache {
    session: CqlSession,

    cache: RwLock<FxHashMap<String, CacheEntry>>,

    max_size: usize,

    hits: AtomicU64,

    misses: AtomicU64,

    evictions: AtomicU64,
}

impl PreparedCache {
    /// Creates a new prepared statement cache.
    pub fn new(session: CqlSession, max_size: usize) -> Self {
        Self {
            session,
            cache: RwLock::new(FxHashMap::default()),
            max_size,
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            evictions: AtomicU64::new(0),
        }
    }

    /// Gets or prepares a statement.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn get_or_prepare(&self, cql: &str) -> CqlResult<Arc<PreparedStatement>> {
        {
            let mut cache = self.cache.write().await;
            if let Some(entry) = cache.get_mut(cql) {
                entry.last_access = Instant::now();
                self.hits.fetch_add(1, Ordering::Relaxed);
                trace!(cql = %cql, "Using cached prepared statement");
                return Ok(Arc::clone(&entry.statement));
            }
        }

        self.misses.fetch_add(1, Ordering::Relaxed);
        self.prepare_and_cache(cql).await
    }

    /// Prepares a statement and adds it to the cache.
    async fn prepare_and_cache(&self, cql: &str) -> CqlResult<Arc<PreparedStatement>> {
        let mut cache = self.cache.write().await;

        // Double-check in case another task prepared it while we were waiting
        if let Some(entry) = cache.get_mut(cql) {
            entry.last_access = Instant::now();
            return Ok(Arc::clone(&entry.statement));
        }

        debug!(cql = %cql, "Preparing statement");
        let stmt = self
            .session
            .inner()
            .prepare(cql)
            .await
            .map_err(|e| CqlError::Prepared(e.to_string()))?;

        let stmt = Arc::new(stmt);

        if cache.len() >= self.max_size {
            self.evict_lru(&mut cache);
        }

        cache.insert(
            cql.to_string(),
            CacheEntry {
                statement: Arc::clone(&stmt),
                last_access: Instant::now(),
            },
        );
        Ok(stmt)
    }

    /// Evicts the least recently used entry from the cache.
    fn evict_lru(&self, cache: &mut FxHashMap<String, CacheEntry>) {
        let oldest_key = cache
            .iter()
            .min_by_key(|(_, entry)| entry.last_access)
            .map(|(key, _)| key.clone());

        if let Some(key) = oldest_key {
            debug!(cql = %key, "Evicting LRU prepared statement from cache");
            cache.remove(&key);
            self.evictions.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Invalidates a specific statement.
    pub async fn invalidate(&self, cql: &str) {
        let mut cache = self.cache.write().await;
        cache.remove(cql);
    }

    /// Clears the entire cache.
    pub async fn clear(&self) {
        let mut cache = self.cache.write().await;
        cache.clear();
        debug!("Cleared prepared statement cache");
    }

    /// Returns the current cache size.
    pub async fn size(&self) -> usize {
        let cache = self.cache.read().await;
        cache.len()
    }

    /// Returns cache statistics including hit/miss rates.
    pub async fn stats(&self) -> CacheStats {
        let cache = self.cache.read().await;
        let hits = self.hits.load(Ordering::Relaxed);
        let misses = self.misses.load(Ordering::Relaxed);
        let evictions = self.evictions.load(Ordering::Relaxed);
        let total = hits + misses;

        CacheStats {
            size: cache.len(),
            max_size: self.max_size,
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

    /// Resets the hit, miss, and eviction counters.
    pub fn reset_stats(&self) {
        self.hits.store(0, Ordering::Relaxed);
        self.misses.store(0, Ordering::Relaxed);
        self.evictions.store(0, Ordering::Relaxed);
    }
}
