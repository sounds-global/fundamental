//! Reusable, leased stores for database-backed test suites.
//!
//!
//! A lease expires, so a holder killed before its `Drop` loses the store to the next claimant.

mod pooled_schema;

pub use pooled_schema::PooledSchema;
