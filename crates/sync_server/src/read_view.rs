use std::sync::Arc;

use fundamental_db::{Filter, FilterValue};
use fundamental_sync::{ServerSyncable, SliceReader, SyncError, SyncRecord};

/// A read view over the server's backend store, shared by the slice and projection
/// resolution contexts, dropping tombstones.
#[derive(Clone)]
pub(crate) struct ReadView {
    reader: Arc<dyn SliceReader>,
}

impl ReadView {
    /// Wraps the backend read view.
    pub(crate) fn new(reader: Arc<dyn SliceReader>) -> Self {
        Self { reader }
    }

    /// Reads the live rows of syncable model `M` matching `filters`.
    ///
    /// # Errors
    /// Returns a [`SyncError`] if the backend read fails.
    pub(crate) async fn read<M: ServerSyncable>(
        &self,
        filters: &[Filter],
    ) -> Result<Vec<SyncRecord>, SyncError> {
        let rows = self
            .reader
            .read_records(M::TABLE, M::data_columns(), M::KEY_COLUMNS, filters)
            .await?;
        Ok(rows
            .into_iter()
            .filter(|record| !record.meta.tombstone)
            .collect())
    }

    /// Reads one `column` of model `M` for the live rows matching `filters`, as
    /// [`FilterValue`]s for `Filter::in_list`.
    ///
    /// # Errors
    /// Returns a [`SyncError`] if the backend read fails.
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    pub(crate) async fn read_column<M: ServerSyncable>(
        &self,
        column: &str,
        filters: &[Filter],
    ) -> Result<Vec<FilterValue>, SyncError> {
        // A key column's value sits at its position in the record's key, not in the payload.
        let key_position = M::KEY_COLUMNS.iter().position(|c| *c == column);

        let rows = self.read::<M>(filters).await?;
        let mut values = Vec::with_capacity(rows.len());
        for record in rows {
            match key_position {
                Some(at) => {
                    // Fallible, not skipped: a dropped value would quietly narrow a permission.
                    let value = record.id.components().nth(at).ok_or_else(|| {
                        SyncError::Pull(format!(
                            "`{}` names {column} at key position {at} but a row's key has only \
                             {} component(s)",
                            M::TABLE,
                            record.id.len()
                        ))
                    })?;
                    values.push(FilterValue::String(value.to_string()));
                }
                None => {
                    if let Some(value) = record.payload.get(column) {
                        values.push(FilterValue::String(value.clone()));
                    }
                }
            }
        }
        Ok(values)
    }
}
