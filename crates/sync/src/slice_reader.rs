//! The [`SliceReader`]: an object-safe read seam a slice resolver uses to read
//! *other* models when computing a relational scope.

use std::future::Future;
use std::pin::Pin;

use fundamental_db::Filter;

use crate::{SliceStore, SyncError, SyncRecord};

type ReadFuture<'a> = Pin<Box<dyn Future<Output = Result<Vec<SyncRecord>, SyncError>> + Send + 'a>>;

/// An object-safe read view over the server's store, letting a slice resolver read a second model
/// to scope the first.
pub trait SliceReader: Send + Sync {
    /// Reads every row of `table` matching `filters`, tombstones included.
    ///
    /// # Errors
    /// Returns a [`SyncError`] if the store read fails.
    fn read_records<'a>(
        &'a self,
        table: &'a str,
        data_columns: &'a [&'a str],
        key_columns: &'a [&'a str],
        filters: &'a [Filter],
    ) -> ReadFuture<'a>;
}

impl<S: SliceStore + Send + Sync> SliceReader for S {
    fn read_records<'a>(
        &'a self,
        table: &'a str,
        data_columns: &'a [&'a str],
        key_columns: &'a [&'a str],
        filters: &'a [Filter],
    ) -> ReadFuture<'a> {
        Box::pin(self.changed_since(table, data_columns, key_columns, filters, 0))
    }
}
