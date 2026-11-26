//! Which columns a change-capture source keeps from a watched table.

use crate::SingleKeySyncable;

/// A syncable base table to capture changes from, and which captured columns a change keeps.
#[derive(Debug, Clone)]
pub struct CaptureTable {
    table: String,
    key_columns: Vec<String>,
    partition_columns: Vec<String>,
    data_columns: Vec<String>,
    owner_column: String,
}

impl CaptureTable {
    /// Names a table explicitly: its key columns in key order, synced data columns and owner column.
    pub fn new(
        table: impl Into<String>,
        key_columns: impl IntoIterator<Item = impl Into<String>>,
        partition_columns: impl IntoIterator<Item = impl Into<String>>,
        data_columns: impl IntoIterator<Item = impl Into<String>>,
        owner_column: impl Into<String>,
    ) -> Self {
        Self {
            table: table.into(),
            key_columns: key_columns.into_iter().map(Into::into).collect(),
            partition_columns: partition_columns.into_iter().map(Into::into).collect(),
            data_columns: data_columns.into_iter().map(Into::into).collect(),
            owner_column: owner_column.into(),
        }
    }

    /// Builds the spec from a syncable model `M`; `owner_column` scopes its rows to a principal.
    ///
    /// Bounded on [`SingleKeySyncable`] because every capture path downstream carries a one-column key.
    #[must_use]
    pub fn for_model<M: SingleKeySyncable>(owner_column: impl Into<String>) -> Self {
        Self::new(
            M::TABLE,
            [M::KEY_COLUMN],
            M::PARTITION_COLUMNS.iter().copied(),
            M::data_columns().iter().copied(),
            owner_column,
        )
    }

    /// The base table name.
    #[must_use]
    pub fn table(&self) -> &str {
        &self.table
    }

    /// The key columns, in key order, carried as the captured change's record id.
    #[must_use]
    pub fn key_columns(&self) -> &[String] {
        &self.key_columns
    }

    /// The leading key columns forming the storage partition, which a change outbox partitions by.
    #[must_use]
    pub fn partition_columns(&self) -> &[String] {
        &self.partition_columns
    }

    /// The synced data columns kept in the captured change's payload.
    #[must_use]
    pub fn data_columns(&self) -> &[String] {
        &self.data_columns
    }

    /// The column whose value scopes a change to its owning principal.
    #[must_use]
    pub fn owner_column(&self) -> &str {
        &self.owner_column
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_keeps_the_named_columns() {
        let spec = CaptureTable::new("animals", ["id"], ["id"], ["name", "owner_id"], "owner_id");
        assert_eq!(spec.table(), "animals");
        assert_eq!(spec.key_columns(), ["id".to_string()]);
        assert_eq!(
            spec.data_columns(),
            ["name".to_string(), "owner_id".to_string()]
        );
        assert_eq!(spec.owner_column(), "owner_id");
    }
}
