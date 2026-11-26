//! Batch configuration.

use crate::{BatchType, DEFAULT_MAX_BATCH_SIZE};

/// Configuration for batch operations.
#[derive(Debug, Clone)]
pub struct BatchConfig {
    /// Maximum number of statements per batch.
    pub max_statements: usize,

    /// Batch type (logged, unlogged, or counter).
    pub batch_type: BatchType,

    /// TTL for all rows in the batch (None = no TTL).
    pub ttl: Option<i32>,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            max_statements: DEFAULT_MAX_BATCH_SIZE,
            batch_type: BatchType::Logged,
            ttl: None,
        }
    }
}

impl BatchConfig {
    /// Creates a new batch configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the batch type.
    pub fn batch_type(mut self, bt: BatchType) -> Self {
        self.batch_type = bt;
        self
    }

    /// Sets the batch type to unlogged for better performance.
    pub fn unlogged(mut self) -> Self {
        self.batch_type = BatchType::Unlogged;
        self
    }

    /// Sets a TTL for all rows in the batch.
    pub fn ttl(mut self, seconds: i32) -> Self {
        self.ttl = Some(seconds);
        self
    }

    /// Sets the maximum number of statements per batch.
    pub fn max_statements(mut self, max: usize) -> Self {
        self.max_statements = max;
        self
    }
}
