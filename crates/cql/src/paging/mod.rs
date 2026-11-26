//! Paging support for YCQL queries.
//!
//! # Usage
//!
//! ```rust,no_run
//! use fundamental_cql::{
//!     CqlValue, DEFAULT_MAX_UNPAGED_RESULTS, CqlConfig, CqlPool, check_result_size,
//! };
//! # #[derive(Clone, Debug, PartialEq, fundamental_macros::Model)]
//! # #[model(cql, table = "events")]
//! # struct Event {
//! #     #[model(partition_key)]
//! #     tenant_id: uuid::Uuid,
//! #     payload: String,
//! # }
//! # async fn example(config: CqlConfig, tenant_id: uuid::Uuid) -> Result<(), Box<dyn std::error::Error>> {
//! let pool = CqlPool::connect(&config).await?;
//! let results = pool
//!     .query::<Event>()
//!     .partition_key(CqlValue::uuid(tenant_id))
//!     .execute()
//!     .await?;
//! check_result_size(&results, DEFAULT_MAX_UNPAGED_RESULTS)?;
//! # Ok(())
//! # }
//! ```

// The doctest above documents the re-exported paging items.
#![allow(rustdoc::private_doc_tests)]
#![expect(
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]

mod config;
mod size;

pub use config::PagingConfig;
pub use size::check_result_size;

/// Default maximum number of rows to accept from an unpaged query.
pub const DEFAULT_MAX_UNPAGED_RESULTS: usize = 10_000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paging_config_default() {
        let config = PagingConfig::default();
        assert_eq!(config.page_size, 1000);
        assert_eq!(config.max_rows, 0);
        assert!(config.retry_on_error);
    }

    #[test]
    fn test_paging_config_builder() {
        let config = PagingConfig::new(500).max_rows(10000).no_retry();

        assert_eq!(config.page_size, 500);
        assert_eq!(config.max_rows, 10000);
        assert!(!config.retry_on_error);
    }

    #[test]
    fn test_check_result_size_ok() {
        let results: Vec<i32> = vec![1, 2, 3, 4, 5];
        assert!(check_result_size(&results, 10).is_ok());
    }

    #[test]
    fn test_check_result_size_exceeds() {
        let results: Vec<i32> = vec![1, 2, 3, 4, 5];
        let err = check_result_size(&results, 3).unwrap_err();
        assert!(err.to_string().contains("too large"));
    }
}
