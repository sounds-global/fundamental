//! Translator from an abstract [`Filter`] tree to a partition-scoped count.

use std::ops::Bound;

use fundamental_db::{Filter, FilterCondition};

use crate::{ClusteringFilter, ClusteringOp, CqlError, CqlResult, CqlValue, ToRow};

/// A partition-scoped predicate ready to bind into a CQL `COUNT`.
#[derive(Debug)]
pub(crate) struct PartitionScopedCount {
    /// Equality values for the partition-key columns, in column order.
    pub pk_values: Vec<CqlValue>,
    /// Clustering-key restrictions applied within the partition.
    pub clustering: Vec<ClusteringFilter>,
}

/// Translates the abstract `count_where` filters into a partition-scoped count.
///
/// The shape is a conjunction pinning every partition-key column by one `Eq`, with clustering
/// columns narrowed by comparison.
///
/// # Errors
///
/// [`CqlError::InvalidQuery`] for anything else, which would need `ALLOW FILTERING`.
pub(crate) fn build_count_predicate<M: ToRow>(
    filters: &[Filter],
) -> CqlResult<PartitionScopedCount> {
    let pk_cols = M::partition_key_columns();
    let ck_cols = M::clustering_key_columns();

    // One slot per partition-key column; each must be filled exactly once.
    let mut pk_slots: Vec<Option<CqlValue>> = (0..pk_cols.len()).map(|_| None).collect();
    let mut clustering: Vec<ClusteringFilter> = Vec::new();

    // Nested `And` groups count as sibling filters.
    let mut stack: Vec<&FilterCondition> = filters.iter().map(|f| &f.condition).collect();
    while let Some(cond) = stack.pop() {
        match cond {
            FilterCondition::And(inner) => {
                stack.extend(inner.iter().map(|f| &f.condition));
            }
            leaf => classify_leaf(leaf, pk_cols, ck_cols, &mut pk_slots, &mut clustering)?,
        }
    }

    // YCQL cannot count a partial partition key without a full scan.
    let mut pk_values = Vec::with_capacity(pk_cols.len());
    for (col, slot) in pk_cols.iter().zip(pk_slots) {
        match slot {
            Some(v) => pk_values.push(v),
            None => {
                return Err(CqlError::invalid_query(format!(
                    "count_where on YCQL requires an equality predicate on every \
                     partition-key column; '{col}' is unconstrained (use count() for a \
                     full count)"
                )));
            }
        }
    }

    Ok(PartitionScopedCount {
        pk_values,
        clustering,
    })
}

/// Classifies a single leaf condition, routing it to the partition-key slots or
/// the clustering filters, or rejecting it as non-partition-scoped.
#[expect(clippy::too_many_lines, reason = "readability baseline")]
fn classify_leaf(
    leaf: &FilterCondition,
    pk_cols: &[&str],
    ck_cols: &[&str],
    pk_slots: &mut [Option<CqlValue>],
    clustering: &mut Vec<ClusteringFilter>,
) -> CqlResult<()> {
    match leaf {
        FilterCondition::Eq { column, value } => {
            if let Some(idx) = pk_cols.iter().position(|c| *c == column) {
                let cql = CqlValue::try_from(value.clone())?;
                if pk_slots[idx].replace(cql).is_some() {
                    return Err(CqlError::invalid_query(format!(
                        "partition-key column '{column}' is constrained more than once"
                    )));
                }
                Ok(())
            } else if ck_cols.contains(&column.as_str()) {
                clustering.push(ClusteringFilter {
                    column: column.clone(),
                    op: ClusteringOp::Eq(CqlValue::try_from(value.clone())?),
                });
                Ok(())
            } else {
                Err(reject_non_key(column))
            }
        }
        FilterCondition::Gt { column, value } => push_clustering_cmp(
            column,
            value,
            pk_cols,
            ck_cols,
            clustering,
            ClusteringOp::Gt,
        ),
        FilterCondition::Gte { column, value } => push_clustering_cmp(
            column,
            value,
            pk_cols,
            ck_cols,
            clustering,
            ClusteringOp::Gte,
        ),
        FilterCondition::Lt { column, value } => push_clustering_cmp(
            column,
            value,
            pk_cols,
            ck_cols,
            clustering,
            ClusteringOp::Lt,
        ),
        FilterCondition::Lte { column, value } => push_clustering_cmp(
            column,
            value,
            pk_cols,
            ck_cols,
            clustering,
            ClusteringOp::Lte,
        ),
        FilterCondition::Between { column, low, high } => {
            if ck_cols.contains(&column.as_str()) {
                clustering.push(ClusteringFilter {
                    column: column.clone(),
                    op: ClusteringOp::Range {
                        low: Bound::Included(CqlValue::try_from(low.clone())?),
                        high: Bound::Included(CqlValue::try_from(high.clone())?),
                    },
                });
                Ok(())
            } else if pk_cols.iter().any(|c| *c == column) {
                Err(reject_pk_non_eq(column))
            } else {
                Err(reject_non_key(column))
            }
        }
        // Operators YCQL cannot express in a partition-scoped count.
        other => Err(CqlError::invalid_query(format!(
            "count_where on YCQL cannot express {} in a partition-scoped predicate; \
             use count() or the typestate CqlRepository::query builder",
            describe_unsupported(other)
        ))),
    }
}

