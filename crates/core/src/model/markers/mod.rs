//! Database marker types and the traits that bind a [`Model`](crate::Model) to a backend.
//!
//! The [`Database`] trait is sealed, so the set of marker types is closed.

mod bound_to;
mod connection;
mod cql;
mod cql_model;
mod database;
mod has_database_affinity;
mod has_leading_key;
mod leading_key_addressed;
mod no_leading_key;
mod offline;
mod offline_model;
mod primary;
mod same_database;
mod satisfied;
mod sql;
mod sql_model;
mod test;

pub use bound_to::BoundTo;
pub use connection::Connection;
pub use cql::CqlDb;
pub use cql_model::{CompositePartitionModel, CqlModel, SinglePartitionModel};
pub use database::Database;
pub use has_database_affinity::HasDatabaseAffinity;
pub use has_leading_key::HasLeadingKey;
pub use leading_key_addressed::LeadingKeyAddressed;
pub use no_leading_key::NoLeadingKey;
pub use offline::Offline;
pub use offline_model::OfflineModel;
pub use primary::Primary;
pub use same_database::SameDatabase;
pub use satisfied::Satisfied;
pub use sql::SqlDb;
pub use sql_model::{CompositeKeyModel, SingleKeyModel, SqlModel};

pub use test::{TestDb, TestModel};
