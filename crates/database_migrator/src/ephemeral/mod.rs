//! Ephemeral, freshly-migrated stores for database-backed test suites.
//!
//! Each test provisions its own uniquely-named store, migrated from the app's shipped migrations,
//! and drops it when the handle goes out of scope.

mod ephemeral_error;
#[cfg(feature = "cql")]
mod ephemeral_keyspace;
#[cfg(feature = "sql")]
mod ephemeral_schema;
#[cfg(feature = "cql")]
mod sweep_keyspaces_once;

pub use ephemeral_error::EphemeralError;
#[cfg(feature = "cql")]
pub use ephemeral_keyspace::EphemeralKeyspace;
#[cfg(feature = "sql")]
pub use ephemeral_schema::EphemeralSchema;
#[cfg(feature = "cql")]
use sweep_keyspaces_once::sweep_keyspaces_once;
