//! Offline (SQLite) storage backend for relation tuples — native targets only.

mod offline_row;
mod offline_tuple_store;

pub use offline_tuple_store::OfflineTupleStore;
