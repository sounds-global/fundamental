//! Benchmark-only access to the crate's internal SQL builders, behind `internal-benches`.

use crate::SqlError;
use crate::query::{build_select_sql, build_update_sql, build_where_clause};
use fundamental_db::SchemaName;

use fundamental_core::model::{BoundTo, Model, SqlDb, SqlModel};
use fundamental_db::{Filter, FilterValue, QueryBuilder};

struct BenchModel;

impl Model for BenchModel {
    const TYPE_NAME: &'static str = "BenchModel";
}

impl BoundTo<SqlDb> for BenchModel {}

impl SqlModel for BenchModel {
    type Id = i64;
    fn id(&self) -> &Self::Id {
        &0
    }
    const TABLE: &'static str = "bench_rows";
}

/// Builds a `SELECT … WHERE` with `filters` equality predicates.
///
/// # Errors
///
/// Propagates a [`SqlError`] if identifier validation rejects a column.
pub fn select_sql(filters: usize) -> Result<(String, Vec<FilterValue>), SqlError> {
    let mut query = QueryBuilder::<BenchModel, SqlDb>::new();
    for i in 0..filters {
        query = query.filter(Filter::eq(format!("col_{i}"), 0_i64));
    }
    build_select_sql::<BenchModel, _>(&SchemaName::default(), &query, None)
}

/// Builds a bulk `UPDATE … SET` with `columns` assignments and one WHERE filter.
///
/// # Errors
///
/// Propagates a [`SqlError`] if identifier validation rejects a column.
pub fn update_sql(columns: usize) -> Result<(String, Vec<FilterValue>), SqlError> {
    let query = QueryBuilder::<BenchModel, SqlDb>::new().filter(Filter::eq("id", 1_i64));
    let updates: Vec<(String, FilterValue)> = (0..columns)
        .map(|i| (format!("col_{i}"), FilterValue::Int(0)))
        .collect();
    build_update_sql::<BenchModel, _>(&SchemaName::default(), &query, &updates)
}

/// Builds a standalone `WHERE` clause from `filters` equality predicates.
///
/// # Errors
///
/// Propagates a [`SqlError`] if identifier validation rejects a column.
pub fn where_clause(filters: usize) -> Result<(String, Vec<FilterValue>, usize), SqlError> {
    let f: Vec<Filter> = (0..filters)
        .map(|i| Filter::eq(format!("col_{i}"), 0_i64))
        .collect();
    build_where_clause(&f, 1)
}
