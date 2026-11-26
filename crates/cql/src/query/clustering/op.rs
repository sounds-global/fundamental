//! Comparison operators for clustering-key restrictions.

use std::ops::Bound;

use crate::CqlValue;

/// Clustering key filter operators.
#[derive(Debug, Clone)]
pub(crate) enum ClusteringOp {
    Eq(CqlValue),
    Gt(CqlValue),
    Gte(CqlValue),
    Lt(CqlValue),
    Lte(CqlValue),
    /// `Included` emits `>=`/`<=`, `Excluded` `>`/`<`, and `Unbounded` omits that side.
    Range {
        low: Bound<CqlValue>,
        high: Bound<CqlValue>,
    },
}
