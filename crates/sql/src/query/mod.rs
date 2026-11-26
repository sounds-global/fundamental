//! YSQL query construction and execution; every value is bound as `$N` and every identifier validated.

mod bind_key;
mod builder;
mod executor;
mod fns;
mod row_lock;
mod sql_str;

pub use bind_key::BindKey;
pub use builder::SqlQuery;
pub use executor::SqlQueryExecutor;
pub(crate) use row_lock::RowLock;

pub(crate) use sql_str::pg_query;

pub(crate) use fns::*;

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::model::{BoundTo, Model, SqlDb, SqlModel};
    use fundamental_db::{Filter, FilterCondition, FilterValue, OrderBy, QueryBuilder, SchemaName};

    /// Hand-implemented, as the derive needs named fields and these tests need only the `SqlModel` consts.
    struct TestUser;

    impl Model for TestUser {
        const TYPE_NAME: &'static str = "TestUser";
    }

    impl BoundTo<SqlDb> for TestUser {}

    impl SqlModel for TestUser {
        type Id = i64;
        fn id(&self) -> &Self::Id {
            &0
        }
        const TABLE: &'static str = "users";
        const PRIMARY_KEY: &'static str = "id";
    }

    #[test]
    fn test_filter_eq() {
        let (sql, values, next_idx) = build_filter_condition(
            &FilterCondition::Eq {
                column: "name".into(),
                value: FilterValue::String("alice".into()),
            },
            1,
        )
        .unwrap();
        assert_eq!(sql, "name = $1");
        assert_eq!(values.len(), 1);
        assert_eq!(next_idx, 2);
    }

    #[test]
    fn test_filter_ne() {
        let (sql, values, next_idx) = build_filter_condition(
            &FilterCondition::Ne {
                column: "status".into(),
                value: FilterValue::String("deleted".into()),
            },
            1,
        )
        .unwrap();
        assert_eq!(sql, "status != $1");
        assert_eq!(values.len(), 1);
        assert_eq!(next_idx, 2);
    }

    #[test]
    fn test_filter_gt() {
        let (sql, _, next_idx) = build_filter_condition(
            &FilterCondition::Gt {
                column: "age".into(),
                value: FilterValue::Int(18),
            },
            1,
        )
        .unwrap();
        assert_eq!(sql, "age > $1");
        assert_eq!(next_idx, 2);
    }

    #[test]
    fn test_filter_gte() {
        let (sql, _, _) = build_filter_condition(
            &FilterCondition::Gte {
                column: "score".into(),
                value: FilterValue::Int(100),
            },
            5,
        )
        .unwrap();
        assert_eq!(sql, "score >= $5");
    }

    #[test]
    fn test_filter_lt() {
        let (sql, _, _) = build_filter_condition(
            &FilterCondition::Lt {
                column: "price".into(),
                value: FilterValue::Float(99.99),
            },
            1,
        )
        .unwrap();
        assert_eq!(sql, "price < $1");
    }

    #[test]
    fn test_filter_lte() {
        let (sql, _, _) = build_filter_condition(
            &FilterCondition::Lte {
                column: "quantity".into(),
                value: FilterValue::Int(0),
            },
            1,
        )
        .unwrap();
        assert_eq!(sql, "quantity <= $1");
    }

    #[test]
    fn test_filter_in() {
        let (sql, values, next_idx) = build_filter_condition(
            &FilterCondition::In {
                column: "status".into(),
                values: vec![
                    FilterValue::String("active".into()),
                    FilterValue::String("pending".into()),
                    FilterValue::String("review".into()),
                ],
            },
            1,
        )
        .unwrap();
        assert_eq!(sql, "status IN ($1, $2, $3)");
        assert_eq!(values.len(), 3);
        assert_eq!(next_idx, 4);
    }

    #[test]
    fn test_filter_not_in() {
        let (sql, values, next_idx) = build_filter_condition(
            &FilterCondition::NotIn {
                column: "id".into(),
                values: vec![FilterValue::Int(1), FilterValue::Int(2)],
            },
            1,
        )
        .unwrap();
        assert_eq!(sql, "id NOT IN ($1, $2)");
        assert_eq!(values.len(), 2);
        assert_eq!(next_idx, 3);
    }

    #[test]
    fn test_filter_is_null() {
        let (sql, values, next_idx) = build_filter_condition(
            &FilterCondition::IsNull {
                column: "deleted_at".into(),
            },
            1,
        )
        .unwrap();
        assert_eq!(sql, "deleted_at IS NULL");
        assert!(values.is_empty());
        assert_eq!(next_idx, 1); // No parameter consumed
    }

    #[test]
    fn test_filter_is_not_null() {
        let (sql, values, next_idx) = build_filter_condition(
            &FilterCondition::IsNotNull {
                column: "email".into(),
            },
            1,
        )
        .unwrap();
        assert_eq!(sql, "email IS NOT NULL");
        assert!(values.is_empty());
        assert_eq!(next_idx, 1);
    }

    #[test]
    fn test_filter_like() {
        let (sql, values, next_idx) = build_filter_condition(
            &FilterCondition::Like {
                column: "name".into(),
                pattern: "%alice%".into(),
            },
            1,
        )
        .unwrap();
        assert_eq!(sql, "name LIKE $1");
        assert_eq!(values.len(), 1);
        assert_eq!(next_idx, 2);
    }

    #[test]
    fn test_filter_between() {
        let (sql, values, next_idx) = build_filter_condition(
            &FilterCondition::Between {
                column: "created_at".into(),
                low: FilterValue::Timestamp(1000),
                high: FilterValue::Timestamp(2000),
            },
            1,
        )
        .unwrap();
        assert_eq!(sql, "created_at BETWEEN $1 AND $2");
        assert_eq!(values.len(), 2);
        assert_eq!(next_idx, 3);
    }

    #[test]
    fn test_filter_and() {
        let (sql, values, next_idx) = build_filter_condition(
            &FilterCondition::And(vec![Filter::eq("status", "active"), Filter::gt("age", 18)]),
            1,
        )
        .unwrap();
        assert_eq!(sql, "(status = $1 AND age > $2)");
        assert_eq!(values.len(), 2);
        assert_eq!(next_idx, 3);
    }

    #[test]
    fn test_filter_or() {
        let (sql, values, next_idx) = build_filter_condition(
            &FilterCondition::Or(vec![
                Filter::eq("role", "admin"),
                Filter::eq("role", "moderator"),
            ]),
            1,
        )
        .unwrap();
        assert_eq!(sql, "(role = $1 OR role = $2)");
        assert_eq!(values.len(), 2);
        assert_eq!(next_idx, 3);
    }

    #[test]
    fn test_filter_not() {
        let (sql, values, next_idx) = build_filter_condition(
            &FilterCondition::Not(Box::new(Filter::eq("deleted", true))),
            1,
        )
        .unwrap();
        assert_eq!(sql, "NOT (deleted = $1)");
        assert_eq!(values.len(), 1);
        assert_eq!(next_idx, 2);
    }

    #[test]
    fn test_filter_nested_and_or() {
        let (sql, values, next_idx) = build_filter_condition(
            &FilterCondition::And(vec![
                Filter::eq("status", "active"),
                Filter::or(vec![
                    Filter::eq("role", "admin"),
                    Filter::eq("role", "moderator"),
                ]),
            ]),
            1,
        )
        .unwrap();
        assert_eq!(sql, "(status = $1 AND (role = $2 OR role = $3))");
        assert_eq!(values.len(), 3);
        assert_eq!(next_idx, 4);
    }

    #[test]
    fn test_parameter_index_starts_at_given_value() {
        let (sql, _, next_idx) = build_filter_condition(
            &FilterCondition::Eq {
                column: "x".into(),
                value: FilterValue::Int(1),
            },
            10,
        )
        .unwrap();
        assert_eq!(sql, "x = $10");
        assert_eq!(next_idx, 11);
    }

    #[test]
    fn test_parameter_index_continuity_in_and() {
        let (sql, _, next_idx) = build_filter_condition(
            &FilterCondition::And(vec![
                Filter::eq("a", 1),
                Filter::eq("b", 2),
                Filter::eq("c", 3),
            ]),
            1,
        )
        .unwrap();
        assert_eq!(sql, "(a = $1 AND b = $2 AND c = $3)");
        assert_eq!(next_idx, 4);
    }

    #[test]
    fn test_where_clause_empty() {
        let (sql, values, _) = build_where_clause(&[], 1).unwrap();
        assert_eq!(sql, "");
        assert!(values.is_empty());
    }

    #[test]
    fn test_where_clause_single_filter() {
        let filters = vec![Filter::eq("active", true)];
        let (sql, values, _) = build_where_clause(&filters, 1).unwrap();
        assert_eq!(sql, "active = $1");
        assert_eq!(values.len(), 1);
    }

    #[test]
    fn test_where_clause_multiple_filters_joined_with_and() {
        let filters = vec![
            Filter::eq("status", "active"),
            Filter::gt("age", 18),
            Filter::is_not_null("email".to_string()),
        ];
        let (sql, values, next_idx) = build_where_clause(&filters, 1).unwrap();
        assert_eq!(sql, "status = $1 AND age > $2 AND email IS NOT NULL");
        assert_eq!(values.len(), 2); // IS NOT NULL has no value
        assert_eq!(next_idx, 3);
    }

    #[test]
    fn test_order_by_single_asc() {
        let orders = vec![OrderBy::asc("name")];
        let clause = build_order_clause(&orders).unwrap();
        assert_eq!(clause, "name ASC");
    }

    #[test]
    fn test_order_by_single_desc() {
        let orders = vec![OrderBy::desc("created_at")];
        let clause = build_order_clause(&orders).unwrap();
        assert_eq!(clause, "created_at DESC");
    }

    #[test]
    fn test_order_by_with_nulls_first() {
        let orders = vec![OrderBy::asc("priority").nulls_first()];
        let clause = build_order_clause(&orders).unwrap();
        assert_eq!(clause, "priority ASC NULLS FIRST");
    }

    #[test]
    fn test_order_by_with_nulls_last() {
        let orders = vec![OrderBy::desc("updated_at").nulls_last()];
        let clause = build_order_clause(&orders).unwrap();
        assert_eq!(clause, "updated_at DESC NULLS LAST");
    }

    #[test]
    fn test_order_by_multiple() {
        let orders = vec![
            OrderBy::desc("created_at"),
            OrderBy::asc("name").nulls_last(),
        ];
        let clause = build_order_clause(&orders).unwrap();
        assert_eq!(clause, "created_at DESC, name ASC NULLS LAST");
    }

    #[test]
    fn test_select_sql_no_filters() {
        let query = QueryBuilder::<TestUser, SqlDb>::new();
        let (sql, values) =
            build_select_sql::<TestUser, _>(&SchemaName::default(), &query, None).unwrap();
        assert_eq!(sql, "SELECT * FROM public.users");
        assert!(values.is_empty());
    }

    #[test]
    fn test_select_sql_with_filter() {
        let query = QueryBuilder::<TestUser, SqlDb>::new().filter(Filter::eq("status", "active"));
        let (sql, values) =
            build_select_sql::<TestUser, _>(&SchemaName::default(), &query, None).unwrap();
        assert_eq!(sql, "SELECT * FROM public.users WHERE status = $1");
        assert_eq!(values.len(), 1);
    }

    #[test]
    fn test_select_sql_with_order_by() {
        let query = QueryBuilder::<TestUser, SqlDb>::new().order_by(OrderBy::desc("created_at"));
        let (sql, _) =
            build_select_sql::<TestUser, _>(&SchemaName::default(), &query, None).unwrap();
        assert_eq!(sql, "SELECT * FROM public.users ORDER BY created_at DESC");
    }

    #[test]
    fn test_select_sql_with_limit() {
        let query = QueryBuilder::<TestUser, SqlDb>::new().limit(10);
        let (sql, _) =
            build_select_sql::<TestUser, _>(&SchemaName::default(), &query, None).unwrap();
        assert_eq!(sql, "SELECT * FROM public.users LIMIT 10");
    }

    #[test]
    fn test_select_sql_with_limit_and_offset() {
        let query = QueryBuilder::<TestUser, SqlDb>::new().limit(10).offset(20);
        let (sql, _) =
            build_select_sql::<TestUser, _>(&SchemaName::default(), &query, None).unwrap();
        assert_eq!(sql, "SELECT * FROM public.users LIMIT 10 OFFSET 20");
    }

    #[test]
    fn test_select_sql_with_select_columns() {
        let query = QueryBuilder::<TestUser, SqlDb>::new().select(["id", "name", "email"]);
        let (sql, _) =
            build_select_sql::<TestUser, _>(&SchemaName::default(), &query, None).unwrap();
        assert_eq!(sql, "SELECT id, name, email FROM public.users");
    }

    #[test]
    fn test_select_sql_full_query() {
        let query = QueryBuilder::<TestUser, SqlDb>::new()
            .select(["id", "name"])
            .filter(Filter::eq("status", "active"))
            .filter(Filter::gt("age", 18))
            .order_by(OrderBy::desc("created_at"))
            .order_by(OrderBy::asc("name"))
            .limit(10)
            .offset(5);
        let (sql, values) =
            build_select_sql::<TestUser, _>(&SchemaName::default(), &query, None).unwrap();
        assert_eq!(
            sql,
            "SELECT id, name FROM public.users \
             WHERE status = $1 AND age > $2 \
             ORDER BY created_at DESC, name ASC \
             LIMIT 10 OFFSET 5"
        );
        assert_eq!(values.len(), 2);
    }

    #[test]
    fn an_offset_without_a_limit_reads_every_row_past_it() {
        let query = QueryBuilder::<TestUser, SqlDb>::new().offset(3);
        let (sql, _) =
            build_select_sql::<TestUser, _>(&SchemaName::default(), &query, None).unwrap();
        assert_eq!(sql, "SELECT * FROM public.users OFFSET 3");
    }

    #[test]
    fn test_count_sql_no_filters() {
        let query = QueryBuilder::<TestUser, SqlDb>::new();
        let (sql, values) = build_count_sql::<TestUser, _>(&SchemaName::default(), &query).unwrap();
        assert_eq!(sql, "SELECT COUNT(*) FROM public.users");
        assert!(values.is_empty());
    }

    #[test]
    fn test_count_sql_with_filters() {
        let query = QueryBuilder::<TestUser, SqlDb>::new()
            .filter(Filter::eq("active", true))
            .filter(Filter::is_null("deleted_at".to_string()));
        let (sql, values) = build_count_sql::<TestUser, _>(&SchemaName::default(), &query).unwrap();
        assert_eq!(
            sql,
            "SELECT COUNT(*) FROM public.users WHERE active = $1 AND deleted_at IS NULL"
        );
        assert_eq!(values.len(), 1);
    }

    #[test]
    fn test_delete_sql_no_filters() {
        let query = QueryBuilder::<TestUser, SqlDb>::new();
        let (sql, values) =
            build_delete_sql::<TestUser, _>(&SchemaName::default(), &query).unwrap();
        assert_eq!(sql, "DELETE FROM public.users");
        assert!(values.is_empty());
    }

    #[test]
    fn test_delete_sql_with_filters() {
        let query = QueryBuilder::<TestUser, SqlDb>::new()
            .filter(Filter::eq("status", "deleted"))
            .filter(Filter::lt("deleted_at", 1000));
        let (sql, values) =
            build_delete_sql::<TestUser, _>(&SchemaName::default(), &query).unwrap();
        assert_eq!(
            sql,
            "DELETE FROM public.users WHERE status = $1 AND deleted_at < $2"
        );
        assert_eq!(values.len(), 2);
    }

    #[test]
    fn a_limited_delete_chooses_its_keys_first() {
        let query = QueryBuilder::<TestUser, SqlDb>::new()
            .filter(Filter::eq("status", "deleted"))
            .order_by(OrderBy::asc("created_at"))
            .limit(1000);
        let (sql, values) =
            build_delete_sql::<TestUser, _>(&SchemaName::default(), &query).unwrap();
        assert_eq!(
            sql,
            "DELETE FROM public.users WHERE id IN (SELECT id FROM public.users WHERE status = $1 ORDER BY created_at ASC LIMIT 1000) AND (status = $1)"
        );
        assert_eq!(values.len(), 1);
    }

    #[test]
    fn an_ordered_delete_without_a_limit_is_still_addressed_by_key() {
        let query = QueryBuilder::<TestUser, SqlDb>::new().order_by(OrderBy::desc("created_at"));
        let (sql, _) = build_delete_sql::<TestUser, _>(&SchemaName::default(), &query).unwrap();
        assert_eq!(
            sql,
            "DELETE FROM public.users WHERE id IN (SELECT id FROM public.users ORDER BY created_at DESC)"
        );
    }

    #[test]
    fn test_update_sql_single_field() {
        let query = QueryBuilder::<TestUser, SqlDb>::new().filter(Filter::eq("id", 123));
        let updates = vec![("status".to_string(), FilterValue::String("inactive".into()))];
        let (sql, values) =
            build_update_sql::<TestUser, _>(&SchemaName::default(), &query, &updates).unwrap();
        assert_eq!(sql, "UPDATE public.users SET status = $1 WHERE id = $2");
        assert_eq!(values.len(), 2);
    }

    #[test]
    fn test_update_sql_multiple_fields() {
        let query = QueryBuilder::<TestUser, SqlDb>::new().filter(Filter::eq("id", 123));
        let updates = vec![
            ("name".to_string(), FilterValue::String("Alice".into())),
            (
                "email".to_string(),
                FilterValue::String("alice@example.com".into()),
            ),
            ("updated_at".to_string(), FilterValue::Timestamp(1234567890)),
        ];
        let (sql, values) =
            build_update_sql::<TestUser, _>(&SchemaName::default(), &query, &updates).unwrap();
        assert_eq!(
            sql,
            "UPDATE public.users SET name = $1, email = $2, updated_at = $3 WHERE id = $4"
        );
        assert_eq!(values.len(), 4);
    }

    #[test]
    fn test_update_sql_no_where_clause() {
        let query = QueryBuilder::<TestUser, SqlDb>::new();
        let updates = vec![("active".to_string(), FilterValue::Bool(false))];
        let (sql, _) =
            build_update_sql::<TestUser, _>(&SchemaName::default(), &query, &updates).unwrap();
        assert_eq!(sql, "UPDATE public.users SET active = $1");
    }

    #[test]
    fn test_empty_in_list() {
        let (sql, values, next_idx) = build_filter_condition(
            &FilterCondition::In {
                column: "id".into(),
                values: vec![],
            },
            1,
        )
        .unwrap();
        assert_eq!(sql, "1 = 0");
        assert!(values.is_empty());
        assert_eq!(next_idx, 1); // No parameters consumed
    }

    #[test]
    fn test_empty_not_in_list() {
        let (sql, values, next_idx) = build_filter_condition(
            &FilterCondition::NotIn {
                column: "id".into(),
                values: vec![],
            },
            1,
        )
        .unwrap();
        assert_eq!(sql, "1 = 1");
        assert!(values.is_empty());
        assert_eq!(next_idx, 1);
    }

    #[test]
    fn test_deeply_nested_conditions() {
        let condition = FilterCondition::Or(vec![
            Filter::and(vec![Filter::eq("a", 1), Filter::eq("b", 2)]),
            Filter::and(vec![Filter::eq("c", 3), Filter::eq("d", 4)]),
        ]);
        let (sql, values, next_idx) = build_filter_condition(&condition, 1).unwrap();
        assert_eq!(sql, "((a = $1 AND b = $2) OR (c = $3 AND d = $4))");
        assert_eq!(values.len(), 4);
        assert_eq!(next_idx, 5);
    }

    #[test]
    fn test_filter_with_special_column_names() {
        let (sql, _, _) = build_filter_condition(
            &FilterCondition::Eq {
                column: "user.name".into(), // Qualified name
                value: FilterValue::String("test".into()),
            },
            1,
        )
        .unwrap();
        assert_eq!(sql, "user.name = $1");
    }

    #[test]
    fn test_string_with_quotes() {
        let (sql, values, _) = build_filter_condition(
            &FilterCondition::Eq {
                column: "name".into(),
                value: FilterValue::String("O'Brien".into()),
            },
            1,
        )
        .unwrap();
        assert_eq!(sql, "name = $1");
        match &values[0] {
            FilterValue::String(s) => assert_eq!(s, "O'Brien"),
            _ => panic!("Expected string value"),
        }
    }

    #[test]
    fn test_update_sql_with_null_value() {
        let query = QueryBuilder::<TestUser, SqlDb>::new().filter(Filter::eq("id", 123));
        let updates = vec![
            ("name".to_string(), FilterValue::String("Alice".into())),
            ("deleted_at".to_string(), FilterValue::Null),
            ("updated_at".to_string(), FilterValue::Timestamp(1234567890)),
        ];
        let (sql, values) =
            build_update_sql::<TestUser, _>(&SchemaName::default(), &query, &updates).unwrap();
        assert_eq!(
            sql,
            "UPDATE public.users SET name = $1, deleted_at = NULL, updated_at = $2 WHERE id = $3"
        );
        assert_eq!(values.len(), 3);
    }

    #[test]
    fn test_update_sql_with_only_null_values() {
        let query = QueryBuilder::<TestUser, SqlDb>::new().filter(Filter::eq("id", 1));
        let updates = vec![
            ("field_a".to_string(), FilterValue::Null),
            ("field_b".to_string(), FilterValue::Null),
        ];
        let (sql, values) =
            build_update_sql::<TestUser, _>(&SchemaName::default(), &query, &updates).unwrap();
        assert_eq!(
            sql,
            "UPDATE public.users SET field_a = NULL, field_b = NULL WHERE id = $1"
        );
        assert_eq!(values.len(), 1);
    }

    #[test]
    fn test_multiple_between_filters() {
        let filters = vec![
            Filter::between("age", 18, 65),
            Filter::between("salary", 50000, 150000),
        ];
        let (sql, values, next_idx) = build_where_clause(&filters, 1).unwrap();
        assert_eq!(sql, "age BETWEEN $1 AND $2 AND salary BETWEEN $3 AND $4");
        assert_eq!(values.len(), 4);
        assert_eq!(next_idx, 5);
    }

    #[test]
    fn row_lock_is_appended_after_limit() {
        // PostgreSQL grammar puts the locking clause after LIMIT/OFFSET.
        let query = QueryBuilder::<TestUser, SqlDb>::new()
            .filter(Filter::is_null("relayed_at"))
            .order_by(OrderBy::asc("next_attempt_at"))
            .limit(128);
        let (sql, _) = build_select_sql::<TestUser, _>(
            &SchemaName::default(),
            &query,
            Some(RowLock::ForUpdateSkipLocked),
        )
        .unwrap();
        assert_eq!(
            sql,
            "SELECT * FROM public.users WHERE relayed_at IS NULL \
             ORDER BY next_attempt_at ASC LIMIT 128 FOR UPDATE SKIP LOCKED"
        );
    }

    #[test]
    fn no_row_lock_leaves_the_statement_untouched() {
        let query = QueryBuilder::<TestUser, SqlDb>::new().limit(10);
        let (locked, _) =
            build_select_sql::<TestUser, _>(&SchemaName::default(), &query, None).unwrap();
        assert_eq!(locked, "SELECT * FROM public.users LIMIT 10");
    }

    #[test]
    fn the_sql_query_wrapper_carries_the_lock() {
        let query = SqlQuery::new(QueryBuilder::<TestUser, SqlDb>::new());
        assert_eq!(query.get_row_lock(), None, "unlocked by default");
        assert_eq!(
            query.for_update_skip_locked().get_row_lock(),
            Some(RowLock::ForUpdateSkipLocked)
        );
    }

    // Identifiers are interpolated into SQL text, so a malicious column name must be refused.

    const INJECTION_PAYLOADS: &[&str] = &[
        "x; DROP TABLE users; --",
        "a\" OR \"1\"=\"1",
        "1 OR 1=1",
        "id); DELETE FROM users WHERE (1=1",
        "name/**/UNION/**/SELECT",
    ];

    #[test]
    fn test_filter_rejects_injection_in_column() {
        for payload in INJECTION_PAYLOADS {
            let res = build_filter_condition(
                &FilterCondition::Eq {
                    column: (*payload).into(),
                    value: FilterValue::String("v".into()),
                },
                1,
            );
            assert!(res.is_err(), "filter column {payload:?} must be rejected");
        }
    }

    #[test]
    fn test_select_columns_reject_injection() {
        for payload in INJECTION_PAYLOADS {
            let query = QueryBuilder::<TestUser, SqlDb>::new().select([*payload]);
            assert!(
                build_select_sql::<TestUser, _>(&SchemaName::default(), &query, None).is_err(),
                "select column {payload:?} must be rejected"
            );
        }
        let query = QueryBuilder::<TestUser, SqlDb>::new().select(["*"]);
        assert!(build_select_sql::<TestUser, _>(&SchemaName::default(), &query, None).is_ok());
    }

    #[test]
    fn test_order_by_rejects_injection() {
        for payload in INJECTION_PAYLOADS {
            let query = QueryBuilder::<TestUser, SqlDb>::new().order_by(OrderBy::asc(*payload));
            assert!(
                build_select_sql::<TestUser, _>(&SchemaName::default(), &query, None).is_err(),
                "order-by column {payload:?} must be rejected"
            );
        }
    }

    #[test]
    fn test_update_set_column_rejects_injection() {
        for payload in INJECTION_PAYLOADS {
            let query = QueryBuilder::<TestUser, SqlDb>::new().filter(Filter::eq("id", 1));
            let updates = vec![((*payload).to_string(), FilterValue::String("v".into()))];
            assert!(
                build_update_sql::<TestUser, _>(&SchemaName::default(), &query, &updates).is_err(),
                "update SET column {payload:?} must be rejected"
            );
        }
    }

    #[test]
    fn test_valid_qualified_identifier_still_allowed() {
        let (sql, _, _) = build_filter_condition(
            &FilterCondition::Eq {
                column: "public.users.name".into(),
                value: FilterValue::String("x".into()),
            },
            1,
        )
        .unwrap();
        assert_eq!(sql, "public.users.name = $1");
    }
}
