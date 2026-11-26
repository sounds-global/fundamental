//! [`CaveatValue`]: a typed scalar used in caveat literals and request context.

use serde::{Deserialize, Serialize};

/// A typed value in a [`CaveatIr`](super::CaveatIr); `Timestamp` is Unix seconds, so
/// comparisons are integer comparisons.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CaveatValue {
    /// A boolean.
    Bool(bool),
    /// A signed 64-bit integer.
    Int(i64),
    /// A 64-bit float.
    Double(f64),
    /// A UTF-8 string.
    String(String),
    /// A point in time, as seconds since the Unix epoch.
    Timestamp(i64),
    /// An ordered list of values (for membership tests).
    List(Vec<CaveatValue>),
}

impl CaveatValue {
    /// The static name of this value's type, for diagnostics.
    #[must_use]
    pub fn type_name(&self) -> &'static str {
        match self {
            CaveatValue::Bool(_) => "bool",
            CaveatValue::Int(_) => "int",
            CaveatValue::Double(_) => "double",
            CaveatValue::String(_) => "string",
            CaveatValue::Timestamp(_) => "timestamp",
            CaveatValue::List(_) => "list",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_type_names() {
        assert_eq!(CaveatValue::Bool(true).type_name(), "bool");
        assert_eq!(CaveatValue::Int(1).type_name(), "int");
        assert_eq!(CaveatValue::List(vec![]).type_name(), "list");
    }

    #[test]
    fn round_trips_json() {
        let v = CaveatValue::List(vec![CaveatValue::Int(1), CaveatValue::String("x".into())]);
        let json = serde_json::to_string(&v).expect("serialize");
        let back: CaveatValue = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(v, back);
    }
}
