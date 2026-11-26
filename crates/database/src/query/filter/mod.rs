//! Query filter conditions, which backends translate into their own dialect.

mod condition;
mod value;

pub use condition::FilterCondition;
pub use value::FilterValue;

/// A query filter condition.
///
/// # Examples
///
/// ```
/// use fundamental_db::Filter;
///
/// let active_adults = Filter::and(vec![
///     Filter::eq("status", "active"),
///     Filter::gt("age", 18),
/// ]);
/// ```
#[derive(Debug, Clone)]
pub struct Filter {
    /// The condition.
    pub condition: FilterCondition,
}

impl Filter {
    /// Creates an equality filter.
    pub fn eq(column: impl Into<String>, value: impl Into<FilterValue>) -> Self {
        Self {
            condition: FilterCondition::Eq {
                column: column.into(),
                value: value.into(),
            },
        }
    }

    /// Creates a not-equal filter.
    pub fn ne(column: impl Into<String>, value: impl Into<FilterValue>) -> Self {
        Self {
            condition: FilterCondition::Ne {
                column: column.into(),
                value: value.into(),
            },
        }
    }

    /// Creates a greater-than filter.
    pub fn gt(column: impl Into<String>, value: impl Into<FilterValue>) -> Self {
        Self {
            condition: FilterCondition::Gt {
                column: column.into(),
                value: value.into(),
            },
        }
    }

    /// Creates a greater-than-or-equal filter.
    pub fn gte(column: impl Into<String>, value: impl Into<FilterValue>) -> Self {
        Self {
            condition: FilterCondition::Gte {
                column: column.into(),
                value: value.into(),
            },
        }
    }

    /// Creates a less-than filter.
    pub fn lt(column: impl Into<String>, value: impl Into<FilterValue>) -> Self {
        Self {
            condition: FilterCondition::Lt {
                column: column.into(),
                value: value.into(),
            },
        }
    }

    /// Creates a less-than-or-equal filter.
    pub fn lte(column: impl Into<String>, value: impl Into<FilterValue>) -> Self {
        Self {
            condition: FilterCondition::Lte {
                column: column.into(),
                value: value.into(),
            },
        }
    }

    /// Creates an IN filter.
    pub fn in_list(column: impl Into<String>, values: Vec<FilterValue>) -> Self {
        Self {
            condition: FilterCondition::In {
                column: column.into(),
                values,
            },
        }
    }

    /// Creates a NOT IN filter.
    pub fn not_in(column: impl Into<String>, values: Vec<FilterValue>) -> Self {
        Self {
            condition: FilterCondition::NotIn {
                column: column.into(),
                values,
            },
        }
    }

    /// Creates an IS NULL filter.
    pub fn is_null(column: impl Into<String>) -> Self {
        Self {
            condition: FilterCondition::IsNull {
                column: column.into(),
            },
        }
    }

    /// Creates an IS NOT NULL filter.
    pub fn is_not_null(column: impl Into<String>) -> Self {
        Self {
            condition: FilterCondition::IsNotNull {
                column: column.into(),
            },
        }
    }

    /// Creates a LIKE filter (SQL only).
    pub fn like(column: impl Into<String>, pattern: impl Into<String>) -> Self {
        Self {
            condition: FilterCondition::Like {
                column: column.into(),
                pattern: pattern.into(),
            },
        }
    }

    /// Creates a case-insensitive ILIKE filter (PostgreSQL/YSQL).
    pub fn ilike(column: impl Into<String>, pattern: impl Into<String>) -> Self {
        Self {
            condition: FilterCondition::ILike {
                column: column.into(),
                pattern: pattern.into(),
            },
        }
    }

    /// Creates a case-insensitive *contains* filter over `text` taken literally.
    ///
    /// `text` is never a pattern: `%`, `_` and `\` are escaped before it is wrapped in `%…%`.
    pub fn icontains(column: impl Into<String>, text: &str) -> Self {
        let mut pattern = String::with_capacity(text.len() + 2);
        pattern.push('%');
        for character in text.chars() {
            if matches!(character, '\\' | '%' | '_') {
                pattern.push('\\');
            }
            pattern.push(character);
        }
        pattern.push('%');
        Self::ilike(column, pattern)
    }

    /// Creates a BETWEEN filter.
    pub fn between(
        column: impl Into<String>,
        low: impl Into<FilterValue>,
        high: impl Into<FilterValue>,
    ) -> Self {
        Self {
            condition: FilterCondition::Between {
                column: column.into(),
                low: low.into(),
                high: high.into(),
            },
        }
    }

    /// Creates a row-value (tuple) greater-than filter — `(a, b) > (x, y)`.
    ///
    /// The keyset-pagination predicate: give it the query's `ORDER BY` tuple and the last row's
    /// values. Never hand-assemble it from [`Filter::gt`] and [`Filter::and`], which drops rows.
    ///
    /// # Panics
    /// Never; a mismatched or empty tuple is refused by the backend as a typed error.
    pub fn row_value_gt(columns: Vec<String>, values: Vec<FilterValue>) -> Self {
        Self {
            condition: FilterCondition::RowValueGt { columns, values },
        }
    }

    /// Creates a row-value (tuple) less-than filter — `(a, b) < (x, y)`.
    ///
    /// The descending counterpart of [`Filter::row_value_gt`].
    ///
    /// # Panics
    /// Never; see [`Filter::row_value_gt`].
    pub fn row_value_lt(columns: Vec<String>, values: Vec<FilterValue>) -> Self {
        Self {
            condition: FilterCondition::RowValueLt { columns, values },
        }
    }

    /// Combines filters with AND.
    pub fn and(filters: Vec<Filter>) -> Self {
        Self {
            condition: FilterCondition::And(filters),
        }
    }

    /// Combines filters with OR.
    pub fn or(filters: Vec<Filter>) -> Self {
        Self {
            condition: FilterCondition::Or(filters),
        }
    }

    /// Negates a filter.
    // A named constructor beside `and`/`or`, not `std::ops::Not::not`.
    #[allow(clippy::should_implement_trait)]
    #[expect(
        clippy::allow_attributes,
        clippy::allow_attributes_without_reason,
        reason = "readability baseline"
    )]
    pub fn not(filter: Filter) -> Self {
        Self {
            condition: FilterCondition::Not(Box::new(filter)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Filter, FilterCondition};

    fn pattern_of(filter: Filter) -> String {
        match filter.condition {
            FilterCondition::ILike { pattern, .. } => pattern,
            other => panic!("icontains built {other:?}, not an ILIKE"),
        }
    }

    #[test]
    fn a_contains_search_is_never_a_pattern() {
        assert_eq!(pattern_of(Filter::icontains("name", "jan")), "%jan%");
        assert_eq!(pattern_of(Filter::icontains("name", "50%")), "%50\\%%");
        assert_eq!(pattern_of(Filter::icontains("name", "a_b")), "%a\\_b%");
        assert_eq!(pattern_of(Filter::icontains("name", "c:\\x")), "%c:\\\\x%");
        assert_eq!(pattern_of(Filter::icontains("name", "%")), "%\\%%");
    }
}
