//! The [`CqlValue`] enum and its `scylla` driver `SerializeValue` implementation.

use fundamental_db::FilterValue;

use crate::CqlError;

/// A CQL value for query parameters.
#[derive(Debug, Clone)]
pub enum CqlValue {
    /// Null value.
    Null,
    /// Boolean value.
    Bool(bool),
    /// 64-bit integer.
    BigInt(i64),
    /// 32-bit integer.
    Int(i32),
    /// 64-bit float.
    Double(f64),
    /// Text string.
    Text(String),
    /// Binary blob.
    Blob(Vec<u8>),
    /// UUID.
    Uuid(uuid::Uuid),
    /// Timestamp (milliseconds since epoch).
    Timestamp(i64),
}

impl CqlValue {
    /// Creates a BigInt value.
    pub fn bigint(v: i64) -> Self {
        Self::BigInt(v)
    }

    /// Creates a Text value.
    pub fn text(v: impl Into<String>) -> Self {
        Self::Text(v.into())
    }

    /// Creates a UUID value.
    pub fn uuid(v: uuid::Uuid) -> Self {
        Self::Uuid(v)
    }

    /// Creates a Timestamp value.
    pub fn timestamp(millis: i64) -> Self {
        Self::Timestamp(millis)
    }
}

/// A Rust value that can address one column of a partition key.
///
/// Not a blanket impl over `Into<CqlValue>`: each listed type is a column type somebody chose.
pub trait ToCqlValue {
    /// This value as the CQL parameter that addresses its column.
    fn to_cql_value(&self) -> CqlValue;
}

impl ToCqlValue for String {
    fn to_cql_value(&self) -> CqlValue {
        CqlValue::Text(self.clone())
    }
}

impl ToCqlValue for &str {
    fn to_cql_value(&self) -> CqlValue {
        CqlValue::Text((*self).to_owned())
    }
}

impl ToCqlValue for uuid::Uuid {
    fn to_cql_value(&self) -> CqlValue {
        CqlValue::Uuid(*self)
    }
}

impl ToCqlValue for i64 {
    fn to_cql_value(&self) -> CqlValue {
        CqlValue::BigInt(*self)
    }
}

impl ToCqlValue for i32 {
    fn to_cql_value(&self) -> CqlValue {
        CqlValue::Int(*self)
    }
}

impl ToCqlValue for bool {
    fn to_cql_value(&self) -> CqlValue {
        CqlValue::Bool(*self)
    }
}

impl ToCqlValue for Vec<u8> {
    fn to_cql_value(&self) -> CqlValue {
        CqlValue::Blob(self.clone())
    }
}

/// A value already in CQL form addresses its column as itself.
/// An escape hatch for a partition column type this trait does not list.
impl ToCqlValue for CqlValue {
    fn to_cql_value(&self) -> CqlValue {
        self.clone()
    }
}

/// Converts a backend-agnostic [`FilterValue`] into a bindable [`CqlValue`].
///
/// # Errors
///
/// [`CqlError::InvalidQuery`] for an unparsable `Uuid`, a `Null` or a `Json`, none of which
/// binds into a key comparison.
impl TryFrom<FilterValue> for CqlValue {
    type Error = CqlError;

    fn try_from(value: FilterValue) -> Result<Self, Self::Error> {
        match value {
            FilterValue::Bool(v) => Ok(Self::Bool(v)),
            FilterValue::Int(v) => Ok(Self::BigInt(v)),
            FilterValue::Float(v) => Ok(Self::Double(v)),
            FilterValue::String(v) => Ok(Self::Text(v)),
            FilterValue::Bytes(v) => Ok(Self::Blob(v)),
            FilterValue::Timestamp(v) => Ok(Self::Timestamp(v)),
            FilterValue::TimestampTz(dt) => Ok(Self::Timestamp(dt.timestamp_millis())),
            FilterValue::Uuid(s) => s.parse::<uuid::Uuid>().map(Self::Uuid).map_err(|e| {
                CqlError::invalid_query(format!("filter value is not a valid UUID '{s}': {e}"))
            }),
            FilterValue::Null => Err(CqlError::invalid_query(
                "null filter value cannot bind into a CQL key predicate",
            )),
            FilterValue::Json(_) => Err(CqlError::invalid_query(
                "JSON filter value has no CQL key-predicate equivalent",
            )),
        }
    }
}

