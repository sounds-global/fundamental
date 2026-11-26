//! Typestate query builder for YCQL.
//!
//! # Typestate Pattern
//!
//! ```rust
//! #![allow(dead_code)]
//! // Type states
//! struct NoPartitionKey;     // Query hasn't specified partition key
//! struct HasPartitionKey;    // Query has specified partition key
//!
//! // CqlQuery<M, State> transitions:
//! // CqlQuery<M, NoPartitionKey> --partition_key()--> CqlQuery<M, HasPartitionKey>
//!
//! // Only CqlQuery<M, HasPartitionKey> has execute() method
//! ```
//!
//! # Example
//!
//! This compiles — `execute()` is reachable once a partition key is bound:
//!
//! ```rust,no_run
//! use fundamental_cql::{CqlValue, CqlPool};
//! # #[derive(Clone, Debug, PartialEq, fundamental_macros::Model)]
//! # #[model(cql, table = "events")]
//! # struct Event {
//! #     #[model(partition_key)]
//! #     tenant_id: uuid::Uuid,
//! #     #[model(clustering_key)]
//! #     bucket: i64,
//! #     payload: String,
//! # }
//! # async fn example(pool: CqlPool, user_id: uuid::Uuid) -> Result<(), Box<dyn std::error::Error>> {
//! pool.query::<Event>()
//!     .partition_key(CqlValue::uuid(user_id)) // State: NoPartitionKey -> HasPartitionKey
//!     .clustering_gte("bucket", CqlValue::bigint(0))
//!     .limit(100)
//!     .execute()                              // Available because state is HasPartitionKey
//!     .await?;
//! # Ok(())
//! # }
//! ```
//!
//! This does **not** compile — `execute()` is absent without a partition key:
//!
//! ```compile_fail
//! use fundamental_cql::{CqlValue, CqlPool};
//! # #[derive(Clone, Debug, PartialEq, fundamental_macros::Model)]
//! # #[model(cql, table = "events")]
//! # struct Event {
//! #     #[model(partition_key)]
//! #     tenant_id: uuid::Uuid,
//! #     #[model(clustering_key)]
//! #     bucket: i64,
//! #     payload: String,
//! # }
//! # async fn example(pool: CqlPool) -> Result<(), Box<dyn std::error::Error>> {
//! pool.query::<Event>()
//!     .clustering_gte("bucket", CqlValue::bigint(0))
//!     .limit(100)
//!     .execute()                              // Error: method `execute` not found
//!     .await?;
//! # Ok(())
//! # }
//! ```

// The doctests above document the re-exported query types.
#![allow(rustdoc::private_doc_tests)]
#![expect(
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]

mod build;
mod builder;
mod clustering;
mod cql;
mod partition;

pub use builder::CqlQuery;
pub use partition::{HasPartitionKey, NoPartitionKey};

pub(crate) use build::*;
pub(crate) use clustering::*;
pub use cql::*;

#[cfg(test)]
mod tests {
    use std::ops::Bound;

    use super::*;

    #[test]
    fn test_cql_value_bigint() {
        let v = CqlValue::bigint(42);
        assert!(matches!(v, CqlValue::BigInt(42)));
    }

    #[test]
    fn test_cql_value_text() {
        let v = CqlValue::text("hello");
        assert!(matches!(v, CqlValue::Text(s) if s == "hello"));
    }

    #[test]
    fn test_cql_value_uuid() {
        let id = uuid::Uuid::new_v4();
        let v = CqlValue::uuid(id);
        assert!(matches!(v, CqlValue::Uuid(u) if u == id));
    }

    #[test]
    fn test_cql_value_timestamp() {
        let v = CqlValue::timestamp(1234567890);
        assert!(matches!(v, CqlValue::Timestamp(1234567890)));
    }

    #[test]
    fn test_cql_value_all_types() {
        let _ = CqlValue::Null;
        let _ = CqlValue::Bool(true);
        let _ = CqlValue::Int(42);
        let _ = CqlValue::Double(2.5);
        let _ = CqlValue::Blob(vec![1, 2, 3]);
    }

