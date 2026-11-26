//! Binding of [`FilterValue`] variants onto sqlx Postgres queries.

use fundamental_db::FilterValue;
use uuid::Uuid;

/// Binds a [`FilterValue`] to an sqlx query; `Null` binds nothing, an unparseable `Uuid` binds as text, and `Timestamp` is milliseconds.
pub(crate) fn bind_filter_value(
    query: sqlx::query::Query<'_, sqlx::Postgres, sqlx::postgres::PgArguments>,
    value: FilterValue,
) -> sqlx::query::Query<'_, sqlx::Postgres, sqlx::postgres::PgArguments> {
    match value {
        FilterValue::Null => query,
        FilterValue::Bool(v) => query.bind(v),
        FilterValue::Int(v) => query.bind(v),
        FilterValue::Float(v) => query.bind(v),
        FilterValue::String(v) => query.bind(v),
        FilterValue::Bytes(v) => query.bind(v),
        FilterValue::Uuid(v) => match Uuid::parse_str(&v) {
            Ok(uuid) => query.bind(uuid),
            Err(_) => {
                tracing::warn!(
                    uuid_string = %v,
                    "Invalid UUID format in filter value, binding as string"
                );
                query.bind(v)
            }
        },
        FilterValue::Timestamp(v) => {
            let dt = chrono::DateTime::from_timestamp_millis(v).unwrap_or_default();
            query.bind(dt)
        }
        FilterValue::TimestampTz(v) => query.bind(v),
        FilterValue::Json(v) => query.bind(v),
    }
}
