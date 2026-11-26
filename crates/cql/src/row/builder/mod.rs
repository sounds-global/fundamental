//! CQL statement builders for row write operations.

mod counter;
mod delete;
mod insert;
mod update;

pub(crate) use delete::DeleteBuilder;
pub(crate) use insert::InsertBuilder;
pub(crate) use update::UpdateBuilder;

pub(crate) use counter::*;

/// `keyspace.table`, or bare `table` when the keyspace is empty.
pub(crate) fn qualify(keyspace: &str, table: &str) -> String {
    if keyspace.is_empty() {
        table.to_string()
    } else {
        format!("{keyspace}.{table}")
    }
}
