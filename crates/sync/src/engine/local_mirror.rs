//! The client side of a sync: the local offline mirror.

use std::future::Future;

use fundamental_db::RecordKey;

use crate::SyncError;
use crate::engine::SyncRecord;

/// The local offline mirror the engine reconciles against the remote source.
pub trait LocalMirror {
    /// Returns the mirror row addressed by `key`, if present.
    ///
    /// # Errors
    /// Returns a [`SyncError`] if the mirror cannot be read.
    fn get(
        &self,
        table: &str,
        key: &RecordKey,
    ) -> impl Future<Output = Result<Option<SyncRecord>, SyncError>>;

    /// Inserts or updates `record` in the mirror and marks it clean.
    ///
    /// # Errors
    /// Returns a [`SyncError`] if the write fails.
    fn apply_remote(
        &self,
        table: &str,
        record: &SyncRecord,
    ) -> impl Future<Output = Result<(), SyncError>>;

    /// Returns the rows changed locally since the last successful push.
    ///
    /// # Errors
    /// Returns a [`SyncError`] if the mirror cannot be read.
    fn dirty(&self, table: &str) -> impl Future<Output = Result<Vec<SyncRecord>, SyncError>>;

    /// Marks the row addressed by `key` clean after it has been pushed to the remote.
    ///
    /// # Errors
    /// Returns a [`SyncError`] if the update fails.
    fn mark_clean(
        &self,
        table: &str,
        key: &RecordKey,
    ) -> impl Future<Output = Result<(), SyncError>>;

    /// Returns the high-water mark of the last pull for `table`, or `0` if never pulled.
    ///
    /// # Errors
    /// Returns a [`SyncError`] if the bookkeeping cannot be read.
    fn last_pulled(&self, table: &str) -> impl Future<Output = Result<i64, SyncError>>;

    /// Records the high-water mark after a successful pull.
    ///
    /// # Errors
    /// Returns a [`SyncError`] if the bookkeeping cannot be written.
    fn set_last_pulled(
        &self,
        table: &str,
        updated_at: i64,
    ) -> impl Future<Output = Result<(), SyncError>>;
}
