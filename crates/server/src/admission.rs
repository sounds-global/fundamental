use std::fmt::Debug;

use fundamental_core::AnyError;

use crate::AdmissionRequest;

/// Decides whether the server will do any work for a request at all.
///
/// A backend runs it first, before reading the body or verifying a credential, so a refused
/// request costs nothing more. Synchronous, so it stays cheap enough for every request.
pub trait Admission: Debug + Send + Sync + 'static {
    /// Admits the request, or refuses it with the error its response is rendered from.
    ///
    /// # Errors
    /// The refusal, whatever the implementation decides it is.
    fn admit(&self, request: &AdmissionRequest<'_>) -> Result<(), AnyError>;
}
