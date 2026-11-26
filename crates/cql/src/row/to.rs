//! Serializing Rust models into CQL row values for writes.

use fundamental_core::model::cql::ClusteringOrder;
use fundamental_encryption::KeyManager;

use crate::CqlResult;

/// Trait for types that can be serialized to CQL values.
///
/// # Example
///
/// ```rust
/// use fundamental_cql::ToRow;
/// use uuid::Uuid;
///
/// struct Event {
///     user_id: Uuid,
///     timestamp: i64,
///     event_type: String,
///     data: Vec<u8>,
/// }
///
/// impl ToRow for Event {
///     fn columns() -> &'static [&'static str] {
///         &["user_id", "timestamp", "event_type", "data"]
///     }
///
///     fn partition_key_columns() -> &'static [&'static str] {
///         &["user_id"]
///     }
///
///     fn clustering_key_columns() -> &'static [&'static str] {
///         &["timestamp"]
///     }
///
///     type InsertValues = (Uuid, i64, String, Vec<u8>);
///     type UpdateValues = (String, Vec<u8>, Uuid, i64);  // SET cols, then WHERE cols
///     type KeyValues = (Uuid, i64);  // pk + ck
///
///     fn to_values(&self) -> Self::InsertValues {
///         (self.user_id, self.timestamp, self.event_type.clone(), self.data.clone())
///     }
///
///     fn to_update_values(&self) -> Self::UpdateValues {
///         (self.event_type.clone(), self.data.clone(), self.user_id, self.timestamp)
///     }
///
///     fn to_key_values(&self) -> Self::KeyValues {
///         (self.user_id, self.timestamp)
///     }
/// }
/// ```
pub trait ToRow {
    /// Returns all column names.
    fn columns() -> &'static [&'static str];

    /// Returns the partition key column names.
    fn partition_key_columns() -> &'static [&'static str];

    /// Returns the clustering key column names.
    fn clustering_key_columns() -> &'static [&'static str];

    /// The order each clustering column is stored in, beside
    /// [`clustering_key_columns`](ToRow::clustering_key_columns).
    ///
    /// Written by `#[derive(Model)]` from `#[model(clustering_key = "desc")]`; empty when not
    /// declared.
    fn clustering_key_orders() -> &'static [ClusteringOrder] {
        &[]
    }

    /// Type for INSERT values (all columns in order).
    type InsertValues: scylla::serialize::row::SerializeRow + Send + Sync;

    /// Type for UPDATE values (SET columns, then WHERE columns).
    type UpdateValues: scylla::serialize::row::SerializeRow + Send + Sync;

    /// Type for key values (partition key + clustering key).
    type KeyValues: scylla::serialize::row::SerializeRow + Send + Sync;

    /// Serializes all field values for INSERT, in `columns()` order.
    fn to_values(&self) -> Self::InsertValues;

    /// Serializes values for UPDATE: SET columns, then WHERE columns.
    fn to_update_values(&self) -> Self::UpdateValues;

    /// Serializes the key values for DELETE/SELECT by key: partition, then clustering columns.
    fn to_key_values(&self) -> Self::KeyValues;

    /// Like [`to_values`](ToRow::to_values), but encrypts any
    /// [`secret_columns`](fundamental_core::Model::secret_columns) using `enc`
    /// before placing their ciphertext envelope in the value tuple.
    ///
    /// Keys are never encrypted, so [`to_key_values`](ToRow::to_key_values) has no encrypting
    /// variant.
    ///
    /// # Errors
    ///
    /// Returns an error if a `Secret` column needs encryption but `enc` is `None`,
    /// or if encryption fails.
    fn to_values_encrypted(&self, enc: Option<&KeyManager>) -> CqlResult<Self::InsertValues> {
        let _ = enc;
        Ok(self.to_values())
    }

    /// Like [`to_update_values`](ToRow::to_update_values), but encrypts any
    /// `Secret` columns with `enc`. See [`to_values_encrypted`](ToRow::to_values_encrypted).
    ///
    /// # Errors
    ///
    /// Returns an error if a `Secret` column needs encryption but `enc` is `None`,
    /// or if encryption fails.
    fn to_update_values_encrypted(
        &self,
        enc: Option<&KeyManager>,
    ) -> CqlResult<Self::UpdateValues> {
        let _ = enc;
        Ok(self.to_update_values())
    }

    /// Returns the non-key column names (columns that are not part of the primary key).
    fn value_columns() -> Vec<&'static str> {
        let pk_cols = Self::partition_key_columns();
        let ck_cols = Self::clustering_key_columns();
        Self::columns()
            .iter()
            .filter(|c| !pk_cols.contains(c) && !ck_cols.contains(c))
            .copied()
            .collect()
    }

    /// Returns the number of columns.
    fn column_count() -> usize {
        Self::columns().len()
    }
}
