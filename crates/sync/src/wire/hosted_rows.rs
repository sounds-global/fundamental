//! [`HostedRows`] — the row transport between a hosted database and its views.

use fundamental_offline::db::{OfflineDbError, Row, Value};
use fundamental_offline::store::OfflineRecord;
use serde::{Deserialize, Serialize};

/// A table read shipped across a process boundary: the column names once, then each row's values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostedRows {
    /// The column names, in the model's declared order.
    pub columns: Vec<String>,
    /// One entry per row, each aligned with `columns`.
    pub rows: Vec<Vec<HostedValue>>,
}

impl HostedRows {
    /// Packages `records` for the wire, using `M`'s declared columns and values.
    #[must_use]
    pub fn from_records<M: OfflineRecord>(records: &[M]) -> Self {
        Self {
            columns: M::column_names().iter().map(|c| (*c).to_owned()).collect(),
            rows: records
                .iter()
                .map(|record| record.to_values().into_iter().map(Into::into).collect())
                .collect(),
        }
    }

    /// Reconstructs the typed models on the receiving side.
    ///
    /// # Errors
    /// Returns the model's own `from_row` error when a row does not fit `M`.
    pub fn typed<M: OfflineRecord>(self) -> Result<Vec<M>, OfflineDbError> {
        let columns = self.columns;
        self.rows
            .into_iter()
            .map(|values| {
                let row = Row::from_parts(
                    columns.clone(),
                    values.into_iter().map(Into::into).collect(),
                );
                M::from_row(&row)
            })
            .collect()
    }
}

/// A [`Value`] in wire form, untagged; the variant order keeps deserialization unambiguous.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HostedValue {
    /// SQL NULL.
    Null,
    /// 64-bit signed integer.
    Integer(i64),
    /// 64-bit IEEE floating point.
    Real(f64),
    /// UTF-8 text.
    Text(String),
    /// Binary data.
    Blob(Vec<u8>),
}

impl From<Value> for HostedValue {
    fn from(value: Value) -> Self {
        match value {
            Value::Null => Self::Null,
            Value::Integer(i) => Self::Integer(i),
            Value::Real(r) => Self::Real(r),
            Value::Text(t) => Self::Text(t),
            Value::Blob(b) => Self::Blob(b),
        }
    }
}

impl From<HostedValue> for Value {
    fn from(value: HostedValue) -> Self {
        match value {
            HostedValue::Null => Self::Null,
            HostedValue::Integer(i) => Self::Integer(i),
            HostedValue::Real(r) => Self::Real(r),
            HostedValue::Text(t) => Self::Text(t),
            HostedValue::Blob(b) => Self::Blob(b),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{HostedRows, HostedValue};
    use fundamental_offline::db::Value;

    fn round_trip(value: Value) -> Value {
        let wire: HostedValue = value.into();
        let json = serde_json::to_string(&wire).expect("serialize");
        let back: HostedValue = serde_json::from_str(&json).expect("deserialize");
        back.into()
    }

    #[test]
    fn every_storage_class_round_trips_through_json() {
        for value in [
            Value::Null,
            Value::Integer(-42),
            Value::Real(2.5),
            Value::Text("héllo".to_owned()),
            Value::Blob(vec![0, 1, 255]),
        ] {
            assert_eq!(round_trip(value.clone()), value);
        }
    }

    #[test]
    fn json_form_is_natural_untagged() {
        assert_eq!(
            serde_json::to_string(&HostedValue::Integer(5)).expect("serialize"),
            "5"
        );
        assert_eq!(
            serde_json::to_string(&HostedValue::Real(5.5)).expect("serialize"),
            "5.5"
        );
        assert_eq!(
            serde_json::to_string(&HostedValue::Null).expect("serialize"),
            "null"
        );
    }

    #[test]
    fn rows_payload_round_trips_structurally() {
        let payload = HostedRows {
            columns: vec!["id".to_owned(), "n".to_owned()],
            rows: vec![vec![
                HostedValue::Text("a".to_owned()),
                HostedValue::Integer(1),
            ]],
        };
        let json = serde_json::to_string(&payload).expect("serialize");
        let back: HostedRows = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, payload);
    }

    // `from_records` / `typed` are tested in `fundamental_web`: the derive does not resolve here.
}
