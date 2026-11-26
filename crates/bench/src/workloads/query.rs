//! Queries built into their statement text and bound values.

use fundamental_cql::bench_support as cql;
use fundamental_sql::bench_support as sql;

/// How many filters a query carries: a list page filtered by a tenant, a kind and a few more.
pub const FILTERS: usize = 10;

/// A YSQL `SELECT … WHERE` with [`FILTERS`] predicates; the statement's length.
#[must_use]
pub fn select_sql() -> usize {
    sql::select_sql(FILTERS).map_or(0, |(text, _)| text.len())
}

/// A YSQL bulk `UPDATE … SET` of [`FILTERS`] columns; the statement's length.
#[must_use]
pub fn update_sql() -> usize {
    sql::update_sql(FILTERS).map_or(0, |(text, _)| text.len())
}

/// A YCQL `SELECT` bounded on [`FILTERS`] clustering columns; the statement's length.
#[must_use]
pub fn select_cql() -> usize {
    cql::select_cql(FILTERS).map_or(0, |(text, _)| text.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_query_is_built() {
        assert!(select_sql() > 0);
        assert!(update_sql() > 0);
        assert!(select_cql() > 0);
    }
}
