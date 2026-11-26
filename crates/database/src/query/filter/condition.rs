//! Filter condition nodes.

use super::{Filter, FilterValue};

/// A filter condition.
#[derive(Debug, Clone)]
pub enum FilterCondition {
    /// Equality: column = value
    Eq {
        /// Name of the column being compared.
        column: String,
        /// Value the column must equal.
        value: FilterValue,
    },
    /// Not equal: column != value
    Ne {
        /// Name of the column being compared.
        column: String,
        /// Value the column must differ from.
        value: FilterValue,
    },
    /// Greater than: column > value
    Gt {
        /// Name of the column being compared.
        column: String,
        /// Exclusive lower bound the column must exceed.
        value: FilterValue,
    },
    /// Greater than or equal: column >= value
    Gte {
        /// Name of the column being compared.
        column: String,
        /// Inclusive lower bound the column must meet or exceed.
        value: FilterValue,
    },
    /// Less than: column < value
    Lt {
        /// Name of the column being compared.
        column: String,
        /// Exclusive upper bound the column must stay below.
        value: FilterValue,
    },
    /// Less than or equal: column <= value
    Lte {
        /// Name of the column being compared.
        column: String,
        /// Inclusive upper bound the column must stay at or below.
        value: FilterValue,
    },
    /// In list: column IN (values)
    In {
        /// Name of the column being tested for membership.
        column: String,
        /// Set of allowed values.
        values: Vec<FilterValue>,
    },
    /// Not in list: column NOT IN (values)
    NotIn {
        /// Name of the column being tested for membership.
        column: String,
        /// Set of disallowed values.
        values: Vec<FilterValue>,
    },
    /// Is null: column IS NULL
    IsNull {
        /// Name of the column that must be null.
        column: String,
    },
    /// Is not null: column IS NOT NULL
    IsNotNull {
        /// Name of the column that must not be null.
        column: String,
    },
    /// Like pattern: column LIKE pattern (SQL only)
    Like {
        /// Name of the column being matched.
        column: String,
        /// SQL `LIKE` pattern (case-sensitive).
        pattern: String,
    },
    /// Case-insensitive like pattern: column ILIKE pattern (PostgreSQL/YSQL)
    ILike {
        /// Name of the column being matched.
        column: String,
        /// SQL `ILIKE` pattern (case-insensitive).
        pattern: String,
    },
    /// Between: column BETWEEN low AND high
    Between {
        /// Name of the column being range-checked.
        column: String,
        /// Inclusive lower bound of the range.
        low: FilterValue,
        /// Inclusive upper bound of the range.
        high: FilterValue,
    },
    /// Row-value (tuple) comparison: `(a, b, …) > (x, y, …)`.
    ///
    /// The keyset-pagination predicate. Hand-assembled `a > x AND b > y` drops rows, and the
    /// correct `OR` expansion cannot seek an index. The columns must be exactly the query's
    /// `ORDER BY` tuple, and `columns` and `values` the same non-zero length; a backend that
    /// cannot express it refuses it.
    RowValueGt {
        /// The ordered tuple of columns being compared, left to right.
        columns: Vec<String>,
        /// The boundary tuple, positionally matching `columns`.
        values: Vec<FilterValue>,
    },
    /// Row-value (tuple) comparison: `(a, b, …) < (x, y, …)`.
    ///
    /// The descending counterpart of [`FilterCondition::RowValueGt`].
    RowValueLt {
        /// The ordered tuple of columns being compared, left to right.
        columns: Vec<String>,
        /// The boundary tuple, positionally matching `columns`.
        values: Vec<FilterValue>,
    },
    /// Logical AND
    And(Vec<Filter>),
    /// Logical OR
    Or(Vec<Filter>),
    /// Logical NOT
    Not(Box<Filter>),
}

impl FilterCondition {
    /// Renames every column this condition names, nested ones included, to what `rename` answers
    /// for it; a column it answers `None` for keeps its name.
    pub fn rename_columns(&mut self, rename: &impl Fn(&str) -> Option<String>) {
        let one = |column: &mut String| {
            if let Some(renamed) = rename(column) {
                *column = renamed;
            }
        };
        match self {
            Self::Eq { column, .. }
            | Self::Ne { column, .. }
            | Self::Gt { column, .. }
            | Self::Gte { column, .. }
            | Self::Lt { column, .. }
            | Self::Lte { column, .. }
            | Self::In { column, .. }
            | Self::NotIn { column, .. }
            | Self::IsNull { column }
            | Self::IsNotNull { column }
            | Self::Like { column, .. }
            | Self::ILike { column, .. }
            | Self::Between { column, .. } => one(column),
            Self::RowValueGt { columns, .. } | Self::RowValueLt { columns, .. } => {
                columns.iter_mut().for_each(one);
            }
            Self::And(filters) | Self::Or(filters) => filters
                .iter_mut()
                .for_each(|filter| filter.condition.rename_columns(rename)),
            Self::Not(filter) => filter.condition.rename_columns(rename),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_column_a_condition_names_is_renamed_nested_ones_too() {
        let mut filter = Filter::and(vec![
            Filter::icontains("title", "fiets"),
            Filter::not(Filter::eq("position", 1)),
            Filter::or(vec![Filter::eq("title", "Zon"), Filter::is_null("title")]),
        ]);
        filter
            .condition
            .rename_columns(&|column| (column == "title").then(|| "shown_title".to_owned()));
        let said = format!("{filter:?}");
        assert!(!said.contains("\"title\""), "{said}");
        assert_eq!(said.matches("shown_title").count(), 3, "{said}");
        assert!(said.contains("\"position\""), "{said}");
    }
}
