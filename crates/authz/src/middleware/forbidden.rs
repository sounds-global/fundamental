//! [`Forbidden`]: the error a permission guard returns when access is denied.

use fundamental_core::error::AuthorizationError;
use fundamental_macros::Error;

/// Why a guarded request did not proceed.
///
/// [`Denied`](Forbidden::Denied) is a `403` ([`AuthorizationError`]);
/// [`MissingObject`](Forbidden::MissingObject) is a misconfiguration, a `500`.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Forbidden {
    /// The subject may not perform the relation on the object.
    #[error(code = "AUTHZ_014", message = "Permission denied", status = 403)]
    Denied,

    /// The guarded route exposes no object id to authorize against.
    #[error(
        code = "AUTHZ_015",
        message = "Guarded route resolved no object id to authorize against",
        status = 500
    )]
    MissingObject,

    /// The object id in the request is not a well-formed id of its namespace.
    ///
    /// A `403`, so an unauthorized caller cannot tell it from a denial; the code tells the operator.
    #[error(code = "AUTHZ_016", message = "Permission denied", status = 403)]
    MalformedObject,
}

impl AuthorizationError for Forbidden {}
