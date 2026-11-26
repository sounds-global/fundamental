//! # Fundamental Database Layer
//!
//! The database traits every backend implements: models declare their database at compile
//! time and are reached through type-safe repositories. The runnable version of this sketch
//! lives in [`fundamental_sql`]:
//!
//! ```ignore
//! use fundamental_sql::prelude::*;
//!
//! // The model declares its database affinity. Note: the marker is `#[model(...)]`,
//! // not `#[database(...)]`; the id field is tagged `#[model(id)]`.
//! #[derive(Model)]
//! #[model(sql, table = "users")]
//! pub struct User {
//!     #[model(id)]
//!     pub id: UserId,
//!     #[model(indexed)]
//!     pub email: String,
//! }
//!
//! // The pool exposes a type-safe repository for the model.
//! async fn create_user(db: &SqlPool, user: &User) -> DatabaseResult<()> {
//!     db.repository::<User>().insert(user).await
//! }
//! ```
//!
//! [`fundamental_sql`]: https://docs.rs/fundamental_sql

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![warn(rustdoc::all)]

// Its own feature: `uuid/v4` needs `getrandom`, which has no wasm backend by default.
#[cfg(feature = "ephemeral")]
pub mod ephemeral;
pub mod ident;
pub mod migration;
mod record_key;

mod constraint_kind;
mod ddl_time_budget;
mod error;
mod schema_name;
// Gated at the module so a no-`std` build has no metrics module rather than broken re-exports.
#[cfg(feature = "std")]
mod metrics;
mod otel;
mod outbox;
mod pool;
mod query;
mod repository;
mod result;
mod tls;
mod transaction;

/// The `chrono` crate this crate's public API is typed in, so callers use the same version.
pub use chrono;
pub use constraint_kind::ConstraintKind;
pub use ddl_time_budget::DDL_TIME_BUDGET;
pub use error::DatabaseError;
pub use result::DatabaseResult;
pub use schema_name::SchemaName;
pub use tls::{TlsConfig, TlsMode};

#[cfg(feature = "std")]
pub use metrics::*;
pub use otel::*;
pub use outbox::*;
pub use pool::*;
pub use query::*;
pub use repository::*;
pub use transaction::*;

pub use record_key::RecordKey;
