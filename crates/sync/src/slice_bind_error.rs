//! Error type for building a device-side slice binding from typed parameters.

use fundamental_core::validate::ValidationErrors;
use fundamental_macros::Error;

/// Errors produced while binding a device to a slice from typed parameters, before any network call.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SliceBindError {
    /// The supplied parameters failed their own [`Validate`](fundamental_core::validate::Validate)
    /// rules.
    #[error(
        code = "SLICE_B01",
        message = "Slice parameters failed validation: {0}",
        status = 422
    )]
    Invalid(#[from] ValidationErrors),

    /// The parameters could not be serialized to the JSON payload the binding carries.
    #[error(
        code = "SLICE_B02",
        message = "Could not serialize slice parameters: {0}",
        status = 500
    )]
    Serialize(String),
}