    #[test]
    fn test_clustering_filter_eq() {
        let filter = ClusteringFilter {
            column: "timestamp".into(),
            op: ClusteringOp::Eq(CqlValue::BigInt(1000)),
        };
        let (clause, values) = build_clustering_filter(&filter);
        assert_eq!(clause, "timestamp = ?");
        assert_eq!(values.len(), 1);
    }

    #[test]
    fn test_clustering_filter_gt() {
        let filter = ClusteringFilter {
            column: "timestamp".into(),
            op: ClusteringOp::Gt(CqlValue::BigInt(1000)),
        };
        let (clause, values) = build_clustering_filter(&filter);
        assert_eq!(clause, "timestamp > ?");
        assert_eq!(values.len(), 1);
    }

    #[test]
    fn test_clustering_filter_gte() {
        let filter = ClusteringFilter {
            column: "timestamp".into(),
            op: ClusteringOp::Gte(CqlValue::BigInt(1000)),
        };
        let (clause, values) = build_clustering_filter(&filter);
        assert_eq!(clause, "timestamp >= ?");
        assert_eq!(values.len(), 1);
    }

    #[test]
    fn test_clustering_filter_lt() {
        let filter = ClusteringFilter {
            column: "timestamp".into(),
            op: ClusteringOp::Lt(CqlValue::BigInt(2000)),
        };
        let (clause, values) = build_clustering_filter(&filter);
        assert_eq!(clause, "timestamp < ?");
        assert_eq!(values.len(), 1);
    }

    #[test]
    fn test_clustering_filter_lte() {
        let filter = ClusteringFilter {
            column: "timestamp".into(),
            op: ClusteringOp::Lte(CqlValue::BigInt(2000)),
        };
        let (clause, values) = build_clustering_filter(&filter);
        assert_eq!(clause, "timestamp <= ?");
        assert_eq!(values.len(), 1);
    }

    #[test]
    fn test_clustering_filter_range_inclusive() {
        let filter = ClusteringFilter {
            column: "timestamp".into(),
            op: ClusteringOp::Range {
                low: Bound::Included(CqlValue::BigInt(1000)),
                high: Bound::Included(CqlValue::BigInt(2000)),
            },
        };
        let (clause, values) = build_clustering_filter(&filter);
        assert_eq!(clause, "timestamp >= ? AND timestamp <= ?");
        assert_eq!(values.len(), 2);
    }

    #[test]
    fn test_clustering_filter_range_exclusive() {
        let filter = ClusteringFilter {
            column: "timestamp".into(),
            op: ClusteringOp::Range {
                low: Bound::Excluded(CqlValue::BigInt(1000)),
                high: Bound::Excluded(CqlValue::BigInt(2000)),
            },
        };
        let (clause, values) = build_clustering_filter(&filter);
        assert_eq!(clause, "timestamp > ? AND timestamp < ?");
        assert_eq!(values.len(), 2);
    }

    #[test]
    fn test_clustering_filter_range_half_open_low() {
        let filter = ClusteringFilter {
            column: "timestamp".into(),
            op: ClusteringOp::Range {
                low: Bound::Included(CqlValue::BigInt(1000)),
                high: Bound::Unbounded,
            },
        };
        let (clause, values) = build_clustering_filter(&filter);
        assert_eq!(clause, "timestamp >= ?");
        assert_eq!(values.len(), 1);
    }

    #[test]
    fn test_clustering_filter_range_half_open_high() {
        let filter = ClusteringFilter {
            column: "timestamp".into(),
            op: ClusteringOp::Range {
                low: Bound::Unbounded,
                high: Bound::Excluded(CqlValue::BigInt(2000)),
            },
        };
        let (clause, values) = build_clustering_filter(&filter);
        assert_eq!(clause, "timestamp < ?");
        assert_eq!(values.len(), 1);
    }

