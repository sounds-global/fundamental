//! [`AppState`] — the state the pages are served over.

use std::sync::Arc;

use fundamental::authz::authorizer::{DynAuthorizer, HasAuthorizer};

/// The application state: here, only the authorizer the page guards ask.
#[derive(Clone)]
pub struct AppState {
    authz: Arc<dyn DynAuthorizer>,
}

impl AppState {
    /// A state asking `authz`.
    #[must_use]
    pub fn new(authz: Arc<dyn DynAuthorizer>) -> Self {
        Self { authz }
    }
}

impl HasAuthorizer for AppState {
    fn authorizer(&self) -> &dyn DynAuthorizer {
        self.authz.as_ref()
    }
}
