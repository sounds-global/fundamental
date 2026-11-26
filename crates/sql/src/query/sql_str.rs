//! Bridges internally generated SQL strings to sqlx's `SqlSafeStr` API.
//!
//! Every string routed here binds values as `$N` and interpolates only validated identifiers,
//! placeholders and literals; never pass raw input.

use sqlx::postgres::{PgArguments, Postgres};

/// Wraps an internally generated, owned SQL string for execution.
#[inline]
pub(crate) fn pg_query(sql: String) -> sqlx::query::Query<'static, Postgres, PgArguments> {
    sqlx::query(sqlx::AssertSqlSafe(sql))
}
