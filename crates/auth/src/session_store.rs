//! [`SessionStore`] — where an application keeps its live sessions.

use std::future::Future;

use crate::{AuthError, SessionDigest, SessionRecord};

/// The application's store of live sessions, read by digest.
pub trait SessionStore {
    /// The live session whose handle digests to `digest`, or `None` when there is none.
    ///
    /// # Errors
    /// [`AuthError::SessionStoreUnavailable`] when the store cannot be read; never `Ok(None)`.
    fn find(
        &self,
        digest: &SessionDigest,
    ) -> impl Future<Output = Result<Option<SessionRecord>, AuthError>> + Send;
}