/// Routes a `>`/`>=`/`<`/`<=` comparison to a clustering filter, rejecting it on
/// a partition-key column (which allows equality only) or a non-key column.
#[expect(clippy::too_many_arguments, reason = "readability baseline")]
fn push_clustering_cmp(
    column: &str,
    value: &fundamental_db::FilterValue,
    pk_cols: &[&str],
    ck_cols: &[&str],
    clustering: &mut Vec<ClusteringFilter>,
    op: fn(CqlValue) -> ClusteringOp,
) -> CqlResult<()> {
    if ck_cols.contains(&column) {
        clustering.push(ClusteringFilter {
            column: column.to_string(),
            op: op(CqlValue::try_from(value.clone())?),
        });
        Ok(())
    } else if pk_cols.contains(&column) {
        Err(reject_pk_non_eq(column))
    } else {
        Err(reject_non_key(column))
    }
}

/// A predicate on a non-key column would require `ALLOW FILTERING`.
fn reject_non_key(column: &str) -> CqlError {
    CqlError::invalid_query(format!(
        "count_where on YCQL cannot filter on the non-key column '{column}' \
         without a full-cluster scan; restrict the partition (and clustering) key instead"
    ))
}

/// A partition-key column may only be pinned by equality.
fn reject_pk_non_eq(column: &str) -> CqlError {
    CqlError::invalid_query(format!(
        "partition-key column '{column}' supports only equality in a count_where predicate"
    ))
}

