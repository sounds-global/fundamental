//! YCQL storage backend for relation tuples.

mod cql_tuple_store;
mod forward_row;
mod reverse_row;

pub use cql_tuple_store::CqlTupleStore;