    #[test]
    fn test_where_clause_partition_key_only() {
        let pk_cols = &["user_id"];
        let pk_values = vec![CqlValue::uuid(uuid::Uuid::nil())];
        let (clause, values) = build_where_clause(pk_cols, &pk_values, &[])
            .expect("the fixture binds every partition column");
        assert_eq!(clause, "user_id = ?");
        assert_eq!(values.len(), 1);
    }

    #[test]
    fn test_where_clause_composite_partition_key() {
        let pk_cols = &["tenant_id", "metric_id"];
        let pk_values = vec![CqlValue::text("tenant1"), CqlValue::BigInt(42)];
        let (clause, values) = build_where_clause(pk_cols, &pk_values, &[])
            .expect("the fixture binds every partition column");
        assert_eq!(clause, "tenant_id = ? AND metric_id = ?");
        assert_eq!(values.len(), 2);
    }

    #[test]
    fn test_where_clause_with_clustering_filter() {
        let pk_cols = &["user_id"];
        let pk_values = vec![CqlValue::uuid(uuid::Uuid::nil())];
        let filters = vec![ClusteringFilter {
            column: "timestamp".into(),
            op: ClusteringOp::Gte(CqlValue::BigInt(1000)),
        }];
        let (clause, values) = build_where_clause(pk_cols, &pk_values, &filters)
            .expect("the fixture binds every partition column");
        assert_eq!(clause, "user_id = ? AND timestamp >= ?");
        assert_eq!(values.len(), 2);
    }

    #[test]
    fn test_where_clause_with_multiple_clustering_filters() {
        let pk_cols = &["user_id"];
        let pk_values = vec![CqlValue::uuid(uuid::Uuid::nil())];
        let filters = vec![
            ClusteringFilter {
                column: "timestamp".into(),
                op: ClusteringOp::Gte(CqlValue::BigInt(1000)),
            },
            ClusteringFilter {
                column: "timestamp".into(),
                op: ClusteringOp::Lt(CqlValue::BigInt(2000)),
            },
        ];
        let (clause, values) = build_where_clause(pk_cols, &pk_values, &filters)
            .expect("the fixture binds every partition column");
        assert_eq!(clause, "user_id = ? AND timestamp >= ? AND timestamp < ?");
        assert_eq!(values.len(), 3);
    }

    #[test]
    fn test_select_cql_basic() {
        let (cql, values) = build_select_cql(
            "events.user_events".into(),
            None,
            &["user_id"],
            &[CqlValue::uuid(uuid::Uuid::nil())],
            &[],
            None,
            false,
        )
        .unwrap();
        assert_eq!(cql, "SELECT * FROM events.user_events WHERE user_id = ?");
        assert_eq!(values.len(), 1);
    }

    #[test]
    fn test_select_cql_with_columns() {
        let cols = vec!["user_id".into(), "timestamp".into(), "event_type".into()];
        let (cql, _) = build_select_cql(
            "events.user_events".into(),
            Some(&cols),
            &["user_id"],
            &[CqlValue::uuid(uuid::Uuid::nil())],
            &[],
            None,
            false,
        )
        .unwrap();
        assert_eq!(
            cql,
            "SELECT user_id, timestamp, event_type FROM events.user_events WHERE user_id = ?"
        );
    }

    #[test]
    fn test_select_cql_with_limit() {
        let (cql, _) = build_select_cql(
            "events.user_events".into(),
            None,
            &["user_id"],
            &[CqlValue::uuid(uuid::Uuid::nil())],
            &[],
            Some(100),
            false,
        )
        .unwrap();
        assert_eq!(
            cql,
            "SELECT * FROM events.user_events WHERE user_id = ? LIMIT 100"
        );
    }

    #[test]
    fn test_select_cql_with_allow_filtering() {
        let (cql, _) = build_select_cql(
            "events.user_events".into(),
            None,
            &["user_id"],
            &[CqlValue::uuid(uuid::Uuid::nil())],
            &[],
            None,
            true,
        )
        .unwrap();
        assert_eq!(
            cql,
            "SELECT * FROM events.user_events WHERE user_id = ? ALLOW FILTERING"
        );
    }