/// Names the unsupported operator for the rejection message.
fn describe_unsupported(cond: &FilterCondition) -> &'static str {
    match cond {
        FilterCondition::Ne { .. } => "a not-equal (`!=`) predicate",
        FilterCondition::In { .. } => "an `IN` predicate",
        FilterCondition::NotIn { .. } => "a `NOT IN` predicate",
        FilterCondition::IsNull { .. } => "an `IS NULL` predicate",
        FilterCondition::IsNotNull { .. } => "an `IS NOT NULL` predicate",
        FilterCondition::Like { .. } => "a `LIKE` predicate",
        FilterCondition::ILike { .. } => "an `ILIKE` predicate",
        FilterCondition::Or(_) => "an `OR` predicate",
        FilterCondition::Not(_) => "a `NOT` predicate",
        // Named: YCQL has no cursored read, which the caller needs to know.
        FilterCondition::RowValueGt { .. } | FilterCondition::RowValueLt { .. } => {
            "a row-value (tuple) comparison"
        }
        // Eq/Gt/Gte/Lt/Lte/Between/And are handled before this point.
        _ => "this predicate",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_db::FilterValue;
    use fundamental_macros::Model;

    /// Composite-key model: `tenant_id` (partition) + `bucket` (clustering).
    #[derive(Clone, Debug, PartialEq, Model)]
    #[model(cql)]
    struct Event {
        #[model(partition_key)]
        tenant_id: uuid::Uuid,
        #[model(clustering_key)]
        bucket: i64,
        payload: String,
    }

    /// Equality on `tenant_id`, as a [`FilterValue::Uuid`]; a plain string would bind as text.
    fn tenant_eq(id: uuid::Uuid) -> Filter {
        Filter::eq("tenant_id", FilterValue::Uuid(id.to_string()))
    }

    #[test]
    fn full_partition_key_equality_translates() {
        let id = uuid::Uuid::nil();
        let pred = build_count_predicate::<Event>(&[tenant_eq(id)]).expect("valid");
        assert_eq!(pred.pk_values.len(), 1);
        assert!(matches!(pred.pk_values[0], CqlValue::Uuid(u) if u == id));
        assert!(pred.clustering.is_empty());
    }

    #[test]
    fn partition_key_plus_clustering_comparison_translates() {
        let id = uuid::Uuid::nil();
        let pred = build_count_predicate::<Event>(&[tenant_eq(id), Filter::gte("bucket", 10_i64)])
            .expect("valid");
        assert_eq!(pred.pk_values.len(), 1);
        assert_eq!(pred.clustering.len(), 1);
        assert_eq!(pred.clustering[0].column, "bucket");
        assert!(matches!(pred.clustering[0].op, ClusteringOp::Gte(_)));
    }

    #[test]
    fn between_on_clustering_becomes_inclusive_range() {
        let id = uuid::Uuid::nil();
        let pred = build_count_predicate::<Event>(&[
            tenant_eq(id),
            Filter::between("bucket", 1_i64, 9_i64),
        ])
        .expect("valid");
        assert!(matches!(
            pred.clustering[0].op,
            ClusteringOp::Range {
                low: Bound::Included(_),
                high: Bound::Included(_),
            }
        ));
    }

    #[test]
    fn missing_partition_key_is_rejected() {
        // Only a clustering predicate — the partition key is unconstrained.
        let err = build_count_predicate::<Event>(&[Filter::gte("bucket", 0_i64)])
            .expect_err("unconstrained partition key must reject");
        assert!(matches!(err, CqlError::InvalidQuery(_)));
    }

    #[test]
    fn non_key_column_is_rejected() {
        let id = uuid::Uuid::nil();
        let err = build_count_predicate::<Event>(&[tenant_eq(id), Filter::eq("payload", "x")])
            .expect_err("non-key predicate must reject");
        assert!(matches!(err, CqlError::InvalidQuery(_)));
    }

    #[test]
    fn non_equality_on_partition_key_is_rejected() {
        let err = build_count_predicate::<Event>(&[Filter::gt("tenant_id", "x")])
            .expect_err("range on partition key must reject");
        assert!(matches!(err, CqlError::InvalidQuery(_)));
    }

    #[test]
    fn duplicated_partition_key_is_rejected() {
        let id = uuid::Uuid::nil();
        let err = build_count_predicate::<Event>(&[tenant_eq(id), tenant_eq(id)])
            .expect_err("duplicate partition-key predicate must reject");
        assert!(matches!(err, CqlError::InvalidQuery(_)));
    }

    #[test]
    fn unsupported_operators_are_rejected() {
        let id = uuid::Uuid::nil();
        // OR is not a partition-scoped conjunction.
        let or = Filter::or(vec![tenant_eq(id), Filter::eq("bucket", 1_i64)]);
        assert!(matches!(
            build_count_predicate::<Event>(&[or]),
            Err(CqlError::InvalidQuery(_))
        ));
        // LIKE has no clustering-key equivalent.
        let like = Filter::and(vec![tenant_eq(id), Filter::like("bucket", "1%")]);
        assert!(matches!(
            build_count_predicate::<Event>(&[like]),
            Err(CqlError::InvalidQuery(_))
        ));
    }
}
