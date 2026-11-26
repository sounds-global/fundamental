//! The platform-agnostic SQLite value type.

/// A SQLite value, mirroring SQLite's five storage classes.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// SQL NULL.
    Null,
    /// 64-bit signed integer (also used for booleans as 0/1).
    Integer(i64),
    /// 64-bit IEEE floating point.
    Real(f64),
    /// UTF-8 text (also used for UUIDs and ISO8601 timestamps).
    Text(String),
    /// Binary data.
    Blob(Vec<u8>),
}

impl Value {
    /// Returns true if this value is NULL.
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Returns the integer value, if this is an Integer.
    pub fn as_integer(&self) -> Option<i64> {
        match self {
            Self::Integer(i) => Some(*i),
            _ => None,
        }
    }

    /// Returns the real value, if this is a Real.
    pub fn as_real(&self) -> Option<f64> {
        match self {
            Self::Real(r) => Some(*r),
            _ => None,
        }
    }

    /// Returns the text value, if this is Text.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(t) => Some(t),
            _ => None,
        }
    }

    /// Returns the blob value, if this is a Blob.
    pub fn as_blob(&self) -> Option<&[u8]> {
        match self {
            Self::Blob(b) => Some(b),
            _ => None,
        }
    }

    /// Returns the boolean value.
    ///
    /// Accepts `Integer` 0/1 and the `Text` forms a synced payload carries
    /// (`true`/`false`, `t`/`f`, `1`/`0`, `yes`/`no`).
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Integer(i) => Some(*i != 0),
            Self::Text(t) => match t.trim().to_ascii_lowercase().as_str() {
                "1" | "true" | "t" | "yes" | "y" => Some(true),
                "0" | "false" | "f" | "no" | "n" => Some(false),
                _ => None,
            },
            _ => None,
        }
    }

    /// Returns the SQLite storage class name of this value.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Null => "NULL",
            Self::Integer(_) => "INTEGER",
            Self::Real(_) => "REAL",
            Self::Text(_) => "TEXT",
            Self::Blob(_) => "BLOB",
        }
    }
}

impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Self::Integer(v.into())
    }
}

impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Self::Integer(v)
    }
}

impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Self::Real(v)
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Self::Integer(v.into())
    }
}

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Self::Text(v.to_string())
    }
}

impl From<String> for Value {
    fn from(v: String) -> Self {
        Self::Text(v)
    }
}

impl From<Vec<u8>> for Value {
    fn from(v: Vec<u8>) -> Self {
        Self::Blob(v)
    }
}

impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(v: Option<T>) -> Self {
        match v {
            Some(v) => v.into(),
            None => Self::Null,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Value;

    #[test]
    fn as_bool_reads_integer_and_text_forms() {
        assert_eq!(Value::from(true).as_bool(), Some(true));
        assert_eq!(Value::from(false).as_bool(), Some(false));
        assert_eq!(Value::Integer(1).as_bool(), Some(true));

        // A synced payload carries text: Postgres `t`/`f`, `true`/`false`, or `1`/`0`.
        for truthy in ["true", "TRUE", "t", "1", "yes"] {
            assert_eq!(
                Value::Text(truthy.to_string()).as_bool(),
                Some(true),
                "{truthy}"
            );
        }
        for falsy in ["false", "False", "f", "0", "no"] {
            assert_eq!(
                Value::Text(falsy.to_string()).as_bool(),
                Some(false),
                "{falsy}"
            );
        }

        assert_eq!(Value::Text("maybe".to_string()).as_bool(), None);
        assert_eq!(Value::Null.as_bool(), None);
    }
}
