//! Executing the framework's abstract query builder against SQLite.

mod filter_sql;
mod offline_query_executor;

pub use offline_query_executor::OfflineQueryExecutor;
