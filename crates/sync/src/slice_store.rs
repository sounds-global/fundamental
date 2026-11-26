//! The [`SliceStore`] trait: a backend's read side for slice-based sync.

use std::future::Future;

use fundamental_db::Filter;

use crate::wire::SliceCursor;
use crate::{SyncError, SyncRecord};

/// A server-side store that reads a slice's rows out of a base table.
pub trait SliceStore {
    /// Returns up to `limit` rows of `table` matching `filters`, in `(updated_at, id)` order, after
    /// the keyset point `after` or, without one, changed since `since`; tombstones included.
    ///
    /// # Errors
    /// Returns a [`SyncError`] if the store cannot be read.
    #[allow(clippy::too_many_arguments)]
    #[expect(
        clippy::allow_attributes,
        clippy::allow_attributes_without_reason,
        reason = "readability baseline"
    )]
    fn changed_page(
        &self,
        table: &str,
        data_columns: &[&str],
        key_columns: &[&str],
        filters: &[Filter],
        since: i64,
        after: Option<&SliceCursor>,
        limit: u32,
    ) -> impl Future<Output = Result<Vec<SyncRecord>, SyncError>> + Send;

    /// Returns all rows changed since `since`, unpaged.
    ///
    /// # Errors
    /// Returns a [`SyncError`] if the store cannot be read.
    #[expect(clippy::too_many_arguments, reason = "readability baseline")]
    fn changed_since(
        &self,
        table: &str,
        data_columns: &[&str],
        key_columns: &[&str],
        filters: &[Filter],
        since: i64,
    ) -> impl Future<Output = Result<Vec<SyncRecord>, SyncError>> + Send {
        self.changed_page(
            table,
            data_columns,
            key_columns,
            filters,
            since,
            None,
            u32::MAX,
        )
    }

    /// Whether `table`, a served model's hub table, is provisioned in this store; `true` by default.
    /// # Errors
    /// Returns a [`SyncError`] if the store cannot be introspected.
    fn table_exists(&self, table: &str) -> impl Future<Output = Result<bool, SyncError>> + Send {
        async move {
            let _ = table;
            Ok(true)
        }
    }
}

/// A [`SliceStore`] that also accepts writes; only stores backing a bidirectional table.
pub trait SliceWriteStore: SliceStore {
    /// Inserts or updates `record` by its key, server-stamping the overlay; a tombstone deletes.
    ///
    /// # Errors
    /// Returns a [`SyncError`] if the write fails.
    fn upsert(
        &self,
        table: &str,
        data_columns: &[&str],
        key_columns: &[&str],
        record: &SyncRecord,
    ) -> impl Future<Output = Result<(), SyncError>> + Send;
}
