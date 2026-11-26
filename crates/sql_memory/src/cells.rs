use chrono::{DateTime, Duration, NaiveDate, TimeZone as _, Utc};
use fundamental_db::{DatabaseError, DatabaseResult, FilterValue};
use fundamental_sql::sqlx::encode::IsNull;
use fundamental_sql::sqlx::postgres::PgArgumentBuffer;
use fundamental_sql::sqlx::{Encode, Postgres, Type, TypeInfo as _};
use fundamental_sql::{BindValues, ToRow};

use crate::Held;

/// The values of `model`'s columns, in [`ToRow::columns`] order.
///
/// # Errors
///
/// [`DatabaseError::Configuration`] for a model with a `Secret` column, or a value that cannot encode itself.
pub(crate) fn cells_of<M: ToRow>(model: &M) -> DatabaseResult<Vec<FilterValue>> {
    let capture = model
        .bind_to_encrypted(Capture::default(), None)
        .map_err(|error| DatabaseError::configuration(error.to_string()))?;
    match capture.refused {
        Some(refused) => Err(DatabaseError::configuration(refused)),
        None => Ok(capture.cells),
    }
}

/// Where the column `name` stands among a row of `M`'s cells; a soft-deleted model's deletion column comes last.
///
/// # Errors
///
/// [`DatabaseError::Query`] for a column `M`'s table does not have.
pub(crate) fn column<M: Held>(name: &str) -> DatabaseResult<usize> {
    M::columns()
        .iter()
        .position(|column| *column == name)
        .or_else(|| (M::SOFT_DELETE && name == M::DELETED_AT_COLUMN).then(|| M::columns().len()))
        .ok_or_else(|| DatabaseError::query(format!("column \"{name}\" does not exist")))
}

/// `value` as the cell a column of its type holds.
///
/// # Errors
///
/// [`DatabaseError::Configuration`] when the value cannot encode itself.
pub(crate) fn cell_of<'q, T>(value: &T) -> DatabaseResult<FilterValue>
where
    T: Encode<'q, Postgres> + Type<Postgres>,
{
    let mut buffer = PgArgumentBuffer::default();
    let null = value
        .encode_by_ref(&mut buffer)
        .map_err(|error| DatabaseError::configuration(error.to_string()))?;
    Ok(match null {
        IsNull::Yes => FilterValue::Null,
        IsNull::No => decoded(T::type_info().name(), &buffer),
    })
}

#[derive(Default)]
struct Capture {
    cells: Vec<FilterValue>,
    refused: Option<String>,
}

impl<'q> BindValues<'q> for Capture {
    fn bind_value<T: Encode<'q, Postgres> + Type<Postgres> + Send + 'q>(
        mut self,
        value: T,
    ) -> Self {
        match cell_of(&value) {
            Ok(cell) => self.cells.push(cell),
            Err(error) => {
                self.cells.push(FilterValue::Null);
                self.refused.get_or_insert(error.to_string());
            }
        }
        self
    }
}

/// The value Postgres's binary `bytes` hold for type `name`; an unread type is kept as its bytes.
fn decoded(name: &str, bytes: &[u8]) -> FilterValue {
    let fixed = |n: usize| bytes.get(..n).filter(|_| bytes.len() == n);
    match name {
        "BOOL" => FilterValue::Bool(bytes.first().is_some_and(|byte| *byte != 0)),
        "INT2" => fixed(2).map_or_else(
            || FilterValue::Bytes(bytes.to_vec()),
            |b| FilterValue::Int(i64::from(i16::from_be_bytes([b[0], b[1]]))),
        ),
        "INT4" => fixed(4).map_or_else(
            || FilterValue::Bytes(bytes.to_vec()),
            |b| FilterValue::Int(i64::from(i32::from_be_bytes([b[0], b[1], b[2], b[3]]))),
        ),
        "INT8" => be8(bytes).map_or_else(
            || FilterValue::Bytes(bytes.to_vec()),
            |b| FilterValue::Int(i64::from_be_bytes(b)),
        ),
        "FLOAT4" => fixed(4).map_or_else(
            || FilterValue::Bytes(bytes.to_vec()),
            |b| FilterValue::Float(f64::from(f32::from_be_bytes([b[0], b[1], b[2], b[3]]))),
        ),
        "FLOAT8" => be8(bytes).map_or_else(
            || FilterValue::Bytes(bytes.to_vec()),
            |b| FilterValue::Float(f64::from_be_bytes(b)),
        ),
        "TEXT" | "VARCHAR" | "BPCHAR" | "NAME" | "CITEXT" => {
            FilterValue::String(String::from_utf8_lossy(bytes).into_owned())
        }
        "UUID" => uuid::Uuid::from_slice(bytes).map_or_else(
            |_| FilterValue::Bytes(bytes.to_vec()),
            |id| FilterValue::Uuid(id.to_string()),
        ),
        "TIMESTAMPTZ" | "TIMESTAMP" => be8(bytes)
            .and_then(|b| epoch().checked_add_signed(Duration::microseconds(i64::from_be_bytes(b))))
            .map_or_else(
                || FilterValue::Bytes(bytes.to_vec()),
                FilterValue::TimestampTz,
            ),
        "DATE" => fixed(4)
            .and_then(|b| {
                let days = i32::from_be_bytes([b[0], b[1], b[2], b[3]]);
                NaiveDate::from_ymd_opt(2000, 1, 1)?
                    .checked_add_signed(Duration::days(i64::from(days)))
            })
            .map_or_else(
                || FilterValue::Bytes(bytes.to_vec()),
                |day| FilterValue::String(day.format("%Y-%m-%d").to_string()),
            ),
        "JSONB" => json(bytes.get(1..).unwrap_or_default(), bytes),
        "JSON" => json(bytes, bytes),
        _ => FilterValue::Bytes(bytes.to_vec()),
    }
}

fn be8(bytes: &[u8]) -> Option<[u8; 8]> {
    bytes.try_into().ok()
}

fn json(text: &[u8], bytes: &[u8]) -> FilterValue {
    serde_json::from_slice(text)
        .map_or_else(|_| FilterValue::Bytes(bytes.to_vec()), FilterValue::Json)
}

fn epoch() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2000, 1, 1, 0, 0, 0)
        .single()
        .unwrap_or_default()
}
