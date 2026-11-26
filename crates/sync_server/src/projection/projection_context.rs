use std::sync::Arc;

use fundamental_db::{Filter, FilterValue};
use fundamental_sync::{ServerSyncable, SliceReader, SyncRecord};

use crate::projection::ProjectionError;
use crate::read_view::ReadView;

/// The context a projection recompute handler receives: a read view over the backend, with no
/// principal.
#[derive(Clone)]
pub struct ProjectionContext {
    read: ReadView,
}

impl ProjectionContext {
    /// Wraps the backend read view the handler reads its source models through.
    pub(crate) fn new(reader: Arc<dyn SliceReader>) -> Self {
        Self {
            read: ReadView::new(reader),
        }
    }

    /// Reads the live rows of syncable source model `M` matching `filters`.
    ///
    /// # Errors
    /// Returns [`ProjectionError::Read`] if the backend read fails.
    pub async fn read<M: ServerSyncable>(
        &self,
        filters: &[Filter],
    ) -> Result<Vec<SyncRecord>, ProjectionError> {
        self.read
            .read::<M>(filters)
            .await
            .map_err(|e| ProjectionError::Read(e.to_string()))
    }

    /// Reads one `column` of source model `M` for the live rows matching `filters`.
    ///
    /// # Errors
    /// Returns [`ProjectionError::Read`] if the backend read fails.
    pub async fn read_column<M: ServerSyncable>(
        &self,
        column: &str,
        filters: &[Filter],
    ) -> Result<Vec<FilterValue>, ProjectionError> {
        self.read
            .read_column::<M>(column, filters)
            .await
            .map_err(|e| ProjectionError::Read(e.to_string()))
    }
}
