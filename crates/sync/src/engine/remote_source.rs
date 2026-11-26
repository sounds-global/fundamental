//! The server side of a sync: the authoritative remote store, addressed by slice.

use std::future::Future;

use serde_json::Value;

use crate::SyncError;
use crate::engine::SyncRecord;

/// The authoritative remote the engine pulls a slice's rows from and pushes to.
pub trait RemoteSource {
    /// Returns every row of `slice` changed since `since`, tombstones included, across all pages.
    ///
    /// # Errors
    /// Returns a [`SyncError`] if the remote cannot be read.
    fn changed_since(
        &self,
        slice: &str,
        params: &Value,
        since: i64,
    ) -> impl Future<Output = Result<Vec<SyncRecord>, SyncError>>;

    /// Pushes `record` back through `slice`.
    ///
    /// # Errors
    /// Returns a [`SyncError`] if the write is refused or fails.
    fn upsert(
        &self,
        slice: &str,
        params: &Value,
        record: &SyncRecord,
    ) -> impl Future<Output = Result<(), SyncError>>;
}
