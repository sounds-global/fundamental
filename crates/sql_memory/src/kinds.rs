use chrono::{NaiveDate, NaiveTime};
use fundamental_db::{DatabaseError, DatabaseResult, FilterValue};

use crate::Held;

/// The type of a column, or of a value bound beside it, as far as comparing them goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// `INT2`, `INT4`, `INT8`.
    Integer,
    /// `FLOAT4`, `FLOAT8`, `NUMERIC`.
    Float,
    /// `TEXT`, `VARCHAR`.
    Text,
    /// `UUID`.
    Uuid,
    /// `BOOL`.
    Bool,
    /// `TIMESTAMPTZ`, `TIMESTAMP`.
    Moment,
    /// `DATE`.
    Day,
    /// `JSON`, `JSONB`.
    Json,
    /// `BYTEA`.
    Bytes,
    /// A type this database does not judge: any value is compared with it.
    Other,
}

impl Kind {
    fn of_column(data_type: &str) -> Self {
        let name = data_type
            .split(['(', ' ', '['])
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        if data_type.contains('[') {
            return Self::Other;
        }
        match name.as_str() {
            "INT2" | "INT4" | "INT8" | "SMALLINT" | "INTEGER" | "INT" | "BIGINT" => Self::Integer,
            "FLOAT4" | "FLOAT8" | "REAL" | "DOUBLE" | "NUMERIC" | "DECIMAL" => Self::Float,
            "TEXT" | "VARCHAR" | "CHAR" | "BPCHAR" | "CITEXT" | "NAME" => Self::Text,
            "UUID" => Self::Uuid,
            "BOOL" | "BOOLEAN" => Self::Bool,
            "TIMESTAMPTZ" | "TIMESTAMP" => Self::Moment,
            "DATE" => Self::Day,
            "JSON" | "JSONB" => Self::Json,
            "BYTEA" => Self::Bytes,
            _ => Self::Other,
        }
    }

    /// The type `value` is bound as, or `None` for a NULL.
    fn of_value(value: &FilterValue) -> Option<Self> {
        Some(match value {
            FilterValue::Null => return None,
            FilterValue::Bool(_) => Self::Bool,
            FilterValue::Int(_) => Self::Integer,
            FilterValue::Float(_) => Self::Float,
            FilterValue::String(_) => Self::Text,
            FilterValue::Bytes(_) => Self::Bytes,
            FilterValue::Uuid(text) if uuid::Uuid::try_parse(text).is_ok() => Self::Uuid,
            FilterValue::Uuid(_) => Self::Text,
            FilterValue::Timestamp(_) | FilterValue::TimestampTz(_) => Self::Moment,
            FilterValue::Json(_) => Self::Json,
        })
    }

    const fn sql(self) -> &'static str {
        match self {
            Self::Integer => "bigint",
            Self::Float => "double precision",
            Self::Text => "text",
            Self::Uuid => "uuid",
            Self::Bool => "boolean",
            Self::Moment => "timestamp with time zone",
            Self::Day => "date",
            Self::Json => "jsonb",
            Self::Bytes => "bytea",
            Self::Other => "unknown",
        }
    }

    /// Whether the database compares this kind with `value`'s: the same type, integer with float, or day with moment.
    fn takes(self, value: Self) -> bool {
        self == value
            || matches!(
                (self, value),
                (Self::Other, _)
                    | (Self::Integer | Self::Float, Self::Integer | Self::Float)
                    | (Self::Day, Self::Moment)
            )
    }
}

pub(crate) fn kinds<M: Held>() -> Vec<Kind> {
    let schema = M::table_schema();
    let kind = |name: &str| {
        schema
            .columns
            .get(name)
            .map_or(Kind::Other, |column| Kind::of_column(&column.data_type))
    };
    let mut kinds: Vec<Kind> = M::columns().iter().map(|name| kind(name)).collect();
    if M::SOFT_DELETE {
        kinds.push(kind(M::DELETED_AT_COLUMN));
    }
    kinds
}

/// Refuses `value` beside `column` under `operator` when the database would.
///
/// # Errors
///
/// [`DatabaseError::Query`] saying which operator does not exist.
pub(crate) fn refuse_unrelated(
    column: &str,
    kind: Kind,
    operator: &str,
    value: &FilterValue,
) -> DatabaseResult<()> {
    let Some(bound) = Kind::of_value(value) else {
        return Err(DatabaseError::query(format!(
            "`{column} {operator}` binds no value for NULL: a NULL is asked for with IS NULL"
        )));
    };
    if kind.takes(bound) {
        Ok(())
    } else {
        Err(DatabaseError::query(format!(
            "operator does not exist: {} {operator} {}",
            kind.sql(),
            bound.sql()
        )))
    }
}

/// Refuses a pattern matched against `column` of the kind `kind`, unless it holds text.
///
/// # Errors
///
/// [`DatabaseError::Query`] saying which operator does not exist.
pub(crate) fn refuse_unpatterned(column: &str, kind: Kind) -> DatabaseResult<()> {
    if matches!(kind, Kind::Text | Kind::Other) {
        Ok(())
    } else {
        Err(DatabaseError::query(format!(
            "operator does not exist: {} ~~ text (`{column}`)",
            kind.sql()
        )))
    }
}

/// `cell` as compared with `value`: a day beside a moment as its first instant in UTC.
pub(crate) fn as_compared(kind: Kind, cell: &FilterValue, value: &FilterValue) -> FilterValue {
    match (kind, cell, value) {
        (
            Kind::Day,
            FilterValue::String(day),
            FilterValue::TimestampTz(_) | FilterValue::Timestamp(_),
        ) => NaiveDate::parse_from_str(day, "%Y-%m-%d").map_or_else(
            |_| cell.clone(),
            |day| FilterValue::TimestampTz(day.and_time(NaiveTime::MIN).and_utc()),
        ),
        _ => cell.clone(),
    }
}
