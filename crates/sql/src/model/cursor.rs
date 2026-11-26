//! Opaque cursor types for YSQL keyset pagination.

use chrono::DateTime;
use fundamental_db::{Filter, FilterValue};
use fundamental_macros::Error;

use serde::{Deserialize, Serialize};

/// A value that can be stored in a cursor (serializable subset of FilterValue).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CursorValue {
    /// String value.
    String(String),
    /// Integer value.
    Int(i64),
    /// Boolean value.
    Bool(bool),
    /// UUID value, as its canonical hyphenated text, bound as a real `UUID` so it compares at the column's type.
    Uuid(String),
    /// A timezone-aware instant, as microseconds since the Unix epoch: `TIMESTAMPTZ`'s precision, as rounding skips rows.
    Timestamp(i64),
}

impl From<CursorValue> for FilterValue {
    fn from(val: CursorValue) -> FilterValue {
        match val {
            CursorValue::String(s) => FilterValue::String(s),
            CursorValue::Int(i) => FilterValue::Int(i),
            CursorValue::Bool(b) => FilterValue::Bool(b),
            CursorValue::Uuid(u) => FilterValue::Uuid(u),
            // `TimestampTz`, as `FilterValue::Timestamp` is milliseconds.
            CursorValue::Timestamp(micros) => DateTime::from_timestamp_micros(micros)
                .map_or(FilterValue::Null, FilterValue::TimestampTz),
        }
    }
}

/// One ORDER BY component of a [`SqlCursor`]: a column, its boundary value, and its direction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorEntry {
    /// Name of the ordered column.
    pub column: String,
    /// Boundary value of the column from the last returned row.
    pub value: CursorValue,
    /// Whether the column is sorted descending.
    pub descending: bool,
}

/// Opaque, serializable cursor for YSQL keyset pagination.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqlCursor {
    /// Per-column boundary values, in ORDER BY order, defining the keyset position.
    pub values: Vec<CursorEntry>,
}

impl SqlCursor {
    /// Creates a new cursor with a single column value.
    pub fn new(column: impl Into<String>, value: CursorValue, descending: bool) -> Self {
        Self {
            values: vec![CursorEntry {
                column: column.into(),
                value,
                descending,
            }],
        }
    }

    /// Adds an additional column to the cursor (for multi-column sorts).
    #[must_use]
    pub fn and(mut self, column: impl Into<String>, value: CursorValue, descending: bool) -> Self {
        self.values.push(CursorEntry {
            column: column.into(),
            value,
            descending,
        });
        self
    }

    /// The same boundary with every column's direction flipped, for reading a page backwards.
    #[must_use]
    pub fn reversed(mut self) -> Self {
        for entry in &mut self.values {
            entry.descending = !entry.descending;
        }
        self
    }

    /// Converts the cursor into the single row-value [`Filter`] that resumes the scan.
    ///
    /// # Errors
    /// [`SqlCursorError::CursorDirectionMixed`] if the entries do not all sort the
    /// same way, and [`SqlCursorError::CursorEmpty`] if there are none.
    pub fn into_filter(self) -> Result<Filter, SqlCursorError> {
        if self.values.is_empty() {
            return Err(SqlCursorError::CursorEmpty);
        }
        let descending = self.values[0].descending;
        if self.values.iter().any(|e| e.descending != descending) {
            return Err(SqlCursorError::CursorDirectionMixed);
        }

        let mut columns = Vec::with_capacity(self.values.len());
        let mut values = Vec::with_capacity(self.values.len());
        for entry in self.values {
            columns.push(entry.column);
            values.push(FilterValue::from(entry.value));
        }
        Ok(if descending {
            Filter::row_value_lt(columns, values)
        } else {
            Filter::row_value_gt(columns, values)
        })
    }
}

/// What a cursor can be wrong about.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SqlCursorError {
    /// The cursor names no columns, so there is no boundary to resume from.
    #[error(
        code = "CUR_001",
        message = "the pagination cursor is not valid",
        developer_message = "a keyset cursor must name at least one ordering column",
        status = 400
    )]
    CursorEmpty,

    /// The cursor's columns do not all sort the same way.
    #[error(
        code = "CUR_002",
        message = "the pagination cursor is not valid",
        developer_message = "a keyset cursor must sort every column the same way; a tuple comparison has one direction",
        status = 400
    )]
    CursorDirectionMixed,
}

#[cfg(test)]
mod tests {
    use super::{CursorValue, SqlCursor, SqlCursorError};
    use fundamental_core::Error as _;
    use fundamental_db::FilterCondition;

    /// With `(a, b)` ascending past `(5, 9)`, `a > 5 AND b > 9` drops the row `(6, 2)`.
    #[test]
    fn a_multi_column_cursor_is_one_row_value_comparison() {
        let cursor =
            SqlCursor::new("a", CursorValue::Int(5), false).and("b", CursorValue::Int(9), false);
        let filter = cursor.into_filter().expect("a single-direction cursor");
        match filter.condition {
            FilterCondition::RowValueGt { columns, values } => {
                assert_eq!(columns, vec!["a".to_owned(), "b".to_owned()]);
                assert_eq!(values.len(), 2);
            }
            other => panic!("expected a row-value comparison, got {other:?}"),
        }
    }

    #[test]
    fn a_descending_cursor_resumes_downwards() {
        let cursor = SqlCursor::new("created_at", CursorValue::Int(100), true);
        let filter = cursor.into_filter().expect("a single-direction cursor");
        assert!(matches!(
            filter.condition,
            FilterCondition::RowValueLt { .. }
        ));
    }

    #[test]
    fn a_mixed_direction_cursor_is_refused() {
        let cursor =
            SqlCursor::new("a", CursorValue::Int(1), false).and("b", CursorValue::Int(2), true);
        let error = cursor.into_filter().expect_err("mixed directions");
        assert!(matches!(error, SqlCursorError::CursorDirectionMixed));
        assert_eq!(error.error_code(), "CUR_002");
        assert_eq!(error.http_status_code(), 400);
    }

    #[test]
    fn an_empty_cursor_is_refused() {
        let cursor = SqlCursor { values: Vec::new() };
        let error = cursor.into_filter().expect_err("no columns");
        assert!(matches!(error, SqlCursorError::CursorEmpty));
        assert_eq!(error.error_code(), "CUR_001");
    }
}
