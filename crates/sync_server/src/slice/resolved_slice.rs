use std::sync::Arc;

use fundamental_db::Filter;

/// A resolved slice: what a server-side store needs to read the subscription's rows.
#[derive(Debug, Clone)]
pub struct ResolvedSlice {
    /// The slice's declared name; a client's name is recorded only once it matches.
    pub name: Arc<str>,
    /// The base server table to read from.
    pub table: &'static str,
    /// The base table's key columns, in key order.
    pub key_columns: &'static [&'static str],
    /// The synced data columns.
    pub data_columns: &'static [&'static str],
    /// Whether the base table accepts client pushes.
    pub bidirectional: bool,
    /// The scoped predicate selecting this subscription's rows.
    pub filter: Filter,
}
