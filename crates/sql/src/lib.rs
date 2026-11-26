// Each repository future embeds an acquire future; `runner::applied` nests past the default 128.
#![recursion_limit = "256"]

//! YugabyteDB YSQL adapter for the Fundamental framework.

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![warn(rustdoc::all)]

extern crate self as fundamental_sql;

// Lets `#[derive(Model)]`-generated code reach `fundamental_sql::sqlx` without a direct dependency.
#[doc(hidden)]
pub use sqlx;

/// Schema migration support: SQL DDL generation from `#[derive(Model)]` types.
pub mod migration;

/// Migration runner: applies migrations and tracks them in `_fundamental_migrations`.
pub mod runner;

#[cfg(test)]
mod derive_compile_test;

mod batch;
mod circuit;
mod cluster_time;
mod column_guard;
mod commit_outcome_unknown;
mod config;
mod connection;
mod connections;
mod effect_transaction;
mod error;
mod ident;
mod is_undefined_table;
mod metrics;
mod migration_cause;
mod model;
mod otel;
mod outbox;
mod pool;
mod pool_gauges;
mod prometheus;
mod query;
mod repositories;
mod repository;
mod repository_ops;
mod result;
mod row;
mod row_lock_outcome;
mod sequence_ref;
mod slice_store;
mod sqlstate_of;
mod violated_constraint;
// `debug_assertions` too: this module calls `development()`, which is compiled out of release builds.
#[cfg(all(feature = "test-support", debug_assertions))]
pub mod test_support;
mod transaction;
mod write_outcome;

#[cfg(feature = "internal-benches")]
#[doc(hidden)]
pub mod bench_support;

pub use batch::BatchConfig;
pub use column_guard::ColumnGuard;
pub use commit_outcome_unknown::commit_outcome_unknown;
pub use config::SqlConfig;
pub use connection::SqlConn;
pub use connections::SqlConnections;
pub use effect_transaction::EffectTransaction;
pub use error::SqlError;
pub use is_undefined_table::is_undefined_table;
pub use migration_cause::MigrationCause;
pub use repositories::{Repositories, Stored};
pub use repository::SqlRepository;
pub use result::SqlResult;
pub use row_lock_outcome::RowLockOutcome;
pub use sequence_ref::SequenceRef;
pub use slice_store::SqlSliceStore;
pub use sqlstate_of::sqlstate_of;
pub use transaction::SqlTransaction;
pub use violated_constraint::ViolatedConstraint;
pub use write_outcome::WriteOutcome;

pub use circuit::*;
pub use cluster_time::ClusterTime;
pub use metrics::*;
pub use model::*;
pub use otel::*;
pub use pool::*;
pub use pool_gauges::SqlPoolGauges;
pub use prometheus::*;
pub use query::*;
pub use row::*;
