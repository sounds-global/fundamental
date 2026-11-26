//! The one Docker-container harness this workspace's integration tests boot from.
//!
//! Containers are named and reused, so readiness is a client-side poll, fixtures are swept on
//! the way in, a recipe change removes the old container, and acquire is locked across
//! processes. Only ever a dev-dependency.
//!
//! ```no_run
//! # #[cfg(feature = "sql")]
//! # async fn example() {
//! # use fundamental_test_containers::sql::SqlDatabase;
//! let db = SqlDatabase::mint().await.expect("a test database");
//! let pool = db.pool().clone();
//! // ...this test is the only writer of `pool`. Assert on counts freely.
//! # let _ = pool;
//! # }
//! ```
//!
//! Hold the fixture for the whole test: it is removed when it drops, so binding it to `_`
//! would delete it immediately.

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![warn(rustdoc::all)]

mod acquire_lock;
mod auth_node;
pub mod container_name;
mod container_recipe;
mod dedicated_container;
mod dedicated_engine_node;
mod engine_node;
mod ensure_tablet_budget;
mod install_harness_logger;
mod once_per_run;
mod start_container;
mod sweep_stale_containers;
mod tcp_proxy;
mod test_container_error;

#[cfg(feature = "browser")]
pub mod browser;
// `fundamental_cql::test_support` exists only with `debug_assertions`.
#[cfg(all(feature = "cql", debug_assertions))]
pub mod cql;
#[cfg(feature = "nats")]
pub mod nats;
#[cfg(feature = "openbao")]
pub mod openbao;
// `fundamental_sql::test_support` exists only with `debug_assertions`.
#[cfg(all(feature = "sql", debug_assertions))]
pub mod sql;

pub use auth_node::AuthNode;
pub use dedicated_engine_node::DedicatedEngineNode;
pub use engine_node::EngineNode;
pub use once_per_run::once_per_run;
pub use tcp_proxy::TcpProxy;
pub use test_container_error::TestContainerError;
