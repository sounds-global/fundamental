//! Filter value types.

/// A value used in filters.
#[derive(Debug, Clone)]
pub enum FilterValue {
    /// Null value.
    Null,
    /// Boolean value.
    Bool(bool),
    /// Integer value.
    Int(i64),
    /// Floating point value.
    Float(f64),
    /// String value.
    String(String),
    /// Binary data.
    Bytes(Vec<u8>),
    /// UUID value (as string).
    Uuid(String),
    /// Timestamp (milliseconds since epoch).
    Timestamp(i64),
    /// Timezone-aware timestamp (chrono `DateTime<Utc>`).
    /// Binds directly to TIMESTAMPTZ columns via sqlx.
    TimestampTz(chrono::DateTime<chrono::Utc>),
    /// JSON value (for JSONB columns).
    Json(serde_json::Value),
}

impl From<bool> for FilterValue {
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}

impl From<i32> for FilterValue {
    fn from(v: i32) -> Self {
        Self::Int(v as i64)
    }
}

impl From<i64> for FilterValue {
    fn from(v: i64) -> Self {
        Self::Int(v)
    }
}

impl From<f64> for FilterValue {
    fn from(v: f64) -> Self {
        Self::Float(v)
    }
}

impl From<String> for FilterValue {
    fn from(v: String) -> Self {
        Self::String(v)
    }
}

impl From<&str> for FilterValue {
    fn from(v: &str) -> Self {
        Self::String(v.to_string())
    }
}

impl From<Vec<u8>> for FilterValue {
    fn from(v: Vec<u8>) -> Self {
        Self::Bytes(v)
    }
}

impl From<serde_json::Value> for FilterValue {
    fn from(v: serde_json::Value) -> Self {
        Self::Json(v)
    }
}

impl From<Option<String>> for FilterValue {
    fn from(v: Option<String>) -> Self {
        match v {
            Some(s) => Self::String(s),
            None => Self::Null,
        }
    }
}

impl From<Option<i64>> for FilterValue {
    fn from(v: Option<i64>) -> Self {
        match v {
            Some(i) => Self::Int(i),
            None => Self::Null,
        }
    }
}

impl From<Option<bool>> for FilterValue {
    fn from(v: Option<bool>) -> Self {
        match v {
            Some(b) => Self::Bool(b),
            None => Self::Null,
        }
    }
}

impl From<uuid::Uuid> for FilterValue {
    fn from(v: uuid::Uuid) -> Self {
        Self::Uuid(v.to_string())
    }
}

impl From<Option<uuid::Uuid>> for FilterValue {
    fn from(v: Option<uuid::Uuid>) -> Self {
        match v {
            Some(u) => Self::Uuid(u.to_string()),
            None => Self::Null,
        }
    }
}

impl From<chrono::DateTime<chrono::Utc>> for FilterValue {
    fn from(v: chrono::DateTime<chrono::Utc>) -> Self {
        Self::TimestampTz(v)
    }
}

impl From<Option<chrono::DateTime<chrono::Utc>>> for FilterValue {
    fn from(v: Option<chrono::DateTime<chrono::Utc>>) -> Self {
        match v {
            Some(dt) => Self::TimestampTz(dt),
            None => Self::Null,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FilterValue;
    use uuid::Uuid;

    #[test]
    fn uuid_maps_to_the_uuid_variant_not_string() {
        let id = Uuid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef);
        match FilterValue::from(id) {
            FilterValue::Uuid(s) => assert_eq!(s, id.to_string()),
            other => panic!("expected FilterValue::Uuid, got {other:?}"),
        }
    }

    #[test]
    fn some_uuid_maps_to_uuid_and_none_maps_to_null() {
        let id = Uuid::from_u128(1);
        match FilterValue::from(Some(id)) {
            FilterValue::Uuid(s) => assert_eq!(s, id.to_string()),
            other => panic!("expected FilterValue::Uuid, got {other:?}"),
        }
        match FilterValue::from(Option::<Uuid>::None) {
            FilterValue::Null => {}
            other => panic!("expected FilterValue::Null, got {other:?}"),
        }
    }
}
