use super::{CursorEntry, CursorValue, SqlCursor};
use crate::SqlError;
use fundamental_db::{OrderBy, OrderDirection};
use sqlx::postgres::PgRow;
use sqlx::{Column, Row, TypeInfo};

/// Reads the ordering tuple out of a page's last row as the cursor that resumes after it, each
/// column at its own type, since a text boundary compares differently from the `ORDER BY`.
///
/// # Errors
/// [`SqlError::query`] if the row does not carry an ordering column, carries one at a type a
/// cursor cannot round-trip, or carries a `NULL`, which has no position under a row-value comparison.
pub(crate) fn cursor_after(row: &PgRow, order_by: &[OrderBy]) -> Result<SqlCursor, SqlError> {
    let mut values = Vec::with_capacity(order_by.len());
    for order in order_by {
        let value = column_value(row, &order.column)?;
        values.push(CursorEntry {
            column: order.column.clone(),
            value,
            descending: matches!(order.direction, OrderDirection::Desc),
        });
    }
    Ok(SqlCursor { values })
}

fn column_value(row: &PgRow, column: &str) -> Result<CursorValue, SqlError> {
    let type_name = row
        .columns()
        .iter()
        .find(|c| c.name() == column)
        .ok_or_else(|| {
            SqlError::query(format!(
                "the page is ordered by '{column}', which the selected row does not carry; a \
                 cursored read must select every column it orders by"
            ))
        })?
        .type_info()
        .name()
        .to_owned();

    let missing = || {
        SqlError::query(format!(
            "the ordering column '{column}' is NULL, so it cannot be a page boundary"
        ))
    };
    let decode = |what: &'static str| {
        move |error: sqlx::Error| {
            SqlError::query(format!(
                "the ordering column '{column}' could not be read as {what}: {error}"
            ))
        }
    };

    match type_name.as_str() {
        "INT2" | "INT4" | "INT8" => integer(row, column, &type_name)
            .map_err(decode("an integer"))?
            .map(CursorValue::Int)
            .ok_or_else(missing),
        "TEXT" | "VARCHAR" | "BPCHAR" | "NAME" => row
            .try_get::<Option<String>, _>(column)
            .map_err(decode("text"))?
            .map(CursorValue::String)
            .ok_or_else(missing),
        "UUID" => row
            .try_get::<Option<uuid::Uuid>, _>(column)
            .map_err(decode("a UUID"))?
            .map(|value| CursorValue::Uuid(value.to_string()))
            .ok_or_else(missing),
        // Epoch microseconds: the column's own unit, and no text format for two ends to disagree on.
        "TIMESTAMPTZ" | "TIMESTAMP" => row
            .try_get::<Option<chrono::DateTime<chrono::Utc>>, _>(column)
            .map_err(decode("a timestamp"))?
            .map(|value| CursorValue::Timestamp(value.timestamp_micros()))
            .ok_or_else(missing),
        "BOOL" => row
            .try_get::<Option<bool>, _>(column)
            .map_err(decode("a boolean"))?
            .map(CursorValue::Bool)
            .ok_or_else(missing),
        // Refused rather than stringified, which would resume at the wrong place.
        other => Err(SqlError::query(format!(
            "the ordering column '{column}' has type {other}, which a keyset cursor cannot carry \
             without changing how it compares; order by a column of a type it can"
        ))),
    }
}

/// An integer column read at its own width and widened, as the driver reads only the stored width.
fn integer(row: &PgRow, column: &str, type_name: &str) -> Result<Option<i64>, sqlx::Error> {
    Ok(match type_name {
        "INT2" => row.try_get::<Option<i16>, _>(column)?.map(i64::from),
        "INT4" => row.try_get::<Option<i32>, _>(column)?.map(i64::from),
        _ => row.try_get::<Option<i64>, _>(column)?,
    })
}
