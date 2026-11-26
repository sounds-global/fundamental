//! Chunk-size configuration for batch operations.

/// Configuration for batch operation chunk sizes.
#[derive(Debug, Clone, Copy)]
pub struct BatchConfig {
    /// Maximum items per INSERT batch (default 1000).
    pub insert_chunk_size: usize,

    /// Maximum items per UPDATE batch (default 500), smaller for its CASE expressions.
    pub update_chunk_size: usize,

    /// Maximum items per UPSERT batch (default 1000).
    pub upsert_chunk_size: usize,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            insert_chunk_size: 1000,
            update_chunk_size: 500,
            upsert_chunk_size: 1000,
        }
    }
}

impl BatchConfig {
    /// Creates a new batch configuration with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the INSERT chunk size.
    pub fn insert_chunk_size(mut self, size: usize) -> Self {
        self.insert_chunk_size = size;
        self
    }

    /// Sets the UPDATE chunk size.
    pub fn update_chunk_size(mut self, size: usize) -> Self {
        self.update_chunk_size = size;
        self
    }

    /// Sets the UPSERT chunk size.
    pub fn upsert_chunk_size(mut self, size: usize) -> Self {
        self.upsert_chunk_size = size;
        self
    }

    /// Creates a configuration with larger chunks for high-throughput inserts.
    pub fn high_throughput() -> Self {
        Self {
            insert_chunk_size: 5000,
            update_chunk_size: 1000,
            upsert_chunk_size: 3000,
        }
    }

    /// Creates a configuration with smaller chunks for low-memory environments.
    pub fn low_memory() -> Self {
        Self {
            insert_chunk_size: 100,
            update_chunk_size: 50,
            upsert_chunk_size: 100,
        }
    }
}
