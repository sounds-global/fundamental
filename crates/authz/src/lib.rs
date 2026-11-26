//! Native relationship-based authorization for the
//! [Fundamental](https://docs.rs/fundamental) framework: a Zanzibar-style engine that answers
//! "may this subject do this to this object?" from stored relation tuples.
//!
//! A schema ([`SchemaIr`](schema::SchemaIr)) is written as typed Rust
//! (`#[derive(Namespace)]`) or loaded from config. This crate emits `tracing` spans and events
//! but installs no subscriber.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

// Lets `#[derive(Namespace)]`'s `::fundamental_authz::…` paths resolve inside this crate.
extern crate self as fundamental_authz;

pub mod authorizer;
pub mod backend;
pub mod engine;
mod error;
#[cfg(feature = "server")]
pub mod middleware;
pub mod schema;
#[cfg(feature = "service")]
pub mod service;
pub mod tuple;
pub mod zookie;

/// The relation-tuple table as migrations an app's migrator applies; see
/// [`backend::SqlTupleStore::verify_schema`].
#[cfg(feature = "sql")]
pub use backend::SQL_TUPLE_MIGRATIONS;
pub use error::AuthzError;