    #[test]
    fn test_select_cql_with_clustering_range() {
        let filters = vec![ClusteringFilter {
            column: "timestamp".into(),
            op: ClusteringOp::Range {
                low: Bound::Included(CqlValue::BigInt(1000)),
                high: Bound::Excluded(CqlValue::BigInt(2000)),
            },
        }];
        let (cql, values) = build_select_cql(
            "events.user_events".into(),
            None,
            &["user_id"],
            &[CqlValue::uuid(uuid::Uuid::nil())],
            &filters,
            None,
            false,
        )
        .unwrap();
        assert_eq!(
            cql,
            "SELECT * FROM events.user_events WHERE user_id = ? AND timestamp >= ? AND timestamp < ?"
        );
        assert_eq!(values.len(), 3);
    }

    #[test]
    fn test_select_cql_full_query() {
        let cols = vec!["event_type".into(), "data".into()];
        let filters = vec![
            ClusteringFilter {
                column: "timestamp".into(),
                op: ClusteringOp::Gte(CqlValue::BigInt(1000)),
            },
            ClusteringFilter {
                column: "timestamp".into(),
                op: ClusteringOp::Lt(CqlValue::BigInt(2000)),
            },
        ];
        let (cql, values) = build_select_cql(
            "events.user_events".into(),
            Some(&cols),
            &["user_id"],
            &[CqlValue::uuid(uuid::Uuid::nil())],
            &filters,
            Some(50),
            false,
        )
        .unwrap();
        assert_eq!(
            cql,
            "SELECT event_type, data FROM events.user_events \
             WHERE user_id = ? AND timestamp >= ? AND timestamp < ? LIMIT 50"
        );
        assert_eq!(values.len(), 3);
    }

    #[test]
    fn test_select_cql_composite_partition_key() {
        let (cql, values) = build_select_cql(
            "metrics.time_series".into(),
            None,
            &["tenant_id", "metric_id"],
            &[CqlValue::text("tenant1"), CqlValue::BigInt(42)],
            &[],
            None,
            false,
        )
        .unwrap();
        assert_eq!(
            cql,
            "SELECT * FROM metrics.time_series WHERE tenant_id = ? AND metric_id = ?"
        );
        assert_eq!(values.len(), 2);
    }

    /// Documents the typestate contract; the module's `compile_fail` doctest enforces it.
    #[test]
    fn test_typestate_documentation() {}

    #[test]
    fn test_empty_clustering_filters() {
        let (clause, values) =
            build_where_clause(&["user_id"], &[CqlValue::uuid(uuid::Uuid::nil())], &[])
                .expect("the fixture binds every partition column");
        assert_eq!(clause, "user_id = ?");
        assert_eq!(values.len(), 1);
    }

    #[test]
    fn test_unbounded_range() {
        let filter = ClusteringFilter {
            column: "timestamp".into(),
            op: ClusteringOp::Range {
                low: Bound::Unbounded,
                high: Bound::Unbounded,
            },
        };
        let (clause, values) = build_clustering_filter(&filter);
        assert_eq!(clause, "");
        assert!(values.is_empty());
    }

    #[test]
    fn test_limit_zero() {
        let (cql, _) = build_select_cql(
            "events.user_events".into(),
            None,
            &["user_id"],
            &[CqlValue::uuid(uuid::Uuid::nil())],
            &[],
            Some(0),
            false,
        )
        .unwrap();
        assert!(cql.contains("LIMIT 0"));
    }

    #[test]
    fn test_special_column_names() {
        // Column names that might need quoting in CQL
        let (cql, _) = build_select_cql(
            "keyspace.table".into(),
            None,
            &["user_id"],
            &[CqlValue::text("test")],
            &[],
            None,
            false,
        )
        .unwrap();
        // Not quoted: this pins current behaviour.
        assert!(cql.contains("user_id = ?"));
    }
}
