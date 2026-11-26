//! Benchmark-only access to the crate's internal CQL builders.
//!
//! Re-exposes the `pub(crate)` builders, behind `internal-benches`, to the `query_build` target.

use crate::CqlResult;
use crate::CqlValue;
use crate::query::{ClusteringFilter, ClusteringOp, build_select_cql, build_where_clause};
use crate::translations::{WireKey, group};
use fundamental_core::i18n::{TranslationCheck, TranslationRow};
use scylla::value::CqlValue as WireValue;

fn clustering_filters(n: usize) -> Vec<ClusteringFilter> {
    (0..n)
        .map(|i| ClusteringFilter {
            column: format!("ck_{i}"),
            op: ClusteringOp::Eq(CqlValue::BigInt(0)),
        })
        .collect()
}

/// Builds a full `SELECT … WHERE` CQL statement with `clustering` clustering
/// predicates over a single partition key.
///
/// # Errors
///
/// Propagates a [`CqlError`](crate::CqlError) if the builder rejects an
/// input (e.g. an invalid identifier).
pub fn select_cql(clustering: usize) -> CqlResult<(String, Vec<CqlValue>)> {
    let pk_columns: [&str; 1] = ["partition_id"];
    let pk_values = [CqlValue::BigInt(1)];
    let filters = clustering_filters(clustering);
    build_select_cql(
        "bench_table".to_string(),
        None,
        &pk_columns,
        &pk_values,
        &filters,
        Some(100),
        true,
    )
}

/// Builds just the `WHERE` clause from one partition key and `clustering`
/// clustering predicates.
///
/// # Errors
///
/// [`CqlError::InvalidQuery`](crate::CqlError::InvalidQuery) if the partition columns and
/// values disagree in number, which this fixture never does.
pub fn where_clause(clustering: usize) -> CqlResult<(String, Vec<CqlValue>)> {
    let pk_columns: [&str; 1] = ["partition_id"];
    let pk_values = [CqlValue::BigInt(1)];
    let filters = clustering_filters(clustering);
    build_where_clause(&pk_columns, &pk_values, &filters)
}

/// A page of records and its translations as the companion read returns them: two
/// languages a record, ordered by record, then field, then language.
#[derive(Debug, Clone)]
pub struct TranslatedPage {
    keys: Vec<WireKey>,
    rows: Vec<(WireKey, TranslationRow)>,
}

/// A page of `records` records keyed by a descending `bigint`, each with two translations.
#[must_use]
pub fn translated_page(records: usize) -> TranslatedPage {
    let key = |i: usize| WireKey(vec![Some(WireValue::BigInt(i as i64))]);
    let keys: Vec<WireKey> = (0..records).rev().map(key).collect();
    let rows = keys
        .iter()
        .flat_map(|k| {
            ["de", "nl"].map(|locale| {
                (
                    k.clone(),
                    TranslationRow {
                        field: "title".to_owned(),
                        locale: locale.to_owned(),
                        value: format!("title in {locale}"),
                        check: TranslationCheck::stored(String::new(), String::new(), None),
                    },
                )
            })
        })
        .collect();
    TranslatedPage { keys, rows }
}

/// Groups the page's translations by record and takes each record's group, as a page read
/// does; returns how many translations found their record.
#[must_use]
pub fn match_translations(page: TranslatedPage) -> usize {
    let mut groups = group(page.rows);
    page.keys
        .iter()
        .filter_map(|k| groups.remove(k))
        .map(|rows| rows.len())
        .sum()
}
