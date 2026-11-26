//! Batch operations for YCQL.
//!
//! Batches in YCQL are atomic but not isolated: other transactions may see partial results.
//!
//! # Example
//!
//! ```rust,no_run
//! use fundamental_cql::{BatchType, CqlBatch, CqlConfig, CqlPool};
//! # #[derive(Clone, Debug, PartialEq, fundamental_macros::Model)]
//! # #[model(cql, table = "events")]
//! # struct Event {
//! #     #[model(partition_key)]
//! #     tenant_id: uuid::Uuid,
//! #     #[model(clustering_key)]
//! #     bucket: i64,
//! #     payload: String,
//! # }
//! # async fn example(config: CqlConfig, events: Vec<Event>) -> Result<(), Box<dyn std::error::Error>> {
//! let pool = CqlPool::connect(&config).await?;
//! let repository = pool.repository::<Event>();
//!
//! // Insert multiple events in a single batch
//! repository.insert_batch(&events, BatchType::Logged).await?;
//! # Ok(())
//! # }
//! ```

// The doctest above documents the re-exported batch types.
#![allow(rustdoc::private_doc_tests)]
#![expect(
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]

mod config;
mod kind;
mod ops;

pub use config::BatchConfig;
pub use kind::BatchType;

pub use ops::*;

/// Maximum number of statements allowed in a single batch by default.
pub const DEFAULT_MAX_BATCH_SIZE: usize = 50;

#[cfg(test)]
mod tests {
    use super::*;
    use scylla::statement::batch::BatchType as CqlBatchType;

    #[test]
    fn test_batch_config_default() {
        let config = BatchConfig::default();
        assert_eq!(config.max_statements, DEFAULT_MAX_BATCH_SIZE);
        assert_eq!(config.batch_type, BatchType::Logged);
        assert!(config.ttl.is_none());
    }

    #[test]
    fn test_batch_config_builder() {
        let config = BatchConfig::new().unlogged().ttl(3600).max_statements(100);

        assert_eq!(config.batch_type, BatchType::Unlogged);
        assert_eq!(config.ttl, Some(3600));
        assert_eq!(config.max_statements, 100);
    }

    #[test]
    fn test_batch_type_conversion() {
        let _ = CqlBatchType::from(BatchType::Logged);
        let _ = CqlBatchType::from(BatchType::Unlogged);
        let _ = CqlBatchType::from(BatchType::Counter);
    }
}