impl scylla::serialize::value::SerializeValue for CqlValue {
    fn serialize<'b>(
        &self,
        typ: &scylla::frame::response::result::ColumnType<'_>,
        writer: scylla::serialize::writers::CellWriter<'b>,
    ) -> Result<
        scylla::serialize::writers::WrittenCellProof<'b>,
        scylla::serialize::SerializationError,
    > {
        use scylla::serialize::value::SerializeValue as SV;

        match self {
            CqlValue::Null => Ok(writer.set_null()),
            CqlValue::Bool(v) => SV::serialize(v, typ, writer),
            CqlValue::BigInt(v) => SV::serialize(v, typ, writer),
            CqlValue::Int(v) => SV::serialize(v, typ, writer),
            CqlValue::Double(v) => SV::serialize(v, typ, writer),
            CqlValue::Text(v) => SV::serialize(v, typ, writer),
            CqlValue::Blob(v) => SV::serialize(v, typ, writer),
            CqlValue::Uuid(v) => SV::serialize(v, typ, writer),
            CqlValue::Timestamp(millis) => {
                // YCQL timestamps are milliseconds since the epoch, serialized as a bigint.
                SV::serialize(millis, typ, writer)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    fn converts_scalar_filter_values() {
        assert!(matches!(
            CqlValue::try_from(FilterValue::Bool(true)),
            Ok(CqlValue::Bool(true))
        ));
        // The abstract layer carries every integer as i64; it maps to CQL bigint.
        assert!(matches!(
            CqlValue::try_from(FilterValue::Int(42)),
            Ok(CqlValue::BigInt(42))
        ));
        assert!(matches!(
            CqlValue::try_from(FilterValue::Float(2.5)),
            Ok(CqlValue::Double(v)) if v == 2.5
        ));
        assert!(matches!(
            CqlValue::try_from(FilterValue::String("x".into())),
            Ok(CqlValue::Text(s)) if s == "x"
        ));
        assert!(matches!(
            CqlValue::try_from(FilterValue::Bytes(vec![1, 2])),
            Ok(CqlValue::Blob(b)) if b == vec![1, 2]
        ));
        assert!(matches!(
            CqlValue::try_from(FilterValue::Timestamp(1000)),
            Ok(CqlValue::Timestamp(1000))
        ));
    }

    #[test]
    fn parses_uuid_string() {
        let id = uuid::Uuid::nil();
        assert!(matches!(
            CqlValue::try_from(FilterValue::Uuid(id.to_string())),
            Ok(CqlValue::Uuid(u)) if u == id
        ));
    }

    #[test]
    fn reduces_timestamptz_to_epoch_millis() {
        let dt = chrono::Utc
            .timestamp_millis_opt(1_700_000_000_123)
            .single()
            .unwrap();
        assert!(matches!(
            CqlValue::try_from(FilterValue::TimestampTz(dt)),
            Ok(CqlValue::Timestamp(1_700_000_000_123))
        ));
    }

    #[test]
    fn rejects_unparseable_uuid() {
        let err = CqlValue::try_from(FilterValue::Uuid("not-a-uuid".into()))
            .expect_err("an invalid UUID string must be rejected");
        assert!(matches!(err, CqlError::InvalidQuery(_)));
    }

    #[test]
    fn rejects_null_and_json() {
        assert!(matches!(
            CqlValue::try_from(FilterValue::Null),
            Err(CqlError::InvalidQuery(_))
        ));
        assert!(matches!(
            CqlValue::try_from(FilterValue::Json(serde_json::Value::Null)),
            Err(CqlError::InvalidQuery(_))
        ));
    }
}
