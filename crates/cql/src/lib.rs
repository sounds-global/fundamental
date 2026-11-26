//! The wide-column adapter, over the Cassandra wire.

// `migration::introspector::introspect()`'s nested futures exceed rustc's default layout depth
// under some feature graphs.
#![recursion_limit = "256"]
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![warn(rustdoc::all)]

// Lets `#[derive(Model)]` code, which names `::fundamental_cql::…`, resolve in this crate's tests.
extern crate self as fundamental_cql;

// For `#[derive(Identifier)]`'s generated impls, so a consumer needs no direct `scylla` dependency.
#[doc(hidden)]
pub use scylla;

/// Reading a live keyspace back, for the advisory half of `fun migrate check`.
pub mod migration;

/// Migration runner: applies migrations and tracks them in `_fundamental_migrations`.
pub mod runner;

mod batch;
mod circuit;
mod config;
mod connections;
mod consistency;
mod cql;
mod drop_keyspace_with_tables;
mod error;
mod ident;
mod metrics;
mod migration_cause;
mod otel;
mod paging;
mod pool;
mod prepared;
mod prometheus;
mod query;
mod repository;
mod result;
mod retry;
mod row;
mod session;
mod slice_store;
// `debug_assertions` too: it calls `development()`, which a release build does not have.
#[cfg(all(feature = "test-support", debug_assertions))]
pub mod test_support;
mod translations;

// Exposes the internal CQL builders to the `query_build` benchmark.
#[cfg(feature = "internal-benches")]
#[doc(hidden)]
pub mod bench_support;

pub use config::{CqlConfig, DEFAULT_DDL_TIMEOUT};
pub use connections::CqlConnections;
pub use cql::{Cql, CqlExecutor, CqlIdent, Ddl};
pub use drop_keyspace_with_tables::drop_keyspace_with_tables;
pub use error::CqlError;
pub use migration_cause::CqlMigrationCause;
pub use paging::{DEFAULT_MAX_UNPAGED_RESULTS, PagingConfig, check_result_size};
pub use prometheus::{DEFAULT_DURATION_BUCKETS, PrometheusMetrics};
pub use result::CqlResult;
pub use slice_store::CqlSliceStore;

pub use batch::*;
pub use circuit::*;
pub use consistency::*;
pub use metrics::*;
pub use otel::*;
pub use pool::*;
pub use prepared::*;
pub use query::*;
pub use repository::*;
pub use retry::*;
pub use row::*;
pub use session::*;

/// The YCQL row type, re-exported under the name the `#[derive(Model)]`
/// macro generates against (`FromRow::from_row(row: CqlRow)`).
pub use scylla::value::Row as CqlRow;
