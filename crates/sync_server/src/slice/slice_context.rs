use std::fmt;
use std::sync::Arc;

use fundamental_auth::{AuthenticatedPrincipal, UserId};
use fundamental_core::AnyError;
use fundamental_db::{Filter, FilterValue};
use fundamental_sync::{ServerSyncable, SliceReader, SyncRecord};

use crate::read_view::ReadView;
use crate::slice::SliceError;

/// The context a [slice](crate::SliceRegistry) definition receives while resolving: the
/// server-injected principal, an authorization hook, and a read view for relational scoping.
#[derive(Clone)]
pub struct SliceContext {
    principal: AuthenticatedPrincipal,
    read: ReadView,
}

impl SliceContext {
    /// Wraps an authenticated principal and the backend read view.
    #[must_use]
    pub fn new(principal: AuthenticatedPrincipal, reader: Arc<dyn SliceReader>) -> Self {
        Self {
            principal,
            read: ReadView::new(reader),
        }
    }

    /// The authenticated principal's user id.
    #[must_use]
    pub fn user_id(&self) -> &UserId {
        &self.principal.user_id
    }

    /// The full authenticated principal.
    #[must_use]
    pub fn principal(&self) -> &AuthenticatedPrincipal {
        &self.principal
    }

    /// Whether the principal carries `scope`.
    #[must_use]
    pub fn has_scope(&self, scope: &str) -> bool {
        self.principal.has_scope(scope)
    }

    /// Returns [`SliceError::Forbidden`] unless `allowed` holds.
    ///
    /// # Errors
    /// Returns [`SliceError::Forbidden`] when `allowed` is `false`.
    pub fn require(&self, allowed: bool) -> Result<(), SliceError> {
        if allowed {
            Ok(())
        } else {
            Err(SliceError::Forbidden)
        }
    }

    /// Reads the live rows of another syncable model `M` matching `filters`.
    ///
    /// # Errors
    /// Returns [`SliceError::Other`] if the backend read fails.
    pub async fn read<M: ServerSyncable>(
        &self,
        filters: &[Filter],
    ) -> Result<Vec<SyncRecord>, SliceError> {
        self.read
            .read::<M>(filters)
            .await
            .map_err(|e| SliceError::Other(AnyError::new(e)))
    }

    /// Reads one `column` of another syncable model `M` for the live rows matching `filters`,
    /// as [`FilterValue`]s for `Filter::in_list`.
    ///
    /// # Errors
    /// Returns [`SliceError::Other`] if the backend read fails.
    pub async fn read_column<M: ServerSyncable>(
        &self,
        column: &str,
        filters: &[Filter],
    ) -> Result<Vec<FilterValue>, SliceError> {
        self.read
            .read_column::<M>(column, filters)
            .await
            .map_err(|e| SliceError::Other(AnyError::new(e)))
    }
}

impl fmt::Debug for SliceContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SliceContext")
            .field("principal", &self.principal)
            .finish_non_exhaustive()
    }
}
